//! Settings, sign-in, CLI installs, setup, the vote count and the shares
//! list without a repository through the core (`Core::command`), each with a recording `Window`: the
//! updates each command publishes on success and on a failure with a real
//! cause (a provider that does not exist, a sign-in that is refused, a
//! settings file in a read-only folder, a votes table that is gone),
//! checked against the seam's rules (`src/core/commands.rs`), and a
//! sign-in and an install driven through all their states by a fake CLI
//! and a fake download, every change published once and in order. The test
//! never polls: it blocks on the window's own condition variable until the
//! update it waits for arrives.
//!
//! The process's environment is set once, before any core starts (the CLI
//! lookups read it): `PATH` holds a folder with no CLI in it, then
//! `/usr/bin` and `/bin`; `MCBENCH_CLAUDE_BINARY` names a fake Claude Code
//! (a shell script: Unix); `CLAUDE_CONFIG_DIR` and `CODEX_HOME` are this
//! run's own; no provider key, `MCBENCH_CODEX_BINARY`, `MCBENCH_TOOLS_DIR`
//! or `MCBENCH_SETTINGS_FILE` is set.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Once};
use std::time::Duration;

use async_trait::async_trait;

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::harness::install::{InstallView, Installer};
#[cfg(unix)]
use mcbench_providers::harness::install::{Pin, Progress};
#[cfg(unix)]
use mcbench_ui::SignInState;
use mcbench_ui::{
    Command, KeySource, ProviderChange, SettingsChange, SettingsView, SignInView, TitleModel,
    Update, Window,
};

struct NoModel;

#[async_trait]
impl Backend for NoModel {
    fn provider(&self, _: &str) -> Result<Arc<dyn mcbench_providers::Provider>, String> {
        Err("no model in the settings tests".into())
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(Vec::new())
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn network_access(&self) -> bool {
        false
    }
    async fn shutdown(&self) {}
}

/// This run's own folder for the CLIs' homes and the fake.
fn run_folder() -> PathBuf {
    std::env::temp_dir().join(format!("mcbench-core-settings-{}", std::process::id()))
}

fn claude_dir() -> PathBuf {
    run_folder().join("claude")
}

/// Claude Code as the login sees it: `--version`; `auth login` prints the
/// address and asks for a code in one write, answers a wrong code with
/// its sentence and a right one by storing the login, each once the test
/// lets it (the file `answer` in its folder, which it takes), so the
/// command's own answer comes first; `auth logout` removes it, or
/// fails while `refuse-logout` exists. Each run records the `BROWSER` it
/// was given.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
case "$1" in
  --version) echo "2.1.311 (Claude Code)"; exit 0 ;;
esac
printf '%s' "$BROWSER" > "$CLAUDE_CONFIG_DIR/browser-$2"
case "$1 $2" in
  "auth login")
    printf 'If the browser did not open, visit: https://claude.com/cai/oauth/authorize?code=true&state=fake\nPaste code here if prompted > '
    while read code; do
      while [ ! -f "$CLAUDE_CONFIG_DIR/answer" ]; do sleep 0.01; done
      rm -f "$CLAUDE_CONFIG_DIR/answer"
      if [ "$code" = "good#fake" ]; then break; fi
      echo "Invalid code. Please make sure the full code was copied."
    done
    printf '{"oauthAccount":{"emailAddress":"person@example.com","organizationType":"claude_max"}}' > "$CLAUDE_CONFIG_DIR/.claude.json"
    echo "Login successful."
    exit 0 ;;
  "auth logout")
    if [ -f "$CLAUDE_CONFIG_DIR/refuse-logout" ]; then echo "Could not log out: refused"; exit 1; fi
    rm -f "$CLAUDE_CONFIG_DIR/.claude.json"
    echo "Successfully logged out"
    exit 0 ;;
esac
exit 2
"#;

static ENV: Once = Once::new();

/// How many tests use the run's folder now: the first makes it, the last
/// to end removes it, so nothing is left in `TMPDIR`.
static USERS: Mutex<usize> = Mutex::new(0);

/// A test's hold on the run's folder.
struct Environment;

impl Drop for Environment {
    fn drop(&mut self) {
        let mut users = USERS.lock().unwrap();
        *users -= 1;
        if *users == 0 {
            std::fs::remove_dir_all(run_folder()).unwrap();
        }
    }
}

