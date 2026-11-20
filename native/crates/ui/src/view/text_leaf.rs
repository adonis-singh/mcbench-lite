//! A piece of transcript text: a paragraph, heading, code block, cell or
//! message drawn with GPUI's `StyledText`, plus what the transcript needs
//! around it: the selection painted under it, its laid-out lines kept for
//! hit testing, focusable and clickable links, and its accessibility node
//! with the text as AccessKit text runs (one run per visual line segment,
//! links as `Link` nodes holding their own run), so a screen reader reads
//! it by character, word and line and draws its cursor at the right place.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui_kit::*;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::state::selection::Point as TextPoint;

/// Where each laid-out leaf is this frame, for hit testing a pointer.
#[derive(Default)]
pub struct HitMap {
    leaves: HashMap<(usize, usize), HitLeaf>,
}

/// A leaf's box, text and visual lines.
struct HitLeaf {
    bounds: Bounds<Pixels>,
    text: SharedString,
    lines: Rc<Vec<Segment>>,
    line_height: Pixels,
}

impl HitMap {
    /// Forgets the last frame's leaves; the screen calls this as it renders.
    pub fn clear(&mut self) {
        self.leaves.clear();
    }

    /// The text point under `position`: the leaf whose lines hold it, or
    /// the nearest leaf above or below in reading order; in it the line at
    /// that height (or the nearest), and on the line the nearest character
    /// boundary, as drawn (alignment and right-to-left runs included).
    pub fn point_at(&self, position: Point<Pixels>) -> Option<TextPoint> {
        let mut best: Option<((usize, usize), Pixels)> = None;
        for (key, leaf) in &self.leaves {
            let bounds = leaf.bounds;
            let distance = if position.y < bounds.top() {
                bounds.top() - position.y
            } else if position.y > bounds.bottom() {
                position.y - bounds.bottom()
            } else if position.x < bounds.left() {
                (bounds.left() - position.x) / 1000.
            } else if position.x > bounds.right() {
                (position.x - bounds.right()) / 1000.
            } else {
                px(0.)
            };
            if best.is_none_or(|(k, d)| distance < d || (distance == d && *key < k)) {
                best = Some((*key, distance));
            }
        }
        let ((row, leaf), _) = best?;
        let hit = &self.leaves[&(row, leaf)];
        let line = hit.lines.iter().min_by_key(|line| {
            let top = line.origin.y;
            let bottom = top + hit.line_height;
            let away = if position.y < top {
                top - position.y
            } else if position.y > bottom {
                position.y - bottom
            } else {
                px(0.)
            };
            (away / px(1.) * 1000.) as i64
        })?;
        let text = hit.text.as_ref();
        let boundaries = text[line.range.clone()]
            .grapheme_indices(true)
            .map(|(i, _)| line.range.start + i)
            .chain(std::iter::once(line.range.end));
        let offset = boundaries
            .min_by_key(|byte| {
                let x = line.origin.x + line.x_of(*byte);
                ((x - position.x).abs() / px(1.) * 1000.) as i64
            })
            .unwrap_or(line.range.start);
        Some(TextPoint { row, leaf, offset })
    }
}

/// A link inside a leaf.
pub struct LeafLink {
    /// Bytes of the leaf's text.
    pub range: Range<usize>,
    /// The address.
    pub url: SharedString,
    /// Its keyboard focus.
    pub focus: FocusHandle,
    /// Whether it is a tab stop now (its message is the current one).
    pub tab_stop: bool,
    /// What activating it does.
    pub on_activate: super::Press,
}

/// Accessibility properties of a leaf beyond its role.
#[derive(Clone, Default)]
pub struct LeafA11y {
    /// Its name (a heading's text, "Code, rust").
    pub label: Option<String>,
    /// A heading's level.
    pub level: Option<usize>,
    /// A cell's row and column.
    pub cell: Option<(usize, usize)>,
}

/// The element.
pub struct TextLeaf {
    id: ElementId,
    role: Role,
    a11y: LeafA11y,
    text: SharedString,
    styled: StyledText,
    links: Vec<LeafLink>,
    link_style: Rc<dyn Fn(Stateful<Div>) -> Stateful<Div>>,
    selection: Option<Range<usize>>,
    selection_color: Hsla,
    hits: Rc<RefCell<HitMap>>,
    key: (usize, usize),
}

