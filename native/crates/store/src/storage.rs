//! Storage limits (`server/storage.js`). Each chat has a byte budget covering
//! its attachments, images, world chunks and /work folder, and nothing new is
//! written while the disk holding the data folder has less than a reserve
//! free. A chat's use is what du counts in its four folders (disk blocks),
//! plus what the app has written there since it last measured.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use crate::Error;
use crate::host::Host;

const GIB: u64 = 1024 * 1024 * 1024;
/// The disk's free space is read at most this often (`READ_MS`).
const READ_MS: i64 = 500;
/// The folders a chat holds files in, under the data folder (`FOLDERS`).
pub const FOLDERS: [&str; 4] = ["worlds", "images", "attachments", "sandbox"];

/// The operator's limits: `MCBENCH_CHAT_MAX_BYTES`,
/// `MCBENCH_DISK_RESERVE_BYTES`, `MCBENCH_REPORTS_KEPT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// A chat's budget (`CHAT_MAX_BYTES`, 4 GiB unless set).
    pub chat_max_bytes: u64,
    /// The disk's reserve (`DISK_RESERVE_BYTES`, 2 GiB unless set).
    pub disk_reserve_bytes: u64,
    /// The test_build reports a chat keeps (`REPORTS_KEPT`, 100 unless set).
    pub reports_kept: u64,
}

