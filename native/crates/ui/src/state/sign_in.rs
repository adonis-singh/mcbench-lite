//! A provider's sign-in rows (`SignInRows`, `web/app.js:934-1079`;
//! inventory `components.md` 1.7 and E.7): what the Account section shows
//! of a sign-in and of its CLI's install, in the order the page checks
//! them, and what the one button does. The core pushes each sign-in's and
//! install's changes (`Update::SignIn`, `Update::Install`); nothing polls.

use super::settings::{
    CliAccount, cli_account, login_through, sign_in_help, sign_in_method, sign_in_next,
};
use super::setup::{download_size, megabytes};

/// A CLI's download as a sentence's clause: `a 236 MB download`, or none
/// where no download is pinned (`Program.bytes` 0: Windows today), so no
/// text ever says `0 MB`.
pub fn download_clause(bytes: u64) -> Option<String> {
    (bytes > 0).then(|| format!("a {} download", download_size(bytes)))
}
use crate::{Command, InstallView, ProviderSettings, SignInState, SignInView};

/// What follows an install, remembered from the press until Settings read
/// again find the CLI installed (`after`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    /// The sign-in, with this method.
    SignIn(Option<String>),
    /// Nothing: a login whose CLI alone was missing.
    Nothing,
}

/// A provider's sign-in as the window knows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Login {
    /// The latest sign-in.
    pub sign_in: Option<SignInView>,
    /// The latest install of its CLI.
    pub install: Option<InstallView>,
    /// What follows the install under way.
    pub after: Option<After>,
}

/// A sign-in button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Start {
    /// Its text.
    pub label: String,
    /// The method it starts.
    pub method: Option<String>,
    /// Drawn gold (`btn gold`).
    pub primary: bool,
}

/// The rows, in the page's order of checks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rows {
    /// `Installing <program>`: working dots, no button.
    Installing {
        /// `Installing Claude Code 2.1.311`.
        title: String,
        /// How far it is, and what follows.
        help: String,
    },
    /// `Starting the sign-in` with `Cancel`.
    Starting,
    /// `Open the sign-in page`: the address with `Copy address` and
    /// `Cancel`; the one-time code; the pasted code's form.
    Waiting {
        /// `SIGN_IN_NEXT`.
        next: &'static str,
        /// The address.
        url: String,
        /// Codex's one-time code.
        user_code: Option<String>,
        /// The form's help, when a pasted code is asked for.
        wants_input: Option<String>,
    },
    /// A login whose CLI is missing: `Install <program>`.
    LoginOnly {
        /// The account, or `Signed in`.
        title: String,
        /// Its plan, what it is, and the download.
        help: String,
        /// Why the last install or sign-in failed.
        failure: Option<String>,
        /// `Install <program>`.
        button: String,
    },
    /// `Not signed in`, with a button per method.
    NotSignedIn {
        /// `SIGN_IN_HELP`, and the install that comes first.
        help: String,
        /// Why the last install or sign-in failed.
        failure: Option<String>,
        /// The buttons.
        starts: Vec<Start>,
    },
}

/// The provider's sign-in (`signIn`), which it must have.
fn offer(provider: &ProviderSettings) -> &crate::SignInOffer {
    provider
        .sign_in
        .as_ref()
        .unwrap_or_else(|| panic!("{} has no sign-in", provider.id))
}

/// A login present with its CLI missing (`loginOnly`).
fn login_only(provider: &ProviderSettings) -> bool {
    provider.env_key.is_none() && cli_account(provider) == CliAccount::Install
}

