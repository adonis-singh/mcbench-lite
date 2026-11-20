//! The engine threads: every thread of this process that runs block code
//! (the live worlds), each with a 1 GiB stack and registries of its own.
//!
//! `mcbench_core::boot`'s registries are not `Sync` (their components hold
//! `Rc`s), so two threads may never share one, and `boot` leaks what it
//! makes (the released program boots once per process). So an engine thread
//! boots once, runs one task at a time (a world for its whole life), and
//! waits for the next when the task ends: the threads, and the registries
//! leaked, number the most tasks that ever ran at once, not the tasks run.

use crate::{ENGINE_STACK, EngineError};
use mcbench_core::session::Session;
use std::cell::Cell;
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};

/// What a task finds on its engine thread: the thread's own registries
/// behind the call that needs them.
pub(crate) struct Booted {
    session: Box<dyn Fn() -> Session>,
}

impl Booted {
    /// A new, empty world on this thread's registries.
    pub(crate) fn session(&self) -> Session {
        (self.session)()
    }
}

/// A task: it must not panic out (each catches its own panics, as the
/// thread's registries outlive it).
pub(crate) type Task = Box<dyn FnOnce(&Booted) + Send>;

/// The engine threads waiting for a task.
static IDLE: Mutex<Vec<Sender<Task>>> = Mutex::new(Vec::new());

thread_local! {
    static ENGINE_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// True on the in-process engine's own threads. Their block code refuses
/// what the port does not model with a panic that the thread catches and
/// answers ("unported: ..."), so the app's panic hook leaves those panics
/// to the engine (the hook [`crate::Engine::start`] sets prints none of the
/// refusals).
pub fn on_engine_thread() -> bool {
    ENGINE_THREAD.with(Cell::get)
}

/// Runs `task` on an idle engine thread, or on a new one.
pub(crate) fn run(task: Task) -> Result<(), EngineError> {
    let mut task = task;
    loop {
        let idle = IDLE.lock().unwrap().pop();
        let Some(thread) = idle else { break };
        match thread.send(task) {
            Ok(()) => return Ok(()),
            // That thread is gone: its task panicked out.
            Err(returned) => task = returned.0,
        }
    }
    let (sender, receiver) = channel::<Task>();
    std::thread::Builder::new()
        .name("mcbench-engine".into())
        .stack_size(ENGINE_STACK)
        .spawn(move || serve(task, receiver, sender))
        .map(|_| ())
        .map_err(|error| {
            EngineError::Stopped(format!("The engine could not start a thread: {error}"))
        })
}

fn serve(first: Task, tasks: Receiver<Task>, me: Sender<Task>) {
    ENGINE_THREAD.with(|flag| flag.set(true));
    let (reg, blocks) = mcbench_core::boot();
    let booted = Booted {
        session: Box::new(move || Session::new(reg, blocks)),
    };
    let mut task = first;
    loop {
        task(&booted);
        IDLE.lock().unwrap().push(me.clone());
        match tasks.recv() {
            Ok(next) => task = next,
            Err(_) => return,
        }
    }
}