/// JavaScript's whitespace, which `Number(text)` trims.
pub(crate) fn js_space(c: char) -> bool {
    matches!(
        c,
        '\u{9}' | '\u{a}' | '\u{b}' | '\u{c}' | '\u{d}' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// JavaScript's `Number(text)` for a string.
pub fn js_number(text: &str) -> f64 {
    let text = text.trim_matches(js_space);
    if text.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0o", 8),
        ("0O", 8),
        ("0b", 2),
        ("0B", 2),
    ] {
        if let Some(digits) = text.strip_prefix(prefix) {
            if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return digits.chars().fold(0.0, |n, c| {
                n * f64::from(radix) + f64::from(c.to_digit(radix).expect("a digit"))
            });
        }
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    if unsigned == "Infinity" {
        return if text.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let decimal = crate::regex!(r"^[+-]?(\d+\.?\d*|\.\d+)([eE][+-]?\d+)?$");
    if !decimal.is_match(text) {
        return f64::NAN;
    }
    text.parse().expect("a decimal literal")
}

fn setting(value: Option<&str>, name: &str, standard: u64, min: u64) -> Result<u64, String> {
    let value = match value {
        Some(text) if !text.is_empty() => js_number(text),
        _ => standard as f64,
    };
    let safe = value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_991.0;
    if !safe || value < min as f64 {
        return Err(format!("{name} must be an integer of at least {min}"));
    }
    Ok(value as u64)
}

impl Limits {
    /// The limits the environment names (`lookup` is `std::env::var`'s), or
    /// today's refusal when one is not an integer at least its minimum.
    pub fn from_env(lookup: impl Fn(&str) -> Option<String>) -> Result<Limits, String> {
        let read =
            |name: &str, standard, min| setting(lookup(name).as_deref(), name, standard, min);
        Ok(Limits {
            chat_max_bytes: read("MCBENCH_CHAT_MAX_BYTES", 4 * GIB, 1)?,
            disk_reserve_bytes: read("MCBENCH_DISK_RESERVE_BYTES", 2 * GIB, 0)?,
            reports_kept: read("MCBENCH_REPORTS_KEPT", 100, 0)?,
        })
    }
}

/// `Number.prototype.toFixed(digits)` for a non-negative double below 1e21.
fn to_fixed(value: f64, digits: usize) -> String {
    // The exact decimal expansion: a double below 2^64 with this module's
    // values (bytes over powers of two) has well under 40 fractional digits.
    let exact = format!("{value:.40}");
    let (whole, fraction) = exact.split_once('.').expect("a fraction");
    let mut kept: Vec<u8> = format!("{whole}{}", &fraction[..digits]).into_bytes();
    // A tie is rounded up: toFixed picks the larger n.
    if fraction.as_bytes()[digits] >= b'5' {
        let mut at = kept.len();
        loop {
            if at == 0 {
                kept.insert(0, b'1');
                break;
            }
            at -= 1;
            if kept[at] == b'9' {
                kept[at] = b'0';
            } else {
                kept[at] += 1;
                break;
            }
        }
    }
    let text = String::from_utf8(kept).expect("ASCII digits");
    let split = text.len() - digits;
    if digits == 0 {
        text
    } else {
        format!("{}.{}", &text[..split], &text[split..])
    }
}

/// `size(bytes)`: how the refusals write a number of bytes.
pub fn size(bytes: i64) -> String {
    if bytes >= GIB as i64 {
        format!("{} GiB", to_fixed(bytes as f64 / GIB as f64, 2))
    } else if bytes >= 1024 * 1024 {
        format!("{} MiB", to_fixed(bytes as f64 / (1024.0 * 1024.0), 1))
    } else {
        format!("{} KiB", (bytes as f64 / 1024.0).ceil() as i64)
    }
}

/// A write the limits refuse: HTTP 507, `code: 'STORAGE_FULL'`.
fn refused(message: String) -> Error {
    Error::refused(message, 507)
}

#[derive(Default)]
struct ChatUse {
    /// What it holds, or `None` until measured (again, after a measurement
    /// failed).
    bytes: Option<i64>,
    /// Every byte ever reserved, so a measurement adds those reserved while
    /// it ran.
    charged: i64,
    /// A measurement is running.
    running: bool,
    /// Those waiting for the measurement after the running one.
    queued: Option<Vec<mpsc::Sender<Result<(), String>>>>,
}

struct State {
    chats: HashMap<String, ChatUse>,
    /// The disk's free space as last read: `(at, free)`.
    reading: Option<(i64, i64)>,
}

/// The budgets of one data folder's chats.
pub struct Storage {
    root: PathBuf,
    limits: Limits,
    host: Arc<dyn Host>,
    state: Mutex<State>,
}

/// A measurement [`Storage::measure`] started: [`Measurement::wait`] for it.
pub struct Measurement(mpsc::Receiver<Result<(), String>>);

impl Measurement {
    /// Waits for the measurement; its error is what du's would be.
    pub fn wait(self) -> Result<(), String> {
        self.0.recv().expect("a measurement always answers")
    }
}

impl Storage {
    /// The budgets of the chats under `root`.
    pub fn new(root: PathBuf, limits: Limits, host: Arc<dyn Host>) -> Storage {
        Storage {
            root,
            limits,
            host,
            state: Mutex::new(State {
                chats: HashMap::new(),
                reading: None,
            }),
        }
    }

    /// The limits in force.
    pub fn limits(&self) -> Limits {
        self.limits
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .expect("the storage lock is never poisoned")
    }

    /// `freeBytes`: the disk's free space, read at most every 500 ms, less
    /// what was reserved since.
    pub fn free_bytes(&self) -> Result<i64, Error> {
        let mut state = self.lock();
        self.free_locked(&mut state)
    }

    fn free_locked(&self, state: &mut State) -> Result<i64, Error> {
        let now = self.host.now_ms();
        match state.reading {
            Some((at, free)) if now - at < READ_MS => Ok(free),
            _ => {
                let free = self.host.free_bytes(&self.root).map_err(Error::plain)? as i64;
                state.reading = Some((now, free));
                Ok(free)
            }
        }
    }

    /// Forgets the disk's last reading (`_internal.dropReading`).
    pub fn drop_reading(&self) {
        self.lock().reading = None;
    }

    fn disk_refusal(&self, state: &mut State, bytes: u64) -> Result<Option<String>, Error> {
        let free = self.free_locked(state)?;
        if free - bytes as i64 >= self.limits.disk_reserve_bytes as i64 {
            return Ok(None);
        }
        let needs = if bytes > 0 {
            format!(" and this write needs {}", size(bytes as i64))
        } else {
            String::new()
        };
        Ok(Some(format!(
            "The disk holding the data folder has {} free{needs}; mcbench-lite keeps {} free (MCBENCH_DISK_RESERVE_BYTES) and saves nothing new until space is freed.",
            size(free),
            size(self.limits.disk_reserve_bytes as i64)
        )))
    }

    /// Every folder chat `id` holds, whether or not it exists
    /// (`chatFolders`).
    pub fn chat_folders(&self, id: &str) -> Vec<PathBuf> {
        FOLDERS
            .iter()
            .map(|folder| self.root.join(folder).join(id))
            .collect()
    }

    fn used_locked(&self, state: &mut State, id: &str) -> Result<i64, Error> {
        let chat = state.chats.entry(id.to_string()).or_default();
        if chat.bytes.is_none() {
            chat.bytes = Some(measure_now(&self.chat_folders(id)).map_err(Error::plain)?);
        }
        Ok(chat.bytes.expect("measured"))
    }

    /// `used`: what chat `id` holds now. An ask while it is unknown measures
    /// it, and fails as that measurement failed.
    pub fn used(&self, id: &str) -> Result<i64, Error> {
        let mut state = self.lock();
        self.used_locked(&mut state, id)
    }

    fn chat_refusal(
        &self,
        state: &mut State,
        id: &str,
        bytes: u64,
    ) -> Result<Option<String>, Error> {
        let held = self.used_locked(state, id)?;
        if held + bytes as i64 <= self.limits.chat_max_bytes as i64 {
            return Ok(None);
        }
        let needs = if bytes > 0 {
            format!(" and this write needs {}", size(bytes as i64))
        } else {
            String::new()
        };
        Ok(Some(format!(
            "This chat's storage is full: its attachments, images, world and /work hold {} of its {} budget (MCBENCH_CHAT_MAX_BYTES){needs}.",
            size(held),
            size(self.limits.chat_max_bytes as i64)
        )))
    }

    /// `reserve(id, bytes, { disk })`: counts `bytes` about to be written for
    /// chat `id`, or refuses (507) when they do not fit its budget or the
    /// disk's reserve. `disk: false` skips the disk.
    pub fn reserve(&self, id: &str, bytes: u64, disk: bool) -> Result<(), Error> {
        let mut state = self.lock();
        let refusal = match if disk {
            self.disk_refusal(&mut state, bytes)?
        } else {
            None
        } {
            Some(refusal) => Some(refusal),
            None => self.chat_refusal(&mut state, id, bytes)?,
        };
        if let Some(refusal) = refusal {
            return Err(refused(refusal));
        }
        let chat = state.chats.get_mut(id).expect("measured above");
        chat.bytes = Some(chat.bytes.expect("measured above") + bytes as i64);
        chat.charged += bytes as i64;
        if let Some((_, free)) = &mut state.reading {
            *free -= bytes as i64;
        }
        Ok(())
    }

    /// `refund`: bytes reserved and then not written. A chat forgotten or
    /// unknown since counts nothing.
    pub fn refund(&self, id: &str, bytes: u64) {
        let mut state = self.lock();
        if let Some(ChatUse {
            bytes: Some(held), ..
        }) = state.chats.get_mut(id)
        {
            *held -= bytes as i64;
        }
    }

    /// `refusal`: why nothing more may be written for chat `id`, or `None`.
    pub fn refusal(&self, id: &str) -> Result<Option<String>, Error> {
        let mut state = self.lock();
        match self.disk_refusal(&mut state, 0)? {
            Some(refusal) => Ok(Some(refusal)),
            None => self.chat_refusal(&mut state, id, 0),
        }
    }

    /// `measure`: measures chat `id` again in the background. Asked while
    /// one runs, it measures once more after that one, so the result
    /// postdates the ask. Writes reserved meanwhile stay counted.
    pub fn measure(self: &Arc<Self>, id: &str) -> Measurement {
        let (tx, rx) = mpsc::channel();
        let mut state = self.lock();
        let chat = state.chats.entry(id.to_string()).or_default();
        if let Some(queued) = &mut chat.queued {
            queued.push(tx);
        } else if chat.running {
            chat.queued = Some(vec![tx]);
        } else {
            chat.running = true;
            // What was reserved before this ask is counted by the walk or
            // was never written; what is reserved from now on is added.
            let before = chat.charged;
            drop(state);
            let storage = Arc::clone(self);
            let id = id.to_string();
            self.host.background(Box::new(move || {
                storage.run_measurements(&id, before, vec![tx])
            }));
        }
        Measurement(rx)
    }

    fn run_measurements(
        &self,
        id: &str,
        mut before: i64,
        mut waiting: Vec<mpsc::Sender<Result<(), String>>>,
    ) {
        loop {
            let measured = measure_now(&self.chat_folders(id));
            let mut state = self.lock();
            let chat = state.chats.entry(id.to_string()).or_default();
            let result = match measured {
                Ok(bytes) => {
                    chat.bytes = Some(bytes + chat.charged - before);
                    Ok(())
                }
                Err(error) => {
                    chat.bytes = None;
                    Err(error)
                }
            };
            for waiter in waiting.drain(..) {
                // One that stopped waiting has nothing to be told.
                let _ = waiter.send(result.clone());
            }
            match chat.queued.take() {
                Some(next) => {
                    waiting = next;
                    before = chat.charged;
                }
                None => {
                    chat.running = false;
                    return;
                }
            }
        }
    }

    /// `forget`: drops chat `id`'s measurement.
    pub fn forget(&self, id: &str) {
        self.lock().chats.remove(id);
    }
}

/// `expendable(names, referenced, keep, age)`: which of `names` to delete so
/// at most `keep` remain: those `referenced` does not hold, oldest first by
/// `age` (then by name). Only when some are kept is anything aged.
pub fn expendable(
    names: &[String],
    referenced: &HashSet<String>,
    keep: usize,
    age: impl Fn(&str) -> f64,
) -> Vec<String> {
    let excess = names.len() as i64 - keep as i64;
    if excess <= 0 {
        return Vec::new();
    }
    let candidates: Vec<&String> = names
        .iter()
        .filter(|name| !referenced.contains(*name))
        .collect();
    if candidates.len() as i64 <= excess {
        return candidates.into_iter().cloned().collect();
    }
    let mut aged: Vec<(f64, &String)> = candidates
        .into_iter()
        .map(|name| (age(name), name))
        .collect();
    aged.sort_by(|a, b| {
        // `a[0] - b[0] || (a[1] < b[1] ? -1 : 1)`, the names compared by
        // UTF-16 code units as JavaScript's `<` does.
        let by_age = a.0 - b.0;
        if by_age != 0.0 && !by_age.is_nan() {
            return by_age.partial_cmp(&0.0).expect("a number");
        }
        if a.1.encode_utf16().lt(b.1.encode_utf16()) {
            std::cmp::Ordering::Less
        } else {
            std::cmp::Ordering::Greater
        }
    });
    aged.into_iter()
        .take(excess as usize)
        .map(|(_, name)| name.clone())
        .collect()
}

/// What `du -sk folders...` counts, in bytes: each folder's disk blocks,
/// itself included, a file with several links counted once, links not
/// followed, each folder's total rounded up to whole KiB. Folders that do not
/// exist are skipped (`folders(id)`); a file that vanishes during the walk is
/// not counted.
pub fn measure_now(folders: &[PathBuf]) -> Result<i64, String> {
    let mut seen = HashSet::new();
    let mut total = 0i64;
    for folder in folders.iter().filter(|folder| folder.exists()) {
        let blocks = walk(folder, &mut seen)
            .map_err(|error| format!("du did not measure {}: {error}", folder.display()))?;
        total += blocks.div_ceil(1024) as i64 * 1024;
    }
    Ok(total)
}

/// The bytes of disk blocks under `path`.
fn walk(path: &Path, seen: &mut HashSet<(u64, u64)>) -> std::io::Result<u64> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut bytes = allocated(&metadata, seen);
    if metadata.is_dir() {
        let entries = match std::fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(bytes),
            Err(error) => return Err(error),
        };
        for entry in entries {
            bytes += walk(&entry?.path(), seen)?;
        }
    }
    Ok(bytes)
}

#[cfg(unix)]
fn allocated(metadata: &std::fs::Metadata, seen: &mut HashSet<(u64, u64)>) -> u64 {
    use std::os::unix::fs::MetadataExt;
    if !metadata.is_dir() && metadata.nlink() > 1 && !seen.insert((metadata.dev(), metadata.ino()))
    {
        return 0;
    }
    metadata.blocks() * 512
}

#[cfg(windows)]
fn allocated(metadata: &std::fs::Metadata, _: &mut HashSet<(u64, u64)>) -> u64 {
    // Windows has no du and no block count in its metadata: a file counts
    // its length rounded up to the 4 KiB cluster NTFS uses by default.
    if metadata.is_dir() {
        0
    } else {
        metadata.len().div_ceil(4096) * 4096
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_written_as_today() {
        assert_eq!(size(3 * 1024 * 1024), "3.0 MiB");
        assert_eq!(size(2 * GIB as i64), "2.00 GiB");
        assert_eq!(size(1000 * 4096), "3.9 MiB");
        assert_eq!(size(10), "1 KiB");
        assert_eq!(size(0), "0 KiB");
        // A tie rounds up, as toFixed does: 1.125 GiB.
        assert_eq!(size((GIB + GIB / 8) as i64), "1.13 GiB");
        assert_eq!(to_fixed(0.05, 1), "0.1");
        assert_eq!(to_fixed(9.96, 1), "10.0");
    }

    #[test]
    fn numbers_read_as_javascript_reads_them() {
        assert!(js_number("4GB").is_nan());
        assert_eq!(js_number(" 12 "), 12.0);
        assert_eq!(js_number("0x10"), 16.0);
        assert_eq!(js_number("1e3"), 1000.0);
        assert_eq!(js_number("1.5"), 1.5);
        assert_eq!(js_number(""), 0.0);
        assert!(js_number("1_000").is_nan());
    }
}
