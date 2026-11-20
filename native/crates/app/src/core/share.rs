//! Sharing (`server/index.js`'s share routes over `server/share.js`): the
//! core's five share commands and the adapter that gives
//! `mcbench_tools::share` the app's export and viewer.
//!
//! | Command | Today's route | Ends in |
//! |---|---|---|
//! | `ReadShare` | `GET .../share` with the settings' `shareRepo` | one `Update::ShareRead` |
//! | `Publish` | `POST .../share` (the NDJSON stream) | `Update::PublishStep` at each move, ending in one `Live` or `Failed`; a refusal is `Failed` alone |
//! | `Unshare` | `DELETE .../share` | `Update::ShareChanged` with no url; on a failure `Update::Error`, then `ShareChanged` with the url it still has |
//! | `ReadShares` | `GET /api/shares` | `Update::Shares` |
//! | `RemoveShare` | `DELETE /api/shares/:shareId` | `Update::ShareRemoved`; when it worked, `ShareChanged { url: None }` for the chat or bench it came from while that exists, then `Update::Shares` |
//!
//! A publish runs as its own task on the core's runtime, whoever looks: it
//! ends live or failed, and a publish whose task panics or is ended
//! ([`Core::shutdown`]) ends in `Failed` with [`STOPPED`]. The site's clone
//! is today's, `share/site` of the data folder.
//!
//! The adapter ([`AppSite`]): the export is `share::RenderedExport` over
//! the prepared engines' settles and the app's one renderer; the viewer is
//! `share_viewer::write_viewer` and `share_page_html` over the prepared
//! folders (`minecraft/<id>`, `textures/<id>`) and BlueMap's web app,
//! fetched into the data folder at the first export. Before setup has
//! prepared them the export fails with today's [`NOT_BAKED`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use mcbench_render::Cell;
use mcbench_tools::share::{
    self, BenchSource, Export, Exported, Options, SharePage, Shares, SiteViewer, Source,
};
use mcbench_tools::share_viewer::{self, ViewerParts};
use mcbench_ui::{PublishStep, Published, PublishedBuild, ShareRead, Subject, Update};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::Core;
use super::backend::{AppBackend, Prepared};

/// Today's `SHARE_REPO_NEEDED`.
pub const SHARE_REPO_NEEDED: &str = "Set the GitHub repository for shared builds in Settings";

/// Today's 404 of a removal.
pub const NOT_SHARED: &str = "This build is not shared";

/// Today's `exportBuild` before the viewer's assets are made.
pub const NOT_BAKED: &str = "The viewer's assets are not baked yet";

/// A publish that ended without `live` or `failed` (the page's text for a
/// stream that ended early).
pub const STOPPED: &str = "The publish stopped before the page was live";

/// The app's sharing options: the share folder of `data`, git and gh as
/// processes, and [`AppSite`] as the export and the viewer.
/// With `stand`, its jar and its options made from the app's site.
pub fn options(
    data: &Path,
    app: Option<Arc<AppBackend>>,
    client: reqwest::Client,
    cancel: CancellationToken,
    stand: Option<Stand>,
) -> Options {
    let (jar, made) = match stand {
        Some(stand) => (stand.jar, Some(stand.options)),
        None => (mcbench_setup::pins::bluemap(), None),
    };
    let site = Arc::new(AppSite {
        app,
        data: data.to_path_buf(),
        client,
        cancel,
        jar,
    });
    match made {
        Some(made) => made(site),
        None => Options::new(data.join("share"), site.clone(), site),
    }
}

/// A test's stand-in for [`options`]: where BlueMap's CLI jar comes from,
/// and the share's options made from the app's site (which a test may use
/// as the export and the viewer, or not).
pub struct Stand {
    /// BlueMap's CLI jar.
    pub jar: mcbench_setup::pins::Download,
    /// The options, given the app's site.
    pub options: Box<dyn FnOnce(Arc<AppSite>) -> Options + Send>,
}

/// The shared builds and the publishes under way.
pub struct Sharing {
    shares: Arc<Shares>,
    publishes: Mutex<Vec<JoinHandle<()>>>,
}

impl Sharing {
    /// Sharing made with `options`.
    pub fn new(options: Options) -> Sharing {
        Sharing {
            shares: Shares::new(options),
            publishes: Mutex::new(Vec::new()),
        }
    }

    /// Waits for every publish to end (the core's stop has cancelled them).
    pub(crate) async fn ended(&self) {
        let publishes = std::mem::take(&mut *self.publishes.lock().expect("the publishes"));
        for publish in publishes {
            // A publish that panicked has ended too: its panic is the core's
            // fatal failure.
            let _ = publish.await;
        }
    }
}