/// The rows for `provider` (`SignInRows`' render).
pub fn rows(provider: &ProviderSettings, login: &Login) -> Rows {
    let offer = offer(provider);
    let program = &offer.program;
    let install_state = login.install.as_ref().map(|i| i.state.as_str());
    let sign_in_state = login.sign_in.as_ref().map(|s| s.state);
    if install_state == Some("running") || (login.after.is_some() && install_state == Some("done"))
    {
        let install = login.install.as_ref().expect("an install");
        let how = if install.state == "done" {
            "Installed.".to_string()
        } else {
            match install.progress {
                Some(p) => format!("Downloading, {}.", megabytes(p.done, p.total)),
                None => "Starting the download.".into(),
            }
        };
        let follows = if matches!(login.after, Some(After::SignIn(_))) {
            " The sign-in starts when it is done."
        } else {
            ""
        };
        return Rows::Installing {
            title: format!("Installing {}", program.title),
            help: format!("{how}{follows}"),
        };
    }
    if sign_in_state == Some(SignInState::Starting) {
        return Rows::Starting;
    }
    if sign_in_state == Some(SignInState::Waiting) {
        let view = login.sign_in.as_ref().expect("a sign-in");
        return Rows::Waiting {
            next: sign_in_next(&provider.id),
            url: view
                .url
                .clone()
                .unwrap_or_else(|| panic!("a waiting sign-in of {} has no address", provider.id)),
            user_code: view.user_code.clone(),
            wants_input: view.wants_input.then(|| {
                view.message
                    .clone()
                    .unwrap_or_else(|| "Paste the code the page shows after you sign in.".into())
            }),
        };
    }
    let failure = if install_state == Some("failed") {
        Some(format!(
            "{} was not installed: {}",
            program.title,
            login
                .install
                .as_ref()
                .and_then(|i| i.error.clone())
                .unwrap_or_default()
        ))
    } else if sign_in_state == Some(SignInState::Failed) {
        login.sign_in.as_ref().and_then(|s| s.error.clone())
    } else {
        None
    };
    if login_only(provider) {
        return Rows::LoginOnly {
            title: provider
                .account
                .clone()
                .unwrap_or_else(|| "Signed in".into()),
            help: format!(
                "{}. {} is not installed: the app installs it{} to use this login.",
                super::settings::account_help(provider),
                program.title,
                download_clause(program.bytes)
                    .map(|c| format!(", {c},"))
                    .unwrap_or_default()
            ),
            failure,
            button: format!("Install {}", program.title),
        };
    }
    let mut help = sign_in_help(&provider.id).to_string();
    if !offer.installed {
        help.push_str(&format!(
            " The app installs {} first{}.",
            program.title,
            download_clause(program.bytes)
                .map(|c| format!(", {c}"))
                .unwrap_or_default()
        ));
    }
    let starts = if offer.methods.is_empty() {
        vec![Start {
            label: "Sign in".into(),
            method: None,
            primary: true,
        }]
    } else {
        offer
            .methods
            .iter()
            .enumerate()
            .map(|(i, method)| Start {
                label: sign_in_method(method).into(),
                method: Some(method.clone()),
                primary: i == 0,
            })
            .collect()
    };
    Rows::NotSignedIn {
        help,
        failure,
        starts,
    }
}

/// The one button (`begin`): the sign-in when the CLI is installed; else
/// the install, remembering what follows it.
pub fn begin(provider: &ProviderSettings, login: &mut Login, method: Option<String>) -> Command {
    let offer = offer(provider);
    if offer.installed {
        return Command::SignIn {
            provider: provider.id.clone(),
            method,
        };
    }
    login.after = Some(if login_only(provider) {
        After::Nothing
    } else {
        After::SignIn(method)
    });
    Command::InstallCli {
        program: offer.program.name.clone(),
    }
}

/// Settings were read again: when they find the CLI installed, what the
/// install remembered follows (the sign-in, or nothing), and the memory
/// goes.
///
/// When the CLI is not installed and no install of it is running (a refused
/// install, which ends in `Update::Error` and fresh settings, with no
/// `Update::Install`), what was to follow is forgotten: the row goes back to
/// its button.
pub fn installed(provider: &ProviderSettings, login: &mut Login) -> Option<Command> {
    if !offer(provider).installed {
        let running = login
            .install
            .as_ref()
            .is_some_and(|i| i.state == "running" || i.state == "done");
        if !running {
            login.after = None;
        }
        return None;
    }
    match login.after.take()? {
        After::SignIn(method) => Some(Command::SignIn {
            provider: provider.id.clone(),
            method,
        }),
        After::Nothing => None,
    }
}

