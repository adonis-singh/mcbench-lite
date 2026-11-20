//! A CLI's whole process tree on Windows: a Job object made with
//! kill-on-job-close holds the CLI from just after its start, so every
//! process it starts (Claude Code's tool bridge, a WSL relay) is in it too.
//! [`Tree::end`] ends them all; dropping the [`Tree`] (the run's task ended
//! or dropped, or the app's process gone however it went, which closes the
//! handle) ends whatever still runs in it. On macOS and Linux a [`Tree`] is
//! nothing: a CLI's run there is stopped as today, by its signals.
//!
//! The CLI joins the job right after its spawn returns, as the
//! engine's child does (`mcbench-engine`'s `child.rs`): a process it started
//! before that would not be in the job, and the CLIs start their children
//! (the MCP bridge) only after reading their configuration.

/// A run's job, or nothing off Windows.
#[cfg(not(windows))]
pub(crate) struct Tree;

#[cfg(not(windows))]
impl Tree {
    /// Nothing to hold.
    pub(crate) fn holding(_: &tokio::process::Child) -> std::io::Result<Tree> {
        Ok(Tree)
    }

    /// Nothing to close.
    pub(crate) fn close(self) {}
}

/// A run's job: its handle, closed once by `drop`.
#[cfg(windows)]
pub(crate) struct Tree(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a job handle is a kernel handle, usable from any thread; it is
// closed once, by `drop`.
#[cfg(windows)]
unsafe impl Send for Tree {}
#[cfg(windows)]
unsafe impl Sync for Tree {}

#[cfg(windows)]
impl Tree {
    /// A new kill-on-close job holding `child`.
    pub(crate) fn holding(child: &tokio::process::Child) -> std::io::Result<Tree> {
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        let process = child
            .raw_handle()
            .ok_or_else(|| std::io::Error::other("the CLI was reaped before its job was made"))?;
        // SAFETY: both pointers may be null (no security attributes, no
        // name); the result is a new handle or null.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        // From here the handle is the Tree's, closed by its drop on every
        // path below.
        let tree = Tree(job);
        // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is valid
        // (no limits); the call reads exactly its size from it.
        let ok = unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                tree.0,
                JobObjectExtendedLimitInformation,
                (&raw const info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: `tree.0` is the job made above and `process` the child's
        // own handle, open while `child` is not reaped (it was just spawned
        // and is waited for only after this returns).
        if unsafe { AssignProcessToJobObject(tree.0, process as _) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(tree)
    }

    /// Closes the job's handle now, which ends what still runs in it.
    pub(crate) fn close(self) {}

    /// Ends every process of the job with exit code 1, as libuv's `kill`
    /// ends a process on Windows.
    pub(crate) fn end(&self) {
        // SAFETY: the handle is this value's, open until it is dropped.
        unsafe {
            windows_sys::Win32::System::JobObjects::TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for Tree {
    fn drop(&mut self) {
        // SAFETY: the handle is this value's and is closed only here; with
        // kill-on-job-close, closing it ends what still runs in the job.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
