//! The gallery's pickers (`mcbench-ui-dev --gallery`, [`super::gallery`]):
//! the composer footer's pickers over the stand-in's authored catalogues
//! (`view::pickers::footer`, through `view::chat::controls::footer_pickers`,
//! as the footer mounts them), with a usage for the context meter and the
//! limit chip; a read-only resolution picker; the resolution tags; the
//! meter and chip in their other states; and a model picker with no AI
//! connected. Each action outside the pickers is logged (`opened`,
//! `remembered`, `notice`, `bench`); with a log the model picker records its
//! open, key and scroll timings (`picker-timing`). `MCBENCH_GALLERY_MODELS`
//! adds that many models to OpenRouter's catalogue (the 400-model timing).

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

use super::fixtures;
use super::standin::StandIn;
use crate::state::meters::{RateLimit, chip, context};
use crate::view::controls::fonts;
use crate::view::controls::popover::PopoverState;
use crate::view::pickers::catalog::{Catalog, Hooks, Remember};
use crate::view::pickers::footer::FooterPickers;
use crate::view::pickers::meters::{context_meter, limit_chip};
use crate::view::pickers::model::ModelPicker;
use crate::view::pickers::resolution::{resolution_picker, resolution_tag};
use crate::view::pickers::{Bounds, bounds};
use crate::view::theme::{self, TextKind};
use crate::{ActionLog, Core, Preferences, Provider, Resolution, Update};

/// The usage the gallery's footer shows: Claude Code's Opus at 84k of its
/// window, near its weekly limit.
pub fn usage_message() -> mcbench_store::StoredMessage {
    let payload = serde_json::json!({
        "role": "assistant",
        "content": "Done.",
        "telemetry": {
            "provider": "claude-code", "model": super::standin::MODEL, "effort": null,
            "startedAt": "2026-11-17T10:00:00.000Z", "wallMs": 1200, "firstDeltaMs": 300,
            "contextBytes": 1000, "messageCount": 2, "usage": null, "contextTokens": 840_000
        },
        "rateLimit": { "status": "allowed_warning", "rateLimitType": "seven_day", "resetsAt": 1_794_930_000 },
        "createdAt": "2026-11-17T10:00:01.000Z"
    });
    mcbench_store::StoredMessage::from_payload("usage-1", None, 0, payload.to_string())
        .expect("the gallery's usage is a stored message")
}

/// A window for the stand-in that hands its updates to GPUI's thread.
struct Channel(futures::channel::mpsc::UnboundedSender<Update>);

impl crate::Window for Channel {
    fn publish(&self, update: Update) {
        self.0
            .unbounded_send(update)
            .expect("the gallery's channel is open while it runs");
    }
}

/// The gallery's pickers.
pub struct PickerGallery {
    catalog: Entity<Catalog>,
    footer: Entity<FooterPickers>,
    read_only: (Entity<PopoverState>, Bounds, Bounds),
    no_ai: Entity<ModelPicker>,
    _keys: Subscription,
}

fn logged(log: &Option<ActionLog>, action: &'static str, fields: serde_json::Value) {
    if let Some(log) = log {
        log.record(action, fields);
    }
}

