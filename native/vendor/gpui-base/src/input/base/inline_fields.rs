//! mcbench patch: editable spans ("fields"). A field is a byte range of the
//! text that stays plain text in the rope, drawn as a box the person types
//! into. From outside it is one unit, as a token is: the caret steps over
//! it, a selection that reaches into it takes all of it, and deleting such a
//! selection removes the field. While the caret is in it (the active field)
//! edits stay inside, the range grows and shrinks with them, a newline is
//! refused, and Left at its start or Right at its end steps out at that edge.
//! Coordinates are source bytes, as tokens' are.
use std::ops::Range;

use gpui::{Context, Hsla, Pixels, SharedString, Window, px};

use super::{InputBaseState, InputModeKind};

/// A field as the application names it: `id` the resource it came from (the
/// same id may name several fields), `name` what it asks for, drawn in the
/// empty box and given to readers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InlineField {
    id: SharedString,
    name: SharedString,
}

impl InlineField {
    pub fn new(id: impl Into<SharedString>, name: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
        }
    }
    pub fn id(&self) -> &SharedString {
        &self.id
    }
    pub fn name(&self) -> &SharedString {
        &self.name
    }
}

/// A field and its current half-open UTF-8 byte range (empty while nothing
/// is typed in it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineFieldSpan {
    range: Range<usize>,
    field: InlineField,
}

impl InlineFieldSpan {
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }
    pub fn field(&self) -> &InlineField {
        &self.field
    }
}

/// How the boxes are drawn: `background` and a 1 px `border` (`focus_border`
/// while the caret is in it) around the value with `inset` on each side and
/// `radius` corners; an empty box shows its name in `placeholder` and is as
/// wide as the name plus one `0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InlineFieldStyle {
    pub background: Hsla,
    pub border: Hsla,
    pub focus_border: Hsla,
    pub placeholder: Hsla,
    pub radius: Pixels,
    pub inset: Pixels,
}

impl Default for InlineFieldStyle {
    fn default() -> Self {
        Self {
            background: gpui::transparent_black(),
            border: gpui::black().opacity(0.3),
            focus_border: gpui::black().opacity(0.6),
            placeholder: gpui::black().opacity(0.45),
            radius: px(4.),
            inset: px(4.),
        }
    }
}

/// A rejected field operation never changes the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineFieldError {
    /// A range outside the text, off a character boundary, or overlapping
    /// another field or a token.
    InvalidRange,
    /// A field's text holds a line break.
    LineBreak,
    /// No field at that index.
    NoSuchField,
    /// Fields are not shown by this mode (code editors, masks, passwords) or
    /// an IME composition is open.
    UnsupportedMode,
}

impl std::error::Error for InlineFieldError {}
impl std::fmt::Display for InlineFieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidRange => "field range is outside the text, splits a character or overlaps",
            Self::LineBreak => "a field holds no line break",
            Self::NoSuchField => "no field at that index",
            Self::UnsupportedMode => "fields are not supported here now",
        })
    }
}

/// The fields before and after one edit, recorded with it so undo and redo
/// put back both the text and its boxes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FieldDelta {
    before: Vec<InlineFieldSpan>,
    after: Vec<InlineFieldSpan>,
}

impl FieldDelta {
    /// One edit's fields before and the next one's after: two keystrokes
    /// kept as one change.
    pub(super) fn joined(&self, next: &FieldDelta) -> FieldDelta {
        FieldDelta {
            before: self.before.clone(),
            after: next.after.clone(),
        }
    }
}

impl<M: InputModeKind> InputBaseState<M> {
    pub(super) fn field_spans(&self) -> &[InlineFieldSpan] {
        &self.inline_fields
    }

    /// The field the caret is in: the active one while every selection lies
    /// within its range (edges included).
    pub(super) fn caret_field(&self) -> Option<usize> {
        let ix = self.active_field?;
        let range = self.inline_fields.get(ix)?.range.clone();
        self.selections
            .iter()
            .all(|s| range.start <= s.start && s.end <= range.end)
            .then_some(ix)
    }

    /// Forgets the active field once the selection has left it.
    pub(super) fn reconcile_field(&mut self) {
        if self.editing_field.is_some() {
            return;
        }
        self.active_field = self.caret_field();
    }

