//! Signing in to a subscription and installing a CLI (`server/index.js`:
//! `POST /api/provider-logins`, `GET`, `DELETE` and `POST .../input` of
//! `/api/provider-logins/:provider`, `POST /api/provider-logouts`,
//! `POST /api/cli-installs`, `GET /api/cli-installs/:program`), over
//! `mcbench-providers`' `SignIns` and `CliInstaller`.
//!
//! Nothing polls: the core watches both (`mcbench_providers::changes`) and
//! publishes `Update::SignIn` for each change of a sign-in's view (for every
//! provider of its login: ChatGPT and the Codex CLI share Codex's) and
//! `Update::Install` for each change of an install (its progress at most
//! ten times a second); once either has ended, `Update::Providers` and the
//! settings read fresh (`Update::Settings`) follow.

use mcbench_agent::lifecycle::Refusal;
use mcbench_providers::ProviderId;
use mcbench_providers::harness::install::InstallView;
use mcbench_providers::login::{Login, LoginError, SignInView, State};
use mcbench_store::json::Json;
use mcbench_ui::Update;

use crate::core::{Core, refusal};

/// The providers that sign in with `login`, in `PROVIDERS`' order.
fn providers_of(login: Login) -> Vec<&'static str> {
    mcbench_store::PROVIDERS
        .iter()
        .filter(|info| provider(info.id).is_ok_and(|id| Login::of(id) == Ok(login)))
        .map(|info| info.id)
        .collect()
}

fn login_refusal(error: LoginError) -> Refusal {
    refusal(error.status, error.message)
}

/// The provider a sign-in route names (`loginOf`'s refusal for any other).
fn provider(key: &str) -> Result<ProviderId, Refusal> {
    serde_json::from_value(Json::String(key.into()))
        .map_err(|_| refusal(400, format!("{key} has no sign-in")))
}

/// An install as the page reads it (`view`).
pub fn install_json(view: &InstallView) -> Json {
    mcbench_store::object! {
        "state" => view.state.as_str(),
        "progress" => view.progress.map(|p| mcbench_store::object! { "done" => p.done, "total" => p.total }),
        "error" => view.error.clone(),
    }
}

impl Core {
    /// `POST /api/provider-logins` with `{ provider, method }`.
    pub fn sign_in(&self, key: &str, method: Option<&str>) -> Result<SignInView, Refusal> {
        let _entered = self.0.handle.enter();
        self.0
            .sign_ins
            .start(provider(key)?, method)
            .map_err(login_refusal)
    }

    /// `GET /api/provider-logins/:provider`: the latest sign-in, or none.
    pub fn sign_in_state(&self, key: &str) -> Result<Option<SignInView>, Refusal> {
        self.0.sign_ins.state(provider(key)?).map_err(login_refusal)
    }

    /// `DELETE /api/provider-logins/:provider`.
    pub fn cancel_sign_in(&self, key: &str) -> Result<SignInView, Refusal> {
        self.0
            .sign_ins
            .cancel(provider(key)?)
            .map_err(login_refusal)
    }

    /// `POST /api/provider-logins/:provider/input` with `{ text }`: the
    /// pasted code goes to the CLI and nowhere else.
    pub async fn sign_in_input(&self, key: &str, text: &str) -> Result<SignInView, Refusal> {
        self.0
            .sign_ins
            .input(provider(key)?, text)
            .await
            .map_err(login_refusal)
    }

    /// `POST /api/provider-logouts` with `{ provider }`.
    pub async fn sign_out(&self, key: &str) -> Result<(), Refusal> {
        self.0
            .sign_ins
            .logout(provider(key)?)
            .await
            .map_err(login_refusal)
    }

    /// `POST /api/cli-installs` with `{ program }` (`claude` or `codex`).
    pub fn install_cli(&self, program: &str) -> Result<InstallView, Refusal> {
        let _entered = self.0.handle.enter();
        self.0
            .installs
            .start(program)
            .map_err(|e| refusal(e.status, e.message))
    }

    /// `GET /api/cli-installs/:program`: the latest install, or none.
    pub fn cli_install(&self, program: &str) -> Result<Option<InstallView>, Refusal> {
        self.0
            .installs
            .state(program)
            .map_err(|e| refusal(e.status, e.message))
    }
}

impl Core {
    /// Watches the sign-ins and the installs: each change is published
    /// (see the module's doc). Called once, as the core starts.
    pub(crate) fn watch_sign_ins_and_installs(&self) {
        let core = std::sync::Arc::downgrade(&self.0);
        self.0.sign_ins.watch(std::sync::Arc::new(
            move |(login, view): (Login, SignInView)| {
                if let Some(inner) = core.upgrade() {
                    Core(inner).sign_in_changed(login, view);
                }
            },
        ));
        let core = std::sync::Arc::downgrade(&self.0);
        self.0.installs.watch(std::sync::Arc::new(
            move |(program, view): (String, InstallView)| {
                if let Some(inner) = core.upgrade() {
                    Core(inner).install_changed(program, view);
                }
            },
        ));
    }

