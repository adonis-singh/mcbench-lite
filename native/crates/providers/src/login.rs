//! Signing in to a subscription from Settings (`server/provider-login.js`,
//! without its HTTP routes). The app does what a terminal does: it starts
//! the provider's own login (the CLI's login command), shows the address and
//! one-time code the login prints, and passes a line the person types
//! (Claude Code's pasted code) to the CLI's stdin. Anthropic's terms for
//! hosting Claude Code ask for exactly this: the unmodified CLI with both of
//! its sign-in methods, each person signing in through Anthropic's own pages,
//! and no credential held by the app. The line the person types goes to the
//! CLI and nowhere else: it is not kept, logged or returned. The login is
//! the CLI's: it writes its own files (Claude Code's `.claude.json`, Codex's
//! `auth.json`), so a data folder signed in under Node is signed in here.
//!
//! No flow opens a browser: each CLI is started with `BROWSER=true` (its
//! command comes from the harness, [`LoginPrograms`]), and the address it
//! prints is in [`SignInView::url`] for the app to open or show.
//!
//! Each CLI was run with pipes for stdin and stdout and prints its address
//! there, so no pseudo-terminal is needed: Claude Code 2.1.322 prints the
//! address, then "Paste code here if prompted > ", and reads "code#state"
//! lines from stdin; Codex 0.167.1 prints the address and a one-time code in
//! colour, then waits for OpenAI to see the code entered. Cursor's SDK login
//! is not here: Cursor is not in the native app.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::ProviderId;
use crate::changes::{Changes, Watcher};

/// A sign-in nobody finishes is ended after this long (`LIMIT_MS`).
pub const LIMIT: Duration = Duration::from_secs(10 * 60);
const LAST_LINES: usize = 4;

/// A login, which may serve several providers: ChatGPT sign-in and the
/// Codex CLI share Codex's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Login {
    /// Claude Code's (`claude auth login`).
    Claude,
    /// Codex's (`codex login --device-auth`).
    Codex,
}

impl Login {
    /// The login a provider signs in with (`loginOf`).
    pub fn of(provider: ProviderId) -> Result<Login, LoginError> {
        match provider {
            ProviderId::ClaudeCode => Ok(Login::Claude),
            ProviderId::Codex | ProviderId::CodexCli => Ok(Login::Codex),
            other => Err(LoginError::new(
                400,
                format!("{} has no sign-in", other.key()),
            )),
        }
    }

    /// The sign-in methods the person picks from: Claude Code's `claude auth
    /// login` offers a Claude subscription (`--claudeai`) and the Anthropic
    /// Console (`--console`), and both stay offered.
    pub fn methods(self) -> Option<&'static [&'static str]> {
        match self {
            Login::Claude => Some(&["claudeai", "console"]),
            Login::Codex => None,
        }
    }

    /// What the login printed that the page shows, read from all its output
    /// so far, or `None` while its address is not out.
    pub fn shown(self, output: &str) -> Option<Shown> {
        match self {
            Login::Claude => claude_shown(output),
            Login::Codex => codex_shown(output),
        }
    }
}

/// A program to run (`{file, args, env}`), with its whole environment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// The program.
    pub file: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its environment (nothing else is inherited).
    pub env: Vec<(String, String)>,
}

/// The CLIs' side, which the harnesses know (`claude-code.js`
/// `authCommand`, `storedLogin`; `codex-cli.js` `loginCommand`,
/// `storedLogin`).
pub trait LoginPrograms: Send + Sync {
    /// The login command (`claude auth login --<method>`, `codex login
    /// --device-auth`), with `BROWSER=true`.
    fn login(&self, login: Login, method: Option<&str>) -> Command;
    /// The logout command (`claude auth logout`, `codex logout`).
    fn logout(&self, login: Login) -> Command;
    /// Whether the login is stored; a login file that cannot be read is
    /// the error today's `storedLogin` throws (its `JSON.parse` sentence).
    fn stored(&self, login: Login) -> Result<bool, String>;
}

/// A refusal, with the HTTP status today's route answers with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoginError {
    /// 400, 404 or 409 (a request the state refuses); 500 for a logout
    /// that failed.
    pub status: u16,
    /// The sentence.
    pub message: String,
}

impl LoginError {
    fn new(status: u16, message: impl Into<String>) -> LoginError {
        LoginError {
            status,
            message: message.into(),
        }
    }
}

