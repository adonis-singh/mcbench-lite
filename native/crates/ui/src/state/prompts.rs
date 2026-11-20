//! The prompt library's rules with no GPUI types (inventory `components.md`
//! I.1 to I.8, `app.md` 6.5, 6.11 and 8.4; `web/app.js` 1704-1751,
//! 5646-5680, 5739-5806): a prompt's fill-ins and how it arrives in the
//! composer (`web/composer.mjs` `FILL_IN`, `promptSegments`, `promptLinks`,
//! tests ported from `test/composer.test.js`, same cases and names), a
//! tag's colour, the palette's sections, the list's filter, the `Added by`
//! mark, a prompt saved from the composer, tags typed in the editor, and a
//! reorder by drag.

use crate::{LibraryPrompt, PromptLink};

/// A part of the composer's content (`composer.mjs`'s segments).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    /// `{ type: 'text', text }`.
    Text(String),
    /// `{ type: 'prompt', id, title, text }`: a prompt token, `/` and its
    /// title, sending its text.
    Prompt {
        /// The prompt's id.
        id: String,
        /// Its title.
        title: String,
        /// Its text.
        text: String,
    },
    /// `{ type: 'field', name, value, from }`: a fill-in box.
    Field {
        /// The fill-in's name (its placeholder), trimmed.
        name: String,
        /// What was typed in it.
        value: String,
        /// The prompt it came from.
        from: PromptLink,
    },
}

/// The fill-ins of `text` (`FILL_IN`, `\{\{([^{}\n]*)\}\}`, global): each
/// match's byte range and the text between its braces, untrimmed.
pub fn fill_ins(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let pattern = regex::Regex::new(r"\{\{([^{}\n]*)\}\}").expect("the fill-in pattern");
    pattern
        .captures_iter(text)
        .map(|c| {
            let whole = c.get(0).expect("a match");
            (whole.range(), c.get(1).expect("the name group").as_str())
        })
        .collect()
}

/// A prompt as the composer takes it (`promptSegments`): without fill-ins
/// one token; with them, its text with a box in place of each.
pub fn prompt_segments(prompt: &LibraryPrompt) -> Vec<Segment> {
    let text = &prompt.text;
    let found = fill_ins(text);
    if found.is_empty() {
        return vec![Segment::Prompt {
            id: prompt.id.clone(),
            title: prompt.title.clone(),
            text: text.clone(),
        }];
    }
    let from = PromptLink {
        id: prompt.id.clone(),
        title: prompt.title.clone(),
    };
    let mut segments = Vec::new();
    let mut last = 0;
    for (range, name) in found {
        if range.start > last {
            segments.push(Segment::Text(text[last..range.start].to_string()));
        }
        segments.push(Segment::Field {
            name: name.trim().to_string(),
            value: String::new(),
            from: from.clone(),
        });
        last = range.end;
    }
    if last < text.len() {
        segments.push(Segment::Text(text[last..].to_string()));
    }
    segments
}

/// The library prompts a message holding `segments` was inserted from, in
/// the order they appear, each once (`promptLinks`): a prompt counts while
/// its token, or any of its boxes, is still in the message.
pub fn prompt_links(segments: &[Segment]) -> Vec<PromptLink> {
    let mut links: Vec<PromptLink> = Vec::new();
    for segment in segments {
        let link = match segment {
            Segment::Prompt { id, title, .. } => Some(PromptLink {
                id: id.clone(),
                title: title.clone(),
            }),
            Segment::Field { from, .. } => Some(from.clone()),
            Segment::Text(_) => None,
        };
        if let Some(link) = link
            && !links.iter().any(|l| l.id == link.id)
        {
            links.push(link);
        }
    }
    links
}

/// The text a send carries (`composer.read`'s `text`): tokens expanded to
/// their text, boxes to their values.
pub fn sent_text(segments: &[Segment]) -> String {
    segments
        .iter()
        .map(|s| match s {
            Segment::Text(text) => text.as_str(),
            Segment::Prompt { text, .. } => text.as_str(),
            Segment::Field { value, .. } => value.as_str(),
        })
        .collect()
}

