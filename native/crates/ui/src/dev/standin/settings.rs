//! The stand-in's Settings, sign-ins, CLI installs, setup steps, vote count
//! and published builds (settings agent): every command of the settings
//! seam answered from recorded data, the seam's two rules kept (a read's
//! answer carries a `Result`; a change ends in the update that carries its
//! state, `Update::Error` before it on a failure), every failure on demand.
//!
//! Flows that take time (a sign-in, an install, setup) move one step per
//! [`StandIn::settings_tick`]: the dev binary ticks on a clock
//! (`--settings-step-ms`), the tests by hand. What it plays is chosen by
//! [`Play`]s (the dev binary's `--settings <play>`).

use std::collections::HashMap;
use std::path::PathBuf;

use super::{StandIn, now_ms};
use crate::{
    Auth, Command, InstallView, KeySource, Program, Progress, Provider, ProviderSettings,
    Published, PublishedBuild, Runtime, SettingsView, SignInOffer, SignInState, SignInView,
    TitleModel, Update,
};
use mcbench_setup::{Bytes, Step, StepState};

/// What the stand-in plays (`--settings <play>`, repeated).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Play {
    /// `claude-sign-in`: Claude Code not installed and not signed in; its
    /// sign-in installs it first (MB progress), then starts, waits with the
    /// address, wants the pasted code, and is done.
    ClaudeSignIn,
    /// `chatgpt-sign-in`: ChatGPT and Codex CLI not signed in; the sign-in
    /// shows a one-time code and is done three steps later.
    ChatgptSignIn,
    /// `sign-in-fails`: every sign-in ends failed (not finished within 10
    /// minutes).
    SignInFails,
    /// `sign-in-cancelled`: every sign-in ends cancelled by the CLI.
    SignInCancelled,
    /// `install-fails`: an install fails half way.
    InstallFails,
    /// `install-refused`: an install is refused before it starts.
    InstallRefused,
    /// `save-refused`: a save is refused.
    SaveRefused,
    /// `read-fails`: reading the settings fails.
    ReadFails,
    /// `shares-empty`: no builds published.
    SharesEmpty,
    /// `shares-no-repo`: no repository set.
    SharesNoRepo,
    /// `shares-failed`: the core answers that sharing has not joined.
    SharesFailed,
    /// `remove-fails`: a removal fails.
    RemoveFails,
    /// `votes-0`, `votes-1`, `votes-unreadable`; many (12) by default.
    VotesNone,
    /// `votes-1`.
    VotesOne,
    /// `votes-unreadable`.
    VotesUnreadable,
    /// `clear-fails`: clearing the votes fails.
    ClearFails,
    /// `setup-play`: setup from nothing, each step through its states with
    /// download progress.
    SetupPlay,
    /// `setup-fails`: a needed step fails; `RetrySetup` runs it again and
    /// it finishes.
    SetupFails,
    /// `setup-optional-fails`: the optional step fails; the app is ready.
    SetupOptionalFails,
    /// `no-ai`: no provider connected (no key, no login).
    NoAi,
    /// `setup-wsl`: the model's shell needs WSL; `InstallWsl` runs the
    /// installer (a step) and WSL is ready with no restart.
    SetupWsl,
    /// `setup-wsl-restart`: WSL is installed and Windows needs a restart;
    /// `RestartComputer` is taken and nothing follows (a program holds the
    /// restart up): nothing is published.
    SetupWslRestart,
    /// `setup-wsl-refused`: the installer ends without the administrator's
    /// permission.
    SetupWslRefused,
    // s4-needs: the same three with setup otherwise ready (the shell is
    // optional: nothing is held).
    /// `setup-wsl-optional`: `setup-wsl` with every other step done.
    SetupWslOptional,
    /// `setup-wsl-optional-restart`: `setup-wsl-restart` with every other
    /// step done.
    SetupWslOptionalRestart,
    /// `setup-wsl-optional-refused`: `setup-wsl-refused` with every other
    /// step done.
    SetupWslOptionalRefused,
}

/// Where the stand-in's WSL is; the shell's step carries its `Needs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Wsl {
    /// Not installed (`Needs::InstallWsl`).
    NotInstalled,
    /// The installer runs.
    Installing,
    /// Installed, Windows not restarted (`Needs::Restart`).
    NeedsRestart,
    /// Ready.
    Ready,
    /// The installer ended without the permission.
    Refused,
}

/// The six sentences of `ui-seam-wsl-install.md`, as the core writes them.
pub const WSL_NEEDED: &str = "To use bash and build, Windows needs WSL. Install it here: Windows asks for an administrator's permission once, and for a restart when its virtualisation feature was off.";
/// Sentence 2.
pub const WSL_INSTALLING: &str =
    "Installing WSL. Windows is asking for an administrator's permission.";
/// Sentence 3.
pub const WSL_RESTART: &str =
    "WSL is installed. Restart Windows to finish; the app opens again by itself and carries on.";
/// Sentence 4.
pub const WSL_REFUSED: &str =
    "WSL was not installed: the administrator's permission was not given.";

