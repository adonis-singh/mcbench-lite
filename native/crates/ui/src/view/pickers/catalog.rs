//! What every picker reads from the core, held once per window: the
//! providers (`Update::Providers`), each provider's catalogue
//! (`Command::Models`, `Update::Models`), each model's facts
//! (`Command::ModelInfo`, `Update::ModelInfo`) and the starred models
//! ([`crate::state::favorites`]: `Command::SetFavorites`,
//! `Update::Favorites`). It replaces the page's shared cache of
//! `/api/models` and `/api/model-info` (`shared.mjs`: asked once and shared,
//! a failure forgotten so the next ask goes again, its error kept until a
//! later answer) and `/api/config`'s providers and favourites.
//!
//! The shell creates it ([`Catalog::install`]) and hands it the four
//! updates; the hooks say what the pickers' actions do outside them
//! (Settings, Bench, the preferences, a notice).

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::*;

use crate::state::favorites::{Favorites, Key, Phase, Saved};
use crate::state::models::{Catalogue, usable};
use crate::{Command, Core, ModelFacts, Provider, Resolution, Update};

/// What a choice in the pickers asks the window to remember.
#[derive(Clone, Debug, PartialEq)]
pub enum Remember {
    /// A model picked in the composer (`mcb-last-model`).
    Model(String, String),
    /// A resolution chosen (`mcb-last-resolution`).
    Resolution(Resolution),
}

/// Saves a choice with the window's preferences.
pub type OnRemember = Rc<dyn Fn(Remember, &mut App)>;
/// Shows a notice.
pub type OnNotice = Rc<dyn Fn(String, &mut App)>;

/// What the pickers' actions do outside them.
#[derive(Clone)]
pub struct Hooks {
    /// Opens the Settings layer (the rail's Settings, Connect an AI).
    pub open_settings: crate::view::Press,
    /// The footer's `Compare several in Bench`.
    pub compare_in_bench: crate::view::Press,
    /// Saves a choice with the window's preferences.
    pub remember: OnRemember,
    /// Shows a notice (`showError`).
    pub notice: OnNotice,
    /// A model chosen in the picker that opened by itself: the composer
    /// takes focus (`onOpened`, `app.md` 6.6).
    pub chosen_after_ask: crate::view::Press,
}

/// The window's catalogue of providers, models and favourites.
pub struct Catalog {
    core: Arc<dyn Core>,
    providers: Vec<Provider>,
    providers_arrived: bool,
    catalogues: HashMap<String, Catalogue>,
    asked: HashSet<String>,
    facts: HashMap<(String, String), Result<ModelFacts, String>>,
    facts_asked: HashSet<(String, String)>,
    favorites: Favorites,
    hooks: Hooks,
    /// The chat model picker is to open by itself (`askModel`, `app.md` 2.5).
    ask_model: bool,
}

/// The shell's catalogue, for views that the shell does not build (the
/// chat pane's footer).
struct Installed(Entity<Catalog>);

impl Global for Installed {}

impl Catalog {
    /// A catalogue over `core`, empty until the core publishes.
    pub fn new(core: Arc<dyn Core>, hooks: Hooks) -> Self {
        Catalog {
            core,
            providers: Vec::new(),
            providers_arrived: false,
            catalogues: HashMap::new(),
            asked: HashSet::new(),
            facts: HashMap::new(),
            facts_asked: HashSet::new(),
            favorites: Favorites::default(),
            hooks,
            ask_model: false,
        }
    }

    /// Creates the window's catalogue and makes it the one
    /// [`Catalog::installed`] gives.
    pub fn install(core: Arc<dyn Core>, hooks: Hooks, cx: &mut App) -> Entity<Catalog> {
        let catalog = cx.new(|_| Catalog::new(core, hooks));
        cx.set_global(Installed(catalog.clone()));
        catalog
    }

    /// The catalogue the shell installed.
    pub fn installed(cx: &App) -> Entity<Catalog> {
        cx.global::<Installed>().0.clone()
    }

    /// The hooks.
    pub fn hooks(&self) -> Hooks {
        self.hooks.clone()
    }

