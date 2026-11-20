//! Images and attachments: stored as files under the data folder and named
//! by URL from messages (`server/store.js`).

use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::json::Json;
use crate::json::Truthy;
use crate::payload::Attachment;
use crate::{Error, MAX_ATTACHMENT_BYTES, MAX_MESSAGE_ATTACHMENT_BYTES, Store};

/// An image to store (`saveImages`' `images`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageData {
    /// PNG bytes (a Buffer).
    Png(Vec<u8>),
    /// A `data:image/<type>;base64,` URL.
    DataUrl(String),
}

/// The sha256 of `bytes` in hex.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// `Buffer.from(text, 'base64')`: Node's forgiving decoder. Characters
/// outside the alphabet are skipped, `-` and `_` read as `+` and `/`, and
/// the first `=` ends the data.
pub(crate) fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            _ => continue,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    out
}

/// `path.normalize` of an absolute path: `.` and `..` resolved by name.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    None | Some(Component::RootDir | Component::Prefix(_))
                ) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path.basename(name)` (POSIX): the last component.
fn basename(name: &str) -> &str {
    let trimmed = name.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(at) => &trimmed[at + 1..],
        None => trimmed,
    }
}

/// `path.extname(name)` (POSIX), as Node computes it.
pub(crate) fn extname(path: &str) -> &str {
    let bytes = path.as_bytes();
    let (mut start_dot, mut start_part, mut end) = (None::<usize>, 0usize, None::<usize>);
    let mut matched_slash = true;
    let mut pre_dot_state = 0;
    for i in (0..bytes.len()).rev() {
        let code = bytes[i];
        if code == b'/' {
            if !matched_slash {
                start_part = i + 1;
                break;
            }
            continue;
        }
        if end.is_none() {
            matched_slash = false;
            end = Some(i + 1);
        }
        if code == b'.' {
            if start_dot.is_none() {
                start_dot = Some(i);
            } else if pre_dot_state != 1 {
                pre_dot_state = 1;
            }
        } else if start_dot.is_some() {
            pre_dot_state = -1;
        }
    }
    match (start_dot, end) {
        (Some(dot), Some(end))
            if pre_dot_state != 0
                && !(pre_dot_state == 1 && dot + 1 == end && dot == start_part + 1) =>
        {
            &path[dot..end]
        }
        _ => "",
    }
}

/// `path.basename(name, ext)` of a name with no `/`.
fn stem<'a>(name: &'a str, ext: &str) -> &'a str {
    if ext.is_empty() {
        return name;
    }
    if name == ext {
        return "";
    }
    name.strip_suffix(ext).unwrap_or(name)
}

/// `text.slice(0, units)` by UTF-16 code units; a pair cut in two loses its
/// half (JavaScript keeps a lone surrogate, which Rust text cannot hold).
fn slice_utf16(text: &str, units: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for c in text.chars() {
        count += c.len_utf16();
        if count > units {
            break;
        }
        out.push(c);
    }
    out
}

/// JavaScript's `String(file?.name || '')` of a field of the request body.
fn field(file: &Json, key: &str) -> String {
    match file.get(key) {
        Some(value) if value.truthy() => crate::json::to_js_string(Some(value)),
        _ => String::new(),
    }
}

struct Decoded {
    name: String,
    mime: String,
    bytes: Vec<u8>,
}

/// `decodeAttachment(file)`.
fn decode_attachment(file: &Json) -> Result<Decoded, Error> {
    let name: String = basename(&field(file, "name"))
        .chars()
        .filter(|c| !matches!(*c as u32, 0..=0x1f | 0x7f))
        .collect();
    let name = slice_utf16(&name, 160);
    if name.is_empty() {
        return Err(Error::refused("Attachment name is required", 400));
    }
    let data = field(file, "data");
    let pattern = crate::regex!(r"^data:([^;,]+);base64,");
    let Some(found) = pattern.captures(&data) else {
        return Err(Error::refused(
            format!("Attachment \"{name}\" is not base64 data"),
            400,
        ));
    };
    let mime = found[1].to_string();
    let bytes = base64_decode(&data[found[0].len()..]);
    if bytes.is_empty() {
        return Err(Error::refused(
            format!("Attachment \"{name}\" is empty"),
            400,
        ));
    }
    if bytes.len() as u64 > MAX_ATTACHMENT_BYTES {
        return Err(Error::refused(
            format!("Attachment \"{name}\" exceeds 100 MB"),
            400,
        ));
    }
    Ok(Decoded { name, mime, bytes })
}

/// An attachment's file and how it is shown (`ATTACHMENT_TYPES`): a type
/// a browser shows and never runs keeps it and opens inline; any other is
/// bytes to save. Made by the core's `attachment_file`
/// (`GET /api/attachments/:chat/:name`) from [`Store::attachment_file`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentFile {
    /// The file.
    pub path: PathBuf,
    /// `content-type`.
    pub content_type: &'static str,
    /// `content-disposition`'s `inline` (else `attachment`).
    pub inline: bool,
    /// Its name, without `"`.
    pub file_name: String,
}

