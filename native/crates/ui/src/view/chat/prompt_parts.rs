//! A library prompt in the composer (bench agent; `web/composer.mjs`,
//! inventory `app.md` 6.5 and 6.11, `components.md` K.2): a prompt without
//! fill-ins arrives as one token, `/` and its title, that sends its text
//! (the text box's own atomic inline token, which a reader reads as its
//! label and Backspace removes whole); a prompt with fill-ins arrives as its
//! text with a box (the text box's editable span) in place of each, the caret
//! in the first. Tab and Shift+Tab walk the boxes; entering one says its name
//! (`Fill in: size`); a send while a box is empty puts the caret in it and
//! goes nowhere (the page's box that flashes and takes the focus). The
//! message names each prompt it came from (`promptLinks`) while its token or
//! any of its boxes is in it. The prompt's files join the draft.

use gpui_kit::component::input::{InlineField, InlineToken, InputContent};
use gpui_kit::*;

use super::ChatPane;
use crate::state::prompts::{self, Segment};
use crate::{LibraryPrompt, OutgoingFile, PromptLink};

// par-chat (stage four's parity pass): the page sends a prompt's files as
// their data URLs and the route refuses one that is not base64 data
// (`server/store.js` `decodeAttachment`); the window reads them first, with
// the route's words.
/// `Attachment "<name>" is not base64 data`.
fn not_base64(name: &str) -> String {
    format!("Attachment \"{name}\" is not base64 data")
}

/// A prompt's files as a message carries them: each data URL's bytes.
pub fn prompt_files(prompt: &LibraryPrompt) -> Result<Vec<OutgoingFile>, String> {
    use base64::Engine as _;
    prompt
        .attachments
        .iter()
        .map(|a| {
            let data = a
                .data
                .split_once(";base64,")
                .map(|(_, b)| b)
                .ok_or_else(|| not_base64(&a.name))?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|_| not_base64(&a.name))?;
            Ok(OutgoingFile {
                name: a.name.clone(),
                content_type: a.content_type.clone(),
                size: a.size,
                data: bytes.into(),
            })
        })
        .collect()
}

impl ChatPane {
    /// Inserts a prompt (`insertPrompt`): where the caret is, or (with
    /// `replace`, `Run bench`) in place of the whole draft.
    pub fn put_prompt(
        &mut self,
        prompt: &LibraryPrompt,
        replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let segments = prompts::prompt_segments(prompt);
        let link = PromptLink {
            id: prompt.id.clone(),
            title: prompt.title.clone(),
        };
        if replace {
            self.put_draft(crate::state::draft::empty(), window, cx);
            self.inserted.clear();
        }
        match segments.as_slice() {
            [Segment::Prompt { id, title, text }] if !text.is_empty() => {
                let token = InlineToken::new(id.clone(), text.clone())
                    .with_label(format!("/{}", prompts::title_or_untitled(title)));
                self.composer.update(cx, |c, cx| {
                    let at = c.selected_range();
                    c.replace_range_with_token(at, token, window, cx)
                        .unwrap_or_else(|e| panic!("a prompt's token fits the composer: {e:?}"));
                });
            }
            _ => {
                // Its text with an empty box in place of each fill-in.
                let mut text = String::new();
                let mut fields = Vec::new();
                for segment in &segments {
                    match segment {
                        Segment::Text(t) | Segment::Prompt { text: t, .. } => text.push_str(t),
                        Segment::Field { name, from, .. } => fields.push((
                            text.len()..text.len(),
                            InlineField::new(from.id.clone(), name.clone()),
                        )),
                    }
                }
                self.composer.update(cx, |c, cx| {
                    if replace {
                        c.set_value(InputContent::new(""), window, cx);
                    }
                    let start = c.selected_range().start;
                    c.replace_with_fields(&text, fields, window, cx)
                        .unwrap_or_else(|e| panic!("a prompt's boxes fit the composer: {e:?}"));
                    let first = c
                        .fields()
                        .iter()
                        .position(|f| f.range().start >= start)
                        .expect("the prompt's first box");
                    c.focus_field(first, cx).expect("a box");
                });
            }
        }
        if !self.inserted.iter().any(|l| l.id == link.id) {
            self.inserted.push(link);
        }
        match prompt_files(prompt) {
            Ok(files) => self.draft.attachments.extend(files),
            Err(error) => self.notice(error, window, cx),
        }
        self.composer_empty = false;
        self.composer.update(cx, |c, cx| c.focus(window, cx));
        cx.notify();
    }