impl TextLeaf {
    /// A leaf with `runs` (from [`crate::view::markdown`]) at `key` (row, leaf).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: impl Into<ElementId>,
        role: Role,
        a11y: LeafA11y,
        text: SharedString,
        runs: Vec<TextRun>,
        links: Vec<LeafLink>,
        link_style: Rc<dyn Fn(Stateful<Div>) -> Stateful<Div>>,
        selection: Option<Range<usize>>,
        selection_color: Hsla,
        hits: Rc<RefCell<HitMap>>,
        key: (usize, usize),
    ) -> Self {
        TextLeaf {
            id: id.into(),
            role,
            a11y,
            styled: StyledText::new(text.clone()).with_runs(runs),
            text,
            links,
            link_style,
            selection,
            selection_color,
            hits,
            key,
        }
    }
}

/// A visual line's segment: bytes of the text and where they sit.
#[derive(Clone)]
struct Segment {
    range: Range<usize>,
    origin: Point<Pixels>,
    width: Pixels,
    /// x of each character's start, relative to `origin`, and the end.
    xs: Vec<Pixels>,
    /// It ends a hard line that another follows: its text run keeps the
    /// line break, as AccessKit expects of runs.
    breaks: bool,
}

/// What prepaint learned and paint and the accessibility tree use.
pub struct LeafPrepaint {
    lines: Rc<Vec<Segment>>,
    line_height: Pixels,
    overlays: Vec<AnyElement>,
    link_nodes: Vec<accesskit::NodeId>,
    scale_factor: f32,
    focused_link: Option<usize>,
}

impl IntoElement for TextLeaf {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextLeaf {
    type RequestLayoutState = ();
    type PrepaintState = LeafPrepaint;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn a11y_role(&self) -> Option<Role> {
        Some(self.role)
    }