/// What the page shows of a sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shown {
    /// The address to open.
    pub url: String,
    /// Codex's one-time code.
    pub user_code: Option<String>,
    /// Whether the CLI asks for a pasted code.
    pub wants_input: bool,
    /// The last line the CLI printed after asking for a code.
    pub message: Option<String>,
}

/// A sign-in's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Its login runs; its address is not out yet.
    Starting,
    /// The address is shown: the login waits for the person.
    Waiting,
    /// The login ended well and is stored.
    Done,
    /// It ended any other way, or ran past [`LIMIT`].
    Failed,
    /// The person ended it.
    Cancelled,
}

impl State {
    /// Its name (`starting`, `waiting`, `done`, `failed`, `cancelled`).
    pub fn name(self) -> &'static str {
        match self {
            State::Starting => "starting",
            State::Waiting => "waiting",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
        }
    }

    /// `MOVES`.
    fn moves(self) -> &'static [State] {
        match self {
            State::Starting => &[State::Waiting, State::Done, State::Failed, State::Cancelled],
            State::Waiting => &[State::Done, State::Failed, State::Cancelled],
            _ => &[],
        }
    }

    fn running(self) -> bool {
        matches!(self, State::Starting | State::Waiting)
    }
}

/// `signIn.view()`: `{state, url, userCode, wantsInput, message, error}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInView {
    /// The state.
    pub state: State,
    /// The address, once out.
    pub url: Option<String>,
    /// Codex's one-time code.
    pub user_code: Option<String>,
    /// Whether a pasted code is asked for.
    pub wants_input: bool,
    /// The CLI's last line after the prompt.
    pub message: Option<String>,
    /// How it failed.
    pub error: Option<String>,
}

impl SignInView {
    /// The object today's route answers with.
    pub fn to_json(&self) -> serde_json::Value {
        let text = |v: &Option<String>| {
            v.clone()
                .map(serde_json::Value::String)
                .unwrap_or(serde_json::Value::Null)
        };
        let mut out = serde_json::Map::new();
        out.insert("state".into(), self.state.name().into());
        out.insert("url".into(), text(&self.url));
        out.insert("userCode".into(), text(&self.user_code));
        out.insert("wantsInput".into(), self.wants_input.into());
        out.insert("message".into(), text(&self.message));
        out.insert("error".into(), text(&self.error));
        serde_json::Value::Object(out)
    }
}

struct Inner {
    view: SignInView,
    output: String,
    stdin: Option<tokio::process::ChildStdin>,
}

/// One sign-in.
struct SignIn {
    login: Login,
    inner: Mutex<Inner>,
    /// Ends the login's process.
    end: CancellationToken,
    /// Ends the 10-minute timer.
    timer: CancellationToken,
    programs: Arc<dyn LoginPrograms>,
    /// Told of each change of the view.
    changes: Arc<Changes<(Login, SignInView)>>,
}

impl SignIn {
    /// Tells the watcher of the view as `inner` holds it now; called with
    /// the lock held, so changes are told in their order.
    fn changed(&self, inner: &Inner) {
        self.changes.changed((self.login, inner.view.clone()));
    }

    fn view(&self) -> SignInView {
        self.inner.lock().expect("the sign-in").view.clone()
    }

    fn running(&self) -> bool {
        self.inner.lock().expect("the sign-in").view.state.running()
    }

    fn to(&self, inner: &mut Inner, state: State) {
        assert!(
            inner.view.state.moves().contains(&state),
            "A sign-in cannot go from {} to {}",
            inner.view.state.name(),
            state.name()
        );
        inner.view.state = state;
        if state.moves().is_empty() {
            self.timer.cancel();
        }
    }

    /// The address is out: the page can show it.
    fn show(&self, inner: &mut Inner, shown: Shown) {
        inner.view.url = Some(shown.url);
        inner.view.user_code = shown.user_code;
        inner.view.wants_input = shown.wants_input;
        inner.view.message = shown.message;
        if inner.view.state == State::Starting {
            self.to(inner, State::Waiting);
        }
    }

    /// Ends the login for `state` (failed or cancelled); the process's exit
    /// that follows changes nothing.
    fn stop(&self, state: State, error: Option<String>) {
        let mut inner = self.inner.lock().expect("the sign-in");
        if !inner.view.state.running() {
            return;
        }
        self.to(&mut inner, state);
        inner.view.error = error;
        self.changed(&inner);
        self.end.cancel();
    }