    /// A sign-in's view changed (called with its lock held: it only
    /// publishes, and reads the settings in a task of its own).
    fn sign_in_changed(&self, login: Login, view: SignInView) {
        *self
            .0
            .sign_ins_told
            .lock()
            .expect("the sign-ins told")
            .entry(login)
            .or_default() += 1;
        let ended = !matches!(view.state, State::Starting | State::Waiting);
        for provider in providers_of(login) {
            self.publish(Update::SignIn {
                provider: provider.to_string(),
                state: Some(view.clone()),
            });
        }
        if ended {
            self.settings_after_end();
        }
    }

    /// An install's view changed (called with its lock held, as above).
    fn install_changed(&self, program: String, view: InstallView) {
        *self
            .0
            .installs_told
            .lock()
            .expect("the installs told")
            .entry(program.clone())
            .or_default() += 1;
        let ended = view.state != "running";
        self.publish(Update::Install {
            program,
            state: view,
        });
        if ended {
            self.settings_after_end();
        }
    }

    /// After a sign-in or an install ended: the providers, then the
    /// settings read fresh (logins, keys and versions read again). Not
    /// while the core stops (its sign-ins are cancelled then).
    fn settings_after_end(&self) {
        if self.stopped() {
            return;
        }
        let core = self.clone();
        self.0.handle.spawn(async move {
            core.publish_providers().await;
            core.publish_fresh_settings().await;
        });
    }

    /// `Update::Settings` read fresh (`?refresh=1`).
    pub(crate) async fn publish_fresh_settings(&self) {
        let view = self.settings_view(true).await.map_err(|e| e.message);
        self.publish(Update::Settings(view));
    }

    /// How many changes of `key`'s login were published; none for a
    /// provider with no sign-in.
    fn sign_in_told(&self, key: &str) -> Option<u64> {
        let login = Login::of(provider(key).ok()?).ok()?;
        let told = self.0.sign_ins_told.lock().expect("the sign-ins told");
        Some(told.get(&login).copied().unwrap_or(0))
    }

    /// How many changes of `program`'s install were published.
    fn install_told(&self, program: &str) -> u64 {
        let told = self.0.installs_told.lock().expect("the installs told");
        told.get(program).copied().unwrap_or(0)
    }

    /// The end of `SignIn`, `SignInInput` and `CancelSignIn` on `key`,
    /// whose call `done` began when `before` changes were published: on a
    /// refusal `Update::Error`, then `Update::SignIn` with the state as it
    /// is; on success `Update::SignIn` unless the call's change was
    /// published already (a start, a cancel), so no change is published
    /// twice.
    pub(crate) fn end_in_sign_in(&self, key: &str, before: Option<u64>, done: Result<(), Refusal>) {
        let refused = done.is_err();
        if let Err(error) = done {
            self.publish(Update::Error(error.message));
        }
        if refused || self.sign_in_told(key) == before {
            self.publish(Update::SignIn {
                provider: key.to_string(),
                state: self.sign_in_state(key).ok().flatten(),
            });
        }
    }

    /// `Command::SignIn`, `Command::SignInInput`, `Command::CancelSignIn`.
    pub(crate) async fn sign_in_command(&self, key: &str, run: SignInCall<'_>) {
        let before = self.sign_in_told(key);
        let done = match run {
            SignInCall::Start(method) => self.sign_in(key, method).map(drop),
            SignInCall::Input(text) => self.sign_in_input(key, text).await.map(drop),
            SignInCall::Cancel => self.cancel_sign_in(key).map(drop),
        };
        self.end_in_sign_in(key, before, done);
    }

    /// `Command::InstallCli`: `Update::Install` with the install started
    /// (published by its watcher) or the one under way; on a refusal
    /// `Update::Error`, the latest install when there is one, then
    /// `Update::Settings` read fresh, whose `sign_in.installed` and
    /// `install` are the state the command was about (a window whose view
    /// was stale, the CLI being installed already, is brought up to date).
    pub(crate) async fn install_command(&self, program: &str) {
        let before = self.install_told(program);
        match self.install_cli(program) {
            Ok(view) => {
                if self.install_told(program) == before {
                    self.publish(Update::Install {
                        program: program.to_string(),
                        state: view,
                    });
                }
            }
            Err(error) => {
                self.publish(Update::Error(error.message));
                if let Ok(Some(state)) = self.cli_install(program) {
                    self.publish(Update::Install {
                        program: program.to_string(),
                        state,
                    });
                }
                self.publish_fresh_settings().await;
            }
        }
    }
}

/// What a sign-in command does.
pub(crate) enum SignInCall<'a> {
    /// Starts one with this method.
    Start(Option<&'a str>),
    /// Hands the CLI the pasted code.
    Input(&'a str),
    /// Cancels it.
    Cancel,
}
