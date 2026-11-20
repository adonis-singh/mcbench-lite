//! A [`Budget`] for this crate's tests: `storage.js`'s budget of a chat's
//! `/work` alone, measured by walking it, with a free-space reading the
//! test sets. The app's budget is `mcbench-store`'s `Storage`; its refusal
//! text is `storage.js`'s, which these tests expect.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;

use crate::Budget;

const GIB: u64 = 1024 * 1024 * 1024;

/// `storage.js` `size`.
pub fn size(bytes: u64) -> String {
    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= 1024 * 1024 {
        format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{} KiB", bytes.div_ceil(1024))
    }
}

/// The test budget.
pub struct TestBudget {
    /// The sandbox folder: a chat's `/work` is `<root>/<chat>`.
    pub root: PathBuf,
    /// `MCBENCH_CHAT_MAX_BYTES`.
    pub chat_max: u64,
    /// `MCBENCH_DISK_RESERVE_BYTES`.
    pub reserve: u64,
    /// The disk's free bytes as the test sets them; `u64::MAX` reads the
    /// real disk.
    pub free: AtomicU64,
    used: Mutex<HashMap<String, u64>>,
    /// How many measurements ran.
    pub measured: AtomicU64,
}

impl TestBudget {
    /// A budget of `chat_max` bytes per chat and today's 2 GiB reserve.
    pub fn new(root: PathBuf, chat_max: u64) -> Self {
        TestBudget {
            root,
            chat_max,
            reserve: 2 * GIB,
            free: AtomicU64::new(u64::MAX),
            used: Mutex::new(HashMap::new()),
            measured: AtomicU64::new(0),
        }
    }

    fn walk(path: &std::path::Path) -> u64 {
        let Ok(meta) = std::fs::symlink_metadata(path) else {
            return 0;
        };
        if meta.is_dir() {
            std::fs::read_dir(path)
                .map(|entries| {
                    entries
                        .flatten()
                        .map(|entry| Self::walk(&entry.path()))
                        .sum()
                })
                .unwrap_or(0)
        } else {
            meta.len()
        }
    }

    fn disk_refusal(&self) -> Result<Option<String>, String> {
        let free = self.free_bytes()?;
        Ok((free < self.reserve).then(|| {
            format!(
                "The disk holding the data folder has {} free; mcbench-lite keeps {} free (MCBENCH_DISK_RESERVE_BYTES) and saves nothing new until space is freed.",
                size(free),
                size(self.reserve)
            )
        }))
    }
}

#[async_trait]
impl Budget for TestBudget {
    async fn refusal(&self, chat: &str) -> Result<Option<String>, String> {
        if let Some(disk) = self.disk_refusal()? {
            return Ok(Some(disk));
        }
        let known = self.used.lock().expect("used").get(chat).copied();
        let held = match known {
            Some(held) => held,
            None => {
                let held = Self::walk(&self.root.join(chat));
                self.used
                    .lock()
                    .expect("used")
                    .insert(chat.to_string(), held);
                held
            }
        };
        Ok((held > self.chat_max).then(|| {
            format!(
                "This chat's storage is full: its attachments, images, world and /work hold {} of its {} budget (MCBENCH_CHAT_MAX_BYTES).",
                size(held),
                size(self.chat_max)
            )
        }))
    }

    async fn measure(&self, chat: &str) -> Result<(), String> {
        let held = Self::walk(&self.root.join(chat));
        self.used
            .lock()
            .expect("used")
            .insert(chat.to_string(), held);
        self.measured.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn free_bytes(&self) -> Result<u64, String> {
        match self.free.load(Ordering::SeqCst) {
            u64::MAX => Ok(fs_free(&self.root)),
            free => Ok(free),
        }
    }

    fn disk_reserve_bytes(&self) -> u64 {
        self.reserve
    }

    fn forget(&self, chat: &str) {
        self.used.lock().expect("used").remove(chat);
    }
}

#[cfg(unix)]
fn fs_free(path: &std::path::Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).expect("no NUL in a path");
    // SAFETY: statvfs fills the struct it is given from a NUL-terminated path.
    unsafe {
        let mut stat: libc::statvfs = std::mem::zeroed();
        assert_eq!(
            libc::statvfs(path.as_ptr(), &mut stat),
            0,
            "statvfs of the test root"
        );
        stat.f_bavail as u64 * stat.f_frsize as u64
    }
}

/// The free bytes of the disk that holds `path` (the chats' `/work` stay on
/// Windows' own disk, which the distro reaches by drvfs).
#[cfg(windows)]
fn fs_free(path: &std::path::Path) -> u64 {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let mut free = 0u64;
    // SAFETY: a NUL-terminated wide path and one out-pointer to a u64; the
    // other two out-pointers may be null.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_ne!(ok, 0, "GetDiskFreeSpaceExW of the test root");
    free
}
