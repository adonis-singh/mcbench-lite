//! The children `test_build`'s experiments run on: [`VerifyEngines`]
//! keeps the children that finished an experiment warm for the next, as
//! each lane of today's `EngineVerifier` keeps its engine process, and
//! starts one when none is free.

use crate::{ChildEngine, EngineError, EngineProgram, VerifyReport, VerifyRequest};
use std::collections::BTreeSet;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio_util::sync::CancellationToken;

/// The verifier's engine children. An experiment takes a free child (or
/// starts one) and gives it back when it ends with the child alive; a
/// child whose experiment was cancelled, or that died, is gone, so the next
/// experiment starts a fresh one (`EngineVerifier`: "the next test starts
/// it again"). Its calls need the tokio runtime.
pub struct VerifyEngines {
    program: EngineProgram,
    free: Mutex<Vec<ChildEngine>>,
    running: Mutex<BTreeSet<u32>>,
    started: AtomicUsize,
}

impl VerifyEngines {
    /// No child yet; they are children of `program`.
    pub fn new(program: EngineProgram) -> VerifyEngines {
        VerifyEngines {
            program,
            free: Mutex::new(Vec::new()),
            running: Mutex::new(BTreeSet::new()),
            started: AtomicUsize::new(0),
        }
    }

    /// The program the children are started from.
    pub fn program(&self) -> &EngineProgram {
        &self.program
    }

    fn spawn(&self) -> Result<ChildEngine, EngineError> {
        self.started.fetch_add(1, Ordering::SeqCst);
        self.program.spawn()
    }

    /// Starts children until `count` are free (`VerifierPool.warm`: every
    /// lane's engine), without waiting for them to be ready.
    pub fn warm(&self, count: usize) -> Result<(), EngineError> {
        let mut free = self.free.lock().unwrap();
        free.retain(ChildEngine::alive);
        while free.len() < count {
            free.push(self.spawn()?);
        }
        Ok(())
    }

    /// Runs one experiment on a child of its own ([`ChildEngine::verify`]).
    /// When `cancel` fires the child is stopped and this answers
    /// [`EngineError::Cancelled`] at once.
    pub async fn verify(
        &self,
        request: &VerifyRequest,
        progress: impl FnMut(&str) + Send,
        cancel: &CancellationToken,
    ) -> Result<VerifyReport, EngineError> {
        if cancel.is_cancelled() {
            return Err(EngineError::Cancelled);
        }
        let free = {
            let mut free = self.free.lock().unwrap();
            free.retain(ChildEngine::alive);
            free.pop()
        };
        let engine = match free {
            Some(engine) => engine,
            None => self.spawn()?,
        };
        self.running.lock().unwrap().insert(engine.pid());
        let report = engine.verify(request, progress, cancel).await;
        self.running.lock().unwrap().remove(&engine.pid());
        if engine.alive() && !cancel.is_cancelled() {
            self.free.lock().unwrap().push(engine);
        }
        report
    }

    /// The process ids of the children it holds now, free or running an
    /// experiment.
    pub fn pids(&self) -> Vec<u32> {
        let mut pids: Vec<u32> = self.running.lock().unwrap().iter().copied().collect();
        pids.extend(self.free.lock().unwrap().iter().map(ChildEngine::pid));
        pids.sort();
        pids
    }

    /// How many children it has started.
    pub fn started(&self) -> usize {
        self.started.load(Ordering::SeqCst)
    }

    /// Stops the free children; resolves once they have exited. An
    /// experiment still running keeps its child until it ends (the
    /// verifier's shutdown cancels it).
    pub async fn stop(&self) {
        let free: Vec<ChildEngine> = self.free.lock().unwrap().drain(..).collect();
        for engine in &free {
            engine.stop().await;
        }
    }
}
