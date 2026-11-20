//! A provider's Models section in Settings (`ModelsSection`,
//! `web/app.js:731-875`; inventory `components.md` E.5): the filter, the
//! hide or show of every match, the count, the first 150 rows, and where a
//! model lands when it is moved by its grip (`place`) or dragged.

use std::collections::HashSet;

use super::models::{display_name, order_models};
use crate::ModelInfo;

/// `MODEL_ROWS`: the rows drawn; the rest are reached by the filter.
pub const MODEL_ROWS: usize = 150;

/// What the section shows of a catalogue under a filter.
#[derive(Clone, Debug, PartialEq)]
pub struct Section {
    /// The catalogue in its saved order (`orderModels`).
    pub ordered: Vec<ModelInfo>,
    /// The indices into `ordered` that match the filter.
    pub matches: Vec<usize>,
    /// Every match is hidden (the link then shows them).
    pub all_matches_hidden: bool,
}

/// The section of `catalogue` in `order` under `query` (trimmed, any case,
/// a substring of the name or the id).
pub fn section(
    catalogue: &[ModelInfo],
    order: &[String],
    hidden: &[String],
    query: &str,
) -> Section {
    let ordered = order_models(catalogue, order);
    let needle = query.trim().to_lowercase();
    let matches: Vec<usize> = ordered
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            needle.is_empty()
                || m.name.to_lowercase().contains(&needle)
                || m.id.to_lowercase().contains(&needle)
        })
        .map(|(i, _)| i)
        .collect();
    let hidden: HashSet<&str> = hidden.iter().map(String::as_str).collect();
    let all_matches_hidden = !matches.is_empty()
        && matches
            .iter()
            .all(|i| hidden.contains(ordered[*i].id.as_str()));
    Section {
        ordered,
        matches,
        all_matches_hidden,
    }
}

impl Section {
    /// The rows drawn: the first [`MODEL_ROWS`] matches.
    pub fn shown(&self) -> Vec<&ModelInfo> {
        self.matches
            .iter()
            .take(MODEL_ROWS)
            .map(|i| &self.ordered[*i])
            .collect()
    }

    /// The link's text: `Hide all`, `Show all`, `Hide matches`, `Show
    /// matches`.
    pub fn link(&self, query: &str) -> String {
        format!(
            "{} {}",
            if self.all_matches_hidden {
                "Show"
            } else {
                "Hide"
            },
            if query.trim().is_empty() {
                "all"
            } else {
                "matches"
            }
        )
    }

    /// The hidden models after the link's press: every match hidden, or
    /// shown when all of them were hidden.
    pub fn toggle_matches(&self, hidden: &[String]) -> Vec<String> {
        let ids: Vec<String> = self
            .matches
            .iter()
            .map(|i| self.ordered[*i].id.clone())
            .collect();
        set_hidden(hidden, &ids, !self.all_matches_hidden)
    }

    /// `<visible> of <total> shown`, over the whole catalogue.
    pub fn count(&self, hidden: &[String]) -> String {
        let hidden: HashSet<&str> = hidden.iter().map(String::as_str).collect();
        let visible = self
            .ordered
            .iter()
            .filter(|m| !hidden.contains(m.id.as_str()))
            .count();
        format!("{visible} of {} shown", self.ordered.len())
    }

    /// `<n> more. Filter to find them.` past the rows drawn.
    pub fn more(&self) -> Option<String> {
        (self.matches.len() > MODEL_ROWS).then(|| {
            format!(
                "{} more. Filter to find them.",
                self.matches.len() - MODEL_ROWS
            )
        })
    }

    /// `place(modelId, to)`: the full order with `model` put before the
    /// model now shown at row `to` (among the rows shown without it), or
    /// right after the last shown row when `to` is past them; models
    /// filtered out keep their places.
    pub fn place(&self, model: &str, to: usize) -> Vec<String> {
        let others: Vec<&str> = self
            .shown()
            .into_iter()
            .map(|m| m.id.as_str())
            .filter(|id| *id != model)
            .collect();
        let mut ids: Vec<String> = self
            .ordered
            .iter()
            .map(|m| m.id.clone())
            .filter(|id| id != model)
            .collect();
        let index_of = |id: &str| {
            ids.iter()
                .position(|i| i == id)
                .unwrap_or_else(|| panic!("{id} is in the order"))
        };
        let at = if to < others.len() {
            index_of(others[to])
        } else {
            index_of(others[others.len() - 1]) + 1
        };
        ids.insert(at, model.to_string());
        ids
    }
}

/// The hidden models with `ids` hidden (`hide`) or shown, in order of
/// first hiding.
pub fn set_hidden(hidden: &[String], ids: &[String], hide: bool) -> Vec<String> {
    let mut next: Vec<String> = hidden.to_vec();
    for id in ids {
        if hide {
            if !next.contains(id) {
                next.push(id.clone());
            }
        } else {
            next.retain(|h| h != id);
        }
    }
    next
}

/// A model's row name (the vendor prefix dropped).
pub fn row_name(model: &ModelInfo) -> &str {
    display_name(&model.name)
}

/// The grip's name.
pub fn grip_name(model: &ModelInfo) -> String {
    format!(
        "Reorder {}: drag, or use the up and down arrow keys",
        model.name
    )
}

/// The toggle's name.
pub fn toggle_name(model: &ModelInfo) -> String {
    format!("Show {} in the model picker", model.name)
}