/// An install's update: a failed one forgets what was to follow.
pub fn install_changed(login: &mut Login, state: InstallView) {
    if state.state == "failed" {
        login.after = None;
    }
    login.install = Some(state);
}

/// Who the core's sign-in of `provider` also serves (ChatGPT and Codex CLI
/// share one login).
pub fn shares_login(a: &str, b: &str) -> bool {
    a == b || matches!((a, b), ("codex", "codex-cli") | ("codex-cli", "codex"))
}

/// What the signed-in row shows (`<account>` or `Signed in`, and its help).
pub fn signed_in_row(provider: &ProviderSettings) -> (String, String) {
    (
        provider
            .account
            .clone()
            .unwrap_or_else(|| "Signed in".into()),
        super::settings::account_help(provider),
    )
}

/// A CLI login is signed in, its CLI there (`cliAccount` = connected).
pub fn is_signed_in(provider: &ProviderSettings) -> bool {
    cli_account(provider) == CliAccount::Connected
}

/// `LOGIN_THROUGH`, for the views.
pub fn through(id: &str) -> &'static str {
    login_through(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Auth, Program, SignInOffer};

    fn claude(installed: bool) -> ProviderSettings {
        let mut p = super::super::settings::tests::entry("claude-code", false, true);
        p.auth = Auth::CliLogin;
        p.harness = true;
        p.sign_in = Some(SignInOffer {
            methods: vec!["claudeai".into(), "console".into()],
            program: Program {
                name: "claude".into(),
                title: "Claude Code 2.1.311".into(),
                bytes: 236_000_000,
            },
            installed,
            current: None,
            install: None,
        });
        p
    }

    fn view(state: SignInState) -> SignInView {
        SignInView {
            state,
            url: None,
            user_code: None,
            wants_input: false,
            message: None,
            error: None,
        }
    }

    #[test]
    fn a_sign_in_without_its_cli_installs_first_then_signs_in() {
        let mut p = claude(false);
        let mut login = Login::default();
        match rows(&p, &login) {
            Rows::NotSignedIn {
                help,
                starts,
                failure,
            } => {
                assert!(help.ends_with(
                    "The sign-in page opens in your browser. The app installs Claude Code 2.1.311 first, a 236 MB download."
                ));
                assert_eq!(failure, None);
                assert_eq!(starts.len(), 2);
                assert_eq!(starts[0].label, "Claude subscription");
                assert!(starts[0].primary && !starts[1].primary);
            }
            other => panic!("{other:?}"),
        }
        let command = begin(&p, &mut login, Some("claudeai".into()));
        assert!(matches!(command, Command::InstallCli { program } if program == "claude"));
        install_changed(
            &mut login,
            InstallView {
                state: "running".into(),
                progress: None,
                error: None,
            },
        );
        assert_eq!(
            rows(&p, &login),
            Rows::Installing {
                title: "Installing Claude Code 2.1.311".into(),
                help: "Starting the download. The sign-in starts when it is done.".into()
            }
        );
        install_changed(
            &mut login,
            InstallView {
                state: "running".into(),
                progress: Some(crate::Progress {
                    done: 84_200_000,
                    total: Some(236_000_000),
                }),
                error: None,
            },
        );
        assert!(
            matches!(rows(&p, &login), Rows::Installing { help, .. } if help == "Downloading, 84 of 236 MB. The sign-in starts when it is done.")
        );
        install_changed(
            &mut login,
            InstallView {
                state: "done".into(),
                progress: None,
                error: None,
            },
        );
        // Installed, until Settings read again find it so.
        assert!(
            matches!(rows(&p, &login), Rows::Installing { help, .. } if help.starts_with("Installed."))
        );
        assert!(installed(&p, &mut login).is_none());
        p.sign_in.as_mut().unwrap().installed = true;
        assert!(matches!(
            installed(&p, &mut login),
            Some(Command::SignIn { method: Some(m), .. }) if m == "claudeai"
        ));
        assert_eq!(login.after, None);
        login.sign_in = Some(view(SignInState::Starting));
        assert_eq!(rows(&p, &login), Rows::Starting);
        let mut waiting = view(SignInState::Waiting);
        waiting.url = Some("https://claude.ai/oauth".into());
        waiting.wants_input = true;
        login.sign_in = Some(waiting);
        assert!(matches!(
            rows(&p, &login),
            Rows::Waiting { wants_input: Some(help), user_code: None, .. }
                if help == "Paste the code the page shows after you sign in."
        ));
    }

    #[test]
    fn a_failed_install_or_sign_in_says_why_on_the_not_signed_in_row() {
        let p = claude(false);
        let mut login = Login::default();
        let _ = begin(&p, &mut login, None);
        install_changed(
            &mut login,
            InstallView {
                state: "failed".into(),
                progress: None,
                error: Some("the download's checksum did not match".into()),
            },
        );
        assert_eq!(login.after, None);
        assert!(matches!(
            rows(&p, &login),
            Rows::NotSignedIn { failure: Some(f), .. }
                if f == "Claude Code 2.1.311 was not installed: the download's checksum did not match"
        ));
        let mut login = Login {
            sign_in: Some(SignInView {
                error: Some("The sign-in was not finished within 10 minutes, so it was ended. Start it again when you are ready.".into()),
                ..view(SignInState::Failed)
            }),
            ..Login::default()
        };
        assert!(
            matches!(rows(&p, &login), Rows::NotSignedIn { failure: Some(f), .. } if f.contains("10 minutes"))
        );
        // Cancelled and done fall through with no failure.
        login.sign_in = Some(view(SignInState::Cancelled));
        assert!(matches!(
            rows(&p, &login),
            Rows::NotSignedIn { failure: None, .. }
        ));
    }

    #[test]
    fn a_login_whose_cli_is_missing_offers_the_install_alone() {
        let mut p = claude(false);
        p.signed_in = true;
        p.account = Some("ada@example.com".into());
        p.plan = Some("Max".into());
        let mut login = Login::default();
        assert_eq!(
            rows(&p, &login),
            Rows::LoginOnly {
                title: "ada@example.com".into(),
                help: "Max \u{b7} Claude login through Claude Code. Claude Code 2.1.311 is not installed: the app installs it, a 236 MB download, to use this login.".into(),
                failure: None,
                button: "Install Claude Code 2.1.311".into(),
            }
        );
        let _ = begin(&p, &mut login, None);
        assert_eq!(login.after, Some(After::Nothing));
        p.sign_in.as_mut().unwrap().installed = true;
        assert!(installed(&p, &mut login).is_none());
        assert_eq!(login.after, None);
    }

    #[test]
    fn no_text_says_a_download_of_0_mb() {
        assert_eq!(
            download_clause(236_418_112).as_deref(),
            Some("a 236 MB download")
        );
        assert_eq!(download_clause(0), None);
        let mut p = claude(false);
        p.sign_in.as_mut().unwrap().program.bytes = 0;
        match rows(&p, &Login::default()) {
            Rows::NotSignedIn { help, .. } => {
                assert!(
                    help.ends_with(" The app installs Claude Code 2.1.311 first."),
                    "{help}"
                );
                assert!(!help.contains("MB"));
            }
            other => panic!("{other:?}"),
        }
        p.signed_in = true;
        match rows(&p, &Login::default()) {
            Rows::LoginOnly { help, .. } => {
                assert!(help.ends_with("Claude Code 2.1.311 is not installed: the app installs it to use this login."), "{help}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_refused_install_with_no_install_running_forgets_the_sign_in_on_the_next_settings() {
        let p = claude(false);
        let mut login = Login::default();
        let _ = begin(&p, &mut login, Some("claudeai".into()));
        assert!(login.after.is_some());
        // The core refused: an error, then fresh settings, no install.
        assert!(installed(&p, &mut login).is_none());
        assert_eq!(login.after, None);
        assert!(matches!(rows(&p, &login), Rows::NotSignedIn { .. }));
        // While an install runs, the follow-up is kept.
        let mut login = Login::default();
        let _ = begin(&p, &mut login, Some("claudeai".into()));
        install_changed(
            &mut login,
            InstallView {
                state: "running".into(),
                progress: None,
                error: None,
            },
        );
        assert!(installed(&p, &mut login).is_none());
        assert!(login.after.is_some());
    }
}
