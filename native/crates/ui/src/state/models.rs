//! The model picker's rules (`ModelPicker`, `web/app.js:1303-1538`;
//! inventory `components.md` 1.2, 1.3, A.9 and D.3) and the effort
//! picker's (`EffortPicker`, `effortFor`; 1.4 and D.4), with no GPUI types:
//! which providers the rail shows and what each says, the tab the picker
//! opens on, which models a tab or a search lists and in what order, what
//! the list says while loading, failed or empty, and the rail's lines.

use std::collections::HashMap;

use crate::{Auth, ModelFacts, ModelInfo, Provider};

/// `usable` (`settings-panel.mjs`): connected and enabled.
pub fn usable(provider: &Provider) -> bool {
    provider.connected && provider.enabled
}

/// A tab of the picker.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    /// The starred models.
    Favorites,
    /// One provider's models, by its key.
    Provider(String),
}

/// What a provider's catalogue is in the window.
#[derive(Clone, Debug, PartialEq)]
pub enum Catalogue {
    /// Asked for, not answered yet.
    Loading,
    /// The models, newest first.
    Ready(Vec<ModelInfo>),
    /// The provider's failure; a later ask goes again.
    Failed(String),
}

/// One item of the rail, after the Favorites tab.
#[derive(Clone, Debug, PartialEq)]
pub struct RailEntry {
    /// The provider's key.
    pub id: String,
    /// Its title, which is its accessible name: the label, what a harness
    /// is, and why it is disabled.
    pub title: String,
    /// Disabled: enabled in Settings but not connected.
    pub disabled: bool,
    /// A harness (the `›_` badge).
    pub harness: bool,
    /// The `Harnesses` divider is drawn before it (the first harness).
    pub divider_before: bool,
}

/// The rail's harness text, after the label.
pub const HARNESS: &str = " harness: its own agent loop with mcbench tools attached";

/// The rail: every provider enabled in Settings, in the core's order (the
/// ones turned off are not on it), the divider before the first harness.
pub fn rail(providers: &[Provider]) -> Vec<RailEntry> {
    let enabled: Vec<&Provider> = providers.iter().filter(|p| p.enabled).collect();
    enabled
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let why = if p.connected {
                ""
            } else if p.auth == Auth::CliLogin {
                " \u{2014} CLI not signed in"
            } else {
                " \u{2014} not configured"
            };
            RailEntry {
                id: p.id.clone(),
                title: format!("{}{}{why}", p.label, if p.harness { HARNESS } else { "" }),
                disabled: !usable(p),
                harness: p.harness,
                divider_before: p.harness && !(index > 0 && enabled[index - 1].harness),
            }
        })
        .collect()
}

fn provider<'a>(providers: &'a [Provider], id: &str) -> Option<&'a Provider> {
    providers.iter().find(|p| p.id == id)
}

fn usable_id(providers: &[Provider], id: &str) -> bool {
    provider(providers, id).is_some_and(usable)
}

/// The tab the picker opens on: the chosen model's provider if usable, else
/// Favorites when a favourite is of a usable provider, else the first usable
/// provider; none when none can be used.
pub fn opening_tab(
    providers: &[Provider],
    chosen_provider: &str,
    favorites: &[(String, String)],
) -> Option<Tab> {
    if !chosen_provider.is_empty() && usable_id(providers, chosen_provider) {
        return Some(Tab::Provider(chosen_provider.to_string()));
    }
    if favorites.iter().any(|(p, _)| usable_id(providers, p)) {
        return Some(Tab::Favorites);
    }
    providers
        .iter()
        .find(|p| usable(p))
        .map(|p| Tab::Provider(p.id.clone()))
}

/// A tab whose provider can no longer be used gives way to the opening tab
/// (`useEffect` on `activeProvider`, `providers`).
pub fn settled_tab(
    tab: Option<Tab>,
    providers: &[Provider],
    chosen_provider: &str,
    favorites: &[(String, String)],
) -> Option<Tab> {
    match tab {
        Some(Tab::Favorites) => Some(Tab::Favorites),
        Some(Tab::Provider(id)) if usable_id(providers, &id) => Some(Tab::Provider(id)),
        _ => opening_tab(providers, chosen_provider, favorites),
    }
}

