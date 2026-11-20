//! What the composer says of the first start (`web/setup-status.mjs`
//! `setupStatus`, `heldLine`): of the steps the app needs (optional ones are
//! not counted) how many are done and whether one failed, and the line over
//! the composer for a message sent before the app is ready (a held send).

use mcbench_setup::{Step, StepState};

/// The steps the app needs, counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SetupStatus {
    /// Every needed step is done.
    pub ready: bool,
    /// The needed steps done.
    pub done: usize,
    /// The needed steps.
    pub total: usize,
    /// A needed step failed.
    pub failed: bool,
}

/// The count of `steps` (`setupStatus`): an optional step that cannot be
/// done is no failure.
pub fn setup_status(steps: &[Step]) -> SetupStatus {
    let needed: Vec<&Step> = steps.iter().filter(|s| !s.optional).collect();
    let done = needed
        .iter()
        .filter(|s| matches!(s.state, StepState::Done { .. }))
        .count();
    SetupStatus {
        ready: done == needed.len(),
        done,
        total: needed.len(),
        failed: needed
            .iter()
            .any(|s| matches!(s.state, StepState::Failed { .. })),
    }
}

/// The held send's line (`heldLine`). It names where to retry only when
/// the steps (with their Try again) are on screen above it: today's page
/// said `Try it again above.` also in a chat with messages, where nothing
/// is above it (a defect, inventory README).
pub fn held_line(status: &SetupStatus, steps_shown: bool) -> String {
    if status.failed {
        return if steps_shown {
            "Not sent yet: a step failed. Try it again above.".to_string()
        } else {
            "Not sent yet: a step failed.".to_string()
        };
    }
    format!(
        "Sends when the app is ready. {} of {} done.",
        status.done, status.total
    )
}

// settings agent: the setup screen's rows, sizes and links.

/// Bytes in MB, as the terminal states a download's size (`mb`: bytes over
/// 10^6, rounded).
fn mb(bytes: u64) -> u64 {
    (bytes as f64 / 1e6).round() as u64
}

/// A download's size: `172 MB` (`downloadSize`).
pub fn download_size(bytes: u64) -> String {
    format!("{} MB", mb(bytes))
}

/// A download's progress: `84 of 136 MB`, or `84 MB` without a total
/// (`megabytes`; a total of 0 counts as none, as the page's falsy test).
pub fn megabytes(done: u64, total: Option<u64>) -> String {
    match total.filter(|t| *t > 0) {
        Some(total) => format!("{} of {} MB", mb(done), mb(total)),
        None => format!("{} MB", mb(done)),
    }
}

/// A step's state's name (`waiting`, `running`, `done`, `failed`,
/// `unavailable`).
pub fn state_name(state: &StepState) -> &'static str {
    match state {
        StepState::Waiting => "waiting",
        StepState::Running { .. } => "running",
        StepState::Done { .. } => "done",
        StepState::Failed { .. } => "failed",
        StepState::Unavailable { .. } => "unavailable",
    }
}

/// One step as a row (`stepRow`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StepRow {
    /// The step's id.
    pub id: String,
    /// Its state's name.
    pub state: &'static str,
    /// The app works without it.
    pub optional: bool,
    /// What it makes, or what its work does while it runs.
    pub label: String,
    /// The short word at its end: the download's MB while one runs.
    pub value: String,
    /// The line under it: what a running step does now, or why a failed or
    /// unavailable one is not done.
    pub note: Option<String>,
}

