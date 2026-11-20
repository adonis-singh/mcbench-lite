//! The setup screen's WSL install and restart on a real Windows
//! (`ui-seam-wsl-install.md`), through the core with the app's own backend
//! and a recording window: the steps a person takes, one per run, on the
//! data folder the app itself uses (`mcbench_setup::data_folder`), so that
//! the app started by Windows after the restart carries on in the same
//! folder.
//!
//! `MCBENCH_WSL_SETUP` names the step:
//!
//! | Step | What it does | What it checks |
//! |---|---|---|
//! | `install` | waits for the shell step to need `Install WSL`, sends `Command::InstallWsl` | sentence 1 before; sentence 2 running; then sentence 3 with `Restart`, sentence 4 failed with `Install WSL` (the prompt declined), or the step going on |
//! | `restart` | waits for the shell step to need `Restart now`, sends `Command::RestartComputer` | Windows restarts (the run is cut short), or sentence 6 and no `RunOnce` value |
//! | `done` | waits for the shell step | done, with nothing needed |
//!
//! It needs `MCBENCH_WSL_SETUP_APP` (the `mcbench-lite.exe` that `RunOnce`
//! is to start) and the shell's files (`MCBENCH_SANDBOX_ROOTFS`,
//! `MCBENCH_SANDBOX_GUEST` and their sha256), and is run in the person's
//! own desktop session (Windows shows its administrator's prompt there),
//! one test thread: `cargo test -p mcbench-lite --test wsl_setup -- --ignored`.

#![cfg(windows)]

use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use mcbench_lite::core::{Core, Parts};
use mcbench_setup::{Needs, Step, StepState, wsl};
use mcbench_ui::{Command, Update, Window};

#[derive(Default)]
struct Recording {
    updates: Mutex<Vec<Update>>,
    arrived: Condvar,
}

impl Window for Recording {
    fn publish(&self, update: Update) {
        if let Update::Setup(steps) = &update {
            let shell = shell(steps);
            println!("setup: shell {:?}, needs {:?}", shell.state, shell.needs);
        }
        if let Update::Error(error) = &update {
            println!("error: {error}");
        }
        self.updates.lock().unwrap().push(update);
        self.arrived.notify_all();
    }
}

fn shell(steps: &[Step]) -> &Step {
    steps.iter().find(|s| s.id == "shell").unwrap()
}

impl Recording {
    /// Blocks until the shell step, in the updates from `from` on, is as
    /// `wanted` says; its state and what it needs.
    fn shell_until(
        &self,
        from: usize,
        within: Duration,
        wanted: impl Fn(&Step) -> bool,
    ) -> (StepState, Option<Needs>) {
        let updates = self.updates.lock().unwrap();
        let found = |u: &[Update]| {
            u[from.min(u.len())..].iter().find_map(|u| match u {
                Update::Setup(steps) if wanted(shell(steps)) => {
                    Some((shell(steps).state.clone(), shell(steps).needs))
                }
                _ => None,
            })
        };
        let (updates, timeout) = self
            .arrived
            .wait_timeout_while(updates, within, |u| found(u).is_none())
            .unwrap();
        assert!(!timeout.timed_out(), "the shell step never came to it");
        found(&updates).unwrap()
    }

    fn len(&self) -> usize {
        self.updates.lock().unwrap().len()
    }
}