/// `orderModels`: the models named in `order` first, in that order; the
/// others after, in catalogue order.
pub fn order_models(models: &[ModelInfo], order: &[String]) -> Vec<ModelInfo> {
    if order.is_empty() {
        return models.to_vec();
    }
    let rank: HashMap<&str, usize> = order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.as_str(), i))
        .collect();
    let mut ranked: Vec<(usize, &ModelInfo)> = models
        .iter()
        .enumerate()
        .map(|(i, m)| {
            (
                rank.get(m.id.as_str()).copied().unwrap_or(order.len() + i),
                m,
            )
        })
        .collect();
    ranked.sort_by_key(|(r, _)| *r);
    ranked.into_iter().map(|(_, m)| m.clone()).collect()
}

/// `displayName`: the catalogue's name without a leading `Vendor: `.
pub fn display_name(name: &str) -> &str {
    match name.split_once(':') {
        Some((vendor, rest)) if !vendor.is_empty() => rest.trim_start(),
        _ => name,
    }
}

/// `modelLabel`: the id after its last `/`.
pub fn model_label(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

/// `CURATED_VENDORS`: the OpenRouter tab's default vendors.
pub const CURATED_VENDORS: [&str; 4] = ["openai/", "anthropic/", "x-ai/", "google/"];
/// `CURATED_LIMIT`.
pub const CURATED_LIMIT: usize = 30;

/// A listed model.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed {
    /// Its provider's key.
    pub provider: String,
    /// The model.
    pub model: ModelInfo,
}

/// What the list shows.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    /// The providers whose catalogues it reads (to be asked for when not
    /// held).
    pub shown: Vec<String>,
    /// The models, in order.
    pub models: Vec<Listed>,
    /// `Loading models...`: some shown catalogue is loading and no model is
    /// listed yet.
    pub loading: bool,
    /// The first failing catalogue's message (prefixed with its provider's
    /// label when searching).
    pub error: Option<String>,
    /// The empty text, when nothing is listed, nothing loads and nothing
    /// failed.
    pub empty: Option<&'static str>,
}

/// The texts of the list's states.
pub const LOADING: &str = "Loading models...";
/// With a search.
pub const NO_MATCH: &str = "No model matches";
/// On Favorites.
pub const NO_FAVORITES: &str = "No favorite models yet.";
/// On a provider.
pub const NO_MODELS: &str = "No matching models.";
/// With no usable provider.
pub const NO_AI: &str = "No AI connected yet";
/// Its button.
pub const CONNECT_AI: &str = "Connect an AI";
/// The search's placeholder.
pub const SEARCH_PLACEHOLDER: &str = "Search models...";

