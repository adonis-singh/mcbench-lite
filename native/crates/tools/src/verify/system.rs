//! What the verifier reads of the machine: its total memory (Node's
//! `os.totalmem()`, libuv's `uv_get_total_memory`), and random UUIDs
//! (`crypto.randomUUID()`) for report names.

/// The machine's physical memory in bytes, as `os.totalmem()` reads it:
/// `MemTotal` of `/proc/meminfo` on Linux.
#[cfg(target_os = "linux")]
pub fn total_memory() -> Result<u64, String> {
    let text = std::fs::read_to_string("/proc/meminfo")
        .map_err(|error| format!("/proc/meminfo: {error}"))?;
    let line = text
        .lines()
        .find(|line| line.starts_with("MemTotal:"))
        .ok_or("/proc/meminfo has no MemTotal")?;
    let kib: u64 = line
        .trim_start_matches("MemTotal:")
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .map_err(|_| format!("/proc/meminfo: {line}"))?;
    Ok(kib * 1024)
}

/// The machine's physical memory in bytes, as `os.totalmem()` reads it:
/// `hw.memsize` on macOS.
#[cfg(target_os = "macos")]
pub fn total_memory() -> Result<u64, String> {
    let output = mcbench_spawn::command("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .map_err(|error| format!("sysctl hw.memsize: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse()
        .map_err(|_| format!("sysctl hw.memsize printed {text:?}"))
}

/// The machine's physical memory in bytes, as `os.totalmem()` reads it:
/// `ullTotalPhys` of `GlobalMemoryStatusEx` on Windows.
#[cfg(windows)]
pub fn total_memory() -> Result<u64, String> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    // SAFETY: `status` is a MEMORYSTATUSEX with its length set, as the
    // function requires; it writes only into that struct.
    if unsafe { GlobalMemoryStatusEx(&mut status) } == 0 {
        return Err(format!(
            "GlobalMemoryStatusEx: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(status.total_phys)
}

type UuidSource = Box<dyn Fn() -> String + Send + Sync>;

static SOURCE: std::sync::OnceLock<UuidSource> = std::sync::OnceLock::new();

/// Makes [`random_uuid`] answer `source` for the rest of the process (the
/// differential replays' deterministic hosts, as `det.js` replaces
/// `crypto.randomUUID` in Node). A second call fails.
#[doc(hidden)]
pub fn set_uuid_source(source: UuidSource) -> Result<(), String> {
    SOURCE
        .set(source)
        .map_err(|_| "the UUID source is set once per process".to_string())
}

/// A random version 4 UUID in its usual text (`crypto.randomUUID()`).
pub fn random_uuid() -> String {
    if let Some(source) = SOURCE.get() {
        return source();
    }
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the system's random source");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uuid_is_written_as_random_uuid_writes_it() {
        let id = random_uuid();
        assert_eq!(id.len(), 36);
        assert_eq!(&id[14..15], "4");
        assert!("89ab".contains(&id[19..20]));
        assert_ne!(random_uuid(), id);
        assert!(total_memory().unwrap() > 1 << 28);
    }
}
