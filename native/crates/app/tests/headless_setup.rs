//! `mcbench-lite headless setup <data folder>` started as a program
//! (`PROTOCOL.md`, family `setup`): its command line, the data folder's
//! lock, and, in a build without the shipped data, the data step's
//! sentence. The steps' work against a download server is
//! `mcbench-headless`' `tests/setup.rs` (the built program's bundle names
//! Mojang's address).

use std::path::PathBuf;
use std::process::{Command, Output};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mcbench-headless-setup-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn setup(words: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mcbench-lite"))
        .arg("headless")
        .arg("setup")
        .args(words)
        .output()
        .unwrap()
}

#[test]
fn a_command_line_without_one_data_folder_is_the_usage_and_status_2() {
    for words in [&[][..], &["a", "b"][..]] {
        let output = setup(words);
        assert_eq!(output.status.code(), Some(2), "{words:?}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            "mcbench-lite headless setup: usage: mcbench-lite headless setup <data folder>\n"
        );
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn a_data_folder_another_process_holds_is_refused_with_status_1() {
    let folder = scratch("held");
    let held = mcbench_setup::InstanceLock::take(&folder).unwrap();
    let output = setup(&[folder.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        format!(
            "mcbench-lite headless setup: {} is held by process {}, which is still running\n",
            folder.join("server.pid").display(),
            std::process::id()
        )
    );
    assert!(output.stdout.is_empty());
    assert!(!folder.join("minecraft").exists());
    drop(held);
    std::fs::remove_dir_all(&folder).unwrap();
}

#[cfg(not(feature = "shipped-data"))]
#[test]
fn a_build_without_the_shipped_data_fails_the_data_step_with_its_sentence_and_status_1() {
    let folder = scratch("unshipped");
    let output = setup(&[folder.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "mcbench-lite headless setup: Minecraft's data: running\n\
         mcbench-lite headless setup: Minecraft's data: failed\n\
         mcbench-lite headless setup: This build has no shipped Minecraft data: build it with --features shipped-data and MCBENCH_DATA_BUNDLE naming the bundle\n"
    );
    assert!(output.stdout.is_empty());
    assert!(
        !folder.join("server.pid").exists(),
        "the folder's lock is let go"
    );
    std::fs::remove_dir_all(&folder).unwrap();
}