    /// `offset` moved out of a field the caret is not in, by `bias`; `None`
    /// when no such field holds it.
    pub(super) fn field_boundary(&self, offset: usize, bias: sum_tree::Bias) -> Option<usize> {
        let active = self.caret_field();
        self.inline_fields
            .iter()
            .enumerate()
            .find(|(ix, f)| Some(*ix) != active && f.range.start < offset && offset < f.range.end)
            .map(|(_, f)| {
                if bias == sum_tree::Bias::Left {
                    f.range.start
                } else {
                    f.range.end
                }
            })
    }

    /// An edit while the caret is in a field stays in it: the part of
    /// `range` outside the field is dropped. `None` when nothing is left to
    /// replace and nothing is inserted.
    pub(super) fn clamp_to_field(
        &self,
        range: Range<usize>,
        inserts: bool,
    ) -> Option<Range<usize>> {
        let Some(ix) = self.caret_field() else {
            return Some(range);
        };
        let field = &self.inline_fields[ix].range;
        let clamped =
            range.start.clamp(field.start, field.end)..range.end.clamp(field.start, field.end);
        (inserts || !clamped.is_empty()).then_some(clamped)
    }

    /// Text typed or pasted into a field: one line.
    pub(super) fn field_text<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        if self.caret_field().is_some() && text.contains(['\n', '\r']) {
            text.replace(['\n', '\r'], "").into()
        } else {
            text.into()
        }
    }

    /// The fields after `range` became `new_len` bytes: the caret's field
    /// grows or shrinks with an edit inside it, a field the edit reaches into
    /// otherwise goes, and the ones after it move.
    pub(super) fn edit_fields(
        &mut self,
        range: &Range<usize>,
        new_len: usize,
    ) -> Option<Box<FieldDelta>> {
        if self.replaying_history
            || (self.inline_fields.is_empty() && self.pending_fields.is_empty())
        {
            self.editing_field = None;
            return None;
        }
        let before = self.inline_fields.clone();
        let caret = self.editing_field.take().or_else(|| self.caret_field());
        let shift = new_len as isize - range.len() as isize;
        let mut active = None;
        let mut kept = Vec::with_capacity(before.len());
        for (ix, span) in before.iter().enumerate() {
            let r = &span.range;
            let inside = Some(ix) == caret && r.start <= range.start && range.end <= r.end;
            let reaches = if r.is_empty() {
                range.start < r.start && r.start < range.end
            } else {
                range.start < r.end && r.start < range.end
            };
            if inside {
                if Some(ix) == self.active_field {
                    active = Some(kept.len());
                }
                kept.push(InlineFieldSpan {
                    range: r.start..r.end.checked_add_signed(shift).expect("field end"),
                    field: span.field.clone(),
                });
            } else if reaches {
                continue;
            } else {
                let moved = if r.start >= range.end {
                    r.start.checked_add_signed(shift).expect("field start")
                        ..r.end.checked_add_signed(shift).expect("field end")
                } else {
                    r.clone()
                };
                if Some(ix) == self.active_field {
                    active = Some(kept.len());
                }
                kept.push(InlineFieldSpan {
                    range: moved,
                    field: span.field.clone(),
                });
            }
        }
        for pending in std::mem::take(&mut self.pending_fields) {
            kept.push(InlineFieldSpan {
                range: range.start + pending.range.start..range.start + pending.range.end,
                field: pending.field,
            });
        }
        kept.sort_by_key(|s| s.range.start);
        self.inline_fields = kept;
        self.active_field = active;
        (self.inline_fields != before).then(|| {
            Box::new(FieldDelta {
                before,
                after: self.inline_fields.clone(),
            })
        })
    }

    /// Undo and redo put back the fields recorded with the edit; the caret
    /// is in none.
    pub(super) fn replay_fields(&mut self, delta: Option<&FieldDelta>, undo: bool) {
        if let Some(delta) = delta {
            self.inline_fields = if undo {
                delta.before.clone()
            } else {
                delta.after.clone()
            };
            self.active_field = None;
        }
    }

    /// Left at the start of the caret's field, or Right at its end, with no
    /// selection: the caret steps out of the box at that edge. True when it
    /// did.
    pub(super) fn step_out_of_field(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(ix) = self.caret_field() else {
            return false;
        };
        if !self.selections.is_single() || !self.active_selection().is_empty() {
            return false;
        }
        let range = self.inline_fields[ix].range.clone();
        let at = self.cursor();
        if (forward && at == range.end) || (!forward && at == range.start) {
            self.active_field = None;
            self.undo_manager.break_transaction_coalescing();
            self.pause_blink_cursor(cx);
            cx.notify();
            return true;
        }
        false
    }

    /// The field whose box holds `position` in the last frame, and the byte
    /// of its value nearest it.
    pub(super) fn field_press(&self, position: gpui::Point<Pixels>) -> Option<(usize, usize)> {
        self.field_bounds
            .iter()
            .find(|(index, bounds, _, _)| {
                *index < self.inline_fields.len() && bounds.contains(&position)
            })
            .map(|(index, bounds, inset, value)| {
                (
                    *index,
                    value.closest_index_for_x(position.x - bounds.origin.x - *inset),
                )
            })
    }

    fn check_field_mode(&self) -> Result<(), InlineFieldError> {
        if self.ime_marked_range.is_some()
            || M::CODE_EDITOR
            || self.masked
            || !self.mask_pattern.is_none()
        {
            return Err(InlineFieldError::UnsupportedMode);
        }
        Ok(())
    }
}