/// The first box left blank (`emptyField`), which holds a send back.
pub fn empty_field(segments: &[Segment]) -> Option<usize> {
    segments
        .iter()
        .position(|s| matches!(s, Segment::Field { value, .. } if value.trim().is_empty()))
}

/// A prompt token's text: `/` and its title, or `Untitled prompt`.
pub fn title_or_untitled(title: &str) -> &str {
    if title.is_empty() { UNTITLED } else { title }
}

/// A prompt with no title.
pub const UNTITLED: &str = "Untitled prompt";

/// How many tag colours there are (`TAG_COLORS`, `view::theme::colour::tag`).
pub const TAG_COLORS: u32 = 8;

/// A tag's colour (`tagColor`), as its index in `TAG_COLORS`: a stable hash
/// of its name, so a tag is the same colour everywhere without storing
/// anything.
pub fn tag_color(tag: &str) -> usize {
    let mut hash: u32 = 0;
    for ch in tag.chars() {
        hash = hash.wrapping_mul(31).wrapping_add(ch as u32);
    }
    (hash % TAG_COLORS) as usize
}

/// Whether a prompt matches a query (title, text or a tag, case-insensitive;
/// the query trimmed and lowercased).
pub fn matches(prompt: &LibraryPrompt, query: &str) -> bool {
    let needle = query.trim().to_lowercase();
    needle.is_empty()
        || prompt.title.to_lowercase().contains(&needle)
        || prompt.text.to_lowercase().contains(&needle)
        || prompt
            .tags
            .iter()
            .any(|t| t.to_lowercase().contains(&needle))
}

/// The palette's sections (`promptSections`): `Favorites`, then the five
/// most recently used, then the rest in library order, labelled `All`
/// when there is a query or an earlier section, else unlabelled; empty
/// ones dropped. Each holds indices into `prompts`.
pub fn prompt_sections(prompts: &[LibraryPrompt], query: &str) -> Vec<(&'static str, Vec<usize>)> {
    let needle = query.trim();
    let matching: Vec<usize> = (0..prompts.len())
        .filter(|&i| matches(&prompts[i], query))
        .collect();
    let favorites: Vec<usize> = matching
        .iter()
        .copied()
        .filter(|&i| prompts[i].favorite)
        .collect();
    let mut recent: Vec<usize> = matching
        .iter()
        .copied()
        .filter(|&i| !prompts[i].favorite && prompts[i].used_at.is_some())
        .collect();
    // A stable sort, newest first, as `Array.prototype.sort`.
    recent.sort_by(|&a, &b| prompts[b].used_at.cmp(&prompts[a].used_at));
    recent.truncate(5);
    let rest: Vec<usize> = matching
        .iter()
        .copied()
        .filter(|i| !favorites.contains(i) && !recent.contains(i))
        .collect();
    let all = if !needle.is_empty() || !favorites.is_empty() || !recent.is_empty() {
        "All"
    } else {
        ""
    };
    [("Favorites", favorites), ("Recent", recent), (all, rest)]
        .into_iter()
        .filter(|(_, list)| !list.is_empty())
        .collect()
}

/// The sidebar list's filter (`PromptList`): the query over title, text and
/// tags, and when tags are chosen, a prompt with any of them.
pub fn list_shows(prompt: &LibraryPrompt, query: &str, tags: &[String]) -> bool {
    matches(prompt, query) && (tags.is_empty() || prompt.tags.iter().any(|t| tags.contains(t)))
}

/// Every tag any prompt has, in alphabetical order (the tag menu).
pub fn all_tags(prompts: &[LibraryPrompt]) -> Vec<String> {
    let mut tags: Vec<String> = prompts.iter().flat_map(|p| p.tags.clone()).collect();
    tags.sort();
    tags.dedup();
    tags
}