impl Play {
    /// The play of a `--settings` value.
    pub fn named(name: &str) -> Play {
        match name {
            "claude-sign-in" => Play::ClaudeSignIn,
            "chatgpt-sign-in" => Play::ChatgptSignIn,
            "sign-in-fails" => Play::SignInFails,
            "sign-in-cancelled" => Play::SignInCancelled,
            "install-fails" => Play::InstallFails,
            "install-refused" => Play::InstallRefused,
            "save-refused" => Play::SaveRefused,
            "read-fails" => Play::ReadFails,
            "shares-empty" => Play::SharesEmpty,
            "shares-no-repo" => Play::SharesNoRepo,
            "shares-failed" => Play::SharesFailed,
            "remove-fails" => Play::RemoveFails,
            "votes-0" => Play::VotesNone,
            "votes-1" => Play::VotesOne,
            "votes-unreadable" => Play::VotesUnreadable,
            "clear-fails" => Play::ClearFails,
            "setup-play" => Play::SetupPlay,
            "setup-fails" => Play::SetupFails,
            "setup-optional-fails" => Play::SetupOptionalFails,
            "no-ai" => Play::NoAi,
            "setup-wsl" => Play::SetupWsl,
            "setup-wsl-restart" => Play::SetupWslRestart,
            "setup-wsl-refused" => Play::SetupWslRefused,
            "setup-wsl-optional" => Play::SetupWslOptional,
            "setup-wsl-optional-restart" => Play::SetupWslOptionalRestart,
            "setup-wsl-optional-refused" => Play::SetupWslOptionalRefused,
            other => panic!(
                "--settings takes claude-sign-in, chatgpt-sign-in, sign-in-fails, sign-in-cancelled, install-fails, install-refused, save-refused, read-fails, shares-empty, shares-no-repo, shares-failed, remove-fails, votes-0, votes-1, votes-unreadable, clear-fails, setup-play, setup-fails, setup-optional-fails, no-ai, setup-wsl, setup-wsl-restart, setup-wsl-refused, setup-wsl-optional, setup-wsl-optional-restart or setup-wsl-optional-refused, not {other}"
            ),
        }
    }
}

/// The sentence of a refused save (the core's words for a write it could
/// not make).
pub const SAVE_REFUSED: &str =
    "The settings were not saved: settings.json.tmp already exists. Remove it and save again.";
/// The sentence of a read that fails.
pub const READ_FAILED: &str =
    "The settings could not be read: settings.json is not JSON (a fixture error)";
/// The sentence the core gives while sharing is not in the app.
pub const SHARING_NOT_JOINED: &str = "Sharing has not joined the native app yet";
/// A removal that fails.
pub const REMOVE_FAILED: &str = "Wait for the build to finish publishing";
/// A clear that fails.
pub const CLEAR_FAILED: &str =
    "The votes could not be deleted: the store is busy (a fixture error)";
/// An install refused before it starts (`install-refused`).
pub const INSTALL_REFUSED: &str =
    "Claude Code could not be installed: the tools folder is not writable (a fixture error)";
/// An install that fails.
pub const INSTALL_FAILED: &str = "the download's checksum did not match (a fixture error)";
/// A sign-in that ran out of time (`server/provider-login.js:122`).
pub const SIGN_IN_TIMED_OUT: &str = "The sign-in was not finished within 10 minutes, so it was ended. Start it again when you are ready.";
/// The setup step that fails (`setup-fails`).
pub const SETUP_FAILURE: &str =
    "fetch failed: see https://www.minecraft.net/en-us/download/server. (a fixture error)";
/// The address a sign-in opens.
pub const SIGN_IN_URL: &str = "https://claude.ai/oauth/authorize?code=true&client_id=stand-in";
/// Codex's sign-in address.
pub const CODEX_URL: &str = "https://auth.openai.com/codex/device";
/// Codex's one-time code.
pub const USER_CODE: &str = "WXYZ-2026";
/// What Claude Code's sign-in prints while it waits for the code.
pub const PASTE_PROMPT: &str = "Paste code here if prompted >";

/// The stand-in's Settings.
pub(super) struct State {
    plays: Vec<Play>,
    entries: Vec<ProviderSettings>,
    title_model: Option<TitleModel>,
    share_repo: Option<String>,
    checked_at: i64,
    settings_file: PathBuf,
    /// Saved keys, kept here and never published.
    keys: HashMap<String, String>,
    pub(super) votes: Result<u64, String>,
    shares: Result<Published, String>,
    sign_ins: HashMap<String, SignInView>,
    /// Steps a sign-in has been waiting.
    waited: HashMap<String, u32>,
    installs: HashMap<String, InstallView>,
    /// Setup plays: ticks into the play.
    setup_tick: Option<u32>,
    /// The tick of the last `RetrySetup`.
    retried: Option<u32>,
    /// The WSL plays' state.
    wsl: Option<Wsl>,
    /// `RestartComputer` requests taken (the stand-in restarts nothing).
    restarts: u32,
    /// The installer has run one step.
    wsl_stepped: bool,
}

// share agent: the Share panel's stand-in reads and changes Settings' list.
impl State {
    /// The saved repository for shared builds.
    pub(super) fn repo(&self) -> Option<String> {
        self.share_repo.clone()
    }

    /// The published builds.
    pub(super) fn published(&self) -> &Result<Published, String> {
        &self.shares
    }

    /// The published builds, to change.
    pub(super) fn published_mut(&mut self) -> &mut Result<Published, String> {
        &mut self.shares
    }
}
// end of the share agent's block

impl State {
    pub(super) fn new(providers: &[Provider]) -> State {
        State {
            plays: Vec::new(),
            entries: providers.iter().map(entry_of).collect(),
            title_model: None,
            share_repo: Some("mcbench-builds/gallery".into()),
            checked_at: now_ms(),
            settings_file: PathBuf::from("stand-in/settings.json"),
            keys: HashMap::new(),
            votes: Ok(12),
            shares: Ok(published_fixture()),
            sign_ins: HashMap::new(),
            waited: HashMap::new(),
            installs: HashMap::new(),
            setup_tick: None,
            retried: None,
            wsl: None,
            restarts: 0,
            wsl_stepped: false,
        }
    }

    fn plays(&self, play: Play) -> bool {
        self.plays.contains(&play)
    }

    /// A WSL play with every other step done.
    fn wsl_optional(&self) -> bool {
        [
            Play::SetupWslOptional,
            Play::SetupWslOptionalRestart,
            Play::SetupWslOptionalRefused,
        ]
        .into_iter()
        .any(|p| self.plays(p))
    }