/// The environment of the module's doc, set once, first thing in every
/// test (each test calls it before it starts anything), and the run's
/// folder made when no test holds it.
fn environment() -> Environment {
    let run = run_folder();
    let empty = run.join("bin");
    let fake = run.join("fake-claude");
    let mut users = USERS.lock().unwrap();
    if *users == 0 {
        std::fs::create_dir_all(&empty).unwrap();
        std::fs::create_dir_all(claude_dir()).unwrap();
        std::fs::create_dir_all(run.join("codex")).unwrap();
        std::fs::write(&fake, FAKE_CLAUDE).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&fake, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
    }
    *users += 1;
    drop(users);
    ENV.call_once(|| {
        let path = format!("{}:/usr/bin:/bin", empty.display());
        // SAFETY: called once through `Once`, first thing in every test, so
        // every other test thread is blocked in `call_once` and no thread
        // reads the environment while it is changed.
        unsafe {
            std::env::set_var("PATH", path);
            std::env::set_var("MCBENCH_CLAUDE_BINARY", &fake);
            std::env::set_var("CLAUDE_CONFIG_DIR", claude_dir());
            std::env::set_var("CODEX_HOME", run.join("codex"));
            for name in [
                "MCBENCH_CODEX_BINARY",
                "MCBENCH_TOOLS_DIR",
                "MCBENCH_SETTINGS_FILE",
                "OPENROUTER_API_KEY",
                "OPENAI_API_KEY",
                "ANTHROPIC_API_KEY",
                "GROQ_API_KEY",
                "GOOGLE_API_KEY",
                "QWENCLOUD_API_KEY",
                "CURSOR_API_KEY",
            ] {
                std::env::remove_var(name);
            }
        }
    });
    Environment
}

/// The window: every update, and a condition variable the test waits on.
#[derive(Default)]
struct Recording {
    updates: Mutex<Vec<Update>>,
    arrived: Condvar,
}

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.updates.lock().unwrap().push(update);
        self.arrived.notify_all();
    }
}

impl Recording {
    /// Blocks until `done` holds of the updates (a minute at most: a hang,
    /// not a slow machine).
    fn wait(&self, what: &str, done: impl Fn(&[Update]) -> bool) {
        let updates = self.updates.lock().unwrap();
        let (updates, timeout) = self
            .arrived
            .wait_timeout_while(updates, Duration::from_secs(60), |u| !done(u))
            .unwrap();
        assert!(!timeout.timed_out(), "{what}: {:?}", kinds(&updates));
    }

    fn all(&self) -> Vec<Update> {
        self.updates.lock().unwrap().clone()
    }
}

struct App {
    core: Core,
    window: Arc<Recording>,
    data: PathBuf,
    /// Dropped last: the run's folder goes with the last test.
    _environment: Environment,
}

impl App {
    fn new(name: &str) -> App {
        App::with_installer(name, None)
    }

    fn with_installer(name: &str, installer: Option<Installer>) -> App {
        let environment = environment();
        let data = run_folder().join(name);
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let core = Core::start(Parts {
            data: data.clone(),
            program: std::env::current_exe().unwrap(),
            host: Arc::new(mcbench_store::SystemHost),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(Arc::new(NoModel)),
            installer,
            share: None,
            live: Default::default(),
        })
        .unwrap();
        let app = App {
            core,
            window: Arc::new(Recording::default()),
            data,
            _environment: environment,
        };
        let handle = app.core.handle().clone();
        let core = app.core.clone();
        handle
            .block_on(handle.spawn(async move { core.started().await }))
            .unwrap();
        mcbench_ui::Core::attach(&app.core, app.window.clone());
        app.window.wait("the attach's favourites", |u| {
            u.iter().any(|u| matches!(u, Update::Favorites(_)))
        });
        app
    }

    fn len(&self) -> usize {
        self.window.updates.lock().unwrap().len()
    }

    /// Runs `command` to its end; the updates it published, the lists and
    /// setup's steps left out.
    fn command(&self, command: Command) -> Vec<Update> {
        let from = self.len();
        let handle = self.core.handle().clone();
        let core = self.core.clone();
        handle
            .block_on(handle.spawn(async move { core.command(command).await }))
            .unwrap();
        self.since(from)
    }

