//! The model's shell's pinned assets fetched from stand-in files served on
//! loopback: fetched and checked into a fresh folder, nothing fetched the
//! second time, nothing kept of a fetch that failed or was stopped, this
//! architecture's lines taken, redirects to another host followed, and the
//! environment's four variables read only by a build that is not shipped.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::{Answer, Server, client, names, scratch, serve, sha256};
use mcbench_setup::shell::{Build, NOT_INCLUDED, ShellFiles, files_of, kept_in};
use mcbench_setup::steps::{StepError, StepState, Steps, native_steps};
use tokio_util::sync::CancellationToken;

/// A stand-in file of `n` bytes, different for each `seed`.
fn bytes(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31) ^ seed).collect()
}

/// A lock line's kind, architecture and file name, and the file's bytes.
type Line = (String, String, String, Vec<u8>);

/// The stand-in assets: each served path and its bytes, and the lock lines.
fn assets() -> (Vec<(String, Vec<u8>)>, Vec<Line>) {
    let mut lines = Vec::new();
    for (seed, (kind, arch)) in [
        ("rootfs", "x86_64"),
        ("rootfs", "aarch64"),
        ("guest-base", "x86_64"),
        ("guest-base", "aarch64"),
    ]
    .into_iter()
    .enumerate()
    {
        let name = format!("{kind}-{arch}.tar.zst");
        lines.push((
            kind.to_string(),
            arch.to_string(),
            name,
            bytes(200_000 + seed * 1000, seed as u8),
        ));
    }
    let files = lines
        .iter()
        .map(|(_, _, name, b)| (format!("/release/{name}"), b.clone()))
        .collect();
    (files, lines)
}

/// The lock text of `lines` with `url`; `wrong` changes a line's sha256 or
/// size.
fn lock(lines: &[Line], url: &str, wrong: Wrong) -> String {
    let mut text = String::from("# a test lock\n");
    for (kind, arch, name, b) in lines {
        let (mut hash, mut size) = (sha256(b), b.len() as u64);
        match wrong {
            Wrong::Sha256 => hash = sha256(b"something else"),
            Wrong::Size => size += 1,
            Wrong::Smaller => size -= 1,
            Wrong::No => {}
        }
        text += &format!("{kind}\t{arch}\t{name}\t{hash}\t{size}\n");
    }
    text + &format!("url\t{url}\n")
}

#[derive(Clone, Copy)]
enum Wrong {
    No,
    Sha256,
    Size,
    Smaller,
}

fn steps() -> Arc<Steps> {
    let steps = Arc::new(Steps::new(native_steps(), Box::new(|_| {})));
    running(&steps);
    steps
}

fn running(steps: &Steps) {
    steps
        .set(
            "shell",
            StepState::Running {
                detail: None,
                progress: None,
            },
            None,
        )
        .unwrap();
}

/// A variable reader that fails the test when called.
fn no_var(name: &str) -> Option<String> {
    panic!("a shipped build read the variable {name}")
}

/// The four variables, naming `/r` and `/g`.
fn four(name: &str) -> Option<String> {
    Some(
        match name {
            "MCBENCH_SANDBOX_ROOTFS" => "/r",
            "MCBENCH_SANDBOX_ROOTFS_SHA256" => "aa",
            "MCBENCH_SANDBOX_GUEST" => "/g",
            "MCBENCH_SANDBOX_GUEST_SHA256" => "bb",
            _ => return None,
        }
        .to_string(),
    )
}

async fn run(
    lock: &str,
    address: Option<&str>,
    arch: &str,
    folder: &std::path::Path,
    steps: Arc<Steps>,
    cancel: CancellationToken,
) -> Result<ShellFiles, StepError> {
    let build = Build {
        shipped: true,
        lock,
        address,
        arch,
    };
    files_of(
        &build,
        &no_var,
        steps,
        folder.to_path_buf(),
        client(),
        cancel,
    )
    .await
}

fn requests(server: &Server) -> Vec<String> {
    let state = server.state.lock().unwrap();
    state.requests.iter().map(|(p, _)| p.clone()).collect()
}

/// The files this platform takes for `arch`.
fn wanted(arch: &str) -> Vec<String> {
    let mut list = vec![format!("/release/rootfs-{arch}.tar.zst")];
    if !cfg!(target_os = "linux") {
        list.push(format!("/release/guest-base-{arch}.tar.zst"));
    }
    list
}