/// The models the picker lists for `tab` and the search `query` (the page's
/// `shownProviders`, `models`, `loading`, `error`).
pub fn listing(
    providers: &[Provider],
    catalogues: &HashMap<String, Catalogue>,
    favorites: &[(String, String)],
    tab: Option<&Tab>,
    query: &str,
    require_tools: bool,
) -> Listing {
    let needle = query.trim().to_lowercase();
    let usable_ids: Vec<&str> = providers
        .iter()
        .filter(|p| usable(p))
        .map(|p| p.id.as_str())
        .collect();
    let mut shown: Vec<String> = if !needle.is_empty() {
        usable_ids.iter().map(|s| s.to_string()).collect()
    } else {
        match tab {
            Some(Tab::Favorites) => {
                let mut ids: Vec<String> = Vec::new();
                for (p, _) in favorites {
                    if usable_id(providers, p) && !ids.contains(p) {
                        ids.push(p.clone());
                    }
                }
                ids
            }
            Some(Tab::Provider(id)) => vec![id.clone()],
            None => Vec::new(),
        }
    };
    shown.retain(|id| usable_id(providers, id));
    let entries: Vec<Option<&Catalogue>> = shown.iter().map(|id| catalogues.get(id)).collect();
    let loading = entries
        .iter()
        .any(|e| matches!(e, None | Some(Catalogue::Loading)));
    let error = shown
        .iter()
        .zip(&entries)
        .find_map(|(id, entry)| match entry {
            Some(Catalogue::Failed(message)) => Some(if needle.is_empty() {
                message.clone()
            } else {
                let label = provider(providers, id)
                    .map(|p| p.label.as_str())
                    .unwrap_or(id);
                format!("{label}: {message}")
            }),
            _ => None,
        });
    let is_favorite = |p: &str, m: &str| favorites.iter().any(|(fp, fm)| fp == p && fm == m);
    let mut models: Vec<Listed> = shown
        .iter()
        .zip(&entries)
        .flat_map(|(id, entry)| {
            let p = provider(providers, id).expect("a shown provider is listed");
            let held: Vec<ModelInfo> = match entry {
                Some(Catalogue::Ready(models)) => models
                    .iter()
                    .filter(|m| !p.hidden_models.contains(&m.id))
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            };
            order_models(&held, &p.model_order)
                .into_iter()
                .map(|model| Listed {
                    provider: id.clone(),
                    model,
                })
                .collect::<Vec<_>>()
        })
        .collect();
    if needle.is_empty() && tab == Some(&Tab::Favorites) {
        models.retain(|l| is_favorite(&l.provider, &l.model.id));
    }
    models.retain(|l| {
        if require_tools && l.model.facts.tools != Some(true) {
            return false;
        }
        needle.is_empty()
            || l.model.name.to_lowercase().contains(&needle)
            || l.model.id.to_lowercase().contains(&needle)
    });
    if needle.is_empty() && tab == Some(&Tab::Provider("openrouter".into())) {
        let (starred, rest): (Vec<Listed>, Vec<Listed>) = models
            .into_iter()
            .partition(|l| is_favorite("openrouter", &l.model.id));
        let curated = rest
            .into_iter()
            .filter(|l| CURATED_VENDORS.iter().any(|v| l.model.id.starts_with(v)))
            .take(CURATED_LIMIT);
        models = starred.into_iter().chain(curated).collect();
    }
    let empty =
        (!loading && error.is_none() && models.is_empty()).then_some(if !needle.is_empty() {
            NO_MATCH
        } else if tab == Some(&Tab::Favorites) {
            NO_FAVORITES
        } else {
            NO_MODELS
        });
    Listing {
        shown,
        loading: loading && models.is_empty(),
        error,
        empty,
        models,
    }
}

/// A model row's accessible name: its display name, and its provider's
/// label when searching (the row shows it then).
pub fn row_name(listed: &Listed, providers: &[Provider], searching: bool) -> String {
    let name = display_name(&listed.model.name);
    if searching {
        let label = provider(providers, &listed.provider)
            .map(|p| p.label.as_str())
            .unwrap_or(&listed.provider);
        format!("{name}, {label}")
    } else {
        name.to_string()
    }
}

/// The star's name (a toggle): `Favorite <model>` in the page's spelling;
/// its title says what a press does.
pub fn star_name(listed: &Listed) -> String {
    format!("Favorite {}", display_name(&listed.model.name))
}

/// The star's title: `Add favorite` or `Remove favorite`.
pub fn star_title(favorite: bool) -> &'static str {
    if favorite {
        "Remove favorite"
    } else {
        "Add favorite"
    }
}

/// The trigger's empty label (`Choose <label lowercased>`) for the chat
/// model picker.
pub const EMPTY_LABEL: &str = "Choose chat model";

/// The rail's lines: the fewest columns its items (each of its height in
/// `heights`, `gap` apart, in order) fit in `available` height, the items
/// shared out evenly, as the page lays it out on open; with its per-line
/// count.
pub fn rail_lines(heights: &[f32], gap: f32, available: f32) -> (usize, usize) {
    let fits = |per: usize| {
        heights.chunks(per.max(1)).all(|column| {
            column.iter().sum::<f32>() + column.len().saturating_sub(1) as f32 * gap <= available
        })
    };
    let n = heights.len();
    let mut lines = 1;
    while lines < n && !fits(n.div_ceil(lines)) {
        lines += 1;
    }
    (lines, n.div_ceil(lines).max(1))
}

