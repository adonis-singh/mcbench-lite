//! The Settings panel (`web/settings-panel.mjs`, tests ported from
//! `test/settings-panel.test.js`, same cases and names; inventory
//! `components.md` E.6 and E.10, `app.md` 8.1 and 8.5) and the texts of
//! its panes.
//!
//! The panel is one value with four phases: closed; opening while the
//! saved settings are read (on the pane asked for); open with the draft and
//! the last save's error; saving, during which there is no edit, no second
//! save and no closing: a save that lands closes the panel, one that fails
//! opens it again with its error.
//!
//! Every use of the seam's settings types ([`crate::SettingsView`],
//! [`crate::SettingsChange`]) goes through this module and
//! [`super::sign_in`], so a rename of theirs is one edit here.

use std::path::{Path, PathBuf};

use crate::{
    Auth, KeySource, ProviderChange, ProviderSettings, SettingsChange, SettingsView, TitleModel,
};

/// A pane of Settings (`view`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pane {
    /// `connect`: Connect an AI.
    Connect,
    /// `general`.
    General,
    /// A provider's pane, by its id.
    Provider(String),
}

/// `CONNECT`: the Connect an AI pane's id.
pub const CONNECT: &str = "connect";

impl Pane {
    /// Its id in the page (`connect`, `general`, the provider's).
    pub fn id(&self) -> &str {
        match self {
            Pane::Connect => CONNECT,
            Pane::General => "general",
            Pane::Provider(id) => id,
        }
    }
}

/// A provider in the draft: the core's entry, edited in place (`enabled`,
/// `hidden_models`, `model_order`), and the key typed for it.
#[derive(Clone, PartialEq)]
pub struct DraftProvider {
    /// The provider as read, with the draft's edits.
    pub entry: ProviderSettings,
    /// The key typed in Settings; empty keeps the saved one. A secret: it
    /// leaves only in [`change_of`]'s [`ProviderChange`], and `Debug`
    /// prints `***`.
    pub api_key: String,
}

impl std::fmt::Debug for DraftProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraftProvider")
            .field("entry", &self.entry)
            .field("api_key", &"***")
            .finish()
    }
}

/// The settings as edited and not yet saved.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    /// `checkedAt`, ms since the epoch.
    pub checked_at: i64,
    /// `titleModel`.
    pub title_model: Option<TitleModel>,
    /// `shareRepo`.
    pub share_repo: Option<String>,
    /// The providers, in the core's order.
    pub providers: Vec<DraftProvider>,
    /// The settings file's place (the key rows' help).
    pub settings_file: PathBuf,
}

impl Draft {
    /// The provider `id` (an id the draft lacks is an error).
    pub fn provider(&self, id: &str) -> &DraftProvider {
        self.providers
            .iter()
            .find(|p| p.entry.id == id)
            .unwrap_or_else(|| panic!("the settings have no provider {id}"))
    }

    /// The provider `id`, to edit.
    pub fn provider_mut(&mut self, id: &str) -> &mut DraftProvider {
        self.providers
            .iter_mut()
            .find(|p| p.entry.id == id)
            .unwrap_or_else(|| panic!("the settings have no provider {id}"))
    }

    /// The providers' entries, in order.
    pub fn entries(&self) -> Vec<&ProviderSettings> {
        self.providers.iter().map(|p| &p.entry).collect()
    }
}

/// The draft of settings just read (`readSettings`'s `editable`): no key
/// typed.
pub fn draft_of(view: SettingsView) -> Draft {
    Draft {
        checked_at: view.checked_at,
        title_model: view.title_model,
        share_repo: view.share_repo,
        providers: view
            .providers
            .into_iter()
            .map(|entry| DraftProvider {
                entry,
                api_key: String::new(),
            })
            .collect(),
        settings_file: view.settings_file,
    }
}

