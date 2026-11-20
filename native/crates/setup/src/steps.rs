//! The first start's steps and their states, for the setup screen
//! (`server/setup.js`): one step per thing, each marked by the work that
//! does it ([`Steps::track`]), so a later start, which finds everything in
//! place, marks every step done at once.

use std::future::Future;
use std::sync::Mutex;
use std::time::Instant;

use serde::{Deserialize, Serialize};

/// A download's bytes so far, of its total when the server said it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bytes {
    /// Received.
    pub done: u64,
    /// The whole, when known.
    pub total: Option<u64>,
}

/// A step's state (`server/setup.js`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum StepState {
    /// Its work has not started (it may wait for another step).
    Waiting,
    /// Its work runs.
    Running {
        /// What it does now.
        detail: Option<String>,
        /// How far its download is.
        progress: Option<Bytes>,
    },
    /// Its work is done.
    Done {
        /// How long it took, in seconds to a tenth.
        seconds: f64,
    },
    /// Its work failed: it is tried again when next needed.
    Failed {
        /// What failed.
        error: String,
    },
    /// It cannot be done on this machine as it is (only an optional step).
    Unavailable {
        /// What is missing.
        error: String,
    },
}

/// What the person must do before a step can go on (the setup screen shows
/// the one button it names: `Install WSL`, `Restart now`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Needs {
    /// WSL is not installed: `Command::InstallWsl` installs it.
    InstallWsl,
    /// WSL is installed and Windows must restart: `Command::RestartComputer`.
    Restart,
}

/// One step, for the setup screen (today's `STEPS` entries).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// Its id.
    pub id: String,
    /// What it makes ("Minecraft's textures").
    pub title: String,
    /// What its work does while it runs ("Downloading Minecraft's textures
    /// from Mojang").
    pub doing: String,
    /// Whether the app works without it (only `shell`).
    pub optional: bool,
    /// Where it is.
    pub state: StepState,
    /// What the person must do first. `Some` only for the shell step on
    /// Windows, when WSL must be installed or Windows restarted
    /// ([`StepError::Needs`], [`Steps::set`]); `None` on every other
    /// platform.
    #[serde(default)]
    pub needs: Option<Needs>,
}

/// How a step's work failed.
#[derive(Clone, Debug, PartialEq)]
pub enum StepError {
    /// It failed; it runs again when next needed.
    Failed(String),
    /// The machine lacks what it needs (an optional step only).
    Unavailable(String),
    /// The person must do what `needs` names first: the step ends failed
    /// (`failed`) or unavailable with `sentence`, and its `needs` is set.
    Needs {
        /// The one button the setup screen shows.
        needs: Needs,
        /// Failed (the person may try again) or unavailable.
        failed: bool,
        /// The step's text.
        sentence: String,
    },
}

impl From<String> for StepError {
    fn from(error: String) -> StepError {
        StepError::Failed(error)
    }
}

/// The native app's steps in order: the shipped data unpacked, the
/// textures fetched from Mojang and baked, and the model's shell (the
/// `bash` root filesystem, downloaded in the background; on Windows its
/// work starts with the WSL check, and it is unavailable without WSL).
pub fn native_steps() -> Vec<Step> {
    [
        (
            "data",
            "Minecraft's data",
            "Unpacking Minecraft's data",
            false,
        ),
        (
            "textures",
            "Minecraft's textures",
            "Downloading Minecraft's textures from Mojang",
            false,
        ),
        (
            "shell",
            "Model's shell",
            "Downloading the model's shell",
            true,
        ),
    ]
    .into_iter()
    .map(|(id, title, doing, optional)| Step {
        id: id.into(),
        title: title.into(),
        doing: doing.into(),
        optional,
        state: StepState::Waiting,
        needs: None,
    })
    .collect()
}

/// The steps of one start, which call `notify` with the whole list each
/// time one changes.
pub struct Steps {
    steps: Mutex<Vec<Step>>,
    notify: Notify,
}

/// What [`Steps`] calls with the whole list each time a step changes.
pub type Notify = Box<dyn Fn(&[Step]) + Send + Sync>;

impl Steps {
    /// `list`, every one waiting.
    pub fn new(list: Vec<Step>, notify: Notify) -> Steps {
        Steps {
            steps: Mutex::new(list),
            notify,
        }
    }

    /// Every step in order, and whether the app is ready: every step that
    /// is not optional is done.
    pub fn view(&self) -> (bool, Vec<Step>) {
        let list = self.steps.lock().expect("the steps' lock").clone();
        let ready = list
            .iter()
            .all(|s| s.optional || matches!(s.state, StepState::Done { .. }));
        (ready, list)
    }

