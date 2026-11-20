//! A `test_build` call (`server/agent.js` `executeTestBuild`): its tests on
//! the pool's lanes side by side, their reports kept under
//! `/work/verification/`, and the result a model reads, trimmed to the
//! inline limit.

use std::path::Path;
use std::sync::atomic::Ordering;

use async_trait::async_trait;
use mcbench_world::World;
use serde_json::{Map, Value};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::error::VerifyError;
use super::request::call_tests;
use super::system::random_uuid;
use super::verifier::{Channels, MAX_INLINE_BYTES, VerifierPool};

/// The note of a failed test or call.
pub const FAILURE_NOTE: &str = "No completed Minecraft verification is available from this call.";

/// What a call does with the chat's `/work` folder and its storage budget
/// (`storage.refusal`, `pruneReports`, `storage.reserve` and
/// `sandbox.copyIntoWork`, which are the store's and the sandbox's).
#[async_trait]
pub trait Work: Send + Sync {
    /// Before any test runs: the chat's storage refusal, then the pruning
    /// of old reports. An error refuses the call with its message.
    async fn prepare(&self) -> Result<(), String>;
    /// Keeps the report at `file` as `/work/<name>` (`name` is
    /// `verification/<uuid>.json`), within the chat's budget.
    async fn keep(&self, name: &str, file: &Path) -> Result<(), String>;
}

/// A `/work` folder with no storage budget: reports are copied into it.
pub struct WorkFolder(pub std::path::PathBuf);

#[async_trait]
impl Work for WorkFolder {
    async fn prepare(&self) -> Result<(), String> {
        Ok(())
    }

    async fn keep(&self, name: &str, file: &Path) -> Result<(), String> {
        let to = self.0.join(name);
        let from = file.to_path_buf();
        tokio::task::spawn_blocking(move || {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            std::fs::copy(&from, &to)
                .map(|_| ())
                .map_err(|e| format!("{}: {e}", to.display()))
        })
        .await
        .expect("a copy does not panic")
    }
}

/// `failure(error)`: a test's or a call's answer when it has no report.
pub fn failure(error: &VerifyError, cancelled: bool) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("ok".into(), Value::Bool(false));
    if cancelled {
        out.insert("cancelled".into(), Value::Bool(true));
    }
    if let Some(budget) = error.budget_object() {
        out.insert("code".into(), Value::from("VERIFICATION_BUDGET_EXCEEDED"));
        out.insert("budget".into(), Value::Object(budget.clone()));
    }
    out.insert("error".into(), Value::from(error.to_string()));
    out.insert("note".into(), Value::from(FAILURE_NOTE));
    out
}

/// Runs a `test_build` call on `world` (`executeTestBuild(conversation,
/// args, signal, verifier, onUpdate)`): the result object the tool message
/// carries. `progress` is given each update with every test's message
/// (`Test 1: ...; Test 2: ...`).
pub async fn execute_test_build(
    pool: &VerifierPool,
    world: &World,
    args: &Map<String, Value>,
    cancel: &CancellationToken,
    progress: &mut (dyn FnMut(Map<String, Value>) + Send),
    work: &dyn Work,
) -> Map<String, Value> {
    let settings = pool.settings.lock().unwrap().clone();
    let tests = match call_tests(args, &settings.budgets) {
        Ok(tests) => tests,
        Err(error) => return failure(&error, cancel.is_cancelled()),
    };
    if let Err(message) = work.prepare().await {
        return failure(&VerifyError::Message(message), cancel.is_cancelled());
    }
    let count = tests.len();
    let mut messages = vec!["Waiting for the Minecraft verifier".to_string(); count];
    let mut results: Vec<Option<Map<String, Value>>> = vec![None; count];
    let mut files = Vec::with_capacity(count);
    let mut started = vec![None; count];
    let (events, mut events_out) = mpsc::unbounded_channel();
    let (answers, mut answers_out) = mpsc::unbounded_channel();
    for (index, test) in tests.iter().enumerate() {
        files.push(format!("verification/{}.json", random_uuid()));
        let channels = Channels {
            events: events.clone(),
            answers: answers.clone(),
        };
        match pool.submit(world, test, &settings, cancel, None, index, channels) {
            Ok(submitted) => started[index] = Some(submitted.started),
            Err(error) => results[index] = Some(failure(&error, cancel.is_cancelled())),
        }
    }
    drop((events, answers));
    let mut forward =
        |(index, update): (usize, Map<String, Value>),
         progress: &mut (dyn FnMut(Map<String, Value>) + Send)| {
            messages[index] = update
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let mut out = update;
            let joined: Vec<String> = messages
                .iter()
                .enumerate()
                .map(|(i, message)| format!("Test {}: {message}", i + 1))
                .collect();
            out.insert("message".into(), Value::from(joined.join("; ")));
            progress(out);
        };
    let waiting = |results: &Vec<Option<Map<String, Value>>>| results.iter().any(Option::is_none);
    while waiting(&results) {
        let queued = started.iter().enumerate().any(|(i, s)| {
            results[i].is_none() && s.as_ref().is_some_and(|s| !s.load(Ordering::SeqCst))
        });
        tokio::select! {
            biased;
            Some(event) = events_out.recv() => forward(event, progress),
            Some((index, answer)) = answers_out.recv() => {
                if results[index].is_some() {
                    continue;
                }
                results[index] = Some(match answer {
                    Ok(report) => keep(report, &files[index], work, cancel).await,
                    Err(error) => failure(&error, cancel.is_cancelled()),
                });
            }
            _ = cancel.cancelled(), if queued => {
                let reason = pool.aborted_reason(cancel).expect("cancelled");
                answer_queued(&mut results, &started, reason, true);
            }
            _ = pool.shutdown().cancelled(), if queued => {
                let reason = pool.aborted_reason(cancel).expect("shut down");
                answer_queued(&mut results, &started, reason, cancel.is_cancelled());
            }
        }
    }
    while let Ok(event) = events_out.try_recv() {
        forward(event, progress);
    }
    let results: Vec<Map<String, Value>> =
        results.into_iter().map(|r| r.expect("answered")).collect();
    call_result(results, cancel.is_cancelled())
}

