//! Codex CLI (`codex exec`) as a harness (`server/codex-cli.js`).
//!
//! The catalogue's models are all `tool_mode: code_mode_only`: the model's
//! one tool is Code Mode's `exec`, JavaScript run in Codex's
//! codex-code-mode-host that calls the mcbench tools as
//! `tools.mcp__mcbench__*`. Beside them `exec` reaches built-ins that no
//! feature flag removes (apply_patch, MCP resource listing, a clock), so on
//! Linux Codex runs sandboxed with only an empty workspace, its own
//! CODEX_HOME and the ChatGPT login, and its only route out is the
//! allow-list proxy to the model API hosts ([`Launch::Sandboxed`], built by
//! `mcbench-sandbox`). On macOS Codex runs on the host ([`Launch::Host`]):
//! each chat's own CODEX_HOME linking to the login, user config, rules and
//! AGENTS.md ignored, every setting passed as an override, writes denied by
//! Codex's read-only sandbox. Each chat keeps one Codex thread that
//! follow-up turns resume, a turn that was stopped or failed included. A
//! completed turn's context reading comes from the thread's rollout file.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use super::completion;
use super::process::{self, JsonLinesRun};
use super::relay::{Relay, State as RelayState, Waiter};
use super::{BridgeCommand, CommandSpec, Env, HarnessRequest, PromptImage, Record, text};
use crate::bridge::{self, BridgeHost, McpContent, js_string, truthy};
use crate::catalog::CodexModels;
use crate::{
    AssistantMessage, CliRuntime, CompletionRequest, Delta, ModelFacts, ModelInfo, Provider,
    ProviderError, ProviderId, ToolSchema,
};

/// The harness's name in records and folders.
pub const NAME: &str = "codex-cli";
/// The ChatGPT-login model API and its token refresh endpoint: the only
/// hosts the sandboxed Codex reaches.
pub const ALLOWED_HOSTS: [&str; 2] = ["chatgpt.com", "auth.openai.com"];
/// Features that would give the model a tool of Codex's own (each known
/// and not deprecated in Codex 0.167.1 and 0.169.0).
pub const DISABLED_FEATURES: [&str; 20] = [
    "shell_tool",
    "unified_exec",
    "view_image",
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "image_generation",
    "plugins",
    "memories",
    "goals",
    "in_app_browser",
    "tool_suggest",
    "skill_search",
    "remote_plugin",
    "shell_snapshot",
    "skill_mcp_dependency_install",
    "workspace_dependencies",
    "sleep_tool",
    "multi_agent_v2",
];
/// Code Mode's host runs `exec`, the only way a code_mode_only model
/// reaches mcbench's tools.
pub const ENABLED_FEATURES: [&str; 1] = ["code_mode_host"];
/// What a person reads when Codex reports a thread whose id is not a UUID.
/// The id is stored for the chat's next turn, which passes it to `codex
/// exec resume`; a line a process in the box wrote to Codex's stdout can
/// claim to be a `thread.started` of any id.
pub const NOT_A_THREAD_ID: &str = "The Codex CLI started a thread whose id is not a UUID, the form of every Codex thread id, and mcbench stopped the turn";

/// Whether `id` is a thread id as Codex writes one: a UUID in its
/// lowercase hyphenated form (8-4-4-4-12 hex digits).
pub fn is_thread_id(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == b'-',
            _ => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        })
}

/// The items a turn may report; any other is a tool mcbench did not give.
pub const ALLOWED_ITEMS: [&str; 5] = [
    "agent_message",
    "reasoning",
    "todo_list",
    "error",
    "mcp_tool_call",
];

/// The bridge inside the sandbox: the static `mcbench-sandbox-agent`
/// bound at `/run/mcbench-relay` (`mcbench_sandbox::proxy::CodexRun::relay`)
/// with the `mcbench-relay` crate's relay, the socket at
/// `/run/mcbench/mcp.sock` (today `python3 /run/mcbench/mcp-bridge.py
/// /run/mcbench/mcp.sock`). The app's own binary is the bridge everywhere
/// else; it does not run on the box's root filesystem.
pub const SANDBOX_BRIDGE: (&str, [&str; 2]) = (
    "/run/mcbench-relay",
    ["tool-bridge", "/run/mcbench/mcp.sock"],
);

/// What Codex's box is made of besides the run's own folders: the root
/// filesystem (`mcbench_sandbox::linux::rootfs::unpack`'s answer), its
/// environment (`image.json`'s `env`) and the static agent that relays the
/// network and the tool bridge inside it.
#[derive(Clone, Debug)]
pub struct SandboxFiles {
    /// The unpacked root filesystem.
    pub rootfs: PathBuf,
    /// Its environment, `KEY=value`.
    pub image_env: Vec<String>,
    /// The static `mcbench-sandbox-agent`.
    pub relay: PathBuf,
}

/// Where a sandboxed launch finds its box's files, asked at each launch:
/// in the app the root filesystem is unpacked by setup's shell step, which
/// may end after the provider is made, so a turn before then is refused
/// with the source's sentence.
pub type SandboxSource = Arc<dyn Fn() -> Result<SandboxFiles, String> + Send + Sync>;

/// A test's stand-in launcher.
pub type Launcher =
    Arc<dyn Fn(&LaunchSpec, Arc<dyn BridgeHost>) -> Result<Launched, String> + Send + Sync>;

/// How a turn's Codex is started.
#[derive(Clone)]
pub enum Launch {
    /// On the host, in Codex's read-only sandbox (macOS today).
    Host,
    /// Under bubblewrap with the allow-list proxy (Linux), as
    /// `mcbench-sandbox` builds it (`proxy::codex_command`), in the box
    /// its source gives.
    Sandboxed(SandboxSource),
    /// In the same box inside the app's WSL distro (Windows), run by the
    /// guest base's agent (`mcbench_sandbox::wsl_codex`).
    Wsl(WslBox),
    /// A test's stand-in.
    #[doc(hidden)]
    Custom(Launcher),
}

impl Launch {
    /// Sandboxed in the box `files` make, the same at every launch.
    pub fn sandboxed(files: SandboxFiles) -> Launch {
        Launch::Sandboxed(Arc::new(move || Ok(files.clone())))
    }

    /// This platform's: sandboxed on Linux (in the box `files` gives), on
    /// the host on macOS, in the WSL distro's box on Windows (`wsl`).
    pub fn for_this_platform(files: SandboxSource, wsl: impl FnOnce() -> WslBox) -> Option<Launch> {
        if cfg!(target_os = "linux") {
            Some(Launch::Sandboxed(files))
        } else if cfg!(target_os = "macos") {
            Some(Launch::Host)
        } else if cfg!(windows) {
            Some(Launch::Wsl(wsl()))
        } else {
            None
        }
    }
}

/// Codex's box in the app's WSL distro: how the agent there is reached, and
/// where the Linux box's files are inside the distro.
#[derive(Clone)]
pub struct WslBox {
    /// `wsl.exe`.
    pub program: OsString,
    /// Its arguments up to the agent's verb: `--distribution <distro>
    /// --user root --cd / --exec /usr/bin/mcbench-sandbox-agent codex
    /// <login> <tools> <harness> <uid>:<gid>` ([`CodexConfig::prefix`] is
    /// these and `cli <program>`).
    pub agent: Vec<String>,
    /// The data folder's `harness` folder, mounted at
    /// `mcbench_sandbox::wsl_codex::HARNESS`.
    pub harness: PathBuf,
    /// The Codex install's folder in the distro (under
    /// `mcbench_sandbox::wsl_codex::TOOLS`) and its program under
    /// `/opt/codex`.
    pub install: (String, String),
    /// The box's user in the distro.
    pub ids: (u32, u32),
    /// The unpacked root filesystem in the distro, asked at each launch: a
    /// turn before WSL and the shell are ready is refused with its sentence.
    pub rootfs: Arc<dyn Fn() -> Result<String, String> + Send + Sync>,
}

