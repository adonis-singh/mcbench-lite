//! Shared builds ([`mcbench_tools::share`]): today's `test/share.test.js`,
//! every test by name. A chat's build is published to a GitHub Pages
//! repository's working tree and pushed. The remote is a local bare
//! repository, `gh` is a fake, GitHub Pages is a local HTTP server that
//! serves what a remote holds, and the export (the settle and the preview
//! shot) and the viewer's files are given.
//!
//! Unix only: every test drives the real `git` program against those local
//! repositories, and the Windows check's guest has no `git` (the first
//! Windows half on this file failed 18 tests with "program not found"). On
//! Windows the module's own code is compiled and linted with the crate, and
//! the core's share commands are tested on a stand-in for `git`
//! (`mcbench-lite`'s `tests/core_share.rs`); the site's real git work is not
//! run there.
#![cfg(unix)]

use std::collections::HashMap;
use std::future::Future;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use mcbench_render::Cell;
use mcbench_tools::share::{
    BenchSource, Bounds, Code, Export, Exported, Failure, Options, Processes, Publish, PublishStep,
    Ran, Run, ShareError, SharePage, Shares, SiteViewer, Source, Subject, bench_label, share_id,
};
use mcbench_tools::shots::montage_cells;
use mcbench_tools::verify::random_uuid;
use mcbench_world::{BlockState, World};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// The scratch folder and git as it is on a machine with no configuration of
// its own: no signing, no hooks of the developer's.

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let dir = std::env::temp_dir().join(format!("mcbench-share-test-{}", random_uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("gitconfig"), "").unwrap();
        Scratch(dir)
    }

    fn env(&self) -> Vec<(String, String)> {
        [
            (
                "GIT_CONFIG_GLOBAL",
                self.0.join("gitconfig").display().to_string(),
            ),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("GIT_AUTHOR_NAME", "Test".into()),
            ("GIT_AUTHOR_EMAIL", "test@example.invalid".into()),
            ("GIT_COMMITTER_NAME", "Test".into()),
            ("GIT_COMMITTER_EMAIL", "test@example.invalid".into()),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
    }

    fn git_bytes(&self, args: &[&str]) -> Vec<u8> {
        let output = std::process::Command::new("git")
            .args(args)
            .envs(self.env())
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn git(&self, args: &[&str]) -> String {
        String::from_utf8(self.git_bytes(args)).unwrap()
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A bare repository with one commit on main, as a repository GitHub Pages
/// serves from has.
fn remote(scratch: &Scratch, n: usize) -> PathBuf {
    let bare = scratch.0.join(format!("remote-{n}.git"));
    let b = bare.to_str().unwrap();
    scratch.git(&["init", "--quiet", "--bare", "-b", "main", b]);
    let seed = format!("{b}-seed");
    scratch.git(&["clone", "--quiet", b, &seed]);
    std::fs::write(Path::new(&seed).join("README.md"), "shared builds\n").unwrap();
    scratch.git(&["-C", &seed, "add", "-A"]);
    scratch.git(&["-C", &seed, "commit", "--quiet", "-m", "init"]);
    scratch.git(&["-C", &seed, "push", "--quiet", "origin", "HEAD:main"]);
    bare
}

fn shown(scratch: &Scratch, bare: &Path, file: &str) -> Vec<u8> {
    scratch.git_bytes(&[
        "--git-dir",
        bare.to_str().unwrap(),
        "show",
        &format!("main:{file}"),
    ])
}

fn shown_json(scratch: &Scratch, bare: &Path, file: &str) -> Value {
    serde_json::from_slice(&shown(scratch, bare, file)).unwrap()
}

fn listed(scratch: &Scratch, bare: &Path) -> Vec<String> {
    scratch
        .git(&[
            "--git-dir",
            bare.to_str().unwrap(),
            "ls-tree",
            "-r",
            "--name-only",
            "main",
        ])
        .split('\n')
        .filter(|line| !line.is_empty())
        .map(String::from)
        .collect()
}

fn gunzip(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .unwrap();
    out
}

fn sha256_hex(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// The given export, viewer and commands.

fn mask() -> Value {
    json!({ "minX": -64, "maxX": 63, "minZ": -64, "maxZ": 63 })
}

const BOUNDS: Bounds = Bounds {
    min: [-3, 64, 2],
    max: [4, 90, 9],
};

fn build() -> Exported {
    Exported {
        blueprint: b"blueprint bytes".to_vec(),
        mask: mask(),
        bounds: BOUNDS,
        preview: b"jpeg".to_vec(),
    }
}

type Exporting = Pin<Box<dyn Future<Output = Result<Exported, String>> + Send>>;
type ExportFn = Arc<dyn Fn(&Source, [u32; 2]) -> Exporting + Send + Sync>;

struct FakeExport(ExportFn);

#[async_trait]
impl Export for FakeExport {
    async fn export_build(&self, source: &Source, size: [u32; 2]) -> Result<Exported, String> {
        (self.0)(source, size).await
    }
    // The card of a bench: the cells and the shots, as text.
    async fn montage(
        &self,
        tiles: Vec<Vec<u8>>,
        cells: Vec<Cell>,
        size: [u32; 2],
    ) -> Result<Vec<u8>, String> {
        let tiles: Vec<String> = tiles
            .iter()
            .map(|tile| String::from_utf8(tile.clone()).unwrap())
            .collect();
        Ok(json!({ "tiles": tiles, "cells": cells, "size": size })
            .to_string()
            .into_bytes())
    }
}

fn exports(make: impl Fn(&Source, [u32; 2]) -> Exporting + Send + Sync + 'static) -> ExportFn {
    Arc::new(make)
}

fn plain_export() -> ExportFn {
    exports(|_, _| Box::pin(async { Ok(build()) }))
}

struct FakeViewer {
    calls: AtomicUsize,
}

impl SiteViewer for FakeViewer {
    fn write_viewer(&self, site: &Path) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::fs::create_dir_all(site.join("viewer").join("v1")).unwrap();
        std::fs::write(site.join("viewer").join("v1").join("viewer.js"), "viewer").unwrap();
        Ok("v1".into())
    }
    fn share_page_html(&self, page: &SharePage<'_>) -> String {
        format!(
            "<html>{}</html>",
            json!({
                "viewerId": page.viewer_id,
                "shareId": page.share_id,
                "title": page.title,
                "description": page.description,
                "url": page.url,
                "image": page.image,
                "labels": page.labels,
            })
        )
    }
}

/// What a step gives in place of the command it is called for.
enum Step {
    Fail(Failure),
    /// Runs until its time limit stops it.
    Hang,
    /// The real command, then this.
    After(Box<dyn FnOnce() + Send>),
}

type StepFn = Box<dyn Fn(&str, &[String]) -> Option<Step> + Send + Sync>;

struct FakeRun {
    env: Vec<(String, String)>,
    commands: Mutex<Vec<String>>,
    pages_error: Option<String>,
    pages_source: Value,
    build_type: String,
    port: u16,
    step: StepFn,
}

#[async_trait]
impl Run for FakeRun {
    async fn run(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Ran, Failure> {
        self.commands.lock().unwrap().push(
            std::iter::once(command.to_string())
                .chain(args.iter().cloned())
                .collect::<Vec<_>>()
                .join(" "),
        );
        // git never prompts.
        if command == "git" {
            let get = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
            assert_eq!(
                [
                    get("GIT_TERMINAL_PROMPT"),
                    get("GCM_INTERACTIVE"),
                    get("GIT_ASKPASS")
                ],
                [Some("0"), Some("never"), Some("")]
            );
        }
        let mut after = None;
        match (self.step)(command, args) {
            Some(Step::Fail(failure)) => return Err(failure),
            Some(Step::Hang) => std::future::pending::<()>().await,
            Some(Step::After(then)) => after = Some(then),
            None => {}
        }
        if command == "gh" && args[1] == "user" {
            assert_eq!(
                args,
                [
                    "api",
                    "user",
                    "--jq",
                    r#".login, "\(.id)+\(.login)@users.noreply.github.com""#
                ]
            );
            return Ok(Ran {
                stdout: "owner\n42+owner@users.noreply.github.com\n".into(),
                stderr: String::new(),
            });
        }
        if command == "gh" {
            let parts: Vec<&str> = args[1].split('/').collect();
            assert!(
                parts.len() == 4
                    && parts[0] == "repos"
                    && parts[1] == "owner"
                    && parts[3] == "pages",
                "{args:?}"
            );
            assert_eq!(args.len(), 2);
            if let Some(stderr) = &self.pages_error {
                return Err(Failure {
                    code: Some(Code::Exit(1)),
                    stderr: stderr.clone(),
                    message: "gh failed".into(),
                });
            }
            let html_url = format!("http://127.0.0.1:{}/{}/", self.port, parts[2]);
            return Ok(Ran {
                stdout: json!({ "html_url": html_url, "build_type": self.build_type, "source": self.pages_source }).to_string(),
                stderr: String::new(),
            });
        }
        assert_eq!(command, "git");
        let mut all = self.env.clone();
        all.extend(env.iter().cloned());
        let ran = Processes.run(command, args, cwd, &all).await;
        if let Some(then) = after {
            then();
        }
        ran
    }
}

// ---------------------------------------------------------------------------
// GitHub Pages: serves what a remote holds after `deploy_at` reads of
// build.json (none: never).

type Repos = Arc<Mutex<HashMap<String, PathBuf>>>;

fn pages(
    scratch_env: Vec<(String, String)>,
    repos: Repos,
    deploy_at: Option<usize>,
    fetches: Arc<AtomicUsize>,
) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buffer).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
            }
            let request = String::from_utf8_lossy(&request).to_lowercase();
            // Asked past its cache, as fetch's `cache: 'no-store'` asks.
            assert!(
                request.contains("\r\ncache-control: no-cache\r\n"),
                "{request}"
            );
            let path = request.split(' ').nth(1).unwrap().to_string();
            let path = path.split('?').next().unwrap();
            let fetched = fetches.fetch_add(1, Ordering::SeqCst) + 1;
            let response = if deploy_at.is_none_or(|at| fetched <= at) {
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec()
            } else {
                let mut parts = path.trim_start_matches('/').splitn(2, '/');
                let (name, rest) = (parts.next().unwrap(), parts.next().unwrap());
                let bare = repos.lock().unwrap()[&format!("owner/{name}")].clone();
                let output = std::process::Command::new("git")
                    .args([
                        "--git-dir",
                        bare.to_str().unwrap(),
                        "show",
                        &format!("main:{rest}"),
                    ])
                    .envs(scratch_env.clone())
                    .output()
                    .unwrap();
                let mut response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    output.stdout.len()
                )
                .into_bytes();
                response.extend_from_slice(&output.stdout);
                response
            };
            stream.write_all(&response).unwrap();
        }
    });
    port
}

