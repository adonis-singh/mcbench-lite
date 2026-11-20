//! `test/provider-login.test.js`'s sign-in flows (without the HTTP routes;
//! a route's answer is the status of [`LoginError`]), against the same fake
//! CLIs, `test/fixtures/fake-claude.js` and `fake-codex.js`, which print what
//! the real ones print. They need Node, so they run only with
//! `MCBENCH_PROVIDERS_FAKE_CLIS=<the folder holding the two fakes>` (and fail,
//! never skip, when it is set and something is missing).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mcbench_providers::ProviderId;
use mcbench_providers::login::{
    Command, LIMIT, Login, LoginError, LoginPrograms, SignInView, SignIns, State,
};

#[path = "scratch/mod.rs"]
mod scratch;

struct Fakes {
    /// Removed with the last handle on the fakes.
    _scratch: scratch::Scratch,
    fixtures: PathBuf,
    claude_dir: PathBuf,
    codex_home: PathBuf,
}

impl LoginPrograms for Fakes {
    fn login(&self, login: Login, method: Option<&str>) -> Command {
        match login {
            Login::Claude => self.command(
                "fake-claude.js",
                &["auth", "login", &format!("--{}", method.unwrap())],
                login,
            ),
            Login::Codex => self.command("fake-codex.js", &["login", "--device-auth"], login),
        }
    }
    fn logout(&self, login: Login) -> Command {
        match login {
            Login::Claude => self.command("fake-claude.js", &["auth", "logout"], login),
            Login::Codex => self.command("fake-codex.js", &["logout"], login),
        }
    }
    fn stored(&self, login: Login) -> Result<bool, String> {
        Ok(match login {
            Login::Claude => std::fs::read_to_string(self.claude_dir.join(".claude.json"))
                .ok()
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .is_some_and(|config| config.get("oauthAccount").is_some_and(|a| !a.is_null())),
            Login::Codex => self.codex_home.join("auth.json").exists(),
        })
    }
}

impl Fakes {
    fn command(&self, fake: &str, args: &[&str], login: Login) -> Command {
        let mut env = vec![
            ("PATH".to_owned(), std::env::var("PATH").unwrap_or_default()),
            ("BROWSER".to_owned(), "true".to_owned()),
        ];
        env.push(match login {
            Login::Claude => (
                "CLAUDE_CONFIG_DIR".into(),
                self.claude_dir.display().to_string(),
            ),
            Login::Codex => ("CODEX_HOME".into(), self.codex_home.display().to_string()),
        });
        Command {
            file: self.fixtures.join(fake).display().to_string(),
            args: args.iter().map(|a| a.to_string()).collect(),
            env,
        }
    }
}

fn setup(name: &str) -> (Arc<Fakes>, SignIns) {
    let fixtures = PathBuf::from(
        std::env::var("MCBENCH_PROVIDERS_FAKE_CLIS")
            .expect("MCBENCH_PROVIDERS_FAKE_CLIS names test/fixtures"),
    );
    for fake in ["fake-claude.js", "fake-codex.js"] {
        assert!(
            fixtures.join(fake).exists(),
            "{fake} is not in {}",
            fixtures.display()
        );
    }
    let scratch = scratch::Scratch::new(&format!("login-{name}"));
    let fakes = Arc::new(Fakes {
        fixtures,
        claude_dir: scratch.join("claude"),
        codex_home: scratch.join("codex"),
        _scratch: scratch,
    });
    std::fs::create_dir_all(&fakes.claude_dir).unwrap();
    std::fs::create_dir_all(&fakes.codex_home).unwrap();
    (fakes.clone(), SignIns::new(fakes))
}