/// `stepRow`. The seam's step has only the five states, so the page's
/// `A setup step cannot be <state>` cannot happen here.
pub fn step_row(step: &Step) -> StepRow {
    let (value, note) = match &step.state {
        StepState::Waiting => ("Waiting".to_string(), None),
        StepState::Running { detail, progress } => (
            progress
                .map(|p| megabytes(p.done, p.total))
                .unwrap_or_default(),
            detail.clone().filter(|d| !d.is_empty()),
        ),
        StepState::Done { .. } => ("Done".to_string(), None),
        StepState::Failed { error } => ("Failed".to_string(), Some(error.clone())),
        StepState::Unavailable { error } => ("Not available".to_string(), Some(error.clone())),
    };
    let running = matches!(step.state, StepState::Running { .. });
    StepRow {
        id: step.id.clone(),
        state: state_name(&step.state),
        optional: step.optional,
        label: if running {
            step.doing.clone()
        } else {
            step.title.clone()
        },
        value,
        note: note.filter(|n| !n.is_empty()),
    }
}

/// A part of a note: text, or a web address.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// Text.
    Text(String),
    /// A web address.
    Url(String),
}

/// Text with its web addresses as links, in order (`linkParts`): an
/// address ends at a space or a closing bracket, and a full stop or comma
/// after it is the sentence's.
pub fn link_parts(text: &str) -> Vec<Part> {
    static ADDRESS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let address = ADDRESS.get_or_init(|| {
        regex::Regex::new(r"https?://[^\s)]+[^\s).,]").expect("the address pattern")
    });
    let mut parts = Vec::new();
    let mut at = 0;
    for m in address.find_iter(text) {
        if m.start() > at {
            parts.push(Part::Text(text[at..m.start()].to_string()));
        }
        parts.push(Part::Url(m.as_str().to_string()));
        at = m.end();
    }
    if at < text.len() {
        parts.push(Part::Text(text[at..].to_string()));
    }
    parts
}

/// The setup bar's fill, 0 to 1 (`done / total`); a total of 0 draws an
/// empty bar (the page drew `NaN%`).
pub fn fill(status: &SetupStatus) -> f32 {
    if status.total == 0 {
        0.
    } else {
        status.done as f32 / status.total as f32
    }
}

// settings agent: what a step needs the person to do (installing WSL).

/// What a step needs the person to do before it can go on, and the one
/// button its row shows (`ui-seam-wsl-install.md`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepNeeds {
    /// WSL is not installed: `Install WSL` (`Command::InstallWsl`).
    InstallWsl,
    /// WSL is installed and Windows needs a restart: `Restart now`
    /// (`Command::RestartComputer`, one press, no confirmation).
    Restart,
}

/// `Install WSL`.
pub const INSTALL_WSL: &str = "Install WSL";
/// `Restart now`.
pub const RESTART_NOW: &str = "Restart now";

/// What `step` needs (`Step.needs`, `mcbench_setup::Needs`).
pub fn step_needs(step: &Step) -> Option<StepNeeds> {
    step.needs.map(|needs| match needs {
        mcbench_setup::Needs::InstallWsl => StepNeeds::InstallWsl,
        mcbench_setup::Needs::Restart => StepNeeds::Restart,
    })
}

// settings agent (s4-needs): a step that needs the person, wherever a
// composer is.

/// The step that needs the person (its row's button), or that ran after
/// needing them (the installer: running, `needs` none), to show above the
/// composer and in the empty chat's steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NeedsLine {
    /// The step's id.
    pub id: String,
    /// Its text as the core gives it: the step's note (its sentence), else
    /// its label.
    pub text: String,
    /// The button it shows; none while its work runs (the working dots).
    pub needs: Option<StepNeeds>,
}

/// The step needing the person now, or the one `remembered` (it needed them
/// at the last update) while it runs: the installer the button started.
pub fn needs_line(steps: &[Step], remembered: Option<&str>) -> Option<NeedsLine> {
    let line = |step: &Step, needs| {
        let row = step_row(step);
        NeedsLine {
            id: step.id.clone(),
            text: row.note.unwrap_or(row.label),
            needs,
        }
    };
    if let Some(step) = steps.iter().find(|s| step_needs(s).is_some()) {
        return Some(line(step, step_needs(step)));
    }
    let id = remembered?;
    steps
        .iter()
        .find(|s| s.id == id && matches!(s.state, StepState::Running { .. }))
        .map(|step| line(step, None))
}