impl Store {
    /// `saveImages(conversationId, images, prefix)`: stores the images, all
    /// counted against the chat's budget before any is written, and returns
    /// their URLs.
    pub fn save_images(
        &self,
        id: &str,
        images: &[ImageData],
        prefix: &str,
    ) -> Result<Vec<String>, Error> {
        let pattern = crate::regex!(r"^data:image/(\w+);base64,");
        let mut decoded = Vec::new();
        for image in images {
            decoded.push(match image {
                ImageData::Png(bytes) => (bytes.clone(), "png".to_string()),
                ImageData::DataUrl(data) => {
                    let found = pattern
                        .captures(data)
                        .ok_or_else(|| Error::plain("Expected an image data URL"))?;
                    let ext = if &found[1] == "jpeg" {
                        "jpg".to_string()
                    } else {
                        found[1].to_string()
                    };
                    (base64_decode(&data[found[0].len()..]), ext)
                }
            });
        }
        self.storage.reserve(
            id,
            decoded.iter().map(|(bytes, _)| bytes.len() as u64).sum(),
            true,
        )?;
        let dir = self.folder.images().join(id);
        std::fs::create_dir_all(&dir)?;
        let mut urls = Vec::new();
        for (bytes, ext) in decoded {
            let name = format!("{prefix}{}.{ext}", &self.host.uuid()[..8]);
            std::fs::write(dir.join(&name), bytes)?;
            urls.push(format!("/api/images/{id}/{name}"));
        }
        Ok(urls)
    }

    /// `imageFile(url)`: where an image URL is on disk.
    pub fn image_file(&self, url: &str) -> Result<PathBuf, Error> {
        let parts: Vec<&str> = url.split('/').collect();
        let (Some(chat), Some(name)) = (parts.get(3), parts.get(4)) else {
            return Err(Error::plain("Bad image path"));
        };
        let images = self.folder.images();
        let file = normalize(&images.join(chat).join(name));
        if !file
            .to_string_lossy()
            .starts_with(&*images.to_string_lossy())
        {
            return Err(Error::plain("Bad image path"));
        }
        Ok(file)
    }

    /// `readImage(url)`.
    pub fn read_image(&self, url: &str) -> Result<Vec<u8>, Error> {
        Ok(std::fs::read(self.image_file(url)?)?)
    }

    /// `attachmentFile(url)`: where an attachment URL is on disk.
    pub fn attachment_file(&self, url: &str) -> Result<PathBuf, Error> {
        let parts: Vec<&str> = url.split('/').collect();
        let attachments = self.folder.attachments();
        let mut file = attachments.clone();
        for part in [parts.get(3), parts.get(4)]
            .into_iter()
            .flatten()
            .filter(|part| !part.is_empty())
        {
            file.push(part);
        }
        let file = normalize(&file);
        let inside = format!("{}{}", attachments.display(), std::path::MAIN_SEPARATOR);
        if !file.to_string_lossy().starts_with(&inside) {
            return Err(Error::plain("Bad attachment path"));
        }
        Ok(file)
    }

    /// `readAttachment(url)`.
    pub fn read_attachment(&self, url: &str) -> Result<Vec<u8>, Error> {
        Ok(std::fs::read(self.attachment_file(url)?)?)
    }

    /// Where any file URL a message names is on disk: `/api/images/...`
    /// through [`Store::image_file`], `/api/attachments/...` through
    /// [`Store::attachment_file`], each with today's refusals.
    pub fn file_path(&self, url: &str) -> Result<PathBuf, Error> {
        match url.split('/').nth(2) {
            Some("images") if url.starts_with("/api/") => self.image_file(url),
            Some("attachments") if url.starts_with("/api/") => self.attachment_file(url),
            _ => Err(Error::plain(format!(
                "{url} is not an image or attachment URL"
            ))),
        }
    }

