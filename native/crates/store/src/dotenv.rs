//! `.env` (`server/dotenv.js`): the variables it names that the environment
//! does not set. It holds API keys, so a file the group or other accounts can
//! read or write fails before anything in it is used.

use std::path::Path;

use crate::Error;

/// The variables `.env` at `file` sets, in its order: those `lookup` (the
/// environment, `std::env::var`'s) does not set, each once. No file sets
/// nothing. The caller puts them in the environment (`loadDotEnv`).
pub fn dot_env(
    file: &Path,
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<Vec<(String, String)>, Error> {
    if !file.exists() {
        return Ok(Vec::new());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(file)?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 {
            let folder = file
                .parent()
                .map_or_else(|| ".".into(), |folder| folder.display().to_string());
            return Err(Error::plain(format!(
                "{} can be read or changed by other accounts on this machine (mode {mode:o}). It holds API keys: run `chmod 600 .env` in {folder}, then start the app again.",
                file.display()
            )));
        }
    }
    // JavaScript's `.` stops at every line terminator, `\r` included.
    let line_pattern =
        crate::regex!(r"^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*([^\n\r\u{2028}\u{2029}]*)\s*$");
    let quotes = crate::regex!(r#"^["']|["']$"#);
    let mut set: Vec<(String, String)> = Vec::new();
    for line in std::fs::read_to_string(file)?.split('\n') {
        let Some(found) = line_pattern.captures(line) else {
            continue;
        };
        if line.trim().starts_with('#') {
            continue;
        }
        let name = &found[1];
        if lookup(name).is_none() && !set.iter().any(|(set_name, _)| set_name == name) {
            set.push((
                name.to_string(),
                quotes.replace_all(&found[2], "").into_owned(),
            ));
        }
    }
    Ok(set)
}
