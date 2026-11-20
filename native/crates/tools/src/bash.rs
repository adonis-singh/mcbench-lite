//! `bash`: `agent.js` `executeBash`. A non-zero exit code is information,
//! not a failure: the transcript (`mcbench-sandbox`'s, today's
//! `formatResult` text with its halt and storage notes) goes to the model as
//! plain terminal text, and only sandbox problems count as errors. A chat
//! over its storage limits gets a failure that carries the transcript as its
//! `output`.

use std::sync::Arc;

use async_trait::async_trait;
use mcbench_sandbox::BashCommand;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::{Chat, Progress, Services, Tool, ToolError, ToolOutcome, js};

/// The `bash` tool.
pub struct Bash(pub Arc<Services>);

impl Bash {
    /// `executeBash(conversation, args, signal)`.
    pub async fn run(
        &self,
        chat: &Chat,
        args: &Map<String, Value>,
        cancel: &CancellationToken,
    ) -> Value {
        let command = js::string_or_empty(args.get("command"));
        if js::trim(&command).is_empty() {
            return json!({"ok": false, "error": "bash tool called with an empty command"});
        }
        let failed = |error: String| {
            let mut result = Map::new();
            result.insert("ok".into(), json!(false));
            if cancel.is_cancelled() {
                result.insert("cancelled".into(), json!(true));
            }
            result.insert("error".into(), json!(error));
            Value::Object(result)
        };
        let folder = self.0.store.folder().clone();
        if let Err(error) = mcbench_sandbox::work::sync_images(&chat.workspace, &folder.images()) {
            return failed(error);
        }
        if let Err(error) =
            mcbench_sandbox::work::sync_attachments(&chat.workspace, &folder.attachments())
        {
            return failed(error);
        }
        let request = BashCommand {
            command,
            timeout_ms: args
                .get("timeout_ms")
                .map(|value| js::to_number(Some(value))),
            network: args.get("network") == Some(&Value::Bool(true)),
        };
        match self.0.sandbox.exec(&chat.workspace, &request, cancel).await {
            Ok(run) => match run.storage {
                Some(full) => json!({"ok": false, "error": full, "output": run.formatted}),
                None => json!({"ok": true, "text": run.formatted}),
            },
            Err(error) => failed(error.to_string()),
        }
    }
}

#[async_trait]
impl Tool for Bash {
    fn name(&self) -> &'static str {
        "bash"
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        _progress: &mut (dyn FnMut(Progress) + Send),
        cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::result(self.run(chat, arguments, cancel).await))
    }
}