/// `AddedByMark`'s label: `Added by <model after its last slash> in a chat`.
pub fn added_by_label(model: &str) -> String {
    format!("Added by {} in a chat", super::models::model_label(model))
}

/// `PromptText`'s pieces: the text, with each fill-in as a mark of the name
/// inside its braces (untrimmed), or `…` for `{{}}`. `true` marks a piece.
pub fn text_marks(text: &str) -> Vec<(String, bool)> {
    let mut parts = Vec::new();
    let mut last = 0;
    for (range, name) in fill_ins(text) {
        parts.push((text[last..range.start].to_string(), false));
        let shown = if name.is_empty() { "\u{2026}" } else { name };
        parts.push((shown.to_string(), true));
        last = range.end;
    }
    parts.push((text[last..].to_string(), false));
    parts
}

/// The title of a prompt saved from the composer (`saveDraftAsPrompt`): the
/// text's first line, cut to 47 characters and `…` when longer than 48.
pub fn draft_title(text: &str) -> String {
    let first = text.trim().split('\n').next().unwrap_or("");
    // `String.length` counts UTF-16 units.
    let units: Vec<u16> = first.encode_utf16().collect();
    if units.len() > 48 {
        format!("{}\u{2026}", String::from_utf16_lossy(&units[..47]))
    } else {
        first.to_string()
    }
}

/// A tag typed in the editor (Enter, a comma, leaving the field): trimmed
/// and lower case; none when empty or already on the prompt.
pub fn new_tag(tags: &[String], typed: &str) -> Option<String> {
    let tag = typed.trim().to_lowercase();
    (!tag.is_empty() && !tags.contains(&tag)).then_some(tag)
}

/// The tag field's suggestions (`knownTags`, the field's `datalist`):
/// every tag the prompts have that the shown one does not, in the order
/// they first appear.
pub fn known_tags(prompts: &[LibraryPrompt], own: &[String]) -> Vec<String> {
    let mut known: Vec<String> = Vec::new();
    for tag in prompts.iter().flat_map(|p| &p.tags) {
        if !own.contains(tag) && !known.contains(tag) {
            known.push(tag.clone());
        }
    }
    known
}

/// The suggestions a browser's list offers for what is typed: those that
/// hold it, ignoring case, other than one it already is; none for an empty
/// field.
pub fn suggested_tags(known: &[String], typed: &str) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return Vec::new();
    }
    known
        .iter()
        .filter(|t| t.to_lowercase().contains(&typed) && t.to_lowercase() != typed)
        .cloned()
        .collect()
}

/// Typing a comma in the tag field: the first comma is removed and the text
/// before and after it is the tag.
pub fn comma_tag(typed: &str) -> Option<String> {
    typed
        .find(',')
        .map(|at| format!("{}{}", &typed[..at], &typed[at + 1..]))
}

/// `reorderPrompts(from, to)`: the prompt at `from` moves to where the one at
/// `to` is, by ids (after it when moving down, before it when moving up).
pub fn reorder(prompts: &[LibraryPrompt], from: usize, to: usize) -> Vec<LibraryPrompt> {
    if from == to || from >= prompts.len() || to >= prompts.len() {
        return prompts.to_vec();
    }
    let moved = prompts[from].clone();
    let target = prompts[to].id.clone();
    let mut list: Vec<LibraryPrompt> = prompts
        .iter()
        .filter(|p| p.id != moved.id)
        .cloned()
        .collect();
    let at = list
        .iter()
        .position(|p| p.id == target)
        .expect("the target stays in the list");
    list.insert(if from < to { at + 1 } else { at }, moved);
    list
}