async fn until(
    sign_ins: &SignIns,
    provider: ProviderId,
    check: impl Fn(&SignInView) -> bool,
) -> SignInView {
    // A minute, in steps of 20 ms: a hang fails, a slow machine does not.
    for _ in 0..3000 {
        let view = sign_ins.state(provider).unwrap().expect("a sign-in");
        if check(&view) {
            return view;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!(
        "the sign-in did not get there: {:?}",
        sign_ins.state(provider)
    );
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn status(result: Result<impl std::fmt::Debug, LoginError>) -> u16 {
    result.map(|_| 200).unwrap_or_else(|e| e.status)
}

#[tokio::test]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn claude_code_signs_in_with_a_pasted_code_that_reaches_the_cli_and_no_response() {
    let (fakes, sign_ins) = setup("claude");
    // No sign-in started yet.
    assert_eq!(sign_ins.state(ProviderId::ClaudeCode).unwrap(), None);
    let started = sign_ins
        .start(ProviderId::ClaudeCode, Some("claudeai"))
        .unwrap();
    assert_eq!(started.state, State::Starting);
    let waiting = until(&sign_ins, ProviderId::ClaudeCode, |v| {
        v.state == State::Waiting && v.wants_input
    })
    .await;
    let url = waiting.url.clone().unwrap();
    assert!(
        url.starts_with("https://claude.com/cai/oauth/authorize?code=true&")
            && url.ends_with("state=fake-state"),
        "{url}"
    );
    assert_eq!(waiting.user_code, None);
    // The CLI was told to open no browser itself.
    assert_eq!(read(&fakes.claude_dir.join("fake-browser")), "true");
    // A second start while one runs is refused.
    assert_eq!(
        status(sign_ins.start(ProviderId::ClaudeCode, Some("console"))),
        409
    );
    // A line that is not a whole code: the CLI says so and waits on.
    let partial = sign_ins
        .input(ProviderId::ClaudeCode, "half-a-code")
        .await
        .unwrap();
    assert!(!format!("{:?}", partial.to_json()).contains("half-a-code"));
    let refused = until(&sign_ins, ProviderId::ClaudeCode, |v| v.message.is_some()).await;
    assert_eq!(
        refused.message.as_deref(),
        Some("Invalid code. Please make sure the full code was copied.")
    );
    assert_eq!(refused.state, State::Waiting);
    let code = format!("good#{}", std::process::id());
    let sent = sign_ins.input(ProviderId::ClaudeCode, &code).await.unwrap();
    assert!(!sent.to_json().to_string().contains(&code));
    let done = until(&sign_ins, ProviderId::ClaudeCode, |v| {
        v.state != State::Waiting
    })
    .await;
    assert_eq!(done.state, State::Done);
    assert_eq!(read(&fakes.claude_dir.join("fake-received")), code);
    assert!(!done.to_json().to_string().contains(&code));
    assert!(fakes.stored(Login::Claude).unwrap());
    // Signing out runs `claude auth logout`, which removes the login.
    sign_ins.logout(ProviderId::ClaudeCode).await.unwrap();
    assert!(!fakes.stored(Login::Claude).unwrap());
}

#[tokio::test]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn a_claude_code_sign_in_that_fails_shows_the_clis_own_last_lines() {
    let (fakes, sign_ins) = setup("claude-fails");
    sign_ins
        .start(ProviderId::ClaudeCode, Some("console"))
        .unwrap();
    let waiting = until(&sign_ins, ProviderId::ClaudeCode, |v| {
        v.state == State::Waiting && v.wants_input
    })
    .await;
    assert!(
        waiting
            .url
            .unwrap()
            .starts_with("https://platform.claude.com/oauth/authorize?")
    );
    sign_ins
        .input(ProviderId::ClaudeCode, "bad#state")
        .await
        .unwrap();
    let failed = until(&sign_ins, ProviderId::ClaudeCode, |v| {
        v.state != State::Waiting
    })
    .await;
    assert_eq!(failed.state, State::Failed);
    assert!(
        failed
            .error
            .unwrap()
            .ends_with("Login failed: Request failed with status code 400")
    );
    assert!(!fakes.stored(Login::Claude).unwrap());
}

#[tokio::test]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn claude_code_offers_both_of_its_sign_in_methods_and_no_other() {
    let (_, sign_ins) = setup("methods");
    let refused = sign_ins
        .start(ProviderId::ClaudeCode, Some("sso"))
        .unwrap_err();
    assert_eq!(refused.status, 400);
    assert!(refused.message.contains("claudeai or console"));
    assert_eq!(status(sign_ins.start(ProviderId::OpenAi, None)), 400);
    assert_eq!(
        sign_ins.state(ProviderId::OpenRouter).unwrap_err().status,
        400
    );
}