/// The draft after a check (`recheckSettings`) or a fresh read pushed by
/// the core: `checkedAt` and each provider's fresh fields, keeping the
/// draft's key, `enabled`, hidden models and order, its title model and
/// repository. A provider the draft lacks comes in as read.
pub fn refreshed(draft: &Draft, fresh: SettingsView) -> Draft {
    let providers = fresh
        .providers
        .into_iter()
        .map(
            |mut entry| match draft.providers.iter().find(|p| p.entry.id == entry.id) {
                Some(kept) => {
                    entry.enabled = kept.entry.enabled;
                    entry.hidden_models = kept.entry.hidden_models.clone();
                    entry.model_order = kept.entry.model_order.clone();
                    DraftProvider {
                        entry,
                        api_key: kept.api_key.clone(),
                    }
                }
                None => DraftProvider {
                    entry,
                    api_key: String::new(),
                },
            },
        )
        .collect();
    Draft {
        checked_at: fresh.checked_at,
        title_model: draft.title_model.clone(),
        share_repo: draft.share_repo.clone(),
        providers,
        settings_file: fresh.settings_file,
    }
}

/// What a save sends (`PUT /api/settings`'s body).
pub fn change_of(draft: &Draft) -> SettingsChange {
    SettingsChange {
        title_model: draft.title_model.clone(),
        share_repo: draft.share_repo.clone(),
        providers: draft
            .providers
            .iter()
            .map(|p| ProviderChange {
                id: p.entry.id.clone(),
                enabled: p.entry.enabled,
                api_key: p.api_key.clone(),
                hidden_models: p.entry.hidden_models.clone(),
                model_order: p.entry.model_order.clone(),
            })
            .collect(),
    }
}

/// The panel (`settings-panel.mjs`).
#[derive(Clone, Debug, PartialEq)]
pub enum Panel {
    /// `closed`.
    Closed,
    /// `opening`: the saved settings are being read, to open on `view`.
    Opening {
        /// The pane to open on.
        view: Pane,
    },
    /// `open`: the draft and why the last save failed (empty: it did not).
    Open {
        /// The pane shown.
        view: Pane,
        /// The draft.
        draft: Draft,
        /// The last save's error, or empty.
        error: String,
    },
    /// `saving`: the draft on its way.
    Saving {
        /// The pane shown.
        view: Pane,
        /// The draft.
        draft: Draft,
    },
}

impl Panel {
    /// Its phase's name.
    pub fn phase(&self) -> &'static str {
        match self {
            Panel::Closed => "closed",
            Panel::Opening { .. } => "opening",
            Panel::Open { .. } => "open",
            Panel::Saving { .. } => "saving",
        }
    }

    /// The pane, none when closed.
    pub fn view(&self) -> Option<&Pane> {
        match self {
            Panel::Closed => None,
            Panel::Opening { view } | Panel::Open { view, .. } | Panel::Saving { view, .. } => {
                Some(view)
            }
        }
    }
}

/// The panel after `panel`: `next`, if it can follow it (`panelTo`). A
/// draft only in open and saving and a pane only when not closed hold by
/// the type itself (the page checks them at run time).
pub fn panel_to(panel: &Panel, next: Panel) -> Result<Panel, String> {
    let allowed: &[&str] = match panel {
        Panel::Closed => &["opening"],
        Panel::Opening { .. } => &["opening", "open", "closed"],
        Panel::Open { .. } => &["open", "saving", "closed"],
        Panel::Saving { .. } => &["open", "closed"],
    };
    if !allowed.contains(&next.phase()) {
        return Err(format!(
            "Settings cannot go from {} to {}",
            panel.phase(),
            next.phase()
        ));
    }
    Ok(next)
}

/// The draft changed by `change`, which also lets go of the last save's
/// error (`panelEdit`). Only open settings can be edited.
pub fn panel_edit(panel: &Panel, change: impl FnOnce(&mut Draft)) -> Result<Panel, String> {
    let Panel::Open { view, draft, .. } = panel else {
        return Err(format!("Settings cannot be edited while {}", panel.phase()));
    };
    let mut draft = draft.clone();
    change(&mut draft);
    Ok(Panel::Open {
        view: view.clone(),
        draft,
        error: String::new(),
    })
}

/// The draft shown, or none: closed settings and ones still opening have
/// none (`panelDraft`).
pub fn panel_draft(panel: &Panel) -> Option<&Draft> {
    match panel {
        Panel::Open { draft, .. } | Panel::Saving { draft, .. } => Some(draft),
        _ => None,
    }
}