/// What a launcher is given (`LAUNCHERS`' options).
#[derive(Debug)]
pub struct LaunchSpec {
    /// The chat's folder (or a one-shot's temporary one).
    pub dir: PathBuf,
    /// The run's runtime folder.
    pub runtime_dir: PathBuf,
    /// The bridge's socket.
    pub socket: PathBuf,
    /// The model.
    pub model: String,
    /// Its effort.
    pub effort: Option<String>,
    /// The system prompt.
    pub system_prompt: String,
    /// Whether the thread is resumed.
    pub resume: bool,
    /// The thread, when resumed.
    pub thread_id: Option<String>,
    /// The prompt's images.
    pub images: Vec<PromptImage>,
}

/// What a launcher returns: the command, and what to release afterwards.
pub struct Launched {
    /// The program.
    pub command: OsString,
    /// Its arguments.
    pub args: Vec<String>,
    /// Its working folder.
    pub cwd: PathBuf,
    /// Its whole environment.
    pub env: Env,
    /// Run when the turn is stopped.
    pub on_stop: Box<dyn FnOnce() + Send>,
    /// Run once the process has exited.
    pub release: Box<dyn FnOnce() -> Result<(), String> + Send>,
}

/// CODEX_HOME when Settings names none: `CODEX_HOME` from this process,
/// else `~/.codex` (on Windows `%USERPROFILE%\.codex`).
pub fn default_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| super::home_dir().join(".codex"))
}

/// Where and how Codex runs.
#[derive(Clone)]
pub struct CodexConfig {
    /// The data folder.
    pub data_dir: PathBuf,
    /// The Codex command, run by the name or path the environment gives
    /// (on Windows `wsl.exe`, with [`CodexConfig::prefix`]).
    pub binary: OsString,
    /// Arguments before Codex's own: none, except on Windows, where Codex
    /// runs in the WSL distro as the agent's `codex ... cli <program>`, with
    /// CODEX_HOME the person's login folder there.
    pub prefix: Vec<String>,
    /// CODEX_HOME when Settings names one; else `CODEX_HOME` from this
    /// process, else `~/.codex`.
    pub home: Option<PathBuf>,
    /// How the CLI reaches the bridge on the host.
    pub bridge: BridgeCommand,
    /// How a turn's Codex starts; `None` refuses turns on this platform.
    pub launch: Option<Launch>,
}

impl CodexConfig {
    /// `codexHome`.
    pub fn codex_home(&self) -> PathBuf {
        self.home.clone().unwrap_or_else(default_home)
    }

    /// The login file.
    pub fn auth_file(&self) -> PathBuf {
        self.codex_home().join("auth.json")
    }

    /// `storedLogin`.
    pub fn stored_login(&self) -> bool {
        self.auth_file().exists()
    }

    /// `hostEnv`. Through WSL ([`CodexConfig::prefix`]) the agent sets
    /// Codex's environment in the distro, and `wsl.exe` gets what a Windows
    /// program needs and `WSL_UTF8=1` (its own words in UTF-8).
    pub fn host_env(&self, home: Option<&Path>) -> Env {
        if !self.prefix.is_empty() {
            return super::cli_env(&[("WSL_UTF8", OsString::from("1"))]);
        }
        let home = home
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.codex_home());
        super::cli_env(&[("CODEX_HOME", home.into_os_string())])
    }

    /// [`CodexConfig::prefix`] and `args`.
    fn args<'a>(&'a self, args: &'a [&'a str]) -> Vec<String> {
        self.prefix
            .iter()
            .map(String::as_str)
            .chain(args.iter().copied())
            .map(str::to_owned)
            .collect()
    }

    /// `loginCommand`: `codex login --device-auth` and `codex logout`,
    /// whose `BROWSER` opens nothing (in the WSL distro the agent sets it).
    pub fn login_command(&self, args: &[&str]) -> CommandSpec {
        let mut env = self.host_env(None);
        if self.prefix.is_empty() {
            match env.iter_mut().find(|(k, _)| k == "BROWSER") {
                Some(slot) => slot.1 = OsString::from("true"),
                None => env.push((OsString::from("BROWSER"), OsString::from("true"))),
            }
        }
        CommandSpec {
            file: self.binary.clone(),
            args: self.args(args),
            env,
        }
    }

    /// `account`: the ChatGPT login's email and plan (`pro` reads
    /// `ChatGPT Pro`); only those two claims leave this function.
    pub fn account(&self) -> Result<Option<Value>, String> {
        if !self.stored_login() {
            return Ok(None);
        }
        let file = self.auth_file();
        let text =
            std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
        let auth = crate::api::v8json::parse(&text)?;
        let token = auth
            .get("tokens")
            .and_then(|t| t.get("id_token"))
            .filter(|t| truthy(Some(t)));
        let claims = match token {
            Some(token) => {
                let token = js_string(token);
                let payload = token.split('.').nth(1).unwrap_or("");
                let bytes = base64url(payload)?;
                crate::api::v8json::parse(&String::from_utf8_lossy(&bytes))?
            }
            None => json!({}),
        };
        let plan = claims
            .get("https://api.openai.com/auth")
            .and_then(|a| a.get("chatgpt_plan_type"))
            .filter(|p| truthy(Some(p)))
            .map(|p| {
                let p = js_string(p);
                let mut chars = p.chars();
                let first = chars.next().unwrap();
                format!("ChatGPT {}{}", first.to_uppercase(), chars.as_str())
            });
        let email = claims
            .get("email")
            .filter(|e| truthy(Some(e)))
            .cloned()
            .unwrap_or(Value::Null);
        Ok(Some(json!({ "email": email, "plan": plan })))
    }
}

/// Base64url without padding, as `Buffer.from(text, 'base64url')` reads it
/// (characters outside the alphabet are skipped).
fn base64url(text: &str) -> Result<Vec<u8>, String> {
    let mut bits = 0u32;
    let mut count = 0;
    let mut out = Vec::new();
    for c in text.chars() {
        let value = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '-' | '+' => 62,
            '_' | '/' => 63,
            '=' => break,
            _ => continue,
        };
        bits = (bits << 6) | value;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    Ok(out)
}

/// `toml`: a value as TOML reads a JSON string or number.
fn toml(value: &str) -> String {
    mcbench_json::stringify(&json!(value))
}