/// The steps a ready app's empty chat shows: those needing the person, and
/// the `remembered` one while it runs.
pub fn needs_steps(steps: &[Step], remembered: Option<&str>) -> Vec<Step> {
    steps
        .iter()
        .filter(|s| {
            step_needs(s).is_some()
                || (Some(s.id.as_str()) == remembered
                    && matches!(s.state, StepState::Running { .. }))
        })
        .cloned()
        .collect()
}

/// What to remember after `steps` arrived: the step needing the person, or
/// the remembered one while it runs; else nothing.
pub fn remembered_after(steps: &[Step], remembered: Option<&str>) -> Option<String> {
    needs_line(steps, remembered).map(|line| line.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, optional: bool, state: StepState) -> Step {
        Step {
            id: id.into(),
            title: id.into(),
            doing: id.into(),
            optional,
            state,
            needs: None,
        }
    }

    #[test]
    fn the_count_is_of_the_steps_the_app_needs_an_optional_step_that_cannot_be_done_is_no_failure()
    {
        let mut steps = vec![
            step("jdk", false, StepState::Done { seconds: 1. }),
            step("engine", false, StepState::Done { seconds: 1. }),
            step("browser", false, StepState::Waiting),
            step("pack", false, StepState::Waiting),
            step("textures", false, StepState::Waiting),
            step("bake", false, StepState::Waiting),
            step("data", false, StepState::Waiting),
            step(
                "shell",
                true,
                StepState::Unavailable {
                    error: "Docker is not installed".into(),
                },
            ),
        ];
        let status = setup_status(&steps);
        assert_eq!(
            (status.ready, status.done, status.total, status.failed),
            (false, 2, 7, false)
        );
        assert_eq!(
            held_line(&status, true),
            "Sends when the app is ready. 2 of 7 done."
        );
        steps[2].state = StepState::Failed {
            error: "fetch failed".into(),
        };
        let failed = setup_status(&steps);
        assert!(failed.failed);
        assert_eq!(
            held_line(&failed, true),
            "Not sent yet: a step failed. Try it again above."
        );
    }

    #[test]
    fn the_held_line_names_where_to_retry_only_when_the_steps_are_shown() {
        let failed = SetupStatus {
            ready: false,
            done: 1,
            total: 7,
            failed: true,
        };
        assert_eq!(held_line(&failed, false), "Not sent yet: a step failed.");
        assert_eq!(
            held_line(&failed, true),
            "Not sent yet: a step failed. Try it again above."
        );
    }

    // settings agent: the page's tests of the rows, sizes and links.

    #[test]
    fn a_download_says_how_far_it_is_in_mb_of_its_total_when_the_server_said() {
        assert_eq!(megabytes(84_200_000, Some(136_400_000)), "84 of 136 MB");
        assert_eq!(megabytes(12_600_000, None), "13 MB");
        assert_eq!(megabytes(0, Some(280_000_000)), "0 of 280 MB");
    }

    #[test]
    fn a_downloads_size_is_its_bytes_in_mb_rounded_as_its_progress_is() {
        assert_eq!(download_size(172_466_618), "172 MB");
        assert_eq!(megabytes(172_466_618, Some(172_466_618)), "172 of 172 MB");
    }

    #[test]
    fn a_step_reads_as_what_it_makes_or_what_its_work_does_while_it_runs() {
        let at = |state| Step {
            id: "jdk".into(),
            title: "Java".into(),
            doing: "Downloading Java".into(),
            optional: false,
            state,
            needs: None,
        };
        let row = |id: &str, state, label: &str, value: &str, note: Option<&str>| StepRow {
            id: id.into(),
            state,
            optional: false,
            label: label.into(),
            value: value.into(),
            note: note.map(str::to_string),
        };
        assert_eq!(
            step_row(&at(StepState::Waiting)),
            row("jdk", "waiting", "Java", "Waiting", None)
        );
        assert_eq!(
            step_row(&at(StepState::Running {
                detail: Some("Downloading Temurin".into()),
                progress: Some(mcbench_setup::Bytes {
                    done: 84_000_000,
                    total: Some(136_000_000)
                }),
            })),
            row(
                "jdk",
                "running",
                "Downloading Java",
                "84 of 136 MB",
                Some("Downloading Temurin")
            )
        );
        assert_eq!(
            step_row(&at(StepState::Running {
                detail: None,
                progress: None
            })),
            row("jdk", "running", "Downloading Java", "", None)
        );
        assert_eq!(
            step_row(&at(StepState::Done { seconds: 2.4 })),
            row("jdk", "done", "Java", "Done", None)
        );
        // A failure says what the server said.
        assert_eq!(
            step_row(&at(StepState::Failed {
                error: "fetch failed".into()
            })),
            row("jdk", "failed", "Java", "Failed", Some("fetch failed"))
        );
        // The page's `paused` case cannot be written: the seam's step has
        // the five states only.
    }

    #[test]
    fn an_errors_web_addresses_become_links_without_the_bracket_or_full_stop_after_them() {
        assert_eq!(
            link_parts(
                "install Docker Desktop (https://www.docker.com/products/docker-desktop/) on a Mac."
            ),
            vec![
                Part::Text("install Docker Desktop (".into()),
                Part::Url("https://www.docker.com/products/docker-desktop/".into()),
                Part::Text(") on a Mac.".into()),
            ]
        );
        assert_eq!(
            link_parts("see https://docs.docker.com/engine/install/."),
            vec![
                Part::Text("see ".into()),
                Part::Url("https://docs.docker.com/engine/install/".into()),
                Part::Text(".".into()),
            ]
        );
        assert_eq!(
            link_parts("fetch failed"),
            vec![Part::Text("fetch failed".into())]
        );
    }

    #[test]
    fn an_empty_list_of_needed_steps_draws_an_empty_bar() {
        let status = setup_status(&[]);
        assert_eq!(fill(&status), 0.);
    }

    // settings agent (s4-needs)
    #[test]
    fn a_step_that_needs_the_person_is_shown_while_it_needs_them_and_while_its_work_runs() {
        let shell = |state, needs| {
            let mut s = step("shell", true, state);
            s.needs = needs;
            s
        };
        let done = step("data", false, StepState::Done { seconds: 1. });
        let need = shell(
            StepState::Unavailable {
                error: "To use bash and build, Windows needs WSL.".into(),
            },
            Some(mcbench_setup::Needs::InstallWsl),
        );
        let steps = vec![done.clone(), need];
        assert!(
            setup_status(&steps).ready,
            "an optional step needing the person holds nothing"
        );
        let line = needs_line(&steps, None).expect("a line");
        assert_eq!(line.text, "To use bash and build, Windows needs WSL.");
        assert_eq!(line.needs, Some(StepNeeds::InstallWsl));
        let remembered = remembered_after(&steps, None);
        assert_eq!(remembered.as_deref(), Some("shell"));
        let running = vec![
            done.clone(),
            shell(
                StepState::Running {
                    detail: Some("Installing WSL.".into()),
                    progress: None,
                },
                None,
            ),
        ];
        let line = needs_line(&running, remembered.as_deref()).expect("the installer");
        assert_eq!((line.text.as_str(), line.needs), ("Installing WSL.", None));
        assert_eq!(
            needs_line(&running, None),
            None,
            "a running step that never needed them"
        );
        let ids = |v: Vec<Step>| v.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(
            ids(needs_steps(&steps, None)),
            ["shell"],
            "the done rows are not shown"
        );
        assert_eq!(ids(needs_steps(&running, remembered.as_deref())), ["shell"]);
        assert!(needs_steps(&running, None).is_empty());
        let ready = vec![done, shell(StepState::Done { seconds: 3. }, None)];
        assert_eq!(needs_line(&ready, Some("shell")), None);
        assert_eq!(remembered_after(&ready, Some("shell")), None);
    }
}
