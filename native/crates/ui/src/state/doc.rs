//! Markdown as blocks the transcript draws and exposes: parsed with
//! `markdown` (GitHub's flavour: tables, strike, task lists, autolinks)
//! into headings, paragraphs, lists, code, quotes, tables, rules and
//! images, each with its text and styled runs. No GPUI types.

use markdown::mdast::{AlignKind, Node};
use std::ops::Range;

/// A table column's alignment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// None given.
    None,
    /// `:--`.
    Left,
    /// `:-:`.
    Center,
    /// `--:`.
    Right,
}

/// A run of text with one style.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Run {
    /// Bytes of the inline's text.
    pub range: Range<usize>,
    /// `**strong**`.
    pub strong: bool,
    /// `*emphasis*`.
    pub emphasis: bool,
    /// `~~strike~~`.
    pub strike: bool,
    /// `` `code` ``.
    pub code: bool,
    /// The link it belongs to, an index of [`Inline::links`].
    pub link: Option<usize>,
}

/// A link inside an inline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// Bytes of the inline's text.
    pub range: Range<usize>,
    /// The address.
    pub url: String,
}

/// Text with styled runs that cover it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Inline {
    /// The text.
    pub text: String,
    /// Runs in order, covering the text.
    pub runs: Vec<Run>,
    /// Its links in order.
    pub links: Vec<Link>,
}

/// A list's item.
#[derive(Clone, Debug, PartialEq)]
pub struct ListItem {
    /// A task list's box: checked or not.
    pub checked: Option<bool>,
    /// Its blocks.
    pub blocks: Vec<Block>,
}

/// A block of a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    /// A paragraph.
    Paragraph(Inline),
    /// A heading, level 1 to 6.
    Heading {
        /// The level.
        level: u8,
        /// The text.
        text: Inline,
    },
    /// A list, maybe nested in an item of another.
    List {
        /// Numbered.
        ordered: bool,
        /// The first number.
        start: u32,
        /// The items.
        items: Vec<ListItem>,
    },
    /// A fenced or indented code block.
    Code {
        /// The language after the fence.
        lang: Option<String>,
        /// The code, verbatim.
        text: String,
    },
    /// A block quote.
    Quote(Vec<Block>),
    /// A table.
    Table {
        /// Each column's alignment.
        align: Vec<Align>,
        /// The header row's cells.
        head: Vec<Inline>,
        /// The body's rows of cells.
        rows: Vec<Vec<Inline>>,
    },
    /// A thematic break.
    Rule,
    /// An image.
    Image {
        /// Its alt text.
        alt: String,
        /// Its address.
        url: String,
    },
}

/// Parses markdown into blocks.
pub fn parse(text: &str) -> Vec<Block> {
    let root = markdown::to_mdast(text, &markdown::ParseOptions::gfm())
        .unwrap_or_else(|e| panic!("markdown without MDX never fails to parse: {e}"));
    blocks_of(root.children().map(Vec::as_slice).unwrap_or_default())
}

fn blocks_of(nodes: &[Node]) -> Vec<Block> {
    let mut blocks = Vec::new();
    for node in nodes {
        block(node, &mut blocks);
    }
    blocks
}