    /// The login ended by itself: well only if it says so and its login is
    /// stored.
    fn finish(&self, ok: bool, error: String) {
        // A login file that cannot be read fails the sign-in with its error
        // (today it is thrown in the CLI's close handler).
        let (stored, error) = match ok.then(|| self.programs.stored(self.login)) {
            Some(Ok(stored)) => (stored, error),
            Some(Err(unreadable)) => (false, unreadable),
            None => (false, error),
        };
        let mut inner = self.inner.lock().expect("the sign-in");
        if !inner.view.state.running() {
            return;
        }
        if stored {
            self.to(&mut inner, State::Done);
            self.changed(&inner);
            return;
        }
        self.to(&mut inner, State::Failed);
        inner.view.error = Some(if error.is_empty() {
            "The sign-in ended, but no login was stored. Start it again.".into()
        } else {
            error
        });
        self.changed(&inner);
    }

    fn take(&self, chunk: &[u8]) {
        let text = strip_terminal_sequences(&String::from_utf8_lossy(chunk)).replace('\r', "");
        let mut inner = self.inner.lock().expect("the sign-in");
        inner.output.push_str(&text);
        if !inner.view.state.running() {
            return;
        }
        if let Some(shown) = self.login.shown(&inner.output) {
            let before = inner.view.clone();
            self.show(&mut inner, shown);
            if inner.view != before {
                self.changed(&inner);
            }
        }
    }
}

/// The sign-ins of this run of the app: the latest one per login, kept
/// after it ends so the page can show how it ended.
pub struct SignIns {
    programs: Arc<dyn LoginPrograms>,
    latest: Mutex<HashMap<Login, Arc<SignIn>>>,
    changes: Arc<Changes<(Login, SignInView)>>,
}

impl SignIns {
    /// No sign-in yet; the CLIs' commands come from `programs`.
    pub fn new(programs: Arc<dyn LoginPrograms>) -> SignIns {
        SignIns {
            programs,
            latest: Mutex::default(),
            changes: Arc::default(),
        }
    }

    /// Calls `watcher` with each change of a sign-in's view, of every
    /// login: its start, the address and code arriving, `wants_input`, the
    /// last line, its end ([`crate::changes`]). Set once.
    pub fn watch(&self, watcher: Watcher<(Login, SignInView)>) {
        self.changes.watch(watcher);
    }

    fn get(&self, login: Login) -> Option<Arc<SignIn>> {
        self.latest
            .lock()
            .expect("the sign-ins")
            .get(&login)
            .cloned()
    }

