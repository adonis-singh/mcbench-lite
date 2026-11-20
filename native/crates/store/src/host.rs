//! What the store takes from the machine: the clock, random ids, the disk's
//! free space and a thread for background work. `server/store.js` reads
//! these from `Date.now()`, `new Date()`, `crypto.randomUUID()`,
//! `crypto.randomInt()`, `fs.statfsSync` and the event loop; the store reads
//! them from a [`Host`], so the tests and the differential proof run the same
//! operations with the same ids and times on both sides.

use std::path::Path;

/// The machine as the store sees it.
pub trait Host: Send + Sync {
    /// `Date.now()`: milliseconds since the epoch.
    fn now_ms(&self) -> i64;
    /// `crypto.randomUUID()`.
    fn uuid(&self) -> String;
    /// `crypto.randomInt(below)`: an integer in `0..below`.
    fn random_int(&self, below: u32) -> u32;
    /// The bytes free to this user on the disk holding `folder`
    /// (`statfs` `bavail * bsize`).
    fn free_bytes(&self, folder: &Path) -> Result<u64, String>;
    /// Runs `job` in the background: what `server/storage.js` leaves to the
    /// event loop (a measurement after a chat loads).
    fn background(&self, job: Box<dyn FnOnce() + Send>);
}

/// The real machine.
pub struct SystemHost;

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("the operating system's random source");
    bytes
}

impl Host for SystemHost {
    fn now_ms(&self) -> i64 {
        let since = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is after 1970");
        since.as_millis() as i64
    }

    fn uuid(&self) -> String {
        let mut bytes = random_bytes::<16>();
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    fn random_int(&self, below: u32) -> u32 {
        // Rejection sampling, as crypto.randomInt does: no modulo bias.
        let limit = u32::MAX - u32::MAX % below;
        loop {
            let value = u32::from_le_bytes(random_bytes::<4>());
            if value < limit {
                return value % below;
            }
        }
    }

    fn free_bytes(&self, folder: &Path) -> Result<u64, String> {
        free_bytes(folder)
    }

    fn background(&self, job: Box<dyn FnOnce() + Send>) {
        std::thread::spawn(job);
    }
}

#[cfg(unix)]
fn free_bytes(folder: &Path) -> Result<u64, String> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(folder.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
    let mut stat = std::mem::MaybeUninit::<libc::statfs>::zeroed();
    // SAFETY: `path` is a NUL-terminated string that outlives the call, and
    // `stat` is a buffer of the struct statfs fills; it is read only when
    // the call reports success.
    let result = unsafe { libc::statfs(path.as_ptr(), stat.as_mut_ptr()) };
    if result != 0 {
        return Err(format!(
            "statfs {}: {}",
            folder.display(),
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: statfs returned 0, so it filled the struct.
    let stat = unsafe { stat.assume_init() };
    // The fields' widths differ per platform (u32 or u64): the casts widen.
    #[allow(clippy::unnecessary_cast)]
    let free = stat.f_bavail as u64 * stat.f_bsize as u64;
    Ok(free)
}

#[cfg(windows)]
fn free_bytes(folder: &Path) -> Result<u64, String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = folder
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0u64;
    // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call;
    // the one out pointer is a valid u64 and the other two are null, which
    // the function allows.
    let ok = unsafe {
        windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Err(format!(
            "GetDiskFreeSpaceExW {}: {}",
            folder.display(),
            std::io::Error::last_os_error()
        ));
    }
    Ok(available)
}

/// `new Date(ms).toISOString()`.
pub fn iso(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let in_day = ms.rem_euclid(86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:03}Z",
        in_day / 3_600_000,
        in_day / 60_000 % 60,
        in_day / 1000 % 60,
        in_day % 1000
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn iso_matches_to_iso_string() {
        assert_eq!(super::iso(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(super::iso(1_763_380_800_123), "2025-11-17T12:00:00.123Z");
        assert_eq!(super::iso(951_782_400_000), "2000-02-29T00:00:00.000Z");
    }
}
