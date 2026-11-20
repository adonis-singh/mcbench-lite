//! `headless setup`'s work (`mcbench_headless::setup::prepare`, which the
//! family runs) against a download server on loopback, the setup crate's
//! own (`crates/setup/tests/common`): the shipped data here is a bundle of
//! one material whose client-jar manifest names that server, unpacked by
//! `Data::unpack` as the binary's shipped data is. The binary itself can
//! only reach Mojang (its bundle is the lock's, and its manifest names
//! Mojang's address): `mcbench-lite`'s `headless_setup` starts it for the
//! command line, the lock and a build without the shipped data.

#[path = "../../setup/tests/common/mod.rs"]
mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use common::{names, scratch, serve, tiny_bundle};
use mcbench_headless::setup::{DataSource, Say, prepare};
use mcbench_setup::Data;
use mcbench_setup::bundle::Lock;

fn source(bundle: &[u8], lock: &Lock, folder: &std::path::Path) -> DataSource {
    let (bundle, lock, folder) = (bundle.to_vec(), lock.clone(), folder.to_path_buf());
    Box::new(move || Data::unpack(&bundle, &lock, &folder))
}

/// The shell step's lines (`Model's shell: …`, which a test build without
/// the sandbox's files ends unavailable) apart from the others.
fn shell_apart(lines: Vec<String>) -> (Vec<String>, Vec<String>) {
    lines
        .into_iter()
        .partition(|l| l.starts_with("Model's shell: "))
}

/// The lines said, collected.
fn said() -> (Arc<Mutex<Vec<String>>>, Say) {
    let lines = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&lines);
    (
        lines,
        Arc::new(move |line: &str| into.lock().unwrap().push(line.to_string())),
    )
}

#[tokio::test]
async fn a_fresh_folder_gets_both_folders_and_the_line_and_a_second_run_fetches_nothing() {
    let folder = scratch("headless-setup-fresh");
    let server = serve(HashMap::new()).await;
    let (bundle, lock, jar) = tiny_bundle(&format!("{}/client.jar", server.url));
    server
        .state
        .lock()
        .unwrap()
        .files
        .insert("/client.jar".into(), jar);
    let id = format!("1.21.10-{}", &lock.sha256[..16]);
    let (lines, say) = said();
    let line = prepare(
        &folder,
        source(&bundle, &lock, &folder),
        &common::client(),
        say,
    )
    .await
    .unwrap();
    let tables = folder.join("minecraft").join(&id);
    let textures = folder.join("textures").join(&id);
    assert_eq!(
        line,
        mcbench_json::stringify(&serde_json::json!({
            "tables": tables.display().to_string(),
            "textures": textures.display().to_string(),
        }))
    );
    assert_eq!(
        names(&textures),
        [
            "atlas.png",
            "atlas_map.json",
            "client-entries.zip",
            "textures.json"
        ]
    );
    assert!(tables.join("client-jar.json").is_file());
    assert_eq!(names(&folder.join("textures")), [id.as_str()]);
    assert!(
        !folder.join("server.pid").exists(),
        "the folder's lock is let go"
    );
    let (shell, lines) = shell_apart(lines.lock().unwrap().clone());
    assert_eq!(shell[0], "Model's shell: running", "{shell:?}");
    assert!(
        shell.len() == 2
            && (shell[1].starts_with("Model's shell: done in ")
                || shell[1].starts_with("Model's shell: unavailable: ")),
        "{shell:?}"
    );
    assert_eq!(lines[0], "Minecraft's data: running");
    assert!(
        lines[1].starts_with("Minecraft's data: done in "),
        "{lines:?}"
    );
    assert_eq!(lines[2], "Minecraft's textures: running");
    assert!(
        lines[3..lines.len() - 1]
            .iter()
            .all(|l| l.starts_with("Minecraft's textures: running, ") && l.ends_with(" bytes")),
        "{lines:?}"
    );
    assert!(
        lines[lines.len() - 1].starts_with("Minecraft's textures: done in "),
        "{lines:?}"
    );
    let asked = server.state.lock().unwrap().requests.len();
    assert!(asked > 0);
    // Again on the same folder: both are there, nothing is fetched, and the
    // line is the same.
    let (again, say) = said();
    assert_eq!(
        prepare(
            &folder,
            source(b"not a bundle", &lock, &folder),
            &common::client(),
            say
        )
        .await
        .unwrap(),
        line
    );
    assert_eq!(server.state.lock().unwrap().requests.len(), asked);
    let (_, again) = shell_apart(again.lock().unwrap().clone());
    assert_eq!(again.len(), 4, "{again:?}");
    assert!(again[3].starts_with("Minecraft's textures: done in "));
}

#[tokio::test]
async fn a_failed_fetch_is_the_steps_sentence() {
    let folder = scratch("headless-setup-failed");
    let server = serve(HashMap::new()).await;
    let url = format!("{}/client.jar", server.url);
    let (bundle, lock, jar) = tiny_bundle(&url);
    let (lines, say) = said();
    let error = prepare(
        &folder,
        source(&bundle, &lock, &folder),
        &common::client(),
        say,
    )
    .await
    .unwrap_err();
    let entry = &mcbench_setup::jar::entries(&jar).unwrap()[0];
    assert_eq!(
        error,
        format!(
            "The download of Minecraft's textures from {url} failed: HTTP 404 for the range 0-{}",
            entry.data + entry.stored - 1
        )
    );
    assert_eq!(
        lines.lock().unwrap().last().unwrap(),
        "Minecraft's textures: failed"
    );
    assert!(
        !folder
            .join("textures")
            .join(format!("1.21.10-{}", &lock.sha256[..16]))
            .exists()
    );
}

#[tokio::test]
async fn a_held_data_folder_is_refused_and_nothing_is_unpacked() {
    let folder = scratch("headless-setup-held");
    let (bundle, lock, _) = tiny_bundle("http://127.0.0.1:9/client.jar");
    let _held = mcbench_setup::InstanceLock::take(&folder).unwrap();
    let (lines, say) = said();
    let error = prepare(
        &folder,
        source(&bundle, &lock, &folder),
        &common::client(),
        say,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        format!(
            "{} is held by process {}, which is still running",
            folder.join("server.pid").display(),
            std::process::id()
        )
    );
    assert!(lines.lock().unwrap().is_empty());
    assert!(!folder.join("minecraft").exists());
}