/// `effortFor`: the effort a request actually uses: the chosen one if the
/// model offers it, else the model's default (none).
pub fn effort_for(facts: Option<&ModelFacts>, effort: Option<&str>) -> Option<String> {
    let facts = facts?;
    let effort = effort?;
    (facts.reasoning && facts.efforts.iter().any(|e| e == effort)).then(|| effort.to_string())
}

/// Whether the effort picker is shown: the model reasons and lists
/// efforts.
pub fn effort_shown(facts: Option<&ModelFacts>) -> bool {
    facts.is_some_and(|f| f.reasoning && !f.efforts.is_empty())
}

/// The effort the picker shows as current: the chosen one if listed, else
/// the model's default, else the first.
pub fn current_effort<'a>(facts: &'a ModelFacts, effort: Option<&str>) -> &'a str {
    facts
        .efforts
        .iter()
        .find(|e| Some(e.as_str()) == effort)
        .or_else(|| {
            facts
                .efforts
                .iter()
                .find(|e| Some(e.as_str()) == facts.default_effort.as_deref())
        })
        .or(facts.efforts.first())
        .map(String::as_str)
        .expect("the effort picker is shown only with efforts")
}

/// The effort picker's label (`Reasoning effort`).
pub const EFFORT_LABEL: &str = "Reasoning effort";

#[cfg(test)]
mod tests {
    use super::*;

    fn p(
        id: &str,
        label: &str,
        connected: bool,
        enabled: bool,
        harness: bool,
        auth: Auth,
    ) -> Provider {
        Provider {
            id: id.into(),
            label: label.into(),
            connected,
            auth,
            harness,
            enabled,
            hidden_models: Vec::new(),
            model_order: Vec::new(),
        }
    }

    fn facts(tools: Option<bool>) -> ModelFacts {
        ModelFacts {
            known: true,
            image_input: None,
            image_output: false,
            tools,
            reasoning: true,
            efforts: vec!["low".into(), "medium".into(), "high".into()],
            default_effort: Some("medium".into()),
            mandatory_reasoning: false,
            context_window: Some(Some(200_000.)),
            fast: None,
        }
    }

    fn m(id: &str, name: &str, tools: Option<bool>) -> ModelInfo {
        ModelInfo {
            id: id.into(),
            name: name.into(),
            created: 0.,
            facts: facts(tools),
        }
    }

    fn providers() -> Vec<Provider> {
        vec![
            p("openrouter", "OpenRouter", true, true, false, Auth::ApiKey),
            p("openai", "OpenAI", false, true, false, Auth::ApiKey),
            p("codex", "ChatGPT", false, true, false, Auth::CliLogin),
            p("anthropic", "Anthropic", true, true, false, Auth::ApiKey),
            p("qwencloud", "QwenCloud", true, false, false, Auth::ApiKey),
            p(
                "claude-code",
                "Claude Code",
                true,
                true,
                true,
                Auth::CliLogin,
            ),
            p("codex-cli", "Codex CLI", false, true, true, Auth::CliLogin),
        ]
    }

    fn key(p: &str, m: &str) -> (String, String) {
        (p.into(), m.into())
    }

