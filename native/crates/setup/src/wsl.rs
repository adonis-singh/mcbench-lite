//! What the setup screen says while the model's shell waits for WSL on
//! Windows (`ui-seam-wsl-install.md`): the owner's six sentences, which the
//! shell step's text carries and the window shows as given. Nothing a model
//! reads is here (its two sentences are `mcbench_sandbox::windows`'
//! `WSL_MISSING` and `WSL_UNAVAILABLE`).

/// Sentence 1: WSL is not installed (the step needs `Install WSL`).
pub const NEEDS_WSL: &str = "To use bash and build, Windows needs WSL. Install it here: Windows asks for an administrator's permission once, and for a restart when its virtualisation feature was off.";

/// Sentence 2: the installer runs.
pub const INSTALLING: &str = "Installing WSL. Windows is asking for an administrator's permission.";

/// Sentence 3: WSL is installed and Windows must restart (the step needs
/// `Restart now`).
pub const RESTART: &str =
    "WSL is installed. Restart Windows to finish; the app opens again by itself and carries on.";

/// Sentence 4: the administrator's prompt was declined.
pub const DECLINED: &str = "WSL was not installed: the administrator's permission was not given.";

/// Sentence 5: the installer failed, with what Windows said.
pub fn not_installed(said: &str) -> String {
    format!("WSL could not be installed: {said}")
}

/// Sentence 6: Windows refused the restart, with what Windows said.
pub fn not_restarted(said: &str) -> String {
    format!(
        "Windows did not restart: {said}. Restart it yourself; the app carries on when it is opened again."
    )
}
