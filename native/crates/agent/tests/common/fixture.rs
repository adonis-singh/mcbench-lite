//! The one rule for what of a fixture the tests may see. A fixture is a
//! clone of a real data folder, whose `settings.json`, `auth-token` and
//! `backups/` hold real provider keys and the app's token: the tests are
//! handed only a fixture cloned without them (`guard` refuses any other),
//! clone it without them (`clone`), and write their own made-up settings
//! (`write_made_up_settings`). Every crate's differential includes this file
//! (`#[path]`), so the rule is in one place.

#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

/// The entries of a data folder that hold keys or the app's token.
pub const LEFT_OUT: [&str; 3] = ["settings.json", "auth-token", "backups"];

/// Fails the test when `folder` holds one of `LEFT_OUT` at its top level.
/// Every set-up that is handed a fixture calls it before it opens anything.
pub fn guard(folder: &Path) {
    let entries = std::fs::read_dir(folder)
        .unwrap_or_else(|e| panic!("the fixture {} cannot be listed: {e}", folder.display()));
    for entry in entries {
        let name = entry
            .unwrap_or_else(|e| panic!("the fixture {} cannot be listed: {e}", folder.display()))
            .file_name();
        if LEFT_OUT.iter().any(|left| name == *left) {
            panic!(
                "the fixture {} holds {}: hand the tests a fixture cloned without settings.json, auth-token and backups",
                folder.display(),
                name.to_string_lossy()
            );
        }
    }
}

/// A copy of `from` at `to`, entry by entry (copy-on-write where the disk
/// can), without `LEFT_OUT`.
pub fn clone(from: &Path, to: &Path) {
    if to.exists() {
        std::fs::remove_dir_all(to).unwrap();
    }
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if LEFT_OUT.iter().any(|left| name == *left) {
            continue;
        }
        let mut command = Command::new("cp");
        if cfg!(target_os = "macos") {
            command.arg("-c").arg("-R");
        } else {
            command.arg("-r").arg("--reflink=always");
        }
        let status = command
            .arg(entry.path())
            .arg(to.join(&name))
            .status()
            .unwrap();
        assert!(status.success(), "cp {}", entry.path().display());
    }
}

/// The settings the tests write into their clones: invented values with no
/// provider's key shape, several providers (one with an empty key), a title
/// model, starred models, hidden models and a model order.
pub const MADE_UP_SETTINGS: &str = r#"{
  "providers": {
    "openrouter": {
      "apiKey": "made-up-1",
      "enabled": true,
      "hiddenModels": ["made-up/hidden-a", "made-up/hidden-b"],
      "modelOrder": ["made-up/order-b", "made-up/order-a"]
    },
    "openai": {
      "apiKey": "made-up-2",
      "enabled": false,
      "hiddenModels": [],
      "modelOrder": []
    },
    "anthropic": {
      "apiKey": "",
      "enabled": true,
      "hiddenModels": ["made-up-hidden-c"],
      "modelOrder": []
    },
    "groq": {
      "apiKey": "made-up-3",
      "enabled": true,
      "hiddenModels": [],
      "modelOrder": ["made-up-order-c"]
    }
  },
  "titleModel": { "provider": "openrouter", "model": "made-up/title", "effort": "low" },
  "modelFavorites": ["openrouter::made-up/fav-a", "anthropic::made-up-fav-b"],
  "shareRepo": "made-up/builds"
}
"#;

/// Writes `MADE_UP_SETTINGS` as `folder`'s `settings.json`.
pub fn write_made_up_settings(folder: &Path) {
    std::fs::write(folder.join("settings.json"), MADE_UP_SETTINGS).unwrap();
}
