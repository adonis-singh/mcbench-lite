//! The providers of a data folder (`server/llm.js` `resolveProvider`,
//! `settings.js` `connected`, `applyRuntimes`, `LOGINS`; `harness.js`
//! and the CLI modules' `configure`): the API providers made from the
//! settings' keys on each call, and ONE Claude Code and ONE Codex CLI
//! provider for the process (they hold the CLIs' sessions), each CLI run by
//! the program its variable names (`MCBENCH_CLAUDE_BINARY`,
//! `MCBENCH_CODEX_BINARY`), else the one on `PATH`, else the tools
//! folder's, with the CLI's own sign-in where the CLI keeps it (Claude
//! Code: `CLAUDE_CONFIG_DIR`, else the home folder's `.claude.json`;
//! Codex: `CODEX_HOME`, else `~/.codex/auth.json`). Settings never name a
//! CLI's program or folder, as today. On Linux Codex runs in its box (the
//! model's shell's root filesystem, unpacked by the sandbox into the data
//! folder's `sandbox/`, and the static relay agent [`sandbox_agent_program`]).
//!
//! Shared by the app's core (`mcbench_lite::core`) and `headless turn`
//! (`turn.rs`' `adapter::provider`), so both make the same providers the
//! same way.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use mcbench_providers::api::{self, Api, Endpoints};
use mcbench_providers::harness::claude_code::{ClaudeCode, ClaudeConfig, Models};
use mcbench_providers::harness::codex_cli::{
    self, CodexCli, CodexConfig, Launch, SandboxFiles, SandboxSource, WslBox,
};
use mcbench_providers::harness::install::{self, Lookup, Pin};
use mcbench_providers::harness::{BridgeCommand, Logins};
use mcbench_providers::{CliRuntime, Provider, ProviderConfig, ProviderId};
use mcbench_sandbox::wsl_codex;
use mcbench_store::{PROVIDERS, ProviderInfo, Settings, SettingsFile};

/// The sentence a chat whose provider is Cursor is refused with: Cursor is
/// left out of the native app (`native/README.md`, Decisions of stage three).
pub const CURSOR_LEFT_OUT: &str = "Cursor is not in the native app";

/// The static relay agent's program, `mcbench-sandbox-agent`: Codex's box
/// on Linux runs it as its network relay and tool bridge.
pub const SANDBOX_AGENT: &str = "mcbench-sandbox-agent";

/// The variable that names the built static agent for tests and scripts.
pub const SANDBOX_AGENT_VAR: &str = "MCBENCH_SANDBOX_AGENT_STATIC";

/// The static relay agent: the program `MCBENCH_SANDBOX_AGENT_STATIC` names
/// when it is set, otherwise `mcbench-sandbox-agent` beside the running
/// executable, where the app ships it on Linux; an error saying how to
/// build it when that file is not there (the rule of
/// `mcbench_build::child_program`).
pub fn sandbox_agent_program() -> Result<PathBuf, String> {
    let program = match std::env::var_os(SANDBOX_AGENT_VAR).filter(|v| !v.is_empty()) {
        Some(named) => PathBuf::from(named),
        None => std::env::current_exe()
            .map_err(|e| format!("this program's own path: {e}"))?
            .with_file_name(format!("{SANDBOX_AGENT}{}", std::env::consts::EXE_SUFFIX)),
    };
    if program.is_file() {
        Ok(program)
    } else {
        Err(format!(
            "The sandbox's agent {} is not there: build it with `cargo build --release -p mcbench-sandbox --bin mcbench-sandbox-agent --target x86_64-unknown-linux-musl` and put it beside the app, or name the built program with {SANDBOX_AGENT_VAR}",
            program.display()
        ))
    }
}

/// The model's shell's root filesystem as setup's shell step found it
/// (`mcbench_setup::shell::files`), for Codex's box on Linux.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ShellRootfs {
    /// The shell step has not named the files yet.
    #[default]
    Waiting,
    /// This build has none.
    Missing,
    /// Its tar.zst and its sha256.
    File(PathBuf, String),
}

