//! The shipped Minecraft data on disk: the bundle's files unpacked into the
//! data folder once (`minecraft/<version>-<first 16 digits of the bundle's
//! sha256>/`, which appears whole by a rename), read from there by the
//! engine (`Engine::start` takes the folder) and the renderer (`Assets`
//! names its files). Tests make a [`Data`] from any folder holding the
//! files [`FILES`](crate::bundle::FILES) names.

use std::path::{Path, PathBuf};

use crate::bundle::{self, Lock};
use crate::jar::Manifest;

/// The shipped data's folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Data {
    folder: PathBuf,
}

/// The bundle embedded by the build (feature `shipped-data`, on in every
/// release build).
#[cfg(feature = "shipped-data")]
pub const BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/minecraft-data.tar.zst"));

impl Data {
    /// The data in `folder`, which holds the bundle's files.
    pub fn at(folder: &Path) -> Data {
        Data {
            folder: folder.to_path_buf(),
        }
    }

    /// The embedded bundle unpacked into `data_folder` (see [`Data::unpack`]).
    #[cfg(feature = "shipped-data")]
    pub fn shipped(data_folder: &Path) -> Result<Data, String> {
        Data::unpack(BUNDLE, &Lock::parse(bundle::LOCK)?, data_folder)
    }

    /// `bundle`, which must be the one `lock` names, unpacked into
    /// `data_folder` unless it is there: written into a staging folder that
    /// is renamed into place once every file is written. Anything else
    /// there (another version's folder, a staging folder a stopped app
    /// left) is removed: one app holds a data folder ([`InstanceLock`]).
    ///
    /// [`InstanceLock`]: crate::InstanceLock
    pub fn unpack(bundle: &[u8], lock: &Lock, data_folder: &Path) -> Result<Data, String> {
        let parent = data_folder.join("minecraft");
        let id = format!("{}-{}", lock.minecraft, &lock.sha256[..16]);
        let folder = parent.join(&id);
        if !folder.is_dir() {
            let files = bundle::open(bundle, lock)?;
            std::fs::create_dir_all(&parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            let staging = parent.join(format!("{id}.{}.partial", std::process::id()));
            remove(&staging)?;
            std::fs::create_dir(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
            for (name, bytes) in &files {
                let file = staging.join(name);
                std::fs::write(&file, bytes).map_err(|e| format!("{}: {e}", file.display()))?;
            }
            std::fs::rename(&staging, &folder).map_err(|e| format!("{}: {e}", folder.display()))?;
        }
        for entry in std::fs::read_dir(&parent).map_err(|e| format!("{}: {e}", parent.display()))? {
            let entry = entry.map_err(|e| format!("{}: {e}", parent.display()))?;
            if entry.file_name() != id.as_str() {
                remove(&entry.path())?;
            }
        }
        Ok(Data { folder })
    }

    /// The folder: the engine's tables are in it (`Engine::start`).
    pub fn folder(&self) -> &Path {
        &self.folder
    }

    /// Its folder's name, which names what is made from it (the baked
    /// textures' folder).
    pub fn id(&self) -> String {
        self.folder
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    }

    /// One of its files by its bundle name.
    pub fn file(&self, name: &str) -> PathBuf {
        self.folder.join(name)
    }

    /// One of its files' bytes.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let file = self.file(name);
        std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))
    }

    /// One of its files as text.
    pub fn text(&self, name: &str) -> Result<String, String> {
        String::from_utf8(self.read(name)?)
            .map_err(|_| format!("{name} of the shipped data is not UTF-8"))
    }

    /// The client-jar manifest.
    pub fn manifest(&self) -> Result<Manifest, String> {
        serde_json::from_slice(&self.read("client-jar.json")?)
            .map_err(|e| format!("client-jar.json: {e}"))
    }
}

/// Removes `path` (a file or a folder) if it is there.
pub(crate) fn remove(path: &Path) -> Result<(), String> {
    let result = match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    };
    result.map_err(|e| format!("{}: {e}", path.display()))
}