    /// The updates from `from` on, lists and setup's steps left out.
    fn since(&self, from: usize) -> Vec<Update> {
        relevant(&self.window.all()[from..])
    }

    /// Blocks until the last of the updates from `from` on (as
    /// [`App::since`]) is `last` (as [`kinds`] names it); they are answered.
    #[cfg(unix)]
    fn wait_until_last(&self, from: usize, last: &str, what: &str) -> Vec<Update> {
        self.window.wait(what, |u| {
            kinds(&relevant(&u[from.min(u.len())..]))
                .last()
                .map(String::as_str)
                == Some(last)
        });
        self.since(from)
    }

    /// Lets the fake Claude Code answer the code it was given.
    #[cfg(unix)]
    fn let_the_cli_answer(&self) {
        std::fs::write(claude_dir().join("answer"), "").unwrap();
    }
}

impl Drop for App {
    fn drop(&mut self) {
        self.core.shutdown();
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn relevant(updates: &[Update]) -> Vec<Update> {
    updates
        .iter()
        .filter(|u| !matches!(u, Update::Conversations(_) | Update::Setup(_)))
        .cloned()
        .collect()
}

fn sign_in_kind(view: &SignInView) -> String {
    let mut kind = view.state.name().to_string();
    if view.url.is_some() {
        kind.push_str(" url");
    }
    if view.wants_input {
        kind.push_str(" input");
    }
    if let Some(message) = &view.message {
        kind.push_str(&format!(" «{message}»"));
    }
    if let Some(error) = &view.error {
        kind.push_str(&format!(" error «{error}»"));
    }
    kind
}

fn install_kind(view: &InstallView) -> String {
    let mut kind = view.state.clone();
    if let Some(p) = view.progress {
        kind.push_str(&format!(" {}/{:?}", p.done, p.total));
    }
    if let Some(error) = &view.error {
        kind.push_str(&format!(" error «{error}»"));
    }
    kind
}

/// A short name for each update, with its text where it has one.
fn kinds(updates: &[Update]) -> Vec<String> {
    updates
        .iter()
        .map(|u| match u {
            Update::Providers(_) => "providers".into(),
            Update::Favorites(_) => "favorites".into(),
            Update::Error(error) => format!("error: {error}"),
            Update::Settings(Ok(_)) => "settings".into(),
            Update::Settings(Err(error)) => format!("settings failed: {error}"),
            Update::SettingsSaved(Ok(())) => "saved".into(),
            Update::SettingsSaved(Err(error)) => format!("not saved: {error}"),
            Update::SignIn {
                provider,
                state: None,
            } => format!("sign-in {provider}: none"),
            Update::SignIn {
                provider,
                state: Some(view),
            } => format!("sign-in {provider}: {}", sign_in_kind(view)),
            Update::Install { program, state } => {
                format!("install {program}: {}", install_kind(state))
            }
            Update::Leaderboard(Ok(board)) => format!("leaderboard {}", board.votes),
            Update::Leaderboard(Err(error)) => format!("leaderboard failed: {error}"),
            Update::Shares(Err(error)) => format!("shares failed: {error}"),
            Update::ShareRemoved {
                share_id,
                result: Err(error),
            } => format!("share {share_id} kept: {error}"),
            other => format!("{other:?}"),
        })
        .collect()
}

fn settings_of(updates: &[Update]) -> SettingsView {
    updates
        .iter()
        .rev()
        .find_map(|u| match u {
            Update::Settings(Ok(view)) => Some(view.clone()),
            _ => None,
        })
        .expect("a settings view")
}

fn provider<'a>(view: &'a SettingsView, id: &str) -> &'a mcbench_ui::ProviderSettings {
    view.providers.iter().find(|p| p.id == id).unwrap()
}

#[cfg(unix)]
fn read_only(folder: &Path, on: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(
        folder,
        std::fs::Permissions::from_mode(if on { 0o555 } else { 0o755 }),
    )
    .unwrap();
}