macro_rules! field_api {
    ($mode:ty) => {
        impl InputBaseState<$mode> {
            /// Replace the selection with `text`, whose `fields` (ranges in
            /// `text`) become boxes. One undo step.
            pub fn replace_with_fields(
                &mut self,
                text: &str,
                fields: Vec<(Range<usize>, InlineField)>,
                window: &mut Window,
                cx: &mut Context<Self>,
            ) -> Result<(), InlineFieldError> {
                self.check_field_mode()?;
                let mut last = 0;
                for (range, _) in &fields {
                    if range.start < last
                        || range.start > range.end
                        || range.end > text.len()
                        || !text.is_char_boundary(range.start)
                        || !text.is_char_boundary(range.end)
                    {
                        return Err(InlineFieldError::InvalidRange);
                    }
                    if text[range.clone()].contains(['\n', '\r']) {
                        return Err(InlineFieldError::LineBreak);
                    }
                    last = range.end;
                }
                self.active_field = None;
                let at = self.normalize_token_range(self.selected_range());
                self.pending_fields = fields
                    .into_iter()
                    .map(|(range, field)| InlineFieldSpan { range, field })
                    .collect();
                self.undo_manager.break_transaction_coalescing();
                self.undo_manager
                    .set_pending_intent(super::undo_manager::EditIntent::Atomic);
                let range_utf16 = self.range_to_utf16(&at);
                self.with_edits_allowed(|state| {
                    gpui::EntityInputHandler::replace_text_in_range(
                        state,
                        Some(range_utf16),
                        text,
                        window,
                        cx,
                    )
                });
                self.pending_fields.clear();
                Ok(())
            }

            /// The fields in text order.
            pub fn fields(&self) -> &[InlineFieldSpan] {
                self.field_spans()
            }

            /// Each field's box as the last frame drew it, by field index.
            pub fn field_bounds(&self) -> Vec<(usize, gpui::Bounds<Pixels>)> {
                self.field_bounds
                    .iter()
                    .map(|(index, bounds, _, _)| (*index, *bounds))
                    .collect()
            }

            /// The index of the field the caret is in.
            pub fn active_field(&self) -> Option<usize> {
                self.caret_field()
            }

            /// Puts the caret in field `ix` with its value selected (Tab into
            /// a box).
            pub fn focus_field(
                &mut self,
                ix: usize,
                cx: &mut Context<Self>,
            ) -> Result<(), InlineFieldError> {
                let range = self
                    .inline_fields
                    .get(ix)
                    .ok_or(InlineFieldError::NoSuchField)?
                    .range
                    .clone();
                self.active_field = Some(ix);
                self.selections.remove_all_but_active();
                let active = self.active_selection_mut();
                active.start = range.start;
                active.end = range.end;
                active.reversed = false;
                self.undo_manager.break_transaction_coalescing();
                self.pause_blink_cursor(cx);
                cx.notify();
                Ok(())
            }

            /// How the boxes are drawn.
            pub fn field_style(&self) -> InlineFieldStyle {
                self.field_style
            }

            /// Sets how the boxes are drawn.
            pub fn set_field_style(&mut self, style: InlineFieldStyle, cx: &mut Context<Self>) {
                self.field_style = style;
                self.document_revision = self.document_revision.wrapping_add(1);
                cx.notify();
            }
        }
    };
}
field_api!(super::InputMode);
field_api!(super::TextareaMode);