impl PickerGallery {
    /// The section, its stand-in attached.
    pub fn new(log: Option<ActionLog>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let texts = fixtures::Texts::new(std::path::Path::new("lamp.png"));
        let standin = StandIn::new(
            fixtures::gate(&texts),
            texts.streamed.clone(),
            std::time::Duration::ZERO,
            None,
        );
        if let Ok(count) = std::env::var("MCBENCH_GALLERY_MODELS") {
            let count: usize = count
                .parse()
                .unwrap_or_else(|_| panic!("MCBENCH_GALLERY_MODELS takes a count, not {count}"));
            standin.add_synthetic_models("openrouter", count);
        }
        let executor = cx.background_executor().clone();
        standin.set_later(Box::new(move |delay, answer| {
            let timer = executor.timer(delay);
            executor
                .spawn(async move {
                    timer.await;
                    answer()
                })
                .detach();
        }));
        let core: Arc<dyn Core> = Arc::new(standin);
        let hooks = {
            let (a, b, c, d) = (log.clone(), log.clone(), log.clone(), log.clone());
            Hooks {
                open_settings: Rc::new(move |_, _| {
                    logged(&a, "opened", serde_json::json!({ "layer": "settings" }))
                }),
                compare_in_bench: Rc::new(move |_, _| logged(&b, "bench", serde_json::json!({}))),
                remember: Rc::new(move |choice, _| {
                    let fields = match choice {
                        Remember::Model(p, m) => serde_json::json!({ "model": [p, m] }),
                        Remember::Resolution(r) => serde_json::json!({ "resolution": r.value() }),
                    };
                    logged(&c, "remembered", fields)
                }),
                notice: Rc::new(move |text, _| {
                    logged(&d, "notice", serde_json::json!({ "text": text }))
                }),
                chosen_after_ask: Rc::new(|_, _| {}),
            }
        };
        let catalog = cx.new(|_| Catalog::new(core.clone(), hooks.clone()));
        let (sender, mut receiver) = futures::channel::mpsc::unbounded::<Update>();
        let drain = catalog.clone();
        let notices = log.clone();
        cx.spawn(async move |_, cx| {
            use futures::StreamExt as _;
            while let Some(update) = receiver.next().await {
                cx.update(|cx| match update {
                    Update::Providers(_)
                    | Update::Models { .. }
                    | Update::ModelInfo { .. }
                    | Update::Favorites(_) => drain.update(cx, |c, cx| c.apply(update, cx)),
                    Update::Error(text) => {
                        logged(&notices, "notice", serde_json::json!({ "text": text }))
                    }
                    _ => {}
                });
            }
        })
        .detach();
        core.attach(Arc::new(Channel(sender)));
        let preferences = Preferences {
            last_model: Some(("claude-code".into(), super::standin::MODEL.into())),
            ..Preferences::default()
        };
        let footer = cx.new(|cx| FooterPickers::new(catalog.clone(), preferences, window, cx));
        footer.update(cx, |f, cx| f.transcript(&[usage_message()], cx));
        let picker = footer.read(cx).model_picker();
        picker.update(cx, |p, _| p.set_log(log.clone()));
        // A key's arrival at the window, for the search's key-to-paint.
        let timed = picker.downgrade();
        let keys = cx.intercept_keystrokes(move |_, _, cx| {
            let _ = timed.update(cx, |p, cx| p.key_arrived(cx));
        });
        let no_ai_catalog = cx.new(|cx| {
            let mut c = Catalog::new(core.clone(), hooks);
            let none: Vec<Provider> = Vec::new();
            c.apply(Update::Providers(Ok(none)), cx);
            c.apply(Update::Favorites(Vec::new()), cx);
            c
        });
        let no_ai =
            cx.new(|cx| ModelPicker::new(no_ai_catalog, Rc::new(|_, _, _, _| {}), window, cx));
        PickerGallery {
            catalog,
            footer,
            read_only: (
                cx.new(|cx| PopoverState::new(crate::state::popover::RESOLUTION_EXIT, cx)),
                bounds(),
                bounds(),
            ),
            no_ai,
            _keys: keys,
        }
    }

    /// The footer's pickers.
    pub fn footer(&self) -> Entity<FooterPickers> {
        self.footer.clone()
    }

    /// The catalogue.
    pub fn catalog(&self) -> Entity<Catalog> {
        self.catalog.clone()
    }

    /// The picker with no AI connected.
    pub fn no_ai(&self) -> Entity<ModelPicker> {
        self.no_ai.clone()
    }
}

impl Render for PickerGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fonts = fonts(cx);
        let label =
            |text: &str| theme::text(div(), TextKind::SideTitle, &fonts).child(text.to_string());
        let (state, _, track) = &self.read_only;
        let read_only = resolution_picker(
            state,
            track,
            Resolution::High,
            true,
            Rc::new(|_, _, _| {}),
            cx,
        );
        let tags = Resolution::ALL
            .iter()
            .filter_map(|r| resolution_tag(*r, cx))
            .fold(div().flex().gap(px(10.)), |d, t| d.child(t));
        let clock = crate::state::meters::clock;
        let limit = |status: &str, kind: &str| RateLimit {
            status: status.into(),
            kind: Some(kind.into()),
            resets_at: Some(1_794_930_000.),
        };
        let high = context(190_000., 200_000.);
        let near = chip(Some(&limit("allowed_warning", "five_hour")), clock).expect("near a limit");
        let reached = chip(Some(&limit("rejected", "seven_day_opus")), clock).expect("at a limit");
        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(label("Composer footer"))
                    .child(crate::view::chat::controls::footer_pickers(&self.footer)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(label("Read only, tags, meters"))
                    .child(read_only)
                    .child(tags)
                    .child(context_meter("context-meter-high", &high, window, cx))
                    .child(limit_chip("limit-chip-near", &near, window, cx))
                    .child(limit_chip("limit-chip-reached", &reached, window, cx)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(label("No AI connected"))
                    .child(self.no_ai.clone()),
            )
    }
}
