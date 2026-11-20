//! `server-fatal.test.js` (1): a failure outside any request is fatal. In
//! the core: a panic in a turn's task publishes ONE `Update::Fatal` with
//! today's line (`[fatal]` and the error), the core takes no more
//! commands, and everything it started stops (a turn under way is stopped;
//! with the real engines, every engine child and the build child it
//! started is gone). Its own file: the panic hook is the process's.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use mcbench_lite::core::backend::Backend;
use mcbench_lite::core::{Core, Parts};
use mcbench_providers::{
    AssistantMessage, CompletionRequest, Delta, ModelFacts, ModelInfo, Provider, ProviderError,
    ProviderId,
};
use mcbench_ui::{Command, Outgoing, Target, Update, Window};

/// A model that panics on a turn's first completion when its chat's
/// text says `boom`, and otherwise never answers.
struct Model;

#[async_trait]
impl Provider for Model {
    fn id(&self) -> ProviderId {
        ProviderId::OpenRouter
    }
    async fn complete(
        &self,
        request: &CompletionRequest,
        _: &mut (dyn FnMut(Delta) + Send),
        cancel: &CancellationToken,
    ) -> Result<AssistantMessage, ProviderError> {
        if request.tools.is_empty() {
            // A title waits forever too, until the turn is stopped.
            cancel.cancelled().await;
            return Err(ProviderError::new("This operation was aborted"));
        }
        let text = serde_json::to_string(&request.messages.last().unwrap().0).unwrap();
        if text.contains("boom") {
            panic!("boom");
        }
        cancel.cancelled().await;
        Err(ProviderError::new("This operation was aborted"))
    }
    async fn models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }
    async fn model_info(&self, _: &str) -> Result<ModelFacts, ProviderError> {
        Ok(ModelFacts {
            image_input: Some(true),
            ..ModelFacts::unknown()
        })
    }
    async fn close_session(&self, _: &str) {}
}

/// The engines and the build child of a real backend, filled in once the
/// core's store exists.
type Real = Arc<
    Mutex<
        Option<(
            mcbench_tools::Tools,
            Arc<mcbench_tools::Services>,
            mcbench_tools::testing::Shared,
        )>,
    >,
>;

struct Fake {
    real: Real,
}