#[tokio::test]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn a_code_is_passed_only_while_the_cli_asks_for_one_as_one_line() {
    let (fakes, sign_ins) = setup("one-line");
    sign_ins
        .start(ProviderId::ClaudeCode, Some("claudeai"))
        .unwrap();
    until(&sign_ins, ProviderId::ClaudeCode, |v| v.wants_input).await;
    let two_lines = sign_ins
        .input(ProviderId::ClaudeCode, "a#b\nc#d")
        .await
        .unwrap_err();
    assert_eq!(two_lines.status, 400);
    assert!(!two_lines.message.contains("a#b"));
    // Cancelling ends the CLI; the sign-in reads cancelled and takes no code.
    assert_eq!(
        sign_ins.cancel(ProviderId::ClaudeCode).unwrap().state,
        State::Cancelled
    );
    assert_eq!(
        status(sign_ins.input(ProviderId::ClaudeCode, "good#x").await),
        409
    );
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        sign_ins
            .state(ProviderId::ClaudeCode)
            .unwrap()
            .unwrap()
            .state,
        State::Cancelled
    );
    assert!(!fakes.claude_dir.join(".claude.json").exists());
}

#[tokio::test]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn codex_shows_the_address_and_one_time_code_and_the_login_serves_chatgpt_and_codex_cli() {
    let (fakes, sign_ins) = setup("codex");
    sign_ins.start(ProviderId::CodexCli, None).unwrap();
    let waiting = until(&sign_ins, ProviderId::Codex, |v| v.state == State::Waiting).await;
    assert_eq!(
        waiting.to_json(),
        serde_json::json!({"state": "waiting", "url": "https://auth.openai.com/codex/device", "userCode": "FAKE-C0DE1", "wantsInput": false, "message": null, "error": null})
    );
    assert_eq!(read(&fakes.codex_home.join("fake-browser")), "true");
    // One login for both: ChatGPT cannot start a second.
    assert_eq!(status(sign_ins.start(ProviderId::Codex, None)), 409);
    assert_eq!(status(sign_ins.logout(ProviderId::Codex).await), 409);
    std::fs::write(fakes.codex_home.join("fake-approve"), "").unwrap();
    let done = until(&sign_ins, ProviderId::CodexCli, |v| {
        v.state != State::Waiting
    })
    .await;
    assert_eq!(done.state, State::Done);
    std::fs::remove_file(fakes.codex_home.join("fake-approve")).unwrap();
    assert!(fakes.stored(Login::Codex).unwrap());
    sign_ins.logout(ProviderId::Codex).await.unwrap();
    assert!(!fakes.stored(Login::Codex).unwrap());
    // Signed out already: `codex logout` says so and exits 0.
    sign_ins.logout(ProviderId::CodexCli).await.unwrap();
}

#[tokio::test]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn a_codex_sign_in_that_is_denied_shows_the_clis_message() {
    let (fakes, sign_ins) = setup("denied");
    sign_ins.start(ProviderId::Codex, None).unwrap();
    until(&sign_ins, ProviderId::Codex, |v| v.state == State::Waiting).await;
    std::fs::write(fakes.codex_home.join("fake-deny"), "").unwrap();
    let failed = until(&sign_ins, ProviderId::Codex, |v| v.state != State::Waiting).await;
    assert_eq!(failed.state, State::Failed);
    assert!(
        failed
            .error
            .unwrap()
            .ends_with("Error logging in: device code exchange failed: the code was denied")
    );
}

#[tokio::test(start_paused = true)]
#[ignore = "needs Node and the fake CLIs: set MCBENCH_PROVIDERS_FAKE_CLIS=<checkout>/test/fixtures"]
async fn a_sign_in_nobody_finishes_ends_after_10_minutes() {
    let (_, sign_ins) = setup("limit");
    let started = tokio::time::Instant::now();
    sign_ins.start(ProviderId::Codex, None).unwrap();
    // The paused clock runs on only while nothing else can: it reaches the
    // limit's timer, and the sign-in fails then and not before.
    let mut ended = None;
    for _ in 0..2 * (LIMIT.as_secs() + 60) {
        let view = sign_ins.state(ProviderId::Codex).unwrap().unwrap();
        if view.state == State::Failed {
            ended = Some(view);
            break;
        }
        // At the limit itself this check may run before the limit's timer.
        assert!(
            started.elapsed() <= LIMIT,
            "running past the limit: {view:?}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let ended = ended.expect("the sign-in ended");
    let elapsed = started.elapsed();
    assert!(
        elapsed >= LIMIT && elapsed < LIMIT + Duration::from_secs(1),
        "{elapsed:?}"
    );
    assert!(
        ended
            .error
            .unwrap()
            .contains("not finished within 10 minutes")
    );
    // The process is ended, and a new sign-in can start.
    assert_eq!(
        sign_ins.start(ProviderId::Codex, None).unwrap().state,
        State::Starting
    );
    sign_ins.cancel(ProviderId::Codex).unwrap();
}
