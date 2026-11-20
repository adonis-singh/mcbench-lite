//! A row's pieces drawn as GPUI elements: markdown blocks (headings at the
//! prose size, weight 620, as today; paragraphs keeping each newline;
//! lists, code with its language and a Copy button, quotes, tables, rules,
//! images), plain text, folds and tool rows. Every selectable leaf is a
//! [`TextLeaf`], counted in the same order as
//! [`crate::state::rows::Row::leaves`]; [`RowEnv::finish`] checks the count.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::icons::{self, Icon};
use super::text_leaf::{HitMap, LeafA11y, LeafLink, TextLeaf};
use super::{Handler, Press, colour};
use crate::state::code;
use crate::state::doc::{Align, Block, Inline};
use crate::state::rows::{Piece, tool_expandable};
use crate::state::selection::Selection;
use crate::state::transcript::{ToolStatus, diff_stat, tool_row_text, writing_lines};

/// The fonts of the transcript.
#[derive(Clone)]
pub struct Fonts {
    /// The prose face.
    pub ui: SharedString,
    /// The code face.
    pub mono: SharedString,
}

/// What a row's drawing needs from the screen.
pub struct RowEnv<'a> {
    /// The row's index.
    pub row: usize,
    /// The row's key.
    pub key: String,
    /// The leaves drawn so far.
    pub leaf: usize,
    /// The transcript's selection.
    pub selection: Option<Selection>,
    /// Where leaves are, for the pointer.
    pub hits: Rc<RefCell<HitMap>>,
    /// The faces.
    pub fonts: Fonts,
    /// Whether this row is the current message: its parts are tab stops.
    pub current: bool,
    /// The screen's focus handles of parts, by name.
    pub focus: &'a mut HashMap<String, FocusHandle>,
    /// The parts drawn so far.
    pub parts: usize,
    /// The top-level blocks drawn so far (each a stop for Left and Right).
    pub blocks: usize,
    /// In a streaming row, the first block still being written: from it on
    /// a block is named by its kind alone until it settles, so its name
    /// does not change with every token (open item b).
    pub unsettled_from: Option<usize>,
    /// Activates a link: logs it, and in the app opens it.
    pub on_link: Handler,
    /// Copies text to the clipboard and logs it.
    pub on_copy: Handler,
    /// Activates an image.
    pub on_image: Handler,
    /// The file to draw for a picture's path or address
    /// (`state::files::picture`), or the core's sentence.
    pub pictures: Pictures,
    /// Opens or closes a fold or a tool row by key.
    pub on_toggle: Handler,
    // par-chat: each open program's scroll, by its call's id.
    /// The scroll of each tool call's code box.
    pub code_scrolls: &'a mut HashMap<String, ScrollHandle>,
}

impl RowEnv<'_> {
    fn next_leaf(&mut self) -> usize {
        self.leaf += 1;
        self.leaf - 1
    }

    fn selected(&self, leaf: usize, len: usize) -> Option<Range<usize>> {
        self.selection?.range_in(self.row, leaf, len)
    }

    /// The focus handle of this row's next interactive part.
    fn part(&mut self, cx: &mut App) -> FocusHandle {
        let name = format!("{}/part{}", self.key, self.parts);
        self.parts += 1;
        let handle = self
            .focus
            .entry(name)
            .or_insert_with(|| cx.focus_handle())
            .clone();
        handle.tab_stop(self.current)
    }

    /// `element`, a top-level block of the row, as a stop of Left and
    /// Right: focusable (not a Tab stop) and named by its kind and its text
    /// (`doc::block_name`), so a screen reader reads it on its own, once.
    fn block_stop(&mut self, name: String, element: AnyElement, cx: &mut App) -> AnyElement {
        let index = self.blocks;
        self.blocks += 1;
        let name = match self.unsettled_from {
            Some(from) if index >= from => kind_of(&name).to_string(),
            _ => name,
        };
        let handle = self
            .focus
            .entry(block_focus_name(&self.key, index))
            .or_insert_with(|| cx.focus_handle())
            .clone()
            .tab_stop(false);
        super::ring(
            div()
                .id(("block-stop", index))
                .role(Role::Group)
                .aria_label(name)
                .track_focus(&handle),
        )
        .rounded(px(4.))
        .child(element)
        .into_any_element()
    }

    /// Checks that the leaves drawn are the row's leaves.
    pub fn finish(&self, expected: usize) {
        assert_eq!(
            self.leaf, expected,
            "row {} drew {} leaves where its model has {expected}",
            self.row, self.leaf
        );
    }
}