/// How long a dropped row settles into its gap before the order is
/// committed, and how long the other rows take to slide (`150ms`).
pub const SETTLE: std::time::Duration = std::time::Duration::from_millis(150);
/// How far a press moves before it is a drag.
pub const DRAG_SLOP: f32 = 4.;
/// A drag this close to the list's top or bottom edge scrolls it ...
pub const EDGE: f32 = 28.;
/// ... by this much at each pointer move.
pub const EDGE_STEP: f32 = 8.;

/// A press on a prompt row that may become a drag (`PromptList`'s `lift`,
/// `follow` and `drop`): the row follows the pointer within the list, the
/// rows it passes give way once its leading edge passes their middle, and
/// a drop settles it into its gap.
#[derive(Clone, Debug, PartialEq)]
pub struct PromptDrag {
    /// The row pressed.
    pub from: usize,
    /// Where the press was.
    pub x: f32,
    /// Where the press was.
    pub y: f32,
    /// Moved past the slop: a drag.
    pub started: bool,
    /// The rows' tops and heights when the drag started.
    pub tops: Vec<f32>,
    /// The rows' heights.
    pub heights: Vec<f32>,
    /// The list's scroll (`scrollTop`) when the drag started.
    pub scroll: f32,
    /// How far the row is moved.
    pub dy: f32,
    /// The row it would land on.
    pub to: usize,
    /// Dropped: settling into its gap.
    pub dropping: bool,
}

impl PromptDrag {
    /// A press on row `from` at (`x`, `y`).
    pub fn press(from: usize, x: f32, y: f32) -> PromptDrag {
        PromptDrag {
            from,
            x,
            y,
            started: false,
            tops: Vec::new(),
            heights: Vec::new(),
            scroll: 0.,
            dy: 0.,
            to: from,
            dropping: false,
        }
    }

    /// Whether a pointer at (`x`, `y`) has moved past the slop.
    pub fn past_slop(&self, x: f32, y: f32) -> bool {
        (x - self.x).hypot(y - self.y) >= DRAG_SLOP
    }

    /// The drag starts over rows at `tops` with `heights`, the list
    /// scrolled to `scroll`.
    pub fn start(&mut self, tops: Vec<f32>, heights: Vec<f32>, scroll: f32) {
        assert_eq!(tops.len(), heights.len(), "a height for each row's top");
        assert!(self.from < tops.len(), "the row pressed is one of the rows");
        self.started = true;
        self.tops = tops;
        self.heights = heights;
        self.scroll = scroll;
    }

    /// The pointer at `y` with the list scrolled to `scroll`: the row moves
    /// with it within the list, and lands where its leading edge passed the
    /// middle of the rows it went over.
    pub fn follow(&mut self, y: f32, scroll: f32) {
        let (tops, heights, from) = (&self.tops, &self.heights, self.from);
        let last = tops.len() - 1;
        self.dy = (y - self.y + scroll - self.scroll)
            .max(tops[0] - tops[from])
            .min(tops[last] + heights[last] - tops[from] - heights[from]);
        let top = tops[from] + self.dy;
        let below = (from + 1..tops.len())
            .filter(|&j| top + heights[from] > tops[j] + heights[j] / 2.)
            .count();
        let above = (0..from)
            .filter(|&j| top < tops[j] + heights[j] / 2.)
            .count();
        self.to = from + below - above;
    }

    /// The room a row takes: its height and the gap after it.
    pub fn slot(&self) -> f32 {
        let gap = if self.tops.len() > 1 {
            self.tops[1] - self.tops[0] - self.heights[0]
        } else {
            0.
        };
        self.heights[self.from] + gap
    }

    /// Where row `index` is drawn, from its place.
    pub fn offset(&self, index: usize) -> f32 {
        let (from, to) = (self.from, self.to);
        if index == from {
            self.dy
        } else if from < to && index > from && index <= to {
            -self.slot()
        } else if to < from && index >= to && index < from {
            self.slot()
        } else {
            0.
        }
    }

