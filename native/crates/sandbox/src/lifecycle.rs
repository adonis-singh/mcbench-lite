//! `sandbox.js`'s lifecycle over any [`Backend`]: each chat's state, when
//! its boxes are made and removed, the persistent, networked and
//! write-disabled commands, the storage watchdog and the idle reaper.
//!
//! Each chat's state is one of (`sandbox.js` `sandboxes`):
//!
//! | State | Meaning |
//! |---|---|
//! | ready | commands run, and the storage watchdog may end its processes |
//! | restricted | its command with file writes disabled is running, which is how the chat gets back under its limits: the watchdog leaves its processes alone |
//! | halted | the watchdog ended its processes; the next result of a command says why |
//! | removing | its boxes are being removed: a command that arrives meanwhile waits, then starts a fresh one |
//!
//! A chat with no entry has no box this process knows of. Boxes die with
//! the process (the keepers' sockets close), so a new process starts with
//! none (today's `adoptContainers` has nothing to adopt).
//!
//! Commands of one chat's persistent shell run one at a time (today two at
//! once would write into one shell); the networked and write-disabled
//! one-shots run side by side, as today.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::transcript::{
    NETWORK_DISABLED, NETWORK_NOTE, RESTRICTED_NOTE, SESSION_RESET_NOTE, WRITES_DISABLED_LIMITS,
    cannot_become, format_result, halt_reason, storage_message,
};
use crate::{Backend, BashCommand, BashRun, Budget, Sandbox, SandboxError, Workspace, work};

/// `IDLE_MS`: a chat's boxes are removed after this long unused.
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// `WATCH_MS`: the watchdog reads the disk's free space this often.
pub const WATCH_EVERY: Duration = Duration::from_secs(1);
/// `WATCH_STEP`: a drop of free space that has the watchdog measure.
pub const WATCH_STEP: u64 = 64 * 1024 * 1024;
/// The idle reaper's period.
pub const REAP_EVERY: Duration = Duration::from_secs(60);

#[derive(Clone)]
enum State {
    Ready,
    Restricted,
    Halted {
        reason: String,
        ended: watch::Receiver<bool>,
    },
    Removing {
        removed: watch::Receiver<bool>,
    },
}

impl State {
    fn name(&self) -> &'static str {
        match self {
            State::Ready => "ready",
            State::Restricted => "restricted",
            State::Halted { .. } => "halted",
            State::Removing { .. } => "removing",
        }
    }
}

struct Tracked {
    state: State,
    last_used: Instant,
    /// One persistent-shell command at a time.
    shell: Arc<tokio::sync::Mutex<()>>,
}

struct Inner<B: Backend> {
    backend: B,
    budget: Arc<dyn Budget>,
    boxes: Mutex<HashMap<String, Tracked>>,
    network: AtomicBool,
    watched_free: Mutex<Option<u64>>,
    watching: AtomicBool,
    stopped: CancellationToken,
}

/// The sandbox of a platform: [`Sandbox`] over a [`Backend`].
pub struct Sandboxes<B: Backend> {
    inner: Arc<Inner<B>>,
}

async fn done(mut receiver: watch::Receiver<bool>) {
    // A sender dropped without sending means the work ended all the same.
    let _ = receiver.wait_for(|done| *done).await;
}

impl<B: Backend> Sandboxes<B> {
    /// The lifecycle over `backend`, with the watchdog and the idle reaper
    /// running on the current tokio runtime until [`Sandbox::shutdown`].
    pub fn new(backend: B, budget: Arc<dyn Budget>) -> Self {
        let inner = Arc::new(Inner {
            backend,
            budget,
            boxes: Mutex::new(HashMap::new()),
            network: AtomicBool::new(true),
            watched_free: Mutex::new(None),
            watching: AtomicBool::new(false),
            stopped: CancellationToken::new(),
        });
        spawn_every(&inner, WATCH_EVERY, |inner| async move {
            if let Err(error) = inner.watch_storage().await {
                eprintln!("[sandbox] storage watch failed: {error}");
            }
        });
        spawn_every(&inner, REAP_EVERY, |inner| async move {
            inner.reap_idle_at(Instant::now())
        });
        Sandboxes { inner }
    }