    fn entry(&mut self, id: &str) -> &mut ProviderSettings {
        self.entries
            .iter_mut()
            .find(|e| e.id == id)
            .unwrap_or_else(|| panic!("the stand-in has no provider {id}"))
    }

    fn view(&self) -> SettingsView {
        let mut providers = self.entries.clone();
        for p in &mut providers {
            p.summary = summary(p);
            if let Some(offer) = p.sign_in.as_mut() {
                offer.current = self.sign_ins.get(&p.id).cloned();
                offer.install = self.installs.get(&offer.program.name).cloned();
            }
        }
        SettingsView {
            checked_at: self.checked_at,
            title_model: self.title_model.clone(),
            share_repo: self.share_repo.clone(),
            providers,
            settings_file: self.settings_file.clone(),
        }
    }
}

/// `summary` (`server/settings.js:239-248`), without Cursor's lines.
fn summary(entry: &ProviderSettings) -> String {
    if !entry.enabled {
        return "Disabled".into();
    }
    if entry.auth == Auth::CliLogin && entry.signed_in && !entry.connected {
        return "CLI not installed".into();
    }
    if let Some(account) = &entry.account {
        return match &entry.plan {
            Some(plan) => format!("Signed in \u{b7} {plan}"),
            None => format!("Signed in as {account}"),
        };
    }
    match (entry.key_source, &entry.env_key) {
        (Some(KeySource::Saved), _) => "Key saved".into(),
        (Some(KeySource::Env), Some(env)) => format!("Key from {env}"),
        (_, Some(_)) => "No key".into(),
        _ => "Not signed in".into(),
    }
}

fn program(id: &str) -> Option<Program> {
    match id {
        "claude-code" => Some(Program {
            name: "claude".into(),
            title: "Claude Code 2.1.311".into(),
            bytes: 236_418_112,
        }),
        "codex" | "codex-cli" => Some(Program {
            name: "codex".into(),
            title: "Codex CLI 0.169.0".into(),
            bytes: 61_204_884,
        }),
        _ => None,
    }
}

/// A provider's Settings entry, from the pickers' fixture provider and the
/// server's table (`server/settings.js:26-37`).
fn entry_of(p: &Provider) -> ProviderSettings {
    let env_key = (p.auth == Auth::ApiKey).then(|| format!("{}_API_KEY", p.id.to_uppercase()));
    let (protocol, endpoint) = match p.id.as_str() {
        "openrouter" => ("Chat Completions", Some("https://openrouter.ai/api/v1")),
        "openai" => ("Responses", Some("https://api.openai.com/v1")),
        "codex" => ("Responses", None),
        "anthropic" => ("Messages", Some("https://api.anthropic.com/v1")),
        "groq" => ("Chat Completions", Some("https://api.groq.com/openai/v1")),
        "google" => (
            "Chat Completions",
            Some("https://generativelanguage.googleapis.com/v1beta/openai"),
        ),
        "qwencloud" => (
            "Chat Completions",
            Some("https://dashscope-intl.aliyuncs.com/compatible-mode/v1"),
        ),
        "claude-code" => ("Claude Code CLI over MCP", None),
        "codex-cli" => ("Codex CLI over MCP", None),
        other => panic!("the stand-in has no provider {other}"),
    };
    let runtime = match p.id.as_str() {
        "claude-code" => Some(Runtime {
            binary: "claude".into(),
            binary_env: "MCBENCH_CLAUDE_BINARY".into(),
            home_env: "CLAUDE_CONFIG_DIR".into(),
            binary_value: None,
            resolved_home: "stand-in/claude".into(),
        }),
        "codex-cli" => Some(Runtime {
            binary: "codex".into(),
            binary_env: "MCBENCH_CODEX_BINARY".into(),
            home_env: "CODEX_HOME".into(),
            binary_value: None,
            resolved_home: "stand-in/codex".into(),
        }),
        _ => None,
    };
    // Who is signed in: Claude Code on Max, ChatGPT on Plus (Codex CLI shares
    // its login but the fixture lists it not connected: signed out).
    let (signed_in, account, plan, version) = match p.id.as_str() {
        "claude-code" => (
            true,
            Some("builder@example.com".to_string()),
            Some("Max".to_string()),
            Some("2.1.311".to_string()),
        ),
        "codex" => (
            true,
            Some("builder@example.com".to_string()),
            Some("Plus".to_string()),
            Some("0.169.0".to_string()),
        ),
        "codex-cli" => (false, None, None, Some("0.169.0".to_string())),
        _ => (false, None, None, None),
    };
    let key_source = match (p.auth, p.id.as_str(), p.connected) {
        (Auth::ApiKey, "anthropic", true) => Some(KeySource::Env),
        (Auth::ApiKey, _, true) => Some(KeySource::Saved),
        _ => None,
    };
    let sign_in = program(&p.id).map(|program| SignInOffer {
        methods: if p.id == "claude-code" {
            vec!["claudeai".into(), "console".into()]
        } else {
            Vec::new()
        },
        program,
        installed: version.is_some(),
        current: None,
        install: None,
    });
    ProviderSettings {
        id: p.id.clone(),
        label: p.label.clone(),
        auth: p.auth,
        harness: p.harness,
        env_key,
        connected: p.connected,
        enabled: p.enabled,
        key_source,
        signed_in,
        account,
        plan,
        version,
        summary: String::new(),
        hidden_models: p.hidden_models.clone(),
        model_order: p.model_order.clone(),
        runtime,
        sign_in,
        endpoint: endpoint.map(str::to_string),
        protocol: protocol.into(),
    }
}