/// A block name's kind: its first part (`paragraph`, `heading level 1`,
/// `code`, `list`, `table`, `quote`, `image`).
fn kind_of(name: &str) -> &str {
    name.split(", ").next().unwrap_or(name)
}

/// Text size and line height of a register.
#[derive(Clone, Copy)]
pub struct Register {
    /// Size.
    pub size: Pixels,
    /// Line height.
    pub line: Pixels,
    /// Colour.
    pub colour: Hsla,
}

/// The prose register (`.msg`: 14px / 1.62, `--bone`).
pub fn prose() -> Register {
    Register {
        size: px(14.),
        line: px(14. * 1.62),
        colour: colour::bone(),
    }
}

/// The log note register (`.log-note`: 13.5px / 1.62, `--note-text`).
pub fn note() -> Register {
    Register {
        size: px(13.5),
        line: px(13.5 * 1.62),
        colour: colour::note(),
    }
}

fn font(family: &SharedString, weight: f32, italic: bool) -> Font {
    Font {
        family: family.clone(),
        features: FontFeatures::default(),
        fallbacks: None,
        weight: FontWeight(weight),
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
    }
}

/// The text runs of an inline at `weight` in `register`.
fn runs(inline: &Inline, fonts: &Fonts, weight: f32, register: Register) -> Vec<TextRun> {
    inline
        .runs
        .iter()
        .map(|run| TextRun {
            len: run.range.len(),
            font: font(
                if run.code { &fonts.mono } else { &fonts.ui },
                // `strong` is the browser's `bolder`.
                if run.strong { weight.max(700.) } else { weight },
                run.emphasis,
            ),
            color: if run.link.is_some() {
                colour::waiting()
            } else {
                register.colour
            },
            background_color: run.code.then(colour::well),
            underline: run.link.map(|_| UnderlineStyle {
                thickness: px(1.),
                color: Some(colour::waiting()),
                wavy: false,
            }),
            strikethrough: run.strike.then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(register.colour),
            }),
        })
        .collect()
}

fn plain_runs(text: &str, family: &SharedString, weight: f32, colour: Hsla) -> Vec<TextRun> {
    vec![TextRun {
        len: text.len(),
        font: font(family, weight, false),
        color: colour,
        background_color: None,
        underline: None,
        strikethrough: None,
    }]
}

fn link_style() -> Rc<dyn Fn(Stateful<Div>) -> Stateful<Div>> {
    Rc::new(|div: Stateful<Div>| {
        div.rounded(px(3.))
            .border_2()
            .border_color(colour::none())
            .focus_visible(|s| s.border_color(colour::gold()))
    })
}

/// A leaf of `inline` text with `role`.
fn inline_leaf(
    env: &mut RowEnv,
    role: Role,
    a11y: LeafA11y,
    inline: &Inline,
    weight: f32,
    register: Register,
    cx: &mut App,
) -> AnyElement {
    let leaf = env.next_leaf();
    let links = inline
        .links
        .iter()
        .map(|link| {
            let url = link.url.clone();
            let on_link = env.on_link.clone();
            LeafLink {
                range: link.range.clone(),
                url: link.url.clone().into(),
                focus: env.part(cx),
                tab_stop: env.current,
                on_activate: Rc::new(move |window, cx| on_link(url.clone(), window, cx)),
            }
        })
        .collect();
    let selection = env.selected(leaf, inline.text.len());
    TextLeaf::new(
        ("leaf", leaf),
        role,
        a11y,
        inline.text.clone().into(),
        runs(inline, &env.fonts, weight, register),
        links,
        link_style(),
        selection,
        colour::selection(),
        env.hits.clone(),
        (env.row, leaf),
    )
    .into_any_element()
}

/// A leaf of plain text.
fn text_leaf(
    env: &mut RowEnv,
    role: Role,
    label: Option<String>,
    text: &str,
    runs: Vec<TextRun>,
) -> AnyElement {
    let leaf = env.next_leaf();
    let selection = env.selected(leaf, text.len());
    TextLeaf::new(
        ("leaf", leaf),
        role,
        LeafA11y {
            label,
            ..LeafA11y::default()
        },
        text.to_string().into(),
        runs,
        Vec::new(),
        link_style(),
        selection,
        colour::selection(),
        env.hits.clone(),
        (env.row, leaf),
    )
    .into_any_element()
}

