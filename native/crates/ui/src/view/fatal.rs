//! The screen for a failure the app cannot go on from: the core's
//! `Update::Fatal`, or [`crate::fatal`] when the app cannot start (its data
//! folder cannot be made). As today's page does for an error that stops it
//! (`ErrorBoundary`: a heading, the message, a button), it shows a heading,
//! the message as selectable text, and Restart, which has focus (par-shell:
//! the page's `Reload`; it starts the app again and ends this process).
//! Closing the window quits. The process ends with status 2. A panic
//! reaches this screen too, in a new process ([`crate::crash`]).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicI32, Ordering};

use gpui_kit::*;

use super::text_leaf::{HitMap, LeafA11y, TextLeaf};
use super::{Activate, CopySelection, SelectAll, colour, ring};
use crate::state::selection::{Point as TextPoint, Selection};

/// The process's exit status once the window closes: 2 after a fatal error.
pub static EXIT_STATUS: AtomicI32 = AtomicI32::new(0);

/// The heading of the screen.
pub const HEADING: &str = "mcbench-lite stopped on an error";

/// The button (par-shell: the page's `Reload`).
pub const RESTART: &str = "Restart";

/// `Restart`: the app starts again and this process ends; one that cannot
/// start leaves the screen as it is (the reason on stderr).
fn restart(cx: &mut App) {
    match crate::crash::restart() {
        Ok(()) => cx.quit(),
        Err(error) => eprintln!("mcbench-lite could not start again: {error}"),
    }
}

/// The alert's name: the heading and the message.
pub fn alert_name(message: &str) -> String {
    format!("{HEADING}: {message}")
}

/// The screen.
pub struct FatalScreen {
    message: SharedString,
    quit: FocusHandle,
    selection: Option<Selection>,
    dragging: bool,
    /// Frames drawn: Restart takes focus in the second, after the window has
    /// drawn once, as the chat screen's composer does. Focus set before the
    /// first frame reached the tree, but UI Automation did not report it
    /// when the window came to the front (gate-win).
    frames: usize,
    hits: Rc<RefCell<HitMap>>,
    fonts: super::markdown::Fonts,
}

impl FatalScreen {
    /// The screen for `message`; Restart takes focus once it has drawn.
    pub fn new(message: String, _: &mut Window, cx: &mut Context<Self>) -> Self {
        EXIT_STATUS.store(2, Ordering::SeqCst);
        let quit = cx.focus_handle().tab_stop(true);
        let theme = cx.global::<gpui_kit::component::Theme>();
        FatalScreen {
            message: message.into(),
            quit,
            selection: None,
            dragging: false,
            frames: 0,
            hits: Rc::new(RefCell::new(HitMap::default())),
            fonts: super::markdown::Fonts {
                ui: theme.font_family.clone(),
                mono: theme.mono_font_family.clone(),
            },
        }
    }

    fn point(&self, position: Point<Pixels>) -> Option<TextPoint> {
        self.hits.borrow().point_at(position)
    }
}

impl Render for FatalScreen {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.frames += 1;
        // par-shell: the dev binary's `--crash screen` (the error screen's
        // own process panicking ends it and starts nothing).
        if self.frames > 2 && crate::dev::PANIC_WHILE_DRAWING.load(Ordering::SeqCst) {
            panic!("a panic on purpose in the error screen (--crash screen)");
        }
        match self.frames {
            1 => cx.notify(),
            2 => window.focus(&self.quit, cx),
            _ => {}
        }
        self.hits.borrow_mut().clear();
        let selected = self
            .selection
            .and_then(|s| s.range_in(0, 0, self.message.len()));
        let message = TextLeaf::new(
            "message",
            Role::Paragraph,
            LeafA11y::default(),
            self.message.clone(),
            vec![TextRun {
                len: self.message.len(),
                font: font(self.fonts.mono.clone()),
                color: colour::err(),
                background_color: None,
                underline: None,
                strikethrough: None,
            }],
            Vec::new(),
            Rc::new(|d| d),
            selected,
            colour::selection(),
            self.hits.clone(),
            (0, 0),
        );
        div()
            .id("fatal-screen")
            .key_context("Fatal")
            .on_action(cx.listener(|this, _: &SelectAll, _, cx| {
                this.selection = Some(Selection {
                    anchor: TextPoint {
                        row: 0,
                        leaf: 0,
                        offset: 0,
                    },
                    focus: TextPoint {
                        row: 0,
                        leaf: 0,
                        offset: this.message.len(),
                    },
                });
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CopySelection, _, cx| {
                if let Some(range) = this
                    .selection
                    .and_then(|s| s.range_in(0, 0, this.message.len()))
                {
                    cx.write_to_clipboard(ClipboardItem::new_string(
                        this.message[range].to_string(),
                    ));
                }
            }))
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(colour::bedrock())
            .text_color(colour::bone())
            .font_family(self.fonts.ui.clone())
            .child(
                div()
                    .id("fatal")
                    .role(Role::Alert)
                    // The message is in the alert's name, as every text a
                    // reader must hear is: NVDA spoke only the heading while
                    // the message sat inside the alert (open item a).
                    .aria_label(alert_name(&self.message))
                    .max_w(px(640.))
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .p(px(24.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(FontWeight(600.))
                            .child(HEADING),
                    )
                    .child(
                        div()
                            .id("message-box")
                            .text_size(px(12.5))
                            .line_height(px(19.))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                    if let Some(point) = this.point(event.position) {
                                        this.selection = Some(Selection::caret(point));
                                        this.dragging = true;
                                        cx.notify();
                                    }
                                }),
                            )
                            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                                if this.dragging
                                    && event.pressed_button == Some(MouseButton::Left)
                                    && let (Some(point), Some(selection)) =
                                        (this.point(event.position), this.selection.as_mut())
                                {
                                    selection.focus = point;
                                    cx.notify();
                                }
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _: &MouseUpEvent, _, _| this.dragging = false),
                            )
                            .child(message),
                    )
                    .child(
                        ring(
                            div()
                                .id("restart")
                                .role(Role::Button)
                                .aria_label(RESTART)
                                .track_focus(&self.quit)
                                .key_context("Part")
                                .on_action(|_: &Activate, _, cx| restart(cx))
                                .on_click(|_, _, cx| restart(cx)),
                        )
                        .self_start()
                        .px(px(14.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .bg(colour::raised())
                        .cursor_pointer()
                        .child(RESTART),
                    ),
            )
    }
}