/// The title model the draft names: a model picked, on its default effort,
/// or none, which has each chat's own model write its title
/// (`withTitleModel`).
pub fn with_title_model(draft: &mut Draft, choice: Option<(String, String)>) {
    draft.title_model = choice.map(|(provider, model)| TitleModel {
        provider,
        model,
        effort: String::new(),
    });
}

/// The repository shared builds publish to, as typed; an empty field saves
/// none, which turns sharing off (`withShareRepo`).
pub fn with_share_repo(draft: &mut Draft, text: &str) {
    let text = text.trim();
    draft.share_repo = (!text.is_empty()).then(|| text.to_string());
}

/// A provider the app can use: connected and on in Settings (`usable`).
pub fn usable(provider: &ProviderSettings) -> bool {
    provider.connected && provider.enabled
}

/// What a CLI login's Account section shows (`cliAccount`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CliAccount {
    /// `connected`: the login and its CLI to run it.
    Connected,
    /// `install`: a login whose CLI is not on this machine.
    Install,
    /// `sign-in`: no login.
    SignIn,
}

/// `cliAccount`.
pub fn cli_account(provider: &ProviderSettings) -> CliAccount {
    if provider.connected {
        CliAccount::Connected
    } else if provider.signed_in {
        CliAccount::Install
    } else {
        CliAccount::SignIn
    }
}

/// The pane Settings open on when none is named: Connect an AI while no
/// provider can be used, General once one can (`firstPane`).
pub fn first_pane<'a>(providers: impl IntoIterator<Item = &'a ProviderSettings>) -> Pane {
    if providers.into_iter().any(usable) {
        Pane::General
    } else {
        Pane::Connect
    }
}

/// [`first_pane`] over the window's providers (`Update::Providers`).
pub fn first_pane_of(providers: &[crate::Provider]) -> Pane {
    if providers.iter().any(|p| p.connected && p.enabled) {
        Pane::General
    } else {
        Pane::Connect
    }
}

/// One way in (`CONNECT_WAYS`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Way {
    /// `key`.
    pub key: &'static str,
    /// `title`.
    pub title: &'static str,
    /// `need`: what the person needs and who bills it.
    pub need: &'static str,
    /// The provider panes that set it up, the one most people want first.
    pub panes: &'static [&'static str],
}

/// The ways in, in the page's order, without Cursor's (not in the native
/// app; inventory README).
pub const CONNECT_WAYS: [Way; 3] = [
    Way {
        key: "api-key",
        title: "An API key",
        need: "Get a key on the provider's website and paste it into its pane. OpenRouter is the one key that reaches most models; the others reach their own. Usage is billed by that provider.",
        panes: &[
            "openrouter",
            "openai",
            "anthropic",
            "google",
            "groq",
            "qwencloud",
        ],
    },
    Way {
        key: "claude",
        title: "Your Claude subscription, through Claude Code",
        need: "Sign in with your Claude subscription, or an Anthropic Console account from the Claude Code pane. The app installs Claude Code first if this machine does not have it. Usage counts against that plan or account.",
        panes: &["claude-code"],
    },
    Way {
        key: "chatgpt",
        title: "Your ChatGPT subscription, through Codex CLI or ChatGPT sign-in",
        need: "Sign in with your ChatGPT account from either; one sign-in serves both. The app installs Codex CLI first if this machine does not have it. Usage counts against your ChatGPT plan.",
        panes: &["codex-cli", "codex"],
    },
];

/// The ways with each pane's provider entry (`connectWays`); a pane the
/// core does not report is an error.
pub fn connect_ways<'a>(
    providers: &[&'a ProviderSettings],
) -> Result<Vec<(Way, Vec<&'a ProviderSettings>)>, String> {
    CONNECT_WAYS
        .iter()
        .map(|way| {
            let panes = way
                .panes
                .iter()
                .map(|id| {
                    providers
                        .iter()
                        .copied()
                        .find(|p| p.id == *id)
                        .ok_or_else(|| format!("No provider {id} to connect"))
                })
                .collect::<Result<Vec<_>, String>>()?;
            Ok((*way, panes))
        })
        .collect()
}