    /// The release: a drag settles into the gap the others opened (its
    /// order is committed after [`SETTLE`]); a press that never moved is
    /// no drag (returns false).
    pub fn drop(&mut self) -> bool {
        if !self.started {
            return false;
        }
        let (tops, heights, from, to) = (&self.tops, &self.heights, self.from, self.to);
        self.dy = if to > from {
            tops[to] + heights[to] - tops[from] - heights[from]
        } else {
            tops[to] - tops[from]
        };
        self.dropping = true;
        true
    }
}

/// How far a drag at `y` scrolls a list from `top` to `bottom`: up or down
/// by [`EDGE_STEP`] within [`EDGE`] of an edge.
pub fn edge_scroll(y: f32, top: f32, bottom: f32) -> f32 {
    if y < top + EDGE {
        -EDGE_STEP
    } else if y > bottom - EDGE {
        EDGE_STEP
    } else {
        0.
    }
}

/// The rows' slide, `cubic-bezier(0.2, 0.7, 0.2, 1)` at `t` (0 to 1).
pub fn slide_ease(t: f32) -> f32 {
    let (x1, y1, x2, y2) = (0.2_f32, 0.7_f32, 0.2_f32, 1.0_f32);
    let at = |a: f32, b: f32, s: f32| {
        3. * (1. - s) * (1. - s) * s * a + 3. * (1. - s) * s * s * b + s * s * s
    };
    let slope = |a: f32, b: f32, s: f32| {
        3. * (1. - s) * (1. - s) * a + 6. * (1. - s) * s * (b - a) + 3. * s * s * (1. - b)
    };
    let t = t.clamp(0., 1.);
    let mut s = t;
    for _ in 0..8 {
        let d = slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s = (s - (at(x1, x2, s) - t) / d).clamp(0., 1.);
    }
    at(y1, y2, s)
}

/// A new prompt (`createPrompt`): empty, with this id.
pub fn blank(id: String) -> LibraryPrompt {
    LibraryPrompt {
        id,
        title: String::new(),
        text: String::new(),
        tags: Vec::new(),
        favorite: false,
        attachments: Vec::new(),
        used_at: None,
        added_by: None,
    }
}