/// A button-like part: a focus stop with a ring, Enter and Space, a name.
fn part_button(
    env: &mut RowEnv,
    id: impl Into<ElementId>,
    label: String,
    on_press: Press,
    cx: &mut App,
) -> Stateful<Div> {
    let focus = env.part(cx);
    let on_key = on_press.clone();
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .track_focus(&focus)
        .key_context("Part")
        .on_action(move |_: &super::Activate, window, cx| on_key(window, cx))
        .on_click(move |_, window, cx| on_press(window, cx))
        .cursor_pointer()
        .border_2()
        .border_color(colour::none())
        .focus_visible(|s| s.border_color(colour::gold()))
}

/// A disclosure takes the readers' Expand and Collapse as well as Click
/// (UIA ExpandCollapse, AT-SPI's expand-or-collapse, `AXExpanded`): each
/// toggles only when it changes the state.
fn disclosure(div: Stateful<Div>, open: bool, on_toggle: &Handler, key: &str) -> Stateful<Div> {
    let expand = on_toggle.clone();
    let collapse = on_toggle.clone();
    let (key_expand, key_collapse) = (key.to_string(), key.to_string());
    div.on_a11y_action(accesskit::Action::Expand, move |_, window, cx| {
        if !open {
            expand(key_expand.clone(), window, cx)
        }
    })
    .on_a11y_action(accesskit::Action::Collapse, move |_, window, cx| {
        if open {
            collapse(key_collapse.clone(), window, cx)
        }
    })
}

/// The name of the focus handle of a row's top-level block `index`.
pub fn block_focus_name(key: &str, index: usize) -> String {
    format!("{key}/block{index}")
}

/// How a row's pieces are read aloud: its text and markdown blocks as
/// `doc::spoken_blocks` reads them, a line break between pieces; folds and
/// tool rows are not part of it (they are read as the buttons they are).
pub fn spoken_pieces(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Text(text) => Some(text.clone()),
            Piece::Markdown { blocks, .. } => Some(crate::state::doc::spoken_blocks(blocks)),
            Piece::Plan(tasks) => Some(crate::state::rows::plan_spoken(tasks)),
            Piece::Notice { level, text } => Some(crate::state::rows::notice_text(level, text)),
            _ => None,
        })
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// How many top-level blocks (stops of Left and Right) a row's pieces draw.
pub fn block_count(pieces: &[Piece]) -> usize {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(_) | Piece::Plan(_) | Piece::Notice { .. } => 1,
            Piece::Markdown { blocks, .. } => {
                blocks.iter().filter(|b| !matches!(b, Block::Rule)).count()
            }
            _ => 0,
        })
        .sum()
}

/// A row's top-level blocks of markdown, each a stop of Left and Right.
fn top_blocks(
    env: &mut RowEnv,
    parsed: &[Block],
    register: Register,
    cx: &mut App,
) -> Vec<AnyElement> {
    parsed
        .iter()
        .map(|block| {
            let element = block_element(env, block, register, cx);
            // A rule has nothing to read: no stop.
            match crate::state::doc::block_name(block) {
                Some(name) => env.block_stop(name, element, cx),
                None => element,
            }
        })
        .collect()
}

/// Blocks of markdown.
pub fn blocks(
    env: &mut RowEnv,
    blocks: &[Block],
    register: Register,
    cx: &mut App,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        out.push(
            div()
                .id(("block", index))
                .child(block_element(env, block, register, cx))
                .into_any_element(),
        );
    }
    out
}