/// Where the providers read [`ShellRootfs`]; whoever runs the shell step
/// writes it.
pub type RootfsSlot = Arc<Mutex<ShellRootfs>>;

/// Codex's box on Linux, asked at each launch: the root filesystem `shell`
/// holds (its file and sha256), as the sandbox unpacked it under `root`
/// (`mcbench_sandbox::linux::rootfs::unpack`'s `.rootfs-<sha256>`), and the
/// static agent.
fn codex_box(root: PathBuf, shell: RootfsSlot) -> SandboxSource {
    Arc::new(move || {
        let not_yet =
            "The Codex CLI runs in the model's shell on Linux, which setup has not prepared yet";
        let sha256 = match &*shell.lock().expect("the shell's slot") {
            ShellRootfs::Waiting => return Err(not_yet.into()),
            ShellRootfs::Missing => return Err(
                "The Codex CLI runs in the model's shell on Linux, and no root filesystem is named (MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256)".into(),
            ),
            ShellRootfs::File(_, sha256) => sha256.clone(),
        };
        let dir = root.join(format!(".rootfs-{sha256}"));
        if !dir.join(".complete").is_file() {
            return Err(not_yet.into());
        }
        let unpacked = mcbench_sandbox::linux::rootfs::check(&dir)?;
        Ok(SandboxFiles {
            rootfs: unpacked.rootfs,
            image_env: unpacked.image.env,
            relay: sandbox_agent_program()?,
        })
    })
}

/// Codex's box in the app's WSL distro (Windows): the agent there reached
/// as root with `wsl.exe --exec`, with the person's Codex login folder
/// `login`, the tools folder and the data folder's `harness` as its three
/// mounts, and `install` (the install's folder in the distro and its
/// program in the box). A turn is refused with a person's sentence until
/// setup has the distro and the root filesystem; the box's root filesystem
/// is the one the distro's agent unpacked (`/var/mcbench/.rootfs-<sha256>`).
fn codex_wsl(
    data: &std::path::Path,
    tools: &std::path::Path,
    login: &std::path::Path,
    install: (String, String),
    shell: RootfsSlot,
) -> WslBox {
    let harness = data.join("harness");
    let windows = |path: &std::path::Path| {
        path.to_string_lossy()
            .trim_start_matches(r"\\?\")
            .to_owned()
    };
    let (uid, gid) = mcbench_sandbox::windows::GUEST_USER;
    let mut agent: Vec<String> = [
        "--distribution",
        &codex_distro(),
        "--user",
        "root",
        "--cd",
        "/",
        "--exec",
        wsl_codex::AGENT,
        "codex",
    ]
    .map(String::from)
    .into();
    agent.extend([
        windows(login),
        windows(tools),
        windows(&harness),
        format!("{uid}:{gid}"),
    ]);
    let shares = [login.to_path_buf(), tools.to_path_buf(), harness.clone()];
    // drvfs mounts folders that exist; Codex makes the login folder itself
    // at its first sign-in, which runs in the distro.
    if cfg!(windows) {
        for share in &shares {
            std::fs::create_dir_all(share)
                .unwrap_or_else(|e| panic!("mkdir {}: {e}", share.display()));
        }
    }
    WslBox {
        program: codex_wsl_program(),
        agent,
        harness,
        install,
        ids: (uid, gid),
        rootfs: Arc::new(move || {
            codex_wsl_ready()?;
            for share in &shares {
                mcbench_sandbox::windows::drvfs_path(share)
                    .map_err(|why| format!("{}: {why}", wsl_codex::WSL_UNAVAILABLE))?;
            }
            match &*shell.lock().expect("the shell's slot") {
                ShellRootfs::File(_, sha256) => Ok(format!("/var/mcbench/.rootfs-{sha256}/rootfs")),
                ShellRootfs::Waiting | ShellRootfs::Missing => Err(wsl_codex::NOT_YET.to_owned()),
            }
        }),
    }
}

/// The distro's name.
#[cfg(windows)]
fn codex_distro() -> String {
    mcbench_sandbox::windows::wsl::distro()
}

/// WSL is Windows': elsewhere the box is never launched.
#[cfg(not(windows))]
fn codex_distro() -> String {
    String::new()
}

/// `wsl.exe` in Windows' system folder.
#[cfg(windows)]
fn codex_wsl_program() -> OsString {
    mcbench_sandbox::windows::wsl::program()
        .expect("Windows names its system folder")
        .into_os_string()
}

/// WSL is Windows': elsewhere the box is never launched.
#[cfg(not(windows))]
fn codex_wsl_program() -> OsString {
    OsString::from("wsl.exe")
}

/// What setup's shell step found of the distro.
#[cfg(windows)]
fn codex_wsl_ready() -> Result<(), String> {
    wsl_codex::ready()
}

/// WSL is Windows'.
#[cfg(not(windows))]
fn codex_wsl_ready() -> Result<(), String> {
    Err(wsl_codex::NOT_YET.to_owned())
}

/// `settings.js` `LOGIN_CLI`: the CLI whose login and program a
/// `cli-login` provider uses.
fn login_cli(id: &str) -> Option<&'static str> {
    match id {
        "codex" | "codex-cli" => Some("codex-cli"),
        "claude-code" => Some("claude-code"),
        _ => None,
    }
}

