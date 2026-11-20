//! Installing WSL and restarting Windows from the setup screen
//! (`ui-seam-wsl-install.md`, the owner's decision in `docs/native-app.md`):
//! `Command::InstallWsl` and `Command::RestartComputer`. Each acts only
//! while the shell step needs it (`Step.needs`, which only Windows fills);
//! otherwise it changes nothing and ends in `Update::Setup` as the steps
//! are.
//!
//! | Command | What the core does | Its updates |
//! |---|---|---|
//! | `InstallWsl` | `wsl.exe --install --no-distribution` elevated (Windows' own prompt), waited for; a restart is owed when the virtualisation feature was off before or WSL does not answer after (`mcbench_sandbox::windows::install_wsl`) | `Update::Setup` at each change: the step running with sentence 2; then ready and the shell step run again (the distro imported, the sandbox prepared), or `needs: Restart` with sentence 3 (this boot recorded under `<data>\sandbox\.wsl`), or failed with sentence 4 (the prompt declined) or 5 |
//! | `RestartComputer` | the app registered to start once at the next sign-in (`RunOnce`), an ordinary restart asked for | nothing when Windows restarts; when it refuses, `Update::Error` with sentence 6 (the value removed again), then `Update::Setup` |

use mcbench_sandbox::windows::{self, InstallFailure, Installed};
use mcbench_setup::{Needs, StepState, wsl};
use mcbench_ui::Update;

use super::Core;

/// The shell step's id.
const SHELL: &str = "shell";

impl Core {
    /// Moves the shell step from needing `needs` to `state` with `then`, in
    /// one step (two presses of the button act once); whether it did.
    fn take_need(&self, needs: Needs, state: StepState, then: Option<Needs>) -> bool {
        self.0
            .steps
            .set_if_needs(SHELL, needs, state, then)
            .expect("the native steps have a shell step")
    }

    /// `Command::InstallWsl`.
    pub(crate) async fn install_wsl(&self) {
        let Some(app) = self.0.app.clone() else {
            return self.publish(Update::Setup(self.setup_view().1));
        };
        let running = StepState::Running {
            detail: Some(wsl::INSTALLING.into()),
            progress: None,
        };
        if !self.take_need(Needs::InstallWsl, running, None) {
            return self.publish(Update::Setup(self.setup_view().1));
        }
        let root = self.0.store.folder().root.join("sandbox");
        let installed = tokio::task::spawn_blocking(move || windows::install_wsl(&root))
            .await
            .expect("the WSL install does not panic");
        match installed {
            Ok(Installed::Ready) => self.run_shell_step(&app),
            Ok(Installed::Restart) => self.set_shell(
                StepState::Unavailable {
                    error: wsl::RESTART.into(),
                },
                Some(Needs::Restart),
            ),
            Err(InstallFailure::Declined) => self.set_shell(
                StepState::Failed {
                    error: wsl::DECLINED.into(),
                },
                Some(Needs::InstallWsl),
            ),
            Err(InstallFailure::Failed(said)) => self.set_shell(
                StepState::Failed {
                    error: wsl::not_installed(&said),
                },
                Some(Needs::InstallWsl),
            ),
            // WSL is installed but the restart it owes is not recorded: the
            // file's own error, and installing again records it.
            Err(InstallFailure::Record(said)) => {
                self.set_shell(StepState::Failed { error: said }, Some(Needs::InstallWsl))
            }
        }
    }

    /// `Command::RestartComputer`.
    pub(crate) async fn restart_computer(&self) {
        let Some(app) = self.0.app.clone() else {
            return self.publish(Update::Setup(self.setup_view().1));
        };
        // The restart under way: sentence 3's state with no button, so a
        // second press asks nothing; a refusal offers the button again.
        let restart = || StepState::Unavailable {
            error: wsl::RESTART.into(),
        };
        if !self.take_need(Needs::Restart, restart(), None) {
            return self.publish(Update::Setup(self.setup_view().1));
        }
        let program = app.program().to_path_buf();
        let restarted = tokio::task::spawn_blocking(move || windows::restart_computer(&program))
            .await
            .expect("the restart request does not panic");
        if let Err(said) = restarted {
            self.publish(Update::Error(wsl::not_restarted(&said)));
            self.set_shell(restart(), Some(Needs::Restart));
        }
    }

    /// The shell step set (each change is published as `Update::Setup`).
    fn set_shell(&self, state: StepState, needs: Option<Needs>) {
        self.0
            .steps
            .set(SHELL, state, needs)
            .expect("the native steps have a shell step");
    }
}