fn block_element(env: &mut RowEnv, block: &Block, register: Register, cx: &mut App) -> AnyElement {
    match block {
        Block::Paragraph(inline) => inline_leaf(
            env,
            Role::Paragraph,
            LeafA11y::default(),
            inline,
            400.,
            register,
            cx,
        ),
        Block::Heading { level, text } => div()
            // `.md-h`: the prose size, weight 620, margin 10px 0 4px.
            .mt(px(10.))
            .mb(px(4.))
            .child(inline_leaf(
                env,
                Role::Heading,
                LeafA11y {
                    label: Some(text.text.clone()),
                    // AccessKit counts levels from 0; the adapters add 1.
                    level: Some(*level as usize - 1),
                    cell: None,
                },
                text,
                620.,
                register,
                cx,
            ))
            .into_any_element(),
        Block::List {
            ordered,
            start,
            items,
        } => {
            let count = items.len();
            let mut list = div()
                .id("list")
                .role(Role::List)
                .aria_size_of_set(count)
                .my(px(7.))
                .flex()
                .flex_col();
            for (index, item) in items.iter().enumerate() {
                let marker = match (ordered, item.checked) {
                    (_, Some(true)) => "\u{2611}".to_string(),
                    (_, Some(false)) => "\u{2610}".to_string(),
                    (true, None) => format!("{}.", *start as usize + index),
                    (false, None) => "\u{2022}".to_string(),
                };
                let children = blocks(env, &item.blocks, register, cx);
                list = list.child(
                    div()
                        .id(("item", index))
                        .role(Role::ListItem)
                        .aria_position_in_set(index)
                        .aria_size_of_set(count)
                        .my(px(3.))
                        .flex()
                        .child(
                            div()
                                .w(px(20.))
                                .flex_none()
                                .text_color(register.colour)
                                .child(marker),
                        )
                        .child(div().flex_1().min_w_0().children(children)),
                );
            }
            list.into_any_element()
        }
        Block::Code { lang, text } => {
            let code = text.trim_end_matches('\n');
            let label = match lang {
                Some(lang) => format!("Code, {lang}"),
                None => "Code".to_string(),
            };
            let mono = env.fonts.mono.clone();
            let copy_text = code.to_string();
            let on_copy = env.on_copy.clone();
            let copy = part_button(
                env,
                "copy",
                "Copy code".to_string(),
                Rc::new(move |window, cx| on_copy(copy_text.clone(), window, cx)),
                cx,
            )
            .px(px(6.))
            .rounded(px(6.))
            .text_size(px(11.5))
            .text_color(colour::dim())
            .hover(|s| s.text_color(colour::bone()))
            .child("Copy");
            let runs = code_runs(code, &mono, colour::ash());
            div()
                .my(px(6.))
                .rounded(px(8.))
                .bg(colour::well())
                .border_1()
                .border_color(colour::line())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px(px(10.))
                        .pt(px(4.))
                        .text_size(px(11.5))
                        .text_color(colour::dim())
                        .child(lang.clone().unwrap_or_default())
                        .child(copy),
                )
                .child(
                    div()
                        .px(px(10.))
                        .pb(px(8.))
                        .text_size(px(12.))
                        .line_height(px(12. * 1.65))
                        .child(text_leaf(env, Role::Code, Some(label), code, runs)),
                )
                .into_any_element()
        }
        Block::Quote(inner) => {
            let children = blocks(env, inner, register, cx);
            div()
                .id("quote")
                .role(Role::Blockquote)
                .my(px(6.))
                .pl(px(12.))
                .border_l_2()
                .border_color(colour::line())
                .text_color(colour::ash())
                .children(children)
                .into_any_element()
        }
        Block::Table { align, head, rows } => {
            let columns = head.len().max(rows.iter().map(Vec::len).max().unwrap_or(0));
            let mut table = div()
                .id("table")
                .role(Role::Table)
                .aria_row_count(rows.len() + 1)
                .aria_column_count(columns)
                .my(px(6.))
                .flex()
                .flex_col()
                .border_1()
                .border_color(colour::line())
                .rounded(px(8.));
            for (row_index, cells) in std::iter::once(head).chain(rows).enumerate() {
                let header = row_index == 0;
                let mut row = div()
                    .id(("row", row_index))
                    .role(Role::Row)
                    .aria_row_index(row_index)
                    .flex()
                    .when(header, |d| d.bg(colour::raised()))
                    .when(!header, |d| d.border_t_1().border_color(colour::line()));
                for (column, cell) in cells.iter().enumerate() {
                    let alignment = align.get(column).copied().unwrap_or(Align::None);
                    let leaf = inline_leaf(
                        env,
                        if header {
                            Role::ColumnHeader
                        } else {
                            Role::Cell
                        },
                        LeafA11y {
                            label: None,
                            level: None,
                            cell: Some((row_index, column)),
                        },
                        cell,
                        if header { 620. } else { 400. },
                        register,
                        cx,
                    );
                    row = row.child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .px(px(10.))
                            .py(px(4.))
                            .map(|d| match alignment {
                                Align::Right => d.text_right(),
                                Align::Center => d.text_center(),
                                Align::Left | Align::None => d,
                            })
                            .child(leaf),
                    );
                }
                table = table.child(row);
            }
            table.into_any_element()
        }
        Block::Rule => div()
            .my(px(10.))
            .h(px(1.))
            .bg(colour::line())
            .into_any_element(),
        Block::Image { alt, url } => {
            let on_image = env.on_image.clone();
            let target = url.clone();
            let image = part_button(
                env,
                "image",
                alt.clone(),
                Rc::new(move |window, cx| on_image(target.clone(), window, cx)),
                cx,
            )
            .role(Role::Image)
            .flex_none()
            .rounded(px(8.))
            .max_w(px(420.))
            .child(match (env.pictures)(std::path::Path::new(url.as_str())) {
                Ok(file) => img(file)
                    .max_w(px(416.))
                    .max_h(px(260.))
                    .rounded(px(6.))
                    .into_any_element(),
                Err(sentence) => missing_picture(sentence),
            });
            // In a row of its own the image's node is as wide as the picture
            // drawn (it stretched to the message's width; open item c).
            div()
                .my(px(6.))
                .flex()
                .items_start()
                .child(image)
                .into_any_element()
        }
    }
}