/// The published builds of the fixture: a chat, a bench, a chat whose
/// source was deleted, an archived bench.
fn published_fixture() -> Published {
    let day = 86_400_000;
    let now = now_ms();
    let build = |id: &str, title: &str, kind: &str, labels: &[&str], ago: i64| PublishedBuild {
        share_id: id.into(),
        title: title.into(),
        kind: kind.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        url: format!("https://mcbench-builds.github.io/gallery/b/{id}/"),
        published_at: now - ago,
        conversation_id: (kind == "chat").then(|| "chat-lamp".to_string()),
        bench_id: (kind == "bench").then(|| "bench-towers".to_string()),
        archived: false,
    };
    let mut gone = build(
        "0c41d2e9aa10",
        "A windmill by the river",
        "chat",
        &["gpt-5.5"],
        3 * day,
    );
    gone.conversation_id = None;
    let mut archived = build(
        "77ab01e3c4f2",
        "Two towers and a bridge",
        "bench",
        &["claude-opus-5-5", "gpt-5.5", "gemini-3-pro"],
        45 * day,
    );
    archived.bench_id = Some("bench-old".into());
    archived.archived = true;
    Published {
        repo: Some("mcbench-builds/gallery".into()),
        builds: vec![
            build(
                "5f2a9c01b7de",
                "Redstone lamp with a lever",
                "chat",
                &["claude-opus-5-5"],
                2 * 3_600_000,
            ),
            build(
                "9e33c7a0d215",
                "Castle gate",
                "bench",
                &["claude-opus-5-5", "gpt-5.5"],
                day,
            ),
            gone,
            archived,
        ],
    }
}

impl StandIn {
    /// Plays `play` from now on (the dev binary's `--settings`).
    pub fn settings_play(&self, play: Play) {
        let mut s = self.settings.lock().expect("lock");
        s.plays.push(play);
        match play {
            Play::ClaudeSignIn => {
                let e = s.entry("claude-code");
                e.connected = false;
                e.signed_in = false;
                e.account = None;
                e.plan = None;
                e.version = None;
                e.sign_in.as_mut().expect("a sign-in").installed = false;
            }
            Play::ChatgptSignIn => {
                for id in ["codex", "codex-cli"] {
                    let e = s.entry(id);
                    e.connected = false;
                    e.signed_in = false;
                    e.account = None;
                    e.plan = None;
                }
            }
            Play::SetupWsl
            | Play::SetupWslRefused
            | Play::SetupWslOptional
            | Play::SetupWslOptionalRefused => s.wsl = Some(Wsl::NotInstalled),
            Play::SetupWslRestart | Play::SetupWslOptionalRestart => {
                s.wsl = Some(Wsl::NeedsRestart)
            }
            Play::NoAi => {
                for e in &mut s.entries {
                    e.connected = false;
                    e.key_source = None;
                    e.signed_in = false;
                    e.account = None;
                    e.plan = None;
                }
            }
            Play::VotesNone => s.votes = Ok(0),
            Play::VotesOne => s.votes = Ok(1),
            Play::VotesUnreadable => {
                s.votes = Err("The leaderboard could not be read (a fixture error)".into())
            }
            Play::SharesEmpty => {
                s.shares = Ok(Published {
                    repo: Some("mcbench-builds/gallery".into()),
                    builds: Vec::new(),
                })
            }
            Play::SharesNoRepo => {
                s.share_repo = None;
                s.shares = Ok(Published {
                    repo: None,
                    builds: Vec::new(),
                })
            }
            Play::SharesFailed => s.shares = Err(SHARING_NOT_JOINED.into()),
            Play::SetupPlay | Play::SetupFails | Play::SetupOptionalFails => {
                s.setup_tick = Some(0);
            }
            _ => {}
        }
        let providers: Vec<Provider> = s.entries.iter().map(provider_of).collect();
        let setup = s.setup_tick.is_some();
        drop(s);
        self.data.lock().expect("lock").providers = providers;
        let (wsl, optional) = {
            let s = self.settings.lock().expect("lock");
            (s.wsl, s.wsl_optional())
        };
        if let Some(wsl) = wsl {
            self.publish_setup(wsl_steps(wsl, optional));
        }
        if setup {
            self.publish_setup(setup_steps(
                0,
                play_of(&self.settings.lock().expect("lock")),
                None,
            ));
        }
    }

    /// Where the stand-in says its settings file is (under the dev
    /// binary's scratch folder).
    pub fn set_settings_file(&self, path: PathBuf) {
        self.settings.lock().expect("lock").settings_file = path;
    }

