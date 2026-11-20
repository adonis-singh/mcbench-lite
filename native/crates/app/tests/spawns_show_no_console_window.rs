//! Every process a crate of the app starts is made by
//! `mcbench_spawn::command`, which sets `CREATE_NO_WINDOW` on Windows: a
//! console program started by the app (a window program there) without it
//! opens a console window of its own. A source scan, so it runs on every
//! platform and a spawn added anywhere fails the check before it lands.

use std::path::{Path, PathBuf};

/// Every `.rs` file under `folder`.
fn sources(folder: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_process_the_app_starts_goes_through_mcbench_spawn_so_none_opens_a_console_window() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    // Not the app's stage-three crates: the interface's (mcbench-ui) and the
    // updater's (mcbench-updater) are other sessions'; mcbench-spawn is the
    // helper itself.
    let others = ["ui", "updater", "spawn"];
    let mut files = Vec::new();
    for entry in std::fs::read_dir(&crates).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap().to_string();
        if path.join("src").is_dir() && !others.contains(&name.as_str()) {
            sources(&path.join("src"), &mut files);
        }
    }
    assert!(
        files.len() > 100,
        "the crates' sources under {}",
        crates.display()
    );
    let mut bypassing = Vec::new();
    for file in &files {
        // A module file named tests.rs is a #[cfg(test)] module; a file's
        // own test module is from `#[cfg(test)]` before `mod ` to its end.
        if file.file_name().unwrap() == "tests.rs" {
            continue;
        }
        let text = std::fs::read_to_string(file).unwrap();
        let text = text.replace("\r\n", "\n");
        let code = match text.find("#[cfg(test)]\nmod ") {
            Some(at) => &text[..at],
            None => &text[..],
        };
        for (n, line) in code.lines().enumerate() {
            if line.contains("Command::new(") || line.contains("CreateProcess") {
                bypassing.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        bypassing.is_empty(),
        "these start a process without mcbench_spawn::command, so on Windows a console program they start opens a console window:\n{}",
        bypassing.join("\n")
    );
}