/// `configEntries`: the run's configuration as dotted TOML keys. `sandbox`
/// is the policy for Codex's own command sandbox.
pub fn config_entries(
    model: &str,
    effort: Option<&str>,
    system_prompt: &str,
    sandbox: &str,
    bridge: (&str, &[String]),
) -> Vec<(String, String)> {
    let mut entries = vec![("model".to_owned(), toml(model))];
    if let Some(effort) = effort.filter(|e| !e.is_empty()) {
        entries.push(("model_reasoning_effort".to_owned(), toml(effort)));
    }
    for (key, value) in [
        ("approval_policy", toml("never")),
        ("sandbox_mode", toml(sandbox)),
        ("web_search", toml("disabled")),
        ("check_for_update_on_startup", "false".to_owned()),
        ("tool_output_token_limit", "200000".to_owned()),
        ("developer_instructions", toml(system_prompt)),
        ("project_doc_max_bytes", "0".to_owned()),
    ] {
        entries.push((key.to_owned(), value));
    }
    for feature in DISABLED_FEATURES {
        entries.push((format!("features.{feature}"), "false".to_owned()));
    }
    for feature in ENABLED_FEATURES {
        entries.push((format!("features.{feature}"), "true".to_owned()));
    }
    entries.push(("agents.enabled".to_owned(), "false".to_owned()));
    entries.push(("mcp_servers.mcbench.command".to_owned(), toml(bridge.0)));
    entries.push((
        "mcp_servers.mcbench.args".to_owned(),
        format!(
            "[{}]",
            bridge
                .1
                .iter()
                .map(|a| toml(a))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ));
    entries.push((
        "mcp_servers.mcbench.startup_timeout_sec".to_owned(),
        "120".to_owned(),
    ));
    entries.push((
        "mcp_servers.mcbench.tool_timeout_sec".to_owned(),
        "2000000".to_owned(),
    ));
    entries.push((
        "mcp_servers.mcbench.default_tools_approval_mode".to_owned(),
        toml("approve"),
    ));
    entries
}

/// `configToml`: the sandboxed run's `config.toml`.
pub fn config_toml(model: &str, effort: Option<&str>, system_prompt: &str) -> String {
    let args: Vec<String> = SANDBOX_BRIDGE.1.iter().map(|s| (*s).to_owned()).collect();
    config_entries(
        model,
        effort,
        system_prompt,
        "danger-full-access",
        (SANDBOX_BRIDGE.0, &args),
    )
    .iter()
    .map(|(k, v)| format!("{k} = {v}"))
    .collect::<Vec<_>>()
    .join("\n")
        + "\n"
}

/// `hostArgs`: the host run, with the login's CODEX_HOME, no user config
/// or rules, and the settings as overrides.
#[allow(clippy::too_many_arguments)]
pub fn host_args(
    model: &str,
    effort: Option<&str>,
    system_prompt: &str,
    bridge: (&str, &[String]),
    resume: bool,
    thread_id: Option<&str>,
    images: &[String],
) -> Vec<String> {
    let mut args = vec!["exec".to_owned()];
    if resume {
        args.push("resume".to_owned());
    }
    for arg in [
        "--json",
        "--skip-git-repo-check",
        "--ignore-user-config",
        "--ignore-rules",
    ] {
        args.push(arg.to_owned());
    }
    for (key, value) in config_entries(model, effort, system_prompt, "read-only", bridge) {
        args.push("-c".to_owned());
        args.push(format!("{key}={value}"));
    }
    for image in images {
        args.push("-i".to_owned());
        args.push(image.clone());
    }
    args.extend(resumed(resume, thread_id));
    args.push("-".to_owned());
    args
}

/// A resumed thread's id on `codex exec resume`'s command line: after
/// `--`, so Codex reads it as the session id whatever it holds (Codex
/// 0.169.0 resumes, or fails, as with the id before its options).
fn resumed(resume: bool, thread_id: Option<&str>) -> Vec<String> {
    if resume {
        vec!["--".to_owned(), thread_id.unwrap_or("undefined").to_owned()]
    } else {
        Vec::new()
    }
}

/// `checkItem`: an item outside [`ALLOWED_ITEMS`], or an MCP call to
/// another server, ends the run.
pub fn check_item(item: &Value) -> Result<(), String> {
    let kind = item.get("type");
    let allowed = kind
        .and_then(Value::as_str)
        .is_some_and(|k| ALLOWED_ITEMS.contains(&k));
    let foreign_mcp =
        kind == Some(&json!("mcp_tool_call")) && item.get("server") != Some(&json!("mcbench"));
    if !allowed || foreign_mcp {
        let server = item.get("server").filter(|s| truthy(Some(s)));
        return Err(format!(
            "{}{}{}",
            text::CODEX_OUTSIDE,
            kind.map(js_string)
                .unwrap_or_else(|| "undefined".to_owned()),
            server
                .map(|s| format!(" {}", js_string(s)))
                .unwrap_or_default()
        ));
    }
    Ok(())
}

/// `usageOf`: Codex's turn usage as the OpenAI accounting the app reads.
pub fn usage_of(usage: Option<&Value>) -> Result<Value, String> {
    let usage = match usage {
        None => {
            return Err("Cannot read properties of undefined (reading 'input_tokens')".to_owned());
        }
        Some(Value::Null) => {
            return Err("Cannot read properties of null (reading 'input_tokens')".to_owned());
        }
        Some(usage) => usage,
    };
    let mut out = Map::new();
    let put = |out: &mut Map<String, Value>, key: &str, value: Option<&Value>| {
        if let Some(value) = value {
            out.insert(key.to_owned(), value.clone());
        }
    };
    put(&mut out, "input_tokens", usage.get("input_tokens"));
    put(&mut out, "output_tokens", usage.get("output_tokens"));
    let mut cached = Map::new();
    put(
        &mut cached,
        "cached_tokens",
        usage.get("cached_input_tokens"),
    );
    out.insert("input_tokens_details".to_owned(), Value::Object(cached));
    let mut reasoning = Map::new();
    put(
        &mut reasoning,
        "reasoning_tokens",
        usage.get("reasoning_output_tokens"),
    );
    out.insert("output_tokens_details".to_owned(), Value::Object(reasoning));
    Ok(Value::Object(out))
}

/// `fs.readdirSync(dir, { recursive: true })`: relative names, a folder's
/// entries before what is inside them.
fn read_dir_recursive(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut queue = vec![PathBuf::new()];
    while let Some(relative) = queue.pop() {
        let folder = if relative.as_os_str().is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(&relative)
        };
        let entries = std::fs::read_dir(&folder).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                format!(
                    "ENOENT: no such file or directory, scandir '{}'",
                    folder.display()
                )
            } else {
                format!("scandir '{}': {e}", folder.display())
            }
        })?;
        let mut folders = Vec::new();
        let mut names: Vec<_> = entries.filter_map(Result::ok).collect();
        names.sort_by_key(|e| e.file_name());
        for entry in names {
            let name = relative.join(entry.file_name());
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                folders.push(name.clone());
            }
            out.push(name);
        }
        for folder in folders.into_iter().rev() {
            queue.push(folder);
        }
    }
    Ok(out)
}

/// `contextTokensOf`: the whole input of the turn's last model request,
/// from the thread's rollout file under `<home>/sessions`.
pub fn context_tokens_of(home: &Path, thread_id: &str) -> Result<Value, String> {
    let sessions = home.join("sessions");
    let suffix = format!("-{thread_id}.jsonl");
    let file = read_dir_recursive(&sessions)?.into_iter().find(|name| {
        name.file_name()
            .is_some_and(|b| b.to_string_lossy().starts_with("rollout-"))
            && name.to_string_lossy().ends_with(&suffix)
    });
    let Some(file) = file else {
        return Err(format!(
            "No Codex rollout file for thread {thread_id} under {}",
            sessions.display()
        ));
    };
    let path = sessions.join(&file);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut last = None;
    for line in text.split('\n').filter(|l| !l.is_empty()) {
        let event = crate::api::v8json::parse(line)?;
        let payload = event.get("payload");
        if event.get("type") == Some(&json!("event_msg"))
            && payload.and_then(|p| p.get("type")) == Some(&json!("token_count"))
            && truthy(payload.and_then(|p| p.get("info")))
        {
            last = Some(event);
        }
    }
    let Some(last) = last else {
        return Err(format!(
            "No token_count event with usage in {}",
            path.display()
        ));
    };
    Ok(last["payload"]["info"]["last_token_usage"]
        .get("input_tokens")
        .cloned()
        .unwrap_or(Value::Null))
}

fn image_extension(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "png",
    }
}

/// Base64 as `Buffer.from(data, 'base64')` reads it.
fn base64(text: &str) -> Result<Vec<u8>, String> {
    base64url(text)
}