fn run_once_value() -> Option<String> {
    let out = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\RunOnce",
            "/v",
            mcbench_sandbox::windows::RUN_ONCE_VALUE,
        ])
        .output()
        .unwrap();
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
#[ignore = "needs MCBENCH_WSL_SETUP, MCBENCH_WSL_SETUP_APP, the shell's files and a person's desktop session on Windows"]
fn the_setup_screen_installs_wsl_and_restarts_windows() {
    let step = std::env::var("MCBENCH_WSL_SETUP").expect("MCBENCH_WSL_SETUP names the step");
    let app = PathBuf::from(
        std::env::var_os("MCBENCH_WSL_SETUP_APP")
            .expect("MCBENCH_WSL_SETUP_APP names the mcbench-lite.exe RunOnce starts"),
    );
    let data = mcbench_setup::data_folder().unwrap();
    std::fs::create_dir_all(&data).unwrap();
    println!("data folder {}, boot {}", data.display(), {
        mcbench_sandbox::windows::boot_id().unwrap()
    });
    let core = Core::start(Parts {
        data,
        program: app,
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(|| Err("the WSL steps need no shipped data".into())),
        client: mcbench_setup::fetch::client(),
        backend: None,
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    let send = |command: Command| {
        let handle = core.handle().clone();
        let core = core.clone();
        handle
            .block_on(handle.spawn(async move { core.command(command).await }))
            .unwrap();
    };
    let minute = Duration::from_secs(60);
    match step.as_str() {
        "install" => {
            let before = window.shell_until(0, minute, |s| s.needs.is_some());
            assert_eq!(
                before,
                (
                    StepState::Unavailable {
                        error: wsl::NEEDS_WSL.into()
                    },
                    Some(Needs::InstallWsl)
                )
            );
            let from = window.len();
            let handle = core.handle().clone();
            let sending = {
                let core = core.clone();
                handle.spawn(async move { core.command(Command::InstallWsl).await })
            };
            let running = window.shell_until(from, minute, |_| true);
            assert_eq!(
                running,
                (
                    StepState::Running {
                        detail: Some(wsl::INSTALLING.into()),
                        progress: None
                    },
                    None
                )
            );
            handle.block_on(sending).unwrap();
            let after = window.shell_until(from, 20 * minute, |s| {
                !matches!(&s.state, StepState::Running { detail: Some(d), .. } if d == wsl::INSTALLING)
            });
            println!("after the install: {after:?}");
            match after {
                (StepState::Unavailable { error }, Some(Needs::Restart)) => {
                    assert_eq!(error, wsl::RESTART)
                }
                (StepState::Failed { error }, Some(Needs::InstallWsl)) => {
                    assert!(
                        error == wsl::DECLINED || error.starts_with("WSL could not be installed: "),
                        "{error}"
                    )
                }
                (StepState::Running { detail: None, .. }, None) => {
                    let done = window.shell_until(from, 20 * minute, |s| {
                        !matches!(s.state, StepState::Running { .. })
                    });
                    assert!(matches!(done, (StepState::Done { .. }, None)), "{done:?}");
                }
                other => panic!("the install ended in {other:?}"),
            }
        }
        "restart" => {
            window.shell_until(0, minute, |s| s.needs == Some(Needs::Restart));
            assert_eq!(run_once_value(), None, "a RunOnce value before the command");
            let from = window.len();
            send(Command::RestartComputer);
            let errors: Vec<String> = window.updates.lock().unwrap()[from..]
                .iter()
                .filter_map(|u| match u {
                    Update::Error(e) => Some(e.clone()),
                    _ => None,
                })
                .collect();
            if errors.is_empty() {
                println!(
                    "Windows is restarting; the RunOnce value: {:?}",
                    run_once_value()
                );
                std::thread::sleep(10 * minute);
                panic!("Windows did not restart within ten minutes");
            }
            assert_eq!(errors.len(), 1, "{errors:?}");
            assert!(
                errors[0].starts_with("Windows did not restart: ")
                    && errors[0].ends_with(
                        ". Restart it yourself; the app carries on when it is opened again."
                    ),
                "{}",
                errors[0]
            );
            assert_eq!(
                run_once_value(),
                None,
                "the RunOnce value outlived the refusal"
            );
        }
        "done" => {
            let done = window.shell_until(0, 20 * minute, |s| {
                !matches!(s.state, StepState::Running { .. } | StepState::Waiting)
            });
            assert!(matches!(done, (StepState::Done { .. }, None)), "{done:?}");
        }
        other => panic!("MCBENCH_WSL_SETUP is install, restart or done, not {other}"),
    }
    core.shutdown();
}