/// What draws a picture: its file, or the core's sentence.
pub type Pictures = Rc<dyn Fn(&std::path::Path) -> Result<std::path::PathBuf, String>>;

/// The core's sentence in place of a picture it could not give
/// (`Image not found`).
fn missing_picture(sentence: String) -> AnyElement {
    div()
        .px(px(10.))
        .py(px(8.))
        .rounded(px(6.))
        .border_1()
        .border_color(colour::error_border())
        .text_color(colour::err())
        .text_size(px(12.))
        .child(sentence)
        .into_any_element()
}

/// Code coloured as today's page colours it (`.code`, `--tk-*`).
pub fn code_runs(code: &str, mono: &SharedString, base: Hsla) -> Vec<TextRun> {
    let mut out = Vec::new();
    let mut at = 0;
    let run = |len: usize, colour: Hsla| TextRun {
        len,
        font: font(mono, 400., false),
        color: colour,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    for (range, kind) in code::tokens(code) {
        if range.start > at {
            out.push(run(range.start - at, base));
        }
        out.push(run(range.len(), colour::token(kind)));
        at = range.end;
    }
    if at < code.len() || out.is_empty() {
        out.push(run(code.len() - at, base));
    }
    out
}

/// The chevron of a fold: `›` turned when open.
fn chevron(open: bool) -> &'static str {
    if open { "\u{2304}" } else { "\u{203a}" }
}

