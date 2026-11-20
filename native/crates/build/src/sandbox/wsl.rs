//! The app's WSL2 distro as the two Windows sandboxes reach it: the build
//! child's ([`super::windows::Wsl`]) and the `bash` tool's
//! (`mcbench_sandbox::windows::WslDistro`, which imports and configures the
//! distro). Its name, its user, and how `wsl.exe` is started. Nothing else
//! names WSL: callers of either sandbox only see the sandbox's trait.
//!
//! Every `wsl.exe` is started without a console window
//! (`CREATE_NO_WINDOW`), with `WSL_UTF8=1` (its own messages in UTF-8, not
//! UTF-16) and without `WSLENV` (no Windows variable reaches the distro),
//! and joins one job object of the app's with kill-on-close (`adopt`, Windows only): when
//! the app ends, however it ends, Windows closes the job's handle and every
//! `wsl.exe` of the app ends with it, which ends what each started in the
//! distro (its stdin, stdout and session close).
//!
//! `wsl.exe` and `powershell.exe` are found in Windows' system folder as
//! `GetSystemDirectoryW` names it ([`system_program`]), never through a
//! variable. Which distros exist is read from the person's own `Lxss`
//! registry key ([`registered`]), which holds every distro's name and disk
//! folder in any language of Windows, without starting `wsl.exe`.
//!
//! [`state`] is what the app knows of its distro, for the build child: no
//! `wsl.exe` is started for a build child before the shell step of setup has
//! said the distro is there (or it is found registered), and while WSL is
//! missing or the distro cannot be started a build answers the same
//! sentence `bash` does.

use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::watch;

use crate::Readiness;

/// What the app knows of its distro: [`Readiness::Waiting`] until setup's
/// shell step has looked, then ready, or unavailable with the sentence a
/// model reads (`mcbench_sandbox::windows`' `WSL_MISSING`, or
/// `WSL_UNAVAILABLE` with the reason).
fn cell() -> &'static watch::Sender<Readiness> {
    static CELL: OnceLock<watch::Sender<Readiness>> = OnceLock::new();
    CELL.get_or_init(|| watch::channel(Readiness::Waiting).0)
}

/// How many times this process imported the distro: what was put into an
/// earlier disk is gone.
static IMPORTS: AtomicU64 = AtomicU64::new(0);

/// The distro's state, as it changes.
pub fn state() -> watch::Receiver<Readiness> {
    cell().subscribe()
}

/// The count of this process's imports of the distro.
pub fn imports() -> u64 {
    IMPORTS.load(Ordering::SeqCst)
}

/// The distro was imported now (a new disk): ready, and counted.
pub fn imported_now() {
    IMPORTS.fetch_add(1, Ordering::SeqCst);
    cell().send_replace(Readiness::Ready);
}

/// The distro is there as it was: ready.
pub fn found() {
    cell().send_if_modified(|state| {
        let changed = *state != Readiness::Ready;
        *state = Readiness::Ready;
        changed
    });
}

/// The distro cannot be used; `sentence` is what a model reads.
pub fn unavailable(sentence: String) {
    cell().send_replace(Readiness::Unavailable(sentence));
}

/// The distro's name: one per Windows user, the app's.
pub const DISTRO: &str = "mcbench-lite";

/// Names another distro (tests import their own, beside the user's app).
pub const DISTRO_VAR: &str = "MCBENCH_WSL_DISTRO";

/// The distro's user, who runs every box and every build child.
pub const USER: &str = "mcbench";

/// That user's ids.
pub const USER_IDS: (u32, u32) = (1000, 1000);

/// The distro: [`DISTRO`], or what [`DISTRO_VAR`] names.
pub fn distro() -> String {
    std::env::var(DISTRO_VAR).unwrap_or_else(|_| DISTRO.to_string())
}

/// `name` in Windows' system folder, as `GetSystemDirectoryW` gives it.
/// Elsewhere than Windows an error.
pub fn system_program(name: &str) -> Result<PathBuf, String> {
    #[cfg(windows)]
    return os::system_directory().map(|dir| dir.join(name));
    #[cfg(not(windows))]
    Err(format!("{name}: WSL is Windows'"))
}

/// `wsl.exe` in Windows' system folder.
pub fn program() -> Result<PathBuf, String> {
    system_program("wsl.exe")
}

/// `wsl.exe` for one of its own commands (`--import`, `--unregister`, ...).
pub fn manage() -> Result<Command, String> {
    // `mcbench_spawn::command` sets `CREATE_NO_WINDOW`: no console appears
    // for the app's `wsl.exe`.
    let mut command = mcbench_spawn::command(program()?);
    command.env("WSL_UTF8", "1").env_remove("WSLENV");
    Ok(command)
}

/// `wsl.exe` running the arguments that follow as `user` in `distro`, in
/// `/`, without a shell (`--exec`).
pub fn exec(distro: &str, user: &str) -> Result<Command, String> {
    let mut command = manage()?;
    command.args([
        "--distribution",
        distro,
        "--user",
        user,
        "--cd",
        "/",
        "--exec",
    ]);
    Ok(command)
}