    /// The backend.
    pub fn backend(&self) -> &B {
        &self.inner.backend
    }

    /// One tick of the storage watchdog (`watchStorage`).
    pub async fn watch_storage(&self) -> Result<(), String> {
        self.inner.watch_storage().await
    }

    /// One pass of the idle reaper as if it were `now` (`reapIdle`); the
    /// removals it starts are not waited for.
    pub fn reap_idle_at(&self, now: Instant) {
        self.inner.reap_idle_at(now)
    }

    /// The chats with a box this process knows of, sorted.
    pub fn tracked(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .inner
            .boxes
            .lock()
            .expect("boxes")
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }
}

fn spawn_every<B, F, Fut>(inner: &Arc<Inner<B>>, every: Duration, tick: F)
where
    B: Backend,
    F: Fn(Arc<Inner<B>>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send,
{
    let weak: Weak<Inner<B>> = Arc::downgrade(inner);
    let stopped = inner.stopped.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        loop {
            tokio::select! {
                () = stopped.cancelled() => return,
                _ = interval.tick() => {}
            }
            let Some(inner) = weak.upgrade() else { return };
            tick(inner).await;
        }
    });
}

impl<B: Backend> Inner<B> {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Tracked>> {
        self.boxes.lock().expect("the sandboxes' lock")
    }

