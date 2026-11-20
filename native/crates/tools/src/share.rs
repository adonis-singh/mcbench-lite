//! Share: a chat's build, or a bench's builds, published as static files to
//! the GitHub Pages repository the owner names in Settings (`shareRepo`),
//! so a link to it shows the builds in a viewer anyone can move around in
//! (today's `server/share.js`). The site is a git clone of the repository
//! at `<root>/site`:
//!
//! | File | What it is |
//! |---|---|
//! | `.nojekyll` | empty |
//! | `viewer/<viewerId>/...` | the viewer ([`SiteViewer::write_viewer`]) |
//! | `b/<shareId>/index.html` | the page ([`SiteViewer::share_page_html`]) |
//! | `b/<shareId>/build.json` | what the page loads: `{ title, kind, version, url, builds: [{ label, blueprint, mask, bounds }] }` |
//! | `b/<shareId>/<n>.bin.gz` | build n as Minecraft settles it, gzipped |
//! | `b/<shareId>/preview.jpg` | the card's image |
//!
//! `shareId` is [`share_id`] of the chat's or the bench's id. A chat is a
//! share of one build; a bench's has one per model that built something.
//! A publish exports the builds, pulls, writes, commits and pushes, then
//! waits until Pages serves the version it wrote. The site is one working
//! tree, so one publish or removal changes it at a time.
//!
//! Every `git` and `gh` command is started through
//! [`mcbench_spawn::command`] ([`Processes`]); the caller gives the export
//! ([`Export`]; [`RenderedExport`] is the app's) and the viewer
//! ([`SiteViewer`]).

use std::collections::HashSet;
use std::future::Future;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use indexmap::IndexMap;
use mcbench_render::Cell;
use mcbench_world::World;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use crate::settle::{self, Settler};
use crate::shots::{self, SharedRenderer, ToDraw};
use crate::verify::random_uuid;

/// How long Pages may take to serve a push (`WAIT_MS`).
pub const WAIT: Duration = Duration::from_secs(5 * 60);
/// How often the public `build.json` is read while waiting (`POLL_MS`).
pub const POLL: Duration = Duration::from_secs(5);
/// How long a git or gh command that talks to GitHub may take (clone,
/// fetch, push: `NETWORK_MS`).
pub const NETWORK: Duration = Duration::from_secs(2 * 60);
/// How long one that does not, or asks it one thing, may take (local git,
/// `gh api`: `LOCAL_MS`).
pub const LOCAL: Duration = Duration::from_secs(30);
/// The card's image: square, as X's Player Card crops one (`PREVIEW_SIDE`).
pub const PREVIEW_SIDE: u32 = 1024;

/// git never asks for a username or password: the app has no terminal to
/// answer it, so it would wait forever. An empty `GIT_ASKPASS` stops git
/// from looking for an askpass program (`core.askPass`, `SSH_ASKPASS`).
pub const NO_PROMPTS: [(&str, &str); 3] = [
    ("GIT_TERMINAL_PROMPT", "0"),
    ("GCM_INTERACTIVE", "never"),
    ("GIT_ASKPASS", ""),
];

/// What git says when it needed to sign in and could not
/// (`SIGN_IN_FAILED`, matched without regard to case).
const SIGN_IN_FAILED: [&str; 4] = [
    "terminal prompts disabled",
    "could not read username",
    "could not read password",
    "authentication failed",
];

/// The git modes of a symbolic link and of a submodule. A site whose tree
/// holds either is refused: a write or removal in the site could go
/// through one to a folder outside the data folder.
const LINK_MODES: [&str; 2] = ["120000", "160000"];
const PLAIN_FILES: &str = "Shared builds need a repository of plain files.";

/// `gh api user --jq ...`: the signed-in account's login and noreply
/// address, one per line.
const AUTHOR_QUERY: &str = r#".login, "\(.id)+\(.login)@users.noreply.github.com""#;

/// `shareId(id)`: the first 12 hex digits of the sha256 of a chat's or a
/// bench's id.
pub fn share_id(id: &str) -> String {
    Sha256::digest(id.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()[..12]
        .to_string()
}

/// The repository's address on GitHub (the default `remote`).
pub fn github_remote(repo: &str) -> String {
    format!("https://github.com/{repo}.git")
}

/// `duration(ms)`: a time limit as its sentence names it.
fn duration(limit: Duration) -> String {
    let ms = limit.as_millis() as f64;
    let number = |value: f64| mcbench_json::stringify(&json!(value));
    if ms >= 60_000.0 {
        let plural = if ms == 60_000.0 { "" } else { "s" };
        format!("{} minute{plural}", number(ms / 60_000.0))
    } else {
        format!("{} seconds", number(ms / 1000.0))
    }
}

// ---------------------------------------------------------------------------
// The moves of a publish.

/// The moves of a publish (`PUBLISH_MOVES`): `exporting` the build (settle
/// and preview shot), `publishing` it (the site's git work and push),
/// `waiting` for Pages to serve it, then `live`; `failed` from any of the
/// first three.
pub const PUBLISH_MOVES: [(&str, &[&str]); 5] = [
    ("exporting", &["publishing", "failed"]),
    ("publishing", &["waiting", "failed"]),
    ("waiting", &["live", "failed"]),
    ("live", &[]),
    ("failed", &[]),
];

/// A move of a publish as told to its callback (`{ state, ...details }`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublishStep {
    /// `exporting`: the first.
    Exporting,
    /// `publishing`.
    Publishing,
    /// `waiting`.
    Waiting,
    /// `live`: served at `url`; the last.
    Live {
        /// The page's address.
        url: String,
    },
    /// `failed`: the last.
    Failed {
        /// Why.
        error: String,
    },
}