    /// `start(provider, method)`: a new sign-in, unless one is under way.
    pub fn start(
        &self,
        provider: ProviderId,
        method: Option<&str>,
    ) -> Result<SignInView, LoginError> {
        let login = Login::of(provider)?;
        if self.get(login).is_some_and(|s| s.running()) {
            return Err(LoginError::new(
                409,
                "A sign-in is already under way here. Finish it or cancel it first.",
            ));
        }
        if let Some(methods) = login.methods()
            && !method.is_some_and(|m| methods.contains(&m))
        {
            return Err(LoginError::new(
                400,
                format!("Choose a sign-in method: {}", methods.join(" or ")),
            ));
        }
        let command = self.programs.login(login, method);
        let sign_in = Arc::new(SignIn {
            login,
            inner: Mutex::new(Inner {
                view: SignInView {
                    state: State::Starting,
                    url: None,
                    user_code: None,
                    wants_input: false,
                    message: None,
                    error: None,
                },
                output: String::new(),
                stdin: None,
            }),
            end: CancellationToken::new(),
            timer: CancellationToken::new(),
            programs: self.programs.clone(),
            changes: self.changes.clone(),
        });
        self.latest
            .lock()
            .expect("the sign-ins")
            .insert(login, sign_in.clone());
        sign_in.changed(&sign_in.inner.lock().expect("the sign-in"));
        let timed = sign_in.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(LIMIT) => timed.stop(
                    State::Failed,
                    Some(format!("The sign-in was not finished within {} minutes, so it was ended. Start it again when you are ready.", LIMIT.as_secs() / 60)),
                ),
                _ = timed.timer.cancelled() => {}
            }
        });
        run_cli(sign_in.clone(), command);
        Ok(sign_in.view())
    }

    fn current(&self, provider: ProviderId) -> Result<Arc<SignIn>, LoginError> {
        self.get(Login::of(provider)?)
            .ok_or_else(|| LoginError::new(404, "No sign-in was started here"))
    }

    /// `state(provider)`: the latest sign-in of its login, or `None`.
    pub fn state(&self, provider: ProviderId) -> Result<Option<SignInView>, LoginError> {
        Ok(self.get(Login::of(provider)?).map(|s| s.view()))
    }

    /// `input(provider, text)`: the person's line goes to the CLI's stdin as
    /// typed, and nowhere else.
    pub async fn input(&self, provider: ProviderId, text: &str) -> Result<SignInView, LoginError> {
        let sign_in = self.current(provider)?;
        let stdin = {
            let mut inner = sign_in.inner.lock().expect("the sign-in");
            if inner.view.state != State::Waiting || !inner.view.wants_input {
                return Err(LoginError::new(
                    409,
                    "This sign-in is not waiting for a code",
                ));
            }
            if crate::api::js::trim(text).is_empty() || text.contains(['\r', '\n']) {
                return Err(LoginError::new(400, "Paste the code as one line"));
            }
            inner.stdin.take()
        };
        if let Some(mut stdin) = stdin {
            // A code written as the CLI exits fails; the CLI's exit says how
            // it ended.
            let _ = stdin
                .write_all(format!("{}\n", crate::api::js::trim(text)).as_bytes())
                .await;
            let _ = stdin.flush().await;
            sign_in.inner.lock().expect("the sign-in").stdin = Some(stdin);
        }
        Ok(sign_in.view())
    }

    /// `cancel(provider)`.
    pub fn cancel(&self, provider: ProviderId) -> Result<SignInView, LoginError> {
        let sign_in = self.current(provider)?;
        sign_in.stop(State::Cancelled, None);
        Ok(sign_in.view())
    }

    /// `logout(provider)`: the provider's own command removes its login. A
    /// sign-in under way is finished or cancelled first.
    pub async fn logout(&self, provider: ProviderId) -> Result<(), LoginError> {
        let login = Login::of(provider)?;
        if self.get(login).is_some_and(|s| s.running()) {
            return Err(LoginError::new(409, "Finish or cancel the sign-in first"));
        }
        let command = self.programs.logout(login);
        let output = tokio::process::Command::from(mcbench_spawn::command(&command.file))
            .args(&command.args)
            .env_clear()
            .envs(command.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .stdin(Stdio::null())
            .output()
            .await
            .map_err(|error| LoginError::new(500, spawn_error(&command.file, &error)))?;
        if output.status.success() {
            return Ok(());
        }
        let mut text = strip_terminal_sequences(&String::from_utf8_lossy(&output.stdout));
        text.push_str(&strip_terminal_sequences(&String::from_utf8_lossy(
            &output.stderr,
        )));
        let code = output
            .status
            .code()
            .map_or("null".to_owned(), |c| c.to_string());
        Err(LoginError::new(
            500,
            last_lines(&text, LAST_LINES).unwrap_or_else(|| {
                format!(
                    "{} {} ended with exit code {code}",
                    command.file,
                    command.args.join(" ")
                )
            }),
        ))
    }

    /// Ends every sign-in under way, when the app stops.
    pub fn shutdown(&self) {
        for sign_in in self.latest.lock().expect("the sign-ins").values() {
            sign_in.stop(State::Cancelled, None);
        }
    }
}

/// Node's `spawn` error: `spawn <file> ENOENT`.
fn spawn_error(file: &str, error: &std::io::Error) -> String {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT".to_owned(),
        std::io::ErrorKind::PermissionDenied => "EACCES".to_owned(),
        _ => error.to_string(),
    };
    format!("spawn {file} {code}")
}