// ---------------------------------------------------------------------------
// The harness.

struct Harness {
    export: ExportFn,
    deploy_at: Option<usize>,
    pages_error: Option<String>,
    pages_source: Value,
    build_type: &'static str,
    wait: Duration,
    step: StepFn,
    network: Duration,
    local: Duration,
}

impl Default for Harness {
    fn default() -> Harness {
        Harness {
            export: plain_export(),
            deploy_at: Some(1),
            pages_error: None,
            pages_source: json!({ "branch": "main", "path": "/" }),
            build_type: "legacy",
            wait: Duration::from_millis(2000),
            step: Box::new(|_, _| None),
            network: Duration::from_secs(120),
            local: Duration::from_secs(30),
        }
    }
}

struct Site {
    scratch: Scratch,
    shares: Arc<Shares>,
    root: PathBuf,
    repos: Repos,
    run: Arc<FakeRun>,
    viewer: Arc<FakeViewer>,
    fetches: Arc<AtomicUsize>,
    port: u16,
    remotes: AtomicUsize,
}

/// Shares on a fresh data folder: repositories named `owner/<n>` are the
/// bare remotes in `repos`; Pages serves what a remote holds after
/// `deploy_at` fetches of build.json, from `pages_source` of a site built
/// by `build_type`. `step(command, args)`, when it gives anything, runs in
/// place of the command (gh or git) it is called for.
fn harness(options: Harness) -> Site {
    let scratch = Scratch::new();
    let root = scratch.0.join(format!("share-{}", random_uuid()));
    std::fs::create_dir_all(&root).unwrap();
    let repos: Repos = Arc::default();
    let fetches = Arc::new(AtomicUsize::new(0));
    let port = pages(
        scratch.env(),
        Arc::clone(&repos),
        options.deploy_at,
        Arc::clone(&fetches),
    );
    let run = Arc::new(FakeRun {
        env: scratch.env(),
        commands: Mutex::new(Vec::new()),
        pages_error: options.pages_error,
        pages_source: options.pages_source,
        build_type: options.build_type.into(),
        port,
        step: options.step,
    });
    let viewer = Arc::new(FakeViewer {
        calls: AtomicUsize::new(0),
    });
    let remotes = Arc::clone(&repos);
    let shares = Shares::new(Options {
        root: root.clone(),
        remote: Box::new(move |repo| remotes.lock().unwrap()[repo].display().to_string()),
        run: run.clone(),
        export: Arc::new(FakeExport(options.export)),
        viewer: viewer.clone(),
        wait: options.wait,
        poll: Duration::from_millis(5),
        network: options.network,
        local: options.local,
    });
    Site {
        scratch,
        shares,
        root,
        repos,
        run,
        viewer,
        fetches,
        port,
        remotes: AtomicUsize::new(0),
    }
}