    /// One step of every flow under way: sign-ins, installs, setup.
    pub fn settings_tick(&self) {
        let mut published: Vec<Update> = Vec::new();
        let mut settled = false;
        let mut ended = false;
        {
            let mut s = self.settings.lock().expect("lock");
            // Installs.
            let programs: Vec<String> = s
                .installs
                .iter()
                .filter(|(_, v)| v.state == "running")
                .map(|(k, _)| k.clone())
                .collect();
            for name in programs {
                let total = s
                    .entries
                    .iter()
                    .find_map(|e| e.sign_in.as_ref().filter(|o| o.program.name == name))
                    .map(|o| o.program.bytes)
                    .expect("a program");
                let fails = s.plays(Play::InstallFails);
                let view = s.installs.get_mut(&name).expect("an install");
                let done = view.progress.map_or(0, |p| p.done + 60_000_000).min(total);
                if fails && done > total / 3 {
                    *view = InstallView {
                        state: "failed".into(),
                        progress: None,
                        error: Some(INSTALL_FAILED.into()),
                    };
                } else if view.progress.is_some_and(|p| p.done >= total) {
                    *view = InstallView {
                        state: "done".into(),
                        progress: None,
                        error: None,
                    };
                    for e in s
                        .entries
                        .iter_mut()
                        .filter(|e| e.sign_in.as_ref().is_some_and(|o| o.program.name == name))
                    {
                        let offer = e.sign_in.as_mut().expect("a sign-in");
                        offer.installed = true;
                        let version = offer
                            .program
                            .title
                            .rsplit(' ')
                            .next()
                            .expect("a title ends in its version")
                            .to_string();
                        e.version = Some(version);
                        if e.signed_in {
                            e.connected = true;
                        }
                    }
                    settled = true;
                } else {
                    view.progress = Some(Progress {
                        done,
                        total: Some(total),
                    });
                }
                let state = s.installs[&name].clone();
                published.push(Update::Install {
                    program: name,
                    state,
                });
            }
            // Sign-ins.
            let running: Vec<String> = s
                .sign_ins
                .iter()
                .filter(|(_, v)| matches!(v.state, SignInState::Starting | SignInState::Waiting))
                .map(|(k, _)| k.clone())
                .collect();
            let mut done_logins: Vec<String> = Vec::new();
            for id in running {
                if id == "codex-cli" && running_twin(&s, &id) {
                    continue;
                }
                let fails = s.plays(Play::SignInFails);
                let cancels = s.plays(Play::SignInCancelled);
                let waited = *s.waited.entry(id.clone()).or_insert(0);
                let before = s.sign_ins[&id].clone();
                let view = s.sign_ins.get_mut(&id).expect("a sign-in");
                match (view.state, id.as_str()) {
                    (SignInState::Starting, "claude-code") => {
                        view.state = SignInState::Waiting;
                        view.url = Some(SIGN_IN_URL.into());
                    }
                    (SignInState::Starting, _) => {
                        view.state = SignInState::Waiting;
                        view.url = Some(CODEX_URL.into());
                        view.user_code = Some(USER_CODE.into());
                    }
                    (SignInState::Waiting, _) if fails && waited >= 1 => {
                        view.state = SignInState::Failed;
                        view.error = Some(SIGN_IN_TIMED_OUT.into());
                        ended = true;
                    }
                    (SignInState::Waiting, _) if cancels && waited >= 1 => {
                        view.state = SignInState::Cancelled;
                        ended = true;
                    }
                    (SignInState::Waiting, "claude-code") => {
                        if !view.wants_input {
                            view.wants_input = true;
                            view.message = Some(PASTE_PROMPT.into());
                        }
                    }
                    (SignInState::Waiting, _) if waited >= 6 => {
                        view.state = SignInState::Done;
                        done_logins.push(id.clone());
                    }
                    _ => {}
                }
                if s.sign_ins[&id].state == SignInState::Waiting {
                    *s.waited.get_mut(&id).expect("waited") += 1;
                }
                if s.sign_ins[&id] != before {
                    published.extend(sign_in_updates(&s, &id));
                }
            }
            for id in done_logins {
                signed_in(&mut s, &id);
                settled = true;
            }
            // Every end of a sign-in (done, failed, cancelled): the
            // providers, then the settings.
            if ended {
                settled = true;
            }
        }
        // WSL: the installer ends at the next step.
        let wsl = {
            let mut s = self.settings.lock().expect("lock");
            let wsl = s.wsl;
            match wsl {
                // The installer runs for two steps (the window's checks
                // read the running row in between).
                Some(Wsl::Installing) if !s.wsl_stepped => {
                    s.wsl_stepped = true;
                    None
                }
                Some(Wsl::Installing) => {
                    let next = if s.plays(Play::SetupWslRefused)
                        || s.plays(Play::SetupWslOptionalRefused)
                    {
                        Wsl::Refused
                    } else {
                        Wsl::Ready
                    };
                    s.wsl = Some(next);
                    Some(wsl_steps(next, s.wsl_optional()))
                }
                _ => None,
            }
        };
        if let Some(steps) = wsl {
            self.publish_setup(steps);
        }
        // Setup.
        let setup = {
            let mut s = self.settings.lock().expect("lock");
            s.setup_tick.map(|tick| {
                s.setup_tick = Some(tick + 1);
                setup_steps(tick + 1, play_of(&s), s.retried)
            })
        };
        let window = self.window.lock().expect("lock").clone();
        let Some(window) = window else {
            return;
        };
        for update in published {
            window.publish(update);
        }
        if settled {
            self.publish_providers_and_settings();
        }
        if let Some(steps) = setup {
            let unchanged = self.setup.lock().expect("lock").as_ref() == Some(&steps);
            if !unchanged {
                self.publish_setup(steps);
            }
        }
    }

    fn publish_providers_and_settings(&self) {
        let (providers, view) = {
            let s = self.settings.lock().expect("lock");
            (
                s.entries.iter().map(provider_of).collect::<Vec<_>>(),
                s.view(),
            )
        };
        self.data.lock().expect("lock").providers = providers.clone();
        let window = self.window();
        window.publish(Update::Providers(Ok(providers)));
        window.publish(Update::Settings(Ok(view)));
    }