    /// The composer's content as `composer.read` gives it: text, tokens
    /// and boxes with their values, each naming its prompt.
    pub fn composer_segments(&self, cx: &App) -> Vec<Segment> {
        let state = self.composer.read(cx);
        let text = state.value().to_string();
        let title = |id: &str| {
            self.inserted
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.title.clone())
                .unwrap_or_default()
        };
        let mut atoms: Vec<(std::ops::Range<usize>, Segment)> = state
            .tokens()
            .iter()
            .map(|s| {
                let id = s.token().id().to_string();
                (
                    s.range(),
                    Segment::Prompt {
                        title: title(&id),
                        id,
                        text: s.token().text().to_string(),
                    },
                )
            })
            .chain(state.fields().iter().map(|f| {
                let id = f.field().id().to_string();
                (
                    f.range(),
                    Segment::Field {
                        name: f.field().name().to_string(),
                        value: text[f.range()].to_string(),
                        from: PromptLink {
                            title: title(&id),
                            id,
                        },
                    },
                )
            }))
            .collect();
        atoms.sort_by_key(|(r, _)| r.start);
        let mut segments = Vec::new();
        let mut at = 0;
        for (range, atom) in atoms {
            if at < range.start {
                segments.push(Segment::Text(text[at..range.start].to_string()));
            }
            segments.push(atom);
            at = range.end;
        }
        if at < text.len() {
            segments.push(Segment::Text(text[at..].to_string()));
        }
        segments
    }

    /// The prompts the message names (`promptLinks`): each while its token
    /// or one of its boxes is in it.
    pub fn live_prompts(&mut self, cx: &App) -> Vec<PromptLink> {
        prompts::prompt_links(&self.composer_segments(cx))
    }

    /// Puts the caret in the next box after the caret (`back`: the one
    /// before), round; false with none.
    pub(crate) fn step_field(&mut self, back: bool, cx: &mut Context<Self>) -> bool {
        let (count, next) = {
            let c = self.composer.read(cx);
            let fields = c.fields();
            let at = c.selected_range();
            let next = match (c.active_field(), back) {
                (Some(ix), false) => (ix + 1) % fields.len().max(1),
                (Some(ix), true) => (ix + fields.len() - 1) % fields.len().max(1),
                (None, false) => fields
                    .iter()
                    .position(|f| f.range().start >= at.end)
                    .unwrap_or(0),
                (None, true) => fields
                    .iter()
                    .rposition(|f| f.range().end <= at.start)
                    .unwrap_or(fields.len().saturating_sub(1)),
            };
            (fields.len(), next)
        };
        if count == 0 {
            return false;
        }
        self.composer
            .update(cx, |c, cx| c.focus_field(next, cx).expect("a box"));
        true
    }

    /// A send with a box left empty: the caret goes in it, and the send
    /// does not happen (`emptyField`). True when it held the send back.
    pub(crate) fn hold_for_field(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(first) = self
            .composer
            .read(cx)
            .fields()
            .iter()
            .position(|f| f.range().is_empty())
        else {
            return false;
        };
        self.composer.update(cx, |c, cx| {
            c.focus_field(first, cx).expect("a box");
            c.focus(window, cx);
        });
        self.options.announcer.update(cx, |a, cx| {
            a.assertive("Fill in this part before sending".into(), cx)
        });
        true
    }

    /// Says the name of the box the caret has just entered (`Fill in:
    /// size`), once each time it enters one.
    pub(crate) fn say_field(&mut self, cx: &mut Context<Self>) {
        let entered = {
            let c = self.composer.read(cx);
            c.active_field()
                .map(|ix| (ix, c.fields()[ix].field().name().to_string()))
        };
        let index = entered.as_ref().map(|(ix, _)| *ix);
        if index == self.field_said {
            return;
        }
        self.field_said = index;
        if let Some((_, name)) = entered {
            self.options
                .announcer
                .update(cx, |a, cx| a.polite(format!("Fill in: {name}"), cx));
        }
    }

    /// Moves the composer's draft to `key`'s place when that place's draft
    /// is empty (`benchFromBuild`, `buildFromBench`): text and files, not
    /// selections.
    pub fn move_draft_to(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let now = self.current_draft(cx);
        if crate::state::draft::draft_empty(&now) || now.edit.is_some() {
            return;
        }
        if !crate::state::draft::draft_empty(&self.drafts.peek(key)) {
            return;
        }
        let moved = crate::state::draft::Draft {
            text: now.text.clone(),
            attachments: now.attachments.clone(),
            ..crate::state::draft::empty()
        };
        self.drafts.put(key, moved);
        self.put_draft(crate::state::draft::empty(), window, cx);
        self.inserted.clear();
        cx.notify();
    }
}