/// A row's pieces.
pub fn pieces(
    env: &mut RowEnv,
    pieces: &[Piece],
    register: Register,
    cx: &mut App,
) -> Vec<AnyElement> {
    let mut out = Vec::new();
    // The name of the last tool row, which labels its open details.
    let mut tool_name = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        if let Piece::Tool { call, .. } = piece {
            let (action, detail) = tool_row_text(call);
            tool_name = if detail.is_empty() {
                action
            } else {
                format!("{action}: {detail}")
            };
        }
        let element = match piece {
            Piece::Text(text) => {
                let ui = env.fonts.ui.clone();
                let leaf = text_leaf(
                    env,
                    Role::Paragraph,
                    None,
                    text,
                    plain_runs(text, &ui, 400., register.colour),
                );
                env.block_stop(format!("paragraph, {text}"), leaf, cx)
            }
            Piece::Markdown {
                blocks: parsed,
                note: is_note,
            } => {
                let register = if *is_note { note() } else { register };
                div()
                    .text_size(register.size)
                    .line_height(register.line)
                    .text_color(register.colour)
                    .when(*is_note, |d| d.my(px(7.)))
                    .children(top_blocks(env, parsed, register, cx))
                    .into_any_element()
            }
            Piece::Fold {
                key,
                label,
                open,
                title,
            } => {
                let on_toggle = env.on_toggle.clone();
                let fold = key.clone();
                let summary = part_button(
                    env,
                    ("fold", index),
                    label.clone(),
                    Rc::new(move |window, cx| on_toggle(fold.clone(), window, cx)),
                    cx,
                )
                .aria_expanded(*open)
                .map(|d| disclosure(d, *open, &env.on_toggle, key))
                .flex()
                .items_center()
                .gap(px(4.))
                .px(px(4.))
                .min_h(px(20.))
                .rounded(px(6.))
                .text_size(px(12.))
                .text_color(colour::dim())
                .hover(|s| s.text_color(colour::bone()))
                .child(label.clone())
                .child(div().child(chevron(*open)));
                // par-chat: the summary's `title` as its tip.
                match title {
                    Some(title) => super::controls::tip(summary, ("fold-tip", index), *title)
                        .into_any_element(),
                    None => summary.into_any_element(),
                }
            }
            Piece::Tool { call, open } => tool_row(env, index, call, *open, cx),
            Piece::Plan(tasks) => {
                let card = plan_card(env, tasks);
                env.block_stop(crate::state::rows::plan_spoken(tasks), card, cx)
            }
            Piece::Notice { level, text } => {
                let line = notice_line(env, level, text);
                env.block_stop(crate::state::rows::notice_text(level, text), line, cx)
            }
            // A user message's files, selections and tools are the
            // transcript's (`view::chat::message`).
            Piece::Files(_) | Piece::Selections(_) | Piece::Tools(_) => continue,
            Piece::Code {
                call,
                text,
                writing,
                awaiting,
            } => {
                let mono = env.fonts.mono.clone();
                // par-chat: `.code-awaiting` is the body face, italic, at
                // `--dim-text` 72%.
                let runs = if *awaiting {
                    let mut runs =
                        plain_runs(text, &env.fonts.ui, 400., colour::dim().opacity(0.72));
                    runs[0].font.style = FontStyle::Italic;
                    runs
                } else {
                    code_runs(text, &mono, colour::dim())
                };
                // par-chat: `pre.code` scrolls in at most 256 px, unwrapped
                // (`white-space: pre`, `overflow: auto`), and while its call
                // is written it follows its bottom when within 40 px of it
                // (`web/app.js` 6028-6033).
                let scroll = env.code_scrolls.entry(call.clone()).or_default().clone();
                if *writing {
                    follow_bottom(&scroll);
                }
                div()
                    .id(("details", index))
                    .role(Role::Group)
                    .aria_label(format!("{tool_name}, code"))
                    .ml(px(28.))
                    .mt(px(4.))
                    .mb(px(3.))
                    .py(px(2.))
                    .max_h(px(CODE_MAX))
                    .overflow_scroll()
                    .track_scroll(&scroll)
                    .flex()
                    .flex_col()
                    .items_start()
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .line_height(px(11. * 1.65))
                    .child(div().flex_none().child(text_leaf(
                        env,
                        Role::Code,
                        Some("Code".to_string()),
                        text,
                        runs,
                    )))
                    .into_any_element()
            }
            Piece::Output(output) => {
                let mono = env.fonts.mono.clone();
                div()
                    .id(("details", index))
                    .role(Role::Group)
                    .aria_label(format!("{tool_name}, output"))
                    // par-chat: `pre.code.term`, as `.code` with 220 px.
                    .ml(px(28.))
                    .mt(px(4.))
                    .mb(px(3.))
                    .py(px(2.))
                    .max_h(px(TERM_MAX))
                    .overflow_scroll()
                    .flex()
                    .flex_col()
                    .items_start()
                    .whitespace_nowrap()
                    .text_size(px(11.))
                    .line_height(px(11. * 1.65))
                    .child(div().flex_none().child(text_leaf(
                        env,
                        Role::Code,
                        Some("Output".to_string()),
                        output,
                        plain_runs(output, &mono, 400., colour::term()),
                    )))
                    .into_any_element()
            }
            Piece::Shots(images) => {
                let mut strip = div().flex().flex_wrap().gap(px(6.));
                for (n, path) in images.iter().enumerate() {
                    let on_image = env.on_image.clone();
                    // The log names the picture; the element takes the path.
                    let target = path.display().to_string();
                    let file = (env.pictures)(path);
                    // A picture the core could not give is named by its
                    // sentence too, so a reader hears why it is missing.
                    let name = match &file {
                        Ok(_) => format!("Picture {}", n + 1),
                        Err(sentence) => format!("Picture {}, {sentence}", n + 1),
                    };
                    strip = strip.child(
                        part_button(
                            env,
                            ("shot", n),
                            name,
                            Rc::new(move |window, cx| on_image(target.clone(), window, cx)),
                            cx,
                        )
                        .role(Role::Image)
                        .rounded(px(8.))
                        .child(match file {
                            Ok(file) => img(file).h(px(88.)).rounded(px(6.)).into_any_element(),
                            Err(sentence) => missing_picture(sentence),
                        }),
                    );
                }
                strip.into_any_element()
            }
        };
        out.push(element);
    }
    out
}

// par-chat (stage four's parity pass)
/// `.code`'s `max-height`.
const CODE_MAX: f32 = 256.;
/// `.code.term`'s `max-height`.
const TERM_MAX: f32 = 220.;
/// How near its bottom a streaming program's box still follows it.
const CODE_STICK: f32 = 40.;

/// The page's ref on a streaming `pre.code`: it was within 40 px of its
/// last height's bottom, so it goes to the new bottom (`scrollTop +
/// clientHeight >= prevH - 40`; GPUI's handle holds the last layout's
/// height, the page's `prevH`). A box never laid out (height 0) follows.
fn follow_bottom(scroll: &ScrollHandle) {
    let client = scroll.bounds().size.height;
    let top = -scroll.offset().y;
    let previous = scroll.max_offset().y + client;
    if top + client >= previous - px(CODE_STICK) {
        scroll.scroll_to_bottom();
    }
}