/// `runCli`: the login's process, its output read as it comes, its exit.
fn run_cli(sign_in: Arc<SignIn>, command: Command) {
    let spawned = tokio::process::Command::from(mcbench_spawn::command(&command.file))
        .args(&command.args)
        .env_clear()
        .envs(command.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => {
            let message = format!(
                "{} could not be started: {}",
                command.file,
                spawn_error(&command.file, &error)
            );
            sign_in.finish(false, message);
            return;
        }
    };
    // On Windows the sign-in's whole tree, so a cancel ends all of it.
    let tree = match crate::harness::tree::Tree::holding(&child) {
        Ok(tree) => tree,
        Err(error) => {
            let _ = child.start_kill();
            sign_in.finish(
                false,
                format!("{} could not be started: {error}", command.file),
            );
            return;
        }
    };
    sign_in.inner.lock().expect("the sign-in").stdin = child.stdin.take();
    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
    ]
    .into_iter()
    .flatten()
    .map(|mut stream| {
        let sign_in = sign_in.clone();
        tokio::spawn(async move {
            let mut buffer = vec![0u8; 64 * 1024];
            while let Ok(n) = stream.read(&mut buffer).await {
                if n == 0 {
                    break;
                }
                sign_in.take(&buffer[..n]);
            }
        })
    })
    .collect();
    tokio::spawn(async move {
        let status = tokio::select! {
            status = child.wait() => status,
            _ = sign_in.end.cancelled() => {
                terminate(&mut child, &tree);
                child.wait().await
            }
        };
        tree.close();
        for reader in readers {
            let _ = reader.await;
        }
        let ended = match &status {
            Ok(status) => match status.code() {
                Some(code) => format!("exit code {code}"),
                None => signal_name(status),
            },
            Err(error) => error.to_string(),
        };
        let ok = status.as_ref().is_ok_and(|s| s.success());
        let output = sign_in.inner.lock().expect("the sign-in").output.clone();
        let error = last_lines(&output, LAST_LINES)
            .unwrap_or_else(|| format!("{} ended with {ended}", command.file));
        sign_in.finish(ok, error);
    });
}

/// `child.kill('SIGTERM')`; on Windows, which has no SIGTERM, the sign-in's
/// whole tree ends.
fn terminate(child: &mut tokio::process::Child, tree: &crate::harness::tree::Tree) {
    #[cfg(windows)]
    {
        let _ = &child;
        tree.end();
    }
    #[cfg(not(windows))]
    let _ = tree;
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // SAFETY: kill(2) with the pid of a child this process started and
        // has not reaped (it is reaped by the `wait` that follows), and a
        // valid signal number; it touches no memory.
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}

/// The name of the signal that ended a process, as Node reports it.
fn signal_name(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        let names = [
            (libc::SIGHUP, "SIGHUP"),
            (libc::SIGINT, "SIGINT"),
            (libc::SIGQUIT, "SIGQUIT"),
            (libc::SIGABRT, "SIGABRT"),
            (libc::SIGKILL, "SIGKILL"),
            (libc::SIGSEGV, "SIGSEGV"),
            (libc::SIGPIPE, "SIGPIPE"),
            (libc::SIGTERM, "SIGTERM"),
        ];
        if let Some(signal) = status.signal() {
            return names.iter().find(|(n, _)| *n == signal).map_or_else(
                || format!("signal {signal}"),
                |(_, name)| (*name).to_owned(),
            );
        }
    }
    format!("{status}")
}

/// `lastLines(text, count)`: its last non-blank lines, trimmed.
pub fn last_lines(text: &str, count: usize) -> Option<String> {
    let lines: Vec<&str> = text
        .split('\n')
        .map(crate::api::js::trim)
        .filter(|line| !line.is_empty())
        .collect();
    (!lines.is_empty()).then(|| lines[lines.len().saturating_sub(count)..].join("\n"))
}