    /// `saveAttachments(conversationId, files)`: every file decoded and
    /// checked (sizes, then the chat's budget and the disk's reserve) before
    /// any is written, so a refused message leaves no files behind. `files`
    /// are the request body's `{ name, data }` objects.
    pub fn save_attachments(&self, id: &str, files: &[Json]) -> Result<Vec<Attachment>, Error> {
        let decoded = files
            .iter()
            .map(decode_attachment)
            .collect::<Result<Vec<_>, _>>()?;
        let total: u64 = decoded.iter().map(|file| file.bytes.len() as u64).sum();
        if total > MAX_MESSAGE_ATTACHMENT_BYTES {
            return Err(Error::refused("Attachments exceed 200 MB total", 400));
        }
        let dir = self.folder.attachments().join(id);
        let runs = crate::regex!(r"[^a-zA-Z0-9._-]+");
        let saved: Vec<Attachment> = decoded
            .iter()
            .map(|file| {
                let extension = match file.mime.as_str() {
                    "image/jpeg" => ".jpg".to_string(),
                    "image/png" => ".png".to_string(),
                    "image/webp" => ".webp".to_string(),
                    "image/gif" => ".gif".to_string(),
                    _ => extname(&file.name)
                        .chars()
                        .filter(|c| c.is_ascii_alphanumeric() || *c == '.')
                        .take(16)
                        .collect(),
                };
                let base = runs.replace_all(stem(&file.name, extname(&file.name)), "-");
                let base: String = base.trim_matches('-').chars().take(80).collect();
                let base = if base.is_empty() {
                    "file".to_string()
                } else {
                    base
                };
                let stored = format!("{}-{base}{extension}", &self.host.uuid()[..8]);
                Attachment {
                    name: file.name.clone(),
                    mime: file.mime.clone(),
                    size: file.bytes.len() as u64,
                    url: format!("/api/attachments/{id}/{stored}"),
                    path: format!("/work/attachments/{stored}"),
                }
            })
            .collect();
        self.storage.reserve(id, total, true)?;
        let written = (|| -> Result<(), Error> {
            std::fs::create_dir_all(&dir)?;
            for (file, data) in saved.iter().zip(&decoded) {
                std::fs::write(self.attachment_file(&file.url)?, &data.bytes)?;
            }
            Ok(())
        })();
        if let Err(error) = written {
            self.discard_attachments(id, &saved)?;
            return Err(error);
        }
        Ok(saved)
    }

    /// `saveAttachment(conversationId, file)`.
    pub fn save_attachment(&self, id: &str, file: &Json) -> Result<Attachment, Error> {
        Ok(self
            .save_attachments(id, std::slice::from_ref(file))?
            .remove(0))
    }

    /// `discardAttachments(conversationId, saved)`: removes files a message
    /// that was not saved named, gives back their bytes, and removes the
    /// folder when that leaves it empty.
    pub fn discard_attachments(&self, id: &str, saved: &[Attachment]) -> Result<(), Error> {
        for file in saved {
            match std::fs::remove_file(self.attachment_file(&file.url)?) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
            self.storage.refund(id, file.size);
        }
        let dir = self.folder.attachments().join(id);
        if dir.exists() && std::fs::read_dir(&dir)?.next().is_none() {
            std::fs::remove_dir(&dir)?;
        }
        Ok(())
    }

    /// `shareAttachment(attachment, conversationId)`: a round's file, stored
    /// under the bench, copied for chat `id` and counted against its budget.
    pub fn share_attachment(&self, attachment: &Attachment, id: &str) -> Result<Attachment, Error> {
        let name = basename(&attachment.url).to_string();
        let target = self.folder.attachments().join(id).join(&name);
        std::fs::create_dir_all(target.parent().expect("a folder"))?;
        if !target.exists() {
            self.storage.reserve(id, attachment.size, true)?;
            std::fs::copy(self.attachment_file(&attachment.url)?, &target)?;
        }
        Ok(Attachment {
            url: format!("/api/attachments/{id}/{name}"),
            ..attachment.clone()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_reads_as_node_reads_it() {
        for (text, bytes) in [
            ("QUJD", &b"ABC"[..]),
            ("QUJ", b"AB"),
            ("QU", b"A"),
            ("Q", b""),
            ("QUJD=QUJD", b"ABC"),
            ("QU JD", b"ABC"),
            ("QU-_", b"AO\xbf"),
            ("!!QUJD", b"ABC"),
            ("QQ==QUJD", b"A"),
        ] {
            assert_eq!(base64_decode(text), bytes, "{text}");
        }
    }

    #[test]
    fn names_split_as_node_splits_them() {
        for (name, ext, base) in [
            ("a.txt", ".txt", "a"),
            (".txt", "", ".txt"),
            ("a.", ".", "a"),
            ("..", "", ".."),
            ("..a", ".a", "."),
            ("a..b", ".b", "a."),
            (".a.b", ".b", ".a"),
            ("a", "", "a"),
            (".", "", "."),
        ] {
            assert_eq!(extname(name), ext, "{name}");
            assert_eq!(stem(name, extname(name)), base, "{name}");
        }
        assert_eq!(basename("a/b/"), "b");
        assert_eq!(basename("/"), "");
        assert_eq!(basename("a//"), "a");
    }
}