    /// Takes one of the core's provider, model and favourite updates.
    pub fn apply(&mut self, update: Update, cx: &mut Context<Self>) {
        match update {
            // par-chat: a failed read changes nothing here (the shell's).
            Update::Providers(Err(_)) => {}
            Update::Providers(Ok(providers)) => {
                self.providers = providers;
                self.providers_arrived = true;
                // Every usable provider's catalogue is asked for, so the
                // pickers open already filled (`loadConfig`).
                let ids: Vec<String> = self
                    .providers
                    .iter()
                    .filter(|p| usable(p))
                    .map(|p| p.id.clone())
                    .collect();
                for id in ids {
                    self.ask_models(&id);
                }
                // Facts asked for before the core attached go now.
                let waiting: Vec<(String, String)> = self.facts_asked.iter().cloned().collect();
                for (provider, model) in waiting {
                    self.core.send(Command::ModelInfo { provider, model });
                }
            }
            Update::Models { provider, models } => {
                self.asked.remove(&provider);
                self.catalogues.insert(
                    provider,
                    match models {
                        Ok(models) => Catalogue::Ready(models),
                        Err(error) => Catalogue::Failed(error),
                    },
                );
            }
            Update::ModelInfo {
                provider,
                model,
                facts,
            } => {
                let key = (provider, model);
                self.facts_asked.remove(&key);
                self.facts.insert(key, facts);
            }
            Update::Favorites(list) => match self.favorites.phase() {
                Phase::Unloaded => self.favorites.load(list).expect("the favourites load once"),
                Phase::Saving => {
                    if let Some(next) = self.favorites.ended(Saved::Stored(list)) {
                        self.core.send(Command::SetFavorites(next));
                    }
                }
                // The core is the list's source: it publishes it after
                // attach and whenever it changes.
                Phase::Idle => self.favorites.published(list),
            },
            other => panic!("the catalogue takes providers, models and favourites, not {other:?}"),
        }
        cx.notify();
    }

    /// The providers, in the core's order (none before they arrive).
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }

    /// The providers have arrived.
    pub fn providers_arrived(&self) -> bool {
        self.providers_arrived
    }

    /// Some provider can be used.
    pub fn any_usable(&self) -> bool {
        self.providers.iter().any(usable)
    }

    /// The catalogues held.
    pub fn catalogues(&self) -> &HashMap<String, Catalogue> {
        &self.catalogues
    }

    /// Asks for `provider`'s catalogue unless it is held or on its way (a
    /// failed one is asked again; its error stays until the answer).
    pub fn ask_models(&mut self, provider: &str) {
        if matches!(self.catalogues.get(provider), Some(Catalogue::Ready(_)))
            || self.asked.contains(provider)
        {
            return;
        }
        self.asked.insert(provider.to_string());
        if !self.catalogues.contains_key(provider) {
            self.catalogues
                .insert(provider.to_string(), Catalogue::Loading);
        }
        self.core.send(Command::Models {
            provider: provider.to_string(),
        });
    }

    // settings agent: `Checked …` reads a shown catalogue again.
    /// Reads `provider`'s catalogue again (`{ fresh: true }`), keeping the
    /// one held on show until the answer.
    pub fn ask_models_again(&mut self, provider: &str) {
        if self.asked.contains(provider) {
            return;
        }
        self.asked.insert(provider.to_string());
        self.catalogues
            .entry(provider.to_string())
            .or_insert(Catalogue::Loading);
        self.core.send(Command::Models {
            provider: provider.to_string(),
        });
    }

    /// A model's facts, when they have arrived.
    pub fn facts(&self, provider: &str, model: &str) -> Option<&ModelFacts> {
        match self.facts.get(&(provider.to_string(), model.to_string())) {
            Some(Ok(facts)) => Some(facts),
            _ => None,
        }
    }

    /// par-shell: a model's facts have arrived or failed.
    pub fn facts_settled(&self, provider: &str, model: &str) -> bool {
        self.facts
            .contains_key(&(provider.to_string(), model.to_string()))
    }

    /// Asks for a model's facts unless they are held or on their way.
    pub fn ask_facts(&mut self, provider: &str, model: &str) {
        let key = (provider.to_string(), model.to_string());
        if matches!(self.facts.get(&key), Some(Ok(_))) || self.facts_asked.contains(&key) {
            return;
        }
        self.facts_asked.insert(key);
        // Nothing goes to the core before it attached and published its
        // providers (the pickers are made with the window): such an ask is
        // sent when the providers arrive.
        if !self.providers_arrived {
            return;
        }
        self.core.send(Command::ModelInfo {
            provider: provider.to_string(),
            model: model.to_string(),
        });
    }

    /// The starred models shown.
    pub fn favorites(&self) -> Vec<Key> {
        self.favorites.list()
    }

    /// Stars or unstars a model; before the favourites arrive it says why
    /// it cannot (a notice).
    pub fn toggle_favorite(&mut self, provider: &str, model: &str, cx: &mut Context<Self>) {
        match self
            .favorites
            .toggle((provider.to_string(), model.to_string()))
        {
            Ok(Some(list)) => self.core.send(Command::SetFavorites(list)),
            Ok(None) => {}
            Err(error) => (self.hooks.notice)(error, cx),
        }
        cx.notify();
    }

    /// Asks the chat model picker to open by itself (Settings closed with
    /// an AI connected and no model chosen, `app.md` 2.5).
    pub fn ask_model(&mut self, cx: &mut Context<Self>) {
        if self.any_usable() {
            self.ask_model = true;
            cx.notify();
        }
    }

    /// Takes the ask to open the picker by itself, if one is pending.
    pub fn take_ask(&mut self) -> bool {
        std::mem::take(&mut self.ask_model)
    }
}
