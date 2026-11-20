//! A unit test's scratch folder: under the cargo target directory's `tmp`
//! (what integration tests know as `CARGO_TARGET_TMPDIR`), never the
//! system's temporary folder, and removed when it is dropped, so when the
//! test ends, passed or failed (a failing assertion unwinds through it).

use std::path::{Path, PathBuf};

/// The folder; it derefs to its path.
pub(crate) struct Scratch(PathBuf);

/// `<target>/tmp`: this test binary is `<target>/<profile>/deps/<name>`.
fn target_tmp() -> PathBuf {
    let exe = std::env::current_exe().expect("the test binary's path");
    let deps = exe.parent().expect("the test binary is in deps/");
    assert_eq!(
        deps.file_name().and_then(|n| n.to_str()),
        Some("deps"),
        "{} is not in <target>/<profile>/deps",
        exe.display()
    );
    deps.parent()
        .and_then(Path::parent)
        .expect("<target>/<profile>/deps")
        .join("tmp")
}

impl Scratch {
    /// A new, empty `<target>/tmp/<name>-<pid>`.
    pub(crate) fn new(name: &str) -> Scratch {
        let dir = target_tmp().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the scratch folder can be made");
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
