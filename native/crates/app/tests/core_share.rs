//! Sharing through the core (`Core::command`, `src/core/share.rs`): the
//! five share commands with a recording `Window`, each ending as the seam
//! says, with today's refusals in today's order and today's sentences
//! (`test/share-routes.test.js`, ported by name), a publish that panics,
//! one that goes on while the window looks elsewhere and one the core's
//! stop ends, and the app's own export before setup.
//!
//! Nothing runs git or gh and nothing leaves the machine: the share
//! module's commands go to a fake ([`FakeRun`]) whose "remote" is a folder
//! (a clone copies it into the site, a push copies the site back), `gh`
//! answers a Pages site on a local listener, which serves the remote
//! folder's files, and the export and the viewer are fakes. The model is a
//! fake whose turns wait behind a gate, so a turn can be held running. The
//! test waits on the window's condition variable, never on time.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::share::Stand;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Provider, ProviderError,
    ProviderId,
};
use mcbench_store::{BenchMember, Content, NewBench, NewConversation};
use mcbench_tools::share::{
    self, Bounds, Code, Export, Exported, Failure, Options, Ran, Run, SharePage, SiteViewer,
    Source, share_id,
};
use mcbench_ui::{
    BenchChoice, Command, Outgoing, PublishStep, Published, PublishedBuild, Resolution, ShareRead,
    Subject, Target, Update, Window,
};

const REPO: &str = "owner/builds";
const REPO_NEEDED: &str = "Set the GitHub repository for shared builds in Settings";
const NOT_SHARED: &str = "This build is not shared";
const BUSY: &str = "This build is already being published";
const STILL_PUBLISHING: &str = "Wait for the build to finish publishing";
const STOPPED: &str = "The publish stopped before the page was live";

// ---------------------------------------------------------------------------
// The model: every turn waits for its gate, then fails.

struct Gated(watch::Receiver<bool>);

#[async_trait]
impl Provider for Gated {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        _: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        if !request.tools.is_empty() {
            let mut gate = self.0.clone();
            tokio::select! {
                _ = gate.wait_for(|open| *open) => {}
                _ = cancel.cancelled() => {}
            }
        }
        Err(ProviderError::new("The model failed"))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts::unknown())
    }
    async fn close_session(&self, _: &str) {}
}

struct Fake(Arc<Gated>);

#[async_trait]
impl Backend for Fake {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(self.0.clone())
    }
    fn tools(&self) -> mcbench_tools::Tools {
        mcbench_tools::Tools::of(Vec::new())
    }
    fn sync_attachments(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    async fn reset_sandbox(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn network_access(&self) -> bool {
        false
    }
    async fn shutdown(&self) {}
}

// ---------------------------------------------------------------------------
// git and gh.

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == ".git" {
            continue;
        }
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn ran(stdout: impl Into<String>) -> Result<Ran, Failure> {
    Ok(Ran {
        stdout: stdout.into(),
        stderr: String::new(),
    })
}

/// git and gh as the site's work runs them (see the module's doc). The
/// site's origin is kept in its `.git/origin`.
struct FakeRun {
    remote: PathBuf,
    pages: String,
    /// The commits pushed, oldest first: the share id each changed.
    commits: Mutex<Vec<String>>,
}

#[async_trait]
impl Run for FakeRun {
    async fn run(
        &self,
        command: &str,
        args: &[String],
        cwd: Option<&Path>,
        _: &[(String, String)],
    ) -> Result<Ran, Failure> {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        // gh runs in the process's folder, git in one of the share's.
        let cwd = || cwd.expect("git runs in a folder of the share's");
        match (command, args.as_slice()) {
            ("gh", ["api", "user", ..]) => ran("owner\n42+owner@users.noreply.github.com\n"),
            ("gh", ["api", pages]) if pages.ends_with("/pages") => ran(json!({
                "html_url": self.pages,
                "build_type": "legacy",
                "source": { "branch": "main", "path": "/" },
            })
            .to_string()),
            ("git", ["clone", "--quiet", remote, site]) => {
                if remote.contains("missing") {
                    let stderr = format!("fatal: repository '{remote}' does not exist\n");
                    return Err(Failure {
                        code: Some(Code::Exit(128)),
                        message: format!(
                            "Command failed: git clone --quiet {remote} {site}\n{stderr}"
                        ),
                        stderr,
                    });
                }
                let site = Path::new(site);
                if self.remote.exists() {
                    copy_tree(&self.remote, site);
                }
                std::fs::create_dir_all(site.join(".git")).unwrap();
                std::fs::write(site.join(".git").join("origin"), remote).unwrap();
                ran("")
            }
            ("git", ["config", "--get", "remote.origin.url"]) => {
                match std::fs::read_to_string(cwd().join(".git").join("origin")) {
                    Ok(origin) => ran(format!("{origin}\n")),
                    Err(_) => Err(Failure {
                        code: Some(Code::Exit(1)),
                        stderr: String::new(),
                        message: "Command failed: git config --get remote.origin.url\n".into(),
                    }),
                }
            }
            ("git", ["fetch" | "reset" | "add", ..]) => ran(""),
            ("git", ["ls-files", ..]) => ran(""),
            ("git", ["branch", "--show-current"]) => ran("main\n"),
            ("git", ["commit", "--quiet", "-m", message]) => {
                let id = message
                    .strip_prefix("Share ")
                    .map(|rest| rest.split(':').next().unwrap())
                    .or_else(|| message.strip_prefix("Remove "))
                    .unwrap()
                    .to_string();
                self.commits.lock().unwrap().push(id);
                ran("")
            }
            ("git", ["push", ..]) => {
                let _ = std::fs::remove_dir_all(&self.remote);
                copy_tree(cwd(), &self.remote);
                ran("")
            }
            ("git", ["log", ..]) => {
                let commits = self.commits.lock().unwrap();
                let mut log = String::new();
                for (at, id) in commits.iter().enumerate().rev() {
                    log.push_str(&format!(
                        "\0 2026-11-18T10:{:02}:{:02}Z\n\nb/{id}/build.json\n",
                        at / 60,
                        at % 60
                    ));
                }
                ran(log.replace("\0 ", "\0"))
            }
            _ => panic!("the fake has no {command} {args:?}"),
        }
    }
}

/// GitHub Pages: a local listener serving the remote folder's files.
fn pages(remote: PathBuf) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            loop {
                let mut header = String::new();
                reader.read_line(&mut header).unwrap();
                if header == "\r\n" || header.is_empty() {
                    break;
                }
            }
            let path = line.split(' ').nth(1).unwrap().split('?').next().unwrap();
            let response = match std::fs::read(remote.join(path.trim_start_matches('/'))) {
                Ok(body) => {
                    let mut response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    )
                    .into_bytes();
                    response.extend(body);
                    response
                }
                Err(_) => {
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        .to_vec()
                }
            };
            stream.write_all(&response).unwrap();
        }
    });
    address
}