/// The folder of `distro`'s disk when it is registered for this Windows
/// user, from `HKEY_CURRENT_USER\Software\Microsoft\Windows\CurrentVersion\Lxss`
/// (one subkey per distro with its `DistributionName` and `BasePath`): the
/// same in every language of Windows, read without starting `wsl.exe`. A
/// missing `Lxss` key is no distro at all. Elsewhere than Windows an error.
pub fn registered(distro: &str) -> Result<Option<PathBuf>, String> {
    #[cfg(windows)]
    return os::registered(distro);
    #[cfg(not(windows))]
    Err(format!("{distro}: WSL is Windows'"))
}

#[cfg(windows)]
mod os {
    //! The system folder and the `Lxss` key.

    use std::path::PathBuf;

    use windows_sys::Win32::Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS,
    };
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, RRF_RT_REG_SZ, RegCloseKey, RegEnumKeyExW, RegGetValueW,
        RegOpenKeyExW,
    };
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain([0]).collect()
    }

    fn said(code: u32) -> String {
        std::io::Error::from_raw_os_error(code as i32).to_string()
    }

    pub fn system_directory() -> Result<PathBuf, String> {
        let mut buffer = vec![0u16; 260];
        loop {
            // SAFETY: a buffer of the length given; the call writes at most
            // that many units and answers the length it needs.
            let n = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
            if n == 0 {
                return Err(format!(
                    "GetSystemDirectoryW: {}",
                    std::io::Error::last_os_error()
                ));
            }
            if (n as usize) < buffer.len() {
                return Ok(PathBuf::from(
                    String::from_utf16(&buffer[..n as usize])
                        .map_err(|e| format!("the system folder's name is not UTF-16: {e}"))?,
                ));
            }
            buffer.resize(n as usize, 0);
        }
    }

    /// A registry key closed when dropped.
    struct Key(HKEY);

    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key is open and owned here; it is closed once.
            unsafe { RegCloseKey(self.0) };
        }
    }

    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";

    /// A string value of `key`'s subkey `sub` (null: of `key`).
    fn string(key: &Key, sub: &[u16], name: &str) -> Result<String, String> {
        let name = wide(name);
        let mut size = 0u32;
        // SAFETY: an open key, NUL-terminated names, a size query (null data).
        let code = unsafe {
            RegGetValueW(
                key.0,
                sub.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(said(code));
        }
        let mut buffer = vec![0u16; (size as usize).div_ceil(2)];
        // SAFETY: as above, with a buffer of `size` bytes.
        let code = unsafe {
            RegGetValueW(
                key.0,
                sub.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if code != ERROR_SUCCESS {
            return Err(said(code));
        }
        let units = &buffer[..(size as usize / 2)];
        let units = units.strip_suffix(&[0]).unwrap_or(units);
        String::from_utf16(units).map_err(|e| e.to_string())
    }

    pub fn registered(distro: &str) -> Result<Option<PathBuf>, String> {
        let path = wide(LXSS);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: a predefined root key, a NUL-terminated path, one out-pointer.
        let code =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, path.as_ptr(), 0, KEY_READ, &mut key) };
        if code == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if code != ERROR_SUCCESS {
            return Err(format!("HKEY_CURRENT_USER\\{LXSS}: {}", said(code)));
        }
        let key = Key(key);
        for index in 0.. {
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            // SAFETY: an open key, a buffer of `length` units and its length;
            // the other out-pointers may be null.
            let code = unsafe {
                RegEnumKeyExW(
                    key.0,
                    index,
                    name.as_mut_ptr(),
                    &mut length,
                    std::ptr::null(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if code == ERROR_NO_MORE_ITEMS {
                return Ok(None);
            }
            if code != ERROR_SUCCESS {
                return Err(format!("HKEY_CURRENT_USER\\{LXSS}: {}", said(code)));
            }
            let mut sub = name[..length as usize].to_vec();
            sub.push(0);
            let of = |value: &str| {
                string(&key, &sub, value).map_err(|e| {
                    format!(
                        "HKEY_CURRENT_USER\\{LXSS}\\{}\\{value}: {e}",
                        String::from_utf16_lossy(&name[..length as usize])
                    )
                })
            };
            if of("DistributionName")? == distro {
                return Ok(Some(PathBuf::from(of("BasePath")?)));
            }
        }
        unreachable!("the loop returns")
    }
}

/// Puts `child` (a `wsl.exe` just started) into the app's job object, made
/// once with kill-on-close and never closed by the app, so that the child
/// ends when the app does.
#[cfg(windows)]
pub fn adopt(child: &tokio::process::Child) -> std::io::Result<()> {
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };
    /// The job's handle, as a number: a handle is a process-wide value.
    static JOB: OnceLock<Result<usize, String>> = OnceLock::new();
    let job = JOB
        .get_or_init(|| {
            // SAFETY: both pointers may be null (no security attributes, no
            // name); the result is a new handle or null.
            let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if job.is_null() {
                return Err(std::io::Error::last_os_error().to_string());
            }
            // SAFETY: an all-zero JOBOBJECT_EXTENDED_LIMIT_INFORMATION is
            // valid (no limits); the call reads exactly its size from it.
            let ok = unsafe {
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(job as usize)
        })
        .clone()
        .map_err(std::io::Error::other)?;
    let process = child
        .raw_handle()
        .ok_or_else(|| std::io::Error::other("wsl.exe was reaped before it joined the job"))?;
    // SAFETY: `job` is the handle made above, never closed; `process` is the
    // child's own handle, open while it is not reaped (it was just spawned).
    if unsafe { AssignProcessToJobObject(job as _, process) } == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