    #[test]
    fn the_rail_lists_enabled_providers_with_their_titles_and_the_divider() {
        let rail = rail(&providers());
        let titles: Vec<&str> = rail.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "OpenRouter",
                "OpenAI \u{2014} not configured",
                "ChatGPT \u{2014} CLI not signed in",
                "Anthropic",
                "Claude Code harness: its own agent loop with mcbench tools attached",
                "Codex CLI harness: its own agent loop with mcbench tools attached \u{2014} CLI not signed in",
            ]
        );
        let disabled: Vec<bool> = rail.iter().map(|r| r.disabled).collect();
        assert_eq!(disabled, [false, true, true, false, false, true]);
        let dividers: Vec<bool> = rail.iter().map(|r| r.divider_before).collect();
        assert_eq!(dividers, [false, false, false, false, true, false]);
    }

    #[test]
    fn the_picker_opens_on_the_models_provider_else_favorites_else_the_first_usable() {
        let ps = providers();
        assert_eq!(
            opening_tab(&ps, "anthropic", &[]),
            Some(Tab::Provider("anthropic".into()))
        );
        assert_eq!(
            opening_tab(&ps, "openai", &[key("claude-code", "x")]),
            Some(Tab::Favorites)
        );
        // A favourite of a provider that cannot be used does not count.
        assert_eq!(
            opening_tab(&ps, "", &[key("openai", "gpt-5")]),
            Some(Tab::Provider("openrouter".into()))
        );
        let none: Vec<Provider> = ps
            .iter()
            .map(|p| Provider {
                connected: false,
                ..p.clone()
            })
            .collect();
        assert_eq!(opening_tab(&none, "anthropic", &[]), None);
        // A tab whose provider went away gives way to the opening tab.
        assert_eq!(
            settled_tab(Some(Tab::Provider("openai".into())), &ps, "anthropic", &[]),
            Some(Tab::Provider("anthropic".into()))
        );
    }

    #[test]
    fn a_tab_lists_its_catalogue_without_hidden_models_in_the_saved_order() {
        let mut ps = providers();
        ps[3].hidden_models = vec!["c".into()];
        ps[3].model_order = vec!["d".into(), "b".into()];
        let catalogues: HashMap<String, Catalogue> = [(
            "anthropic".to_string(),
            Catalogue::Ready(vec![
                m("a", "A", Some(true)),
                m("b", "B", Some(true)),
                m("c", "C", Some(true)),
                m("d", "D", Some(true)),
                m("e", "E", None),
                m("f", "F", Some(false)),
            ]),
        )]
        .into();
        let tab = Tab::Provider("anthropic".into());
        let listing = listing(&ps, &catalogues, &[], Some(&tab), "", true);
        let ids: Vec<&str> = listing.models.iter().map(|l| l.model.id.as_str()).collect();
        // Tools not known (null) is dropped as false is.
        assert_eq!(ids, ["d", "b", "a"]);
        assert!(!listing.loading && listing.error.is_none() && listing.empty.is_none());
    }

    #[test]
    fn openrouter_without_a_search_shows_favourites_then_thirty_from_four_vendors() {
        let mut models: Vec<ModelInfo> = (0..40)
            .map(|i| {
                m(
                    &format!("openai/gpt-{i}"),
                    &format!("OpenAI: GPT {i}"),
                    Some(true),
                )
            })
            .collect();
        models.insert(5, m("meta-llama/llama-4", "Meta: Llama 4", Some(true)));
        models.push(m("mistralai/mix", "Mistral: Mix", Some(true)));
        let catalogues: HashMap<String, Catalogue> =
            [("openrouter".to_string(), Catalogue::Ready(models))].into();
        let favorites = [key("openrouter", "mistralai/mix")];
        let tab = Tab::Provider("openrouter".into());
        let shown = listing(&providers(), &catalogues, &favorites, Some(&tab), "", true);
        assert_eq!(shown.models.len(), 31);
        assert_eq!(shown.models[0].model.id, "mistralai/mix");
        assert!(
            shown.models[1..]
                .iter()
                .all(|l| l.model.id.starts_with("openai/"))
        );
        // Everything else is a search away.
        let found = listing(
            &providers(),
            &catalogues,
            &favorites,
            Some(&tab),
            "LLAMA",
            true,
        );
        let ids: Vec<&str> = found.models.iter().map(|l| l.model.id.as_str()).collect();
        assert_eq!(ids, ["meta-llama/llama-4"]);
    }

    #[test]
    fn a_search_reads_every_usable_provider_and_names_a_failure_by_its_provider() {
        let catalogues: HashMap<String, Catalogue> = [
            (
                "openrouter".to_string(),
                Catalogue::Failed("HTTP 401".into()),
            ),
            (
                "anthropic".to_string(),
                Catalogue::Ready(vec![m("claude-x", "Claude X", Some(true))]),
            ),
            ("claude-code".to_string(), Catalogue::Loading),
        ]
        .into();
        let found = listing(&providers(), &catalogues, &[], None, " claude ", true);
        assert_eq!(found.shown, ["openrouter", "anthropic", "claude-code"]);
        assert_eq!(found.error.as_deref(), Some("OpenRouter: HTTP 401"));
        assert_eq!(found.models.len(), 1);
        assert!(!found.loading, "a model is listed");
        assert_eq!(
            row_name(&found.models[0], &providers(), true),
            "Claude X, Anthropic"
        );
        let tab = Tab::Provider("openrouter".into());
        let failed = listing(&providers(), &catalogues, &[], Some(&tab), "", true);
        assert_eq!(failed.error.as_deref(), Some("HTTP 401"));
        assert_eq!(failed.empty, None);
    }

    #[test]
    fn the_empty_and_loading_states_say_what_the_page_says() {
        let catalogues: HashMap<String, Catalogue> = [
            ("anthropic".to_string(), Catalogue::Ready(Vec::new())),
            ("claude-code".to_string(), Catalogue::Loading),
        ]
        .into();
        let ps = providers();
        let a = Tab::Provider("anthropic".into());
        assert_eq!(
            listing(&ps, &catalogues, &[], Some(&a), "", true).empty,
            Some(NO_MODELS)
        );
        assert_eq!(
            listing(&ps, &catalogues, &[], Some(&Tab::Favorites), "", true).empty,
            Some(NO_FAVORITES)
        );
        let c = Tab::Provider("claude-code".into());
        let loading = listing(&ps, &catalogues, &[], Some(&c), "", true);
        assert!(loading.loading && loading.empty.is_none());
        // Hidden models never appear, not even as favourites.
        let mut hidden = ps.clone();
        hidden[3].hidden_models = vec!["x".into()];
        let held: HashMap<String, Catalogue> = [(
            "anthropic".to_string(),
            Catalogue::Ready(vec![m("x", "X", Some(true))]),
        )]
        .into();
        let fav = listing(
            &hidden,
            &held,
            &[key("anthropic", "x")],
            Some(&Tab::Favorites),
            "",
            true,
        );
        assert!(fav.models.is_empty());
        assert_eq!(fav.empty, Some(NO_FAVORITES));
        let all = listing(&hidden, &held, &[], None, "x", true);
        assert_eq!(all.empty, None, "claude-code is still loading");
    }

    #[test]
    fn names_and_labels_drop_the_vendor() {
        assert_eq!(display_name("OpenAI: GPT-5"), "GPT-5");
        assert_eq!(display_name("Claude Opus 5.5"), "Claude Opus 5.5");
        assert_eq!(model_label("openai/gpt-5"), "gpt-5");
        assert_eq!(model_label("claude-opus-5-5"), "claude-opus-5-5");
    }

    #[test]
    fn the_rail_takes_the_fewest_lines_it_fits_in() {
        let ten = [34.; 10];
        assert_eq!(rail_lines(&ten, 3., 500.), (1, 10));
        assert_eq!(rail_lines(&ten, 3., 200.), (2, 5));
        assert_eq!(rail_lines(&ten, 3., 100.), (5, 2));
        // A divider (7 px) is an item of its own height: ten buttons and a
        // divider fit one line in 34 * 10 + 7 + 3 * 10.
        let mut with_divider = ten.to_vec();
        with_divider.insert(7, 7.);
        assert_eq!(rail_lines(&with_divider, 3., 377.), (1, 11));
    }

    #[test]
    fn the_effort_sent_is_the_chosen_one_only_when_the_model_offers_it() {
        let f = facts(Some(true));
        assert_eq!(effort_for(Some(&f), Some("high")), Some("high".into()));
        assert_eq!(effort_for(Some(&f), Some("xhigh")), None);
        assert_eq!(effort_for(Some(&f), None), None);
        assert_eq!(effort_for(None, Some("high")), None);
        assert_eq!(current_effort(&f, None), "medium");
        assert_eq!(current_effort(&f, Some("xhigh")), "medium");
        assert_eq!(current_effort(&f, Some("low")), "low");
        let no_default = ModelFacts {
            default_effort: None,
            ..f.clone()
        };
        assert_eq!(current_effort(&no_default, None), "low");
        assert!(effort_shown(Some(&f)));
        assert!(!effort_shown(Some(&ModelFacts {
            reasoning: false,
            ..f.clone()
        })));
        assert!(!effort_shown(Some(&ModelFacts {
            efforts: Vec::new(),
            ..f
        })));
        assert!(!effort_shown(None));
    }
}
