//! The transcript's text selection: two points in the rows' text, and the
//! plain text it copies. A point names a row, a leaf of that row (a
//! paragraph, heading, code block or cell, [`super::doc::Leaf`]) and a byte
//! in the leaf. The text comes from the rows' model, not from laid-out
//! lines, so a selection through rows the virtual list has not laid out
//! copies them whole.

use super::doc::{Boundary, Leaf};
use unicode_segmentation::UnicodeSegmentation as _;

/// A place in the transcript's text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Point {
    /// The row.
    pub row: usize,
    /// The leaf in the row.
    pub leaf: usize,
    /// The byte in the leaf's text.
    pub offset: usize,
}

/// A selection: where it began and where it is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    /// Where the drag began.
    pub anchor: Point,
    /// Where it is.
    pub focus: Point,
}

impl Selection {
    /// A caret at `point`.
    pub fn caret(point: Point) -> Self {
        Selection {
            anchor: point,
            focus: point,
        }
    }

    /// Its start and end in reading order.
    pub fn ordered(&self) -> (Point, Point) {
        if self.anchor <= self.focus {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    /// Whether it selects nothing.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    /// The selected bytes of one leaf, if any.
    pub fn range_in(&self, row: usize, leaf: usize, len: usize) -> Option<std::ops::Range<usize>> {
        let (start, end) = self.ordered();
        let here = (row, leaf);
        if here < (start.row, start.leaf) || here > (end.row, end.leaf) || self.is_empty() {
            return None;
        }
        let from = if here == (start.row, start.leaf) {
            start.offset
        } else {
            0
        };
        let to = if here == (end.row, end.leaf) {
            end.offset
        } else {
            len
        };
        (from < to).then_some(from..to)
    }
}

/// The plain text of `selection`: leaves joined by their boundaries (a line
/// break, or a tab between cells), rows by line breaks, code verbatim.
/// `rows(index)` gives a row's leaves.
pub fn copied_text<'a>(selection: &Selection, rows: impl Fn(usize) -> &'a [Leaf]) -> String {
    let (start, end) = selection.ordered();
    let mut out = String::new();
    let mut first = true;
    for row in start.row..=end.row {
        let leaves = rows(row);
        for (index, leaf) in leaves.iter().enumerate() {
            let Some(range) = Selection {
                anchor: start,
                focus: end,
            }
            .range_in(row, index, leaf.text.len()) else {
                continue;
            };
            if !first {
                out.push(if index > 0 && leaf.boundary == Boundary::Tab {
                    '\t'
                } else {
                    '\n'
                });
            }
            first = false;
            out.push_str(&leaf.text[range]);
        }
    }
    out
}

/// The word around `offset` in `text`, for a double click.
pub fn word_at(text: &str, offset: usize) -> std::ops::Range<usize> {
    for (start, word) in text.split_word_bound_indices() {
        let end = start + word.len();
        if offset < end || end == text.len() {
            return start..end;
        }
    }
    0..0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf(boundary: Boundary, text: &str) -> Leaf {
        Leaf {
            boundary,
            text: text.into(),
        }
    }

    #[test]
    fn a_selection_across_rows_copies_blocks_cells_and_rows_as_lines() {
        let rows = [
            vec![leaf(Boundary::Line, "Hello there")],
            vec![
                leaf(Boundary::Line, "Head"),
                leaf(Boundary::Line, "A"),
                leaf(Boundary::Tab, "B"),
                leaf(Boundary::Line, "1"),
                leaf(Boundary::Tab, "2"),
                leaf(Boundary::Line, "fn main() {\n    x();\n}"),
            ],
            vec![leaf(Boundary::Line, "Last words")],
        ];
        let selection = Selection {
            anchor: Point {
                row: 2,
                leaf: 0,
                offset: 4,
            },
            focus: Point {
                row: 0,
                leaf: 0,
                offset: 6,
            },
        };
        assert_eq!(
            copied_text(&selection, |r| &rows[r]),
            "there\nHead\nA\tB\n1\t2\nfn main() {\n    x();\n}\nLast"
        );
    }

    #[test]
    fn a_double_click_takes_the_word() {
        assert_eq!(word_at("one two three", 5), 4..7);
        assert_eq!(word_at("one two", 7), 4..7);
    }
}