    fn track<'a>(boxes: &'a mut HashMap<String, Tracked>, chat: &str) -> &'a mut Tracked {
        boxes.entry(chat.to_string()).or_insert_with(|| Tracked {
            state: State::Ready,
            last_used: Instant::now(),
            shell: Arc::new(tokio::sync::Mutex::new(())),
        })
    }

    /// `use`: the chat's sandbox, used now; one being removed is waited for.
    async fn use_box(&self, chat: &str) -> Arc<tokio::sync::Mutex<()>> {
        loop {
            let removing = match self.lock().get(chat).map(|tracked| tracked.state.clone()) {
                Some(State::Removing { removed }) => removed,
                _ => break,
            };
            done(removing).await;
        }
        let mut boxes = self.lock();
        let tracked = Self::track(&mut boxes, chat);
        tracked.last_used = Instant::now();
        tracked.shell.clone()
    }

    fn touch(&self, chat: &str) {
        if let Some(tracked) = self.lock().get_mut(chat) {
            tracked.last_used = Instant::now();
        }
    }

    /// `ensureContainer`.
    async fn ensure_box(&self, workspace: &Workspace, network: bool) -> Result<(), SandboxError> {
        self.use_box(&workspace.chat).await;
        self.backend.ensure(workspace, network).await?;
        self.touch(&workspace.chat);
        Ok(())
    }

    fn state_of(&self, chat: &str) -> Option<State> {
        self.lock().get(chat).map(|tracked| tracked.state.clone())
    }

    /// `move`.
    fn change(&self, chat: &str, from: &[&str], next: State) -> Result<(), SandboxError> {
        let mut boxes = self.lock();
        let tracked = Self::track(&mut boxes, chat);
        if !from.contains(&tracked.state.name()) {
            return Err(SandboxError::Failed(cannot_become(
                chat,
                tracked.state.name(),
                next.name(),
            )));
        }
        tracked.state = next;
        Ok(())
    }

    /// `takeHalt`: why the watchdog ended the chat's processes, once.
    fn take_halt(&self, chat: &str) -> Option<String> {
        let mut boxes = self.lock();
        let tracked = boxes.get_mut(chat)?;
        let State::Halted { reason, .. } = &tracked.state else {
            return None;
        };
        let reason = reason.clone();
        tracked.state = State::Ready;
        Some(reason)
    }

    /// `resetShell`: a new shell for the chat, with the command that ran
    /// away and every background job ended.
    async fn reset_shell(&self, chat: &str) {
        self.backend.kill_session(chat).await;
        self.backend.end_processes(chat, false).await;
    }

    async fn exec(
        self: &Arc<Self>,
        workspace: &Workspace,
        command: &BashCommand,
        cancel: &CancellationToken,
    ) -> Result<BashRun, SandboxError> {
        if cancel.is_cancelled() {
            return Err(SandboxError::Cancelled);
        }
        if command.network && !self.network.load(Ordering::SeqCst) {
            return Err(SandboxError::Failed(NETWORK_DISABLED.to_string()));
        }
        let ms = command.clamped();
        let started = Instant::now();
        let result = async {
            let refusal = self
                .budget
                .refusal(&workspace.chat)
                .await
                .map_err(SandboxError::Failed)?;
            match refusal {
                Some(refusal) => {
                    self.restricted(workspace, &command.command, ms, cancel, &refusal, started)
                        .await
                }
                None if command.network => {
                    self.networked(workspace, &command.command, ms, cancel, started)
                        .await
                }
                None => {
                    self.persistent(workspace, &command.command, ms, cancel, started)
                        .await
                }
            }
        }
        .await;
        let result = result.map_err(|error| match self.take_halt(&workspace.chat) {
            Some(halt) if !cancel.is_cancelled() => SandboxError::Failed(halt),
            _ => error,
        });
        let budget = self.budget.clone();
        let chat = workspace.chat.clone();
        tokio::spawn(async move {
            let _ = budget.measure(&chat).await;
        });
        result
    }

    async fn networked(
        &self,
        workspace: &Workspace,
        command: &str,
        ms: f64,
        cancel: &CancellationToken,
        started: Instant,
    ) -> Result<BashRun, SandboxError> {
        self.ensure_box(workspace, true).await?;
        if cancel.is_cancelled() {
            return Err(SandboxError::Cancelled);
        }
        let result = self
            .backend
            .one_shot(workspace, true, command, ms, "", None, cancel)
            .await?;
        if cancel.is_cancelled() {
            return Err(SandboxError::Cancelled);
        }
        let note = self
            .take_halt(&workspace.chat)
            .unwrap_or_else(|| NETWORK_NOTE.to_string());
        Ok(BashRun {
            exit_code: result.code,
            formatted: format_result(
                &result.output,
                result.code,
                elapsed_ms(started),
                result.code == 124,
                ms,
                Some(&note),
            ),
            storage: None,
        })
    }

    async fn persistent(
        &self,
        workspace: &Workspace,
        command: &str,
        ms: f64,
        cancel: &CancellationToken,
        started: Instant,
    ) -> Result<BashRun, SandboxError> {
        let shell = self.use_box(&workspace.chat).await;
        let _one_at_a_time = shell.lock().await;
        // A live persistent shell proves its box is up.
        if !self.backend.has_session(workspace).await {
            self.ensure_box(workspace, false).await?;
        }
        if cancel.is_cancelled() {
            self.reset_shell(&workspace.chat).await;
            return Err(SandboxError::Cancelled);
        }
        let run = match self
            .backend
            .session(workspace, command, ms, None, cancel)
            .await
        {
            Ok(run) => run,
            Err(error) => {
                self.reset_shell(&workspace.chat).await;
                return Err(error);
            }
        };
        if run.reset {
            self.reset_shell(&workspace.chat).await;
        }
        let note = self
            .take_halt(&workspace.chat)
            .or_else(|| run.timed_out.then(|| SESSION_RESET_NOTE.to_string()));
        Ok(BashRun {
            exit_code: run.exit_code,
            formatted: format_result(
                &run.text,
                run.exit_code,
                elapsed_ms(started),
                run.timed_out,
                ms,
                note.as_deref(),
            ),
            storage: None,
        })
    }

    /// `restricted`: over the limits, the command runs in a one-shot shell
    /// under `ulimit -f 0` after the session and its jobs are ended.
    async fn restricted(
        &self,
        workspace: &Workspace,
        command: &str,
        ms: f64,
        cancel: &CancellationToken,
        refusal: &str,
        started: Instant,
    ) -> Result<BashRun, SandboxError> {
        let chat = &workspace.chat;
        self.ensure_box(workspace, false).await?;
        // A halt under way ends first, so its kill cannot land on this command.
        if let Some(State::Halted { ended, .. }) = self.state_of(chat) {
            done(ended).await;
        }
        self.change(chat, &["ready", "halted"], State::Restricted)?;
        let result = async {
            self.reset_shell(chat).await;
            if cancel.is_cancelled() {
                return Err(SandboxError::Cancelled);
            }
            let result = self
                .backend
                .one_shot(
                    workspace,
                    false,
                    command,
                    ms,
                    WRITES_DISABLED_LIMITS,
                    None,
                    cancel,
                )
                .await?;
            if cancel.is_cancelled() {
                return Err(SandboxError::Cancelled);
            }
            self.budget
                .measure(chat)
                .await
                .map_err(SandboxError::Failed)?;
            let still = self
                .budget
                .refusal(chat)
                .await
                .map_err(SandboxError::Failed)?;
            Ok(BashRun {
                exit_code: result.code,
                formatted: format_result(
                    &result.output,
                    result.code,
                    elapsed_ms(started),
                    result.code == 124,
                    ms,
                    Some(RESTRICTED_NOTE),
                ),
                storage: Some(storage_message(refusal, still.as_deref())),
            })
        }
        .await;
        // A reset meanwhile has the sandbox removing: that stands.
        if let Some(tracked) = self.lock().get_mut(chat.as_str())
            && matches!(tracked.state, State::Restricted)
        {
            tracked.state = State::Ready;
        }
        result
    }

    /// `halt`: the chat's processes ended because of `refusal`.
    async fn halt(&self, chat: &str, refusal: &str) -> Result<(), SandboxError> {
        let (ended_tx, ended) = watch::channel(false);
        self.change(
            chat,
            &["ready"],
            State::Halted {
                reason: halt_reason(refusal),
                ended,
            },
        )?;
        self.backend.kill_session(chat).await;
        tokio::join!(
            self.backend.end_processes(chat, false),
            self.backend.end_processes(chat, true)
        );
        let _ = ended_tx.send(true);
        Ok(())
    }

    /// `watchStorage`.
    async fn watch_storage(&self) -> Result<(), String> {
        if self.lock().is_empty() || self.watching.load(Ordering::SeqCst) {
            return Ok(());
        }
        let free = self.budget.free_bytes()?;
        {
            let mut watched = self.watched_free.lock().expect("watched");
            if watched.is_none_or(|watched| free > watched) {
                *watched = Some(free);
            }
            let reserve = self.budget.disk_reserve_bytes();
            if free >= reserve && watched.expect("set above") - free < WATCH_STEP {
                return Ok(());
            }
            *watched = Some(free);
        }
        self.watching.store(true, Ordering::SeqCst);
        let result = async {
            let ids: Vec<String> = self.lock().keys().cloned().collect();
            for id in ids {
                self.budget.measure(&id).await?;
                let refusal = self.budget.refusal(&id).await?;
                if let Some(refusal) = refusal
                    && matches!(self.state_of(&id), Some(State::Ready))
                {
                    self.halt(&id, &refusal)
                        .await
                        .map_err(|error| error.to_string())?;
                }
            }
            Ok(())
        }
        .await;
        self.watching.store(false, Ordering::SeqCst);
        result
    }

    /// `reapIdle`.
    fn reap_idle_at(self: &Arc<Self>, now: Instant) {
        let idle: Vec<String> = self
            .lock()
            .iter()
            .filter(|(_, tracked)| now.saturating_duration_since(tracked.last_used) > IDLE)
            .map(|(id, _)| id.clone())
            .collect();
        for id in idle {
            let removed = self.begin_reset(&id);
            tokio::spawn(done(removed));
        }
    }

    /// `reset`'s first, synchronous part: the chat is removing from now on,
    /// and the removal runs; what it answers resolves when it is done.
    fn begin_reset(self: &Arc<Self>, chat: &str) -> watch::Receiver<bool> {
        let mut boxes = self.lock();
        let tracked = Self::track(&mut boxes, chat);
        if let State::Removing { removed } = &tracked.state {
            return removed.clone();
        }
        let (removed_tx, removed) = watch::channel(false);
        tracked.state = State::Removing {
            removed: removed.clone(),
        };
        drop(boxes);
        let inner = self.clone();
        let chat = chat.to_string();
        tokio::spawn(async move {
            inner.backend.kill_session(&chat).await;
            inner.backend.remove(&chat).await;
            inner.lock().remove(&chat);
            let _ = removed_tx.send(true);
        });
        removed
    }

    async fn reset(self: &Arc<Self>, chat: &str) {
        done(self.begin_reset(chat)).await;
    }

    /// `viewImage`'s Pillow script.
    const VIEW_IMAGE: &'static str = "import sys\nfrom PIL import Image\nimg = Image.open(sys.argv[1])\nimg = img.convert(\"RGBA\")\nscale = min(1.0, 1568 / max(img.size))\nif scale < 1.0: img = img.resize((round(img.width * scale), round(img.height * scale)), Image.LANCZOS)\nimg.save(sys.stdout.buffer, \"PNG\")";

    async fn view_image(&self, workspace: &Workspace, path: &str) -> Result<Vec<u8>, SandboxError> {
        let file = work::resolve_work_path(workspace, path, false).map_err(SandboxError::Failed)?;
        if !file.exists() {
            return Err(SandboxError::Failed(format!(
                "No such file in /work: {path}"
            )));
        }
        let root = std::fs::canonicalize(work::work_dir(workspace).map_err(SandboxError::Failed)?)
            .map_err(|error| SandboxError::Failed(error.to_string()))?;
        let rel = file
            .strip_prefix(&root)
            .expect("a resolved /work path is inside /work")
            .to_string_lossy()
            .into_owned();
        self.ensure_box(workspace, false).await?;
        let argv = ["python3", "-c", Self::VIEW_IMAGE, &rel].map(String::from);
        let run = self
            .backend
            .program(workspace, &argv, Duration::from_secs(30))
            .await?;
        if run.code != 0 {
            return Err(crate::backend::exec_failed(&run.code.to_string(), &run.all));
        }
        Ok(run.stdout)
    }

    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), SandboxError> {
        self.ensure_box(workspace, false).await?;
        let argv: Vec<String> = ["rm", "-f", "--"]
            .iter()
            .map(|s| s.to_string())
            .chain(paths.iter().cloned())
            .collect();
        let run = self
            .backend
            .program(workspace, &argv, Duration::from_secs(60))
            .await?;
        if run.code != 0 {
            return Err(crate::backend::exec_failed(&run.code.to_string(), &run.all));
        }
        Ok(())
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis() as u64
}

