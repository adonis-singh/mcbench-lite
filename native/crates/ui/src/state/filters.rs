//! The sidebar's filters (`web/filters.mjs`, tests ported from
//! `test/filters.test.js`, same cases and names; inventory `app.md` 4.10,
//! `components.md` J). Chats (Build) or benches (Bench) narrowed by size,
//! by the library prompt they came from, and by model, beside the search
//! text. Each group holds the values chosen in it; a row is listed when it
//! matches every group with a choice, and within a group any chosen value.
//! The window holds the whole list and each row says what it is in each
//! group (its facets), so filtering takes no request: only the text is
//! searched by the core, and its results are narrowed here.
//!
//! Models are keyed `provider::model`, as favourites are. The labels each
//! value shows ([`labels`]) are `filterLabels` (`web/app.js:2071-2096`).

use std::collections::{BTreeSet, HashMap};

use super::resolution;
use super::vendor::{Logo, vendor_icon};

/// A group (`GROUPS`), in its order: size, prompt, model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// `size`.
    Size,
    /// `prompt`.
    Prompt,
    /// `model`.
    Model,
}

/// The groups in order.
pub const GROUPS: [Group; 3] = [Group::Size, Group::Prompt, Group::Model];

impl Group {
    /// Its heading in the menu (`FILTER_GROUPS`).
    pub fn heading(self) -> &'static str {
        match self {
            Group::Size => "Size",
            Group::Prompt => "Prompt",
            Group::Model => "Model",
        }
    }
}

/// A value of a group: a resolution's number, a prompt's id, a model's key.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Value {
    /// A resolution stop's number.
    Size(i64),
    /// A library prompt's id.
    Prompt(String),
    /// `provider::model`.
    Model(String),
}

impl Value {
    /// Its group.
    pub fn group(&self) -> Group {
        match self {
            Value::Size(_) => Group::Size,
            Value::Prompt(_) => Group::Prompt,
            Value::Model(_) => Group::Model,
        }
    }
}

/// The values chosen, per group, in the order chosen (`NO_FILTERS` is the
/// default).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Filters {
    /// Chosen sizes.
    pub size: Vec<i64>,
    /// Chosen prompts' ids.
    pub prompt: Vec<String>,
    /// Chosen models' keys.
    pub model: Vec<String>,
}

/// No filter.
pub const NO_FILTERS: Filters = Filters {
    size: Vec::new(),
    prompt: Vec::new(),
    model: Vec::new(),
};

/// `modelKey`.
pub fn model_key(provider: &str, model: &str) -> String {
    format!("{provider}::{model}")
}

/// A row's facets, one list per group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facets {
    /// Its resolution.
    pub size: Vec<i64>,
    /// The prompts it came from.
    pub prompt: Vec<String>,
    /// Its models' keys.
    pub model: Vec<String>,
}

impl Filters {
    /// Any group has a choice (`filtering`).
    pub fn filtering(&self) -> bool {
        !(self.size.is_empty() && self.prompt.is_empty() && self.model.is_empty())
    }

    /// How many values are chosen.
    pub fn count(&self) -> usize {
        self.size.len() + self.prompt.len() + self.model.len()
    }

    /// The filters with `value` chosen, or no longer chosen (`toggled`).
    pub fn toggled(&self, value: &Value) -> Filters {
        fn flip<T: PartialEq + Clone>(list: &[T], v: &T) -> Vec<T> {
            if list.contains(v) {
                list.iter().filter(|e| *e != v).cloned().collect()
            } else {
                list.iter().cloned().chain([v.clone()]).collect()
            }
        }
        let mut next = self.clone();
        match value {
            Value::Size(v) => next.size = flip(&self.size, v),
            Value::Prompt(v) => next.prompt = flip(&self.prompt, v),
            Value::Model(v) => next.model = flip(&self.model, v),
        }
        next
    }

    /// Whether `value` is chosen.
    pub fn has(&self, value: &Value) -> bool {
        match value {
            Value::Size(v) => self.size.contains(v),
            Value::Prompt(v) => self.prompt.contains(v),
            Value::Model(v) => self.model.contains(v),
        }
    }

