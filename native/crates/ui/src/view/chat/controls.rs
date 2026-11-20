//! The composer footer's controls: [`footer_pickers`] is what the footer
//! mounts before Send and Stop (inventory `app.md` 6.6): the limit chip,
//! the context meter, the resolution picker, the chat model picker and the
//! effort picker, all drawn by [`crate::view::pickers::footer::FooterPickers`],
//! which also holds the message's choice (`state::choice`) that a send
//! reads.

use gpui_kit::*;

use crate::view::pickers::footer::FooterPickers;

/// The footer's pickers.
pub fn footer_pickers(pickers: &Entity<FooterPickers>) -> AnyElement {
    div()
        .flex()
        .items_center()
        .min_w_0()
        .child(pickers.clone())
        .into_any_element()
}