/// `writeImages`: `image-<n>.<ext>` in `dir`.
fn write_images(dir: &Path, images: &[PromptImage]) -> Result<Vec<PathBuf>, String> {
    images
        .iter()
        .enumerate()
        .map(|(i, image)| {
            let file = dir.join(format!(
                "image-{}.{}",
                i + 1,
                image_extension(&image.mime_type)
            ));
            std::fs::write(&file, base64(&image.data)?)
                .map_err(|e| format!("write {}: {e}", file.display()))?;
            Ok(file)
        })
        .collect()
}

/// `linkLogin`: a chat's own CODEX_HOME holding the login, linked so token
/// refreshes reach the real one.
#[cfg_attr(not(unix), allow(unused_variables))]
fn link_login(home: &Path, auth_file: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home).map_err(|e| format!("mkdir {}: {e}", home.display()))?;
    let auth = home.join("auth.json");
    if std::fs::symlink_metadata(&auth).is_err() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(auth_file, &auth)
            .map_err(|e| format!("symlink {}: {e}", auth.display()))?;
        #[cfg(not(unix))]
        return Err(format!(
            "Codex's login cannot be linked into {} on this platform",
            auth.display()
        ));
    }
    Ok(())
}

/// `prepareHome`: a CODEX_HOME holding only what the run writes, plus the
/// mount target for the login.
pub fn prepare_home(home: &Path) -> Result<(), String> {
    std::fs::create_dir_all(home).map_err(|e| format!("mkdir {}: {e}", home.display()))?;
    let auth = home.join("auth.json");
    if !auth.exists() {
        std::fs::write(&auth, "").map_err(|e| format!("write {}: {e}", auth.display()))?;
    }
    Ok(())
}

/// The Codex install the command runs (`codexInstall`): `folder` on the
/// host and `program` as the sandbox names it under `/opt/codex`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexInstall {
    /// The install's folder.
    pub folder: PathBuf,
    /// Its executable, under `/opt/codex`.
    pub program: String,
}

/// `mcbench-sandbox`'s Codex box: starts the allow-list proxy on
/// `<sock_dir>/egress.sock` and gives the `systemd-run` and bubblewrap
/// command line (Linux only).
fn sandbox_command(
    run: &mcbench_sandbox::proxy::CodexRun,
) -> Result<mcbench_sandbox::proxy::CodexCommand, String> {
    #[cfg(unix)]
    {
        mcbench_sandbox::proxy::codex_command(run)
    }
    #[cfg(not(unix))]
    {
        let _ = run;
        Err("Codex's box needs Linux".to_owned())
    }
}

/// `codexInstall`: the install Codex itself names. Every Codex start links
/// apply_patch, in a folder under CODEX_HOME/tmp, to its own executable; a
/// `--version` run with a fresh CODEX_HOME (in the data folder: on Linux
/// Codex makes no link under the temporary folder) leaves exactly one such
/// link, and the install is the folder above it whose codex-package.json
/// names it as its entrypoint.
pub async fn codex_install(config: &CodexConfig) -> Result<CodexInstall, String> {
    std::fs::create_dir_all(&config.data_dir)
        .map_err(|e| format!("mkdir {}: {e}", config.data_dir.display()))?;
    let home = super::mkdtemp(&config.data_dir.join("codex-install-"))?;
    let found = find_install(config, &home).await;
    let _ = std::fs::remove_dir_all(&home);
    found
}

async fn find_install(config: &CodexConfig, home: &Path) -> Result<CodexInstall, String> {
    let binary = config.binary.to_string_lossy().into_owned();
    let (stdout, stderr) = run_capture(
        config,
        &["--version"],
        Some(home),
        Duration::from_millis(60_000),
    )
    .await?;
    let ran = format!("`{binary} --version` ({})", stdout.trim());
    let links: Vec<PathBuf> = read_dir_recursive(home)?
        .into_iter()
        .filter(|name| name.file_name().is_some_and(|b| b == "apply_patch"))
        .collect();
    if links.len() != 1 {
        let said = if stderr.trim().is_empty() {
            String::new()
        } else {
            format!("; it said: {}", stderr.trim())
        };
        return Err(format!(
            "{ran} left {} apply_patch links in its CODEX_HOME {}, not one naming the Codex executable it started{said}",
            links.len(),
            home.display()
        ));
    }
    let executable = std::fs::canonicalize(home.join(&links[0]))
        .map_err(|e| format!("realpath {}: {e}", links[0].display()))?;
    let mut folder = executable.parent().map(Path::to_path_buf);
    while let Some(dir) = folder {
        let Some(parent) = dir.parent() else { break };
        let manifest = dir.join("codex-package.json");
        if manifest.exists() {
            let text = std::fs::read_to_string(&manifest)
                .map_err(|e| format!("{}: {e}", manifest.display()))?;
            let entrypoint = crate::api::v8json::parse(&text)?
                .get("entrypoint")
                .map(js_string)
                .unwrap_or_default();
            if normalize(&dir.join(&entrypoint)) == executable {
                return Ok(CodexInstall {
                    folder: dir.clone(),
                    program: format!("/opt/codex/{}", entrypoint.trim_start_matches("./")),
                });
            }
        }
        folder = Some(parent.to_path_buf());
    }
    Err(format!(
        "{ran} started {}, which no codex-package.json in a folder above it names as its entrypoint, so there is no Codex install to mount in the container",
        executable.display()
    ))
}

/// `path.join`'s normalisation of `.` and `..`.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// `execFile(binary, args, { env: hostEnv(home) })`: stdout and stderr, or
/// `Command failed: <binary> <args>` and what it said.
async fn run_capture(
    config: &CodexConfig,
    args: &[&str],
    home: Option<&Path>,
    timeout: Duration,
) -> Result<(String, String), String> {
    let line = format!("{} {}", config.binary.to_string_lossy(), args.join(" "));
    let mut command = tokio::process::Command::from(mcbench_spawn::command(&config.binary));
    command
        .args(config.args(args))
        .env_clear()
        .envs(config.host_env(home))
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(timeout, command.output())
        .await
        .map_err(|_| format!("Command failed: {line} (timed out)"))?
        .map_err(|e| match e.kind() {
            // execFile's spawn failure: `spawn <file> ENOENT`.
            std::io::ErrorKind::NotFound => {
                format!("spawn {} ENOENT", config.binary.to_string_lossy())
            }
            std::io::ErrorKind::PermissionDenied => {
                format!("spawn {} EACCES", config.binary.to_string_lossy())
            }
            _ => format!("Command failed: {line}\n{e}"),
        })?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        return Err(format!("Command failed: {line}\n{stderr}"));
    }
    Ok((stdout, stderr))
}

struct Session {
    relay: Relay,
    run: Option<JsonLinesRun>,
    closed: CancellationToken,
}

#[derive(Default)]
struct Core {
    by_chat: HashMap<String, u64>,
    sessions: HashMap<u64, Session>,
    next: u64,
}

struct Inner {
    config: CodexConfig,
    core: Mutex<Core>,
    models: Arc<CodexModels>,
}

/// The Codex CLI provider.
pub struct CodexCli {
    inner: Arc<Inner>,
}

impl CodexCli {
    /// The provider; `models` is the catalog's Codex catalogue
    /// (`catalog::Catalog::codex`), so one cache serves it and the ChatGPT
    /// sign-in, as in Node.
    pub fn new(config: CodexConfig, models: Arc<CodexModels>) -> Self {
        CodexCli {
            inner: Arc::new(Inner {
                config,
                core: Mutex::new(Core::default()),
                models,
            }),
        }
    }

