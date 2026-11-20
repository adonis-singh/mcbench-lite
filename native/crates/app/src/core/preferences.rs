//! The window's preferences (`mcbench_ui::Preferences`): `preferences.json`
//! in the data folder, a file of its own (not `settings.json`), whose text
//! is the interface's (`Preferences::to_text`, `Preferences::from_text`);
//! the core only reads and writes it. Today they are the page's
//! `localStorage` keys, which no route serves.
//!
//! No file is the first run: the defaults. A file that cannot be read or
//! does not parse is an error naming it, then the defaults; the file is
//! left as it is until the next save. A save writes the whole text to a
//! temporary file in the same folder and renames it into place, so a crash
//! never leaves half a file.

use std::path::{Path, PathBuf};

use mcbench_ui::Preferences;

/// `preferences.json` of the data folder `data`.
pub fn file(data: &Path) -> PathBuf {
    data.join("preferences.json")
}

/// The preferences kept in `file`, and the error to show when they could
/// not be read (the defaults are given then).
pub fn read(file: &Path) -> (Preferences, Option<String>) {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Preferences::default(), None);
        }
        Err(error) => {
            return (
                Preferences::default(),
                Some(unread(file, &error.to_string())),
            );
        }
    };
    match Preferences::from_text(&text) {
        Ok(preferences) => (preferences, None),
        Err(error) => (Preferences::default(), Some(unread(file, &error))),
    }
}

fn unread(file: &Path, error: &str) -> String {
    format!(
        "The window's preferences in {} could not be read, so the defaults are used until they are saved again: {error}",
        file.display()
    )
}

/// Writes `preferences` to `file` whole: a temporary file beside it, then a
/// rename. The error names the file.
pub fn save(file: &Path, preferences: &Preferences) -> Result<(), String> {
    let temporary = PathBuf::from(format!("{}.{}.tmp", file.display(), std::process::id()));
    let written = std::fs::write(&temporary, preferences.to_text())
        .and_then(|()| std::fs::rename(&temporary, file));
    written.map_err(|error| {
        // A temporary file left by a failed rename goes; a failed write
        // made none, or one that is removed the same way.
        let _ = std::fs::remove_file(&temporary);
        format!(
            "The window's preferences could not be saved to {}: {error}",
            file.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("mcbench-preferences-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    #[test]
    fn no_file_is_the_first_run_and_gives_the_defaults() {
        let folder = folder("first");
        assert_eq!(read(&file(&folder)), (Preferences::default(), None));
        assert!(!file(&folder).exists(), "reading makes no file");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_saved_file_reads_back_as_it_was_saved() {
        let folder = folder("saved");
        let preferences = Preferences {
            light: true,
            sidebar_width: 301.5,
            chat_width: Some(480.0),
            last_model: Some(("openrouter".into(), "own/a".into())),
            place: Some("/chat/abc".into()),
            open_benches: vec!["b1".into()],
            prompts_open: false,
            ..Preferences::default()
        };
        save(&file(&folder), &preferences).unwrap();
        assert_eq!(
            std::fs::read_to_string(file(&folder)).unwrap(),
            preferences.to_text()
        );
        assert_eq!(read(&file(&folder)), (preferences, None));
        let left: Vec<_> = std::fs::read_dir(&folder)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(left, ["preferences.json"], "no temporary file is left");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error_naming_it_and_the_defaults_and_stays() {
        let folder = folder("broken");
        let path = file(&folder);
        std::fs::write(&path, "{\"light\": tru").unwrap();
        let (preferences, error) = read(&path);
        assert_eq!(preferences, Preferences::default());
        let error = error.expect("an error");
        assert!(
            error.starts_with(&format!(
                "The window's preferences in {} could not be read, so the defaults are used until they are saved again: the preferences are not JSON: ",
                path.display()
            )),
            "{error}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{\"light\": tru");
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_save_into_a_read_only_folder_fails_naming_the_file_and_leaves_nothing() {
        use std::os::unix::fs::PermissionsExt as _;
        let folder = folder("read-only");
        let path = file(&folder);
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o555)).unwrap();
        let saved = save(&path, &Preferences::default());
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = saved.unwrap_err();
        assert!(
            error.starts_with(&format!(
                "The window's preferences could not be saved to {}: ",
                path.display()
            )),
            "{error}"
        );
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 0);
        std::fs::remove_dir_all(&folder).unwrap();
    }
}