#[async_trait]
impl Backend for Fake {
    fn provider(&self, _: &str) -> Result<Arc<dyn Provider>, String> {
        Ok(Arc::new(Model))
    }
    fn tools(&self) -> mcbench_tools::Tools {
        match &*self.real.lock().unwrap() {
            Some((tools, _, _)) => tools.clone(),
            None => mcbench_tools::Tools::of(Vec::new()),
        }
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
    async fn shutdown(&self) {
        let real = self.real.lock().unwrap().take();
        if let Some((tools, services, engines)) = real {
            services.verifier.stop().await;
            engines.settler.pool().stop().await;
            drop((tools, services, engines));
        }
    }
}

#[derive(Default)]
struct Recording(Mutex<Vec<Update>>);

impl Window for Recording {
    fn publish(&self, update: Update) {
        self.0.lock().unwrap().push(update);
    }
}

impl Recording {
    fn fatal(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .filter_map(|u| match u {
                Update::Fatal(line) => Some(line.clone()),
                _ => None,
            })
            .collect()
    }
    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcbench-core-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The children of this process (every thread's), on Linux.
fn children() -> Vec<u32> {
    let mut out = Vec::new();
    for task in std::fs::read_dir("/proc/self/task").unwrap() {
        let file = task.unwrap().path().join("children");
        let text = std::fs::read_to_string(file).unwrap_or_default();
        out.extend(text.split_whitespace().map(|p| p.parse::<u32>().unwrap()));
    }
    out.sort();
    out.dedup();
    out
}

fn alive(pid: u32) -> bool {
    // A zombie (exited, not yet reaped) is not running.
    std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .map(|stat| {
            let state = stat.rsplit(')').next().unwrap_or("").trim_start();
            !state.starts_with('Z')
        })
        .unwrap_or(false)
}

fn send(core: &Core, chat: &str, text: &str) {
    mcbench_ui::Core::send(
        core,
        Command::Send(Outgoing {
            target: Target::Chat {
                id: chat.into(),
                provider: "openrouter".into(),
                model: "chat/model".into(),
            },
            text: text.into(),
            attachments: Vec::new(),
            selections: Vec::new(),
            prompts: Vec::new(),
            effort: None,
            resolution: mcbench_ui::Resolution::Standard,
            edit_of: None,
        }),
    );
}

fn until(what: &str, limit: Duration, done: impl Fn() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < limit, "{what}: not within {limit:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_panic_on_the_core_is_one_fatal_update_and_the_core_stops() {
    let data = scratch("fatal");
    let real: Real = Arc::default();
    // The real engines' part counts this process's children in /proc
    // (`children`), so it runs on Linux only.
    let real_engines = std::env::var_os("MCBENCH_TABLES")
        .zip(std::env::var_os("MCBENCH_ENGINE_PROGRAM"))
        .filter(|_| cfg!(target_os = "linux"));
    let program = real_engines
        .as_ref()
        .map(|(_, p)| PathBuf::from(p))
        .unwrap_or_else(|| std::env::current_exe().unwrap());
    let core = Core::start(Parts {
        data: data.clone(),
        program: program.clone(),
        host: Arc::new(mcbench_store::SystemHost),
        shipped: Box::new(|| Err("unused".into())),
        client: mcbench_setup::fetch::client(),
        backend: Some(Arc::new(Fake {
            real: Arc::clone(&real),
        })),
        installer: None,
        share: None,
        live: Default::default(),
    })
    .unwrap();
    let window = Arc::new(Recording::default());
    mcbench_ui::Core::attach(&core, window.clone());
    let handle = core.handle().clone();
    handle.block_on(core.started());

    // With the real engines (Linux): a warm settle child and the build
    // runner's spare child run before the panic.
    let started_children = if let Some((tables, _)) = &real_engines {
        let (tables, store) = (PathBuf::from(tables), Arc::clone(core.store()));
        let made = handle
            .block_on(handle.spawn(async move {
                let engines = mcbench_tools::testing::shared(&tables).unwrap();
                engines.settler.pool().warm().await.unwrap();
                let verifier = engines
                    .verifier(store.folder().root.join("minecraft-verify"))
                    .unwrap();
                let registry = mcbench_tools::testing::registry_from_table(&tables).unwrap();
                let services = mcbench_tools::Services::on(
                    &engines,
                    store,
                    Arc::new(mcbench_tools::testing::RecordedSandbox::new()),
                    verifier,
                    Arc::new(registry),
                    mcbench_tools::shots::SharedRenderer::new(),
                );
                (
                    mcbench_tools::Tools::new(Arc::clone(&services)),
                    services,
                    engines,
                )
            }))
            .unwrap();
        *real.lock().unwrap() = Some(made);
        until(
            "the engine and build children start",
            Duration::from_secs(60),
            || children().len() >= 2,
        );
        children()
    } else {
        Vec::new()
    };

    let chat = |core: &Core| {
        handle
            .block_on(handle.spawn({
                let core = core.clone();
                async move {
                    core.create_conversation("openrouter", "chat/model")
                        .await
                        .unwrap()
                        .id
                }
            }))
            .unwrap()
    };
    let waiting = chat(&core);
    let failing = chat(&core);
    send(&core, &waiting, "Build a hut");
    until("a turn runs", Duration::from_secs(60), || {
        core.turns().turn(&waiting).is_some()
    });

    send(&core, &failing, "boom");
    until("the fatal update", Duration::from_secs(60), || {
        !window.fatal().is_empty()
    });
    let fatal = window.fatal();
    assert_eq!(fatal.len(), 1, "{fatal:?}");
    assert!(
        fatal[0].starts_with("[fatal] Error: boom\n    at "),
        "{}",
        fatal[0]
    );
    assert!(core.stopped());
    // The shutdown stops the turn under way; the panicked turn gave its
    // hold back (executeTurn's `finally`), so the shutdown ends well
    // inside its deadline. The stopped turn publishes its own end (its
    // answer and `Done`) before it gives its hold back, so once nothing
    // holds the chat no update of that turn is still to come.
    until(
        "the turn under way is stopped and has ended",
        Duration::from_secs(60),
        || core.turns().turn(&waiting).is_none(),
    );
    assert!(
        window.0.lock().unwrap().iter().any(|u| matches!(u,
            Update::Turn { chat, event: mcbench_ui::TurnEvent::Assistant { text, .. } }
                if *chat == waiting && text == "(stopped by user)")),
        "the turn under way was stopped"
    );
    // No command is taken from now.
    let before = window.len();
    send(&core, &waiting, "Build again");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(window.fatal().len(), 1);
    assert!(
        window.0.lock().unwrap()[before..]
            .iter()
            .all(|u| !matches!(u, Update::Turn { .. } | Update::Error(_))),
        "a command was taken after the fatal update"
    );
    // Nothing it started is left running.
    for pid in &started_children {
        until(
            &format!("child {pid} ends"),
            Duration::from_secs(60),
            || !alive(*pid),
        );
    }
    core.shutdown();
    drop(core);
    std::fs::remove_dir_all(&data).unwrap();
    println!("fatal: 1 update, {} children ended", started_children.len());
}