impl PublishStep {
    /// The state it moves to (`state`).
    pub fn state(&self) -> &'static str {
        match self {
            PublishStep::Exporting => "exporting",
            PublishStep::Publishing => "publishing",
            PublishStep::Waiting => "waiting",
            PublishStep::Live { .. } => "live",
            PublishStep::Failed { .. } => "failed",
        }
    }

    /// The line today's publish stream writes for it (`JSON.stringify`).
    pub fn to_json(&self) -> Value {
        match self {
            PublishStep::Live { url } => json!({ "state": "live", "url": url }),
            PublishStep::Failed { error } => json!({ "state": "failed", "error": error }),
            other => json!({ "state": other.state() }),
        }
    }
}

/// A publish's state (`Publish`), telling its callback each move.
pub struct Publish {
    state: &'static str,
    on_state: Box<dyn FnMut(PublishStep) + Send>,
}

impl Publish {
    /// A publish that is `exporting`, told to `on_state` at once.
    pub fn new(mut on_state: Box<dyn FnMut(PublishStep) + Send>) -> Publish {
        on_state(PublishStep::Exporting);
        Publish {
            state: "exporting",
            on_state,
        }
    }

    /// Its state.
    pub fn state(&self) -> &'static str {
        self.state
    }

    /// Moves to `step`, telling the callback; a move `PUBLISH_MOVES` does
    /// not have fails.
    pub fn to(&mut self, step: PublishStep) -> Result<(), String> {
        let state = step.state();
        let allowed = PUBLISH_MOVES
            .iter()
            .find(|(from, _)| *from == self.state)
            .expect("every state has its moves")
            .1;
        if !allowed.contains(&state) {
            return Err(format!(
                "A publish that is {} cannot become {state}",
                self.state
            ));
        }
        self.state = state;
        (self.on_state)(step);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// What is shared.

/// A model chat whose build is shared: a Build chat, or a bench's member.
#[derive(Clone, Debug)]
pub struct Source {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `model`.
    pub model: String,
    /// `effort`.
    pub effort: Option<String>,
    /// Its world as stored.
    pub world: World,
}

/// A bench, as its share reads it.
#[derive(Clone, Debug)]
pub struct BenchSource {
    /// `id`.
    pub id: String,
    /// `title`.
    pub title: String,
    /// `blind`.
    pub blind: bool,
    /// `revealed`.
    pub revealed: bool,
    /// `order`: the members' ids in the order their letters run.
    pub order: Vec<String>,
}

/// What a publish shares (today's `build` callback).
#[derive(Clone, Debug)]
pub enum Subject {
    /// A chat's share ([`Shares::chat_share`]).
    Chat(Box<Source>),
    /// A bench's share of its `members` ([`Shares::bench_share`]).
    Bench {
        /// The bench.
        bench: BenchSource,
        /// Its model chats.
        members: Vec<Source>,
    },
}

impl Subject {
    /// A chat's share.
    pub fn chat(source: Source) -> Subject {
        Subject::Chat(Box::new(source))
    }

    /// The chat's or the bench's id (`sourceId`).
    pub fn id(&self) -> &str {
        match self {
            Subject::Chat(source) => &source.id,
            Subject::Bench { bench, .. } => &bench.id,
        }
    }
}

/// `benchLabel(bench, member, members)`: what a bench shows for `member`:
/// while the bench hides names, its letter as the app shows it (its place
/// in `order`); otherwise the model's name, with the effort when the bench
/// runs that model more than once.
pub fn bench_label(bench: &BenchSource, member: &Source, members: &[Source]) -> String {
    if bench.blind && !bench.revealed {
        let place = bench
            .order
            .iter()
            .position(|id| *id == member.id)
            .map_or(-1, |at| at as i64);
        return char::from_u32((65 + place) as u32)
            .expect("a letter")
            .to_string();
    }
    let repeated = members
        .iter()
        .filter(|other| other.model == member.model)
        .count()
        > 1;
    match &member.effort {
        Some(effort) if repeated && !effort.is_empty() => format!("{} · {effort}", member.model),
        _ => member.model.clone(),
    }
}

/// A build's box at Minecraft height (`bounds`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    /// The lowest corner.
    pub min: [i32; 3],
    /// The highest corner, included.
    pub max: [i32; 3],
}

impl Bounds {
    fn to_json(self) -> Value {
        json!({ "min": self.min, "max": self.max })
    }
}

/// One build exported (`exportBuild`'s answer): the world settled at its
/// settle ticks as a blueprint, the viewer's mask and bounds for it, and
/// its three-quarter shot.
#[derive(Clone, Debug)]
pub struct Exported {
    /// The blueprint (`worldBlueprint` at Minecraft height).
    pub blueprint: Vec<u8>,
    /// `renderMask(bounds)`: `{ minX, maxX, minZ, maxZ }`.
    pub mask: Value,
    /// The bounds at Minecraft height.
    pub bounds: Bounds,
    /// The shot's JPEG.
    pub preview: Vec<u8>,
}

/// One build of a share.
#[derive(Clone, Debug)]
pub struct ShareBuild {
    /// What the page labels it.
    pub label: String,
    /// Its blueprint.
    pub blueprint: Vec<u8>,
    /// Its mask.
    pub mask: Value,
    /// Its bounds.
    pub bounds: Bounds,
}