impl Site {
    fn repo(&self, name: &str) -> String {
        let n = self.remotes.fetch_add(1, Ordering::SeqCst) + 1;
        let bare = remote(&self.scratch, n);
        self.repos
            .lock()
            .unwrap()
            .insert(format!("owner/{name}"), bare);
        format!("owner/{name}")
    }

    fn bare(&self, repo: &str) -> PathBuf {
        self.repos.lock().unwrap()[repo].clone()
    }

    fn base(&self, name: &str) -> String {
        format!("http://127.0.0.1:{}/{name}/", self.port)
    }

    fn commands(&self) -> Vec<String> {
        self.run.commands.lock().unwrap().clone()
    }
}

fn chat() -> Source {
    chat_titled("A tower")
}

fn chat_titled(title: &str) -> Source {
    let mut world = World::new();
    world
        .set([0, 0, 0], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    Source {
        id: random_uuid(),
        title: title.into(),
        model: "model/x".into(),
        effort: None,
        world,
    }
}

/// Publishes `subject` to `repo` and gives the states it moved through.
async fn publish(shares: &Arc<Shares>, repo: &str, subject: Subject) -> Vec<PublishStep> {
    let states = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&states);
    shares
        .publish(repo, subject, move |step| noted.lock().unwrap().push(step))
        .unwrap()
        .await;
    states.lock().unwrap().clone()
}

fn names(states: &[PublishStep]) -> Vec<&'static str> {
    states.iter().map(PublishStep::state).collect()
}

fn error_of(states: &[PublishStep]) -> String {
    match states.last() {
        Some(PublishStep::Failed { error }) => error.clone(),
        other => panic!("not failed: {other:?}"),
    }
}

fn is_live(states: &[PublishStep]) -> bool {
    matches!(states.last(), Some(PublishStep::Live { .. }))
}

fn page_of(html: &[u8]) -> Value {
    let text = String::from_utf8(html.to_vec()).unwrap();
    serde_json::from_str(&text[6..text.len() - 7]).unwrap()
}

// ---------------------------------------------------------------------------
// The tests.

#[test]
fn a_share_id_is_the_first_12_hex_digits_of_the_sha256_of_the_chat_id() {
    assert_eq!(share_id("abc"), sha256_hex("abc")[..12]);
}