    /// The settings seam's commands.
    pub(super) fn settings_command(&self, command: Command) {
        let window = self.window();
        match command {
            Command::ReadSettings { refresh } => {
                let mut s = self.settings.lock().expect("lock");
                if s.plays(Play::ReadFails) {
                    drop(s);
                    window.publish(Update::Settings(Err(READ_FAILED.into())));
                    return;
                }
                if refresh {
                    s.checked_at = now_ms();
                    drop(s);
                    self.publish_providers_and_settings();
                } else {
                    let view = s.view();
                    drop(s);
                    window.publish(Update::Settings(Ok(view)));
                }
            }
            Command::SaveSettings(change) => {
                let mut s = self.settings.lock().expect("lock");
                let refusal = if s.plays(Play::SaveRefused) {
                    Some(SAVE_REFUSED.to_string())
                } else if change.share_repo.as_deref().is_some_and(|r| {
                    let parts: Vec<&str> = r.split('/').collect();
                    parts.len() != 2 || parts.iter().any(|p| p.is_empty())
                }) {
                    Some("The repository for shared builds must be owner/name".into())
                } else {
                    change
                        .title_model
                        .as_ref()
                        .filter(|t| !s.entries.iter().any(|e| e.id == t.provider))
                        .map(|t| format!("Unknown title provider: {}", t.provider))
                };
                if let Some(text) = refusal {
                    drop(s);
                    window.publish(Update::SettingsSaved(Err(text)));
                    return;
                }
                s.title_model = change.title_model.clone();
                s.share_repo = change.share_repo.clone();
                for p in &change.providers {
                    let key = p.api_key.trim().to_string();
                    let e = s.entry(&p.id);
                    e.enabled = p.enabled;
                    e.hidden_models = p.hidden_models.clone();
                    e.model_order = p.model_order.clone();
                    if !key.is_empty() && e.env_key.is_some() {
                        e.key_source = Some(KeySource::Saved);
                        e.connected = true;
                        s.keys.insert(p.id.clone(), key);
                    }
                }
                let providers: Vec<Provider> = s.entries.iter().map(provider_of).collect();
                drop(s);
                let favorites = {
                    let mut data = self.data.lock().expect("lock");
                    data.providers = providers.clone();
                    data.favorites.clone()
                };
                window.publish(Update::Providers(Ok(providers)));
                window.publish(Update::Favorites(favorites));
                window.publish(Update::SettingsSaved(Ok(())));
            }
            Command::SignIn { provider, method } => {
                let mut s = self.settings.lock().expect("lock");
                let refusal = if program(&provider).is_none() {
                    Some(format!("{provider} has no sign-in"))
                } else if s.sign_ins.iter().any(|(id, v)| {
                    crate::state::sign_in::shares_login(id, &provider)
                        && matches!(v.state, SignInState::Starting | SignInState::Waiting)
                }) {
                    Some(
                        "A sign-in is already under way here. Finish it or cancel it first.".into(),
                    )
                } else if provider == "claude-code" && method.is_none() {
                    Some("Choose a sign-in method: claudeai or console".into())
                } else {
                    None
                };
                if let Some(text) = refusal {
                    let updates = sign_in_updates(&s, &provider);
                    drop(s);
                    window.publish(Update::Error(text));
                    for u in updates {
                        window.publish(u);
                    }
                    return;
                }
                s.waited.remove(&provider);
                s.sign_ins.insert(
                    provider.clone(),
                    SignInView {
                        state: SignInState::Starting,
                        url: None,
                        user_code: None,
                        wants_input: false,
                        message: None,
                        error: None,
                    },
                );
                let updates = sign_in_updates(&s, &provider);
                drop(s);
                for u in updates {
                    window.publish(u);
                }
            }
            Command::SignInInput { provider, text } => {
                let mut s = self.settings.lock().expect("lock");
                let waiting = s
                    .sign_ins
                    .get(&provider)
                    .is_some_and(|v| v.state == SignInState::Waiting && v.wants_input);
                let refusal = if !s.sign_ins.contains_key(&provider) {
                    Some("No sign-in was started here".to_string())
                } else if !waiting {
                    Some("This sign-in is not waiting for a code".into())
                } else if text.contains('\n') {
                    Some("Paste the code as one line".into())
                } else {
                    None
                };
                if let Some(text) = refusal {
                    let updates = sign_in_updates(&s, &provider);
                    drop(s);
                    window.publish(Update::Error(text));
                    for u in updates {
                        window.publish(u);
                    }
                    return;
                }
                let view = s.sign_ins.get_mut(&provider).expect("a sign-in");
                view.state = SignInState::Done;
                view.wants_input = false;
                signed_in(&mut s, &provider);
                let updates = sign_in_updates(&s, &provider);
                drop(s);
                for u in updates {
                    window.publish(u);
                }
                self.publish_providers_and_settings();
            }
            Command::CancelSignIn { provider } => {
                let mut s = self.settings.lock().expect("lock");
                let running = s.sign_ins.get(&provider).is_some_and(|v| {
                    matches!(v.state, SignInState::Starting | SignInState::Waiting)
                });
                if !running {
                    // Cancelling a finished sign-in changes nothing: its
                    // unchanged state again. None started is a refusal.
                    let started = s.sign_ins.contains_key(&provider);
                    let updates = sign_in_updates(&s, &provider);
                    drop(s);
                    if !started {
                        window.publish(Update::Error("No sign-in was started here".into()));
                    }
                    for u in updates {
                        window.publish(u);
                    }
                    return;
                }
                s.sign_ins.get_mut(&provider).expect("a sign-in").state = SignInState::Cancelled;
                let updates = sign_in_updates(&s, &provider);
                drop(s);
                for u in updates {
                    window.publish(u);
                }
                // Every end of a sign-in: the providers, then the settings.
                self.publish_providers_and_settings();
            }
            Command::SignOut { provider } => {
                let mut s = self.settings.lock().expect("lock");
                let running = s.sign_ins.iter().any(|(id, v)| {
                    crate::state::sign_in::shares_login(id, &provider)
                        && matches!(v.state, SignInState::Starting | SignInState::Waiting)
                });
                if running {
                    drop(s);
                    window.publish(Update::Error("Finish or cancel the sign-in first".into()));
                    self.publish_providers_and_settings();
                    return;
                }
                let twins: Vec<String> = s
                    .entries
                    .iter()
                    .filter(|e| crate::state::sign_in::shares_login(&e.id, &provider))
                    .map(|e| e.id.clone())
                    .collect();
                for id in twins {
                    let e = s.entry(&id);
                    e.signed_in = false;
                    e.connected = false;
                    e.account = None;
                    e.plan = None;
                }
                drop(s);
                self.publish_providers_and_settings();
            }
            Command::InstallCli { program: name } => {
                let mut s = self.settings.lock().expect("lock");
                let installed = s
                    .entries
                    .iter()
                    .find_map(|e| e.sign_in.as_ref().filter(|o| o.program.name == name))
                    .map(|o| o.installed);
                let refusal = match installed {
                    None => Some(format!(
                        "No CLI named {name} can be installed: claude or codex"
                    )),
                    Some(true) => Some(format!(
                        "{name} is installed already, and the app runs that one"
                    )),
                    Some(false) if s.plays(Play::InstallRefused) => Some(INSTALL_REFUSED.into()),
                    Some(false) => None,
                };
                if let Some(text) = refusal {
                    // The refusal, then the program's install when one
                    // exists, then fresh settings, last.
                    let state = s.installs.get(&name).cloned();
                    let view = s.view();
                    drop(s);
                    window.publish(Update::Error(text));
                    if let Some(state) = state {
                        window.publish(Update::Install {
                            program: name,
                            state,
                        });
                    }
                    window.publish(Update::Settings(Ok(view)));
                    return;
                }
                let state = InstallView {
                    state: "running".into(),
                    progress: None,
                    error: None,
                };
                s.installs.insert(name.clone(), state.clone());
                drop(s);
                window.publish(Update::Install {
                    program: name,
                    state,
                });
            }
            Command::RetrySetup => {
                let failed = self
                    .setup
                    .lock()
                    .expect("lock")
                    .as_ref()
                    .is_some_and(|steps| {
                        steps
                            .iter()
                            .any(|s| matches!(s.state, StepState::Failed { .. }))
                    });
                if !failed {
                    window.publish(Update::Error("No setup step failed".into()));
                    let steps = self.setup.lock().expect("lock").clone().unwrap_or_default();
                    window.publish(Update::Setup(steps));
                    return;
                }
                let steps = {
                    let mut s = self.settings.lock().expect("lock");
                    let tick = s.setup_tick.expect("a failed setup is a play");
                    s.retried = Some(tick);
                    setup_steps(tick, play_of(&s), Some(tick))
                };
                // The core's retry can publish its steps twice (the retry's
                // answer and the step's own change): the window applies both.
                self.publish_setup(steps.clone());
                self.publish_setup(steps);
            }
            Command::ReadLeaderboard => {
                // bench agent: the ranking with the count.
                window.publish(Update::Leaderboard(self.leaderboard_now()));
            }
            Command::ClearVotes => {
                let mut s = self.settings.lock().expect("lock");
                if s.plays(Play::ClearFails) {
                    drop(s);
                    window.publish(Update::Error(CLEAR_FAILED.into()));
                    window.publish(Update::Leaderboard(self.leaderboard_now()));
                    return;
                }
                s.votes = Ok(0);
                drop(s);
                // bench agent: every asked bench's votes are gone too.
                self.votes_cleared();
                window.publish(Update::Leaderboard(self.leaderboard_now()));
                self.push_cleared();
            }
            Command::ReadShares => {
                let shares = self.settings.lock().expect("lock").shares.clone();
                window.publish(Update::Shares(shares));
            }
            Command::RemoveShare { share_id } => {
                let mut s = self.settings.lock().expect("lock");
                let fails = s.plays(Play::RemoveFails);
                // share agent: the build taken down, for its subject.
                let mut removed = None;
                let result = match &mut s.shares {
                    Err(e) => Err(e.clone()),
                    Ok(_) if fails => Err(REMOVE_FAILED.into()),
                    Ok(p) => match p.builds.iter().position(|b| b.share_id == share_id) {
                        Some(at) => {
                            removed = Some(p.builds.remove(at));
                            Ok(())
                        }
                        None => Err("This build is not shared".into()),
                    },
                };
                let shares = s.shares.clone();
                drop(s);
                let worked = result.is_ok();
                window.publish(Update::ShareRemoved { share_id, result });
                // share agent: the subject's page is gone too
                // (`ui-seam-share.md` answer 4), before the list.
                if let Some(subject) = removed.as_ref().and_then(|b| self.subject_of(b)) {
                    window.publish(Update::ShareChanged { subject, url: None });
                }
                if worked {
                    window.publish(Update::Shares(shares));
                }
            }
            Command::InstallWsl => {
                let mut s = self.settings.lock().expect("lock");
                match s.wsl {
                    Some(Wsl::NotInstalled | Wsl::Refused) => {
                        s.wsl = Some(Wsl::Installing);
                        s.wsl_stepped = false;
                        let optional = s.wsl_optional();
                        drop(s);
                        self.publish_setup(wsl_steps(Wsl::Installing, optional));
                    }
                    other => {
                        drop(s);
                        window.publish(Update::Error(format!(
                            "WSL needs no install here ({other:?})"
                        )));
                        let steps = self.setup.lock().expect("lock").clone().unwrap_or_default();
                        window.publish(Update::Setup(steps));
                    }
                }
            }
            Command::RestartComputer => {
                // The stand-in's Windows accepts the request and then never
                // restarts (a program with unsaved work holds it up): the
                // app is told nothing and nothing is published.
                self.settings.lock().expect("lock").restarts += 1;
            }
            other => panic!("not a settings command: {other:?}"),
        }
    }
}