#[async_trait]
impl<B: Backend> Sandbox for Sandboxes<B> {
    async fn prepare(&self) -> Result<(), SandboxError> {
        self.inner.backend.prepare().await
    }

    async fn exec(
        &self,
        workspace: &Workspace,
        command: &BashCommand,
        cancel: &CancellationToken,
    ) -> Result<BashRun, SandboxError> {
        self.inner.exec(workspace, command, cancel).await
    }

    async fn view_image(&self, workspace: &Workspace, path: &str) -> Result<Vec<u8>, SandboxError> {
        self.inner.view_image(workspace, path).await
    }

    async fn remove_work_files(
        &self,
        workspace: &Workspace,
        paths: &[String],
    ) -> Result<(), SandboxError> {
        self.inner.remove_work_files(workspace, paths).await
    }

    async fn reset(&self, chat: &str) {
        self.inner.reset(chat).await
    }

    async fn destroy(&self, workspace: &Workspace) -> Result<(), SandboxError> {
        self.inner.reset(&workspace.chat).await;
        match std::fs::remove_dir_all(&workspace.host) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(SandboxError::Failed(error.to_string()));
            }
            _ => {}
        }
        self.inner.budget.forget(&workspace.chat);
        Ok(())
    }

    async fn shutdown(&self) {
        self.inner.stopped.cancel();
        let ids: Vec<String> = self.inner.lock().keys().cloned().collect();
        let removals: Vec<_> = ids.iter().map(|id| self.inner.begin_reset(id)).collect();
        for removed in removals {
            done(removed).await;
        }
        self.inner.backend.shutdown().await;
    }

    fn network_access(&self) -> bool {
        self.inner.network.load(Ordering::SeqCst)
    }

    fn disable_network(&self) {
        self.inner.network.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests;