/// What a share holds ([`Shares::chat_share`], [`Shares::bench_share`]).
#[derive(Clone, Debug)]
pub struct Share {
    /// `title`.
    pub title: String,
    /// `kind`: `chat` or `bench`.
    pub kind: &'static str,
    /// The card's description.
    pub description: String,
    /// The builds.
    pub builds: Vec<ShareBuild>,
    /// The card's JPEG.
    pub preview: Vec<u8>,
}

/// The export of builds (`exportBuild`) and a bench's card
/// (`composeMontage`), given to [`Shares`].
#[async_trait]
pub trait Export: Send + Sync {
    /// `exportBuild(conversation, size)`: `source`'s build with a shot of
    /// `size` (`[width, height]`).
    async fn export_build(&self, source: &Source, size: [u32; 2]) -> Result<Exported, String>;
    /// `composeMontage(tiles, cells, size)`: one JPEG of the tiles in their
    /// cells.
    async fn montage(
        &self,
        tiles: Vec<Vec<u8>>,
        cells: Vec<Cell>,
        size: [u32; 2],
    ) -> Result<Vec<u8>, String>;
}

/// The app's export: the settles of `settler`, the shots of the process's
/// one renderer (as `see_build` takes its pictures).
pub struct RenderedExport {
    /// The settles.
    pub settler: Arc<Settler>,
    /// The renderer.
    pub renderer: Arc<SharedRenderer>,
}

#[async_trait]
impl Export for RenderedExport {
    async fn export_build(&self, source: &Source, size: [u32; 2]) -> Result<Exported, String> {
        let mut world = source.world.clone();
        let ticks = settle::current_ticks(&world);
        let settled = self
            .settler
            .settle(&source.id, &mut world, &[ticks], None)
            .await?;
        let settled = &settled[&ticks].value;
        let Some(bounds) = settled.world.bounds() else {
            return Err(format!(
                "Minecraft keeps none of the blocks of this build at tick {ticks}"
            ));
        };
        let mut spec = Map::new();
        spec.insert("view".into(), json!("iso_se"));
        spec.insert("size".into(), json!("preview"));
        let resolved = shots::resolve_shot_sized(
            &spec,
            &settled.world,
            &settled.entities,
            &[("preview", size)],
        )?;
        let preview = self
            .renderer
            .render(
                &source.id,
                &[ToDraw {
                    shot: &resolved.shot,
                    tick: ticks,
                    settled,
                }],
                &CancellationToken::new(),
            )
            .await?
            .pop()
            .expect("one shot asked, one drawn");
        let y = self.settler.y_offset();
        let lift = |[x, by, z]: [i32; 3]| [x, by + y, z];
        Ok(Exported {
            blueprint: mcbench_world::blueprint::world_blueprint(&settled.world, y)?,
            mask: mcbench_world::live_views::render_mask(Some(bounds)),
            bounds: Bounds {
                min: lift(bounds.min),
                max: lift(bounds.max),
            },
            preview,
        })
    }

    async fn montage(
        &self,
        tiles: Vec<Vec<u8>>,
        cells: Vec<Cell>,
        size: [u32; 2],
    ) -> Result<Vec<u8>, String> {
        self.renderer.montage(tiles, cells, size).await
    }
}

// ---------------------------------------------------------------------------
// The viewer.

/// `sharePageHtml`'s fields.
#[derive(Clone, Copy, Debug)]
pub struct SharePage<'a> {
    /// `viewerId`: what [`SiteViewer::write_viewer`] gave.
    pub viewer_id: &'a str,
    /// `shareId`.
    pub share_id: &'a str,
    /// `title`.
    pub title: &'a str,
    /// `description`.
    pub description: &'a str,
    /// `url`: the page's address.
    pub url: &'a str,
    /// `image`: the card's address.
    pub image: &'a str,
    /// `labels`: each build's.
    pub labels: &'a [String],
}

/// The page's viewer (today's `share-viewer.js`), given by the caller.
pub trait SiteViewer: Send + Sync {
    /// `writeViewer(siteDir, ...)`: writes `viewer/<id>/` into the site unless it is there; the id.
    fn write_viewer(&self, site: &std::path::Path) -> Result<String, String>;
    /// `sharePageHtml({...})`: `b/<shareId>/index.html`.
    fn share_page_html(&self, page: &SharePage<'_>) -> String;
}

// ---------------------------------------------------------------------------
// Commands.

/// How a command failed (Node's `execFile` error).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Code {
    /// The program is not there (`ENOENT`).
    NotFound,
    /// It exited with this status.
    Exit(i32),
}

/// A command that failed, or any failure of the site's work: `code`,
/// `stderr` and `message` as Node's error has them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// `code`.
    pub code: Option<Code>,
    /// `stderr`.
    pub stderr: String,
    /// `message`.
    pub message: String,
}

impl Failure {
    /// A failure that is only its sentence (`new Error(message)`).
    pub fn plain(message: impl Into<String>) -> Failure {
        Failure {
            code: None,
            stderr: String::new(),
            message: message.into(),
        }
    }