    /// The runtime the catalogue runs `codex debug models` with.
    pub fn runtime(&self) -> CliRuntime {
        let config = &self.inner.config;
        CliRuntime {
            program: config.binary.to_string_lossy().into_owned(),
            args: config.prefix.clone(),
            home: config.codex_home(),
            env: config
                .host_env(None)
                .into_iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect(),
        }
    }

    /// The configuration it runs with.
    pub fn config(&self) -> &CodexConfig {
        &self.inner.config
    }

    /// `codex --version`.
    pub async fn version(&self) -> Result<String, String> {
        let (stdout, _) = run_capture(
            &self.inner.config,
            &["--version"],
            None,
            Duration::from_millis(5000),
        )
        .await?;
        Ok(stdout.trim().to_owned())
    }

    /// One completion of a turn: delivers the stored result of the chat's
    /// pending call, or ends any previous run and starts one. The message is
    /// today's module's (before `chatCompletion`'s telemetry).
    pub async fn complete_turn(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<Map<String, Value>, ProviderError> {
        let request = HarnessRequest::from_completion(request).map_err(completion::failure)?;
        self.run_turn(&request, on_delta, cancel).await
    }

    async fn run_turn(
        &self,
        request: &HarnessRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<Map<String, Value>, ProviderError> {
        let (waiter, deltas, done) = Waiter::new(true);
        let fresh = {
            let mut core = self.inner.core.lock().unwrap();
            let key = request.conversation_id().map(str::to_owned);
            let existing = key.as_ref().and_then(|k| core.by_chat.get(k).copied());
            let pending = existing
                .and_then(|sid| core.sessions.get(&sid))
                .and_then(|s| s.relay.pending_id().map(str::to_owned));
            let result = match pending {
                Some(id) => {
                    super::pending_result(&request.values(), &id).map_err(completion::failure)?
                }
                None => None,
            };
            if let (Some(result), Some(sid)) = (result, existing) {
                let delivered = core.sessions.get_mut(&sid).unwrap().relay.deliver(result);
                settle(&mut core, sid);
                delivered.map_err(completion::failure)?;
                core.sessions.get_mut(&sid).unwrap().relay.next(waiter);
                settle(&mut core, sid);
                None
            } else {
                if let Some(sid) = existing {
                    if let Some(session) = core.sessions.get_mut(&sid) {
                        session.relay.close();
                    }
                    settle(&mut core, sid);
                }
                let relay = Relay::new("Codex", cancel.is_cancelled().then_some(text::ABORTED))
                    .map_err(completion::failure)?;
                core.next += 1;
                let sid = core.next;
                let closed = CancellationToken::new();
                core.sessions.insert(
                    sid,
                    Session {
                        relay,
                        run: None,
                        closed: closed.clone(),
                    },
                );
                if let Some(key) = &key {
                    core.by_chat.insert(key.clone(), sid);
                }
                let weak = Arc::downgrade(&self.inner);
                completion::watch_abort(cancel, closed, move || {
                    if let Some(inner) = weak.upgrade() {
                        let mut core = inner.core.lock().unwrap();
                        if let Some(session) = core.sessions.get_mut(&sid) {
                            session.relay.abort(text::ABORTED);
                        }
                        settle(&mut core, sid);
                    }
                });
                // The completion waits from the run's start: what the CLI
                // prints as soon as it runs (a warm process answers the
                // request written to it at once) streams to it, as Node's
                // `next` is set before any of the CLI's output is read.
                core.sessions.get_mut(&sid).unwrap().relay.next(waiter);
                Some(sid)
            }
        };
        if let Some(sid) = fresh {
            let started = start(&self.inner, sid, request).await;
            let mut core = self.inner.core.lock().unwrap();
            if let Err(error) = started {
                if let Some(session) = core.sessions.get_mut(&sid) {
                    session.relay.close();
                }
                settle(&mut core, sid);
                return Err(completion::failure(error));
            }
            settle(&mut core, sid);
        }
        completion::read_answer(deltas, done, on_delta)
            .await
            .map_err(completion::failure)
    }

    /// `closeSession`: ends the chat's run, if any.
    pub fn close_chat(&self, conversation_id: &str) {
        let mut core = self.inner.core.lock().unwrap();
        if let Some(sid) = core.by_chat.get(conversation_id).copied() {
            if let Some(session) = core.sessions.get_mut(&sid) {
                session.relay.close();
            }
            settle(&mut core, sid);
        }
    }

    /// How many runs are open, for tests.
    #[doc(hidden)]
    pub fn live_sessions(&self) -> usize {
        self.inner.core.lock().unwrap().sessions.len()
    }

    /// The chat's harness folder goes (`harness.destroy`; Codex keeps
    /// nothing outside it and no process outlives a turn).
    pub fn destroy(&self, conversation_id: &str) {
        let _ = std::fs::remove_dir_all(super::conversation_dir(
            &self.inner.config.data_dir,
            conversation_id,
        ));
    }
}

#[async_trait]
impl Provider for CodexCli {
    fn id(&self) -> ProviderId {
        ProviderId::CodexCli
    }

    async fn complete(
        &self,
        request: &CompletionRequest,
        on_delta: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        let tracked = completion::Tracked::start(ProviderId::CodexCli, request);
        let reply = {
            let mut noted = |delta: Delta| {
                tracked.note(&delta);
                on_delta(delta);
            };
            self.complete_turn(request, &mut noted, cancel).await
        };
        tracked.finish(reply, cancel)
    }

    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        self.inner.models.list(&self.runtime()).await
    }

    async fn model_info(&self, model: &str) -> Result<ModelFacts, ProviderError> {
        Ok(self
            .models()
            .await?
            .into_iter()
            .find(|m| m.id == model)
            .map(|m| m.facts)
            .unwrap_or_else(ModelFacts::unknown))
    }

    async fn close_session(&self, conversation_id: &str) {
        self.close_chat(conversation_id);
    }
}

impl Drop for CodexCli {
    fn drop(&mut self) {
        let core = self.inner.core.lock().unwrap();
        for session in core.sessions.values() {
            if let Some(run) = &session.run {
                run.stop();
            }
        }
    }
}

/// After a relay operation: a closed run stops its process when it was
/// running (`CodexSession.stop`), and is forgotten.
fn settle(core: &mut Core, sid: u64) {
    let Some(session) = core.sessions.get_mut(&sid) else {
        return;
    };
    let Some(was_running) = session.relay.take_close() else {
        return;
    };
    session.closed.cancel();
    if was_running && let Some(run) = &session.run {
        run.stop();
    }
    core.sessions.remove(&sid);
    core.by_chat.retain(|_, s| *s != sid);
}

struct Host {
    inner: Weak<Inner>,
    sid: u64,
    prefix: String,
    tools: Vec<ToolSchema>,
}

struct CallGuard {
    inner: Weak<Inner>,
    sid: u64,
    id: String,
    serial: u64,
    answered: bool,
}

impl Drop for CallGuard {
    fn drop(&mut self) {
        if self.answered {
            return;
        }
        if let Some(inner) = self.inner.upgrade() {
            let mut core = inner.core.lock().unwrap();
            if let Some(session) = core.sessions.get_mut(&self.sid) {
                session.relay.cancel_call(&self.id, self.serial);
            }
        }
    }
}

#[async_trait]
impl BridgeHost for Host {
    fn tools(&self) -> Vec<ToolSchema> {
        self.tools.clone()
    }