/// `settings.PROVIDERS[id]`.
pub fn info(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

fn provider_id(key: &str) -> Option<ProviderId> {
    serde_json::from_value(serde_json::Value::String(key.into())).ok()
}

/// The providers of one data folder.
pub struct Providers {
    data: PathBuf,
    settings: Arc<SettingsFile>,
    tools_dir: PathBuf,
    lookup: Lookup,
    bridge: BridgeCommand,
    shell: RootfsSlot,
    api: Arc<Api>,
    claude: OnceLock<Arc<ClaudeCode>>,
    codex: OnceLock<Arc<CodexCli>>,
}

impl Providers {
    /// The providers of `data` with its settings, the CLIs installed in
    /// `tools_dir`, this process's `PATH` and CLI variables, `program` (the
    /// app's own binary) as the CLIs' tool bridge, and `shell` (the model's
    /// shell's root filesystem, as the shell step writes it) as Codex's box
    /// on Linux.
    pub fn new(
        data: PathBuf,
        settings: Arc<SettingsFile>,
        tools_dir: PathBuf,
        program: PathBuf,
        shell: RootfsSlot,
    ) -> Providers {
        Providers {
            data,
            settings,
            tools_dir,
            lookup: Lookup::from_env(),
            bridge: BridgeCommand { program },
            shell,
            api: Api::new(Endpoints::today()),
            claude: OnceLock::new(),
            codex: OnceLock::new(),
        }
    }

    /// The model's shell's root filesystem as Codex's box reads it.
    pub fn shell(&self) -> &RootfsSlot {
        &self.shell
    }

    /// What the API providers share (the catalog, the HTTP client).
    pub fn api(&self) -> &Arc<Api> {
        &self.api
    }

    /// The settings as saved.
    pub fn settings(&self) -> Settings {
        self.settings.get()
    }

    /// The settings file.
    pub fn settings_file(&self) -> &Arc<SettingsFile> {
        &self.settings
    }

    /// The tools folder the CLIs are installed into.
    pub fn tools_dir(&self) -> &PathBuf {
        &self.tools_dir
    }

    /// The CLI lookups of this process's environment.
    pub fn lookup(&self) -> &Lookup {
        &self.lookup
    }

    fn pin(name: &str) -> Pin {
        install::pins()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap_or_else(|| panic!("no pin for the {name} CLI"))
    }

    /// `process.env[binaryEnv] || tools.cliProgram(binary)`.
    pub fn cli_program(&self, name: &str) -> OsString {
        match self.lookup.named(name) {
            Some(named) => OsString::from(named),
            None => install::cli_program(
                &Self::pin(name),
                &self.tools_dir,
                &install::platform(),
                &self.lookup,
            )
            .into_os_string(),
        }
    }

    /// `tools.cliInstalled(binary, binaryEnv)`.
    pub fn cli_installed(&self, name: &str) -> bool {
        install::cli_installed(
            &Self::pin(name),
            &self.tools_dir,
            &install::platform(),
            &self.lookup,
        )
    }

    /// Claude Code's configuration as `claude-code.js` `configure` leaves it.
    pub fn claude_config(&self) -> ClaudeConfig {
        ClaudeConfig {
            data_dir: self.data.clone(),
            binary: self.cli_program("claude"),
            home: None,
            bridge: self.bridge.clone(),
        }
    }

    /// Codex's configuration as `codex-cli.js` `configure` leaves it, with
    /// this platform's launch (`Launch::for_this_platform`): in its box on
    /// Linux (`codex_box`: today's server runs it in a Docker container
    /// there), on the host in Codex's read-only sandbox on macOS, in the same
    /// box in the app's WSL distro on Windows (`codex_wsl`), where every
    /// Codex command runs through `wsl.exe` and the program is the tools
    /// folder's Linux install.
    pub fn codex_config(&self) -> CodexConfig {
        let launch = Launch::for_this_platform(
            codex_box(self.data.join("sandbox"), self.shell.clone()),
            || self.codex_wsl().0,
        );
        let (binary, prefix) = if cfg!(windows) {
            let (wsl, program) = self.codex_wsl();
            let mut prefix = wsl.agent;
            prefix.extend(["cli".to_owned(), program]);
            (wsl.program, prefix)
        } else {
            (self.cli_program("codex"), Vec::new())
        };
        CodexConfig {
            data_dir: self.data.clone(),
            binary,
            prefix,
            home: None,
            bridge: self.bridge.clone(),
            launch,
        }
    }

    /// Codex's box in the WSL distro, and the program of the tools folder's
    /// Linux install there.
    fn codex_wsl(&self) -> (WslBox, String) {
        let pin = Self::pin("codex");
        let bin = pin
            .asset(&install::platform_of("codex", &install::platform()))
            .map(|a| a.bin.clone())
            .expect("Codex is pinned for this platform's box");
        let folder = format!("{}/{}-{}", wsl_codex::TOOLS, pin.name, pin.version);
        let wsl = codex_wsl(
            &self.data,
            &self.tools_dir,
            &codex_cli::default_home(),
            (folder.clone(), format!("/opt/codex/{bin}")),
            self.shell.clone(),
        );
        (wsl, format!("{folder}/{bin}"))
    }

    /// The CLIs' sign-in commands (`provider-login.js`).
    pub fn logins(&self) -> Logins {
        Logins {
            claude: self.claude_config(),
            codex: self.codex_config(),
        }
    }

    /// `LOGINS[id]()`: whether the provider's CLI login is stored. Cursor
    /// has none in the native app.
    pub fn stored_login(&self, id: &str) -> Result<bool, String> {
        match id {
            "codex" | "codex-cli" => Ok(self.codex_config().stored_login()),
            "claude-code" => self.claude_config().stored_login(),
            _ => Ok(false),
        }
    }

    /// `keySource(id)`: `saved`, `env` or none.
    pub fn key_source(&self, settings: &Settings, id: &str) -> Option<&'static str> {
        let info = info(id)?;
        if settings.api_key(id).is_some_and(|key| !key.is_empty()) {
            return Some("saved");
        }
        let from_env = info
            .env_key
            .and_then(|name| std::env::var(name).ok())
            .is_some_and(|value| !mcbench_providers::api::js::trim(&value).is_empty());
        from_env.then_some("env")
    }

    /// `connected(id)`.
    pub fn connected(&self, settings: &Settings, id: &str) -> Result<bool, String> {
        let info = info(id).ok_or_else(|| format!("Unknown provider: {id}"))?;
        if info.auth == Some("cli-login") {
            let cli = login_cli(id).expect("a cli-login provider has a CLI");
            let binary = info_runtime(cli).0;
            return Ok(self.stored_login(id)? && self.cli_installed(binary));
        }
        Ok(self.key_source(settings, id).is_some()
            || (info.auth == Some("api-key-or-login") && self.stored_login(id)?))
    }

    /// The CLI runtime a provider's login or catalogue reads.
    fn cli_runtime(&self, id: ProviderId) -> Option<CliRuntime> {
        let strings = |env: Vec<(OsString, OsString)>| {
            env.into_iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect()
        };
        match id {
            ProviderId::Codex | ProviderId::CodexCli => {
                let config = self.codex_config();
                Some(CliRuntime {
                    program: config.binary.to_string_lossy().into_owned(),
                    args: config.prefix.clone(),
                    home: config.codex_home(),
                    env: strings(config.host_env(None)),
                })
            }
            ProviderId::ClaudeCode => {
                let config = self.claude_config();
                Some(CliRuntime {
                    program: config.binary.to_string_lossy().into_owned(),
                    args: Vec::new(),
                    home: config.config_dir(),
                    env: strings(config.env()),
                })
            }
            _ => None,
        }
    }

    /// The provider's key, switch, connection and CLI as the settings stand
    /// now (`resolveProvider`'s inputs).
    pub fn config(&self, key: &str) -> Result<ProviderConfig, String> {
        let id = provider_id(key).ok_or_else(|| format!("Unknown provider: {key}"))?;
        let info = info(key).ok_or_else(|| format!("Unknown provider: {key}"))?;
        let settings = self.settings();
        let saved = settings.api_key(key).unwrap_or_default().to_string();
        let api_key = if saved.is_empty() {
            info.env_key
                .and_then(|name| std::env::var(name).ok())
                .map(|value| mcbench_providers::api::js::trim(&value).to_string())
                .unwrap_or_default()
        } else {
            saved
        };
        Ok(ProviderConfig {
            id,
            connected: self.connected(&settings, key)?,
            enabled: settings.enabled(key),
            api_key,
            cli: self.cli_runtime(id),
        })
    }

    /// The process's Claude Code provider.
    pub fn claude(&self) -> Result<Arc<ClaudeCode>, String> {
        if let Some(claude) = self.claude.get() {
            return Ok(Arc::clone(claude));
        }
        let models = Models {
            catalog: Arc::clone(&self.api.catalog),
            api: Arc::clone(&self.api),
            config: self.config("claude-code")?,
        };
        Ok(Arc::clone(self.claude.get_or_init(|| {
            Arc::new(ClaudeCode::new(self.claude_config(), Some(models)))
        })))
    }

    /// The process's Codex CLI provider.
    pub fn codex(&self) -> Arc<CodexCli> {
        Arc::clone(self.codex.get_or_init(|| {
            Arc::new(CodexCli::new(
                self.codex_config(),
                Arc::clone(&self.api.catalog.codex),
            ))
        }))
    }

    /// `resolveProvider(key)` and the provider it resolves to: refused when
    /// unknown, off in Settings or not connected, with today's sentences.
    pub fn provider(&self, key: &str) -> Result<Arc<dyn Provider>, String> {
        let config = self.config(key)?;
        if config.id == ProviderId::Cursor {
            return Err(CURSOR_LEFT_OUT.into());
        }
        api::resolve(&config).map_err(|e| e.message)?;
        Ok(match config.id {
            ProviderId::ClaudeCode => self.claude()?,
            ProviderId::CodexCli => self.codex(),
            _ => self.api.provider(config),
        })
    }

    /// `listModels(provider)`: resolved first, as today; Claude Code's from
    /// the catalogue, the Codex CLI's from `codex debug models`.
    pub async fn models(&self, key: &str) -> Result<Vec<mcbench_providers::ModelInfo>, String> {
        let config = self.config(key)?;
        match config.id {
            ProviderId::Cursor => Err(CURSOR_LEFT_OUT.into()),
            ProviderId::CodexCli => {
                api::resolve(&config).map_err(|e| e.message)?;
                self.api
                    .catalog
                    .codex
                    .list(config.cli.as_ref().expect("the Codex CLI's runtime"))
                    .await
                    .map_err(|e| e.message)
            }
            _ => self
                .api
                .catalog
                .list_models(&self.api, &config)
                .await
                .map_err(|e| e.message),
        }
    }

    /// `modelInfo(provider, model)`.
    pub async fn model_info(
        &self,
        key: &str,
        model: &str,
    ) -> Result<mcbench_providers::ModelFacts, String> {
        let config = self.config(key)?;
        if config.id == ProviderId::Cursor {
            return Err(CURSOR_LEFT_OUT.into());
        }
        self.api
            .catalog
            .model_info(&self.api, &config, model)
            .await
            .map_err(|e| e.message)
    }

    /// `harness.shutdownHarnesses()`: every CLI process ends.
    pub async fn shutdown(&self) {
        // A Codex run lives only within its turn, and the turns have ended.
        if let Some(claude) = self.claude.get() {
            claude.shutdown().await;
        }
        self.api.catalog.stop();
    }

    /// `harness.destroy(id)`: a chat removed for good leaves no CLI process,
    /// session record or `harness/<id>` folder.
    pub async fn chat_removed(&self, id: &str) {
        if let Some(claude) = self.claude.get()
            && let Err(error) = claude.destroy(id).await
        {
            eprintln!("[harness] {error}");
        }
        self.codex().destroy(id);
    }
}

