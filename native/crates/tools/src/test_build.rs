//! `test_build`: the tool over the verifier's `executeTestBuild`
//! ([`crate::verify::execute_test_build`]: the call's checks, its tests on
//! the pool's lanes side by side, the inline limit), with the chat's `/work`
//! and storage budget as the verifier's [`Work`]: a chat over its limits
//! runs no experiments, old reports are pruned (`pruneReports`), and each
//! report is copied into `/work/verification` within the budget.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::verify::{Work, execute_test_build};
use crate::{Chat, Progress, Services, Tool, ToolError, ToolOutcome, Workspace, js};

/// `REPORT_NAME`: `^[0-9a-f-]{36}\.json$`.
fn report_name(name: &str) -> bool {
    name.len() == 41
        && name.ends_with(".json")
        && name[..36]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || b == b'-')
}

/// The chat's `/work` and budget as the verifier keeps reports.
pub struct ChatWork {
    services: Arc<Services>,
    chat: String,
    workspace: Workspace,
}

impl ChatWork {
    /// The work of `chat` over the services' store and sandbox.
    pub fn new(services: Arc<Services>, chat: &str, workspace: &Workspace) -> ChatWork {
        ChatWork {
            services,
            chat: chat.to_string(),
            workspace: workspace.clone(),
        }
    }

    /// `pruneReports(conversationId)`: a chat keeps at most `REPORTS_KEPT`
    /// reports in `/work/verification`; before a call adds more, the oldest
    /// past that number go, never one a stored message names. The
    /// listing's file errors are told in `/work` terms (`workError`).
    async fn prune_reports(&self) -> Result<(), String> {
        let workspace = &self.workspace;
        if !mcbench_sandbox::work::work_file_exists(workspace, "verification")? {
            return Ok(());
        }
        let dir = mcbench_sandbox::work::resolve_work_path(workspace, "verification", false)?;
        let keep = self.services.store.storage().limits().reports_kept as usize;
        let id = self.chat.clone();
        let tell = |error: std::io::Error| mcbench_sandbox::work::work_error(error, "verification");
        let doomed = self
            .services
            .blocking(move |store| -> Result<Vec<String>, String> {
                let mut names = Vec::new();
                for entry in std::fs::read_dir(&dir).map_err(tell)? {
                    let name = entry
                        .map_err(tell)?
                        .file_name()
                        .to_string_lossy()
                        .into_owned();
                    if report_name(&name) {
                        names.push(name);
                    }
                }
                if names.len() <= keep {
                    return Ok(Vec::new());
                }
                let referenced = store.report_references(&id).map_err(|e| e.message)?;
                let referenced = referenced.into_iter().collect();
                let failed = std::cell::RefCell::new(None);
                let doomed = mcbench_store::expendable(&names, &referenced, keep, |name| {
                    match std::fs::symlink_metadata(dir.join(name)).and_then(|m| m.modified()) {
                        Ok(time) => time
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_or(0.0, |d| d.as_secs_f64() * 1000.0),
                        Err(error) => {
                            failed.borrow_mut().get_or_insert(tell(error));
                            0.0
                        }
                    }
                });
                match failed.into_inner() {
                    Some(error) => Err(error),
                    None => Ok(doomed),
                }
            })
            .await?;
        if !doomed.is_empty() {
            let paths: Vec<String> = doomed
                .iter()
                .map(|name| format!("verification/{name}"))
                .collect();
            self.services
                .sandbox
                .remove_work_files(workspace, &paths)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }
}

#[async_trait]
impl Work for ChatWork {
    async fn prepare(&self) -> Result<(), String> {
        // A chat over its storage limits runs no experiments.
        let id = self.chat.clone();
        let full = self
            .services
            .blocking(move |store| store.storage().refusal(&id))
            .await
            .map_err(|error| error.message)?;
        if let Some(full) = full {
            return Err(full);
        }
        self.prune_reports().await
    }

    async fn keep(&self, name: &str, file: &Path) -> Result<(), String> {
        let (id, source, workspace, name) = (
            self.chat.clone(),
            file.to_path_buf(),
            self.workspace.clone(),
            name.to_string(),
        );
        self.services
            .blocking(move |store| -> Result<(), String> {
                let size = std::fs::metadata(&source).map_err(|e| e.to_string())?.len();
                store
                    .storage()
                    .reserve(&id, size, true)
                    .map_err(|e| e.message)?;
                mcbench_sandbox::work::copy_into_work(&workspace, &name, &source, true)
            })
            .await
    }
}

/// The `test_build` tool.
pub struct TestBuild(pub Arc<Services>);

impl TestBuild {
    /// `executeTestBuild(conversation, args, signal, minecraftVerifier, onUpdate)`.
    pub async fn run(
        &self,
        chat: &Chat,
        args: &Map<String, Value>,
        progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Value {
        let work = ChatWork::new(self.0.clone(), &chat.id, &chat.workspace);
        let mut forward = |update: Map<String, Value>| {
            let message = update.get("message").map(js::to_string).unwrap_or_default();
            progress(Progress {
                message,
                raw: update,
            });
        };
        Value::Object(
            execute_test_build(
                &self.0.verifier,
                &chat.world,
                args,
                cancel,
                &mut forward,
                &work,
            )
            .await,
        )
    }
}

#[async_trait]
impl Tool for TestBuild {
    fn name(&self) -> &'static str {
        "test_build"
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::result(
            self.run(chat, arguments, progress, cancel).await,
        ))
    }
}