/// Reading the settings: the view alone; `refresh` publishes the providers
/// first. The view lists the providers in `PROVIDERS`' order without
/// Cursor, names the settings file, and carries the core's lines.
#[test]
fn reading_the_settings_answers_the_view_and_a_refresh_the_providers_first() {
    let app = App::new("read");
    let updates = app.command(Command::ReadSettings { refresh: false });
    assert_eq!(kinds(&updates), ["settings"]);
    let view = settings_of(&updates);
    assert_eq!(view.settings_file, app.data.join("settings.json"));
    let ids: Vec<&str> = view.providers.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "openrouter",
            "openai",
            "codex",
            "anthropic",
            "groq",
            "google",
            "qwencloud",
            "claude-code",
            "codex-cli"
        ]
    );
    assert_eq!(provider(&view, "groq").summary, "No key");
    assert_eq!(view.title_model, None);
    let claude = provider(&view, "claude-code");
    let offer = claude.sign_in.as_ref().unwrap();
    assert_eq!(offer.methods, ["claudeai", "console"]);
    assert_eq!(offer.program.name, "claude");
    // The fake Claude Code is a shell script: only Unix can run it, so what
    // the view says of an installed CLI is checked there. (On Windows the
    // view gave no version and `installed` false in the VM; what it must
    // say there comes with the Windows CLIs.)
    #[cfg(unix)]
    {
        assert_eq!(claude.version.as_deref(), Some("2.1.311"));
        assert!(offer.installed);
        assert_eq!(
            claude.runtime.as_ref().unwrap().binary_value.as_deref(),
            Some(run_folder().join("fake-claude").to_str().unwrap())
        );
        assert!(
            provider(&view, "codex")
                .sign_in
                .as_ref()
                .unwrap()
                .methods
                .is_empty()
        );
        let updates = app.command(Command::ReadSettings { refresh: true });
        assert_eq!(kinds(&updates), ["providers", "settings"]);
    }
}

fn change_of(view: &SettingsView) -> SettingsChange {
    SettingsChange {
        title_model: view.title_model.clone(),
        share_repo: view.share_repo.clone(),
        providers: view
            .providers
            .iter()
            .map(|p| ProviderChange {
                id: p.id.clone(),
                enabled: p.enabled,
                api_key: String::new(),
                hidden_models: p.hidden_models.clone(),
                model_order: p.model_order.clone(),
            })
            .collect(),
    }
}

