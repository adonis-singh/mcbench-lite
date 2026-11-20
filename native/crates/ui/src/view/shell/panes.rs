//! The panes that are not the chat: the world pane, and the placeholder a
//! place shows until its milestone builds it.

use gpui_kit::*;

use super::lists::Lists;
use crate::state::place::{self, Place};
use crate::view::controls::fonts;
use crate::view::theme::{self, TextKind, colour};

/// The world pane: where the 3D view goes (a later milestone). It draws
/// its black background (`.viewer`, `#000` in both themes) and nothing
/// else, and adds no node to the accessibility tree yet.
pub struct WorldPane;

impl Render for WorldPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().bg(colour::viewer())
    }
}

/// A place not built yet: a region named by the place's title, saying it.
pub fn placeholder(place: &Place, lists: &Lists, cx: &App) -> AnyElement {
    let fonts = fonts(cx);
    let name = place::place_title(place, lists).unwrap_or_else(|| place::kind(place).into());
    div()
        .id("placeholder")
        .role(Role::Region)
        .aria_label(name.clone())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(theme::text(div(), TextKind::EmptyHeading, &fonts).child(name))
        .into_any_element()
}

/// The missing screen (`app.md` 3.5): an address names a chat or bench the
/// lists do not have. Its texts are the page's; whether it is archived
/// comes from the core's archive (none while that is read).
pub fn missing(shell: &super::Shell, cx: &mut Context<super::Shell>) -> AnyElement {
    use crate::state::place::Mode;
    use crate::view::controls::{Button, Kind};
    let Place::Missing { mode, archived, .. } = shell.place().clone() else {
        panic!("the missing screen is drawn for a missing place");
    };
    let fonts = fonts(cx);
    let what = match mode {
        Mode::Build => "chat",
        Mode::Bench => "bench",
    };
    let heading = if archived == Some(true) {
        format!("This {what} is archived")
    } else {
        format!("There is no {what} at this address")
    };
    let sub = match archived {
        None => "Looking in the archive\u{2026}",
        Some(true) => {
            "Restore it to open it here. What is archived is deleted 7 days after it was archived."
        }
        Some(false) => "It was deleted, or the address was mistyped.",
    };
    let this = cx.entity().downgrade();
    let (new_label, new_place) = match mode {
        Mode::Build => ("New chat", Place::NewChat { id: None }),
        Mode::Bench => ("New bench", Place::NewBench),
    };
    let new = Button::new("missing-new", new_label).on_press(move |window, cx| {
        let place = new_place.clone();
        // par-chat: New chat's model check.
        let _ = this.update(cx, |s, cx| s.new_here(place, window, cx));
    });
    let restore = (archived == Some(true)).then(|| {
        let this = cx.entity().downgrade();
        Button::new("missing-restore", "Restore")
            .kind(Kind::Primary)
            .on_press(move |window, cx| {
                let _ = this.update(cx, |s, cx| s.restore_missing(window, cx));
            })
    });
    div()
        .id("missing")
        .role(Role::Region)
        .aria_label(heading.clone())
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .max_w(px(340.))
                .p(px(30.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(13.))
                .child(theme::text(div(), TextKind::EmptyHeading, &fonts).child(heading))
                .child(theme::text(div(), TextKind::EmptySub, &fonts).child(sub))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(8.))
                        .mt(px(6.))
                        .children(restore)
                        .child(new),
                ),
        )
        .into_any_element()
}
