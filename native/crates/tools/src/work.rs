//! The chats' `/work` folders under `<data>/sandbox/<chat>` (`sandbox.js`
//! `ROOT`), through `mcbench_sandbox::work` (`workDir`, `writeWorkFile` with
//! its symlink checks): the workspace a tool call is given, and the writer
//! of the context archive (`mcbench-agent`'s `WorkFiles`).

use std::path::PathBuf;

use mcbench_sandbox::{Workspace, work};

/// The data folder's `sandbox` folder.
#[derive(Clone, Debug)]
pub struct WorkFolders {
    /// `<data>/sandbox`.
    pub root: PathBuf,
}

impl WorkFolders {
    /// The `/work` of `chat`.
    pub fn workspace(&self, chat: &str) -> Workspace {
        Workspace {
            chat: chat.to_string(),
            host: self.root.join(chat),
        }
    }

    /// `workDir(chat)`: the folder, with `images/` and `attachments/` made.
    pub fn work_dir(&self, chat: &str) -> Result<PathBuf, String> {
        work::work_dir(&self.workspace(chat))
    }

    /// `writeWorkFile(chat, relative, text)`.
    pub fn write(&self, chat: &str, relative: &str, text: &str) -> Result<(), String> {
        work::write_work_file(&self.workspace(chat), relative, text.as_bytes())
    }
}
