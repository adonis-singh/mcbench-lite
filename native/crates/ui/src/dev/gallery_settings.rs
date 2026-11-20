//! The gallery's Settings section (`mcbench-ui-dev --gallery`,
//! [`super::gallery`]): Settings' controls drawn inline
//! ([`crate::view::settings::SettingsLayer::render_gallery`]) over the
//! stand-in's settings. The window's catalogue is installed here (the
//! layer's title model picker reads it), fed by the stand-in.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

use super::fixtures;
use super::standin::StandIn;
use crate::view::pickers::catalog::{Catalog, Hooks};
use crate::view::settings::SettingsLayer;
use crate::view::shell::announce::Announcer;
use crate::{ActionLog, Command, Core, SettingsView, Update};

/// A window for the stand-in that hands its updates to GPUI's thread.
struct Channel(futures::channel::mpsc::UnboundedSender<Update>);

impl crate::Window for Channel {
    fn publish(&self, update: Update) {
        self.0
            .unbounded_send(update)
            .expect("the gallery's channel is open while it runs");
    }
}

/// The gallery's Settings section.
pub struct SettingsGallery {
    layer: Entity<SettingsLayer>,
    announcer: Entity<Announcer>,
    pending: Rc<std::cell::RefCell<Option<SettingsView>>>,
    opened: bool,
    _observe: Subscription,
}

impl SettingsGallery {
    /// The section, its stand-in attached and its settings asked for.
    pub fn new(log: Option<ActionLog>, cx: &mut Context<Self>) -> Self {
        let texts = fixtures::Texts::new(std::path::Path::new("lamp.png"));
        let standin = StandIn::new(
            fixtures::gate(&texts),
            texts.streamed.clone(),
            std::time::Duration::ZERO,
            None,
        );
        standin.set_settings_file(std::path::PathBuf::from("data/settings.json"));
        // A slow catalogue answers on GPUI's timer, not a thread of its own
        // (the in-process tests run on the test scheduler).
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
        let hooks = Hooks {
            open_settings: Rc::new(|_, _| {}),
            compare_in_bench: Rc::new(|_, _| {}),
            remember: Rc::new(|_, _| {}),
            notice: Rc::new(|_, _| {}),
            chosen_after_ask: Rc::new(|_, _| {}),
        };
        let catalog = Catalog::install(core.clone(), hooks, cx);
        let announcer = cx.new(|_| Announcer::default());
        let settings_hooks = crate::view::settings::Hooks {
            notice: Rc::new(|_, _| {}),
            closed: Rc::new(|_, _| {}),
            theme: Rc::new(|light, cx| {
                crate::view::set_theme(
                    if light {
                        crate::view::theme::Mode::Light
                    } else {
                        crate::view::theme::Mode::Dark
                    },
                    cx,
                )
            }),
            binds: Rc::new(|keymap, cx| crate::view::keys::set_keymap(cx, keymap)),
            go_to: Rc::new(|_, _, _| {}),
            light: Rc::new(|_| crate::view::theme::mode() == crate::view::theme::Mode::Light),
        };
        let layer = {
            let (core, announcer) = (core.clone(), announcer.clone());
            cx.new(|cx| {
                SettingsLayer::new(core, log, Rc::new(|_, _| {}), announcer, settings_hooks, cx)
            })
        };
        let pending = Rc::new(std::cell::RefCell::new(None));
        let (sender, mut receiver) = futures::channel::mpsc::unbounded::<Update>();
        let into = pending.clone();
        let this = cx.entity().downgrade();
        cx.spawn(async move |_, cx| {
            use futures::StreamExt as _;
            while let Some(update) = receiver.next().await {
                cx.update(|cx| match update {
                    Update::Providers(_)
                    | Update::Models { .. }
                    | Update::ModelInfo { .. }
                    | Update::Favorites(_) => catalog.update(cx, |c, cx| c.apply(update, cx)),
                    Update::Settings(Ok(view)) => {
                        *into.borrow_mut() = Some(view);
                        let _ = this.update(cx, |_, cx| cx.notify());
                    }
                    _ => {}
                });
            }
        })
        .detach();
        core.attach(Arc::new(Channel(sender)));
        core.send(Command::ReadSettings { refresh: false });
        let observe = cx.observe(&layer, |_, _, cx| cx.notify());
        SettingsGallery {
            _observe: observe,
            layer,
            announcer,
            pending,
            opened: false,
        }
    }
}

impl Render for SettingsGallery {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.opened
            && let Some(view) = self.pending.borrow_mut().take()
        {
            self.opened = true;
            self.layer
                .update(cx, |l, cx| l.gallery_open(view, window, cx));
        }
        if !self.opened {
            return div().into_any_element();
        }
        let pieces = self.layer.update(cx, |l, cx| l.render_gallery(window, cx));
        div()
            .child(pieces)
            .child(self.announcer.clone())
            .into_any_element()
    }
}