#[test]
fn a_publish_moves_only_along_exporting_publishing_waiting_live_or_to_failed_from_the_first_three()
{
    let events = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&events);
    let mut publish = Publish::new(Box::new(move |step| noted.lock().unwrap().push(step)));
    assert_eq!(publish.state(), "exporting");
    let error = publish.to(PublishStep::Waiting).unwrap_err();
    assert!(
        error.contains("exporting") && error.contains("cannot become waiting"),
        "{error}"
    );
    publish.to(PublishStep::Publishing).unwrap();
    publish.to(PublishStep::Waiting).unwrap();
    publish.to(PublishStep::Live { url: "u".into() }).unwrap();
    let error = publish
        .to(PublishStep::Failed { error: "x".into() })
        .unwrap_err();
    assert!(
        error.contains("live") && error.contains("cannot become failed"),
        "{error}"
    );
    assert_eq!(
        *events.lock().unwrap(),
        [
            PublishStep::Exporting,
            PublishStep::Publishing,
            PublishStep::Waiting,
            PublishStep::Live { url: "u".into() }
        ]
    );
    assert_eq!(
        events
            .lock()
            .unwrap()
            .iter()
            .map(|e| e.to_json().to_string())
            .collect::<Vec<_>>(),
        [
            r#"{"state":"exporting"}"#,
            r#"{"state":"publishing"}"#,
            r#"{"state":"waiting"}"#,
            r#"{"state":"live","url":"u"}"#
        ]
    );
    for from in [
        vec![],
        vec![PublishStep::Publishing],
        vec![PublishStep::Publishing, PublishStep::Waiting],
    ] {
        let mut other = Publish::new(Box::new(|_| {}));
        for step in from {
            other.to(step).unwrap();
        }
        other.to(PublishStep::Failed { error: "x".into() }).unwrap();
        let error = other.to(PublishStep::Live { url: "u".into() }).unwrap_err();
        assert!(
            error.contains("failed") && error.contains("cannot become live"),
            "{error}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_publish_pushes_the_page_its_build_and_the_viewer_and_is_live_once_pages_serves_its_version()
 {
    let site = harness(Harness::default());
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    assert_eq!(site.shares.url(&name, &id).await.unwrap(), None);
    let states = publish(&site.shares, &name, Subject::chat(conversation.clone())).await;
    let url = format!("{}b/{id}/", site.base("builds"));
    assert_eq!(
        states,
        [
            PublishStep::Exporting,
            PublishStep::Publishing,
            PublishStep::Waiting,
            PublishStep::Live { url: url.clone() }
        ]
    );
    assert_eq!(
        site.fetches.load(Ordering::SeqCst),
        2,
        "polled until the pushed version was served"
    );

    let bare = site.bare(&name);
    let mut files = listed(&site.scratch, &bare);
    files.sort();
    assert_eq!(
        files,
        [
            ".nojekyll".to_string(),
            "README.md".into(),
            format!("b/{id}/0.bin.gz"),
            format!("b/{id}/build.json"),
            format!("b/{id}/index.html"),
            format!("b/{id}/preview.jpg"),
            "viewer/v1/viewer.js".into()
        ]
    );
    let build = shown_json(&site.scratch, &bare, &format!("b/{id}/build.json"));
    let version = build["version"].as_str().unwrap().to_string();
    assert!(version.len() == 36 && version.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
    assert_eq!(
        mcbench_json::stringify(&build),
        mcbench_json::stringify(&json!({
            "title": "A tower", "kind": "chat", "version": version, "url": url,
            "builds": [{ "label": "model/x", "blueprint": "0.bin.gz", "mask": mask(), "bounds": { "min": BOUNDS.min, "max": BOUNDS.max } }],
        }))
    );
    assert_eq!(
        gunzip(&shown(&site.scratch, &bare, &format!("b/{id}/0.bin.gz"))),
        b"blueprint bytes"
    );
    assert_eq!(
        shown(&site.scratch, &bare, &format!("b/{id}/preview.jpg")),
        b"jpeg"
    );
    assert_eq!(site.viewer.calls.load(Ordering::SeqCst), 1);
    let page = page_of(&shown(&site.scratch, &bare, &format!("b/{id}/index.html")));
    assert_eq!(
        page,
        json!({ "viewerId": "v1", "shareId": id, "title": "A tower", "description": "Built by model/x in mcbench-lite. Drag to look around.", "url": url, "image": format!("{url}preview.jpg"), "labels": ["model/x"] })
    );
    assert_eq!(
        site.shares.url(&name, &id).await.unwrap(),
        Some(url.clone())
    );
    assert!(site.root.join("site").join(".git").exists());
    // The commit is the signed-in GitHub account's, at its noreply address.
    assert_eq!(
        site.scratch
            .git(&[
                "--git-dir",
                bare.to_str().unwrap(),
                "log",
                "-1",
                "--format=%an <%ae> %cn <%ce> %s",
                "main"
            ])
            .trim(),
        format!(
            "owner <42+owner@users.noreply.github.com> owner <42+owner@users.noreply.github.com> Share {id}: A tower"
        )
    );

    // Published again, it has a new version.
    publish(&site.shares, &name, Subject::chat(conversation)).await;
    assert_ne!(
        shown_json(&site.scratch, &bare, &format!("b/{id}/build.json"))["version"],
        json!(version)
    );
}

/// An export held until `release` is sent, when `hold` is set.
fn held_export(hold: Arc<AtomicBool>) -> (ExportFn, tokio::sync::watch::Sender<bool>) {
    let (release, held) = tokio::sync::watch::channel(false);
    let export = exports(move |_, _| {
        let mut held = held.clone();
        let hold = hold.load(Ordering::SeqCst);
        Box::pin(async move {
            if hold {
                held.wait_for(|released| *released).await.unwrap();
            }
            Ok(build())
        })
    });
    (export, release)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_publish_of_a_chat_that_is_publishing_is_refused_409_and_publishes_of_two_chats_change_the_tree_one_at_a_time()
 {
    let (export, release) = held_export(Arc::new(AtomicBool::new(true)));
    let site = harness(Harness {
        export,
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let (first, second) = (chat(), chat());
    let running = tokio::spawn({
        let (shares, name, first) = (site.shares.clone(), name.clone(), first.clone());
        async move { publish(&shares, &name, Subject::chat(first)).await }
    });
    let publishing = || site.shares.publishing(&share_id(&first.id));
    // The spawned publish has registered once it has told `exporting`.
    while !publishing() {
        tokio::task::yield_now().await;
    }
    assert!(matches!(
        site.shares
            .publish(&name, Subject::chat(first.clone()), |_| {}),
        Err(ShareError::Busy(_))
    ));
    assert!(matches!(
        site.shares.remove(&name, &share_id(&first.id)).await,
        Err(ShareError::Busy(_))
    ));
    let other = tokio::spawn({
        let (shares, name) = (site.shares.clone(), name.clone());
        async move { publish(&shares, &name, Subject::chat(second)).await }
    });
    release.send(true).unwrap();
    let (a, b) = (running.await.unwrap(), other.await.unwrap());
    assert!(is_live(&a));
    assert!(is_live(&b));
    assert!(!publishing());
    // Each publish's git commands, clone or fetch to push, run with no other's between.
    let tree: Vec<String> = site
        .commands()
        .iter()
        .filter(|line| {
            ["clone", "fetch", "reset", "add", "commit", "push"]
                .iter()
                .any(|step| line.starts_with(&format!("git {step}")))
        })
        .map(|line| line.split(' ').nth(1).unwrap().to_string())
        .collect();
    assert_eq!(
        tree,
        [
            "clone", "add", "commit", "push", "fetch", "reset", "add", "commit", "push"
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repository_without_github_pages_fails_the_publish_and_says_so_pushing_nothing() {
    let site = harness(Harness {
        pages_error: Some("gh: Not Found (HTTP 404)".into()),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let states = publish(&site.shares, &name, Subject::chat(chat())).await;
    assert_eq!(names(&states), ["exporting", "publishing", "failed"]);
    assert_eq!(
        error_of(&states),
        "GitHub Pages is not set up for owner/builds (gh: Not Found (HTTP 404)): in the repository's Settings → Pages, set Source to \"Deploy from a branch\", branch main, folder / (root)"
    );
    assert_eq!(listed(&site.scratch, &site.bare(&name)), ["README.md"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_pages_serves_from_another_branch_or_folder_fails_before_pushing_saying_what_to_set_one_a_workflow_deploys_is_published()
 {
    for source in [
        json!({ "branch": "gh-pages", "path": "/" }),
        json!({ "branch": "main", "path": "/docs" }),
    ] {
        let site = harness(Harness {
            pages_source: source.clone(),
            ..Harness::default()
        });
        let name = site.repo("builds");
        let states = publish(&site.shares, &name, Subject::chat(chat())).await;
        assert_eq!(names(&states), ["exporting", "publishing", "failed"]);
        assert_eq!(
            error_of(&states),
            format!(
                "GitHub Pages for owner/builds serves the {} branch's {} folder, but builds are pushed to the root of main: in the repository's Settings → Pages, set Source to \"Deploy from a branch\", branch main, folder / (root)",
                source["branch"].as_str().unwrap(),
                source["path"].as_str().unwrap()
            )
        );
        assert_eq!(listed(&site.scratch, &site.bare(&name)), ["README.md"]);
        assert!(
            site.root.join("site").join(".git").exists(),
            "nothing was written, so the copy is kept"
        );
    }
    let site = harness(Harness {
        deploy_at: Some(0),
        build_type: "workflow",
        pages_source: json!({ "branch": "gh-pages", "path": "/" }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    assert!(is_live(
        &publish(&site.shares, &name, Subject::chat(chat())).await
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn gh_not_installed_or_not_signed_in_fails_the_publish_saying_which() {
    let cases = [
        (
            Failure { code: Some(Code::NotFound), stderr: String::new(), message: "spawn gh ENOENT".into() },
            "The GitHub CLI (gh) is not installed: install it from https://cli.github.com and run `gh auth login`".to_string(),
        ),
        (
            Failure { code: Some(Code::Exit(4)), stderr: "To get started with GitHub CLI, please run:  gh auth login\n".into(), message: "gh failed".into() },
            "The GitHub CLI is not signed in to GitHub: run `gh auth login` (gh api repos/owner/builds/pages: To get started with GitHub CLI, please run:  gh auth login)".to_string(),
        ),
        (
            Failure { code: Some(Code::Exit(1)), stderr: "gh: Bad credentials (HTTP 401)\n".into(), message: "gh failed".into() },
            "The GitHub CLI is not signed in to GitHub: run `gh auth login` (gh api repos/owner/builds/pages: gh: Bad credentials (HTTP 401))".to_string(),
        ),
    ];
    for (failure, message) in cases {
        let site = harness(Harness {
            step: Box::new(move |command, _| {
                (command == "gh").then(|| Step::Fail(failure.clone()))
            }),
            ..Harness::default()
        });
        let name = site.repo("builds");
        let states = publish(&site.shares, &name, Subject::chat(chat())).await;
        assert_eq!(names(&states), ["exporting", "publishing", "failed"]);
        assert_eq!(error_of(&states), message);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_git_step_that_does_not_finish_within_its_time_limit_fails_naming_the_step_and_the_jobs_after_it_run()
 {
    let stuck = Arc::new(AtomicBool::new(true));
    let held = Arc::clone(&stuck);
    let site = harness(Harness {
        deploy_at: Some(0),
        network: Duration::from_millis(1500),
        step: Box::new(move |command, args| {
            (held.load(Ordering::SeqCst) && command == "git" && args[0] == "push")
                .then_some(Step::Hang)
        }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let states = publish(&site.shares, &name, Subject::chat(conversation.clone())).await;
    assert_eq!(names(&states), ["exporting", "publishing", "failed"]);
    assert_eq!(
        error_of(&states),
        "`git push` did not finish within 1.5 seconds"
    );
    assert!(
        !site.root.join("site").exists(),
        "a change stopped part-way drops the copy"
    );
    stuck.store(false, Ordering::SeqCst);
    assert!(is_live(
        &publish(&site.shares, &name, Subject::chat(conversation)).await
    ));
    assert_eq!(site.shares.list(&name).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_git_sign_in_git_cannot_make_fails_saying_to_run_gh_auth_setup_git() {
    let site = harness(Harness {
        step: Box::new(|command, args| {
            (command == "git" && args[0] == "clone").then(|| {
                Step::Fail(Failure {
                    code: Some(Code::Exit(128)),
                    stderr: "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n".into(),
                    message: "Command failed".into(),
                })
            })
        }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let states = publish(&site.shares, &name, Subject::chat(chat())).await;
    assert_eq!(
        error_of(&states),
        "Could not get a copy of owner/builds from GitHub: git could not sign in to GitHub over HTTPS (git clone). Run `gh auth setup-git` in a terminal so git signs in with the GitHub CLI's account, then try again."
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_copy_that_cannot_be_brought_up_to_date_is_kept_url_answers_from_it_and_listing_fails_saying_why()
 {
    let offline = Arc::new(AtomicBool::new(false));
    let held = Arc::clone(&offline);
    let site = harness(Harness {
        deploy_at: Some(0),
        step: Box::new(move |command, args| {
            (held.load(Ordering::SeqCst) && command == "git" && args[0] == "fetch").then(|| {
                Step::Fail(Failure {
                    code: Some(Code::Exit(128)),
                    stderr: "fatal: unable to access 'https://github.com/owner/builds.git/': Could not resolve host: github.com\n".into(),
                    message: "Command failed".into(),
                })
            })
        }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    publish(&site.shares, &name, Subject::chat(conversation)).await;
    offline.store(true, Ordering::SeqCst);
    assert_eq!(
        site.shares.list(&name).await.unwrap_err(),
        ShareError::Failed("Could not bring the site's copy up to date with owner/builds on GitHub: fatal: unable to access 'https://github.com/owner/builds.git/': Could not resolve host: github.com".into())
    );
    assert!(
        site.root
            .join("site")
            .join("b")
            .join(&id)
            .join("build.json")
            .exists()
    );
    assert_eq!(
        site.shares.url(&name, &id).await.unwrap(),
        Some(format!("{}b/{id}/", site.base("builds")))
    );
    offline.store(false, Ordering::SeqCst);
    assert_eq!(site.shares.list(&name).await.unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_export_that_fails_fails_the_publish() {
    let site = harness(Harness {
        export: exports(|_, _| {
            Box::pin(async { Err("Minecraft keeps none of the blocks".to_string()) })
        }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let states = publish(&site.shares, &name, Subject::chat(chat())).await;
    assert_eq!(
        states,
        [
            PublishStep::Exporting,
            PublishStep::Failed {
                error: "Minecraft keeps none of the blocks".into()
            }
        ]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_push_pages_does_not_serve_in_time_fails_saying_the_files_are_pushed() {
    let site = harness(Harness {
        deploy_at: None,
        wait: Duration::from_millis(40),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    let states = publish(&site.shares, &name, Subject::chat(conversation)).await;
    assert_eq!(
        names(&states),
        ["exporting", "publishing", "waiting", "failed"]
    );
    assert_eq!(
        error_of(&states),
        format!(
            "The files are pushed, but GitHub Pages has not served them yet after 0 minutes: {}b/{id}/",
            site.base("builds")
        )
    );
    assert!(listed(&site.scratch, &site.bare(&name)).contains(&format!("b/{id}/build.json")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_deletes_the_builds_folder_and_pushes_a_chat_not_shared_is_a_404() {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    publish(&site.shares, &name, Subject::chat(conversation)).await;
    assert!(site.shares.remove(&name, &id).await.unwrap());
    assert!(
        listed(&site.scratch, &site.bare(&name))
            .iter()
            .all(|file| !file.starts_with("b/"))
    );
    assert_eq!(site.shares.url(&name, &id).await.unwrap(), None);
    assert!(!site.shares.remove(&name, &id).await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_changed_repository_gets_a_fresh_clone_and_a_chat_shared_in_the_old_one_is_not_shared_in_it()
 {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let first = site.repo("one");
    let conversation = chat();
    publish(&site.shares, &first, Subject::chat(conversation.clone())).await;
    let second = site.repo("two");
    assert_eq!(
        site.shares
            .url(&second, &share_id(&conversation.id))
            .await
            .unwrap(),
        None
    );
    let other = chat();
    publish(&site.shares, &second, Subject::chat(other.clone())).await;
    let ids: Vec<String> = listed(&site.scratch, &site.bare(&second))
        .iter()
        .filter(|file| file.starts_with("b/"))
        .map(|file| file.split('/').nth(1).unwrap().to_string())
        .collect();
    assert_eq!(ids, vec![share_id(&other.id); 4]);
    assert_eq!(
        site.shares
            .url(&first, &share_id(&conversation.id))
            .await
            .unwrap(),
        None,
        "the tree is the second repository now"
    );
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[tokio::test(flavor = "multi_thread")]
async fn the_sites_builds_are_listed_newest_first_with_their_last_publish_time_and_a_removed_one_is_gone()
 {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    assert_eq!(
        site.shares.list(&name).await.unwrap(),
        [],
        "a site with no b/ folder has no builds"
    );
    let older = chat_titled("Older");
    let newer = chat_titled("Newer");
    publish(&site.shares, &name, Subject::chat(older.clone())).await;
    publish(&site.shares, &name, Subject::chat(newer.clone())).await;
    let builds = site.shares.list(&name).await.unwrap();
    let base = site.base("builds");
    assert_eq!(
        builds
            .iter()
            .map(|b| (
                b.share_id.clone(),
                b.title.clone(),
                b.kind.clone(),
                b.labels.clone(),
                b.url.clone()
            ))
            .collect::<Vec<_>>(),
        [&newer, &older]
            .iter()
            .map(|c| (
                share_id(&c.id),
                c.title.clone(),
                "chat".to_string(),
                vec!["model/x".to_string()],
                format!("{base}b/{}/", share_id(&c.id))
            ))
            .collect::<Vec<_>>()
    );
    for build in &builds {
        assert!(
            (build.published_at - now_ms()).abs() < 60_000,
            "{}",
            build.published_at
        );
    }
    // Published again, the older is the newest.
    publish(&site.shares, &name, Subject::chat(older.clone())).await;
    assert_eq!(
        site.shares
            .list(&name)
            .await
            .unwrap()
            .iter()
            .map(|b| b.title.as_str())
            .collect::<Vec<_>>(),
        ["Older", "Newer"]
    );
    assert!(
        site.shares
            .remove(&name, &share_id(&older.id))
            .await
            .unwrap()
    );
    assert_eq!(
        site.shares
            .list(&name)
            .await
            .unwrap()
            .iter()
            .map(|b| b.share_id.clone())
            .collect::<Vec<_>>(),
        [share_id(&newer.id)]
    );
    assert!(
        site.shares
            .remove(&name, &share_id(&newer.id))
            .await
            .unwrap()
    );
    assert_eq!(
        site.shares.list(&name).await.unwrap(),
        [],
        "a b/ folder emptied by removals has no builds"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_removal_by_share_id_needs_no_chat_and_one_under_a_publish_of_that_build_is_refused_409()
{
    let hold = Arc::new(AtomicBool::new(false));
    let (export, release) = held_export(Arc::clone(&hold));
    let site = harness(Harness {
        export,
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    publish(&site.shares, &name, Subject::chat(conversation.clone())).await;
    hold.store(true, Ordering::SeqCst);
    let running = site
        .shares
        .publish(&name, Subject::chat(conversation), |_| {})
        .unwrap();
    let running = tokio::spawn(running);
    assert_eq!(
        site.shares.remove(&name, &id).await,
        Err(ShareError::Busy("Wait for the build to finish publishing"))
    );
    release.send(true).unwrap();
    running.await.unwrap();
    assert!(site.shares.remove(&name, &id).await.unwrap());
    assert!(
        !listed(&site.scratch, &site.bare(&name))
            .iter()
            .any(|file| file.starts_with(&format!("b/{id}/")))
    );
    assert!(!site.shares.remove(&name, "abcdefabcdef").await.unwrap());
}

/// A bench of three models in `order`; the one named in `empty` built nothing.
fn bench(blind: bool, revealed: bool, empty: Option<&str>) -> (BenchSource, Vec<Source>) {
    let members: Vec<Source> = ["a", "b", "c"]
        .iter()
        .map(|name| {
            let mut world = World::new();
            if Some(*name) != empty {
                for x in 0..3 {
                    world
                        .set([x, 0, 0], BlockState::new("minecraft:stone", &[]))
                        .unwrap();
                }
            }
            Source {
                id: format!("{name}-{}", random_uuid()),
                title: String::new(),
                model: format!("model/{name}"),
                effort: None,
                world,
            }
        })
        .collect();
    let order = vec![
        members[2].id.clone(),
        members[0].id.clone(),
        members[1].id.clone(),
    ];
    (
        BenchSource {
            id: random_uuid(),
            title: "A bridge".into(),
            blind,
            revealed,
            order,
        },
        members,
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bench_share_holds_the_builds_of_the_models_that_built_something_in_the_benchs_order_with_a_card_of_their_shots()
 {
    let exported = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&exported);
    let site = harness(Harness {
        deploy_at: Some(0),
        export: exports(move |member, size| {
            noted.lock().unwrap().push((member.model.clone(), size));
            let model = member.model.clone();
            Box::pin(async move {
                Ok(Exported {
                    blueprint: format!("blueprint of {model}").into_bytes(),
                    preview: format!("shot of {model}").into_bytes(),
                    ..build()
                })
            })
        }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let (subject, members) = bench(false, false, Some("a"));
    let id = share_id(&subject.id);
    let url = format!("{}b/{id}/", site.base("builds"));
    let states = publish(
        &site.shares,
        &name,
        Subject::Bench {
            bench: subject,
            members,
        },
    )
    .await;
    assert!(is_live(&states), "{states:?}");

    // c, a, b in order, a left out; each shot framed for its half of the card.
    assert_eq!(
        *exported.lock().unwrap(),
        [
            ("model/c".to_string(), [512, 1024]),
            ("model/b".to_string(), [512, 1024])
        ]
    );
    let bare = site.bare(&name);
    let mut files: Vec<String> = listed(&site.scratch, &bare)
        .into_iter()
        .filter(|f| f.starts_with("b/"))
        .collect();
    files.sort();
    assert_eq!(
        files,
        [
            "0.bin.gz",
            "1.bin.gz",
            "build.json",
            "index.html",
            "preview.jpg"
        ]
        .map(|f| format!("b/{id}/{f}"))
    );
    let build = shown_json(&site.scratch, &bare, &format!("b/{id}/build.json"));
    let bounds = json!({ "min": BOUNDS.min, "max": BOUNDS.max });
    assert_eq!(
        mcbench_json::stringify(&build),
        mcbench_json::stringify(&json!({
            "title": "A bridge", "kind": "bench", "version": build["version"], "url": url,
            "builds": [
                { "label": "model/c", "blueprint": "0.bin.gz", "mask": mask(), "bounds": bounds },
                { "label": "model/b", "blueprint": "1.bin.gz", "mask": mask(), "bounds": bounds },
            ],
        }))
    );
    assert_eq!(
        gunzip(&shown(&site.scratch, &bare, &format!("b/{id}/0.bin.gz"))),
        b"blueprint of model/c"
    );
    assert_eq!(
        gunzip(&shown(&site.scratch, &bare, &format!("b/{id}/1.bin.gz"))),
        b"blueprint of model/b"
    );
    assert_eq!(
        shown_json(&site.scratch, &bare, &format!("b/{id}/preview.jpg")),
        json!({ "tiles": ["shot of model/c", "shot of model/b"], "cells": montage_cells(2, 1024).unwrap(), "size": [1024, 1024] })
    );
    assert_eq!(site.viewer.calls.load(Ordering::SeqCst), 1);
    let page = page_of(&shown(&site.scratch, &bare, &format!("b/{id}/index.html")));
    assert_eq!(
        page,
        json!({
            "viewerId": "v1", "shareId": id, "title": "A bridge",
            "description": "2 models built this in mcbench-lite. Switch between them and drag to look around.",
            "url": url, "image": format!("{url}preview.jpg"), "labels": ["model/c", "model/b"],
        })
    );
    let listed_share = site.shares.list(&name).await.unwrap().remove(0);
    assert_eq!(
        (
            listed_share.share_id,
            listed_share.title,
            listed_share.kind,
            listed_share.labels,
            listed_share.url.clone()
        ),
        (
            id.clone(),
            "A bridge".to_string(),
            "bench".to_string(),
            vec!["model/c".to_string(), "model/b".to_string()],
            url.clone()
        )
    );
    assert_eq!(site.shares.url(&name, &id).await.unwrap(), Some(url));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bench_that_hides_names_labels_its_builds_with_the_apps_letters_skipping_a_model_left_out_a_revealed_one_names_them()
 {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let labels = async |(bench, members): (BenchSource, Vec<Source>)| -> Vec<String> {
        let id = share_id(&bench.id);
        publish(&site.shares, &name, Subject::Bench { bench, members }).await;
        shown_json(
            &site.scratch,
            &site.bare(&name),
            &format!("b/{id}/build.json"),
        )["builds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|build| build["label"].as_str().unwrap().to_string())
            .collect()
    };
    // In order c, a, b the letters are A, B, C; a built nothing, so B is not shown.
    let hidden = bench(true, false, Some("a"));
    assert_eq!(
        hidden
            .0
            .order
            .iter()
            .map(|id| bench_label(
                &hidden.0,
                hidden.1.iter().find(|m| m.id == *id).unwrap(),
                &hidden.1
            ))
            .collect::<Vec<_>>(),
        ["A", "B", "C"]
    );
    assert_eq!(labels(hidden).await, ["A", "C"]);
    assert_eq!(labels(bench(true, false, None)).await, ["A", "B", "C"]);
    assert_eq!(
        labels(bench(true, true, Some("a"))).await,
        ["model/c", "model/b"]
    );
    assert_eq!(
        labels(bench(false, false, None)).await,
        ["model/c", "model/a", "model/b"]
    );
    // A model run more than once is told apart by its effort.
    let member = |id: &str, model: &str, effort: &str| Source {
        id: id.into(),
        title: String::new(),
        model: model.into(),
        effort: Some(effort.into()),
        world: World::new(),
    };
    let twice = [
        member("x", "model/a", "low"),
        member("y", "model/a", "high"),
        member("z", "model/b", "high"),
    ];
    let open = BenchSource {
        id: String::new(),
        title: String::new(),
        blind: false,
        revealed: false,
        order: vec!["x".into(), "y".into(), "z".into()],
    };
    assert_eq!(
        twice
            .iter()
            .map(|m| bench_label(&open, m, &twice))
            .collect::<Vec<_>>(),
        ["model/a · low", "model/a · high", "model/b"]
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_chats_share_shot_is_the_whole_square_card() {
    let sizes = Arc::new(Mutex::new(Vec::new()));
    let noted = Arc::clone(&sizes);
    let site = harness(Harness {
        deploy_at: Some(0),
        export: exports(move |_, size| {
            noted.lock().unwrap().push(size);
            Box::pin(async { Ok(build()) })
        }),
        ..Harness::default()
    });
    let name = site.repo("builds");
    publish(&site.shares, &name, Subject::chat(chat())).await;
    assert_eq!(*sizes.lock().unwrap(), [[1024, 1024]]);
}

/// Commits to the remote `bare`, through its seed clone, what `change(seed)`
/// does to the tree, and the index entries `staged(seed)` adds.
#[cfg(unix)]
fn push_to_remote(
    scratch: &Scratch,
    bare: &Path,
    change: impl FnOnce(&str),
    staged: impl FnOnce(&str),
) {
    let seed = format!("{}-seed", bare.display());
    change(&seed);
    scratch.git(&["-C", &seed, "add", "-A"]);
    staged(&seed);
    scratch.git(&["-C", &seed, "commit", "--quiet", "-m", "change"]);
    scratch.git(&["-C", &seed, "push", "--quiet", "origin", "HEAD:main"]);
}

/// A file and a folder of the owner's outside the data folder, which a
/// hostile repository's links point at.
#[cfg(unix)]
fn outside_files(scratch: &Scratch, id: &str) -> (PathBuf, impl Fn()) {
    let home = scratch.0.join(format!("home-{}", random_uuid()));
    std::fs::create_dir_all(home.join("Documents").join(id)).unwrap();
    std::fs::write(home.join(".zshrc"), "export PATH=kept\n").unwrap();
    std::fs::write(home.join("Documents").join(id).join("thesis.txt"), "kept\n").unwrap();
    let (kept, id) = (home.clone(), id.to_string());
    let untouched = move || {
        assert_eq!(
            std::fs::read_to_string(kept.join(".zshrc")).unwrap(),
            "export PATH=kept\n"
        );
        assert_eq!(
            std::fs::read_to_string(kept.join("Documents").join(&id).join("thesis.txt")).unwrap(),
            "kept\n"
        );
    };
    (home, untouched)
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_repository_whose_tree_holds_symbolic_links_or_a_submodule_fails_the_publish_naming_them_writes_nothing_through_them_and_drops_the_copy()
 {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    let (home, untouched) = outside_files(&site.scratch, &id);
    push_to_remote(
        &site.scratch,
        &site.bare(&name),
        |seed| {
            std::os::unix::fs::symlink(home.join(".zshrc"), Path::new(seed).join(".nojekyll"))
                .unwrap();
            std::os::unix::fs::symlink(home.join("Documents"), Path::new(seed).join("b")).unwrap();
        },
        |seed| {
            let head = site.scratch.git(&["-C", seed, "rev-parse", "HEAD"]);
            site.scratch.git(&[
                "-C",
                seed,
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{},module", head.trim()),
            ]);
        },
    );
    let states = publish(&site.shares, &name, Subject::chat(conversation)).await;
    assert_eq!(names(&states), ["exporting", "publishing", "failed"]);
    assert_eq!(
        error_of(&states),
        "owner/builds holds symbolic links or submodules (.nojekyll, b, module). Shared builds need a repository of plain files. Remove them from the repository and try again."
    );
    untouched();
    assert!(!site.root.join("site").exists(), "the copy is removed");
    let error = site
        .shares
        .remove(&name, &id)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("holds symbolic links or submodules (.nojekyll, b, module)"),
        "{error}"
    );
    untouched();
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn links_pushed_to_a_repository_after_its_first_publish_fail_the_next_fetch_the_same_way() {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    assert!(is_live(
        &publish(&site.shares, &name, Subject::chat(conversation)).await
    ));
    let (home, untouched) = outside_files(&site.scratch, &id);
    push_to_remote(
        &site.scratch,
        &site.bare(&name),
        |seed| {
            site.scratch
                .git(&["-C", seed, "pull", "--quiet", "origin", "main"]);
            std::fs::remove_dir_all(Path::new(seed).join("b")).unwrap();
            std::os::unix::fs::symlink(home.join("Documents"), Path::new(seed).join("b")).unwrap();
        },
        |_| {},
    );
    assert_eq!(
        site.shares.remove(&name, &id).await.unwrap_err(),
        ShareError::Failed("owner/builds holds symbolic links or submodules (b). Shared builds need a repository of plain files. Remove them from the repository and try again.".into())
    );
    untouched();
    assert!(!site.root.join("site").exists());
}

#[cfg(unix)]
type Plant = Arc<Mutex<Option<Box<dyn Fn(&Path) + Send>>>>;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn a_link_made_in_the_site_after_its_tree_was_checked_is_not_written_or_removed_through() {
    let plant: Plant = Arc::default();
    let planted = Arc::clone(&plant);
    let site_dir = Arc::new(Mutex::new(PathBuf::new()));
    let site_of = Arc::clone(&site_dir);
    // The real ls-files, then `plant`, as another program could do between
    // the check and the publish's writes.
    let site = harness(Harness {
        deploy_at: Some(0),
        step: Box::new(move |command, args| {
            if command != "git" || args[0] != "ls-files" || planted.lock().unwrap().is_none() {
                return None;
            }
            let (planted, site) = (Arc::clone(&planted), site_of.lock().unwrap().clone());
            Some(Step::After(Box::new(move || {
                (planted.lock().unwrap().as_ref().unwrap())(&site)
            })))
        }),
        ..Harness::default()
    });
    *site_dir.lock().unwrap() = site.root.join("site");
    let name = site.repo("builds");
    let conversation = chat();
    let id = share_id(&conversation.id);
    let (home, untouched) = outside_files(&site.scratch, &id);
    let zshrc = home.join(".zshrc");
    *plant.lock().unwrap() = Some(Box::new(move |site| {
        std::os::unix::fs::symlink(&zshrc, site.join(".nojekyll")).unwrap()
    }));
    let states = publish(&site.shares, &name, Subject::chat(conversation.clone())).await;
    assert_eq!(
        error_of(&states),
        format!(
            "{} is a symbolic link. Shared builds need a repository of plain files.",
            site.root.join("site").join(".nojekyll").display()
        )
    );
    untouched();
    let documents = home.join("Documents");
    *plant.lock().unwrap() = Some(Box::new(move |site| {
        std::os::unix::fs::symlink(&documents, site.join("b")).unwrap()
    }));
    assert_eq!(
        site.shares.remove(&name, &id).await.unwrap_err(),
        ShareError::Failed(format!(
            "{} is a symbolic link. Shared builds need a repository of plain files.",
            site.root.join("site").join("b").display()
        ))
    );
    untouched();
    *plant.lock().unwrap() = None;
    assert!(is_live(
        &publish(&site.shares, &name, Subject::chat(conversation)).await
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_holding_a_share_in_the_old_format_fails_the_listing_and_names_it() {
    let site = harness(Harness {
        deploy_at: Some(0),
        ..Harness::default()
    });
    let name = site.repo("builds");
    let seed = format!("{}-seed", site.bare(&name).display());
    let folder = Path::new(&seed).join("b").join("abcdefabcdef");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("build.json"),
        json!({ "title": "Old", "model": "model/x", "url": "u", "blueprint": "blueprint.bin.gz" })
            .to_string(),
    )
    .unwrap();
    site.scratch.git(&["-C", &seed, "add", "-A"]);
    site.scratch
        .git(&["-C", &seed, "commit", "--quiet", "-m", "old share"]);
    site.scratch
        .git(&["-C", &seed, "push", "--quiet", "origin", "HEAD:main"]);
    let error = site.shares.list(&name).await.unwrap_err().to_string();
    assert!(
        error.contains("b/abcdefabcdef was published in an older format"),
        "{error}"
    );
}
