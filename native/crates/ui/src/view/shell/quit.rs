//! quit-ask: the question before the person's quit while builds run
//! (`state::quit` holds its texts). The quit key and the app menu's
//! `Quit mcbench-lite` (the `Quit` action), the Window menu's `Close` and
//! Cmd-W (`CloseWindow`), and the window's close request (GPUI's
//! `on_window_should_close`: the title bar's close, WM_DELETE_WINDOW on
//! X11, the toplevel's close on Wayland, WM_CLOSE on Windows) ask through
//! [`Shell::may_quit`]; with no build running, or on the error screen,
//! they quit at once.
//!
//! What never asks: a quit the code asks for (`App::quit`: the error
//! screen's buttons, the dev binary's `--quit-after-ms`, an updater's
//! restart), which runs GPUI's quit with no close request; and the
//! system's logout, restart or shutdown (macOS: AppKit's terminate, which
//! GPUI meets in `applicationWillTerminate:` with no
//! `applicationShouldTerminate:`; Windows: `WM_ENDSESSION` to GPUI's
//! hidden window). Either way the core is stopped in GPUI's quit, by
//! `stop_core_on_quit`.
//!
//! The question is the `quit` layer, modal and over every other: a dialog
//! named by its title and described by its body, with `Keep building`
//! (focused when it opens) and `Quit`; Tab stays between the two. `Keep
//! building`, Escape and a press on the scrim close it and focus goes back
//! where it was; a second quit request while it is up does nothing. It
//! stays up when the builds end meanwhile, counting the builds of when it
//! opened.

use gpui_kit::*;

use super::super::controls::{Button, Kind, fonts};
use super::super::theme::{self, TextKind, colour, shadow};
use super::super::{FocusNext, FocusPrevious, keys};
use super::Shell;
use crate::state::layers::Layer;
use crate::state::quit;

/// The question while it is up.
pub(super) struct Asking {
    /// The builds running when it opened.
    running: usize,
    /// What had focus: it gets focus back when the question closes.
    back: Option<FocusHandle>,
}

/// The question's two focus stops.
pub(super) struct QuitFocus {
    keep: FocusHandle,
    quit: FocusHandle,
}

impl QuitFocus {
    pub(super) fn new(cx: &mut App) -> Self {
        QuitFocus {
            keep: cx.focus_handle().tab_stop(true),
            quit: cx.focus_handle().tab_stop(true),
        }
    }
}

impl Shell {
    /// Has the window's close request ask (GPUI's `on_window_should_close`).
    pub(super) fn ask_on_close(window: &mut Window, cx: &mut Context<Self>) {
        let shell = cx.entity().downgrade();
        window.on_window_should_close(cx, move |window, cx| {
            shell
                .update(cx, |s, cx| s.may_quit("close", window, cx))
                .expect("the shell outlives its window's close request")
        });
    }

    /// A quit the person asked for (`how`: `key`, `close`, `close-window`):
    /// true when it goes ahead now (nothing running, or the error screen);
    /// else the question opens, or stays, and it waits for the answer.
    pub(super) fn may_quit(
        &mut self,
        how: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.fatal.is_some() {
            return true;
        }
        if self.asking.is_some() {
            self.log("quit asked again", serde_json::json!({ "how": how }));
            return false;
        }
        let running = quit::running(self.lists.read(cx).all());
        if running == 0 {
            self.log("quit", serde_json::json!({ "how": how, "asked": false }));
            return true;
        }
        self.asking = Some(Asking {
            running,
            back: window.focused(cx),
        });
        let this = cx.entity().downgrade();
        keys::open_layer(cx, Layer::Quit, move |window, cx| {
            let _ = this.update(cx, |s, cx| s.keep_building(window, cx));
        });
        window.focus(&self.quit_focus.keep, cx);
        self.log(
            "quit asked",
            serde_json::json!({ "how": how, "running": running }),
        );
        cx.notify();
        false
    }

    /// `Keep building`, Escape, the scrim: the question closes, the app
    /// stays, focus goes back where it was.
    fn keep_building(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let asking = self
            .asking
            .take()
            .expect("Keep building answers an open question");
        keys::close_layer(cx, Layer::Quit);
        match asking.back {
            Some(handle) => window.focus(&handle, cx),
            None => self.chat.update(cx, |c, cx| c.focus_composer(window, cx)),
        }
        self.log("quit kept", serde_json::json!({}));
        cx.notify();
    }

    /// `Quit`: GPUI's quit, which stops the core (`stop_core_on_quit`).
    fn quit_anyway(&mut self, cx: &mut Context<Self>) {
        self.log(
            "quit",
            serde_json::json!({ "how": "answer", "asked": true }),
        );
        cx.quit();
    }

    /// The question, while it is up.
    pub(super) fn quit_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let running = self.asking.as_ref()?.running;
        let title = quit::title(running);
        let body = quit::body(running);
        let fonts = fonts(cx);
        let size = window.viewport_size();
        let keep = {
            let this = cx.entity().downgrade();
            Button::new("quit-keep", quit::KEEP)
                .kind(Kind::Primary)
                .focus(self.quit_focus.keep.clone())
                .on_press(move |window, cx| {
                    let _ = this.update(cx, |s, cx| s.keep_building(window, cx));
                })
        };
        let go = {
            let this = cx.entity().downgrade();
            Button::new("quit-quit", quit::QUIT)
                .focus(self.quit_focus.quit.clone())
                .on_press(move |_, cx| {
                    let _ = this.update(cx, |s, cx| s.quit_anyway(cx));
                })
        };
        let (keep_focus, quit_focus) = (self.quit_focus.keep.clone(), self.quit_focus.quit.clone());
        let step = move |window: &mut Window, cx: &mut App| {
            let to = if keep_focus.is_focused(window) {
                &quit_focus
            } else {
                &keep_focus
            };
            window.focus(to, cx);
        };
        let back = step.clone();
        let dialog = div()
            .id("quit-ask")
            .role(Role::AlertDialog)
            .aria_label(title.clone())
            .aria_description(body)
            .a11y_synthetic_children(|builder| builder.parent_node().set_modal())
            .on_action(move |_: &FocusNext, window, cx| {
                cx.stop_propagation();
                step(window, cx)
            })
            .on_action(move |_: &FocusPrevious, window, cx| {
                cx.stop_propagation();
                back(window, cx)
            })
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(px(420f32.min(f32::from(size.width) - 32.)))
            .p(px(24.))
            .rounded(px(14.))
            .border_1()
            .border_color(colour::line())
            .bg(colour::cave())
            .shadow(shadow::panel())
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(theme::text(div(), TextKind::PaneTitle, &fonts).child(title))
            .child(theme::text(div(), TextKind::Prose, &fonts).child(body))
            .child(
                div()
                    .pt(px(8.))
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(go)
                    .child(keep),
            );
        let this = cx.entity().downgrade();
        let layer = div()
            .id("quit-scrim")
            .w(size.width)
            .h(size.height)
            .bg(theme::settings::scrim())
            .flex()
            .items_center()
            .justify_center()
            .occlude()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let _ = this.update(cx, |s, cx| s.keep_building(window, cx));
            })
            .child(dialog);
        Some(
            deferred(anchored().position(point(px(0.), px(0.))).child(layer))
                .with_priority(40)
                .into_any_element(),
        )
    }
}