/// Whether `id`'s twin login (ChatGPT for Codex CLI) runs the same sign-in.
fn running_twin(s: &State, id: &str) -> bool {
    let twin = if id == "codex-cli" {
        "codex"
    } else {
        "codex-cli"
    };
    s.sign_ins
        .get(twin)
        .is_some_and(|v| matches!(v.state, SignInState::Starting | SignInState::Waiting))
}

/// The sign-in updates for `id` and the providers sharing its login.
fn sign_in_updates(s: &State, id: &str) -> Vec<Update> {
    let state = s.sign_ins.get(id).cloned();
    s.entries
        .iter()
        .filter(|e| e.sign_in.is_some())
        .filter(|e| crate::state::sign_in::shares_login(&e.id, id))
        .map(|e| Update::SignIn {
            provider: e.id.clone(),
            state: state.clone(),
        })
        .collect()
}

/// A login is done: the provider and those sharing it are signed in.
fn signed_in(s: &mut State, id: &str) {
    let twins: Vec<String> = s
        .entries
        .iter()
        .filter(|e| crate::state::sign_in::shares_login(&e.id, id))
        .map(|e| e.id.clone())
        .collect();
    for twin in twins {
        let e = s.entry(&twin);
        e.signed_in = true;
        e.account = Some("builder@example.com".into());
        e.plan = Some(if twin == "claude-code" { "Max" } else { "Plus" }.into());
        e.connected = e.sign_in.as_ref().is_some_and(|o| o.installed);
    }
}