/// Removes `TERMINAL_SEQUENCES`: CSI sequences (`ESC [ params
/// intermediates final`) and OSC sequences ended by BEL or `ESC \`.
pub fn strip_terminal_sequences(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\x1b'
            && let Some(end) = csi_end(&chars, i).or_else(|| osc_end(&chars, i))
        {
            i = end;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// `\x1b\[[0-9;?]*[ -/]*[@-~]`.
fn csi_end(chars: &[char], at: usize) -> Option<usize> {
    if chars.get(at + 1) != Some(&'[') {
        return None;
    }
    let mut i = at + 2;
    while chars
        .get(i)
        .is_some_and(|c| c.is_ascii_digit() || *c == ';' || *c == '?')
    {
        i += 1;
    }
    while chars.get(i).is_some_and(|c| (' '..='/').contains(c)) {
        i += 1;
    }
    chars
        .get(i)
        .filter(|c| ('@'..='~').contains(*c))
        .map(|_| i + 1)
}

/// `\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)`.
fn osc_end(chars: &[char], at: usize) -> Option<usize> {
    if chars.get(at + 1) != Some(&']') {
        return None;
    }
    let mut i = at + 2;
    while chars.get(i).is_some_and(|c| *c != '\x07' && *c != '\x1b') {
        i += 1;
    }
    match chars.get(i) {
        Some('\x07') => Some(i + 1),
        Some('\x1b') if chars.get(i + 1) == Some(&'\\') => Some(i + 2),
        _ => None,
    }
}

/// `https://` and at least one more non-whitespace character, then `\n`,
/// at `at`: the address.
fn url_then_newline(text: &str, at: usize) -> Option<String> {
    let rest = text.get(at..)?.strip_prefix("https://")?;
    let run: usize = rest
        .chars()
        .take_while(|c| !crate::api::js::is_js_whitespace(*c))
        .map(char::len_utf8)
        .sum();
    (run > 0 && rest[run..].starts_with('\n')).then(|| format!("https://{}", &rest[..run]))
}

/// Skips `[^\n]*\n\s*` from `at`.
fn next_line_start(text: &str, at: usize) -> Option<usize> {
    let newline = at + text[at..].find('\n')?;
    let skipped: usize = text[newline + 1..]
        .chars()
        .take_while(|c| crate::api::js::is_js_whitespace(*c))
        .map(char::len_utf8)
        .sum();
    Some(newline + 1 + skipped)
}

/// Claude Code's: `/(?:visit|Sign in at): (https:\/\/\S+)\n/`, and its
/// prompt for a pasted code.
fn claude_shown(output: &str) -> Option<Shown> {
    let url = output.char_indices().find_map(|(at, _)| {
        ["visit: ", "Sign in at: "].iter().find_map(|label| {
            output[at..]
                .starts_with(label)
                .then(|| url_then_newline(output, at + label.len()))
                .flatten()
        })
    })?;
    let prompt = output.rfind("Paste code here");
    let after = match prompt {
        None => "",
        Some(prompt) => match output[prompt..].find('>') {
            Some(gt) => &output[prompt + gt + 1..],
            None => output,
        },
    };
    Some(Shown {
        url,
        user_code: None,
        wants_input: prompt.is_some(),
        message: last_lines(after, 1),
    })
}

/// Codex's: the address after "Open this link" and the one-time code after
/// "one-time code".
fn codex_shown(output: &str) -> Option<Shown> {
    let url = output
        .match_indices("Open this link")
        .find_map(|(at, _)| url_then_newline(output, next_line_start(output, at)?))?;
    let code = output.match_indices("one-time code").find_map(|(at, _)| {
        let start = next_line_start(output, at)?;
        let rest = &output[start..];
        let b = rest.as_bytes();
        let ok = |c: u8| c.is_ascii_uppercase() || c.is_ascii_digit();
        if b.len() < 5 || !b[..4].iter().all(|c| ok(*c)) || b[4] != b'-' {
            return None;
        }
        let tail = b[5..].iter().take_while(|c| ok(**c)).count();
        (tail >= 4 && b.get(5 + tail) == Some(&b'\n')).then(|| rest[..5 + tail].to_owned())
    })?;
    Some(Shown {
        url,
        user_code: Some(code),
        wants_input: false,
        message: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_prints_its_address_and_code_in_colour() {
        let printed = "\nWelcome to Codex [v\u{1b}[90m0.167.1\u{1b}[0m]\n\u{1b}[90mOpenAI's command-line coding agent\u{1b}[0m\n\n\
            Follow these steps to sign in with ChatGPT using device code authorization:\n\n\
            1. Open this link in your browser and sign in to your account\n   \u{1b}[94mhttps://auth.openai.com/codex/device\u{1b}[0m\n\n\
            2. Enter this one-time code \u{1b}[90m(expires in 15 minutes)\u{1b}[0m\n   \u{1b}[94mFAKE-C0DE1\u{1b}[0m\n\n";
        let shown = codex_shown(&strip_terminal_sequences(printed)).unwrap();
        assert_eq!(shown.url, "https://auth.openai.com/codex/device");
        assert_eq!(shown.user_code.as_deref(), Some("FAKE-C0DE1"));
    }

    #[test]
    fn claude_code_prints_its_address_then_asks_for_a_code() {
        let output = "Opening browser to sign in…\nIf the browser didn't open, visit: https://claude.com/cai/oauth/authorize?code=true&state=fake-state\nPaste code here if prompted > ";
        let shown = claude_shown(output).unwrap();
        assert_eq!(
            shown.url,
            "https://claude.com/cai/oauth/authorize?code=true&state=fake-state"
        );
        assert!(shown.wants_input);
        assert_eq!(shown.message, None);
        let refused = format!("{output}Invalid code. Please make sure the full code was copied.\n");
        assert_eq!(
            claude_shown(&refused).unwrap().message.as_deref(),
            Some("Invalid code. Please make sure the full code was copied.")
        );
        assert!(claude_shown("visit: https://x").is_none());
    }
}