    /// The chosen values, groups in order, values in the order chosen
    /// (the chips' order).
    pub fn values(&self) -> Vec<Value> {
        self.size
            .iter()
            .map(|v| Value::Size(*v))
            .chain(self.prompt.iter().map(|v| Value::Prompt(v.clone())))
            .chain(self.model.iter().map(|v| Value::Model(v.clone())))
            .collect()
    }

    /// Every group with a choice matches, and within a group any chosen
    /// value (`matches`).
    pub fn matches(&self, facets: &Facets) -> bool {
        (self.size.is_empty() || self.size.iter().any(|v| facets.size.contains(v)))
            && (self.prompt.is_empty() || self.prompt.iter().any(|v| facets.prompt.contains(v)))
            && (self.model.is_empty() || self.model.iter().any(|v| facets.model.contains(v)))
    }
}

/// A chat's facets (`chatFacets`): its resolution, the prompts its messages
/// came from, its model.
pub fn chat_facets(row: &mcbench_store::ConversationSummary) -> Facets {
    Facets {
        size: vec![row.resolution],
        prompt: row.prompts.clone(),
        model: vec![model_key(&row.provider, &row.model)],
    }
}

/// A bench's facets (`benchFacets`): its resolution, its rounds' prompts,
/// its members' models.
pub fn bench_facets(
    bench: &crate::ListedBench,
    members: &[&mcbench_store::ConversationSummary],
) -> Facets {
    Facets {
        size: bench.resolution.into_iter().collect(),
        prompt: bench.prompts.clone(),
        model: members
            .iter()
            .map(|m| model_key(&m.provider, &m.model))
            .collect(),
    }
}

/// The rows listed (`listed`): the search's results in their rank order
/// (`found`, the ids, when text is searched; none otherwise; an id no
/// longer listed is dropped) or every row, then those the filters match.
pub fn listed<'a, T>(
    rows: &'a [T],
    id: impl Fn(&T) -> &str,
    found: Option<&[String]>,
    filters: &Filters,
    facets: impl Fn(&T) -> Facets,
) -> Vec<&'a T> {
    let shown: Vec<&T> = match found {
        Some(found) => {
            let by_id: HashMap<&str, &T> = rows.iter().map(|r| (id(r), r)).collect();
            found
                .iter()
                .filter_map(|f| by_id.get(f.as_str()).copied())
                .collect()
        }
        None => rows.iter().collect(),
    };
    if filters.filtering() {
        shown
            .into_iter()
            .filter(|r| filters.matches(&facets(r)))
            .collect()
    } else {
        shown
    }
}

/// A prompt the menu offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OfferedPrompt {
    /// Its id.
    pub id: String,
    /// Its title (the library's, or the one it was last sent under when
    /// deleted from it).
    pub title: String,
    /// Deleted from the library.
    pub deleted: bool,
}

/// What the menu offers (`offered`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// Every resolution stop.
    pub size: Vec<i64>,
    /// The prompts the rows came from, library order, then deleted ones by
    /// title.
    pub prompt: Vec<OfferedPrompt>,
    /// The rows' models, by model id, then key.
    pub model: Vec<String>,
}

/// JavaScript's `localeCompare` as the page's sorts use it on titles and
/// ids: case folded first, then the text itself.
fn locale_compare(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase()
        .cmp(&b.to_lowercase())
        .then_with(|| b.cmp(a))
}

