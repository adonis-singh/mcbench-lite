//! A visual row whose offsets remain in source UTF-8 bytes.
use gpui::{App, Pixels, Point, ShapedLine, SharedString, TextAlign, Window, point, px};
use std::ops::Range;

pub(crate) struct InlineFragment {
    pub(crate) range: Range<usize>,
    pub(crate) x: Pixels,
    pub(crate) width: Pixels,
    pub(crate) text: Option<ShapedLine>,
    /// mcbench patch: an editable span's box (its value is `text`).
    pub(crate) field: Option<FieldBox>,
}

/// mcbench patch: how a field's fragment is drawn and entered.
pub(crate) struct FieldBox {
    /// The field's index in the input's fields.
    pub(crate) index: usize,
    /// The caret is in it: its edges are inside the box.
    pub(crate) active: bool,
    pub(crate) style: crate::input::InlineFieldStyle,
    /// The name, drawn while the box is empty.
    pub(crate) placeholder: Option<ShapedLine>,
}

impl InlineFragment {
    /// The x of byte `ix` (relative to the fragment's range) inside a
    /// field's box.
    fn field_x(&self, field: &FieldBox, ix: usize) -> Pixels {
        let line = self.text.as_ref().expect("a field's value is shaped");
        let inside = ix.clamp(self.range.start, self.range.end) - self.range.start;
        if field.active || (self.range.start < ix && ix < self.range.end) {
            self.x + field.style.inset + line.x_for_index(inside)
        } else if ix <= self.range.start {
            self.x
        } else {
            self.x + self.width
        }
    }
}

pub(crate) struct InputLine {
    pub(crate) len: usize,
    pub(crate) width: Pixels,
    pub(crate) text: SharedString,
    content: Content,
}
// Keep the ordinary shaped row inline: boxing it would add an allocation to
// every existing plain-text row. Only token rows allocate fragment storage.
#[allow(clippy::large_enum_variant)]
enum Content {
    Text(ShapedLine),
    Inline(Vec<InlineFragment>),
}
impl From<ShapedLine> for InputLine {
    fn from(line: ShapedLine) -> Self {
        Self {
            len: line.len,
            width: line.width,
            text: line.text.clone(),
            content: Content::Text(line),
        }
    }
}
impl InputLine {
    pub(crate) fn inline(text: SharedString, fragments: Vec<InlineFragment>) -> Self {
        let width = fragments.last().map_or(px(0.), |f| f.x + f.width);
        Self {
            len: text.len(),
            text,
            width,
            content: Content::Inline(fragments),
        }
    }
    pub(crate) fn x_for_index(&self, ix: usize) -> Pixels {
        match &self.content {
            Content::Text(line) => line.x_for_index(ix),
            Content::Inline(fragments) => {
                for f in fragments {
                    // mcbench patch: a field's start is its own (before the
                    // box), and with the caret in it so is its end.
                    if let Some(field) = &f.field
                        && (ix < f.range.end
                            || ix == f.range.start
                            || (field.active && ix == f.range.end))
                    {
                        return f.field_x(field, ix);
                    }
                    if ix < f.range.end {
                        return f.x
                            + f.text.as_ref().map_or(
                                if ix <= f.range.start { px(0.) } else { f.width },
                                |line| line.x_for_index(ix.saturating_sub(f.range.start)),
                            );
                    }
                }
                self.width
            }
        }
    }
    pub(crate) fn closest_index_for_x(&self, x: Pixels) -> usize {
        match &self.content {
            Content::Text(line) => line.closest_index_for_x(x),
            Content::Inline(fragments) => {
                for f in fragments {
                    if x <= f.x + f.width {
                        // mcbench patch: inside the caret's field, its text.
                        if let (Some(field), Some(line)) = (&f.field, &f.text)
                            && field.active
                        {
                            return f.range.start
                                + line.closest_index_for_x(x - f.x - field.style.inset);
                        }
                        if f.field.is_some() {
                            return if x - f.x < f.width / 2. {
                                f.range.start
                            } else {
                                f.range.end
                            };
                        }
                        return f.text.as_ref().map_or(
                            if x - f.x < f.width / 2. {
                                f.range.start
                            } else {
                                f.range.end
                            },
                            |line| f.range.start + line.closest_index_for_x(x - f.x),
                        );
                    }
                }
                self.len
            }
        }
    }
    pub(crate) fn index_for_x(&self, x: Pixels) -> Option<usize> {
        match &self.content {
            Content::Text(line) => line.index_for_x(x),
            Content::Inline(_) => {
                (x >= px(0.) && x <= self.width).then(|| self.closest_index_for_x(x))
            }
        }
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, false, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn paint_background(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.paint_pass(pos, height, align, width, true, window, cx);
    }
    #[allow(clippy::too_many_arguments)]
    fn paint_pass(
        &self,
        pos: Point<Pixels>,
        height: Pixels,
        align: TextAlign,
        width: Option<Pixels>,
        background: bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        let paint = |line: &ShapedLine, pos, align, width, window: &mut Window, cx: &mut App| {
            if background {
                let _ = line.paint_background(pos, height, align, width, window, cx);
            } else {
                let _ = line.paint(pos, height, align, width, window, cx);
            }
        };
        match &self.content {
            Content::Text(line) => paint(line, pos, align, width, window, cx),
            Content::Inline(fragments) => {
                let remaining = (width.unwrap_or(self.width) - self.width).max(px(0.));
                let offset = match align {
                    TextAlign::Right => remaining,
                    TextAlign::Center => remaining / 2.,
                    _ => px(0.),
                };
                for f in fragments {
                    // mcbench patch: a field is its box, then its value or
                    // its name inset in it.
                    if let Some(field) = &f.field {
                        let origin = pos + point(offset + f.x, px(0.));
                        if background {
                            let style = &field.style;
                            window.paint_quad(gpui::quad(
                                gpui::Bounds::new(
                                    origin + point(px(0.), px(1.)),
                                    gpui::size(f.width, height - px(2.)),
                                ),
                                style.radius,
                                style.background,
                                px(1.),
                                if field.active {
                                    style.focus_border
                                } else {
                                    style.border
                                },
                                gpui::BorderStyle::Solid,
                            ));
                        }
                        let inset = origin + point(field.style.inset, px(0.));
                        let value = f.text.as_ref().expect("a field's value is shaped");
                        if f.range.is_empty()
                            && let Some(name) = &field.placeholder
                        {
                            paint(name, inset, TextAlign::Left, None, window, cx);
                        } else {
                            paint(value, inset, TextAlign::Left, None, window, cx);
                        }
                        continue;
                    }
                    if let Some(line) = &f.text {
                        paint(
                            line,
                            pos + point(offset + f.x, px(0.)),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                }
            }
        }
    }
}

impl InputLine {
    /// mcbench patch: whether this row holds a field's box.
    pub(crate) fn has_fields(&self) -> bool {
        matches!(&self.content, Content::Inline(f) if f.iter().any(|f| f.field.is_some()))
    }

    /// mcbench patch: the field boxes of this row: the field's index, its x
    /// and width, its inset and its shaped value.
    pub(crate) fn field_boxes(&self) -> Vec<(usize, Pixels, Pixels, Pixels, ShapedLine)> {
        match &self.content {
            Content::Text(_) => Vec::new(),
            Content::Inline(fragments) => fragments
                .iter()
                .filter_map(|f| {
                    let field = f.field.as_ref()?;
                    Some((
                        field.index,
                        f.x,
                        f.width,
                        field.style.inset,
                        f.text.clone().expect("a field's value is shaped"),
                    ))
                })
                .collect(),
        }
    }
}
