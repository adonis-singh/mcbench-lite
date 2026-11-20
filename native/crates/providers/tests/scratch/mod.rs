//! An integration test's scratch folder: under `CARGO_TARGET_TMPDIR`, never
//! the system's temporary folder, and removed when it is dropped, so when
//! the test ends, passed or failed (a failing assertion unwinds through it).
//! A folder for Unix sockets is the one exception: it is where production
//! makes its `mcb-XXXXXX` socket folder (`mcbench_relay::socket_base`), and
//! its sockets are bound and reached as production's are
//! (`mcbench_relay::bind_unix`, `connect_unix`), so the length of no
//! folder's path decides whether they work.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// The folder; it derefs to its path.
pub struct Scratch(PathBuf);

impl Scratch {
    /// A new, empty `$CARGO_TARGET_TMPDIR/<name>-<pid>`.
    pub fn new(name: &str) -> Scratch {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch folder can be made");
        Scratch(dir)
    }

    /// A new, empty `<socket base>/mcb-<name>-<pid>` for Unix sockets.
    #[cfg(unix)]
    pub fn for_sockets(name: &str) -> Scratch {
        let dir = mcbench_relay::socket_base()
            .expect("the socket base")
            .join(format!("mcb-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the socket folder can be made");
        Scratch(dir)
    }
}

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Scratch {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
