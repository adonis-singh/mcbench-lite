//! What a failed Codex box left in a chat's folder, for a test's failure
//! message: the proxy's log (outside CODEX_HOME) and, for CODEX_HOME, each
//! file's name, size and mode only. Nothing is read from inside CODEX_HOME:
//! it holds `auth.json`, a person's login with its tokens.

use std::path::Path;

/// Every file and folder under `dir`: its path under `dir`, its size and its
/// mode, sorted; nothing read from inside them.
pub fn listing(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(folder) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path.clone());
            }
            #[cfg(unix)]
            let mode = format!(
                "{:o}",
                std::os::unix::fs::PermissionsExt::mode(&meta.permissions()) & 0o7777
            );
            #[cfg(not(unix))]
            let mode = if meta.permissions().readonly() {
                "read-only".to_owned()
            } else {
                "writable".to_owned()
            };
            let kind = if meta.is_symlink() {
                "link"
            } else if meta.is_dir() {
                "folder"
            } else {
                "file"
            };
            out.push(format!(
                "{} ({kind}, {} bytes, {mode})",
                path.strip_prefix(dir).unwrap_or(&path).display(),
                meta.len()
            ));
        }
    }
    out.sort();
    out
}

/// The failure dump of the chat folder `chat`.
pub fn of_chat(chat: &Path) -> String {
    format!(
        "egress.ndjson: {}\ncodex-home: {:?}",
        std::fs::read_to_string(chat.join("egress.ndjson")).unwrap_or_else(|e| e.to_string()),
        listing(&chat.join("codex-home"))
    )
}

/// A CODEX_HOME holding a stand-in login whose content is a marker, and a
/// record that quotes it: the dump names both files and holds no marker.
pub async fn a_failure_dump_never_holds_the_login() {
    let chat = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("dump-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&chat);
    let marker = "MARKER-token-6c1f0e";
    std::fs::create_dir_all(chat.join("codex-home/fake-record")).unwrap();
    std::fs::write(chat.join("codex-home/auth.json"), marker).unwrap();
    std::fs::write(
        chat.join("codex-home/fake-record/record.jsonl"),
        format!("{{\"auth.json\":\"{marker}\"}}"),
    )
    .unwrap();
    std::fs::write(
        chat.join("egress.ndjson"),
        "{\"target\":\"chatgpt.com:443\"}",
    )
    .unwrap();
    let dump = of_chat(&chat);
    assert!(!dump.contains(marker), "the dump read a file of CODEX_HOME");
    assert!(dump.contains("auth.json (file, 19 bytes"), "{dump}");
    assert!(dump.contains("record.jsonl (file"), "{dump}");
    std::fs::remove_dir_all(&chat).unwrap();
}