/// A drag by a grip: its row, the row pitch, how far it moved.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drag {
    /// The row it started on.
    pub from: usize,
    /// The rows drawn.
    pub rows: usize,
    /// The row pitch in pixels.
    pub height: f32,
    /// How far the pointer moved, clamped to the first and last row.
    pub dy: f32,
}

impl Drag {
    /// A drag from row `from` of `rows`, `height` apart, at `dy` from its
    /// start (clamped to the list).
    pub fn at(from: usize, rows: usize, height: f32, dy: f32) -> Drag {
        let min = -(from as f32) * height;
        let max = (rows.saturating_sub(1) - from) as f32 * height;
        Drag {
            from,
            rows,
            height,
            dy: dy.clamp(min, max),
        }
    }

    /// The row it would land on.
    pub fn to(&self) -> usize {
        (self.from as f32 + (self.dy / self.height).round()) as usize
    }

    /// Where `row` sits while dragging: the dragged row under the pointer,
    /// the rows between it and its landing row one row towards its start.
    pub fn offset(&self, row: usize) -> f32 {
        let to = self.to();
        if row == self.from {
            self.dy
        } else if self.from < to && row > self.from && row <= to {
            -self.height
        } else if self.from > to && row >= to && row < self.from {
            self.height
        } else {
            0.
        }
    }
}

/// What a move by keys says, for the polite region: the model's new
/// position among the rows shown.
pub fn moved_line(model: &ModelInfo, row: usize, rows: usize) -> String {
    format!("{} moved to {} of {rows}", model.name, row + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelFacts;

    fn model(id: &str, name: &str) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: name.into(),
            created: 0.,
            facts: ModelFacts::unknown(),
        }
    }

    fn catalogue() -> Vec<ModelInfo> {
        vec![
            model("a/one", "Vendor: One"),
            model("a/two", "Vendor: Two"),
            model("b/three", "Other: Three"),
            model("b/four", "Other: Four"),
        ]
    }

    #[test]
    fn the_filter_matches_the_name_or_the_id_and_the_link_hides_or_shows_the_matches() {
        let s = section(&catalogue(), &[], &[], "");
        assert_eq!(s.matches.len(), 4);
        assert_eq!(s.link(""), "Hide all");
        assert_eq!(s.count(&[]), "4 of 4 shown");
        let s = section(&catalogue(), &[], &[], "  OTHER ");
        assert_eq!(
            s.shown().iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            ["b/three", "b/four"]
        );
        assert_eq!(s.link(" other"), "Hide matches");
        let hidden = s.toggle_matches(&["a/one".into()]);
        assert_eq!(hidden, ["a/one", "b/three", "b/four"]);
        let s = section(&catalogue(), &[], &hidden, "other");
        assert!(s.all_matches_hidden);
        assert_eq!(s.link("other"), "Show matches");
        assert_eq!(s.toggle_matches(&hidden), ["a/one"]);
        assert_eq!(s.count(&hidden), "1 of 4 shown");
        assert_eq!(
            section(&catalogue(), &[], &[], "zzz").link("zzz"),
            "Hide matches"
        );
    }

    #[test]
    fn only_the_first_150_matches_are_drawn_and_the_rest_are_counted() {
        let many: Vec<ModelInfo> = (0..300)
            .map(|n| model(&format!("v/m{n}"), &format!("M {n}")))
            .collect();
        let s = section(&many, &[], &[], "");
        assert_eq!(s.shown().len(), MODEL_ROWS);
        assert_eq!(s.more().as_deref(), Some("150 more. Filter to find them."));
        assert_eq!(section(&many, &[], &[], "m 29").more(), None);
    }

    #[test]
    fn a_model_is_placed_among_the_rows_shown_and_the_filtered_out_keep_their_places() {
        let s = section(&catalogue(), &[], &[], "");
        assert_eq!(s.place("a/one", 2), ["a/two", "b/three", "a/one", "b/four"]);
        assert_eq!(
            s.place("b/four", 0),
            ["b/four", "a/one", "a/two", "b/three"]
        );
        assert_eq!(s.place("a/two", 3), ["a/one", "b/three", "b/four", "a/two"]);
        // Filtered to `Other`: moving Four above Three keeps One and Two.
        let s = section(&catalogue(), &[], &[], "other");
        assert_eq!(
            s.place("b/four", 0),
            ["a/one", "a/two", "b/four", "b/three"]
        );
        // The saved order is the start.
        let order = vec!["b/four".to_string()];
        let s = section(&catalogue(), &order, &[], "");
        assert_eq!(s.ordered[0].id, "b/four");
    }

    #[test]
    fn a_drag_lands_on_the_nearest_row_and_slides_the_rows_it_passes() {
        let d = Drag::at(1, 4, 40., 75.);
        assert_eq!(d.to(), 3);
        assert_eq!(d.offset(1), 75.);
        assert_eq!(Drag::at(1, 4, 40., 95.).dy, 80., "clamped to the last row");
        assert_eq!(d.offset(2), -40.);
        assert_eq!(d.offset(3), -40.);
        assert_eq!(d.offset(0), 0.);
        let up = Drag::at(2, 4, 40., -500.);
        assert_eq!(up.dy, -80.);
        assert_eq!(up.to(), 0);
        assert_eq!(up.offset(0), 40.);
        assert_eq!(up.offset(1), 40.);
        assert_eq!(up.offset(3), 0.);
    }
}