/// What the menu offers, from the facets of every row the mode lists (not
/// only those shown, so one choice never takes the others away): every
/// size; the prompts the rows came from in the library's order (`library`:
/// id and title), then those deleted from it by the title they were last
/// sent under (`titles`); the rows' models by model id, then provider.
pub fn offered(
    facets: &[Facets],
    library: &[(String, String)],
    titles: &HashMap<String, String>,
) -> Offer {
    let prompts: Vec<&String> = {
        let mut seen = BTreeSet::new();
        facets
            .iter()
            .flat_map(|f| f.prompt.iter())
            .filter(|p| seen.insert(p.as_str()))
            .collect()
    };
    let kept: Vec<OfferedPrompt> = library
        .iter()
        .filter(|(id, _)| prompts.contains(&id))
        .map(|(id, title)| OfferedPrompt {
            id: id.clone(),
            title: title.clone(),
            deleted: false,
        })
        .collect();
    let mut deleted: Vec<OfferedPrompt> = prompts
        .iter()
        .filter(|id| !library.iter().any(|(l, _)| l == **id))
        .map(|id| OfferedPrompt {
            id: (*id).clone(),
            title: titles.get(*id).cloned().unwrap_or_default(),
            deleted: true,
        })
        .collect();
    deleted.sort_by(|a, b| locale_compare(&a.title, &b.title));
    let model_of = |key: &str| key[key.find("::").map_or(0, |i| i + 2)..].to_string();
    let mut models: Vec<String> = facets
        .iter()
        .flat_map(|f| f.model.iter().cloned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    models.sort_by(|a, b| {
        locale_compare(&model_of(a), &model_of(b)).then_with(|| locale_compare(a, b))
    });
    Offer {
        size: resolution::STOPS
            .iter()
            .map(|s| i64::from(s.value()))
            .collect(),
        prompt: kept.into_iter().chain(deleted).collect(),
        model: models,
    }
}

/// The chosen values the list still offers (`inEffect`): a prompt or model
/// no listed row has any more is no choice, and its chip goes; it comes
/// back with a row that has it.
pub fn in_effect(filters: &Filters, offer: &Offer) -> Filters {
    Filters {
        size: filters
            .size
            .iter()
            .filter(|v| offer.size.contains(v))
            .copied()
            .collect(),
        prompt: filters
            .prompt
            .iter()
            .filter(|v| offer.prompt.iter().any(|p| &p.id == *v))
            .cloned()
            .collect(),
        model: filters
            .model
            .iter()
            .filter(|v| offer.model.contains(v))
            .cloned()
            .collect(),
    }
}

/// A value's texts (`filterLabels`): in the menu, on its chip, a note, a
/// logo, and the full name (the hover title and the chip's remove label).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// The menu's text.
    pub text: String,
    /// The chip's text.
    pub chip: String,
    /// `Deleted`, or the provider of a model whose short name another
    /// offered model shares.
    pub note: Option<String>,
    /// A model's vendor logo.
    pub logo: Option<Logo>,
    /// `Size: …`, `Prompt: …`, `Model: … (…)`.
    pub full: String,
}