/// A Connect pane button's state text: `Sign in` for a CLI login with no
/// login, else the core's summary.
pub fn connect_state(provider: &ProviderSettings) -> &str {
    if provider.auth == Auth::CliLogin && cli_account(provider) == CliAccount::SignIn {
        "Sign in"
    } else {
        &provider.summary
    }
}

/// A Connect pane button opens the provider's pane and starts its sign-in
/// (or install): a CLI login not connected.
pub fn connect_starts(provider: &ProviderSettings) -> bool {
    provider.auth == Auth::CliLogin && !provider.connected
}

// ---------------------------------------------------------------------
// The texts of the panes that today name the server, the browser or
// `data/settings.json`, worded for one app (the stage-four brief, agreed
// with stage three 2026-11-17 18:20): the settings file's real place comes
// from the core (`SettingsView::settings_file`), the help says "your
// browser" and "the app's environment", and the `codex` note names the app.
// They reach a person, never a model.
// ---------------------------------------------------------------------

/// The API key row's help (`ProviderPane`): saved, from the environment,
/// or neither.
pub fn key_help(source: Option<KeySource>, env_key: &str, settings_file: &Path) -> String {
    match source {
        Some(KeySource::Saved) => format!(
            "Saved in {}. Paste a new key to replace it.",
            settings_file.display()
        ),
        Some(KeySource::Env) => {
            format!("Read from {env_key}. A key saved here takes precedence.")
        }
        None => format!(
            "Stored in {} and never shown again. {env_key} works too.",
            settings_file.display()
        ),
    }
}

/// The key field's placeholder: dots while a key is saved.
pub fn key_placeholder(source: Option<KeySource>) -> &'static str {
    match source {
        Some(KeySource::Saved) => {
            "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}"
        }
        _ => "Paste your key",
    }
}

/// The Binary row's help.
pub fn binary_help(binary_env: &str, binary: &str) -> String {
    format!(
        "Set by {binary_env} in the app's environment; unset, {binary} on PATH, else the one the app installed."
    )
}

/// The Config directory row's help.
pub fn home_help(home_env: &str) -> String {
    format!("Set by {home_env} in the app's environment. Login and sessions live here.")
}

/// `RUNTIME_NOTES`: the rows at the end of a pane's Runtime section
/// (Cursor's are not in the app).
pub fn runtime_notes(id: &str) -> &'static [(&'static str, &'static str)] {
    match id {
        "claude-code" => &[
            (
                "Tools",
                "Built-in tools removed; mcbench's tools over MCP are the only ones.",
            ),
            (
                "Sessions",
                "One Claude Code session per chat, resumed on later turns.",
            ),
        ],
        "codex-cli" => &[
            (
                "Tools",
                "Shell, file, web and agent features off; mcbench's tools over MCP.",
            ),
            (
                "Isolation",
                "Linux: a container with no network but the model API. macOS: on the host, read-only sandbox.",
            ),
            (
                "Sessions",
                "One Codex thread per chat, resumed on later turns.",
            ),
        ],
        "codex" => &[(
            "Requests",
            "Sent with your Codex login from inside the app.",
        )],
        _ => &[],
    }
}

/// The Loop row: shown for a provider that is not a harness and not
/// `codex`.
pub const LOOP: (&str, &str) = (
    "Loop",
    "mcbench's own turn loop calls this API with mcbench's tools.",
);

/// `SIGN_IN_METHODS`: a method's name on its button.
pub fn sign_in_method(method: &str) -> &'static str {
    match method {
        "claudeai" => "Claude subscription",
        "console" => "Anthropic Console",
        other => panic!("no sign-in method {other}: claudeai or console"),
    }
}

/// `SIGN_IN_HELP`, worded for the app (`opens in your browser`).
pub fn sign_in_help(id: &str) -> &'static str {
    match id {
        "claude-code" => {
            "Sign in with your Claude subscription or with an Anthropic Console account. The sign-in page opens in your browser."
        }
        "codex" | "codex-cli" => {
            "Sign in with your ChatGPT account. The sign-in page opens in your browser and asks for a one-time code shown here."
        }
        other => panic!("{other} has no sign-in"),
    }
}