/// The summary's `title="Show code"` on a part of an open-able tool row
/// that is not its detail (the detail's own title is the innermost one a
/// browser shows there).
fn show_code(element: Div, expandable: bool, id: (&'static str, usize)) -> AnyElement {
    if !expandable {
        return element.into_any_element();
    }
    super::controls::pointer_tip(element.id(id), (id.0, id.1 + 1_000_000), "Show code")
        .into_any_element()
}

/// A tool row (`.log-step`): icon column, the action and its detail, the
/// diff, a failure mark; a disclosure when it has code to show.
fn tool_row(
    env: &mut RowEnv,
    index: usize,
    call: &crate::state::transcript::ToolCall,
    open: bool,
    cx: &mut App,
) -> AnyElement {
    let (action, detail) = tool_row_text(call);
    let expandable = tool_expandable(call);
    let name = if detail.is_empty() {
        action.clone()
    } else {
        format!("{action}: {detail}")
    };
    // par-chat: the page's icons (`toolIcon`), 14 px in `.tool-kind-icon`.
    let icon = match call.tool.as_str() {
        "bash" => Icon::Terminal,
        "build" => Icon::Hammer,
        "see_build" | "view_image" => Icon::Eye,
        _ => Icon::Wrench,
    };
    let mono = env.fonts.mono.clone();
    let on_toggle = env.on_toggle.clone();
    let key = call.call_id.clone();
    let mut row = part_button(
        env,
        ("tool", index),
        name,
        Rc::new(move |window, cx| {
            if expandable {
                on_toggle(key.clone(), window, cx)
            }
        }),
        cx,
    )
    .when(expandable, |d| {
        disclosure(d.aria_expanded(open), open, &env.on_toggle, &call.call_id)
    })
    .group("log-step")
    .w_full()
    .min_h(px(24.))
    .flex()
    .items_center()
    .gap(px(8.))
    .rounded(px(6.))
    .text_size(px(12.))
    .line_height(px(20.))
    // `.log-code > .log-step:hover`: only a row that opens.
    .when(expandable, |d| d.hover(|s| s.bg(colour::faint())))
    .child(
        div()
            .w(px(20.))
            .h(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .text_color(colour::dim())
            .opacity(0.8)
            .child(icons::icon(icon, 14.))
            .map(|d| show_code(d, expandable, ("tool-icon", index))),
    )
    .child(
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_wrap()
            .gap(px(6.))
            .child(
                // `.log-step .s`; `.log-code > summary:hover .s` and
                // `.log-code[open] > summary .s` are `--bone`.
                div()
                    .font_weight(FontWeight(550.))
                    .whitespace_nowrap()
                    .text_color(if expandable && open {
                        colour::bone()
                    } else {
                        colour::bone_82()
                    })
                    .when(expandable, |d| {
                        d.group_hover("log-step", |s| s.text_color(colour::bone()))
                    })
                    .child(action)
                    .map(|d| show_code(d, expandable, ("tool-action", index))),
            )
            .when(!detail.is_empty(), |d| {
                // `.log-step .d` with its `title` (the detail itself).
                let shown = detail.clone();
                d.child(super::controls::pointer_tip(
                    div()
                        .id(("tool-detail", index))
                        .min_w_0()
                        .text_color(colour::dim_82())
                        .child(shown),
                    ("tool-detail-tip", index),
                    detail.clone(),
                ))
            })
            .when_some(writing_lines(call), |d, lines| {
                d.child(
                    div()
                        .flex_none()
                        .font_family(mono.clone())
                        .text_size(px(10.))
                        .text_color(colour::dim())
                        .child(format!("+{lines} lines")),
                )
            })
            .when_some(
                call.diff
                    .map(|d| diff_stat(&d))
                    .filter(|(p, m)| *p > 0 || *m > 0),
                |d, (plus, minus)| {
                    // `.diffstat`: the data face at 10 px, `--diff-plus` and
                    // `--redstone`, side by side.
                    d.child(
                        div()
                            .flex_none()
                            .flex()
                            .px(px(2.))
                            .font_family(mono.clone())
                            .text_size(px(10.))
                            .when(plus > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(colour::diff_plus())
                                        .child(format!("+{plus}")),
                                )
                            })
                            .when(minus > 0, |d| {
                                d.child(
                                    div()
                                        .text_color(colour::redstone())
                                        .child(format!("-{minus}")),
                                )
                            }),
                    )
                },
            ),
    )
    .child(
        // `.tool-actions` with `.tool-failure`'s 12 px x.
        div()
            .w(px(20.))
            .min_h(px(24.))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .opacity(0.72)
            .when(call.status == ToolStatus::Failed, |d| {
                d.child(
                    div()
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_color(colour::err_soft())
                        .child(icons::icon(Icon::X, 12.)),
                )
            }),
    );
    if call.status == ToolStatus::Failed && !call.detail.is_empty() {
        row = row.aria_description(call.detail.clone());
    }
    let mut column = div().w_full().flex().flex_col().child(row);
    if call.status == ToolStatus::Failed && !call.detail.is_empty() {
        let mono = env.fonts.mono.clone();
        column = column.child(
            div()
                .mx(px(28.))
                .mt(px(1.))
                .mb(px(4.))
                .text_size(px(11.))
                .line_height(px(16.5))
                .child(text_leaf(
                    env,
                    Role::Paragraph,
                    None,
                    &call.detail,
                    plain_runs(&call.detail, &mono, 400., colour::err_soft()),
                )),
        );
    }
    column.into_any_element()
}