    /// `said(error)`: what the command said, or the message.
    pub fn said(&self) -> String {
        if self.stderr.is_empty() {
            self.message.trim().to_string()
        } else {
            self.stderr.trim().to_string()
        }
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Failure {
        Failure::plain(message)
    }
}

fn io(error: std::io::Error) -> Failure {
    Failure::plain(error.to_string())
}

/// A command's output.
#[derive(Clone, Debug, Default)]
pub struct Ran {
    /// `stdout`.
    pub stdout: String,
    /// `stderr`.
    pub stderr: String,
}

/// What runs git and gh (`run`, as `execFile` does): the environment is
/// the process's with `env` over it; none of `cwd` is the process's folder.
#[async_trait]
pub trait Run: Send + Sync {
    /// Runs `command` with `args`; a command dropped before it ends is
    /// killed.
    async fn run(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Ran, Failure>;
}

/// The app's [`Run`]: child processes through [`mcbench_spawn::command`].
pub struct Processes;

#[async_trait]
impl Run for Processes {
    async fn run(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
    ) -> Result<Ran, Failure> {
        let mut child = mcbench_spawn::command(command);
        child
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = cwd {
            child.current_dir(cwd);
        }
        for (name, value) in env {
            child.env(name, value);
        }
        let mut child = tokio::process::Command::from(child);
        child.kill_on_drop(true);
        let output = match child.output().await {
            Ok(output) => output,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(Failure {
                    code: Some(Code::NotFound),
                    stderr: String::new(),
                    message: format!("spawn {command} ENOENT"),
                });
            }
            Err(error) => return Err(io(error)),
        };
        let ran = Ran {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        if output.status.success() {
            return Ok(ran);
        }
        let line = std::iter::once(command)
            .chain(args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        Err(Failure {
            code: output.status.code().map(Code::Exit),
            message: format!("Command failed: {line}\n{}", ran.stderr),
            stderr: ran.stderr,
        })
    }
}

// ---------------------------------------------------------------------------
// The site.

/// Why a call of [`Shares`] failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareError {
    /// Refused while the share is publishing (today's 409).
    Busy(&'static str),
    /// The site's failure, its sentence.
    Failed(String),
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareError::Busy(text) => f.write_str(text),
            ShareError::Failed(text) => f.write_str(text),
        }
    }
}

impl From<Failure> for ShareError {
    fn from(failure: Failure) -> ShareError {
        ShareError::Failed(failure.message)
    }
}

/// A share on the site ([`Shares::list`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedBuild {
    /// `shareId`.
    pub share_id: String,
    /// `title`.
    pub title: String,
    /// `kind`.
    pub kind: String,
    /// `labels`.
    pub labels: Vec<String>,
    /// `url`.
    pub url: String,
    /// `publishedAt`, in ms since the epoch: the time of the last commit
    /// that changed the build's `build.json`.
    pub published_at: i64,
}

/// What [`Shares`] is made with (today's constructor's options).
pub struct Options {
    /// The share folder (`DATA_DIR/share`); the site is its `site`.
    pub root: PathBuf,
    /// The repository's address ([`github_remote`]).
    pub remote: Box<dyn Fn(&str) -> String + Send + Sync>,
    /// What runs git and gh ([`Processes`]).
    pub run: Arc<dyn Run>,
    /// The export.
    pub export: Arc<dyn Export>,
    /// The viewer.
    pub viewer: Arc<dyn SiteViewer>,
    /// [`WAIT`].
    pub wait: Duration,
    /// [`POLL`].
    pub poll: Duration,
    /// [`NETWORK`].
    pub network: Duration,
    /// [`LOCAL`].
    pub local: Duration,
}

impl Options {
    /// The app's: the share folder `root` on GitHub, with today's limits.
    pub fn new(root: PathBuf, export: Arc<dyn Export>, viewer: Arc<dyn SiteViewer>) -> Options {
        Options {
            root,
            remote: Box::new(github_remote),
            run: Arc::new(Processes),
            export,
            viewer,
            wait: WAIT,
            poll: POLL,
            network: NETWORK,
            local: LOCAL,
        }
    }
}

/// The shared builds (`Shares`): the site, the publishes under way, and
/// the queue of the site's work.
pub struct Shares {
    site: PathBuf,
    remote: Box<dyn Fn(&str) -> String + Send + Sync>,
    run: Arc<dyn Run>,
    export: Arc<dyn Export>,
    viewer: Arc<dyn SiteViewer>,
    wait: Duration,
    poll: Duration,
    network: Duration,
    local: Duration,
    http: reqwest::Client,
    publishes: Mutex<HashSet<String>>,
    tree: tokio::sync::Mutex<()>,
}

/// Takes a share off the publishes under way when its publish ends, or is
/// dropped.
struct Publishing {
    shares: Arc<Shares>,
    id: String,
}

impl Drop for Publishing {
    fn drop(&mut self) {
        self.shares.publishes.lock().unwrap().remove(&self.id);
    }
}

/// The HTTP client of the wait for Pages: HTTP/1.1, no proxy, Mozilla's
/// roots with ring's crypto (as `mcbench-providers`' client, which Node's
/// fetch is to both).
fn http() -> reqwest::Client {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    let mut tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .expect("ring supports TLS 1.2 and 1.3")
    .with_root_certificates(roots)
    .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .http1_only()
        .no_proxy()
        .build()
        .expect("the HTTP client's settings are valid")
}

/// Fails when `file` in the site is a symbolic link (`refuseLink`), so
/// nothing is written or removed through one, even a link made after the
/// site's tree was checked.
fn refuse_link(file: &Path) -> Result<(), Failure> {
    match std::fs::symlink_metadata(file) {
        Ok(meta) if meta.file_type().is_symlink() => Err(Failure::plain(format!(
            "{} is a symbolic link. {PLAIN_FILES}",
            file.display()
        ))),
        _ => Ok(()),
    }
}

fn remove_all(path: &Path) -> Result<(), Failure> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other.map_err(io),
    }
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).expect("writing to memory");
    encoder.finish().expect("writing to memory")
}