/// The pickers' provider of a Settings entry.
fn provider_of(e: &ProviderSettings) -> Provider {
    Provider {
        id: e.id.clone(),
        label: e.label.clone(),
        connected: e.connected,
        auth: e.auth,
        harness: e.harness,
        enabled: e.enabled,
        hidden_models: e.hidden_models.clone(),
        model_order: e.model_order.clone(),
    }
}

/// Which setup play runs.
fn play_of(s: &State) -> Play {
    [Play::SetupFails, Play::SetupOptionalFails, Play::SetupPlay]
        .into_iter()
        .find(|p| s.plays(*p))
        .expect("a setup play")
}

/// The app's steps (`mcbench_setup::steps::native_steps`) at `tick` of a
/// play: the data unpacked (2 ticks), the textures downloaded (8 ticks of
/// 9 MB of 64 MB, then baked), the optional shell downloaded alongside
/// (no total known). `setup-fails` fails the textures at tick 6 until a
/// retry, which downloads them again from the start; `setup-optional-fails`
/// leaves the shell unavailable.
fn setup_steps(tick: u32, play: Play, retried: Option<u32>) -> Vec<Step> {
    const TEXTURES: u64 = 64_300_000;
    mcbench_setup::steps::native_steps()
        .into_iter()
        .map(|mut step| {
            step.state = match step.id.as_str() {
                "data" => match tick {
                    0 => StepState::Running {
                        detail: Some("Unpacking blocks.json".into()),
                        progress: None,
                    },
                    1 => StepState::Running {
                        detail: Some("Unpacking the block models".into()),
                        progress: None,
                    },
                    _ => StepState::Done { seconds: 1.2 },
                },
                "textures" => {
                    let t = tick.saturating_sub(retried.unwrap_or(2));
                    if play == Play::SetupFails && tick >= 6 && retried.is_none() {
                        StepState::Failed {
                            error: SETUP_FAILURE.into(),
                        }
                    } else if tick < 2 {
                        StepState::Waiting
                    } else if t < 8 {
                        StepState::Running {
                            detail: Some("Downloading the client jar".into()),
                            progress: Some(Bytes {
                                done: (t as u64 * 9_000_000).min(TEXTURES),
                                total: Some(TEXTURES),
                            }),
                        }
                    } else if t < 9 {
                        StepState::Running {
                            detail: Some("Baking the block textures".into()),
                            progress: None,
                        }
                    } else {
                        StepState::Done { seconds: 14.8 }
                    }
                }
                "shell" => {
                    if play == Play::SetupOptionalFails && tick >= 3 {
                        StepState::Unavailable {
                            error: "WSL is not installed: install it from https://learn.microsoft.com/windows/wsl/install, then start the app again.".into(),
                        }
                    } else if tick < 1 {
                        StepState::Waiting
                    } else if tick < 7 {
                        StepState::Running {
                            detail: None,
                            progress: Some(Bytes {
                                done: tick as u64 * 21_000_000,
                                total: None,
                            }),
                        }
                    } else {
                        StepState::Done { seconds: 6.1 }
                    }
                }
                other => panic!("no setup step {other}"),
            };
            step
        })
        .collect()
}

/// The steps of a WSL play: the data done, the textures downloading (so the
/// screen stays; done when `optional`, so setup is otherwise ready), the
/// shell as WSL is, with its sentence as the core writes it and what it
/// needs (`Step.needs`).
fn wsl_steps(wsl: Wsl, optional: bool) -> Vec<Step> {
    mcbench_setup::steps::native_steps()
        .into_iter()
        .map(|mut step| {
            step.state = match step.id.as_str() {
                "data" => StepState::Done { seconds: 1.2 },
                "textures" if optional => StepState::Done { seconds: 14.8 },
                "textures" => StepState::Running {
                    detail: Some("Downloading the client jar".into()),
                    progress: Some(Bytes {
                        done: 27_000_000,
                        total: Some(64_300_000),
                    }),
                },
                _ => {
                    step.needs = match wsl {
                        Wsl::NotInstalled | Wsl::Refused => Some(mcbench_setup::Needs::InstallWsl),
                        Wsl::NeedsRestart => Some(mcbench_setup::Needs::Restart),
                        Wsl::Installing | Wsl::Ready => None,
                    };
                    match wsl {
                        Wsl::NotInstalled => StepState::Unavailable {
                            error: WSL_NEEDED.into(),
                        },
                        Wsl::Installing => StepState::Running {
                            detail: Some(WSL_INSTALLING.into()),
                            progress: None,
                        },
                        Wsl::NeedsRestart => StepState::Unavailable {
                            error: WSL_RESTART.into(),
                        },
                        Wsl::Ready => StepState::Done { seconds: 40.0 },
                        Wsl::Refused => StepState::Failed {
                            error: WSL_REFUSED.into(),
                        },
                    }
                }
            };
            step
        })
        .collect()
}

impl StandIn {
    /// How many `RestartComputer` requests the stand-in took.
    pub fn restarts_asked(&self) -> u32 {
        self.settings.lock().expect("lock").restarts
    }
}