/// `PROVIDERS[id].runtime`'s `(binary, binaryEnv, homeEnv, home)`.
pub fn info_runtime(id: &str) -> (&'static str, &'static str, &'static str, &'static str) {
    info(id)
        .and_then(|info| info.runtime)
        .unwrap_or_else(|| panic!("{id} has no CLI runtime"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder unpacked as `rootfs::unpack` leaves it, with empty files.
    fn unpacked(root: &std::path::Path, sha256: &str) {
        let dir = root.join(format!(".rootfs-{sha256}"));
        for program in mcbench_sandbox::linux::rootfs::REQUIRED {
            let file = dir.join("rootfs").join(program);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "").unwrap();
        }
        std::fs::write(dir.join("pasta"), "").unwrap();
        std::fs::write(
            dir.join("image.json"),
            r#"{"env":["PATH=/usr/bin","LANG=C.UTF-8"]}"#,
        )
        .unwrap();
        std::fs::write(dir.join(".complete"), sha256).unwrap();
    }

    #[test]
    fn codex_box_is_refused_until_the_shell_is_unpacked_then_names_its_files() {
        let root = std::env::temp_dir().join(format!("mcbench-codex-box-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let agent = root.join("agent");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&agent, "").unwrap();
        // SAFETY: no other test of this crate reads or sets this variable.
        unsafe { std::env::set_var(SANDBOX_AGENT_VAR, &agent) };
        let slot = RootfsSlot::default();
        let source = codex_box(root.clone(), Arc::clone(&slot));
        let not_yet =
            "The Codex CLI runs in the model's shell on Linux, which setup has not prepared yet";
        assert_eq!(source().unwrap_err(), not_yet);
        *slot.lock().unwrap() = ShellRootfs::Missing;
        assert_eq!(
            source().unwrap_err(),
            "The Codex CLI runs in the model's shell on Linux, and no root filesystem is named (MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256)"
        );
        *slot.lock().unwrap() = ShellRootfs::File(root.join("r.tar.zst"), "ab12".into());
        assert_eq!(source().unwrap_err(), not_yet);
        unpacked(&root, "ab12");
        let files = source().unwrap();
        assert_eq!(files.rootfs, root.join(".rootfs-ab12").join("rootfs"));
        assert_eq!(files.image_env, ["PATH=/usr/bin", "LANG=C.UTF-8"]);
        assert_eq!(files.relay, agent);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
