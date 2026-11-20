//! `mcbench-lite headless setup <data folder>`: the data step and the
//! textures step of the app's setup on a data folder, with no window
//! (`PROTOCOL.md`, family `setup`), so a machine with no display can make
//! the two folders `turn` and `shots` name as `MCBENCH_TABLES` and
//! `MCBENCH_TEXTURES`. They are `mcbench_setup::start`'s own two steps
//! ([`mcbench_setup::start::data_and_textures`]) on the shipped data the
//! app unpacks (the binary's `shipped`, given as [`Shipped`]), under the
//! data folder's one-instance lock. Then the shell step's files
//! (`mcbench_setup::shell::files`: in a shipped build the pinned root
//! filesystem, and the guest base where a guest runs the boxes, fetched and
//! checked), so a machine is ready before its first turn and a download
//! that fails is seen here, once, and not in every turn; the boxes
//! themselves are `turn`'s, which finds the files kept and fetches nothing.
//! A build that has no model's shell (the step unavailable) does not fail
//! setup.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mcbench_setup::steps::native_steps;
use mcbench_setup::{Data, InstanceLock, Step, StepState, Steps};
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// What makes the shipped data, once, on the blocking pool (the app's
/// `DataSource`).
pub type DataSource = Box<dyn Fn() -> Result<Data, String> + Send + Sync>;

/// The binary's maker of the shipped data of a data folder: the embedded
/// bundle unpacked (feature `shipped-data`), or the app's sentence for a
/// build without it.
pub type Shipped = fn(PathBuf) -> DataSource;

/// What is called with each progress line.
pub type Say = Arc<dyn Fn(&str) + Send + Sync>;

/// What the progress line of `step` says: its title and its state.
pub fn progress_line(step: &Step) -> String {
    let state = match &step.state {
        StepState::Waiting => "waiting".to_string(),
        StepState::Running { detail, progress } => {
            let mut said = "running".to_string();
            if let Some(detail) = detail {
                said.push_str(&format!(", {detail}"));
            }
            if let Some(bytes) = progress {
                said.push_str(&match bytes.total {
                    Some(total) => format!(", {} of {total} bytes", bytes.done),
                    None => format!(", {} bytes", bytes.done),
                });
            }
            said
        }
        StepState::Done { seconds } => format!("done in {seconds} s"),
        StepState::Failed { .. } => "failed".to_string(),
        StepState::Unavailable { error } => format!("unavailable: {error}"),
    };
    format!("{}: {state}", step.title)
}

/// Runs the data step, the textures step and the shell step's files on
/// `folder` (an absolute path) while holding its one-instance lock, calling `say` with one
/// [`progress_line`] per step change. It answers the line for stdout,
/// `{"tables":"<folder>/minecraft/<id>","textures":"<folder>/textures/<id>"}`,
/// or the sentence of the failure: the failed step's, as the setup screen
/// shows it, or the lock's refusal.
pub async fn prepare(
    folder: &Path,
    data: DataSource,
    client: &reqwest::Client,
    say: Say,
) -> Result<String, String> {
    let _lock = InstanceLock::take(folder)?;
    let seen = Mutex::new(native_steps());
    let steps = Arc::new(Steps::new(
        native_steps(),
        Box::new(move |list: &[Step]| {
            let mut seen = seen.lock().expect("the steps seen");
            for (step, before) in list.iter().zip(seen.iter()) {
                if step.state != before.state {
                    say(&progress_line(step));
                }
            }
            *seen = list.to_vec();
        }),
    ));
    let made = mcbench_setup::start::data_and_textures(
        &steps,
        folder,
        Box::new(data),
        client,
        CancellationToken::new(),
    )
    .await;
    let failed = || {
        steps
            .view()
            .1
            .into_iter()
            .find_map(|step| match step.state {
                StepState::Failed { error } => Some(error),
                _ => None,
            })
    };
    let (data, textures) =
        made.map_err(|_| failed().expect("a step that fails is marked failed with its sentence"))?;
    let shell = mcbench_setup::shell::files(
        Arc::clone(&steps),
        folder.to_path_buf(),
        client.clone(),
        CancellationToken::new(),
    );
    if steps.track("shell", shell).await.is_err() {
        // Unavailable (this build has no model's shell) is not a failure.
        if let Some(sentence) = failed() {
            return Err(sentence);
        }
    }
    Ok(mcbench_json::stringify(&json!({
        "tables": data.folder().display().to_string(),
        "textures": textures.folder().display().to_string(),
    })))
}

fn refuse(message: &str) -> ! {
    eprintln!("mcbench-lite headless setup: {message}");
    std::process::exit(2)
}

fn stop(message: &str) -> ! {
    eprintln!("mcbench-lite headless setup: {message}");
    std::process::exit(1)
}

/// `headless setup <data folder>`: [`prepare`] on the folder (made absolute)
/// with the shipped data and the app's client; the line on stdout and
/// status 0, or the sentence on stderr and status 1. Progress goes to
/// stderr.
pub(crate) fn run(arguments: &[String], shipped: Shipped) -> ! {
    let [folder] = arguments else {
        refuse("usage: mcbench-lite headless setup <data folder>")
    };
    let folder = std::path::absolute(folder).unwrap_or_else(|e| stop(&format!("{folder}: {e}")));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("a tokio runtime");
    let made = runtime.block_on(prepare(
        &folder,
        shipped(folder.clone()),
        &mcbench_setup::fetch::client(),
        Arc::new(|line: &str| eprintln!("mcbench-lite headless setup: {line}")),
    ));
    match made {
        Ok(line) => {
            println!("{line}");
            std::process::exit(0)
        }
        Err(sentence) => stop(&sentence),
    }
}