#[tokio::test]
async fn a_fresh_folder_fetches_and_checks_and_a_second_call_fetches_nothing() {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect()).await;
    let dir = scratch("shell-fresh");
    let lock = lock(&lines, "not-published-yet", Wrong::No);
    let address = format!("{}/release", server.url);
    for arch in ["x86_64", "aarch64"] {
        let got = run(
            &lock,
            Some(&address),
            arch,
            &dir,
            steps(),
            CancellationToken::new(),
        )
        .await
        .unwrap();
        let rootfs = &lines
            .iter()
            .find(|l| l.0 == "rootfs" && l.1 == arch)
            .unwrap()
            .3;
        assert_eq!(got.rootfs.sha256, sha256(rootfs));
        assert_eq!(std::fs::read(&got.rootfs.file).unwrap(), *rootfs);
        assert!(got.rootfs.file.starts_with(kept_in(&dir)));
        #[cfg(not(target_os = "linux"))]
        {
            let guest = &lines
                .iter()
                .find(|l| l.0 == "guest-base" && l.1 == arch)
                .unwrap()
                .3;
            assert_eq!(std::fs::read(&got.guest.file).unwrap(), *guest);
        }
    }
    let first: Vec<String> = wanted("x86_64")
        .into_iter()
        .chain(wanted("aarch64"))
        .collect();
    assert_eq!(requests(&server), first);
    // Kept: a second start asks the server for nothing.
    let again = run(
        &lock,
        Some(&address),
        "x86_64",
        &dir,
        steps(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(requests(&server), first);
    assert!(again.rootfs.file.is_file());
    assert!(!names(&kept_in(&dir)).iter().any(|n| n.ends_with(".part")));
}

#[tokio::test]
async fn the_progress_is_the_bytes_of_this_platforms_files() {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect()).await;
    let dir = scratch("shell-progress");
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let record = seen.clone();
    let steps = Arc::new(Steps::new(
        native_steps(),
        Box::new(move |list| {
            if let StepState::Running {
                progress: Some(p), ..
            } = &list[2].state
            {
                record.lock().unwrap().push(*p);
            }
        }),
    ));
    running(&steps);
    let lock = lock(&lines, "not-published-yet", Wrong::No);
    run(
        &lock,
        Some(&format!("{}/release", server.url)),
        "x86_64",
        &dir,
        steps,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let total: u64 = lines
        .iter()
        .filter(|l| l.1 == "x86_64" && (l.0 == "rootfs" || !cfg!(target_os = "linux")))
        .map(|l| l.3.len() as u64)
        .sum();
    let seen = seen.lock().unwrap();
    assert_eq!(seen.last().unwrap().done, total);
    assert!(seen.iter().all(|p| p.total == Some(total)));
}

/// `answer` for the first request, then the files served: the fetch fails
/// with `cause` in the shell step's sentence and keeps nothing.
async fn fails(name: &str, wrong: Wrong, answer: Option<Answer>, cause: &str) {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect::<HashMap<_, _>>()).await;
    if let Some(answer) = answer {
        server.state.lock().unwrap().script.push(answer);
    }
    let dir = scratch(name);
    let address = format!("{}/release", server.url);
    let lock = lock(&lines, "not-published-yet", wrong);
    let error = run(
        &lock,
        Some(&address),
        "x86_64",
        &dir,
        steps(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        StepError::Failed(format!(
            "The download of the model's shell from {address}/rootfs-x86_64.tar.zst failed: {cause}; try again"
        ))
    );
    assert_eq!(names(&kept_in(&dir)), Vec::<String>::new());
}

#[tokio::test]
async fn a_wrong_sha256_fails_with_the_sentence_and_keeps_nothing() {
    let (_, lines) = assets();
    let got = sha256(&lines[0].3);
    let pinned = sha256(b"something else");
    fails(
        "shell-sha",
        Wrong::Sha256,
        None,
        &format!("its sha256 is {got}, not the pinned {pinned}"),
    )
    .await;
}

#[tokio::test]
async fn a_wrong_size_fails_with_the_sentence_and_keeps_nothing() {
    let (_, lines) = assets();
    let n = lines[0].3.len();
    fails(
        "shell-size",
        Wrong::Size,
        None,
        &format!("it ended after {n} of {} bytes", n + 1),
    )
    .await;
    fails(
        "shell-size-smaller",
        Wrong::Smaller,
        None,
        &format!("it is longer than {} bytes", n - 1),
    )
    .await;
}

#[tokio::test]
async fn a_404_fails_with_the_sentence_and_keeps_nothing() {
    fails(
        "shell-404",
        Wrong::No,
        Some(Answer::Status(404)),
        "HTTP 404",
    )
    .await;
}

#[tokio::test]
async fn a_cut_connection_fails_with_the_sentence_and_keeps_nothing() {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect()).await;
    server.state.lock().unwrap().script.push(Answer::Short);
    let dir = scratch("shell-cut");
    let address = format!("{}/release", server.url);
    let lock = lock(&lines, "not-published-yet", Wrong::No);
    let error = run(
        &lock,
        Some(&address),
        "x86_64",
        &dir,
        steps(),
        CancellationToken::new(),
    )
    .await
    .unwrap_err();
    let StepError::Failed(sentence) = error else {
        panic!("{error:?}")
    };
    let prefix =
        format!("The download of the model's shell from {address}/rootfs-x86_64.tar.zst failed: ");
    assert!(sentence.starts_with(&prefix), "{sentence}");
    assert!(sentence.ends_with("; try again"), "{sentence}");
    assert_eq!(names(&kept_in(&dir)), Vec::<String>::new());
    // The next start fetches from the first byte (no range request) and
    // keeps the file.
    run(
        &lock,
        Some(&address),
        "x86_64",
        &dir,
        steps(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let state = server.state.lock().unwrap();
    assert!(state.requests.iter().all(|(_, range)| range.is_none()));
}

#[tokio::test]
async fn a_stopped_fetch_keeps_nothing() {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect()).await;
    server.state.lock().unwrap().script.push(Answer::Stall);
    let dir = scratch("shell-stop");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let steps = Arc::new(Steps::new(
        native_steps(),
        Box::new(move |list| {
            if let StepState::Running {
                progress: Some(p), ..
            } = &list[2].state
                && p.done > 0
            {
                let _ = tx.send(());
            }
        }),
    ));
    running(&steps);
    let cancel = CancellationToken::new();
    let address = format!("{}/release", server.url);
    let lock = lock(&lines, "not-published-yet", Wrong::No);
    let fetch = tokio::spawn({
        let (lock, address, dir, cancel) = (
            lock.clone(),
            address.clone(),
            dir.path_buf(),
            cancel.clone(),
        );
        async move { run(&lock, Some(&address), "x86_64", &dir, steps, cancel).await }
    });
    // Bytes arrived, and the part file holds them: stop it.
    rx.recv().await.unwrap();
    assert!(names(&kept_in(&dir)).iter().any(|n| n.ends_with(".part")));
    cancel.cancel();
    assert_eq!(
        fetch.await.unwrap().unwrap_err(),
        StepError::Failed(format!(
            "The download of the model's shell from {address}/rootfs-x86_64.tar.zst failed: it was stopped; try again"
        ))
    );
    assert_eq!(names(&kept_in(&dir)), Vec::<String>::new());
}

#[tokio::test]
async fn a_redirect_to_another_host_is_followed() {
    let (files, lines) = assets();
    let second = serve(files.into_iter().collect()).await;
    let first = serve(HashMap::new()).await;
    first.state.lock().unwrap().script.extend(
        wanted("x86_64")
            .iter()
            .map(|_| Answer::Redirect(second.url.clone())),
    );
    let dir = scratch("shell-redirect");
    let lock = lock(&lines, "not-published-yet", Wrong::No);
    let got = run(
        &lock,
        Some(&format!("{}/release", first.url)),
        "x86_64",
        &dir,
        steps(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&got.rootfs.file).unwrap(), lines[0].3);
    assert_eq!(requests(&first), wanted("x86_64"));
    assert_eq!(requests(&second), wanted("x86_64"));
}

#[tokio::test]
async fn the_locks_url_is_used_without_a_build_address_and_unpublished_means_no_shell() {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect()).await;
    let dir = scratch("shell-url");
    let published = lock(&lines, &format!("{}/release", server.url), Wrong::No);
    run(
        &published,
        None,
        "x86_64",
        &dir,
        steps(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(requests(&server), wanted("x86_64"));
    let unpublished = lock(&lines, "not-published-yet", Wrong::No);
    assert_eq!(
        run(
            &unpublished,
            None,
            "x86_64",
            &dir,
            steps(),
            CancellationToken::new()
        )
        .await
        .unwrap_err(),
        StepError::Unavailable(NOT_INCLUDED.into())
    );
    // An architecture the release has no assets for has no shell either.
    assert_eq!(
        run(
            &published,
            None,
            "riscv64",
            &dir,
            steps(),
            CancellationToken::new()
        )
        .await
        .unwrap_err(),
        StepError::Unavailable(NOT_INCLUDED.into())
    );
    assert_eq!(requests(&server), wanted("x86_64"));
}

#[tokio::test]
async fn only_a_build_that_is_not_shipped_reads_the_four_variables() {
    let (files, lines) = assets();
    let server = serve(files.into_iter().collect()).await;
    let dir = scratch("shell-vars");
    let lock = lock(&lines, &format!("{}/release", server.url), Wrong::No);
    let build = |shipped| Build {
        shipped,
        lock: &lock,
        address: None,
        arch: "x86_64",
    };
    // Shipped: `no_var` panics if it is called; the files are fetched.
    let shipped = files_of(
        &build(true),
        &no_var,
        steps(),
        dir.path_buf(),
        client(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert!(shipped.rootfs.file.starts_with(kept_in(&dir)));
    // Not shipped: the variables' files, nothing fetched.
    let named = files_of(
        &build(false),
        &four,
        steps(),
        dir.path_buf(),
        client(),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(named.rootfs.file, std::path::PathBuf::from("/r"));
    assert_eq!(named.rootfs.sha256, "aa");
    #[cfg(not(target_os = "linux"))]
    assert_eq!(named.guest.file, std::path::PathBuf::from("/g"));
    assert_eq!(
        files_of(
            &build(false),
            &|_| None,
            steps(),
            dir.path_buf(),
            client(),
            CancellationToken::new(),
        )
        .await
        .unwrap_err(),
        StepError::Unavailable(NOT_INCLUDED.into())
    );
    assert_eq!(requests(&server), wanted("x86_64"));
}
