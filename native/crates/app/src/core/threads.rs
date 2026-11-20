//! The core's runtime and how a panic on it becomes [`mcbench_ui::Update::Fatal`].
//!
//! Every thread of a core's tokio runtime (its workers and its blocking
//! pool) is named [`THREAD_NAME`] and holds that core's number in a
//! thread-local, so [`on_core_thread`] says whether a panic happened on one:
//! the interface's crash hook (`mcbench_ui::crash::install(logs, version,
//! handled)`) leaves those to the core, as it leaves the engine's refusals
//! to the engine ([`handled`]).
//!
//! The core learns of a panic on its threads from a panic hook of its own,
//! chained before whatever hook was set when the first core was built (the
//! interface's, or the default): a panic on a core thread is recorded for
//! that core with today's fatal line (`server/index.js` `exitOnFatal`:
//! `console.error('[fatal]', error)`) and wakes that core's fatal watcher,
//! which publishes it once and stops the core. Tasks the core does not
//! spawn itself (the turns `mcbench_agent::lifecycle::Turns` spawns) are
//! covered the same way, because they run on the same threads.

use std::cell::Cell;
use std::collections::HashMap;
use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use tokio::sync::watch;

thread_local! {
    static CORE: Cell<u64> = const { Cell::new(0) };
}

/// The name of every thread of a core's runtime.
pub const THREAD_NAME: &str = "mcbench-core";

/// True on a core's runtime threads (workers and blocking pool).
pub fn on_core_thread() -> bool {
    CORE.with(Cell::get) != 0
}

/// The interface's crash-hook filter (`crash::install`'s `handled`): a
/// panic on an engine thread is a refusal the engine answers, and one on a
/// core thread is the core's to report as [`mcbench_ui::Update::Fatal`].
pub fn handled(_info: &PanicHookInfo) -> bool {
    mcbench_engine::on_engine_thread() || on_core_thread()
}

/// How many workers a core's runtime has: the machine's threads, at most 4.
pub fn workers() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZero::get)
        .unwrap_or(1)
        .min(4)
}

/// A core's number and the watch its panics turn to `Some(line)`.
pub struct Watch {
    /// The core's number (never 0).
    pub core: u64,
    /// `Some` with the first panic's fatal line once one happened.
    pub fatal: watch::Receiver<Option<String>>,
}

fn watchers() -> &'static Mutex<HashMap<u64, watch::Sender<Option<String>>>> {
    static WATCHERS: OnceLock<Mutex<HashMap<u64, watch::Sender<Option<String>>>>> = OnceLock::new();
    WATCHERS.get_or_init(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let core = CORE.with(Cell::get);
            if core != 0 {
                panicked(core, info);
            }
            previous(info);
        }));
        Mutex::new(HashMap::new())
    })
}

/// A new core's runtime: multi-thread, [`workers`] workers, every thread
/// named [`THREAD_NAME`] and marked as this core's; and its fatal watch.
/// The first call installs the core's panic hook.
pub fn runtime() -> std::io::Result<(tokio::runtime::Runtime, Watch)> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let core = NEXT.fetch_add(1, Ordering::SeqCst);
    let (sender, fatal) = watch::channel(None);
    watchers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(core, sender);
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(workers())
        .thread_name(THREAD_NAME)
        .on_thread_start(move || CORE.with(|flag| flag.set(core)))
        .enable_all()
        .build()?;
    Ok((runtime, Watch { core, fatal }))
}

/// Forgets a core that has stopped.
pub fn forget(core: u64) {
    watchers()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&core);
}

/// Records a panic on a core's thread: the first one becomes its fatal line.
fn panicked(core: u64, info: &PanicHookInfo) {
    let watchers = watchers().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(sender) = watchers.get(&core) {
        sender.send_if_modified(|fatal| {
            if fatal.is_some() {
                return false;
            }
            *fatal = Some(line(info));
            true
        });
    }
}

/// `console.error('[fatal]', error)` of the panic: `[fatal] Error: <message>`
/// as Node prints an Error's first line, then where it panicked as a stack
/// line.
pub fn line(info: &PanicHookInfo) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic without a message".to_string());
    match info.location() {
        Some(at) => format!(
            "[fatal] Error: {message}\n    at {}:{}",
            at.file(),
            at.line()
        ),
        None => format!("[fatal] Error: {message}"),
    }
}
