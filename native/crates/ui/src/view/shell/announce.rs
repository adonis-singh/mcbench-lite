//! The window's two live regions: the status (polite) and the alert
//! (assertive). Their text is both their name (AT-SPI and UIA announce a
//! live region's name change) and their value (macOS announces its value
//! change); none, not an empty one, between a turn's start and its
//! announcement, so the same text again is a change and is announced.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

/// The live regions.
#[derive(Default)]
pub struct Announcer {
    polite: LiveText,
    assertive: LiveText,
}

impl Announcer {
    /// Says `text` politely (an answer, a chat's title on opening).
    pub fn polite(&mut self, text: String, cx: &mut Context<Self>) {
        self.polite.set(text);
        cx.notify();
    }

    /// Says `text` at once (an error).
    pub fn assertive(&mut self, text: String, cx: &mut Context<Self>) {
        self.assertive.set(text);
        cx.notify();
    }

    /// Empties both regions (a turn starts; a place is left).
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        self.polite.clear();
        self.assertive.clear();
        cx.notify();
    }

    /// Empties the polite region.
    pub fn clear_polite(&mut self, cx: &mut Context<Self>) {
        self.polite.clear();
        cx.notify();
    }
}

impl Render for Announcer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let polite = self.polite.frame(cx);
        let assertive = self.assertive.frame(cx);
        div()
            .absolute()
            .size(px(1.))
            .child(
                div()
                    .id("polite")
                    .role(Role::Status)
                    .when_some(polite, |d, text| {
                        d.aria_label(text.clone()).aria_value(text)
                    })
                    .a11y_synthetic_children(|builder| {
                        builder.parent_node().set_live(accesskit::Live::Polite)
                    })
                    .absolute()
                    .size(px(1.)),
            )
            .child(
                div()
                    .id("assertive")
                    .role(Role::Alert)
                    .when_some(assertive, |d, text| {
                        d.aria_label(text.clone()).aria_value(text)
                    })
                    .a11y_synthetic_children(|builder| {
                        builder.parent_node().set_live(accesskit::Live::Assertive)
                    })
                    .absolute()
                    .size(px(1.)),
            )
    }
}

/// A live region's text. Assistive technology announces a change between
/// two frames' trees, so text equal to what the last frame showed is put off
/// by one frame, which shows the region empty: a turn's start empties it and
/// the same error arriving before the next frame (the stand-in's, or a
/// provider's refusal) would otherwise be no change at all.
#[derive(Default)]
struct LiveText {
    text: Option<String>,
    pending: Option<String>,
    drawn: Option<String>,
}

impl LiveText {
    fn set(&mut self, text: String) {
        if self.drawn.as_ref() == Some(&text) {
            self.text = None;
            self.pending = Some(text);
        } else {
            self.text = Some(text);
            self.pending = None;
        }
    }

    fn clear(&mut self) {
        self.text = None;
        self.pending = None;
    }

    /// The text this frame shows; a put-off text shows in the next frame.
    fn frame<T: 'static>(&mut self, cx: &mut Context<T>) -> Option<String> {
        let shown = self.text.clone();
        self.drawn = shown.clone();
        if shown.is_none()
            && let Some(pending) = self.pending.take()
        {
            self.text = Some(pending);
            cx.notify();
        }
        shown
    }
}
