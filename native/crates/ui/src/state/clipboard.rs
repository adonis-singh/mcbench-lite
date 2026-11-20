//! A message on the clipboard (`web/clipboard.mjs`): a copied message is its
//! text, with the message itself (text and attachments, their bytes as
//! base64 data URLs, so a copy outlives its source) beside it, so a paste
//! into this app brings both back. The page kept the message in an HTML
//! flavour's attribute; GPUI's clipboard keeps it as the text's JSON
//! metadata, which every platform's clipboard of this app carries.

use std::sync::Arc;

use base64::Engine as _;
use serde_json::{Value, json};

use crate::OutgoingFile;

/// The metadata of a copied message: `{ "mcbenchMessage": { text,
/// attachments: [{ name, type, size, data }] } }`.
pub fn message_metadata(text: &str, attachments: &[OutgoingFile]) -> Value {
    let files: Vec<Value> = attachments
        .iter()
        .map(|file| {
            json!({
                "name": file.name,
                "type": file.content_type,
                "size": file.size,
                "data": format!(
                    "data:{};base64,{}",
                    file.content_type,
                    base64::engine::general_purpose::STANDARD.encode(&file.data)
                ),
            })
        })
        .collect();
    json!({ "mcbenchMessage": { "text": text, "attachments": files } })
}

/// The message a paste carries, when the clipboard holds one
/// (`readMessageClipboard`): none for other metadata; an attachment that is
/// not a file is refused with the page's text. A file's size is its data's.
pub fn read_message(metadata: &Value) -> Result<Option<(String, Vec<OutgoingFile>)>, String> {
    let Some(message) = metadata.get("mcbenchMessage") else {
        return Ok(None);
    };
    let refused = || "The pasted message holds an attachment that is not a file".to_string();
    let text = message["text"].as_str().unwrap_or_default().to_string();
    let mut files = Vec::new();
    for file in message["attachments"].as_array().into_iter().flatten() {
        let name = file["name"].as_str().ok_or_else(refused)?;
        let data = file["data"].as_str().ok_or_else(refused)?;
        let rest = data.strip_prefix("data:").ok_or_else(refused)?;
        let (content_type, base64) = rest.split_once(";base64,").ok_or_else(refused)?;
        if content_type.is_empty() || content_type.contains(',') {
            return Err(refused());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64)
            .map_err(|_| refused())?;
        files.push(OutgoingFile {
            name: name.to_string(),
            content_type: content_type.to_string(),
            size: bytes.len() as u64,
            data: Arc::from(bytes),
        });
    }
    Ok(Some((text, files)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copied_message_comes_back_with_its_files() {
        let file = OutgoingFile {
            name: "door.png".into(),
            content_type: "image/png".into(),
            size: 4,
            data: Arc::from(&b"\x89PNG"[..]),
        };
        let metadata = message_metadata("Add a door", std::slice::from_ref(&file));
        assert_eq!(
            read_message(&metadata),
            Ok(Some(("Add a door".to_string(), vec![file])))
        );
        assert_eq!(read_message(&json!({ "other": 1 })), Ok(None));
        let broken = json!({ "mcbenchMessage": { "text": "x", "attachments": [{ "name": "a", "data": "http://x" }] } });
        assert_eq!(
            read_message(&broken),
            Err("The pasted message holds an attachment that is not a file".to_string())
        );
    }
}