/// The tests not started yet are answered with `reason`.
fn answer_queued(
    results: &mut [Option<Map<String, Value>>],
    started: &[Option<std::sync::Arc<std::sync::atomic::AtomicBool>>],
    reason: &str,
    cancelled: bool,
) {
    for (index, flag) in started.iter().enumerate() {
        if results[index].is_none() && flag.as_ref().is_some_and(|f| !f.load(Ordering::SeqCst)) {
            results[index] = Some(failure(&VerifyError::message(reason), cancelled));
        }
    }
}

/// A finished test: its report kept in `/work`, the archive removed, and
/// `report` naming it.
async fn keep(
    mut report: Map<String, Value>,
    name: &str,
    work: &dyn Work,
    cancel: &CancellationToken,
) -> Map<String, Value> {
    let file = report
        .shift_remove("reportFile")
        .and_then(|v| v.as_str().map(str::to_string))
        .expect("a finished test has its report file");
    let kept = work.keep(name, Path::new(&file)).await;
    let _ = std::fs::remove_file(&file);
    if let Err(message) = kept {
        return failure(&VerifyError::Message(message), cancel.is_cancelled());
    }
    report.insert("report".into(), Value::from(format!("/work/{name}")));
    report
}

/// The call's result from its tests' (`call`, then `trimmed` and
/// `summaries` past `MAX_INLINE_BYTES`).
fn call_result(results: Vec<Map<String, Value>>, cancelled: bool) -> Map<String, Value> {
    let failed: Vec<String> = results
        .iter()
        .enumerate()
        .filter(|(_, r)| !super::js::truthy(r.get("ok")))
        .map(|(i, r)| {
            format!(
                "test {}: {}",
                i + 1,
                r.get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("undefined")
            )
        })
        .collect();
    let call = |tests: Vec<Map<String, Value>>| {
        let mut out = Map::new();
        out.insert("ok".into(), Value::Bool(failed.is_empty()));
        if cancelled {
            out.insert("cancelled".into(), Value::Bool(true));
        }
        if !failed.is_empty() {
            out.insert("error".into(), Value::from(failed.join("\n")));
        }
        out.insert(
            "tests".into(),
            Value::Array(tests.into_iter().map(Value::Object).collect()),
        );
        out
    };
    let without = |result: &Map<String, Value>, names: &[&str], added: &[&str]| {
        let mut out: Map<String, Value> = result
            .iter()
            .filter(|(key, _)| !names.contains(&key.as_str()))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if super::js::truthy(out.get("ok")) {
            for name in added {
                out.insert((*name).into(), Value::Bool(true));
            }
        }
        out
    };
    let trimmed: Vec<Map<String, Value>> = results
        .iter()
        .map(|r| {
            without(
                r,
                &[
                    "initial",
                    "observations",
                    "probes",
                    "actions",
                    "inputs",
                    "sampleTicks",
                ],
                &["observationsInFile", "probesInFile", "inputsInFile"],
            )
        })
        .collect();
    let summaries: Vec<Map<String, Value>> = trimmed
        .iter()
        .map(|r| {
            without(
                r,
                &[
                    "preview",
                    "blockStateMigrations",
                    "importedEntities",
                    "importedBlockEntities",
                ],
                &["previewInFile"],
            )
        })
        .collect();
    let forms = [call(results), call(trimmed), call(summaries)];
    let last = forms.len() - 1;
    forms
        .into_iter()
        .enumerate()
        .find(|(i, form)| {
            *i == last
                || mcbench_json::stringify(&Value::Object(form.clone())).len() <= MAX_INLINE_BYTES
        })
        .map(|(_, form)| form)
        .expect("the last form is taken")
}