// ---------------------------------------------------------------------------
// The export and the viewer.

#[derive(Default)]
struct FakeExport {
    gate: Mutex<Option<watch::Receiver<bool>>>,
    panics: AtomicBool,
    started: AtomicUsize,
    started_changed: Condvar,
    started_lock: Mutex<()>,
}

impl FakeExport {
    /// Exports wait until the returned sender is set.
    fn hold(&self) -> watch::Sender<bool> {
        let (open, gate) = watch::channel(false);
        *self.gate.lock().unwrap() = Some(gate);
        open
    }

    fn let_go(&self, open: &watch::Sender<bool>) {
        *self.gate.lock().unwrap() = None;
        open.send_replace(true);
    }

    /// Blocks until `count` exports have started.
    fn wait_started(&self, count: usize) {
        let lock = self.started_lock.lock().unwrap();
        let (_lock, timeout) = self
            .started_changed
            .wait_timeout_while(lock, Duration::from_secs(60), |_| {
                self.started.load(Ordering::SeqCst) < count
            })
            .unwrap();
        assert!(!timeout.timed_out(), "{count} exports did not start");
    }
}

#[async_trait]
impl Export for FakeExport {
    async fn export_build(&self, _: &Source, _: [u32; 2]) -> Result<Exported, String> {
        {
            let _lock = self.started_lock.lock().unwrap();
            self.started.fetch_add(1, Ordering::SeqCst);
            self.started_changed.notify_all();
        }
        assert!(!self.panics.load(Ordering::SeqCst), "the export panicked");
        let gate = self.gate.lock().unwrap().clone();
        if let Some(mut gate) = gate {
            let _ = gate.wait_for(|open| *open).await;
        }
        Ok(Exported {
            blueprint: b"blueprint".to_vec(),
            mask: json!({ "minX": -64, "maxX": 63, "minZ": -64, "maxZ": 63 }),
            bounds: Bounds {
                min: [0, 64, 0],
                max: [0, 64, 0],
            },
            preview: b"jpeg".to_vec(),
        })
    }
    async fn montage(
        &self,
        _: Vec<Vec<u8>>,
        _: Vec<mcbench_render::Cell>,
        _: [u32; 2],
    ) -> Result<Vec<u8>, String> {
        Ok(b"montage".to_vec())
    }
}

struct FakeViewer;

impl SiteViewer for FakeViewer {
    fn write_viewer(&self, _: &Path) -> Result<String, String> {
        Ok("v1".into())
    }
    fn share_page_html(&self, _: &SharePage<'_>) -> String {
        "<html>".into()
    }
}

// ---------------------------------------------------------------------------
// The app.

#[derive(Default)]
struct Recording {
    updates: Mutex<Vec<Update>>,
    arrived: Condvar,
}

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.updates.lock().unwrap().push(update);
        self.arrived.notify_all();
    }
}