/// Ends a publish in `Failed` with [`STOPPED`] when its task is dropped or
/// panics before it ended.
struct Unfinished {
    core: Core,
    subject: Subject,
    ended: Arc<AtomicBool>,
}

impl Drop for Unfinished {
    fn drop(&mut self) {
        if !self.ended.load(Ordering::SeqCst) {
            self.core.publish(Update::PublishStep {
                subject: self.subject.clone(),
                step: PublishStep::Failed {
                    error: STOPPED.into(),
                },
            });
        }
    }
}

fn step_of(step: share::PublishStep) -> PublishStep {
    match step {
        share::PublishStep::Exporting => PublishStep::Exporting,
        share::PublishStep::Publishing => PublishStep::Publishing,
        share::PublishStep::Waiting => PublishStep::Waiting,
        share::PublishStep::Live { url } => PublishStep::Live { url },
        share::PublishStep::Failed { error } => PublishStep::Failed { error },
    }
}

/// The chats and the benches stored, by share id: their id and whether
/// they are archived.
type Sources = (
    HashMap<String, (String, bool)>,
    HashMap<String, (String, bool)>,
);

/// Today's `SHARE_ID`: `^[0-9a-f]{12}$`.
fn is_share_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Core {
    /// Runs one of the five share commands.
    pub(crate) async fn share_command(&self, command: mcbench_ui::Command) {
        use mcbench_ui::Command;
        match command {
            Command::ReadShare { subject } => {
                let read = self.share_read(&subject).await;
                self.publish(Update::ShareRead { subject, read });
            }
            Command::Publish { subject } => {
                if let Err(error) = self.start_publish(&subject).await {
                    self.publish(Update::PublishStep {
                        subject,
                        step: PublishStep::Failed { error },
                    });
                }
            }
            Command::Unshare { subject } => match self.unshare(&subject).await {
                Ok(()) => self.publish(Update::ShareChanged { subject, url: None }),
                Err(error) => {
                    self.publish(Update::Error(error));
                    // A chat or bench that cannot be read has no page.
                    let url = self.share_read(&subject).await.ok().and_then(|r| r.url);
                    self.publish(Update::ShareChanged { subject, url });
                }
            },
            Command::ReadShares => {
                let published = self.published().await;
                self.publish(Update::Shares(published));
            }
            Command::RemoveShare { share_id } => match self.remove_share(&share_id).await {
                Err(error) => self.publish(Update::ShareRemoved {
                    share_id,
                    result: Err(error),
                }),
                Ok(()) => {
                    self.publish(Update::ShareRemoved {
                        share_id: share_id.clone(),
                        result: Ok(()),
                    });
                    match self.share_sources().await {
                        Ok((chats, benches)) => {
                            let subject = chats
                                .get(&share_id)
                                .map(|(id, _)| Subject::Chat(id.clone()))
                                .or_else(|| {
                                    benches
                                        .get(&share_id)
                                        .map(|(id, _)| Subject::Bench(id.clone()))
                                });
                            if let Some(subject) = subject {
                                self.publish(Update::ShareChanged { subject, url: None });
                            }
                        }
                        Err(error) => self.publish(Update::Error(error)),
                    }
                    let published = self.published().await;
                    self.publish(Update::Shares(published));
                }
            },
            other => unreachable!("not a share command: {other:?}"),
        }
    }

    fn shares(&self) -> &Arc<Shares> {
        &self.0.sharing.shares
    }

    /// `store.getConversation(id)` as a share reads it; today's 404.
    async fn chat_source(&self, id: &str) -> Result<Source, String> {
        let chat = id.to_string();
        self.blocking(move |core| {
            let live = core
                .0
                .store
                .conversation(&chat)
                .map_err(|e| e.message)?
                .ok_or_else(|| "Conversation not found".to_string())?;
            let conversation = live.lock().expect("a chat's lock");
            Ok(Source {
                id: conversation.id.0.clone(),
                title: conversation.title.clone(),
                model: conversation.model.clone(),
                effort: conversation.effort.clone(),
                world: conversation.world.clone(),
            })
        })
        .await
    }

    /// `store.getBench(id)` and `store.benchMembers(id)`; today's 404.
    async fn bench_source(&self, id: &str) -> Result<(BenchSource, Vec<Source>), String> {
        let bench = id.to_string();
        self.blocking(move |core| {
            let found = core.bench_of(&bench).map_err(|e| e.message)?;
            let members = core
                .0
                .store
                .bench_members(&bench)
                .map_err(|e| e.message)?
                .iter()
                .map(|member| {
                    let member = member.lock().expect("a chat's lock");
                    Source {
                        id: member.id.0.clone(),
                        title: member.title.clone(),
                        model: member.model.clone(),
                        effort: member.effort.clone(),
                        world: member.world.clone(),
                    }
                })
                .collect();
            let order = found
                .order
                .as_array()
                .expect("a bench's order is a list")
                .iter()
                .map(|id| id.as_str().expect("a member's id").to_string())
                .collect();
            Ok((
                BenchSource {
                    id: found.id,
                    title: found.title,
                    blind: found.blind,
                    revealed: found.revealed,
                    order,
                },
                members,
            ))
        })
        .await
    }

    /// The chat or bench is there (each route reads it first).
    async fn subject_exists(&self, subject: &Subject) -> Result<(), String> {
        match subject {
            Subject::Chat(id) => self.chat_source(id).await.map(drop),
            Subject::Bench(id) => self.bench_source(id).await.map(drop),
        }
    }

    fn subject_id(subject: &Subject) -> &str {
        match subject {
            Subject::Chat(id) | Subject::Bench(id) => id,
        }
    }

    /// `GET .../share`, with the saved repository.
    async fn share_read(&self, subject: &Subject) -> Result<ShareRead, String> {
        self.subject_exists(subject).await?;
        let repo = self.share_repo().await;
        let url = match &repo {
            None => None,
            Some(repo) => self
                .shares()
                .url(repo, &share::share_id(Core::subject_id(subject)))
                .await
                .map_err(|e| e.to_string())?,
        };
        Ok(ShareRead { repo, url })
    }

    /// `POST .../share`: its refusals in today's order, then the publish,
    /// which tells `Exporting` before this returns and goes on as its own
    /// task.
    async fn start_publish(&self, subject: &Subject) -> Result<(), String> {
        let (what, repo) = match subject {
            Subject::Chat(id) => {
                let source = self.chat_source(id).await?;
                let repo = self.share_repo().await.ok_or(SHARE_REPO_NEEDED)?;
                if source.world.size() == 0 {
                    return Err("The build is empty: there is nothing to share".into());
                }
                if self.0.turns.held(id) {
                    return Err("Wait for the running turn before sharing".into());
                }
                (share::Subject::chat(source), repo)
            }
            Subject::Bench(id) => {
                let (bench, members) = self.bench_source(id).await?;
                let repo = self.share_repo().await.ok_or(SHARE_REPO_NEEDED)?;
                if members.iter().all(|member| member.world.size() == 0) {
                    return Err("No model in this bench has built anything yet".into());
                }
                if std::iter::once(format!("bench:{id}"))
                    .chain(members.iter().map(|member| member.id.clone()))
                    .any(|key| self.0.turns.held(&key))
                {
                    return Err("Wait for the bench's running turns before sharing".into());
                }
                (share::Subject::Bench { bench, members }, repo)
            }
        };
        let ended = Arc::new(AtomicBool::new(false));
        let on_state = {
            let (core, subject, ended) = (self.clone(), subject.clone(), Arc::clone(&ended));
            move |step: share::PublishStep| {
                if matches!(
                    step,
                    share::PublishStep::Live { .. } | share::PublishStep::Failed { .. }
                ) {
                    ended.store(true, Ordering::SeqCst);
                }
                core.publish(Update::PublishStep {
                    subject: subject.clone(),
                    step: step_of(step),
                });
            }
        };
        let run = self
            .shares()
            .publish(&repo, what, on_state)
            .map_err(|e| e.to_string())?;
        let unfinished = Unfinished {
            core: self.clone(),
            subject: subject.clone(),
            ended,
        };
        let stopping = self.0.stopping.clone();
        let task = self.0.handle.spawn(async move {
            let _unfinished = unfinished;
            tokio::select! {
                () = run => {}
                () = stopping.cancelled() => {}
            }
        });
        let mut publishes = self.0.sharing.publishes.lock().expect("the publishes");
        publishes.retain(|publish| !publish.is_finished());
        publishes.push(task);
        Ok(())
    }

    /// `DELETE .../share`.
    async fn unshare(&self, subject: &Subject) -> Result<(), String> {
        self.subject_exists(subject).await?;
        let Some(repo) = self.share_repo().await else {
            return Err(NOT_SHARED.into());
        };
        match self
            .shares()
            .remove(&repo, &share::share_id(Core::subject_id(subject)))
            .await
        {
            Ok(true) => Ok(()),
            Ok(false) => Err(NOT_SHARED.into()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// `store.listConversationIds()` and `store.listBenchIds()` by share id.
    async fn share_sources(&self) -> Result<Sources, String> {
        self.blocking(|core| {
            let store = &core.0.store;
            let by_share = |ids: Vec<(String, bool)>| {
                ids.into_iter()
                    .map(|(id, archived)| (share::share_id(&id), (id, archived)))
                    .collect::<HashMap<_, _>>()
            };
            Ok((
                by_share(store.list_conversation_ids().map_err(|e| e.message)?),
                by_share(store.list_bench_ids().map_err(|e| e.message)?),
            ))
        })
        .await
    }

    /// `GET /api/shares`: every share on the site, newest first, with the
    /// chat or bench it came from (none once that is deleted for good).
    async fn published(&self) -> Result<Published, String> {
        let Some(repo) = self.share_repo().await else {
            return Ok(Published {
                repo: None,
                builds: Vec::new(),
            });
        };
        let (chats, benches) = self.share_sources().await?;
        let listed = self.shares().list(&repo).await.map_err(|e| e.to_string())?;
        let builds = listed
            .into_iter()
            .map(|build| {
                let source = match build.kind.as_str() {
                    "chat" => chats.get(&build.share_id),
                    "bench" => benches.get(&build.share_id),
                    _ => None,
                };
                let id = |kind: &str| {
                    source
                        .filter(|_| build.kind == kind)
                        .map(|(id, _)| id.clone())
                };
                PublishedBuild {
                    conversation_id: id("chat"),
                    bench_id: id("bench"),
                    archived: source.is_some_and(|(_, archived)| *archived),
                    share_id: build.share_id,
                    title: build.title,
                    kind: build.kind,
                    labels: build.labels,
                    url: build.url,
                    published_at: build.published_at,
                }
            })
            .collect();
        Ok(Published {
            repo: Some(repo),
            builds,
        })
    }

    /// `DELETE /api/shares/:shareId`.
    async fn remove_share(&self, id: &str) -> Result<(), String> {
        if !is_share_id(id) {
            return Err(NOT_SHARED.into());
        }
        let Some(repo) = self.share_repo().await else {
            return Err(SHARE_REPO_NEEDED.into());
        };
        match self.shares().remove(&repo, id).await {
            Ok(true) => Ok(()),
            Ok(false) => Err(NOT_SHARED.into()),
            Err(error) => Err(error.to_string()),
        }
    }
}

/// The app's export and viewer (see the module's doc).
pub struct AppSite {
    app: Option<Arc<AppBackend>>,
    data: PathBuf,
    client: reqwest::Client,
    cancel: CancellationToken,
    jar: mcbench_setup::pins::Download,
}

impl AppSite {
    fn prepared(&self) -> Result<(Arc<AppBackend>, Arc<Prepared>), String> {
        let app = self.app.as_ref().ok_or(NOT_BAKED)?;
        let prepared = app.prepared().ok_or(NOT_BAKED)?;
        Ok((Arc::clone(app), prepared))
    }

    fn export(&self) -> Result<share::RenderedExport, String> {
        let (app, prepared) = self.prepared()?;
        Ok(share::RenderedExport {
            settler: Arc::clone(&prepared.engines.settler),
            renderer: Arc::clone(app.renderer()),
        })
    }

    fn parts(&self) -> Result<ViewerParts, String> {
        let (_, prepared) = self.prepared()?;
        Ok(ViewerParts {
            web_app: share_viewer::web_app_folder(&self.data),
            textures: prepared.textures.file("textures.json"),
            pack: prepared.data.file("mcbench-pack.bin"),
            settings: prepared.data.file("settings.json"),
            map_settings: prepared.data.file("map-settings.json"),
        })
    }
}

#[async_trait]
impl Export for AppSite {
    async fn export_build(&self, source: &Source, size: [u32; 2]) -> Result<Exported, String> {
        let export = self.export()?;
        share_viewer::bluemap_web_app(&self.data, &self.client, &self.jar, &self.cancel).await?;
        export.export_build(source, size).await
    }

    async fn montage(
        &self,
        tiles: Vec<Vec<u8>>,
        cells: Vec<Cell>,
        size: [u32; 2],
    ) -> Result<Vec<u8>, String> {
        self.export()?.montage(tiles, cells, size).await
    }
}

impl SiteViewer for AppSite {
    fn write_viewer(&self, site: &Path) -> Result<String, String> {
        share_viewer::write_viewer(site, &self.parts()?)
    }

    fn share_page_html(&self, page: &SharePage<'_>) -> String {
        share_viewer::share_page_html(
            page.viewer_id,
            page.share_id,
            page.title,
            page.description,
            page.url,
            page.image,
            page.labels,
        )
    }
}