    async fn call(
        &self,
        name: &str,
        arguments: Value,
        call_id: &str,
        _tool_use_id: Option<&str>,
        _cancel: &CancellationToken,
    ) -> Result<Vec<McpContent>, String> {
        let id = format!("{}:{call_id}", self.prefix);
        let inner = self
            .inner
            .upgrade()
            .ok_or_else(|| text::TURN_ENDED.to_owned())?;
        let (answer, serial) = {
            let mut core = inner.core.lock().unwrap();
            let session = core
                .sessions
                .get_mut(&self.sid)
                .ok_or_else(|| text::TURN_ENDED.to_owned())?;
            session.relay.call(&id, name, arguments)
        };
        let mut guard = CallGuard {
            inner: self.inner.clone(),
            sid: self.sid,
            id,
            serial,
            answered: false,
        };
        let answer = answer
            .await
            .unwrap_or_else(|_| Err(text::TURN_ENDED.to_owned()));
        guard.answered = true;
        answer.map(|blocks| blocks.into_iter().map(McpContent).collect())
    }
}

/// The host launcher (`LAUNCHERS.darwin`).
fn launch_host(
    config: &CodexConfig,
    spec: &LaunchSpec,
    host: Arc<Host>,
) -> Result<Launched, String> {
    let home = spec.dir.join("codex-home");
    let workspace = spec.dir.join("workspace");
    link_login(&home, &config.auth_file())?;
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("mkdir {}: {e}", workspace.display()))?;
    let images: Vec<String> = write_images(&spec.runtime_dir, &spec.images)?
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let address = super::bridge_address(&spec.socket);
    let server = bridge::serve(&address, host)?;
    let (command, args) = config.bridge.for_address(&address.to_arg());
    Ok(Launched {
        command: config.binary.clone(),
        args: host_args(
            &spec.model,
            spec.effort.as_deref(),
            &spec.system_prompt,
            (&command, &args),
            spec.resume,
            spec.thread_id.as_deref(),
            &images,
        ),
        cwd: workspace,
        env: config.host_env(Some(&home)),
        on_stop: Box::new(|| {}),
        release: Box::new(move || {
            server.stop();
            Ok(())
        }),
    })
}

/// The command inside the box: `codex exec` (resumed when the thread is),
/// its JSON events, the prompt's images, the prompt on stdin.
pub fn box_command(program: &str, spec: &LaunchSpec, images: Vec<String>) -> Vec<String> {
    let mut command = vec![program.to_owned(), "exec".to_owned()];
    if spec.resume {
        command.push("resume".to_owned());
    }
    command.push("--json".to_owned());
    command.push("--skip-git-repo-check".to_owned());
    for image in images {
        command.push("-i".to_owned());
        command.push(image);
    }
    command.extend(resumed(spec.resume, spec.thread_id.as_deref()));
    command.push("-".to_owned());
    command
}

/// A chat's folder (under the data folder's `harness`) as the WSL distro
/// sees it, under `mcbench_sandbox::wsl_codex::HARNESS`, with `/` between
/// the parts.
fn in_distro(wsl: &WslBox, path: &Path) -> Result<PathBuf, String> {
    let inside = path.strip_prefix(&wsl.harness).map_err(|_| {
        format!(
            "{} is not in the harness folder {}",
            path.display(),
            wsl.harness.display()
        )
    })?;
    let mut out = mcbench_sandbox::wsl_codex::HARNESS.to_owned();
    for part in inside.components() {
        out.push('/');
        out.push_str(
            part.as_os_str()
                .to_str()
                .ok_or_else(|| format!("{} is not UTF-8", path.display()))?,
        );
    }
    Ok(PathBuf::from(out))
}

/// `wsl.exe`'s arguments for the box of `run`: the agent's `box` verb,
/// then the box Linux runs ([`mcbench_sandbox::proxy::codex_box_args`])
/// for the box's user in the distro.
pub fn wsl_box_args(
    wsl: &WslBox,
    run: &str,
    images: &Path,
    run_spec: &mcbench_sandbox::proxy::CodexRun,
) -> Vec<String> {
    let mut args = wsl.agent.clone();
    args.extend([
        "box".to_owned(),
        run.to_owned(),
        images.to_string_lossy().into_owned(),
        run_spec.proxy_log.to_string_lossy().into_owned(),
        run_spec.allowed_hosts.join(","),
        "--".to_owned(),
    ]);
    args.extend(mcbench_sandbox::proxy::codex_box_args(run_spec, wsl.ids));
    args
}

/// The run's tool bridge in the distro (`... bridge <run>`, listening on
/// the box's `mcp.sock`), its stdin and stdout joined to a connection to
/// the app's named pipe `address`; the answer stops it.
async fn wsl_bridge(
    wsl: &WslBox,
    run: &str,
    address: &crate::bridge::BridgeAddress,
) -> Result<CancellationToken, String> {
    use tokio::io::AsyncBufReadExt;
    let mut command = tokio::process::Command::from(mcbench_spawn::command(&wsl.program));
    command
        .args(&wsl.agent)
        .args(["bridge", run])
        .env_clear()
        .envs(super::cli_env(&[("WSL_UTF8", OsString::from("1"))]))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let program = wsl.program.to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .map_err(|e| format!("spawn {program}: {e}"))?;
    let tree = super::tree::Tree::holding(&child).map_err(|e| format!("spawn {program}: {e}"))?;
    let mut lines = tokio::io::BufReader::new(child.stderr.take().expect("piped")).lines();
    let mut said = String::new();
    let ready = tokio::time::timeout(Duration::from_secs(60), async {
        while let Ok(Some(line)) = lines.next_line().await {
            if line == "ready" {
                return true;
            }
            said = line;
        }
        false
    })
    .await;
    if ready != Ok(true) {
        return Err(format!(
            "Codex's tool bridge in the WSL distro did not start: {said}"
        ));
    }
    let pipe = connect_pipe(address).await?;
    let stop = CancellationToken::new();
    let stopped = stop.clone();
    tokio::spawn(async move {
        let stdout = child.stdout.take().expect("piped");
        let stdin = child.stdin.take().expect("piped");
        let mut stream = tokio::io::join(stdout, stdin);
        let mut pipe = pipe;
        tokio::select! {
            _ = tokio::io::copy_bidirectional(&mut pipe, &mut stream) => {}
            () = stopped.cancelled() => {}
        }
        tree.close();
        let _ = child.start_kill();
        let _ = child.wait().await;
    });
    Ok(stop)
}