    fn change(&self, id: &str, change: impl FnOnce(&mut Step)) -> Result<(), String> {
        let list = {
            let mut list = self.steps.lock().expect("the steps' lock");
            let step = list
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| format!("No setup step {id}"))?;
            change(step);
            list.clone()
        };
        (self.notify)(&list);
        Ok(())
    }

    fn state(&self, id: &str) -> Result<StepState, String> {
        let list = self.steps.lock().expect("the steps' lock");
        Ok(list
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("No setup step {id}"))?
            .state
            .clone())
    }

    /// Runs `work` as step `id`: running while it runs, then done with how
    /// long it took, or failed (unavailable when `work` says the machine
    /// lacks something) with the last 2,000 characters of its error,
    /// trimmed; [`StepError::Needs`] also sets the step's `needs`, which is
    /// `None` while it runs and once it is done. A step that is done stays
    /// done: its work runs and changes nothing here. An unknown step is a
    /// mistake in the code.
    pub async fn track<T>(
        &self,
        id: &str,
        work: impl Future<Output = Result<T, StepError>>,
    ) -> Result<T, String> {
        if matches!(self.state(id)?, StepState::Done { .. }) {
            return work.await.map_err(|e| match e {
                StepError::Failed(m) | StepError::Unavailable(m) => m,
                StepError::Needs { sentence, .. } => sentence,
            });
        }
        let started = Instant::now();
        self.change(id, |s| {
            s.state = StepState::Running {
                detail: None,
                progress: None,
            };
            s.needs = None;
        })?;
        match work.await {
            Ok(value) => {
                let seconds = (started.elapsed().as_millis() as f64 / 100.0).round() / 10.0;
                self.change(id, |s| {
                    s.state = StepState::Done { seconds };
                    s.needs = None;
                })?;
                Ok(value)
            }
            Err(error) => {
                let (unavailable, message, needs) = match error {
                    StepError::Failed(m) => (false, m, None),
                    StepError::Unavailable(m) => (true, m, None),
                    StepError::Needs {
                        needs,
                        failed,
                        sentence,
                    } => (!failed, sentence, Some(needs)),
                };
                let trimmed = message.trim();
                let chars: Vec<char> = trimmed.chars().collect();
                let error: String = chars[chars.len().saturating_sub(2000)..].iter().collect();
                self.change(id, |s| {
                    s.state = if unavailable {
                        StepState::Unavailable {
                            error: error.clone(),
                        }
                    } else {
                        StepState::Failed {
                            error: error.clone(),
                        }
                    };
                    s.needs = needs;
                })?;
                Err(message)
            }
        }
    }

    /// Sets step `id`'s state and what it needs at once, outside its work
    /// (the WSL installer's states while `Command::InstallWsl` runs).
    pub fn set(&self, id: &str, state: StepState, needs: Option<Needs>) -> Result<(), String> {
        self.change(id, |s| {
            s.state = state;
            s.needs = needs;
        })
    }

    /// Sets step `id`'s state and what it needs only while it needs
    /// `expected`, in one step under the steps' lock (two presses of one
    /// button start one action); whether it did.
    pub fn set_if_needs(
        &self,
        id: &str,
        expected: Needs,
        state: StepState,
        needs: Option<Needs>,
    ) -> Result<bool, String> {
        let list = {
            let mut list = self.steps.lock().expect("the steps' lock");
            let step = list
                .iter_mut()
                .find(|s| s.id == id)
                .ok_or_else(|| format!("No setup step {id}"))?;
            if step.needs != Some(expected) {
                return Ok(false);
            }
            step.state = state;
            step.needs = needs;
            list.clone()
        };
        (self.notify)(&list);
        Ok(true)
    }

    /// What step `id` needs the person to do now.
    pub fn needs(&self, id: &str) -> Result<Option<Needs>, String> {
        let list = self.steps.lock().expect("the steps' lock");
        Ok(list
            .iter()
            .find(|s| s.id == id)
            .ok_or_else(|| format!("No setup step {id}"))?
            .needs)
    }

    /// Says what running step `id` does now.
    pub fn detail(&self, id: &str, detail: &str) -> Result<(), String> {
        self.change(id, |s| {
            if let StepState::Running { detail: d, .. } = &mut s.state {
                *d = Some(detail.to_string());
            }
        })
    }

    /// Tells how far running step `id`'s download is.
    pub fn progress(&self, id: &str, bytes: Bytes) -> Result<(), String> {
        self.change(id, |s| {
            if let StepState::Running { progress, .. } = &mut s.state {
                *progress = Some(bytes);
            }
        })
    }

    /// Whether a retry may start (`POST /api/setup/retry`): only after a
    /// step failed.
    pub fn retry(&self) -> Result<(), String> {
        let (_, list) = self.view();
        if list
            .iter()
            .any(|s| matches!(s.state, StepState::Failed { .. }))
        {
            Ok(())
        } else {
            Err("No setup step failed".into())
        }
    }
}