/// `modelLabel`: a model id after its last `/`.
pub fn model_label(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

/// Every offered value's texts; `provider_label` names a provider by its
/// key (an unknown provider shows its key).
pub fn labels(
    offer: &Offer,
    provider_label: impl Fn(&str) -> Option<String>,
) -> Vec<(Value, Label)> {
    let mut out = Vec::new();
    for value in &offer.size {
        let stop = resolution::stop(*value);
        out.push((
            Value::Size(*value),
            Label {
                text: resolution::label(stop).to_string(),
                chip: if *value == 1 {
                    resolution::label(stop).to_string()
                } else {
                    resolution::short(stop).to_string()
                },
                note: None,
                logo: None,
                full: format!("Size: {}", resolution::label(stop)),
            },
        ));
    }
    for entry in &offer.prompt {
        let title = if entry.title.is_empty() {
            "Untitled prompt".to_string()
        } else {
            entry.title.clone()
        };
        out.push((
            Value::Prompt(entry.id.clone()),
            Label {
                text: title.clone(),
                chip: title.clone(),
                note: entry.deleted.then(|| "Deleted".to_string()),
                logo: None,
                full: format!(
                    "Prompt: {title}{}",
                    if entry.deleted {
                        " (deleted from the library)"
                    } else {
                        ""
                    }
                ),
            },
        ));
    }
    let split = |key: &str| -> (String, String) {
        let at = key.find("::").expect("a model key is provider::model");
        (key[..at].to_string(), key[at + 2..].to_string())
    };
    let names: Vec<String> = offer
        .model
        .iter()
        .map(|k| model_label(&split(k).1).to_string())
        .collect();
    for (index, key) in offer.model.iter().enumerate() {
        let (provider, id) = split(key);
        let label = provider_label(&provider).unwrap_or_else(|| provider.clone());
        let shared = names.iter().filter(|n| **n == names[index]).count() > 1;
        out.push((
            Value::Model(key.clone()),
            Label {
                text: names[index].clone(),
                chip: names[index].clone(),
                note: shared.then(|| label.clone()),
                logo: Some(vendor_icon(&provider, &id)),
                full: format!("Model: {id} ({label})"),
            },
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcbench_store::{ConversationId, ConversationSummary};

    fn chat(
        id: &str,
        resolution: i64,
        model: &str,
        prompts: &[&str],
        provider: &str,
    ) -> ConversationSummary {
        ConversationSummary {
            id: ConversationId(id.into()),
            title: id.into(),
            provider: provider.into(),
            model: model.into(),
            effort: None,
            resolution,
            created_at: String::new(),
            bench_id: None,
            bench_slot: None,
            unread: false,
            message_count: 1,
            prompts: prompts.iter().map(|p| p.to_string()).collect(),
        }
    }

    fn chats() -> Vec<ConversationSummary> {
        vec![
            chat("castle", 10, "vendor/alpha", &["p-castle"], "openrouter"),
            chat(
                "tower",
                2,
                "vendor/beta",
                &["p-tower", "p-castle"],
                "openrouter",
            ),
            chat("hut", 1, "vendor/alpha", &[], "openrouter"),
            chat("shed", 5, "vendor/alpha", &[], "anthropic"),
        ]
    }

    fn id(row: &ConversationSummary) -> &str {
        &row.id.0
    }

    fn ids(rows: Vec<&ConversationSummary>) -> Vec<&str> {
        rows.into_iter().map(id).collect()
    }

    fn found(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_filter_lists_every_row_in_the_order_given_or_the_search_ranked() {
        let rows = chats();
        assert!(!NO_FILTERS.filtering());
        assert_eq!(
            ids(listed(&rows, id, None, &NO_FILTERS, chat_facets)),
            ["castle", "tower", "hut", "shed"]
        );
        let ranked = found(&["hut", "gone", "castle"]);
        assert_eq!(
            ids(listed(&rows, id, Some(&ranked), &NO_FILTERS, chat_facets)),
            ["hut", "castle"],
            "a result no longer listed is dropped"
        );
        assert!(
            listed(&rows, id, Some(&[]), &NO_FILTERS, chat_facets).is_empty(),
            "a search that found nothing lists nothing"
        );
    }

    #[test]
    fn each_group_alone_any_chosen_value_matches() {
        let rows = chats();
        let shown = |f: &Filters| ids(listed(&rows, id, None, f, chat_facets));
        assert_eq!(shown(&NO_FILTERS.toggled(&Value::Size(10))), ["castle"]);
        assert_eq!(
            shown(
                &NO_FILTERS
                    .toggled(&Value::Size(10))
                    .toggled(&Value::Size(1))
            ),
            ["castle", "hut"]
        );
        assert_eq!(
            shown(&NO_FILTERS.toggled(&Value::Prompt("p-castle".into()))),
            ["castle", "tower"],
            "a chat matches any prompt one of its messages came from"
        );
        assert_eq!(
            shown(&NO_FILTERS.toggled(&Value::Prompt("p-tower".into()))),
            ["tower"]
        );
        assert_eq!(
            shown(&NO_FILTERS.toggled(&Value::Model(model_key("openrouter", "vendor/alpha")))),
            ["castle", "hut"],
            "the same model through another provider is another model"
        );
        assert_eq!(shown(&NO_FILTERS.toggled(&Value::Size(1))), ["hut"]);
        let off = NO_FILTERS
            .toggled(&Value::Size(10))
            .toggled(&Value::Size(10));
        assert_eq!(off, NO_FILTERS, "toggling again takes the choice back");
        assert_eq!(
            shown(&off),
            ["castle", "tower", "hut", "shed"],
            "an empty choice is no filter"
        );
    }

    #[test]
    fn groups_combine_a_row_must_match_every_group_with_a_choice_and_the_search_text() {
        let rows = chats();
        let alpha = Value::Model(model_key("openrouter", "vendor/alpha"));
        let castle_alpha = NO_FILTERS
            .toggled(&Value::Prompt("p-castle".into()))
            .toggled(&alpha);
        assert_eq!(
            ids(listed(&rows, id, None, &castle_alpha, chat_facets)),
            ["castle"]
        );
        assert!(
            listed(
                &rows,
                id,
                None,
                &castle_alpha.toggled(&Value::Size(2)),
                chat_facets
            )
            .is_empty(),
            "no row is both"
        );
        let ranked = found(&["hut", "castle", "tower"]);
        assert_eq!(
            ids(listed(
                &rows,
                id,
                Some(&ranked),
                &NO_FILTERS.toggled(&alpha),
                chat_facets
            )),
            ["hut", "castle"],
            "in the search's order"
        );
        assert!(
            listed(
                &rows,
                id,
                Some(&found(&["tower"])),
                &NO_FILTERS.toggled(&alpha),
                chat_facets
            )
            .is_empty()
        );
    }

    #[test]
    fn a_bench_matches_by_its_resolution_any_of_its_rounds_prompts_and_any_of_its_models() {
        let bench = |id: &str, resolution: i64, prompts: &[&str]| crate::ListedBench {
            id: id.into(),
            title: id.into(),
            created_at: String::new(),
            head: None,
            resolution: Some(resolution),
            rounds: 1,
            prompts: prompts.iter().map(|p| p.to_string()).collect(),
            blind: false,
            revealed: false,
            order: Vec::new(),
        };
        let benches = vec![bench("b1", 5, &["p-castle"]), bench("b2", 1, &[])];
        let members: HashMap<&str, Vec<ConversationSummary>> = [
            (
                "b1",
                vec![
                    chat("m1", 5, "vendor/alpha", &[], "openrouter"),
                    chat("m2", 5, "vendor/beta", &[], "openrouter"),
                ],
            ),
            (
                "b2",
                vec![
                    chat("m3", 1, "vendor/gamma", &[], "openrouter"),
                    chat("m4", 1, "vendor/beta", &[], "openrouter"),
                ],
            ),
        ]
        .into();
        let facets = |b: &crate::ListedBench| {
            let m: Vec<&ConversationSummary> = members[b.id.as_str()].iter().collect();
            bench_facets(b, &m)
        };
        let shown = |f: Filters| -> Vec<String> {
            listed(&benches, |b| b.id.as_str(), None, &f, facets)
                .into_iter()
                .map(|b| b.id.clone())
                .collect()
        };
        let model = |m: &str| Value::Model(model_key("openrouter", m));
        assert_eq!(
            shown(NO_FILTERS.toggled(&model("vendor/beta"))),
            ["b1", "b2"]
        );
        assert_eq!(shown(NO_FILTERS.toggled(&model("vendor/gamma"))), ["b2"]);
        assert_eq!(shown(NO_FILTERS.toggled(&Value::Size(5))), ["b1"]);
        assert!(
            shown(
                NO_FILTERS
                    .toggled(&Value::Prompt("p-castle".into()))
                    .toggled(&model("vendor/gamma"))
            )
            .is_empty()
        );
    }

    #[test]
    fn the_menu_offers_every_size_the_prompts_the_rows_came_from_in_library_order_then_deleted_ones_and_the_rows_models()
     {
        let library = vec![
            ("p-tower".to_string(), "Tower".to_string()),
            ("p-unused".to_string(), "Unused".to_string()),
            ("p-castle".to_string(), String::new()),
        ];
        let mut rows = chats();
        rows.push(chat(
            "old",
            1,
            "vendor/alpha",
            &["p-gone-b", "p-gone-a"],
            "openrouter",
        ));
        let titles: HashMap<String, String> = [
            ("p-gone-a", "Windmill"),
            ("p-gone-b", "Barn"),
            ("p-castle", "Old castle"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let facets: Vec<Facets> = rows.iter().map(chat_facets).collect();
        let offer = offered(&facets, &library, &titles);
        assert_eq!(offer.size, [1, 2, 5, 10]);
        let prompt = |id: &str, title: &str, deleted: bool| OfferedPrompt {
            id: id.into(),
            title: title.into(),
            deleted,
        };
        assert_eq!(
            offer.prompt,
            [
                prompt("p-tower", "Tower", false),
                prompt("p-castle", "", false),
                prompt("p-gone-b", "Barn", true),
                prompt("p-gone-a", "Windmill", true),
            ],
            "a prompt in the library goes by its title there; one deleted by the title it was last sent under"
        );
        assert_eq!(
            offer.model,
            [
                model_key("anthropic", "vendor/alpha"),
                model_key("openrouter", "vendor/alpha"),
                model_key("openrouter", "vendor/beta"),
            ]
        );
        assert_eq!(
            offered(&[], &library, &HashMap::new()),
            Offer {
                size: vec![1, 2, 5, 10],
                prompt: Vec::new(),
                model: Vec::new()
            }
        );
    }

    #[test]
    fn a_chosen_prompt_or_model_no_listed_row_has_is_not_in_effect_and_is_again_once_a_row_has_it()
    {
        let chosen = NO_FILTERS
            .toggled(&Value::Prompt("p-tower".into()))
            .toggled(&Value::Model(model_key("openrouter", "vendor/beta")))
            .toggled(&Value::Size(2));
        let library = vec![("p-tower".to_string(), "Tower".to_string())];
        let all: Vec<Facets> = chats().iter().map(chat_facets).collect();
        assert_eq!(
            in_effect(&chosen, &offered(&all, &library, &HashMap::new())),
            chosen
        );
        let without: Vec<Facets> = chats()
            .iter()
            .filter(|r| r.id.0 != "tower")
            .map(chat_facets)
            .collect();
        assert_eq!(
            in_effect(&chosen, &offered(&without, &library, &HashMap::new())),
            Filters {
                size: vec![2],
                prompt: Vec::new(),
                model: Vec::new()
            },
            "every size is always offered"
        );
    }

    #[test]
    fn each_value_has_its_menu_text_chip_note_and_full_name() {
        let offer = Offer {
            size: vec![1, 5],
            prompt: vec![
                OfferedPrompt {
                    id: "p1".into(),
                    title: String::new(),
                    deleted: false,
                },
                OfferedPrompt {
                    id: "p2".into(),
                    title: "Barn".into(),
                    deleted: true,
                },
            ],
            model: vec![
                model_key("anthropic", "claude-x"),
                model_key("openrouter", "anthropic/claude-x"),
                model_key("mystery", "solo"),
            ],
        };
        let labels = labels(&offer, |p| match p {
            "anthropic" => Some("Anthropic".into()),
            "openrouter" => Some("OpenRouter".into()),
            _ => None,
        });
        let of = |v: Value| {
            labels
                .iter()
                .find(|(k, _)| *k == v)
                .expect("labelled")
                .1
                .clone()
        };
        assert_eq!(of(Value::Size(1)).chip, "Standard");
        assert_eq!(of(Value::Size(5)).chip, "5\u{d7}");
        assert_eq!(of(Value::Size(5)).full, "Size: Very high (5\u{d7})");
        assert_eq!(of(Value::Prompt("p1".into())).text, "Untitled prompt");
        let barn = of(Value::Prompt("p2".into()));
        assert_eq!(barn.note.as_deref(), Some("Deleted"));
        assert_eq!(barn.full, "Prompt: Barn (deleted from the library)");
        let shared = of(Value::Model(model_key("openrouter", "anthropic/claude-x")));
        assert_eq!(shared.text, "claude-x");
        assert_eq!(shared.note.as_deref(), Some("OpenRouter"));
        assert_eq!(shared.full, "Model: anthropic/claude-x (OpenRouter)");
        let solo = of(Value::Model(model_key("mystery", "solo")));
        assert_eq!(solo.note, None);
        assert_eq!(
            solo.full, "Model: solo (mystery)",
            "an unknown provider shows its key"
        );
    }
}
