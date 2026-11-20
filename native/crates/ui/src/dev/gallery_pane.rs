//! The gallery's 3D pane (`mcbench-ui-dev --gallery`, [`super::gallery`]):
//! the pane in each of its states, each over its own stand-in and fake pane
//! ([`super::pane::FakePane::manual`], pumped on GPUI's timer: the
//! in-process tests' scheduler refuses wakes from another thread), and the
//! selection chips' three squares (a block's icon, the core's `Err`, empty
//! space).

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::*;

use super::fixtures;
use super::pane::{Behaviour, FakePane};
use super::standin::{LiveFailures, StandIn};
use crate::view::shell::announce::Announcer;
use crate::view::world::Pane;
use crate::{Command, Core, LiveCommand, Update, ViewKey};

/// The chat each pane shows (the gate fixture's).
const CHAT: &str = "chat-gate";

/// How often the fake panes draw what changed.
const PUMP: Duration = Duration::from_millis(33);

/// A window for one stand-in that hands its updates to GPUI's thread.
struct Channel(futures::channel::mpsc::UnboundedSender<Update>);

impl crate::Window for Channel {
    fn publish(&self, update: Update) {
        self.0
            .unbounded_send(update)
            .expect("the gallery's channel is open while it runs");
    }
}

/// The pane's states shown, by name: what fails, and its size. Every pane
/// stays within the section's first 600 px: the pane's bar, zoom buttons,
/// toolbar and banner take the mouse wheel from the gallery (as BlueMap's
/// controls do), and `check_gallery.py` wheels the gallery at x 640.
fn states() -> Vec<(&'static str, LiveFailures, Size<Pixels>)> {
    let small = size(px(295.), px(180.));
    vec![
        (
            "A running world",
            LiveFailures::default(),
            size(px(600.), px(320.)),
        ),
        (
            "No renderer joined",
            LiveFailures {
                not_joined: true,
                ..LiveFailures::default()
            },
            small,
        ),
        (
            "No adapter",
            LiveFailures {
                no_adapter: true,
                ..LiveFailures::default()
            },
            small,
        ),
        (
            "Placement failed",
            LiveFailures {
                place: true,
                ..LiveFailures::default()
            },
            small,
        ),
        (
            "Stopped",
            LiveFailures {
                stop_after: Some(1),
                ..LiveFailures::default()
            },
            small,
        ),
    ]
}

/// One pane of the section.
struct Shown {
    name: &'static str,
    size: Size<Pixels>,
    pane: Entity<Pane>,
}

/// The section.
pub struct PaneGallery {
    shown: Vec<Shown>,
    _tasks: Vec<Task<()>>,
}

impl PaneGallery {
    /// The panes, each attached to its stand-in and showing the gate chat; the
    /// chips' icons are asked of the first stand-in.
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let handle = window.window_handle();
        let mut shown = Vec::new();
        let mut tasks = Vec::new();
        let mut first_core: Option<Arc<dyn Core>> = None;
        for (name, failures, size) in states() {
            let texts = fixtures::Texts::new(std::path::Path::new("lamp.png"));
            let standin = Arc::new(StandIn::new(
                fixtures::gate(&texts),
                texts.streamed.clone(),
                Duration::ZERO,
                None,
            ));
            let fake = FakePane::manual(Behaviour::default());
            let stops = failures.stop_after.is_some();
            standin.set_pane(fake.clone(), failures);
            let core: Arc<dyn Core> = standin.clone();
            first_core.get_or_insert_with(|| core.clone());
            let announcer = cx.new(|_| Announcer::default());
            let pane = cx.new(|cx| Pane::new(core.clone(), None, announcer, false, window, cx));
            let (sender, mut receiver) = futures::channel::mpsc::unbounded::<Update>();
            core.attach(Arc::new(Channel(sender)));
            pane.update(cx, |p, cx| {
                p.show(
                    Some(ViewKey(CHAT.into())),
                    name.into(),
                    None,
                    None,
                    false,
                    false,
                    window,
                    cx,
                )
            });
            if stops {
                // One tick, and the engine stops the world.
                core.send(Command::Live(LiveCommand::Step));
            }
            let target = pane.downgrade();
            tasks.push(cx.spawn(async move |_, cx| {
                loop {
                    fake.pump();
                    while let Ok(update) = receiver.try_recv() {
                        let is_live = matches!(
                            update,
                            Update::LivePane(_)
                                | Update::Live(_)
                                | Update::Clicked { .. }
                                | Update::PlayerNote(_)
                                | Update::SelectionDescribed { .. }
                                | Update::Exported(_)
                        );
                        if !is_live {
                            continue;
                        }
                        let applied = handle.update(cx, |_, window, cx| {
                            target.update(cx, |p, cx| {
                                p.apply(update, window, cx);
                            })
                        });
                        if !matches!(applied, Ok(Ok(()))) {
                            return;
                        }
                    }
                    cx.background_executor().timer(PUMP).await;
                }
            }));
            shown.push(Shown { name, size, pane });
        }
        crate::view::block_icons::install(first_core.expect("one pane at least"), cx);
        PaneGallery {
            shown,
            _tasks: tasks,
        }
    }
}

impl Render for PaneGallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let fonts = crate::view::controls::fonts(cx);
        let selection = |letter: &str, label: &str, block: Option<&str>| crate::Selection {
            letter: letter.into(),
            label: label.into(),
            min: [4, 0, 4],
            max: [10, 4, 12],
            blocks: 315,
            block: block.map(str::to_string),
        };
        let chips = div()
            .flex()
            .flex_wrap()
            .gap(px(5.))
            .child(crate::view::chat::attachments::selection_chip(
                &selection("A", "Stone Bricks, middle", Some("minecraft:stone_bricks")),
                None,
                cx,
            ))
            .child(crate::view::chat::attachments::selection_chip(
                &selection("B", "Oak Door, lower", Some("minecraft:oak_door")),
                None,
                cx,
            ))
            .child(crate::view::chat::attachments::selection_chip(
                &selection("C", "Empty space", None),
                None,
                cx,
            ));
        div().flex().flex_col().gap(px(10.)).child(chips).child(
            div()
                .flex()
                .flex_wrap()
                .gap(px(10.))
                .children(self.shown.iter().map(|s| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            crate::view::theme::text(
                                div(),
                                crate::view::theme::TextKind::LiveNote,
                                &fonts,
                            )
                            .text_color(crate::view::theme::colour::ash())
                            .child(s.name),
                        )
                        .child(
                            div()
                                .w(s.size.width)
                                .h(s.size.height)
                                .overflow_hidden()
                                .child(s.pane.clone()),
                        )
                })),
        )
    }
}