#[cfg(test)]
pub(crate) fn prompt(id: &str, title: &str, text: &str) -> LibraryPrompt {
    LibraryPrompt {
        title: title.into(),
        text: text.into(),
        ..blank(id.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(id: &str, title: &str) -> PromptLink {
        PromptLink {
            id: id.into(),
            title: title.into(),
        }
    }

    fn text(t: &str) -> Segment {
        Segment::Text(t.into())
    }

    #[test]
    fn a_prompt_arrives_as_a_token_or_as_its_text_with_boxes_that_name_the_prompt_they_came_from() {
        let castle = prompt("castle", "Castle", "Build a castle.");
        let tower = prompt("tower", "Tower", "Build a {{size}} tower lit by {{light}}.");
        assert_eq!(
            prompt_segments(&castle),
            vec![Segment::Prompt {
                id: "castle".into(),
                title: "Castle".into(),
                text: "Build a castle.".into()
            }]
        );
        let from = link("tower", "Tower");
        let field = |name: &str| Segment::Field {
            name: name.into(),
            value: String::new(),
            from: from.clone(),
        };
        assert_eq!(
            prompt_segments(&tower),
            vec![
                text("Build a "),
                field("size"),
                text(" tower lit by "),
                field("light"),
                text(".")
            ]
        );
    }

    #[test]
    fn a_message_names_each_prompt_it_was_inserted_from_once_while_its_token_or_any_of_its_boxes_is_in_it()
     {
        let castle = prompt("castle", "Castle", "Build a castle.");
        let tower = prompt("tower", "Tower", "Build a {{size}} tower lit by {{light}}.");
        let segments = prompt_segments(&tower);
        let with = |s: &Segment, v: &str| match s {
            Segment::Field { name, from, .. } => Segment::Field {
                name: name.clone(),
                value: v.into(),
                from: from.clone(),
            },
            _ => panic!("a field"),
        };
        let (size, light) = (&segments[1], &segments[3]);
        assert_eq!(prompt_links(&[]), vec![]);
        assert_eq!(prompt_links(&[text("Build a hut")]), vec![]);
        // The prompt's text was edited around its boxes, and one box was
        // deleted.
        let edited = vec![
            text("Please build a "),
            with(size, "huge"),
            text(" tower, lit well"),
        ];
        assert_eq!(prompt_links(&edited), vec![link("tower", "Tower")]);
        let mut all = prompt_segments(&castle);
        all.extend(edited.clone());
        all.push(with(light, "torches"));
        all.extend(prompt_segments(&castle));
        assert_eq!(
            prompt_links(&all),
            vec![link("castle", "Castle"), link("tower", "Tower")]
        );
        assert_eq!(
            prompt_links(&[text("Build a huge tower lit by torches.")]),
            vec![],
            "its text alone, every box gone, is no longer the prompt"
        );
    }

    #[test]
    fn a_tag_has_the_same_colour_everywhere() {
        // h = h * 31 + code point, unsigned: "a" is 97, 97 % 8 = 1.
        assert_eq!(tag_color("a"), 1);
        assert_eq!(tag_color("terrain"), tag_color("terrain"));
        assert_eq!(tag_color(""), 0);
    }

    #[test]
    fn the_palette_lists_favorites_then_five_recent_then_the_rest() {
        let mut list: Vec<LibraryPrompt> = (0..9)
            .map(|i| prompt(&format!("p{i}"), &format!("Prompt {i}"), "text"))
            .collect();
        list[8].favorite = true;
        for (i, at) in [(1, 10), (2, 50), (3, 20), (4, 40), (5, 30), (6, 60)] {
            list[i].used_at = Some(at);
        }
        let sections = prompt_sections(&list, "");
        assert_eq!(
            sections,
            vec![
                ("Favorites", vec![8]),
                ("Recent", vec![6, 2, 4, 5, 3]),
                ("All", vec![0, 1, 7])
            ]
        );
        let plain: Vec<LibraryPrompt> = list.iter().take(1).cloned().collect();
        assert_eq!(prompt_sections(&plain, ""), vec![("", vec![0])]);
        assert_eq!(prompt_sections(&plain, "prompt"), vec![("All", vec![0])]);
        assert!(prompt_sections(&plain, "nothing").is_empty());
    }

    #[test]
    fn a_fill_in_shows_its_name_and_an_empty_one_an_ellipsis() {
        assert_eq!(
            text_marks("A {{ size }} hut {{}}."),
            vec![
                ("A ".to_string(), false),
                (" size ".to_string(), true),
                (" hut ".to_string(), false),
                ("\u{2026}".to_string(), true),
                (".".to_string(), false)
            ]
        );
        assert_eq!(fill_ins("{{{a}}}").len(), 1);
        assert_eq!(fill_ins("{{a\nb}}").len(), 0);
    }

    #[test]
    fn a_prompt_saved_from_the_composer_is_titled_by_its_first_line() {
        assert_eq!(draft_title("A hut\nwith a door"), "A hut");
        let long = "x".repeat(49);
        assert_eq!(draft_title(&long), format!("{}\u{2026}", "x".repeat(47)));
        assert_eq!(draft_title(&"y".repeat(48)), "y".repeat(48));
    }

    #[test]
    fn a_prompt_moves_to_where_the_one_it_was_dropped_on_is() {
        let list: Vec<LibraryPrompt> = ["a", "b", "c", "d"]
            .iter()
            .map(|id| prompt(id, id, ""))
            .collect();
        let ids = |l: Vec<LibraryPrompt>| l.into_iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(reorder(&list, 0, 2)), ["b", "c", "a", "d"]);
        assert_eq!(ids(reorder(&list, 3, 1)), ["a", "d", "b", "c"]);
    }

    #[test]
    fn a_tag_is_trimmed_lower_case_and_once() {
        let tags = vec!["farm".to_string()];
        assert_eq!(new_tag(&tags, " Castle "), Some("castle".into()));
        assert_eq!(new_tag(&tags, "FARM"), None);
        assert_eq!(new_tag(&tags, "  "), None);
        assert_eq!(comma_tag("stone,wall"), Some("stonewall".into()));
        assert_eq!(comma_tag("stone"), None);
    }

    #[test]
    fn the_tag_field_suggests_the_other_prompts_tags_that_hold_what_is_typed() {
        let mut a = prompt("a", "a", "");
        a.tags = vec!["castle".into(), "stone".into()];
        let mut b = prompt("b", "b", "");
        b.tags = vec!["farm".into(), "castle".into(), "Moat".into()];
        let known = known_tags(&[a, b], &["stone".to_string()]);
        assert_eq!(known, ["castle", "farm", "Moat"]);
        assert_eq!(suggested_tags(&known, "a"), ["castle", "farm", "Moat"]);
        assert_eq!(suggested_tags(&known, "MO"), ["Moat"]);
        assert_eq!(suggested_tags(&known, "castle"), Vec::<String>::new());
        assert_eq!(suggested_tags(&known, " "), Vec::<String>::new());
    }

    fn rows(n: usize) -> (Vec<f32>, Vec<f32>) {
        // 34 px rows 2 px apart, as `.pe-list` lays them out.
        ((0..n).map(|i| i as f32 * 36.).collect(), vec![34.; n])
    }

    #[test]
    fn a_press_is_a_drag_past_four_pixels() {
        let d = PromptDrag::press(1, 10., 100.);
        assert!(!d.past_slop(12., 102.));
        assert!(d.past_slop(10., 104.));
        let mut d = d;
        assert!(!d.drop(), "a press that never moved is a click");
    }

    #[test]
    fn a_row_gives_way_once_the_lifted_rows_leading_edge_passes_its_middle() {
        let (tops, heights) = rows(4);
        let mut d = PromptDrag::press(0, 0., 100.);
        d.start(tops, heights, 0.);
        // Down 16: the bottom edge (50) is short of row 1's middle (53).
        d.follow(116., 0.);
        assert_eq!(d.to, 0);
        // Down 20: past it (54 > 53).
        d.follow(120., 0.);
        assert_eq!(d.to, 1);
        assert_eq!(d.offset(1), -36.);
        assert_eq!(d.offset(0), 20.);
        // Far below: clamped to the last row's bottom.
        d.follow(1000., 0.);
        assert_eq!((d.dy, d.to), (108., 3));
        // Up from the last row: its top passes row 2's middle (89).
        let (tops, heights) = rows(4);
        let mut d = PromptDrag::press(3, 0., 300.);
        d.start(tops, heights, 0.);
        d.follow(280., 0.);
        assert_eq!(d.to, 2);
        assert_eq!(d.offset(2), 36.);
    }

    #[test]
    fn a_scrolled_list_moves_the_row_with_it_and_a_drop_settles_into_the_gap() {
        let (tops, heights) = rows(6);
        let mut d = PromptDrag::press(1, 0., 100.);
        d.start(tops, heights, 0.);
        // The list scrolled 72 under a still pointer: two rows on.
        d.follow(100., 72.);
        assert_eq!((d.dy, d.to), (72., 3));
        assert!(d.drop());
        assert!(d.dropping);
        // Settled: its bottom where row 3's was.
        assert_eq!(d.dy, 72.);
        assert_eq!(edge_scroll(105., 100., 400.), -EDGE_STEP);
        assert_eq!(edge_scroll(380., 100., 400.), EDGE_STEP);
        assert_eq!(edge_scroll(250., 100., 400.), 0.);
        assert!((slide_ease(0.5) - 0.93).abs() < 0.03);
        assert_eq!(slide_ease(1.), 1.);
    }
}