/// `LOGIN_THROUGH`: what a CLI login is, beside its account.
pub fn login_through(id: &str) -> &'static str {
    match id {
        "claude-code" => "Claude login through Claude Code",
        "codex" | "codex-cli" => "ChatGPT login through the Codex CLI",
        other => panic!("{other} has no CLI login"),
    }
}

/// `SIGN_IN_NEXT`: what to do on the sign-in page.
pub fn sign_in_next(id: &str) -> &'static str {
    match id {
        "claude-code" => "Sign in there. The page then shows a code: paste it below.",
        "codex" | "codex-cli" => {
            "Sign in there and enter the one-time code below. This pane updates when you are done."
        }
        other => panic!("{other} has no sign-in"),
    }
}

/// A signed-in CLI login's help: its plan and what it is, the missing parts
/// left out.
pub fn account_help(provider: &ProviderSettings) -> String {
    [provider.plan.as_deref(), Some(login_through(&provider.id))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" \u{b7} ")
}

/// The Models section's line for a provider not connected.
pub fn models_not_connected(provider: &ProviderSettings) -> String {
    let what = if provider.env_key.is_some() {
        "Add a key".to_string()
    } else if cli_account(provider) == CliAccount::Install {
        format!(
            "Install {}",
            provider
                .sign_in
                .as_ref()
                .expect("a CLI login has a sign-in")
                .program
                .title
        )
    } else {
        "Sign in".to_string()
    };
    format!("{what} to list {}'s models.", provider.label)
}

/// `ago(iso)`: how long ago `at` was (ms), as the re-check button says it:
/// `just now` under 10 s, then seconds, minutes, hours.
pub fn ago(at: i64, now: i64) -> String {
    let seconds = ((now - at) as f64 / 1000.).round().max(0.) as i64;
    if seconds < 10 {
        "just now".into()
    } else if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else {
        format!("{}h ago", seconds / 3600)
    }
}

/// When [`ago`]'s text next changes (ms since the epoch): the coarsest
/// step the text needs, so the `Checked` clock wakes no more than that.
pub fn ago_changes_at(at: i64, now: i64) -> i64 {
    let elapsed = (now - at).max(0);
    // `ago` rounds to the nearest second: a second's text starts half a
    // second before it.
    let second = |n: i64| at + n * 1000 - 500;
    let seconds = ((elapsed as f64) / 1000.).round() as i64;
    if seconds < 10 {
        second(10)
    } else if seconds < 60 {
        second(seconds + 1)
    } else if seconds < 3600 {
        second((seconds / 60 + 1) * 60)
    } else {
        second((seconds / 3600 + 1) * 3600)
    }
}

/// The vote count the danger zone read (`DangerZone`'s `votes`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Votes {
    /// Counting (`null`).
    Counting,
    /// The count.
    Count(u64),
    /// The read failed with this.
    Unknown(String),
}

/// The confirm's question.
pub fn votes_question(votes: &Votes) -> String {
    match votes {
        Votes::Unknown(_) => "Delete every vote?".into(),
        Votes::Count(1) => "Delete the only vote?".into(),
        Votes::Count(n) => format!("Delete all {n} votes?"),
        // par-shell (E.3): the page's button is disabled while counting
        // (`votes === null`), so its question is never asked then.
        Votes::Counting => panic!("the danger zone asks only once the votes are counted"),
    }
}

