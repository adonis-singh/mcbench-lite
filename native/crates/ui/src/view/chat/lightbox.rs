//! The lightbox (`.lightbox`, inventory 5.9): a picture over the whole
//! window, `rgba(0, 0, 0, 0.78)` behind it, the picture at most
//! `min(1100px, 88vw)` wide and `88vh` high with a 1 px `--line` border,
//! radius 12, the panel shadow, pixelated. A press anywhere on it closes it,
//! as does Escape (the `lightbox` layer, under the palette and over
//! Settings). It is a dialog holding the picture and a `Close` button, and
//! takes focus while open; focus goes back where it was when it closes.
//! Thumbnails of a message's pictures and of shots open it; the composer's
//! tiles do not.

use gpui_kit::*;

use super::super::icons::{self, Icon};
use super::super::{Activate, colour, keys, ring};
use super::message::picture_element;
use super::{ChatPane, Picture};
use crate::state::layers::Layer;

impl ChatPane {
    /// Opens the lightbox on `picture`.
    pub(crate) fn open_lightbox(
        &mut self,
        picture: Picture,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let back = window.focused(cx);
        self.lightbox = Some((picture, back));
        let this = cx.entity().downgrade();
        keys::open_layer(cx, Layer::Lightbox, move |window, cx| {
            let _ = this.update(cx, |pane, cx| pane.close_lightbox(window, cx));
        });
        window.focus(&self.lightbox_focus, cx);
        self.log("opened", serde_json::json!({ "layer": "lightbox" }));
        cx.notify();
    }

    /// Closes it; focus goes back where it was.
    pub(crate) fn close_lightbox(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((_, back)) = self.lightbox.take() else {
            return;
        };
        keys::close_layer(cx, Layer::Lightbox);
        match back {
            Some(handle) => window.focus(&handle, cx),
            None => self.focus_composer(window, cx),
        }
        self.log("closed", serde_json::json!({ "layer": "lightbox" }));
        cx.notify();
    }

    /// Whether the lightbox is open.
    pub fn lightbox_open(&self) -> bool {
        self.lightbox.is_some()
    }

    /// The layer, while open.
    pub(crate) fn lightbox_layer(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (picture, _) = self.lightbox.clone()?;
        let size = window.viewport_size();
        let (width, height) = (f32::from(size.width), f32::from(size.height));
        let close = ring(
            div()
                .id("lightbox-close")
                .role(Role::Button)
                .aria_label("Close")
                .track_focus(&self.lightbox_focus)
                .key_context("Part")
                .on_action(
                    cx.listener(|this, _: &Activate, window, cx| this.close_lightbox(window, cx)),
                )
                .on_click(cx.listener(|this, _, window, cx| this.close_lightbox(window, cx))),
        )
        .absolute()
        .top(px(16.))
        .right(px(16.))
        .size(px(32.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .text_color(colour::bone())
        .cursor_pointer()
        .child(icons::icon(Icon::X, 18.));
        let layer = div()
            .id("lightbox")
            .role(Role::Dialog)
            .aria_label("Picture")
            .occlude()
            .w(px(width))
            .h(px(height))
            .bg(colour::lightbox_scrim())
            .cursor(CursorStyle::PointingHand)
            .flex()
            .items_center()
            .justify_center()
            .on_click(cx.listener(|this, _, window, cx| this.close_lightbox(window, cx)))
            .child(
                div()
                    .id("lightbox-picture")
                    .role(Role::Image)
                    .aria_label("Picture")
                    .rounded(px(12.))
                    .border_1()
                    .border_color(colour::line())
                    .shadow(super::super::theme::shadow::panel())
                    .overflow_hidden()
                    .child(
                        picture_element(picture)
                            .max_w(px((width * 0.88).min(1100.)))
                            .max_h(px(height * 0.88)),
                    ),
            )
            .child(close);
        Some(
            deferred(anchored().position(point(px(0.), px(0.))).child(layer))
                .with_priority(30)
                .into_any_element(),
        )
    }
}
