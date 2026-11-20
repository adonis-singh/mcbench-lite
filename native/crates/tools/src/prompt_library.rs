//! `prompt_library` (chats only): `executeToolCall`'s branch over the
//! store's library (`mcbench_store::PromptLibrary::apply`, today's
//! `prompts.js` `apply`): `{ ok: true, prompts }` with the library as the
//! model sees it, or `{ ok: false, error }` with `apply`'s sentence.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::{Chat, Progress, Services, Tool, ToolError, ToolOutcome};

/// The `prompt_library` tool.
pub struct PromptLibraryTool(pub Arc<Services>);

impl PromptLibraryTool {
    /// `promptLibrary.apply(args, { chat, model })`, as the tool answers it.
    pub async fn run(&self, chat: &Chat, args: &Map<String, Value>) -> Result<Value, ToolError> {
        // The store's JSON is its own type: the arguments cross as text.
        let args =
            mcbench_store::json::parse(&mcbench_json::stringify(&Value::Object(args.clone())))?;
        let by = mcbench_store::json::parse(&mcbench_json::stringify(
            &json!({"chat": chat.id, "model": chat.model}),
        ))?;
        let prompts = self.0.prompts.clone();
        let applied = tokio::task::spawn_blocking(move || prompts.apply(&args, &by))
            .await
            .expect("the prompt library does not panic");
        Ok(match applied {
            Ok(prompts) => {
                let list =
                    mcbench_store::json::stringify(&mcbench_store::json::Json::Array(prompts));
                json!({"ok": true, "prompts": mcbench_json::parse(&list)?})
            }
            Err(error) => json!({"ok": false, "error": error.message}),
        })
    }
}

#[async_trait]
impl Tool for PromptLibraryTool {
    fn name(&self) -> &'static str {
        "prompt_library"
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        _progress: &mut (dyn FnMut(Progress) + Send),
        _cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        Ok(ToolOutcome::result(self.run(chat, arguments).await?))
    }
}