// chat (stage four's chat agent): the plan card and the notice line.

/// `PlanCard`: `Tasks` and `<done> of <total>`, then each task with its
/// mark (a check when done, a ring, a ring with a dot while active).
fn plan_card(env: &mut RowEnv, tasks: &[(String, String)]) -> AnyElement {
    let done = tasks.iter().filter(|(s, _)| s == "done").count();
    let ui = env.fonts.ui.clone();
    let mut list = div().flex().flex_col().gap(px(6.));
    for (status, text) in tasks {
        let is_done = status == "done";
        let mark = div()
            .size(px(14.))
            .flex_none()
            .rounded_full()
            .flex()
            .items_center()
            .justify_center()
            .border(px(1.5))
            .border_color(if status == "active" {
                colour::bone()
            } else {
                colour::dim()
            })
            .when(is_done, |d| {
                d.bg(colour::ash())
                    .border_color(colour::ash())
                    .text_color(colour::bedrock())
                    .child(super::icons::icon(super::icons::Icon::Check, 11.))
            })
            .when(status == "active", |d| {
                d.child(div().size(px(5.)).rounded_full().bg(colour::bone()))
            });
        let colour_of = if is_done {
            colour::dim()
        } else {
            colour::bone()
        };
        let mut runs = plain_runs(text, &ui, 400., colour_of);
        if is_done {
            runs[0].strikethrough = Some(StrikethroughStyle {
                thickness: px(1.),
                color: Some(colour::dim_60()),
            });
        }
        let leaf = text_leaf(env, Role::Paragraph, None, text, runs);
        list = list.child(
            div()
                .flex()
                .items_start()
                .gap(px(9.))
                .text_size(px(13.))
                .line_height(px(13. * 1.45))
                .child(div().pt(px(2.5)).child(mark))
                .child(div().flex_1().min_w_0().child(leaf)),
        );
    }
    div()
        .my(px(6.))
        .px(px(14.))
        .py(px(12.))
        .max_w(px(560.))
        .rounded(px(10.))
        .bg(colour::well())
        .border_1()
        .border_color(colour::line())
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .flex()
                .justify_between()
                .text_size(px(12.))
                .font_weight(FontWeight(500.))
                .text_color(colour::ash())
                .child("Tasks")
                .child(
                    div()
                        .text_color(colour::dim())
                        .child(format!("{done} of {}", tasks.len())),
                ),
        )
        .child(list)
        .into_any_element()
}

/// `NoticeLine`: a dot in the level's colour (`--dim-text`, `--star` for a
/// warning, `--redstone` for an error) and the text.
fn notice_line(env: &mut RowEnv, level: &str, text: &str) -> AnyElement {
    let ui = env.fonts.ui.clone();
    let leaf = text_leaf(
        env,
        Role::Paragraph,
        None,
        text,
        plain_runs(text, &ui, 400., colour::ash()),
    );
    let dot = match level {
        "warning" => colour::star(),
        "error" => colour::redstone(),
        _ => colour::dim(),
    };
    div()
        .my(px(4.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(12.5))
        .line_height(px(12.5 * 1.5))
        .child(div().size(px(6.)).flex_none().rounded_full().bg(dot))
        .child(div().flex_1().min_w_0().child(leaf))
        .into_any_element()
}