/// A client connection to the app's own named pipe.
#[cfg(windows)]
async fn connect_pipe(
    address: &crate::bridge::BridgeAddress,
) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, String> {
    let crate::bridge::BridgeAddress::NamedPipe(name) = address else {
        return Err("the bridge is not a named pipe".to_owned());
    };
    // ERROR_PIPE_BUSY: the server makes the next instance as it accepts.
    const ERROR_PIPE_BUSY: i32 = 231;
    loop {
        match tokio::net::windows::named_pipe::ClientOptions::new().open(name) {
            Ok(client) => return Ok(client),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
}

/// WSL is Windows'.
#[cfg(not(windows))]
async fn connect_pipe(_: &crate::bridge::BridgeAddress) -> Result<tokio::io::DuplexStream, String> {
    Err("Codex's box in WSL runs on Windows".to_owned())
}

/// The WSL launcher (Windows): the Linux box, in the app's distro.
async fn launch_wsl(wsl: &WslBox, spec: &LaunchSpec, host: Arc<Host>) -> Result<Launched, String> {
    let rootfs = (wsl.rootfs)()?;
    let home = spec.dir.join("codex-home");
    let workspace = spec.dir.join("workspace");
    prepare_home(&home)?;
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("mkdir {}: {e}", workspace.display()))?;
    let config_file = home.join("config.toml");
    std::fs::write(
        &config_file,
        config_toml(&spec.model, spec.effort.as_deref(), &spec.system_prompt),
    )
    .map_err(|e| format!("write {}: {e}", config_file.display()))?;
    let run = spec
        .runtime_dir
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{} has no name", spec.runtime_dir.display()))?
        .to_owned();
    let images_dir = spec.dir.join(format!("images-{run}"));
    std::fs::create_dir_all(&images_dir)
        .map_err(|e| format!("mkdir {}: {e}", images_dir.display()))?;
    let images: Vec<String> = write_images(&images_dir, &spec.images)?
        .iter()
        .map(|p| format!("/run/mcbench/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    let run_spec = mcbench_sandbox::proxy::CodexRun {
        rootfs: PathBuf::from(rootfs),
        image_env: Vec::new(),
        relay: PathBuf::from(mcbench_sandbox::wsl_codex::AGENT),
        install: PathBuf::from(&wsl.install.0),
        home: in_distro(wsl, &home)?,
        auth_file: PathBuf::from(format!("{}/auth.json", mcbench_sandbox::wsl_codex::LOGIN)),
        workspace: in_distro(wsl, &workspace)?,
        sock_dir: PathBuf::from(format!("{}/{run}/sock", mcbench_sandbox::wsl_codex::RUNS)),
        allowed_hosts: ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
        proxy_log: in_distro(wsl, &spec.dir.join("egress.ndjson"))?,
        command: box_command(&wsl.install.1, spec, images),
    };
    let args = wsl_box_args(wsl, &run, &in_distro(wsl, &images_dir)?, &run_spec);
    let address = super::bridge_address(&spec.socket);
    let server = bridge::serve(&address, host)?;
    let relay = match wsl_bridge(wsl, &run, &address).await {
        Ok(relay) => relay,
        Err(error) => {
            server.stop();
            return Err(error);
        }
    };
    Ok(Launched {
        command: wsl.program.clone(),
        args,
        cwd: workspace,
        env: super::cli_env(&[("WSL_UTF8", OsString::from("1"))]),
        on_stop: Box::new(|| {}),
        release: Box::new(move || {
            relay.cancel();
            server.stop();
            let _ = std::fs::remove_dir_all(&images_dir);
            Ok(())
        }),
    })
}

/// The sandboxed launcher (`LAUNCHERS.linux`, today a Docker container).
async fn launch_sandboxed(
    config: &CodexConfig,
    source: &SandboxSource,
    spec: &LaunchSpec,
    host: Arc<Host>,
) -> Result<Launched, String> {
    let files = &source()?;
    let home = spec.dir.join("codex-home");
    let workspace = spec.dir.join("workspace");
    prepare_home(&home)?;
    std::fs::create_dir_all(&workspace)
        .map_err(|e| format!("mkdir {}: {e}", workspace.display()))?;
    let config_file = home.join("config.toml");
    std::fs::write(
        &config_file,
        config_toml(&spec.model, spec.effort.as_deref(), &spec.system_prompt),
    )
    .map_err(|e| format!("write {}: {e}", config_file.display()))?;
    let install = codex_install(config).await?;
    let sock_dir = spec.runtime_dir.join("sock");
    let images: Vec<String> = write_images(&sock_dir, &spec.images)?
        .iter()
        .map(|p| format!("/run/mcbench/{}", p.file_name().unwrap().to_string_lossy()))
        .collect();
    let command = box_command(&install.program, spec, images);
    let sandboxed = sandbox_command(&mcbench_sandbox::proxy::CodexRun {
        rootfs: files.rootfs.clone(),
        image_env: files.image_env.clone(),
        relay: files.relay.clone(),
        install: install.folder,
        home,
        auth_file: config.auth_file(),
        workspace: workspace.clone(),
        sock_dir,
        allowed_hosts: ALLOWED_HOSTS.iter().map(|h| (*h).to_owned()).collect(),
        proxy_log: spec.dir.join("egress.ndjson"),
        command,
    })?;
    let server = match bridge::serve(&super::bridge_address(&spec.socket), host) {
        Ok(server) => server,
        Err(error) => {
            (sandboxed.close_proxy)()?;
            return Err(error);
        }
    };
    let close_proxy = sandboxed.close_proxy;
    Ok(Launched {
        command: sandboxed.command,
        args: sandboxed.args,
        cwd: workspace,
        env: sandboxed
            .env
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect(),
        on_stop: Box::new(|| {}),
        release: Box::new(move || {
            let closed = close_proxy();
            server.stop();
            closed
        }),
    })
}

struct TurnState {
    thread_id: String,
    failure: Option<String>,
    completed: bool,
}

/// `CodexSession.start`.
async fn start(inner: &Arc<Inner>, sid: u64, request: &HarnessRequest) -> Result<(), String> {
    let config = &inner.config;
    let Some(launch) = config.launch.clone() else {
        return Err(format!(
            "The Codex CLI harness runs on Linux (in the model's shell) or macOS (on the host), not {}",
            if cfg!(windows) {
                "win32"
            } else {
                std::env::consts::OS
            }
        ));
    };
    let messages = request.values();
    let conversation_id = request.conversation_id().map(str::to_owned);
    let transcript = request.transcript();
    let record = match &conversation_id {
        Some(id) => super::read_session(&config.data_dir, id, NAME)?,
        None => None,
    };
    let terms = super::session_terms(&messages);
    let resume = match (transcript, &record) {
        (Some(transcript), Some(record)) => {
            record.get("home") == Some(&json!("chat"))
                && record.get("instructions") == Some(&json!(terms.instructions))
                && super::session_continues(Some(record), transcript)
        }
        _ => false,
    };
    if let Some(id) = &conversation_id {
        super::forget_session(&config.data_dir, id, NAME)?;
    }
    let request_id = transcript.and_then(|t| t.last()).map(|m| m.id.clone());
    // Codex streams no partial text, so a resumed thread lacks only a
    // changed resolution. Rendered before any folder is made.
    let (prompt, images) = if resume {
        super::harness_prompt(
            &messages,
            false,
            super::resolution_note(record.as_ref().unwrap(), &messages),
        )?
    } else {
        super::harness_prompt(&messages, true, Vec::new())?
    };
    let dir = match (&conversation_id, &launch) {
        (Some(id), _) => super::conversation_dir(&config.data_dir, id),
        // In the WSL distro only the data folder's `harness` is mounted.
        (None, Launch::Wsl(wsl)) => {
            std::fs::create_dir_all(&wsl.harness)
                .map_err(|e| format!("mkdir {}: {e}", wsl.harness.display()))?;
            super::mkdtemp(&wsl.harness.join(".one-shot-"))?
        }
        (None, _) => super::mkdtemp(&super::temp_dir().join("mcbench-codex-"))?,
    };
    let one_shot = conversation_id.is_none();
    let made = super::runtime_dir().and_then(|runtime_dir| {
        match std::fs::create_dir_all(runtime_dir.join("sock")) {
            Ok(()) => Ok(runtime_dir),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&runtime_dir);
                Err(format!("mkdir {}: {e}", runtime_dir.display()))
            }
        }
    });
    let runtime_dir = match made {
        Ok(runtime_dir) => runtime_dir,
        Err(error) => {
            if one_shot {
                let _ = std::fs::remove_dir_all(&dir);
            }
            return Err(error);
        }
    };
    let prefix: String = super::random_uuid().chars().take(8).collect();
    let cleanup = {
        let runtime_dir = runtime_dir.clone();
        let dir = dir.clone();
        move || {
            let _ = std::fs::remove_dir_all(&runtime_dir);
            // A one-shot run (a title, a prompt rewrite) keeps nothing.
            if one_shot {
                let _ = std::fs::remove_dir_all(&dir);
            }
        }
    };
    let thread_id = record
        .as_ref()
        .and_then(|r| r.get("threadId"))
        .map(js_string);
    let spec = LaunchSpec {
        dir: dir.clone(),
        runtime_dir: runtime_dir.clone(),
        socket: runtime_dir.join("sock").join("mcp.sock"),
        model: request.model.clone(),
        effort: request.effort.clone(),
        system_prompt: super::system_text(&messages),
        resume,
        thread_id: thread_id.clone(),
        images,
    };
    let host = Arc::new(Host {
        inner: Arc::downgrade(inner),
        sid,
        prefix,
        tools: request.tools.clone(),
    });
    let launched = match &launch {
        Launch::Host => launch_host(config, &spec, host),
        Launch::Sandboxed(files) => launch_sandboxed(config, files, &spec, host).await,
        Launch::Wsl(wsl) => launch_wsl(wsl, &spec, host).await,
        Launch::Custom(launcher) => launcher(&spec, host),
    };
    let launched = match launched {
        Ok(launched) => launched,
        Err(error) => {
            cleanup();
            return Err(error);
        }
    };
    let Launched {
        command,
        args,
        cwd,
        env,
        on_stop,
        release,
    } = launched;
    let mut core = inner.core.lock().unwrap();
    // Stopped while launching: nothing was spawned.
    if !core.sessions.contains_key(&sid) {
        drop(core);
        let released = release();
        cleanup();
        released?;
        // Its completion has the close's reason already (its waiter).
        return Ok(());
    }
    let state = Arc::new(Mutex::new(TurnState {
        thread_id: if resume {
            thread_id.clone().unwrap_or_else(|| "undefined".to_owned())
        } else {
            "null".to_owned()
        },
        failure: None,
        completed: false,
    }));
    let weak = Arc::downgrade(inner);
    let terms_record = terms.clone();
    let has_transcript = transcript.is_some();
    let on_line = {
        let weak = weak.clone();
        let state = state.clone();
        let conversation_id = conversation_id.clone();
        Box::new(move |event: Value| -> Result<(), String> {
            let Some(inner) = weak.upgrade() else {
                return Ok(());
            };
            let mut core = inner.core.lock().unwrap();
            let item = event.get("item").filter(|i| truthy(Some(i)));
            if let Some(item) = item {
                check_item(item)?;
            }
            let kind = event.get("type").and_then(Value::as_str);
            let Some(session) = core.sessions.get_mut(&sid) else {
                return Ok(());
            };
            let relay = &mut session.relay;
            let mut state = state.lock().unwrap();
            if kind == Some("thread.started") {
                // The id is stored for the next turn's `codex exec resume`:
                // only a thread id as Codex writes one is ever stored.
                let thread = event
                    .get("thread_id")
                    .and_then(Value::as_str)
                    .filter(|id| is_thread_id(id))
                    .ok_or_else(|| NOT_A_THREAD_ID.to_owned())?
                    .to_owned();
                relay.started();
                state.thread_id = thread.clone();
                if has_transcript {
                    let mut record: Record = Map::new();
                    record.insert("threadId".to_owned(), json!(thread));
                    record.insert("home".to_owned(), json!("chat"));
                    record.insert("instructions".to_owned(), json!(terms_record.instructions));
                    record.insert("resolution".to_owned(), json!(terms_record.resolution));
                    if let Some(id) = &request_id {
                        record.insert("lastSeen".to_owned(), json!(id));
                    }
                    super::write_session(
                        &inner.config.data_dir,
                        conversation_id.as_deref().unwrap(),
                        NAME,
                        &record,
                    )?;
                }
            }
            let item_type = item.and_then(|i| i.get("type")).and_then(Value::as_str);
            let item_text = |key: &str| {
                item.and_then(|i| i.get(key))
                    .map(js_string)
                    .unwrap_or_else(|| "undefined".to_owned())
            };
            if kind == Some("item.completed") && item_type == Some("agent_message") {
                let joined = format!(
                    "{}{}",
                    if relay.text.is_empty() { "" } else { "\n\n" },
                    item_text("text")
                );
                relay.update(Some(&joined), None, None);
            }
            if kind == Some("item.completed") && item_type == Some("reasoning") {
                let joined = format!(
                    "{}{}",
                    if relay.reasoning.is_empty() {
                        ""
                    } else {
                        "\n\n"
                    },
                    item_text("text")
                );
                relay.update(None, Some(&joined), None);
            }
            if kind == Some("turn.completed") {
                state.completed = true;
                relay.update(None, None, Some(usage_of(event.get("usage"))?));
            }
            if item_type == Some("todo_list") {
                let entries = match item.and_then(|i| i.get("items")) {
                    Some(Value::Array(entries)) => entries.clone(),
                    _ => {
                        return Err(
                            "Cannot read properties of undefined (reading 'map')".to_owned()
                        );
                    }
                };
                let plan: Vec<Value> = entries
                    .iter()
                    .map(|entry| {
                        let mut out = Map::new();
                        if let Some(text) = entry.get("text") {
                            out.insert("text".to_owned(), text.clone());
                        }
                        out.insert(
                            "status".to_owned(),
                            json!(if truthy(entry.get("completed")) {
                                "done"
                            } else {
                                "pending"
                            }),
                        );
                        Value::Object(out)
                    })
                    .collect();
                relay.set_plan(Value::Array(plan));
            }
            if kind == Some("item.completed") && item_type == Some("error") {
                relay.notice("warning", item.and_then(|i| i.get("message")));
            }
            if kind == Some("turn.failed") {
                let error = event.get("error");
                let message = error
                    .and_then(|e| e.get("message"))
                    .filter(|m| truthy(Some(m)));
                state.failure = Some(match message {
                    Some(m) => js_string(m),
                    None => error
                        .map(mcbench_json::stringify)
                        .unwrap_or_else(|| "undefined".to_owned()),
                });
            }
            if kind == Some("error") {
                let message = event.get("message").filter(|m| truthy(Some(m)));
                state.failure = Some(match message {
                    Some(m) => js_string(m),
                    None => mcbench_json::stringify(&event),
                });
            }
            Ok(())
        })
    };
    let home = dir.join("codex-home");
    let done = {
        let weak = weak.clone();
        Box::new(move |outcome: Result<process::Exit, String>| {
            if let Some(inner) = weak.upgrade() {
                let mut core = inner.core.lock().unwrap();
                let state = state.lock().unwrap();
                let error = match outcome {
                    Err(error) => Some(error),
                    Ok(_) if state.failure.is_some() => Some(format!(
                        "{}{}",
                        text::CODEX_FAILED,
                        state.failure.clone().unwrap()
                    )),
                    Ok(exit) if exit.code != Some(0) => Some(format!(
                        "codex exec exited {}: {}",
                        exit.code_or_signal(),
                        exit.stderr
                    )),
                    Ok(_) => None,
                };
                if let Some(session) = core.sessions.get_mut(&sid) {
                    let mut error = error;
                    // Codex flushes the rollout file before exiting, so it
                    // is complete only now.
                    if error.is_none()
                        && state.completed
                        && session.relay.state == RelayState::Running
                    {
                        match context_tokens_of(&home, &state.thread_id) {
                            Ok(tokens) => session.relay.set_context_tokens(tokens),
                            Err(e) => error = Some(e),
                        }
                    }
                    session.relay.finish(error, None);
                    settle(&mut core, sid);
                }
            }
            let released = release();
            cleanup();
            if let Err(error) = released {
                eprintln!("[codex-cli] {error}");
            }
        })
    };
    let run = process::run_json_lines(
        process::Launch {
            name: "The Codex CLI",
            command: &command,
            args: &args,
            cwd: &cwd,
            env: &env,
        },
        prompt.into_bytes(),
        false,
        on_line,
        on_stop,
        done,
    )?;
    if let Some(session) = core.sessions.get_mut(&sid) {
        session.run = Some(run);
    }
    Ok(())
}