fn is_share(update: &Update) -> bool {
    matches!(
        update,
        Update::ShareRead { .. }
            | Update::PublishStep { .. }
            | Update::ShareChanged { .. }
            | Update::Shares(_)
            | Update::ShareRemoved { .. }
            | Update::Error(_)
    )
}

struct App {
    core: Core,
    window: Arc<Recording>,
    data: PathBuf,
    export: Arc<FakeExport>,
    remote: Arc<Mutex<String>>,
    pages: String,
    turn_gate: watch::Sender<bool>,
}

enum Site {
    /// The fakes of the module's doc.
    Fakes,
    /// The app's own adapter, git and gh.
    Own,
}

impl App {
    fn new(name: &str, repo: Option<&str>) -> App {
        App::with(name, repo, Site::Fakes)
    }

    fn with(name: &str, repo: Option<&str>, site: Site) -> App {
        let data = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("core-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&data);
        std::fs::create_dir_all(&data).unwrap();
        let mut settings = json!({
            "providers": { "openrouter": { "apiKey": "synthetic-test-key", "enabled": true } },
        });
        if let Some(repo) = repo {
            settings["shareRepo"] = json!(repo);
        }
        std::fs::write(data.join("settings.json"), settings.to_string()).unwrap();
        let export = Arc::new(FakeExport::default());
        let remote_folder = data.join("remote");
        let pages = pages(remote_folder.clone());
        let remote = Arc::new(Mutex::new("fake://owner/builds".to_string()));
        let share = match site {
            Site::Own => None,
            Site::Fakes => {
                let address = Arc::clone(&remote);
                let export = export.clone();
                let pages = pages.clone();
                let root = data.join("share");
                Some(Stand {
                    jar: mcbench_setup::pins::bluemap(),
                    options: Box::new(move |_| Options {
                        root,
                        remote: Box::new(move |_| address.lock().unwrap().clone()),
                        run: Arc::new(FakeRun {
                            remote: remote_folder,
                            pages,
                            commits: Mutex::new(Vec::new()),
                        }),
                        export,
                        viewer: Arc::new(FakeViewer),
                        wait: Duration::from_secs(60),
                        poll: Duration::from_millis(20),
                        network: Duration::from_secs(60),
                        local: Duration::from_secs(60),
                    }),
                })
            }
        };
        let (turn_gate, gate) = watch::channel(false);
        let core = Core::start(Parts {
            data: data.clone(),
            program: std::env::current_exe().unwrap(),
            host: Arc::new(mcbench_store::SystemHost),
            shipped: Box::new(|| Err("unused".into())),
            client: mcbench_setup::fetch::client(),
            backend: Some(Arc::new(Fake(Arc::new(Gated(gate))))),
            installer: None,
            share,
            live: Default::default(),
        })
        .unwrap();
        let app = App {
            core,
            window: Arc::new(Recording::default()),
            data,
            export,
            remote,
            pages,
            turn_gate,
        };
        app.on(|core| async move { core.started().await });
        mcbench_ui::Core::attach(&app.core, app.window.clone());
        app
    }

    fn on<T: Send + 'static, F: std::future::Future<Output = T> + Send + 'static>(
        &self,
        work: impl FnOnce(Core) -> F,
    ) -> T {
        let handle = self.core.handle().clone();
        handle
            .block_on(handle.spawn(work(self.core.clone())))
            .unwrap()
    }

    fn len(&self) -> usize {
        self.window.updates.lock().unwrap().len()
    }

    /// Blocks until the updates from `from` meet `done` (a minute at most).
    fn wait(&self, from: usize, what: &str, done: impl Fn(&[Update]) -> bool) -> Vec<Update> {
        let updates = self.window.updates.lock().unwrap();
        let (updates, timeout) = self
            .window
            .arrived
            .wait_timeout_while(updates, Duration::from_secs(60), |u| !done(&u[from..]))
            .unwrap();
        assert!(!timeout.timed_out(), "{what}: {:?}", &updates[from..]);
        updates[from..].to_vec()
    }

    /// Runs `command` to its end: the share updates it published.
    fn command(&self, command: Command) -> Vec<Update> {
        let from = self.len();
        self.on(|core| async move { core.command(command).await });
        self.window.updates.lock().unwrap()[from..]
            .iter()
            .filter(|u| is_share(u))
            .cloned()
            .collect()
    }

    fn read(&self, subject: &Subject) -> Result<ShareRead, String> {
        match &self.command(Command::ReadShare {
            subject: subject.clone(),
        })[..]
        {
            [Update::ShareRead { subject: s, read }] if s == subject => read.clone(),
            other => panic!("{other:?}"),
        }
    }

    /// Publishes `subject`: every step told for it, to the last.
    fn publish(&self, subject: &Subject) -> Vec<PublishStep> {
        let from = self.len();
        self.command(Command::Publish {
            subject: subject.clone(),
        });
        self.ended(from, subject)
    }

    /// The steps of `subject` from `from`, once one is the last.
    fn ended(&self, from: usize, subject: &Subject) -> Vec<PublishStep> {
        let steps = |updates: &[Update]| -> Vec<PublishStep> {
            updates
                .iter()
                .filter_map(|u| match u {
                    Update::PublishStep { subject: s, step } if s == subject => Some(step.clone()),
                    _ => None,
                })
                .collect()
        };
        let updates = self.wait(from, "the publish's end", |u| {
            steps(u)
                .iter()
                .any(|s| matches!(s, PublishStep::Live { .. } | PublishStep::Failed { .. }))
        });
        steps(&updates)
    }

    fn page(&self, id: &str) -> String {
        format!("{}b/{}/", self.pages, share_id(id))
    }

    fn live(&self, id: &str) -> Vec<PublishStep> {
        vec![
            PublishStep::Exporting,
            PublishStep::Publishing,
            PublishStep::Waiting,
            PublishStep::Live { url: self.page(id) },
        ]
    }

    fn on_site(&self, id: &str) -> bool {
        self.data
            .join("remote")
            .join("b")
            .join(share_id(id))
            .join("build.json")
            .exists()
    }

    fn new_chat(&self, blocks: bool) -> String {
        let store = Arc::clone(self.core.store());
        let live = store
            .create_conversation(
                "openrouter",
                "model/x",
                NewConversation {
                    title: "New build".into(),
                    effort: None,
                    resolution: 1,
                    bench: None,
                },
            )
            .unwrap();
        let mut chat = live.lock().unwrap();
        if blocks {
            stone(&mut chat.world);
        }
        store.save(&mut chat).unwrap();
        chat.id.0.clone()
    }

    /// A bench of three models a, b and c, of which those in `built` placed
    /// a block: its id, its order and its members' (id, model).
    fn new_bench(
        &self,
        built: &[&str],
        blind: bool,
    ) -> (String, Vec<String>, Vec<(String, String)>) {
        let store = Arc::clone(self.core.store());
        let id = format!("bench-{}", store.host().uuid());
        let (bench, members) = store
            .create_bench(
                &id,
                NewBench {
                    title: "A bridge".into(),
                    blind,
                    resolution: Some(1),
                    content: Content::Text("Build a bridge".into()),
                    prompts: Vec::new(),
                    members: ["a", "b", "c"]
                        .iter()
                        .map(|name| BenchMember {
                            provider: "openrouter".into(),
                            model: format!("model/{name}"),
                            effort: None,
                        })
                        .collect(),
                },
            )
            .unwrap();
        let mut ids = Vec::new();
        for member in members {
            let mut member = member.lock().unwrap();
            if built.iter().any(|name| member.model.ends_with(name)) {
                stone(&mut member.world);
                store.save(&mut member).unwrap();
            }
            ids.push((member.id.0.clone(), member.model.clone()));
        }
        let order = bench
            .order
            .as_array()
            .unwrap()
            .iter()
            .map(|id| id.as_str().unwrap().to_string())
            .collect();
        (bench.id, order, ids)
    }
}

fn stone(world: &mut mcbench_store::World) {
    world
        .set(
            [0, 0, 0],
            mcbench_world::BlockState::new("minecraft:stone", &[]),
        )
        .unwrap();
}

fn failed(error: &str) -> Vec<PublishStep> {
    vec![PublishStep::Failed {
        error: error.into(),
    }]
}

fn message(target: Target, text: &str) -> Outgoing {
    Outgoing {
        target,
        text: text.into(),
        attachments: Vec::new(),
        selections: Vec::new(),
        prompts: Vec::new(),
        effort: None,
        resolution: Resolution::Standard,
        edit_of: None,
    }
}

fn changed(updates: &[Update], subject: &Subject, url: Option<String>) -> bool {
    matches!(updates, [Update::ShareChanged { subject: s, url: u }] if s == subject && *u == url)
}

fn error_then_changed(updates: &[Update], error: &str, subject: &Subject) -> bool {
    matches!(updates, [Update::Error(e), Update::ShareChanged { subject: s, url: None }] if e == error && s == subject)
}

fn listed(app: &App) -> Published {
    match &app.command(Command::ReadShares)[..] {
        [Update::Shares(Ok(published))] => published.clone(),
        other => panic!("{other:?}"),
    }
}

fn removed(app: &App, id: &str) -> Vec<Update> {
    app.command(Command::RemoveShare {
        share_id: id.into(),
    })
}

/// `listed`'s rows of `ids`, `publishedAt` checked and left out (as 0).
fn rows(published: &Published, ids: &[String]) -> Vec<PublishedBuild> {
    let shares: Vec<String> = ids.iter().map(|id| share_id(id)).collect();
    published
        .builds
        .iter()
        .filter(|b| shares.contains(&b.share_id))
        .map(|b| {
            assert!(b.published_at > 0, "{b:?}");
            PublishedBuild {
                published_at: 0,
                ..b.clone()
            }
        })
        .collect()
}

/// A row as today's route answers it, field by field.
#[allow(clippy::too_many_arguments)]
fn row(
    app: &App,
    id: &str,
    title: &str,
    kind: &str,
    labels: &[&str],
    chat: Option<&str>,
    bench: Option<&str>,
    archived: bool,
) -> PublishedBuild {
    PublishedBuild {
        share_id: share_id(id),
        title: title.into(),
        kind: kind.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        url: app.page(id),
        published_at: 0,
        conversation_id: chat.map(String::from),
        bench_id: bench.map(String::from),
        archived,
    }
}

// ---------------------------------------------------------------------------
// Today's route tests, by name.

#[test]
fn a_publish_is_refused_before_it_starts_without_a_repository_with_an_empty_build_or_under_a_running_turn()
 {
    let bare = App::new("refused-bare", None);
    let id = Subject::Chat(bare.new_chat(true));
    assert_eq!(bare.publish(&id), failed(REPO_NEEDED));
    assert_eq!(
        bare.read(&id),
        Ok(ShareRead {
            repo: None,
            url: None
        })
    );
    // Not found comes before the repository.
    let missing = Subject::Chat("missing".into());
    assert_eq!(bare.publish(&missing), failed("Conversation not found"));
    assert_eq!(bare.read(&missing), Err("Conversation not found".into()));

    let app = App::new("refused", Some(REPO));
    assert_eq!(
        app.publish(&Subject::Chat(app.new_chat(false))),
        failed("The build is empty: there is nothing to share")
    );
    // Nothing built comes before a running turn.
    let empty = app.new_chat(false);
    let built = app.new_chat(true);
    for (chat, refused) in [
        (&empty, "The build is empty: there is nothing to share"),
        (&built, "Wait for the running turn before sharing"),
    ] {
        let from = app.len();
        app.command(Command::Send(message(
            Target::Chat {
                id: chat.clone(),
                provider: "openrouter".into(),
                model: "model/x".into(),
            },
            "Build a tower",
        )));
        app.wait(from, "the turn is joined", |u| {
            u.iter()
                .any(|u| matches!(u, Update::TurnJoined { chat: c, .. } if c == chat))
        });
        assert_eq!(app.publish(&Subject::Chat(chat.clone())), failed(refused));
    }
    app.turn_gate.send_replace(true);
    for chat in [empty, built.clone()] {
        app.on(|core| async move { core.turns().free(&chat).await });
    }
    assert_eq!(app.publish(&Subject::Chat(built.clone())), app.live(&built));
    assert_eq!(app.publish(&missing), failed("Conversation not found"));
}

#[test]
fn a_publish_streams_its_states_the_chat_then_has_its_address_and_a_removal_takes_it_off_the_site()
{
    let app = App::new("streams", Some(REPO));
    let id = app.new_chat(true);
    let chat = Subject::Chat(id.clone());
    assert_eq!(
        app.read(&chat),
        Ok(ShareRead {
            repo: Some(REPO.into()),
            url: None
        })
    );
    assert_eq!(app.publish(&chat), app.live(&id));
    assert_eq!(
        app.read(&chat),
        Ok(ShareRead {
            repo: Some(REPO.into()),
            url: Some(app.page(&id))
        })
    );
    assert!(app.on_site(&id));

    let gone = app.command(Command::Unshare {
        subject: chat.clone(),
    });
    assert!(changed(&gone, &chat, None), "{gone:?}");
    assert!(!app.on_site(&id));
    assert_eq!(
        app.read(&chat),
        Ok(ShareRead {
            repo: Some(REPO.into()),
            url: None
        })
    );
    let again = app.command(Command::Unshare {
        subject: chat.clone(),
    });
    assert!(error_then_changed(&again, NOT_SHARED, &chat), "{again:?}");
    let missing = Subject::Chat("missing".into());
    let refused = app.command(Command::Unshare {
        subject: missing.clone(),
    });
    assert!(
        error_then_changed(&refused, "Conversation not found", &missing),
        "{refused:?}"
    );
}

#[test]
fn a_second_publish_of_a_chat_that_is_publishing_is_refused_409_as_json() {
    let app = App::new("second", Some(REPO));
    let id = app.new_chat(true);
    let chat = Subject::Chat(id.clone());
    let open = app.export.hold();
    let first = app.command(Command::Publish {
        subject: chat.clone(),
    });
    assert!(
        matches!(
            &first[..],
            [Update::PublishStep {
                step: PublishStep::Exporting,
                ..
            }]
        ),
        "{first:?}"
    );
    app.export.wait_started(1);
    // Refused alone: no `Update::Error` beside it.
    let second = app.command(Command::Publish {
        subject: chat.clone(),
    });
    assert!(
        matches!(&second[..], [Update::PublishStep { subject: s, step: PublishStep::Failed { error } }] if *s == chat && error == BUSY),
        "{second:?}"
    );
    // The first goes on to its end.
    let after = app.len();
    app.export.let_go(&open);
    assert_eq!(app.ended(after, &chat), app.live(&id)[1..]);
}

#[test]
fn the_shared_builds_are_listed_with_the_chat_each_came_from_and_any_of_them_is_removed_by_its_share_id()
 {
    let bare = App::new("listed-bare", None);
    assert_eq!(
        listed(&bare),
        Published {
            repo: None,
            builds: Vec::new()
        }
    );
    let refused = removed(&bare, "abcdefabcdef");
    assert!(
        matches!(&refused[..], [Update::ShareRemoved { result: Err(e), .. }] if e == REPO_NEEDED),
        "{refused:?}"
    );

    let app = App::new("listed", Some(REPO));
    let kept = app.new_chat(true);
    let archived = app.new_chat(true);
    let gone = app.new_chat(true);
    for id in [&gone, &archived, &kept] {
        assert_eq!(app.publish(&Subject::Chat(id.clone())), app.live(id));
    }
    let store = Arc::clone(app.core.store());
    store.archive_conversation(&archived).unwrap();
    store.archive_conversation(&gone).unwrap();
    store.delete_conversation(&gone).unwrap();

    let published = listed(&app);
    assert_eq!(published.repo.as_deref(), Some(REPO));
    let ids = [kept.clone(), archived.clone(), gone.clone()];
    assert_eq!(
        rows(&published, &ids),
        [
            row(
                &app,
                &kept,
                "New build",
                "chat",
                &["model/x"],
                Some(&kept),
                None,
                false
            ),
            row(
                &app,
                &archived,
                "New build",
                "chat",
                &["model/x"],
                Some(&archived),
                None,
                true
            ),
            row(
                &app,
                &gone,
                "New build",
                "chat",
                &["model/x"],
                None,
                None,
                false
            ),
        ]
    );

    // Its chat is gone: no `ShareChanged`.
    let updates = removed(&app, &share_id(&gone));
    assert!(
        matches!(
            &updates[..],
            [
                Update::ShareRemoved { result: Ok(()), .. },
                Update::Shares(Ok(_))
            ]
        ),
        "{updates:?}"
    );
    assert!(!app.on_site(&gone));
    assert!(
        !listed(&app)
            .builds
            .iter()
            .any(|b| b.share_id == share_id(&gone))
    );
    let again = removed(&app, &share_id(&gone));
    assert!(
        matches!(&again[..], [Update::ShareRemoved { result: Err(e), .. }] if e == NOT_SHARED),
        "{again:?}"
    );
    for malformed in [
        "ABCDEFABCDEF",
        "abcdef",
        "abcdefabcdef0",
        "abcdef.bcdef",
        "..abcdefabcd",
        "abcdefabcde%2F",
    ] {
        let refused = removed(&app, malformed);
        assert!(
            matches!(&refused[..], [Update::ShareRemoved { share_id, result: Err(e) }] if share_id == malformed && e == NOT_SHARED),
            "{malformed}: {refused:?}"
        );
    }
    // Its chat exists: `ShareChanged` with no page between the removal and
    // the list.
    let updates = removed(&app, &share_id(&kept));
    assert!(
        matches!(&updates[..], [
            Update::ShareRemoved { result: Ok(()), .. },
            Update::ShareChanged { subject: Subject::Chat(c), url: None },
            Update::Shares(Ok(_)),
        ] if *c == kept),
        "{updates:?}"
    );
}

#[test]
fn a_removal_of_a_build_that_is_publishing_is_refused_409_and_a_git_failure_is_answered_with_its_message()
 {
    let app = App::new("removal", Some(REPO));
    let id = app.new_chat(true);
    let chat = Subject::Chat(id.clone());
    let open = app.export.hold();
    let from = app.len();
    app.command(Command::Publish {
        subject: chat.clone(),
    });
    app.export.wait_started(1);
    let refused = removed(&app, &share_id(&id));
    assert!(
        matches!(&refused[..], [Update::ShareRemoved { result: Err(e), .. }] if e == STILL_PUBLISHING),
        "{refused:?}"
    );
    let unshared = app.command(Command::Unshare {
        subject: chat.clone(),
    });
    assert!(
        error_then_changed(&unshared, STILL_PUBLISHING, &chat),
        "{unshared:?}"
    );
    app.export.let_go(&open);
    assert_eq!(app.ended(from, &chat), app.live(&id));

    *app.remote.lock().unwrap() = "fake://missing.git".into();
    let copy = "Could not get a copy of owner/builds from GitHub: fatal: repository 'fake://missing.git' does not exist";
    let list = app.command(Command::ReadShares);
    assert!(
        matches!(&list[..], [Update::Shares(Err(e))] if e == copy),
        "{list:?}"
    );
    let remove = removed(&app, &share_id(&id));
    assert!(
        matches!(&remove[..], [Update::ShareRemoved { result: Err(e), .. }] if e == copy),
        "{remove:?}"
    );
    *app.remote.lock().unwrap() = "fake://owner/builds".into();
    let remove = removed(&app, &share_id(&id));
    assert!(
        matches!(
            &remove[..],
            [
                Update::ShareRemoved { result: Ok(()), .. },
                Update::ShareChanged { .. },
                Update::Shares(Ok(_))
            ]
        ),
        "{remove:?}"
    );
}

#[test]
fn a_bench_publish_is_refused_without_a_repository_with_nothing_built_or_under_a_running_turn() {
    let bare = App::new("bench-refused-bare", None);
    let (id, _, _) = bare.new_bench(&["a"], false);
    let bench = Subject::Bench(id);
    assert_eq!(bare.publish(&bench), failed(REPO_NEEDED));
    assert_eq!(
        bare.read(&bench),
        Ok(ShareRead {
            repo: None,
            url: None
        })
    );
    let unshared = bare.command(Command::Unshare {
        subject: bench.clone(),
    });
    assert!(
        error_then_changed(&unshared, NOT_SHARED, &bench),
        "{unshared:?}"
    );

    let app = App::new("bench-refused", Some(REPO));
    let (none, _, _) = app.new_bench(&[], false);
    assert_eq!(
        app.publish(&Subject::Bench(none)),
        failed("No model in this bench has built anything yet")
    );
    // A bench whose round runs, one of its models having built.
    let from = app.len();
    app.command(Command::Send(message(
        Target::NewBench {
            members: ["model/a", "model/b"]
                .iter()
                .map(|model| BenchChoice {
                    provider: "openrouter".into(),
                    model: model.to_string(),
                    effort: None,
                })
                .collect(),
            blind: false,
        },
        "Build a bridge",
    )));
    let sent = app.wait(from, "the bench is sent", |u| {
        u.iter().any(|u| matches!(u, Update::BenchSent { .. }))
    });
    let running = sent
        .iter()
        .find_map(|u| match u {
            Update::BenchSent { bench, .. } => Some(bench.clone()),
            _ => None,
        })
        .unwrap();
    let store = Arc::clone(app.core.store());
    let member = store.bench_members(&running).unwrap().remove(1);
    stone(&mut member.lock().unwrap().world);
    assert_eq!(
        app.publish(&Subject::Bench(running.clone())),
        failed("Wait for the bench's running turns before sharing")
    );
    app.turn_gate.send_replace(true);
    let key = format!("bench:{running}");
    app.on(|core| async move { core.turns().free(&key).await });
    let missing = Subject::Bench("missing".into());
    assert_eq!(app.publish(&missing), failed("Bench not found"));
    assert_eq!(app.read(&missing), Err("Bench not found".into()));
}

#[test]
fn a_bench_publish_streams_its_states_and_puts_the_models_that_built_something_on_one_page_which_a_removal_takes_off()
 {
    let app = App::new("bench-streams", Some(REPO));
    let (id, order, members) = app.new_bench(&["a", "c"], true);
    let bench = Subject::Bench(id.clone());
    assert_eq!(app.publish(&bench), app.live(&id));
    assert_eq!(app.read(&bench).unwrap().url, Some(app.page(&id)));
    let build: Value = serde_json::from_slice(
        &std::fs::read(
            app.data
                .join("remote/b")
                .join(share_id(&id))
                .join("build.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let model = |member: &str| members.iter().find(|(m, _)| m == member).unwrap().1.clone();
    let letters: Vec<String> = order
        .iter()
        .enumerate()
        .filter(|(_, member)| model(member) != "model/b")
        .map(|(at, _)| char::from(b'A' + at as u8).to_string())
        .collect();
    assert_eq!(build["kind"], "bench");
    let labels: Vec<&str> = build["builds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, letters);
    let blueprints: Vec<&str> = build["builds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["blueprint"].as_str().unwrap())
        .collect();
    assert_eq!(blueprints, ["0.bin.gz", "1.bin.gz"]);

    // A second publish while one runs is refused.
    let open = app.export.hold();
    app.command(Command::Publish {
        subject: bench.clone(),
    });
    app.export.wait_started(3);
    let second = app.command(Command::Publish {
        subject: bench.clone(),
    });
    assert!(
        matches!(&second[..], [Update::PublishStep { step: PublishStep::Failed { error }, .. }] if error == BUSY),
        "{second:?}"
    );
    let after = app.len();
    app.export.let_go(&open);
    assert_eq!(app.ended(after, &bench), app.live(&id)[1..]);

    let gone = app.command(Command::Unshare {
        subject: bench.clone(),
    });
    assert!(changed(&gone, &bench, None), "{gone:?}");
    assert!(!app.on_site(&id));
    assert_eq!(app.read(&bench).unwrap().url, None);
    let again = app.command(Command::Unshare {
        subject: bench.clone(),
    });
    assert!(error_then_changed(&again, NOT_SHARED, &bench), "{again:?}");
}

#[test]
fn the_shares_list_a_bench_with_its_id_an_archived_bench_as_archived_and_a_bench_deleted_for_good_with_none_which_is_still_removable()
 {
    let app = App::new("bench-listed", Some(REPO));
    let (kept, _, _) = app.new_bench(&["a", "b", "c"], false);
    let (archived, _, _) = app.new_bench(&["a"], false);
    let (gone, _, _) = app.new_bench(&["b"], false);
    let chat = app.new_chat(true);
    for id in [&gone, &archived, &kept] {
        assert_eq!(app.publish(&Subject::Bench(id.clone())), app.live(id));
    }
    assert_eq!(app.publish(&Subject::Chat(chat.clone())), app.live(&chat));
    let store = Arc::clone(app.core.store());
    store.archive_bench(&archived).unwrap();
    store.archive_bench(&gone).unwrap();
    store.delete_bench(&gone).unwrap();

    let ids = [chat.clone(), kept.clone(), archived.clone(), gone.clone()];
    assert_eq!(
        rows(&listed(&app), &ids),
        [
            row(
                &app,
                &chat,
                "New build",
                "chat",
                &["model/x"],
                Some(&chat),
                None,
                false
            ),
            row(
                &app,
                &kept,
                "A bridge",
                "bench",
                &["model/a", "model/b", "model/c"],
                None,
                Some(&kept),
                false
            ),
            row(
                &app,
                &archived,
                "A bridge",
                "bench",
                &["model/a"],
                None,
                Some(&archived),
                true
            ),
            row(
                &app,
                &gone,
                "A bridge",
                "bench",
                &["model/b"],
                None,
                None,
                false
            ),
        ]
    );
    let updates = removed(&app, &share_id(&gone));
    assert!(
        matches!(
            &updates[..],
            [
                Update::ShareRemoved { result: Ok(()), .. },
                Update::Shares(Ok(_))
            ]
        ),
        "{updates:?}"
    );
    assert!(!app.on_site(&gone));
    let updates = removed(&app, &share_id(&kept));
    assert!(
        matches!(&updates[..], [
            Update::ShareRemoved { result: Ok(()), .. },
            Update::ShareChanged { subject: Subject::Bench(b), url: None },
            Update::Shares(Ok(_)),
        ] if *b == kept),
        "{updates:?}"
    );
}

// ---------------------------------------------------------------------------
// The core's own.

/// A publish whose task panics ends in `Failed` with the page's sentence
/// for a stream that ended early (and the panic is the core's fatal
/// failure).
#[test]
fn a_publish_whose_task_panics_ends_in_failed_publish_stopped() {
    let app = App::new("panics", Some(REPO));
    let id = app.new_chat(true);
    let chat = Subject::Chat(id);
    app.export.panics.store(true, Ordering::SeqCst);
    assert_eq!(
        app.publish(&chat),
        [
            PublishStep::Exporting,
            PublishStep::Failed {
                error: STOPPED.into()
            }
        ]
    );
    app.wait(0, "the core's fatal failure", |u| {
        u.iter().any(|u| matches!(u, Update::Fatal(_)))
    });
}

/// A publish goes on while the window reads other things (its panel
/// closed, another chat on show), its steps landing on its subject; the
/// core's stop ends the one under way in `Failed`.
#[test]
fn a_publish_outlives_its_panel_and_the_cores_stop_ends_it() {
    let app = App::new("outlives", Some(REPO));
    let id = app.new_chat(true);
    let other = app.new_chat(true);
    let chat = Subject::Chat(id.clone());
    let open = app.export.hold();
    let from = app.len();
    app.command(Command::Publish {
        subject: chat.clone(),
    });
    app.export.wait_started(1);
    assert_eq!(app.read(&Subject::Chat(other.clone())).unwrap().url, None);
    assert_eq!(listed(&app).builds, Vec::new());
    app.export.let_go(&open);
    assert_eq!(app.ended(from, &chat), app.live(&id));

    let open = app.export.hold();
    let from = app.len();
    let next = Subject::Chat(other);
    app.command(Command::Publish {
        subject: next.clone(),
    });
    app.export.wait_started(2);
    app.core.shutdown();
    assert_eq!(
        app.ended(from, &next),
        [
            PublishStep::Exporting,
            PublishStep::Failed {
                error: STOPPED.into()
            }
        ]
    );
    drop(open);
}

/// The app's own export before setup prepared the engines and the bake:
/// today's sentence, after `Exporting`, before any git.
#[test]
fn the_apps_export_fails_before_the_viewers_assets_are_baked() {
    let app = App::with("own", Some(REPO), Site::Own);
    let chat = Subject::Chat(app.new_chat(true));
    assert_eq!(
        app.publish(&chat),
        [
            PublishStep::Exporting,
            PublishStep::Failed {
                error: "The viewer's assets are not baked yet".into()
            }
        ]
    );
    assert!(!app.data.join("share").join("site").exists());
}

/// `share::share_id` is the route's (`[0-9a-f]{12}`), so a removal by any
/// listed id passes the route's check.
#[test]
fn every_share_id_passes_the_removals_check() {
    let id = share::share_id("any chat");
    assert_eq!(id.len(), 12);
    assert!(
        id.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    );
}