/// Saving: the providers, the favourites, then `SettingsSaved(Ok)`; a key
/// left empty keeps the saved one, which never leaves the core; a refusal
/// is `SettingsSaved(Err)` alone, the route's sentence.
#[test]
fn saving_the_settings_ends_in_settings_saved_and_a_refusal_in_it_alone() {
    let app = App::new("save");
    let view = settings_of(&app.command(Command::ReadSettings { refresh: false }));
    let mut change = change_of(&view);
    for p in &mut change.providers {
        if p.id == "groq" {
            p.enabled = false;
        }
        if p.id == "openrouter" {
            p.api_key = "  sk-test-one  ".into();
            p.hidden_models = vec!["a/b".into(), "a/b".into(), "c/d".into()];
            p.model_order = vec!["c/d".into()];
        }
    }
    change.title_model = Some(TitleModel {
        provider: "openrouter".into(),
        model: "a/b".into(),
        effort: String::new(),
    });
    let updates = app.command(Command::SaveSettings(change.clone()));
    assert_eq!(kinds(&updates), ["providers", "favorites", "saved"]);
    let Update::Providers(Ok(list)) = &updates[0] else {
        unreachable!()
    };
    assert!(!list.iter().find(|p| p.id == "groq").unwrap().enabled);
    assert!(
        list.iter()
            .find(|p| p.id == "openrouter")
            .unwrap()
            .connected
    );
    let file = std::fs::read_to_string(app.data.join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(saved["providers"]["openrouter"]["apiKey"], "sk-test-one");
    assert_eq!(
        saved["providers"]["openrouter"]["hiddenModels"],
        serde_json::json!(["a/b", "c/d"])
    );
    // An empty key keeps the saved one; the view says where it comes from.
    for p in &mut change.providers {
        p.api_key = String::new();
    }
    assert_eq!(
        kinds(&app.command(Command::SaveSettings(change.clone()))),
        ["providers", "favorites", "saved"]
    );
    let file = std::fs::read_to_string(app.data.join("settings.json")).unwrap();
    let saved: serde_json::Value = serde_json::from_str(&file).unwrap();
    assert_eq!(saved["providers"]["openrouter"]["apiKey"], "sk-test-one");
    let view = settings_of(&app.command(Command::ReadSettings { refresh: false }));
    let openrouter = provider(&view, "openrouter");
    assert_eq!(openrouter.key_source, Some(KeySource::Saved));
    assert_eq!(openrouter.summary, "Key saved");
    assert!(!format!("{view:?}").contains("sk-test-one"));
    assert_eq!(provider(&view, "groq").summary, "Disabled");
    // Refused by the store's check: the sentence, alone.
    let mut refused = change.clone();
    refused.title_model = Some(TitleModel {
        provider: "nobody".into(),
        model: "x".into(),
        effort: String::new(),
    });
    assert_eq!(
        kinds(&app.command(Command::SaveSettings(refused))),
        ["not saved: Unknown title provider: nobody"]
    );
    // A settings file in a read-only folder.
    #[cfg(unix)]
    {
        read_only(&app.data, true);
        let updates = app.command(Command::SaveSettings(change));
        read_only(&app.data, false);
        assert!(
            matches!(&updates[..], [Update::SettingsSaved(Err(e))] if e.contains("ermission denied")),
            "{:?}",
            kinds(&updates)
        );
    }
}

/// A sign-in refused: `Update::Error`, then `Update::SignIn` with the
/// state as it is (none here); a sign-out refused: the error, the
/// providers, the settings read fresh.
#[test]
fn a_refused_sign_in_or_sign_out_says_why_then_the_state() {
    let app = App::new("refused");
    let nobody = |provider: &str| format!("sign-in {provider}: none");
    assert_eq!(
        kinds(&app.command(Command::SignIn {
            provider: "nosuch".into(),
            method: None
        })),
        ["error: nosuch has no sign-in".to_string(), nobody("nosuch")]
    );
    assert_eq!(
        kinds(&app.command(Command::SignIn {
            provider: "groq".into(),
            method: None
        })),
        ["error: groq has no sign-in".to_string(), nobody("groq")]
    );
    assert_eq!(
        kinds(&app.command(Command::SignIn {
            provider: "claude-code".into(),
            method: None
        })),
        [
            "error: Choose a sign-in method: claudeai or console".to_string(),
            nobody("claude-code")
        ]
    );
    assert_eq!(
        kinds(&app.command(Command::SignInInput {
            provider: "claude-code".into(),
            text: "x".into()
        })),
        [
            "error: No sign-in was started here".to_string(),
            nobody("claude-code")
        ]
    );
    assert_eq!(
        kinds(&app.command(Command::CancelSignIn {
            provider: "codex".into()
        })),
        [
            "error: No sign-in was started here".to_string(),
            nobody("codex")
        ]
    );
    assert_eq!(
        kinds(&app.command(Command::SignOut {
            provider: "nosuch".into()
        })),
        ["error: nosuch has no sign-in", "providers", "settings"]
    );
}

/// Installs refused: no such CLI, or one the app runs already (the one
/// `MCBENCH_CLAUDE_BINARY` names): the error alone, as no install of it
/// was started.
#[test]
fn a_refused_install_says_why() {
    let app = App::new("install-refused");
    assert_eq!(
        kinds(&app.command(Command::InstallCli {
            program: "nosuch".into()
        })),
        [
            "error: No CLI named nosuch can be installed: claude or codex",
            "settings"
        ]
    );
    assert_eq!(
        kinds(&app.command(Command::InstallCli {
            program: "claude".into()
        })),
        [
            "error: claude is installed already, and the app runs that one",
            "settings"
        ]
    );
}

/// Setup's retry with no failed step, the vote count, clearing the votes
/// (and a votes table that is gone), and the share commands.
#[test]
fn setup_votes_and_shares_answer_as_the_seam_says() {
    let app = App::new("votes");
    let updates = app.command(Command::RetrySetup);
    assert!(
        matches!(&app.window.all()[app.len() - 1], Update::Setup(_)),
        "a retry ends in the steps"
    );
    assert_eq!(kinds(&updates), ["error: No setup step failed"]);
    assert_eq!(
        kinds(&app.command(Command::ReadLeaderboard)),
        ["leaderboard 0"]
    );
    assert_eq!(kinds(&app.command(Command::ClearVotes)), ["leaderboard 0"]);
    let db = rusqlite::Connection::open(app.data.join("mcbench.sqlite3")).unwrap();
    db.execute_batch("DROP TABLE bench_votes;").unwrap();
    let updates = kinds(&app.command(Command::ClearVotes));
    assert_eq!(updates.len(), 2, "{updates:?}");
    assert!(
        updates[0].starts_with("error: ") && updates[0].contains("bench_votes"),
        "{updates:?}"
    );
    assert!(
        updates[1].starts_with("leaderboard failed: "),
        "{updates:?}"
    );
    // The shell step needs neither (only Windows fills `needs`, and this
    // core has no shell step's need): each changes nothing and ends in the
    // steps as they are.
    for command in [Command::InstallWsl, Command::RestartComputer] {
        let from = app.len();
        assert_eq!(kinds(&app.command(command)), Vec::<String>::new());
        assert!(
            matches!(&app.window.all()[from..], [.., Update::Setup(_)]),
            "the WSL commands end in the steps"
        );
    }
    assert_eq!(
        kinds(&app.command(Command::ReadShares)),
        ["Shares(Ok(Published { repo: None, builds: [] }))"]
    );
    assert_eq!(
        kinds(&app.command(Command::RemoveShare {
            share_id: "0123456789ab".into()
        })),
        ["share 0123456789ab kept: Set the GitHub repository for shared builds in Settings"]
    );
}

/// A sign-in driven by the fake Claude Code through every state: each
/// change of its view published once, in order, as it happens; the
/// commands that change nothing answer the state as it is; at its end the
/// providers and the settings read fresh. Then a refused second start, a
/// cancel, a sign-out that fails and one that works, and settings that
/// cannot be read. The CLI is started with `BROWSER=true`, so nothing but
/// the person's click opens the address.
#[cfg(unix)]
#[test]
fn a_sign_in_publishes_each_change_once_in_order_and_ends_in_fresh_settings() {
    let app = App::new("sign-in");
    let from = app.len();
    let started = app.command(Command::SignIn {
        provider: "claude-code".into(),
        method: Some("claudeai".into()),
    });
    assert_eq!(kinds(&started)[0], "sign-in claude-code: starting");
    // The CLI writes its address line and its prompt together, and they
    // reach the app in one read or in two (a terminal hands a line over at
    // its end), so the address may be published a moment before the prompt:
    // each change of the view is published, whichever way they arrive.
    let waiting = "sign-in claude-code: waiting url input";
    let updates = app.wait_until_last(from, waiting, "the address and the prompt");
    let seen = kinds(&updates);
    assert!(
        seen == ["sign-in claude-code: starting", waiting]
            || seen
                == [
                    "sign-in claude-code: starting",
                    "sign-in claude-code: waiting url",
                    waiting
                ],
        "{seen:?}"
    );
    let Some(Update::SignIn {
        state: Some(view), ..
    }) = updates.last()
    else {
        unreachable!()
    };
    assert_eq!(
        view.url.as_deref(),
        Some("https://claude.com/cai/oauth/authorize?code=true&state=fake")
    );
    // Opening Settings in the middle of it shows it.
    let view_now = settings_of(&app.command(Command::ReadSettings { refresh: false }));
    assert_eq!(
        provider(&view_now, "claude-code")
            .sign_in
            .as_ref()
            .unwrap()
            .current
            .as_ref()
            .map(|v| v.state),
        Some(SignInState::Waiting)
    );
    // A second start while it runs is refused, with the state as it is.
    assert_eq!(
        kinds(&app.command(Command::SignIn {
            provider: "claude-code".into(),
            method: Some("console".into()),
        })),
        [
            "error: A sign-in is already under way here. Finish it or cancel it first.",
            waiting
        ]
    );
    // A wrong code: the command answers the state, then the CLI's line.
    let from = app.len();
    let invalid = "sign-in claude-code: waiting url input «Invalid code. Please make sure the full code was copied.»";
    assert_eq!(
        kinds(&app.command(Command::SignInInput {
            provider: "claude-code".into(),
            text: "bad#fake".into()
        })),
        [waiting]
    );
    app.let_the_cli_answer();
    assert_eq!(
        kinds(&app.wait_until_last(from, invalid, "the wrong code's line")),
        [waiting, invalid]
    );
    // The right code: the state, the CLI's last line, done, then the
    // providers and the settings read fresh.
    let from = app.len();
    app.command(Command::SignInInput {
        provider: "claude-code".into(),
        text: "good#fake".into(),
    });
    app.let_the_cli_answer();
    let updates = app.wait_until_last(from, "settings", "the sign-in's end");
    assert_eq!(
        kinds(&updates),
        [
            invalid,
            "sign-in claude-code: waiting url input «Login successful.»",
            "sign-in claude-code: done url input «Login successful.»",
            "providers",
            "settings"
        ]
    );
    let view = settings_of(&updates);
    let claude = provider(&view, "claude-code");
    assert!(claude.signed_in && claude.connected);
    assert_eq!(claude.account.as_deref(), Some("person@example.com"));
    assert_eq!(claude.summary, "Signed in · Claude Max");
    assert_eq!(
        claude
            .sign_in
            .as_ref()
            .unwrap()
            .current
            .as_ref()
            .unwrap()
            .state,
        SignInState::Done
    );
    let Update::Providers(Ok(list)) = &updates[3] else {
        unreachable!()
    };
    assert!(
        list.iter()
            .find(|p| p.id == "claude-code")
            .unwrap()
            .connected
    );
    // Nothing more came.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(app.since(from).len(), 5, "{:?}", kinds(&app.since(from)));
    assert_eq!(
        std::fs::read_to_string(claude_dir().join("browser-login")).unwrap(),
        "true",
        "the CLI opens no browser"
    );

    // A sign-out the CLI refuses, then one it does.
    std::fs::write(claude_dir().join("refuse-logout"), "").unwrap();
    assert_eq!(
        kinds(&app.command(Command::SignOut {
            provider: "claude-code".into()
        })),
        ["error: Could not log out: refused", "providers", "settings"]
    );
    std::fs::remove_file(claude_dir().join("refuse-logout")).unwrap();
    let updates = app.command(Command::SignOut {
        provider: "claude-code".into(),
    });
    assert_eq!(kinds(&updates), ["providers", "settings"]);
    assert_eq!(
        provider(&settings_of(&updates), "claude-code").summary,
        "Not signed in"
    );
    assert_eq!(
        std::fs::read_to_string(claude_dir().join("browser-logout")).unwrap(),
        "true"
    );

    // A cancel: the change, then the providers and the settings; a cancel
    // of one that ended answers its state.
    let from = app.len();
    app.command(Command::SignIn {
        provider: "claude-code".into(),
        method: Some("console".into()),
    });
    app.wait_until_last(from, waiting, "the second sign-in's address and prompt");
    let from = app.len();
    let cancelled = app.command(Command::CancelSignIn {
        provider: "claude-code".into(),
    });
    assert_eq!(
        kinds(&cancelled)[0],
        "sign-in claude-code: cancelled url input"
    );
    let updates = app.wait_until_last(from, "settings", "the cancel's settings");
    assert_eq!(
        kinds(&updates),
        [
            "sign-in claude-code: cancelled url input",
            "providers",
            "settings"
        ]
    );
    assert_eq!(
        kinds(&app.command(Command::CancelSignIn {
            provider: "claude-code".into(),
        })),
        ["sign-in claude-code: cancelled url input"]
    );

    // Settings that cannot be read: the login file is not JSON.
    std::fs::write(claude_dir().join(".claude.json"), "{ not json").unwrap();
    let updates = app.command(Command::ReadSettings { refresh: true });
    let [Update::Settings(Err(unread))] = &updates[..] else {
        panic!("{:?}", kinds(&updates));
    };
    // The providers cannot be read either (`GET /api/config` reads the same
    // login): one `Update::Providers` with the read's text, V8's for the
    // file, and no `Update::Error` beside it. `ReadProviders` is the page's
    // `Try again`: the same while the file is broken, the list once it is
    // not.
    let failed = app.command(Command::ReadProviders);
    let [Update::Providers(Err(text))] = &failed[..] else {
        panic!("{:?}", kinds(&failed));
    };
    assert_eq!(text, unread);
    assert_eq!(
        text,
        "Expected property name or '}' in JSON at position 2 (line 1 column 3)"
    );
    std::fs::remove_file(claude_dir().join(".claude.json")).unwrap();
    let read = app.command(Command::ReadProviders);
    let [Update::Providers(Ok(list))] = &read[..] else {
        panic!("{:?}", kinds(&read));
    };
    assert!(
        !list
            .iter()
            .find(|p| p.id == "claude-code")
            .unwrap()
            .connected
    );
}

/// An install driven by a fake download through its states: the start,
/// each progress (held to ten a second), the end, each once and in order,
/// then the providers and the settings read fresh, which find the CLI. A
/// second install of it is refused, with the install as it ended.
#[cfg(unix)]
#[test]
fn an_install_publishes_each_change_once_in_order_and_ends_in_fresh_settings() {
    // Each step of the fake download is its progress readings, given one
    // after the other at once (inside the 100 ms that holds them back).
    let (step, steps) = tokio::sync::mpsc::unbounded_channel::<Option<Vec<Progress>>>();
    let steps = Arc::new(tokio::sync::Mutex::new(steps));
    let tools: Arc<Mutex<Option<PathBuf>>> = Arc::default();
    let tools_of = tools.clone();
    let installer: Installer = Arc::new(
        move |pin: Pin, progress: Arc<dyn Fn(Progress) + Send + Sync>| {
            let (steps, tools) = (steps.clone(), tools_of.clone());
            Box::pin(async move {
                while let Some(Some(readings)) = steps.lock().await.recv().await {
                    for bytes in readings {
                        progress(bytes);
                    }
                }
                let folder = tools.lock().unwrap().clone().unwrap();
                let (_, program) =
                    pin.installed(&folder, &mcbench_providers::harness::install::platform());
                std::fs::create_dir_all(program.parent().unwrap()).unwrap();
                std::fs::write(&program, "#!/bin/sh\necho codex-cli 0.169.0\n").unwrap();
                std::fs::set_permissions(
                    &program,
                    std::os::unix::fs::PermissionsExt::from_mode(0o755),
                )
                .unwrap();
                Ok(program)
            })
        },
    );
    let app = App::with_installer("install", Some(installer));
    *tools.lock().unwrap() = Some(app.data.join("tools"));
    let view = settings_of(&app.command(Command::ReadSettings { refresh: false }));
    assert!(
        !provider(&view, "codex-cli")
            .sign_in
            .as_ref()
            .unwrap()
            .installed
    );
    let from = app.len();
    assert_eq!(
        kinds(&app.command(Command::InstallCli {
            program: "codex".into()
        })),
        ["install codex: running"]
    );
    // Settings opened meanwhile show it.
    let view = settings_of(&app.command(Command::ReadSettings { refresh: false }));
    assert_eq!(
        provider(&view, "codex")
            .sign_in
            .as_ref()
            .unwrap()
            .install
            .as_ref()
            .map(|i| i.state.as_str()),
        Some("running")
    );
    let from_progress = app.len();
    let at = |done| Progress {
        done,
        total: Some(100),
    };
    step.send(Some(vec![at(10), at(11)])).unwrap();
    app.wait_until_last(
        from_progress,
        "install codex: running 10/Some(100)",
        "the first progress",
    );
    // Past the 100 ms that held 11 back (a sleep is at least its length).
    std::thread::sleep(Duration::from_millis(150));
    step.send(Some(vec![at(60)])).unwrap();
    app.wait_until_last(
        from_progress,
        "install codex: running 60/Some(100)",
        "the second progress",
    );
    step.send(None).unwrap();
    let updates = app.wait_until_last(from, "settings", "the install's end");
    assert_eq!(
        kinds(&updates),
        [
            "install codex: running",
            "settings",
            "install codex: running 10/Some(100)",
            "install codex: running 60/Some(100)",
            "install codex: done",
            "providers",
            "settings"
        ]
    );
    let view = settings_of(&updates);
    let codex = provider(&view, "codex-cli");
    assert_eq!(codex.version.as_deref(), Some("0.169.0"));
    assert!(codex.sign_in.as_ref().unwrap().installed);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(app.since(from).len(), 7, "{:?}", kinds(&app.since(from)));
    assert_eq!(
        kinds(&app.command(Command::InstallCli {
            program: "codex".into()
        })),
        [
            "error: codex is installed already, and the app runs that one",
            "install codex: done",
            "settings"
        ]
    );
}
