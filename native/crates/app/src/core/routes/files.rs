//! A chat's images and attachments by their addresses
//! (`GET /api/images/:chat/:name`, `GET /api/attachments/:chat/:name`): the
//! file on disk and how today's route shows it; the window reads the file
//! itself. Revalidation and ETags (`static-file.js`) go with HTTP.

use std::path::PathBuf;

use mcbench_agent::lifecycle::Refusal;
use mcbench_store::AttachmentFile;

use crate::core::{Core, refusal};

/// `MIME[extension]` of `server/index.js`, for the types a file keeps.
fn mime(extension: &str) -> Option<&'static str> {
    Some(match extension {
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".webp" => "image/webp",
        ".gif" => "image/gif",
        ".svg" => "image/svg+xml",
        ".pdf" => "application/pdf",
        ".txt" => "text/plain; charset=utf-8",
        ".md" => "text/markdown; charset=utf-8",
        ".csv" => "text/csv; charset=utf-8",
        _ => return None,
    })
}

/// `path.extname(name).toLowerCase()`.
fn extension(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    match base.rfind('.') {
        Some(0) | None => String::new(),
        Some(at) => base[at..].to_lowercase(),
    }
}

/// The type a picked file is sent with, as a browser's `File.type` names
/// the common ones (the attachment's stored `type`): the shown types'
/// own, else `application/octet-stream`.
pub fn mime_of(name: &str) -> &'static str {
    match extension(name).as_str() {
        ".txt" => "text/plain",
        ".md" => "text/markdown",
        ".csv" => "text/csv",
        ".json" => "application/json",
        ".js" | ".mjs" => "text/javascript",
        ".html" => "text/html",
        ".css" => "text/css",
        other => mime(other).unwrap_or("application/octet-stream"),
    }
}

impl Core {
    /// `GET /api/images/:chat/:name`: the file and its type.
    pub fn image_file(&self, url: &str) -> Result<(PathBuf, String), Refusal> {
        let file = self.0.store.image_file(url)?;
        if !file.exists() {
            return Err(refusal(404, "Image not found"));
        }
        let name = file.to_string_lossy().into_owned();
        let kind = if name.ends_with(".jpg") {
            "image/jpeg".to_string()
        } else {
            format!("image/{}", name.rsplit('.').next().unwrap_or_default())
        };
        Ok((file, kind))
    }

    /// `GET /api/attachments/:chat/:name`.
    pub fn attachment_file(&self, url: &str) -> Result<AttachmentFile, Refusal> {
        let file = self.0.store.attachment_file(url)?;
        if !file.exists() {
            return Err(refusal(404, "Attachment not found"));
        }
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().replace('"', ""))
            .unwrap_or_default();
        let shown = mime(&extension(&name));
        Ok(AttachmentFile {
            content_type: shown.unwrap_or("application/octet-stream"),
            inline: shown.is_some(),
            file_name: name,
            path: file,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `attachment-serving.test.js` (the route's part of 1 of 3: which
    /// types are shown inline).
    #[test]
    fn attachments_of_a_type_a_browser_shows_keep_it_and_the_rest_are_bytes() {
        for (name, shown) in [
            ("photo.png", Some("image/png")),
            ("scan.JPG", Some("image/jpeg")),
            ("notes.txt", Some("text/plain; charset=utf-8")),
            ("doc.pdf", Some("application/pdf")),
            ("drawing.svg", Some("image/svg+xml")),
            ("table.csv", Some("text/csv; charset=utf-8")),
            ("readme.md", Some("text/markdown; charset=utf-8")),
            ("evil.js", None),
            ("page.html", None),
            ("style.css", None),
            ("data.json", None),
            ("noext", None),
        ] {
            assert_eq!(mime(&extension(name)), shown, "{name}");
        }
    }
}
