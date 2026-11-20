//! `view_image`: `agent.js` `executeViewImage` and its branch of
//! `executeToolCall`. The sandbox reads the file from `/work` (the path
//! resolved and refused as `resolveWorkPath` does) and Pillow normalises it
//! to a PNG of at most 1568 pixels a side; the PNG is stored with the chat's
//! images (a `view-` name keeps it out of the `/work/images` mirror) and
//! follows the result as a user message, referred to by URL.

use std::sync::Arc;

use async_trait::async_trait;
use mcbench_store::ImageData;
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

use crate::{Chat, ImageRef, Progress, Screenshots, Services, Tool, ToolError, ToolOutcome, js};

/// The `view_image` tool.
pub struct ViewImage(pub Arc<Services>);

impl ViewImage {
    /// `executeViewImage` and what `executeToolCall` makes of it.
    pub async fn run(&self, chat: &Chat, args: &Map<String, Value>) -> ToolOutcome {
        let path = js::string_or_empty(args.get("path"));
        let saved = async {
            let png = self
                .0
                .sandbox
                .view_image(&chat.workspace, &path)
                .await
                .map_err(|error| error.to_string())?;
            let id = chat.id.clone();
            self.0
                .blocking(move |store| store.save_images(&id, &[ImageData::Png(png)], "view-"))
                .await
                .map_err(|error| error.message)
        };
        match saved.await {
            Err(error) => ToolOutcome::result(json!({"ok": false, "error": error})),
            Ok(urls) => ToolOutcome {
                result: json!({"ok": true, "note": "image attached in the next message"}),
                screenshots: Some(Screenshots {
                    text: format!("[view_image] /work/{path}:"),
                    images: urls.into_iter().map(|url| ImageRef { url }).collect(),
                }),
                images: Vec::new(),
            },
        }
    }
}

#[async_trait]
impl Tool for ViewImage {
    fn name(&self) -> &'static str {
        "view_image"
    }

    async fn call(
        &self,
        chat: &mut Chat,
        arguments: &Map<String, Value>,
        _progress: &mut (dyn FnMut(Progress) + Send),
        _cancel: &CancellationToken,
    ) -> Result<ToolOutcome, ToolError> {
        Ok(self.run(chat, arguments).await)
    }
}