fn block(node: &Node, out: &mut Vec<Block>) {
    match node {
        Node::Paragraph(p) => paragraph(&p.children, out),
        Node::Heading(h) => out.push(Block::Heading {
            level: h.depth,
            text: inline(&h.children),
        }),
        Node::List(list) => out.push(Block::List {
            ordered: list.ordered,
            start: list.start.unwrap_or(1),
            items: list
                .children
                .iter()
                .map(|item| match item {
                    Node::ListItem(item) => ListItem {
                        checked: item.checked,
                        blocks: blocks_of(&item.children),
                    },
                    other => ListItem {
                        checked: None,
                        blocks: blocks_of(std::slice::from_ref(other)),
                    },
                })
                .collect(),
        }),
        Node::Code(code) => out.push(Block::Code {
            lang: code.lang.clone().filter(|l| !l.is_empty()),
            text: code.value.clone(),
        }),
        Node::Blockquote(quote) => out.push(Block::Quote(blocks_of(&quote.children))),
        Node::Table(table) => {
            let mut rows = table.children.iter().map(|row| {
                row.children()
                    .map(|cells| {
                        cells
                            .iter()
                            .map(|cell| {
                                inline(cell.children().map(Vec::as_slice).unwrap_or_default())
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default()
            });
            let head = rows.next().unwrap_or_default();
            out.push(Block::Table {
                align: table
                    .align
                    .iter()
                    .map(|a| match a {
                        AlignKind::Left => Align::Left,
                        AlignKind::Center => Align::Center,
                        AlignKind::Right => Align::Right,
                        AlignKind::None => Align::None,
                    })
                    .collect(),
                head,
                rows: rows.collect(),
            });
        }
        Node::ThematicBreak(_) => out.push(Block::Rule),
        Node::Html(html) => out.push(Block::Paragraph(plain(&html.value))),
        // Footnote definitions and anything else with children: their blocks.
        other => match other.children() {
            Some(children) => {
                for child in children {
                    block(child, out);
                }
            }
            None => paragraph(std::slice::from_ref(other), out),
        },
    }
}

/// A paragraph's inline nodes; an image in it becomes a block of its own,
/// splitting the paragraph around it.
fn paragraph(children: &[Node], out: &mut Vec<Block>) {
    let mut pending: Vec<Node> = Vec::new();
    let flush = |pending: &mut Vec<Node>, out: &mut Vec<Block>| {
        let text = inline(pending);
        if !text.text.trim().is_empty() {
            out.push(Block::Paragraph(text));
        }
        pending.clear();
    };
    for child in children {
        if let Node::Image(image) = child {
            flush(&mut pending, out);
            out.push(Block::Image {
                alt: image.alt.clone(),
                url: image.url.clone(),
            });
        } else {
            pending.push(child.clone());
        }
    }
    flush(&mut pending, out);
}

fn plain(text: &str) -> Inline {
    Inline {
        text: text.to_string(),
        runs: vec![Run {
            range: 0..text.len(),
            ..Run::default()
        }],
        links: Vec::new(),
    }
}

/// Inline nodes as text and runs.
pub fn inline(nodes: &[Node]) -> Inline {
    let mut out = Inline::default();
    for node in nodes {
        walk(node, Run::default(), &mut out);
    }
    // Neighbouring runs of one style merge.
    let mut merged: Vec<Run> = Vec::new();
    for run in out.runs.drain(..) {
        if run.range.is_empty() {
            continue;
        }
        match merged.last_mut() {
            Some(last)
                if last.range.end == run.range.start
                    && (
                        last.strong,
                        last.emphasis,
                        last.strike,
                        last.code,
                        last.link,
                    ) == (run.strong, run.emphasis, run.strike, run.code, run.link) =>
            {
                last.range.end = run.range.end;
            }
            _ => merged.push(run),
        }
    }
    out.runs = merged;
    out
}

fn push_text(text: &str, style: &Run, out: &mut Inline) {
    let start = out.text.len();
    out.text.push_str(text);
    out.runs.push(Run {
        range: start..out.text.len(),
        ..style.clone()
    });
}

fn walk(node: &Node, style: Run, out: &mut Inline) {
    match node {
        Node::Text(t) => push_text(&t.value, &style, out),
        Node::InlineCode(c) => push_text(
            &c.value,
            &Run {
                code: true,
                ..style
            },
            out,
        ),
        Node::Break(_) => push_text("\n", &style, out),
        Node::Html(h) => push_text(&h.value, &style, out),
        Node::Strong(s) => {
            for child in &s.children {
                walk(
                    child,
                    Run {
                        strong: true,
                        ..style.clone()
                    },
                    out,
                );
            }
        }
        Node::Emphasis(e) => {
            for child in &e.children {
                walk(
                    child,
                    Run {
                        emphasis: true,
                        ..style.clone()
                    },
                    out,
                );
            }
        }
        Node::Delete(d) => {
            for child in &d.children {
                walk(
                    child,
                    Run {
                        strike: true,
                        ..style.clone()
                    },
                    out,
                );
            }
        }
        Node::Link(link) => {
            let index = out.links.len();
            let start = out.text.len();
            out.links.push(Link {
                range: start..start,
                url: link.url.clone(),
            });
            for child in &link.children {
                walk(
                    child,
                    Run {
                        link: Some(index),
                        ..style.clone()
                    },
                    out,
                );
            }
            out.links[index].range.end = out.text.len();
        }
        // An image inside a link or a heading reads as its alt text.
        Node::Image(image) => push_text(&image.alt, &style, out),
        other => match other.children() {
            Some(children) => {
                for child in children {
                    walk(child, style.clone(), out);
                }
            }
            None => push_text(&other.to_string(), &style, out),
        },
    }
}

/// What comes before a piece of text when text is copied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Boundary {
    /// A line break: a new block, row or message.
    Line,
    /// A tab: the next cell of a table row.
    Tab,
}

/// A piece of selectable text: a paragraph, a heading, a code block or a
/// cell, in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct Leaf {
    /// What separates it from the piece before.
    pub boundary: Boundary,
    /// Its text.
    pub text: String,
}

/// How a block is read aloud inside a message or an announcement: prose
/// (paragraphs, headings, lists, quotes) as its text; a code block and a
/// table by kind and size only (`code, rust, 5 lines`, `table, 4 rows, 3
/// columns`), since read out they are noise and their content is reached by
/// entering them; an image as `image, <alt>`; a rule not at all.
pub fn spoken_block(block: &Block) -> Option<String> {
    match block {
        Block::Paragraph(_) | Block::Heading { .. } | Block::List { .. } | Block::Quote(_) => {
            Some(spoken(&leaves(std::slice::from_ref(block))))
        }
        Block::Code { lang, text } => {
            let lines = text.trim_end_matches('\n').lines().count();
            let size = if lines == 1 {
                "1 line".to_string()
            } else {
                format!("{lines} lines")
            };
            Some(match lang {
                Some(lang) => format!("code, {lang}, {size}"),
                None => format!("code, {size}"),
            })
        }
        Block::Table { head, rows, .. } => Some(format!(
            "table, {} rows, {} columns",
            rows.len() + 1,
            head.len()
        )),
        Block::Image { alt, .. } => Some(format!("image, {alt}")),
        Block::Rule => None,
    }
}

/// Blocks read aloud one after another, a line break between them.
pub fn spoken_blocks(blocks: &[Block]) -> String {
    blocks
        .iter()
        .filter_map(spoken_block)
        .collect::<Vec<_>>()
        .join("\n")
}

/// A top-level block's accessible name as a stop of Right and Left: its kind
/// and then how it is read (`heading level 1, A lamp on a lever`,
/// `list, 3 items, …`, `code, rust, 5 lines`). None for a rule.
pub fn block_name(block: &Block) -> Option<String> {
    let spoken = spoken_block(block)?;
    Some(match block {
        Block::Paragraph(_) => format!("paragraph, {spoken}"),
        Block::Heading { level, .. } => format!("heading level {level}, {spoken}"),
        Block::List { items, .. } => format!("list, {} items, {spoken}", items.len()),
        Block::Quote(_) => format!("quote, {spoken}"),
        Block::Code { .. } | Block::Table { .. } | Block::Image { .. } | Block::Rule => spoken,
    })
}

/// Leaves as one text to be read aloud: a line break between blocks and
/// rows, a comma between a row's cells.
pub fn spoken(leaves: &[Leaf]) -> String {
    let mut out = String::new();
    for (index, leaf) in leaves.iter().enumerate() {
        if index > 0 {
            out.push_str(match leaf.boundary {
                Boundary::Line => "\n",
                Boundary::Tab => ", ",
            });
        }
        out.push_str(&leaf.text);
    }
    out
}

/// The selectable pieces of blocks in reading order.
pub fn leaves(blocks: &[Block]) -> Vec<Leaf> {
    let mut out = Vec::new();
    collect(blocks, &mut out);
    out
}

fn collect(blocks: &[Block], out: &mut Vec<Leaf>) {
    let line = |text: &str| Leaf {
        boundary: Boundary::Line,
        text: text.to_string(),
    };
    for block in blocks {
        match block {
            Block::Paragraph(text) | Block::Heading { text, .. } => out.push(line(&text.text)),
            Block::Code { text, .. } => out.push(line(text.trim_end_matches('\n'))),
            Block::List { items, .. } => {
                for item in items {
                    collect(&item.blocks, out);
                }
            }
            Block::Quote(inner) => collect(inner, out),
            Block::Table { head, rows, .. } => {
                for row in std::iter::once(head).chain(rows) {
                    for (column, cell) in row.iter().enumerate() {
                        out.push(Leaf {
                            boundary: if column == 0 {
                                Boundary::Line
                            } else {
                                Boundary::Tab
                            },
                            text: cell.text.clone(),
                        });
                    }
                }
            }
            Block::Rule | Block::Image { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_block_kind_parses() {
        let blocks = parse(
            "# Title\n\nSome **bold** and *it* and ~~gone~~ with `code` and [a link](https://example.com).\n\n1. one\n   - nested\n2. two\n\n```rust\nfn main() {}\n```\n\n> quoted\n\n| A | B |\n|:--|--:|\n| 1 | 2 |\n\n---\n\n![alt text](https://example.com/a.png)\n",
        );
        assert!(matches!(&blocks[0], Block::Heading { level: 1, text } if text.text == "Title"));
        let Block::Paragraph(p) = &blocks[1] else {
            panic!("a paragraph")
        };
        assert_eq!(p.text, "Some bold and it and gone with code and a link.");
        assert!(
            p.runs
                .iter()
                .any(|r| r.strong && &p.text[r.range.clone()] == "bold")
        );
        assert!(
            p.runs
                .iter()
                .any(|r| r.emphasis && &p.text[r.range.clone()] == "it")
        );
        assert!(
            p.runs
                .iter()
                .any(|r| r.strike && &p.text[r.range.clone()] == "gone")
        );
        assert!(
            p.runs
                .iter()
                .any(|r| r.code && &p.text[r.range.clone()] == "code")
        );
        assert_eq!(p.links[0].url, "https://example.com");
        assert_eq!(&p.text[p.links[0].range.clone()], "a link");
        let Block::List {
            ordered: true,
            items,
            ..
        } = &blocks[2]
        else {
            panic!("a list")
        };
        assert!(matches!(
            &items[0].blocks[1],
            Block::List { ordered: false, .. }
        ));
        assert!(
            matches!(&blocks[3], Block::Code { lang: Some(l), text } if l == "rust" && text == "fn main() {}")
        );
        assert!(matches!(&blocks[4], Block::Quote(_)));
        let Block::Table { align, head, rows } = &blocks[5] else {
            panic!("a table")
        };
        assert_eq!(align, &[Align::Left, Align::Right]);
        assert_eq!(head.len(), 2);
        assert_eq!(rows[0][1].text, "2");
        assert_eq!(blocks[6], Block::Rule);
        assert!(matches!(&blocks[7], Block::Image { alt, .. } if alt == "alt text"));
    }

    #[test]
    fn leaves_put_cells_on_tabbed_lines() {
        let blocks = parse("| A | B |\n|---|---|\n| 1 | 2 |\n\nafter\n");
        let text: String = leaves(&blocks)
            .iter()
            .map(|l| {
                format!(
                    "{}{}",
                    if l.boundary == Boundary::Tab {
                        "\t"
                    } else {
                        "\n"
                    },
                    l.text
                )
            })
            .collect();
        assert_eq!(text, "\nA\tB\n1\t2\nafter");
    }
}