    fn write_a11y_info(&self, node: &mut accesskit::Node) {
        if let Some(label) = &self.a11y.label {
            node.set_label(label.clone());
        }
        if let Some(level) = self.a11y.level {
            node.set_level(level);
        }
        if let Some((row, column)) = self.a11y.cell {
            node.set_row_index(row);
            node.set_column_index(column);
        }
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        self.styled.request_layout(None, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> LeafPrepaint {
        self.styled
            .prepaint(None, inspector_id, bounds, state, window, cx);
        let layout = self.styled.layout().clone();
        let line_height = layout.line_height();
        let mut lines = segments(&self.text, &layout, bounds.origin, line_height);
        // GPUI draws a right or centred line shifted within the element's
        // width; the selection, the hit boxes and the accessibility boxes
        // follow it.
        let align = window.text_style().text_align;
        for segment in &mut lines {
            segment.origin.x += match align {
                TextAlign::Left => px(0.),
                TextAlign::Center => (bounds.size.width - segment.width) / 2.,
                TextAlign::Right => bounds.size.width - segment.width,
            };
        }
        let lines = Rc::new(lines);
        self.hits.borrow_mut().leaves.insert(
            self.key,
            HitLeaf {
                bounds,
                text: self.text.clone(),
                lines: lines.clone(),
                line_height,
            },
        );

        // One focusable box per link over its first line's segment; the
        // link's accessibility node is a synthetic child (below), so the box
        // itself has no role.
        let mut overlays = Vec::new();
        for (index, link) in self.links.iter().enumerate() {
            let Some((origin, width)) = first_box(&lines, &link.range) else {
                continue;
            };
            let on_activate = link.on_activate.clone();
            let on_key = link.on_activate.clone();
            let overlay = (self.link_style)(
                div()
                    .id(("link", index))
                    .w(width)
                    .h(line_height)
                    .cursor_pointer(),
            )
            .track_focus(&link.focus.clone().tab_stop(link.tab_stop))
            .key_context("Part")
            .on_action(move |_: &crate::view::Activate, window, cx| on_key(window, cx))
            .on_click(move |_, window, cx| on_activate(window, cx));
            let mut element = overlay.into_any_element();
            element.layout_as_root(size(width, line_height).into(), window, cx);
            element.prepaint_at(origin, window, cx);
            overlays.push(element);
        }
        LeafPrepaint {
            lines,
            line_height,
            overlays,
            link_nodes: Vec::new(),
            scale_factor: window.scale_factor(),
            focused_link: self.links.iter().position(|l| l.focus.is_focused(window)),
        }
    }

    fn a11y_synthetic_children(
        &mut self,
        prepaint: &mut LeafPrepaint,
        builder: &mut A11ySubtreeBuilder,
    ) {
        let text = self.text.as_ref();
        // Each segment is split at link edges; a piece outside links is a
        // run of the leaf, one inside a link a run of the link's node.
        struct Piece {
            range: Range<usize>,
            link: Option<usize>,
            segment: usize,
            rtl: bool,
        }
        let mut pieces: Vec<Piece> = Vec::new();
        for (segment_index, segment) in prepaint.lines.iter().enumerate() {
            let mut cuts = vec![segment.range.start, segment.range.end];
            for link in &self.links {
                for edge in [link.range.start, link.range.end] {
                    if edge > segment.range.start && edge < segment.range.end {
                        cuts.push(edge);
                    }
                }
            }
            cuts.sort_unstable();
            cuts.dedup();
            for pair in cuts.windows(2) {
                let range = pair[0]..pair[1];
                let link = self
                    .links
                    .iter()
                    .position(|l| l.range.start <= range.start && range.end <= l.range.end);
                // A run has one direction (a right-to-left script starts
                // a run, neutral characters stay with the run before) and
                // at most 255 characters (AccessKit's lists are u8).
                let mut start = range.start;
                let mut count = 0;
                let mut rtl = None;
                for (offset, grapheme) in text[range.clone()].grapheme_indices(true) {
                    let here = direction_of(grapheme).or(rtl);
                    if count == u8::MAX as usize || (count > 0 && rtl.is_some() && here != rtl) {
                        pieces.push(Piece {
                            range: start..range.start + offset,
                            link,
                            segment: segment_index,
                            rtl: rtl.unwrap_or(false),
                        });
                        start = range.start + offset;
                        count = 0;
                    }
                    rtl = here;
                    count += 1;
                }
                pieces.push(Piece {
                    range: start..range.end,
                    link,
                    segment: segment_index,
                    rtl: rtl.unwrap_or(false),
                });
            }
        }
        // A line break belongs to the run that ends its line.
        for (segment_index, segment) in prepaint.lines.iter().enumerate() {
            if segment.breaks {
                match pieces.iter_mut().rev().find(|p| p.segment == segment_index) {
                    Some(last) => last.range.end += 1,
                    None => pieces.push(Piece {
                        range: segment.range.end..segment.range.end + 1,
                        link: None,
                        segment: segment_index,
                        rtl: false,
                    }),
                }
            }
        }
        pieces.sort_by_key(|p| p.range.start);
        if pieces.is_empty() {
            pieces.push(Piece {
                range: 0..0,
                link: None,
                segment: usize::MAX,
                rtl: false,
            });
        }
        let link_ids: Vec<accesskit::NodeId> = (0..self.links.len())
            .map(|i| builder.synthetic_node_id(("link", i)))
            .collect();
        let run_ids: Vec<accesskit::NodeId> = (0..pieces.len())
            .map(|i| builder.synthetic_node_id(("run", i)))
            .collect();
        let scale = prepaint.scale_factor;
        let mut link_children: Vec<Vec<accesskit::NodeId>> = vec![Vec::new(); self.links.len()];
        let mut link_bounds: Vec<Option<accesskit::Rect>> = vec![None; self.links.len()];
        let mut order: Vec<accesskit::NodeId> = Vec::new();
        let mut runs: Vec<(usize, accesskit::Node)> = Vec::new();
        for (index, piece) in pieces.iter().enumerate() {
            let mut node = accesskit::Node::new(Role::TextRun);
            let slice = &text[piece.range.clone()];
            node.set_value(slice.to_string());
            let graphemes: Vec<(usize, &str)> = slice.grapheme_indices(true).collect();
            node.set_character_lengths(
                graphemes
                    .iter()
                    .map(|(_, g)| u8::try_from(g.len()).expect("a grapheme of at most 255 bytes"))
                    .collect::<Vec<u8>>(),
            );
            let word_starts: Vec<u8> = {
                let mut starts = Vec::new();
                let words: Vec<usize> = slice
                    .split_word_bound_indices()
                    .filter(|(_, w)| w.chars().any(char::is_alphanumeric))
                    .map(|(i, _)| i)
                    .collect();
                for (char_index, (byte, _)) in graphemes.iter().enumerate() {
                    if words.contains(byte) {
                        starts.push(char_index as u8);
                    }
                }
                starts
            };
            node.set_word_starts(word_starts);
            if let Some(segment) = prepaint.lines.get(piece.segment) {
                // Each character's box on its line, then the run's box
                // around them; positions count from the run's left edge
                // (from its right edge in a right-to-left run).
                let boxes: Vec<(Pixels, Pixels)> = graphemes
                    .iter()
                    .map(|(byte, g)| {
                        let a = segment.x_of(piece.range.start + byte);
                        let b = segment.x_of(piece.range.start + byte + g.len());
                        (a.min(b), (a - b).abs())
                    })
                    .collect();
                let left = boxes
                    .iter()
                    .map(|(x, _)| *x)
                    .fold(segment.x_of(piece.range.start), Pixels::min);
                let right = boxes
                    .iter()
                    .map(|(x, w)| *x + *w)
                    .fold(segment.x_of(piece.range.start), Pixels::max);
                node.set_character_positions(
                    boxes
                        .iter()
                        .map(|(x, w)| {
                            if piece.rtl {
                                f(right - (*x + *w)) * scale
                            } else {
                                f(*x - left) * scale
                            }
                        })
                        .collect::<Vec<f32>>(),
                );
                node.set_character_widths(
                    boxes
                        .iter()
                        .map(|(_, w)| f(*w) * scale)
                        .collect::<Vec<f32>>(),
                );
                node.set_text_direction(if piece.rtl {
                    accesskit::TextDirection::RightToLeft
                } else {
                    accesskit::TextDirection::LeftToRight
                });
                let rect = accesskit::Rect {
                    x0: (f(segment.origin.x + left) * scale) as f64,
                    y0: (f(segment.origin.y) * scale) as f64,
                    x1: (f(segment.origin.x + right) * scale) as f64,
                    y1: (f(segment.origin.y + prepaint.line_height) * scale) as f64,
                };
                node.set_bounds(rect);
                if let Some(link) = piece.link {
                    link_bounds[link] = Some(match link_bounds[link] {
                        Some(b) => b.union(rect),
                        None => rect,
                    });
                }
            }
            // Pieces of one visual line read as one line.
            if index > 0 && pieces[index - 1].segment == piece.segment {
                node.set_previous_on_line(run_ids[index - 1]);
            }
            if pieces
                .get(index + 1)
                .is_some_and(|next| next.segment == piece.segment)
            {
                node.set_next_on_line(run_ids[index + 1]);
            }
            match piece.link {
                Some(link) => {
                    if link_children[link].is_empty() {
                        order.push(link_ids[link]);
                    }
                    link_children[link].push(run_ids[index]);
                }
                None => order.push(run_ids[index]),
            }
            runs.push((index, node));
        }
        // Links first (with their runs as their children), then the leaf's
        // runs, then the children put in reading order.
        let mut pushed_links = vec![false; self.links.len()];
        for (index, node) in runs {
            match pieces[index].link {
                Some(link) => {
                    if !pushed_links[link] {
                        pushed_links[link] = true;
                        let mut link_node = accesskit::Node::new(Role::Link);
                        let range = self.links[link].range.clone();
                        link_node.set_label(text[range].to_string());
                        link_node.set_url(self.links[link].url.to_string());
                        link_node.add_action(accesskit::Action::Click);
                        link_node.add_action(accesskit::Action::Focus);
                        if let Some(bounds) = link_bounds[link] {
                            link_node.set_bounds(bounds);
                        }
                        builder.push_child(link_ids[link], link_node);
                    }
                    builder.push_descendant(link_ids[link], run_ids[index], node);
                }
                None => {
                    builder.push_child(run_ids[index], node);
                }
            }
        }
        builder.parent_node().set_children(order);
        if let Some(link) = prepaint.focused_link {
            builder.set_focus(link_ids[link]);
        }
        prepaint.link_nodes = link_ids;
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        prepaint: &mut LeafPrepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(selected) = &self.selection {
            for segment in prepaint.lines.iter() {
                let from = selected.start.max(segment.range.start);
                let to = selected.end.min(segment.range.end);
                if from >= to
                    && !(from == to
                        && selected.contains(&segment.range.end)
                        && segment.range.is_empty())
                {
                    continue;
                }
                let a = segment.x_of(from);
                let b = segment.x_of(to);
                let left = segment.origin.x + a.min(b);
                let width = (a - b).abs().max(px(3.));
                window.paint_quad(fill(
                    Bounds::new(
                        point(left, segment.origin.y),
                        size(width, prepaint.line_height),
                    ),
                    self.selection_color,
                ));
            }
        }
        self.styled
            .paint(None, inspector_id, bounds, state, &mut (), window, cx);
        for overlay in &mut prepaint.overlays {
            overlay.paint(window, cx);
        }
        for (index, link) in self.links.iter().enumerate() {
            let Some(node) = prepaint.link_nodes.get(index).copied() else {
                continue;
            };
            let on_activate = link.on_activate.clone();
            window.on_a11y_action(node, accesskit::Action::Click, move |_, window, cx| {
                on_activate(window, cx)
            });
            let focus = link.focus.clone();
            window.on_a11y_action(node, accesskit::Action::Focus, move |_, window, cx| {
                window.focus(&focus, cx)
            });
        }
    }
}

impl Segment {
    /// The x of byte `index` (clamped to the segment), relative to `origin`.
    fn x_of(&self, index: usize) -> Pixels {
        if index <= self.range.start {
            return px(0.);
        }
        if index >= self.range.end {
            return self.width;
        }
        // xs holds a value per byte offset from the segment's start.
        self.xs[index - self.range.start]
    }
}

/// The visual lines of a laid-out text: each hard line's wrapped segments.
fn segments(
    text: &str,
    layout: &TextLayout,
    origin: Point<Pixels>,
    line_height: Pixels,
) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut y = origin.y;
    let mut line_start = 0;
    for (hard, wrapped) in text.split('\n').zip(layout.line_layouts()) {
        let mut cuts: Vec<usize> = wrapped
            .wrap_boundaries()
            .iter()
            .map(|b| wrapped.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix].index)
            .collect();
        cuts.insert(0, 0);
        cuts.push(hard.len());
        for pair in cuts.windows(2) {
            let (start, end) = (pair[0], pair[1]);
            let base = wrapped.unwrapped_layout.x_for_index(start);
            let xs: Vec<Pixels> = (start..=end)
                .map(|i| wrapped.unwrapped_layout.x_for_index(i) - base)
                .collect();
            let width = *xs.last().expect("a segment has an end");
            out.push(Segment {
                range: line_start + start..line_start + end,
                origin: point(origin.x, y),
                width,
                xs,
                breaks: end == hard.len() && line_start + end < text.len(),
            });
            y += line_height;
        }
        line_start += hard.len() + 1;
    }
    out
}

/// Whether a character is of a right-to-left script (Hebrew, Arabic,
/// Syriac, Thaana, N'Ko and their presentation forms), left-to-right (a
/// letter of another script), or neutral (`None`: spaces, digits,
/// punctuation).
fn direction_of(grapheme: &str) -> Option<bool> {
    let c = grapheme.chars().next()?;
    let rtl = matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF);
    if rtl {
        Some(true)
    } else if c.is_alphabetic() {
        Some(false)
    } else {
        None
    }
}

/// Logical pixels as a number.
fn f(pixels: Pixels) -> f32 {
    pixels / px(1.)
}

/// A link's first box: the origin and width of its part on its first line.
fn first_box(lines: &[Segment], range: &Range<usize>) -> Option<(Point<Pixels>, Pixels)> {
    for segment in lines {
        let from = range.start.max(segment.range.start);
        let to = range.end.min(segment.range.end);
        if from < to {
            let a = segment.x_of(from);
            let b = segment.x_of(to);
            return Some((
                point(segment.origin.x + a.min(b), segment.origin.y),
                (a - b).abs(),
            ));
        }
    }
    None
}
