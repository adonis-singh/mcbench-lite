//! The one place stored addresses become files: every stored picture (a
//! `see_build` shots row, an attached image, a markdown image whose address
//! is one of these) goes through [`picture`], every attachment chip through
//! [`attachment`]. Stored user messages hold `/api/images/...` and
//! `/api/attachments/...` addresses; the core answers each with a file or
//! its sentence (`Image not found`, `Attachment not found`), which shows in
//! place of the picture or on the chip. A streamed result's pictures are
//! files already; a path that starts with `/api/images/` is an address in
//! disguise and goes through the core too.

use std::path::{Path, PathBuf};

use crate::{AttachmentFile, Core};

/// The stored pictures' address prefix.
pub const IMAGES: &str = "/api/images/";
/// The stored attachments' address prefix.
pub const ATTACHMENTS: &str = "/api/attachments/";

/// The file to draw for a picture's path or address.
pub fn picture(path: &Path, core: &dyn Core) -> Result<PathBuf, String> {
    match path.to_str() {
        Some(address) if address.starts_with(IMAGES) => core.image_file(address),
        _ => Ok(path.to_path_buf()),
    }
}

/// The file of an attachment's address.
pub fn attachment(address: &str, core: &dyn Core) -> Result<AttachmentFile, String> {
    assert!(
        address.starts_with(ATTACHMENTS),
        "an attachment's address starts with {ATTACHMENTS}: {address}"
    );
    core.attachment_file(address)
}