/// The danger zone's button: its text and whether it is enabled.
pub fn votes_button(votes: &Votes) -> (&'static str, bool) {
    match votes {
        Votes::Counting => ("Clear leaderboard", false),
        Votes::Count(0) => ("No votes", false),
        _ => ("Clear leaderboard", true),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn entry(id: &str, connected: bool, enabled: bool) -> ProviderSettings {
        ProviderSettings {
            id: id.into(),
            label: "x".into(),
            auth: Auth::ApiKey,
            harness: false,
            env_key: None,
            connected,
            enabled,
            key_source: None,
            signed_in: false,
            account: None,
            plan: None,
            version: None,
            summary: String::new(),
            hidden_models: Vec::new(),
            model_order: Vec::new(),
            runtime: None,
            sign_in: None,
            endpoint: None,
            protocol: String::new(),
        }
    }

    fn draft() -> Draft {
        Draft {
            checked_at: 0,
            title_model: Some(TitleModel {
                provider: "p".into(),
                model: "m".into(),
                effort: String::new(),
            }),
            share_repo: None,
            providers: Vec::new(),
            settings_file: PathBuf::from("settings.json"),
        }
    }

    fn open(view: Pane, draft: Draft, error: &str) -> Panel {
        Panel::Open {
            view,
            draft,
            error: error.into(),
        }
    }

    #[test]
    fn the_panel_opens_from_a_read_saves_once_and_closes_only_when_not_saving() {
        let draft = draft();
        let mut panel = Panel::Closed;
        assert_eq!(panel_draft(&panel), None);
        panel = panel_to(
            &panel,
            Panel::Opening {
                view: Pane::General,
            },
        )
        .unwrap();
        // Another pane asked for while the read is on its way.
        panel = panel_to(
            &panel,
            Panel::Opening {
                view: Pane::Provider("openrouter".into()),
            },
        )
        .unwrap();
        assert_eq!(panel_draft(&panel), None);
        let view = panel.view().unwrap().clone();
        panel = panel_to(&panel, open(view.clone(), draft.clone(), "")).unwrap();
        assert_eq!(panel_draft(&panel), Some(&draft));
        panel = panel_to(
            &panel,
            Panel::Saving {
                view: view.clone(),
                draft: draft.clone(),
            },
        )
        .unwrap();
        assert_eq!(panel_draft(&panel), Some(&draft));
        // No second save, and no opening anew, while one is on its way.
        let saving = Panel::Saving {
            view: view.clone(),
            draft: draft.clone(),
        };
        assert!(
            panel_to(&panel, saving)
                .unwrap_err()
                .contains("cannot go from saving to saving")
        );
        assert!(
            panel_to(
                &panel,
                Panel::Opening {
                    view: Pane::General
                }
            )
            .unwrap_err()
            .contains("cannot go from saving to opening")
        );
        // A failed save opens the panel again with its error; one that
        // lands closes it.
        let failed = panel_to(&panel, open(view, draft.clone(), "Choose a title model")).unwrap();
        assert!(matches!(&failed, Panel::Open { error, .. } if error == "Choose a title model"));
        assert_eq!(panel_to(&panel, Panel::Closed).unwrap(), Panel::Closed);
        // A read that fails closes the opening.
        assert_eq!(
            panel_to(
                &Panel::Opening {
                    view: Pane::General
                },
                Panel::Closed
            )
            .unwrap(),
            Panel::Closed
        );
        assert!(
            panel_to(&Panel::Closed, open(Pane::General, draft.clone(), ""))
                .unwrap_err()
                .contains("cannot go from closed to open")
        );
        assert!(
            panel_to(
                &Panel::Closed,
                Panel::Saving {
                    view: Pane::General,
                    draft
                }
            )
            .unwrap_err()
            .contains("cannot go from closed to saving")
        );
        // The page's two last cases (an open panel without a draft, a
        // closed one on a pane) cannot be written: the type holds a draft
        // in open and saving only, and a pane everywhere but closed.
    }

    #[test]
    fn only_open_settings_are_edited_and_an_edit_lets_go_of_the_last_saves_error() {
        let draft = draft();
        let panel = open(Pane::General, draft.clone(), "Choose a title model");
        let edited = panel_edit(&panel, |d| {
            d.title_model = Some(TitleModel {
                provider: "p".into(),
                model: "n".into(),
                effort: String::new(),
            })
        })
        .unwrap();
        let mut expected = draft.clone();
        expected.title_model.as_mut().unwrap().model = "n".into();
        assert_eq!(edited, open(Pane::General, expected, ""));
        let saving = Panel::Saving {
            view: Pane::General,
            draft,
        };
        assert!(
            panel_edit(&saving, |_| {})
                .unwrap_err()
                .contains("cannot be edited while saving")
        );
        assert!(
            panel_edit(&Panel::Closed, |_| {})
                .unwrap_err()
                .contains("cannot be edited while closed")
        );
        assert!(
            panel_edit(
                &Panel::Opening {
                    view: Pane::General
                },
                |_| {}
            )
            .unwrap_err()
            .contains("cannot be edited while opening")
        );
    }

    #[test]
    fn the_title_model_is_a_model_picked_or_null_for_the_chats_own_model_and_saves_as_such() {
        let mut d = draft();
        d.title_model = None;
        let mut panel = panel_to(
            &Panel::Opening {
                view: Pane::General,
            },
            open(Pane::General, d, ""),
        )
        .unwrap();
        panel = panel_edit(&panel, |d| {
            with_title_model(d, Some(("google".into(), "gemini-2.5-flash".into())))
        })
        .unwrap();
        assert_eq!(
            panel_draft(&panel).unwrap().title_model,
            Some(TitleModel {
                provider: "google".into(),
                model: "gemini-2.5-flash".into(),
                effort: String::new()
            })
        );
        panel = panel_edit(&panel, |d| with_title_model(d, None)).unwrap();
        assert_eq!(panel_draft(&panel).unwrap().title_model, None);
        assert_eq!(change_of(panel_draft(&panel).unwrap()).title_model, None);
    }

    #[test]
    fn the_repository_for_shared_builds_saves_as_typed_and_empty_saves_null() {
        let mut panel = panel_to(
            &Panel::Opening {
                view: Pane::General,
            },
            open(Pane::General, draft(), ""),
        )
        .unwrap();
        panel = panel_edit(&panel, |d| with_share_repo(d, " owner/builds ")).unwrap();
        assert_eq!(
            panel_draft(&panel).unwrap().share_repo.as_deref(),
            Some("owner/builds")
        );
        panel = panel_edit(&panel, |d| with_share_repo(d, "  ")).unwrap();
        assert_eq!(change_of(panel_draft(&panel).unwrap()).share_repo, None);
    }

    #[test]
    fn settings_open_on_connect_an_ai_while_no_provider_can_be_used_and_on_general_once_one_can() {
        assert_eq!(CONNECT, "connect");
        assert_eq!(first_pane(&[]), Pane::Connect);
        let off = entry("openrouter", false, true);
        assert_eq!(first_pane([&off]), Pane::Connect);
        // Connected but off in Settings is not usable.
        let unused = entry("openrouter", true, false);
        assert!(!usable(&unused));
        assert_eq!(first_pane([&unused]), Pane::Connect);
        let claude = entry("claude-code", true, true);
        assert_eq!(first_pane([&off, &claude]), Pane::General);
        // Connect is a pane like any other: the panel opens on it and turns
        // from it.
        let mut panel = panel_to(
            &Panel::Closed,
            Panel::Opening {
                view: Pane::Connect,
            },
        )
        .unwrap();
        panel = panel_to(&panel, open(Pane::Connect, draft(), "")).unwrap();
        let turned = panel_to(
            &panel,
            open(Pane::Provider("openrouter".into()), draft(), ""),
        )
        .unwrap();
        assert_eq!(turned.view(), Some(&Pane::Provider("openrouter".into())));
    }

    #[test]
    fn the_ways_to_connect_name_every_provider_pane_once_each_with_its_entry() {
        let every = [
            "openrouter",
            "openai",
            "anthropic",
            "google",
            "groq",
            "qwencloud",
            "claude-code",
            "codex-cli",
            "codex",
        ];
        let mut panes: Vec<&str> = CONNECT_WAYS
            .iter()
            .flat_map(|w| w.panes.iter().copied())
            .collect();
        panes.sort();
        let mut sorted = every.to_vec();
        sorted.sort();
        assert_eq!(panes, sorted);
        assert_eq!(CONNECT_WAYS[0].panes[0], "openrouter");
        let providers: Vec<ProviderSettings> = every
            .iter()
            .map(|id| entry(id, *id == "codex", true))
            .collect();
        let refs: Vec<&ProviderSettings> = providers.iter().collect();
        let ways = connect_ways(&refs).unwrap();
        assert_eq!(
            ways.iter().map(|(w, _)| w.key).collect::<Vec<_>>(),
            ["api-key", "claude", "chatgpt"]
        );
        assert_eq!(ways[2].1[1].id, "codex");
        assert!(ways[2].1[1].connected);
        // A pane the core does not report fails, not a way quietly missing.
        let without: Vec<&ProviderSettings> = refs
            .iter()
            .copied()
            .filter(|p| p.id != "codex-cli")
            .collect();
        assert_eq!(
            connect_ways(&without).unwrap_err(),
            "No provider codex-cli to connect"
        );
    }

    #[test]
    fn a_check_keeps_the_drafts_edits_and_takes_the_fresh_fields() {
        let mut first = entry("openrouter", false, true);
        first.summary = "No key".into();
        let mut d = draft_of(SettingsView {
            checked_at: 1,
            title_model: None,
            share_repo: Some("o/r".into()),
            providers: vec![first.clone()],
            settings_file: PathBuf::from("a/settings.json"),
        });
        d.provider_mut("openrouter").api_key = "sk-test-0000".into();
        d.provider_mut("openrouter").entry.enabled = false;
        d.provider_mut("openrouter").entry.hidden_models = vec!["m".into()];
        let mut fresh = first;
        fresh.connected = true;
        fresh.summary = "Key saved".into();
        let next = refreshed(
            &d,
            SettingsView {
                checked_at: 2,
                title_model: None,
                share_repo: None,
                providers: vec![fresh],
                settings_file: PathBuf::from("a/settings.json"),
            },
        );
        let p = next.provider("openrouter");
        assert_eq!(next.checked_at, 2);
        assert_eq!(next.share_repo.as_deref(), Some("o/r"));
        assert!(p.entry.connected && !p.entry.enabled);
        assert_eq!(p.entry.summary, "Key saved");
        assert_eq!(p.entry.hidden_models, ["m"]);
        assert_eq!(p.api_key, "sk-test-0000");
        assert_eq!(change_of(&next).providers[0].api_key, "sk-test-0000");
    }

    #[test]
    fn a_typed_key_never_prints() {
        let mut d = draft();
        d.providers.push(DraftProvider {
            entry: entry("openai", false, true),
            api_key: "sk-test-0000".into(),
        });
        let change = change_of(&d);
        for text in [
            format!("{d:?}"),
            format!("{change:?}"),
            format!(
                "{:?}",
                Panel::Open {
                    view: Pane::General,
                    draft: d.clone(),
                    error: String::new()
                }
            ),
        ] {
            assert!(!text.contains("sk-test-0000"), "{text}");
            assert!(text.contains("***"));
        }
    }

    #[test]
    fn checked_reads_its_largest_unit_and_wakes_when_the_text_changes() {
        let at = 1_000_000;
        assert_eq!(ago(at, at + 9_000), "just now");
        assert_eq!(ago(at, at + 12_000), "12s ago");
        assert_eq!(ago(at, at + 61_000), "1m ago");
        assert_eq!(ago(at, at + 2 * 86_400_000), "48h ago");
        for now in [
            at,
            at + 9_400,
            at + 12_000,
            at + 59_000,
            at + 61_000,
            at + 3_700_000,
        ] {
            let next = ago_changes_at(at, now);
            assert!(next > now, "{now}");
            assert_ne!(ago(at, next - 1), ago(at, next), "{now}");
            assert_eq!(ago(at, now), ago(at, next - 1), "{now}");
        }
    }

    #[test]
    fn the_danger_zone_asks_by_the_count() {
        assert_eq!(votes_question(&Votes::Count(1)), "Delete the only vote?");
        assert_eq!(votes_question(&Votes::Count(7)), "Delete all 7 votes?");
        assert_eq!(
            votes_question(&Votes::Unknown("x".into())),
            "Delete every vote?"
        );
        assert_eq!(votes_button(&Votes::Counting), ("Clear leaderboard", false));
        assert_eq!(votes_button(&Votes::Count(0)), ("No votes", false));
        assert_eq!(votes_button(&Votes::Count(3)), ("Clear leaderboard", true));
        assert_eq!(
            votes_button(&Votes::Unknown("x".into())),
            ("Clear leaderboard", true)
        );
    }
}