/// The ms since the epoch of git's strict ISO date (`%cI`:
/// `YYYY-MM-DDTHH:MM:SS` and `Z` or `+HH:MM`).
fn iso_ms(text: &str) -> Result<i64, Failure> {
    let bad = || Failure::plain(format!("git gave the date {text:?}"));
    let number = |range: std::ops::Range<usize>| -> Result<i64, Failure> {
        text.get(range).and_then(|s| s.parse().ok()).ok_or_else(bad)
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let (hour, minute, second) = (number(11..13)?, number(14..16)?, number(17..19)?);
    let offset = match text.get(19..) {
        Some("Z") => 0,
        Some(zone) if zone.len() == 6 => {
            let sign = match &zone[..1] {
                "+" => 1,
                "-" => -1,
                _ => return Err(bad()),
            };
            sign * (number(20..22)? * 60 + number(23..25)?)
        }
        _ => return Err(bad()),
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let seconds = days * 86_400 + hour * 3600 + minute * 60 + second - offset * 60;
    Ok(seconds * 1000)
}

/// A share's `build.json` as the listing reads it.
#[derive(Deserialize)]
struct Listed {
    title: String,
    kind: String,
    url: String,
    builds: Vec<Labelled>,
}

#[derive(Deserialize)]
struct Labelled {
    label: String,
}

impl Shares {
    /// The shared builds of `options`.
    pub fn new(options: Options) -> Arc<Shares> {
        Arc::new(Shares {
            site: options.root.join("site"),
            remote: options.remote,
            run: options.run,
            export: options.export,
            viewer: options.viewer,
            wait: options.wait,
            poll: options.poll,
            network: options.network,
            local: options.local,
            http: http(),
            publishes: Mutex::new(HashSet::new()),
            tree: tokio::sync::Mutex::new(()),
        })
    }

    /// The site's folder.
    pub fn site(&self) -> &Path {
        &self.site
    }

    /// Whether share `id` is publishing.
    pub fn publishing(&self, id: &str) -> bool {
        self.publishes.lock().unwrap().contains(id)
    }

    /// `url(repo, id)`: the page's address while the site, cloned from
    /// `repo`, holds build `id`; none otherwise.
    pub async fn url(&self, repo: &str, id: &str) -> Result<Option<String>, ShareError> {
        let file = self.site.join("b").join(id).join("build.json");
        if !file.exists() || self.origin().await? != (self.remote)(repo) {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&file).map_err(io)?;
        let build: Value =
            serde_json::from_str(&text).map_err(|error| Failure::plain(error.to_string()))?;
        Ok(build["url"].as_str().map(String::from))
    }

    /// The address the site was cloned from, as written: `git remote
    /// get-url` would give it rewritten by the owner's
    /// `url.<base>.insteadOf` settings.
    async fn origin(&self) -> Result<String, Failure> {
        Ok(self
            .git(&["config", "--get", "remote.origin.url"], None, &[], false)
            .await?
            .stdout
            .trim()
            .to_string())
    }

    /// Runs `command` with `args`, killed after `limit`: a command still
    /// running then fails, naming itself.
    async fn exec(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        env: &[(String, String)],
        limit: Duration,
    ) -> Result<Ran, Failure> {
        match tokio::time::timeout(limit, self.run.run(command, args, cwd, env)).await {
            Ok(done) => done,
            Err(_) => Err(Failure::plain(format!(
                "`{command} {}` did not finish within {}",
                args[0],
                duration(limit)
            ))),
        }
    }

    /// git `args` in the site (`cwd`: elsewhere), with no prompts, within
    /// the network time limit when `network`. A sign-in git could not make
    /// fails saying how to let git use gh's sign-in.
    async fn git(
        &self,
        args: &[&str],
        cwd: Option<&Path>,
        env: &[(String, String)],
        network: bool,
    ) -> Result<Ran, Failure> {
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        let mut all: Vec<(String, String)> = NO_PROMPTS
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        all.extend(env.iter().cloned());
        let limit = if network { self.network } else { self.local };
        let cwd = cwd.unwrap_or(&self.site);
        match self.exec("git", &args, Some(cwd), &all, limit).await {
            Err(failure)
                if {
                    let said = failure.said().to_lowercase();
                    SIGN_IN_FAILED.iter().any(|text| said.contains(text))
                } =>
            {
                Err(Failure::plain(format!(
                    "git could not sign in to GitHub over HTTPS (git {}). Run `gh auth setup-git` in a terminal so git signs in with the GitHub CLI's account, then try again.",
                    args[0]
                )))
            }
            other => other,
        }
    }

    /// gh `args`. gh missing, or not signed in, fails saying so.
    async fn gh(&self, args: &[&str]) -> Result<Ran, Failure> {
        let args: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        match self.exec("gh", &args, None, &[], self.local).await {
            Err(failure) if failure.code == Some(Code::NotFound) => Err(Failure::plain(
                "The GitHub CLI (gh) is not installed: install it from https://cli.github.com and run `gh auth login`",
            )),
            // gh exits 4 when it has no sign-in; GitHub answers 401 to one that has expired.
            Err(failure)
                if failure.code == Some(Code::Exit(4)) || failure.said().contains("HTTP 401") =>
            {
                Err(Failure::plain(format!(
                    "The GitHub CLI is not signed in to GitHub: run `gh auth login` (gh {}: {})",
                    args.join(" "),
                    failure.said()
                )))
            }
            other => other,
        }
    }

    /// `chatShare(conversation)`: what a chat's share holds: its one build,
    /// labelled with its model.
    pub async fn chat_share(&self, source: &Source) -> Result<Share, String> {
        let exported = self
            .export
            .export_build(source, [PREVIEW_SIDE, PREVIEW_SIDE])
            .await?;
        Ok(Share {
            title: source.title.clone(),
            kind: "chat",
            description: format!(
                "Built by {} in mcbench-lite. Drag to look around.",
                source.model
            ),
            builds: vec![ShareBuild {
                label: source.model.clone(),
                blueprint: exported.blueprint,
                mask: exported.mask,
                bounds: exported.bounds,
            }],
            preview: exported.preview,
        })
    }

    /// `benchShare(bench, members)`: what a bench's share holds: the build
    /// of each of `members` whose world is not empty, in `order`, labelled
    /// as the app shows them; its card is every build's shot, each framed
    /// for its cell.
    pub async fn bench_share(
        &self,
        bench: &BenchSource,
        members: &[Source],
    ) -> Result<Share, String> {
        let shown: Vec<&Source> = bench
            .order
            .iter()
            .map(|id| {
                members
                    .iter()
                    .find(|member| member.id == *id)
                    .expect("every id of a bench's order is a member")
            })
            .filter(|member| member.world.size() > 0)
            .collect();
        let cells = shots::montage_cells(shown.len(), PREVIEW_SIDE)?;
        let mut exported = Vec::new();
        for (member, cell) in shown.iter().zip(&cells) {
            exported.push(
                self.export
                    .export_build(member, [cell.width, cell.height])
                    .await?,
            );
        }
        let previews = exported.iter().map(|build| build.preview.clone()).collect();
        let preview = self
            .export
            .montage(previews, cells, [PREVIEW_SIDE, PREVIEW_SIDE])
            .await?;
        Ok(Share {
            title: bench.title.clone(),
            kind: "bench",
            description: format!(
                "{} models built this in mcbench-lite. Switch between them and drag to look around.",
                shown.len()
            ),
            builds: exported
                .into_iter()
                .zip(&shown)
                .map(|(build, member)| ShareBuild {
                    label: bench_label(bench, member, members),
                    blueprint: build.blueprint,
                    mask: build.mask,
                    bounds: build.bounds,
                })
                .collect(),
            preview,
        })
    }

    /// `publish({ repo, sourceId, build, onState })`: publishes to `repo`
    /// the share of `subject` (id [`share_id`] of its id), telling
    /// `on_state` each move (`Exporting` before this returns). Refused at
    /// once while that share is publishing; otherwise the future ends live
    /// or failed, never with an error.
    pub fn publish(
        self: &Arc<Self>,
        repo: &str,
        subject: Subject,
        on_state: impl FnMut(PublishStep) + Send + 'static,
    ) -> Result<impl Future<Output = ()> + Send + 'static, ShareError> {
        let id = share_id(subject.id());
        if !self.publishes.lock().unwrap().insert(id.clone()) {
            return Err(ShareError::Busy("This build is already being published"));
        }
        let held = Publishing {
            shares: Arc::clone(self),
            id: id.clone(),
        };
        let mut publish = Publish::new(Box::new(on_state));
        let (shares, repo) = (Arc::clone(self), repo.to_string());
        Ok(async move {
            let _held = held;
            if let Err(error) = shares.carry(&mut publish, &repo, &subject, &id).await {
                eprintln!("[share] {}", error.message);
                publish
                    .to(PublishStep::Failed {
                        error: error.message,
                    })
                    .expect("a publish under way can fail");
            }
        })
    }

    async fn carry(
        &self,
        publish: &mut Publish,
        repo: &str,
        subject: &Subject,
        id: &str,
    ) -> Result<(), Failure> {
        let share = match subject {
            Subject::Chat(source) => self.chat_share(source).await?,
            Subject::Bench { bench, members } => self.bench_share(bench, members).await?,
        };
        publish.to(PublishStep::Publishing)?;
        let version = random_uuid();
        let url = {
            let _turn = self.tree.lock().await;
            self.update(repo).await?;
            let branch = self
                .git(&["branch", "--show-current"], None, &[], false)
                .await?;
            let url = format!(
                "{}b/{id}/",
                self.pages_base(repo, branch.stdout.trim()).await?
            );
            self.changing(self.write_share(&share, id, &version, &url))
                .await?;
            url
        };
        publish.to(PublishStep::Waiting)?;
        self.served(&url, &version).await?;
        publish.to(PublishStep::Live { url })?;
        Ok(())
    }

    /// The publish's change of the site: its files written, committed and
    /// pushed.
    async fn write_share(
        &self,
        share: &Share,
        id: &str,
        version: &str,
        url: &str,
    ) -> Result<(), Failure> {
        let folder = self.site.join("b").join(id);
        for file in [".nojekyll", "viewer", "b", &format!("b/{id}")] {
            refuse_link(&self.site.join(file))?;
        }
        std::fs::write(self.site.join(".nojekyll"), "").map_err(io)?;
        let viewer_id = self.viewer.write_viewer(&self.site)?;
        remove_all(&folder)?;
        std::fs::create_dir_all(&folder).map_err(io)?;
        let mut builds = Vec::new();
        let mut labels = Vec::new();
        for (i, build) in share.builds.iter().enumerate() {
            let name = format!("{i}.bin.gz");
            std::fs::write(folder.join(&name), gzip(&build.blueprint)).map_err(io)?;
            builds.push(json!({
                "label": build.label,
                "blueprint": name,
                "mask": build.mask,
                "bounds": build.bounds.to_json(),
            }));
            labels.push(build.label.clone());
        }
        let json = json!({
            "title": share.title,
            "kind": share.kind,
            "version": version,
            "url": url,
            "builds": builds,
        });
        std::fs::write(folder.join("build.json"), mcbench_json::stringify(&json)).map_err(io)?;
        std::fs::write(folder.join("preview.jpg"), &share.preview).map_err(io)?;
        let image = format!("{url}preview.jpg");
        let page = self.viewer.share_page_html(&SharePage {
            viewer_id: &viewer_id,
            share_id: id,
            title: &share.title,
            description: &share.description,
            url,
            image: &image,
            labels: &labels,
        });
        std::fs::write(folder.join("index.html"), page).map_err(io)?;
        self.push(&format!("Share {id}: {}", share.title)).await
    }

    /// `pagesBase(repo, branch)`: the Pages site's address, ending in `/`.
    /// Builds are pushed to the root of `branch`; a site that Pages builds
    /// from another branch or folder would never serve them, so it fails,
    /// saying what to set. One that a GitHub Actions workflow deploys
    /// serves what that workflow deploys.
    async fn pages_base(&self, repo: &str, branch: &str) -> Result<String, Failure> {
        let setting = format!(
            "in the repository's Settings → Pages, set Source to \"Deploy from a branch\", branch {branch}, folder / (root)"
        );
        let pages = match self.gh(&["api", &format!("repos/{repo}/pages")]).await {
            Ok(ran) => ran,
            Err(failure) if failure.said().contains("HTTP 404") => {
                return Err(Failure::plain(format!(
                    "GitHub Pages is not set up for {repo} ({}): {setting}",
                    failure.said()
                )));
            }
            Err(failure) => return Err(failure),
        };
        let pages: Value = serde_json::from_str(&pages.stdout)
            .map_err(|error| Failure::plain(error.to_string()))?;
        if pages["build_type"] != "workflow"
            && (pages["source"]["branch"] != branch || pages["source"]["path"] != "/")
        {
            // As a template writes it: a field GitHub left out is `undefined`.
            let text = |value: &Value| match value {
                Value::Null => "undefined".to_string(),
                Value::String(text) => text.clone(),
                other => mcbench_json::stringify(other),
            };
            return Err(Failure::plain(format!(
                "GitHub Pages for {repo} serves the {} branch's {} folder, but builds are pushed to the root of {branch}: {setting}",
                text(&pages["source"]["branch"]),
                text(&pages["source"]["path"]),
            )));
        }
        pages["html_url"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Failure::plain(format!("GitHub gave no html_url for {repo}'s Pages")))
    }

    /// `served(url, version)`: waits until Pages serves `version` at
    /// `url`. Pages deploys a push within a minute or so; its CDN is asked
    /// past its cache each time.
    async fn served(&self, url: &str, version: &str) -> Result<(), Failure> {
        let deadline = Instant::now() + self.wait;
        loop {
            let address = format!("{url}build.json?check={}", random_uuid());
            // `cache: 'no-store'`, as fetch sends it.
            let response = self
                .http
                .get(&address)
                .header("pragma", "no-cache")
                .header("cache-control", "no-cache")
                .send()
                .await
                .map_err(|_| Failure::plain("fetch failed"))?;
            if response.status().is_success() {
                let body = response
                    .text()
                    .await
                    .map_err(|_| Failure::plain("fetch failed"))?;
                let build: Value = serde_json::from_str(&body)
                    .map_err(|error| Failure::plain(error.to_string()))?;
                if build["version"] == version {
                    return Ok(());
                }
            }
            if Instant::now() >= deadline {
                let minutes = (self.wait.as_millis() as f64 / 60_000.0 + 0.5).floor();
                return Err(Failure::plain(format!(
                    "The files are pushed, but GitHub Pages has not served them yet after {} minutes: {url}",
                    mcbench_json::stringify(&json!(minutes))
                )));
            }
            tokio::time::sleep(self.poll).await;
        }
    }

    /// `list(repo)`: the shares on the site cloned from `repo`, newest
    /// first.
    pub async fn list(&self, repo: &str) -> Result<Vec<SharedBuild>, ShareError> {
        let _turn = self.tree.lock().await;
        self.update(repo).await?;
        Ok(self.listed().await?)
    }

    async fn listed(&self) -> Result<Vec<SharedBuild>, Failure> {
        let builds = self.site.join("b");
        if !builds.exists() {
            return Ok(Vec::new());
        }
        // One pass over the history, newest commit first: a build's first
        // appearance is its last change.
        let log = self
            .git(
                &[
                    "log",
                    "--format=%x00%cI",
                    "--name-only",
                    "--",
                    "b/*/build.json",
                ],
                None,
                &[],
                false,
            )
            .await?;
        let mut dates: IndexMap<String, String> = IndexMap::new();
        let mut date = String::new();
        for line in log.stdout.split('\n') {
            if let Some(rest) = line.strip_prefix('\0') {
                date = rest.to_string();
            } else if !line.is_empty() {
                let id = line
                    .split('/')
                    .nth(1)
                    .expect("git lists paths under b/*/")
                    .to_string();
                dates.entry(id).or_insert_with(|| date.clone());
            }
        }
        let mut listed = Vec::new();
        for (id, published) in dates {
            let file = builds.join(&id).join("build.json");
            if !file.exists() {
                continue;
            }
            let text = std::fs::read_to_string(&file).map_err(io)?;
            let value: Value =
                serde_json::from_str(&text).map_err(|error| Failure::plain(error.to_string()))?;
            if !value["builds"].is_array() {
                return Err(Failure::plain(format!(
                    "The shared build b/{id} was published in an older format: publish it again or remove it"
                )));
            }
            let build: Listed =
                serde_json::from_value(value).map_err(|error| Failure::plain(error.to_string()))?;
            listed.push(SharedBuild {
                share_id: id,
                title: build.title,
                kind: build.kind,
                labels: build.builds.into_iter().map(|build| build.label).collect(),
                url: build.url,
                published_at: iso_ms(&published)?,
            });
        }
        Ok(listed)
    }

    /// `remove(repo, id)`: removes build `id` from `repo` and pushes: true,
    /// or false when the site has no such build. Refused while it is
    /// publishing.
    pub async fn remove(&self, repo: &str, id: &str) -> Result<bool, ShareError> {
        if self.publishing(id) {
            return Err(ShareError::Busy("Wait for the build to finish publishing"));
        }
        let _turn = self.tree.lock().await;
        self.update(repo).await?;
        Ok(self
            .changing(async {
                let folder = self.site.join("b").join(id);
                refuse_link(&self.site.join("b"))?;
                refuse_link(&folder)?;
                if !folder.exists() {
                    return Ok(false);
                }
                std::fs::remove_dir_all(&folder).map_err(io)?;
                self.push(&format!("Remove {id}")).await?;
                Ok(true)
            })
            .await?)
    }

    /// `inTree`'s `change`: a copy whose change failed part-way (it may
    /// hold half-written files or a commit not pushed) is dropped, so the
    /// next job starts from a fresh clone.
    async fn changing<T>(
        &self,
        change: impl Future<Output = Result<T, Failure>>,
    ) -> Result<T, Failure> {
        let changed = change.await;
        if changed.is_err() {
            remove_all(&self.site)?;
        }
        changed
    }

    /// The site is public: its commits are made as the signed-in GitHub
    /// account at its noreply address, not as git's own default, the
    /// owner's name and machine.
    async fn author(&self) -> Result<Vec<(String, String)>, Failure> {
        let ran = self.gh(&["api", "user", "--jq", AUTHOR_QUERY]).await?;
        let mut lines = ran.stdout.trim().split('\n');
        // A line gh did not give is `undefined`, as execFile's env writes it.
        let mut line = || lines.next().unwrap_or("undefined").to_string();
        let (name, email) = (line(), line());
        Ok(vec![
            ("GIT_AUTHOR_NAME".into(), name.clone()),
            ("GIT_AUTHOR_EMAIL".into(), email.clone()),
            ("GIT_COMMITTER_NAME".into(), name),
            ("GIT_COMMITTER_EMAIL".into(), email),
        ])
    }

    async fn push(&self, message: &str) -> Result<(), Failure> {
        self.git(&["add", "-A"], None, &[], false).await?;
        let author = self.author().await?;
        self.git(&["commit", "--quiet", "-m", message], None, &author, false)
            .await?;
        self.git(&["push", "--quiet"], None, &[], true).await?;
        Ok(())
    }

    /// `update(repo)`: clones `repo` to the site, or fetches it and moves
    /// the copy to what was fetched, then checks its tree. A clone that
    /// fails is removed: it may be partial. A copy that cannot be brought
    /// up to date is kept as it is: it still holds what was last pushed,
    /// so [`Shares::url`] answers from it. A copy cloned from another
    /// repository is dropped first.
    async fn update(&self, repo: &str) -> Result<(), Failure> {
        let remote = (self.remote)(repo);
        if self.site.exists() && self.origin().await? != remote {
            std::fs::remove_dir_all(&self.site).map_err(io)?;
        }
        if !self.site.exists() {
            let parent = self.site.parent().expect("the site is in the share folder");
            std::fs::create_dir_all(parent).map_err(io)?;
            let site = self.site.to_string_lossy().into_owned();
            if let Err(failure) = self
                .git(
                    &["clone", "--quiet", &remote, &site],
                    Some(parent),
                    &[],
                    true,
                )
                .await
            {
                remove_all(&self.site)?;
                return Err(Failure::plain(format!(
                    "Could not get a copy of {repo} from GitHub: {}",
                    failure.said()
                )));
            }
        } else {
            if let Err(failure) = self.git(&["fetch", "--quiet"], None, &[], true).await {
                return Err(Failure::plain(format!(
                    "Could not bring the site's copy up to date with {repo} on GitHub: {}",
                    failure.said()
                )));
            }
            self.git(
                &["reset", "--quiet", "--hard", "@{upstream}"],
                None,
                &[],
                false,
            )
            .await?;
        }
        self.plain_files(repo).await
    }

    /// A site whose tree holds a symbolic link or a submodule is removed and
    /// fails, naming them: the publish writes `.nojekyll`, `viewer/` and
    /// `b/<id>/` and removes `b/<id>/`, and through a link those would
    /// reach files outside the data folder.
    async fn plain_files(&self, repo: &str) -> Result<(), Failure> {
        // `-z`: each entry is "<mode> <object> <stage>\t<path>", ended by NUL.
        let ran = self
            .git(&["ls-files", "-s", "-z"], None, &[], false)
            .await?;
        let links: Vec<&str> = ran
            .stdout
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .filter(|entry| {
                let mode = entry.split(' ').next().expect("split gives one piece");
                LINK_MODES.contains(&mode)
            })
            .map(|entry| {
                entry
                    .split('\t')
                    .nth(1)
                    .expect("an entry of ls-files -s has its path")
            })
            .collect();
        if links.is_empty() {
            return Ok(());
        }
        std::fs::remove_dir_all(&self.site).map_err(io)?;
        Err(Failure::plain(format!(
            "{repo} holds symbolic links or submodules ({}). {PLAIN_FILES} Remove them from the repository and try again.",
            links.join(", ")
        )))
    }
}
