//! Windows' installed-apps list after a self-update: the installer wrote
//! `DisplayVersion` and `InstallLocation` under its uninstall key. Once a
//! new program's core runs, it sets `DisplayVersion` to its own version
//! when `InstallLocation` names its install folder (a second copy elsewhere
//! must not rewrite the installed copy's entry) and the value differs; a
//! reversed swap therefore leaves nothing to undo. What is read and written
//! is decided here in plain Rust, tested on every system; only the
//! registry calls are Windows code.

use std::path::Path;

/// The installer's uninstall key, under `HKEY_CURRENT_USER`.
const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\mcbench-lite";

/// The value the installed-apps list shows as the version.
const VALUE: &str = "DisplayVersion";

/// The value the installer writes with the install's folder.
const LOCATION: &str = "InstallLocation";

/// The text of NUL-terminated UTF-16 `units` (up to the first NUL).
fn from_utf16z(units: &[u16]) -> String {
    let end = units.iter().position(|&u| u == 0).unwrap_or(units.len());
    String::from_utf16_lossy(&units[..end])
}

/// Whether the installer's `location` names the payload folder `path`:
/// the same path without a `\\?\` prefix, a trailing backslash or ASCII
/// case (Windows paths are case-insensitive), with `/` read as `\`.
fn names_install(location: &str, path: &Path) -> bool {
    let plain = |text: &str| {
        let text = text.replace('/', "\\");
        let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
        text.trim_end_matches('\\').to_ascii_lowercase()
    };
    plain(location) == plain(&path.to_string_lossy())
}

/// `text` as UTF-16 with its terminating NUL, as the registry's wide
/// functions take names.
fn utf16z(text: &str) -> Vec<u16> {
    text.encode_utf16().chain([0]).collect()
}

/// `text` as `REG_SZ` data: UTF-16 little-endian bytes with the
/// terminating NUL counted.
fn reg_sz(text: &str) -> Vec<u8> {
    utf16z(text).iter().flat_map(|u| u.to_le_bytes()).collect()
}

/// The sentence for a failed write of `version`, with the Windows error
/// `code`.
fn failure(version: &str, code: u32) -> String {
    format!(
        "The installed-apps entry HKEY_CURRENT_USER\\{KEY} could not be given {VALUE} {version} (Windows error {code}); the update itself is installed."
    )
}

/// Whether `DisplayVersion` is to be written: the entry's
/// `InstallLocation` names `payload` and it does not show `version` yet.
fn wanted(location: Option<&str>, shown: Option<&str>, payload: &Path, version: &str) -> bool {
    location.is_some_and(|l| names_install(l, payload)) && shown != Some(version)
}

/// Sets `DisplayVersion` of the installer's uninstall entry to `version`
/// (the running program's) when its `InstallLocation` names `payload`, this
/// program's install folder, and the value differs; nothing when the key
/// or that value does not exist (an install the installer did not make) or
/// names another folder. A failure is written to `update.log` in `logs`.
pub(crate) fn record(version: &str, payload: &Path, logs: &Path) {
    let key = utf16z(KEY);
    let data = reg_sz(version);
    let size = u32::try_from(data.len()).expect("a version is far below 4 GiB");
    let read =
        |value: &str| registry::read(&key, &utf16z(value)).map(|v| v.map(|u| from_utf16z(&u)));
    let (location, shown) = match (read(LOCATION), read(VALUE)) {
        (Ok(location), Ok(shown)) => (location, shown),
        (Err(code), _) | (_, Err(code)) => {
            crate::flow::log(logs, &failure(version, code));
            return;
        }
    };
    if !wanted(location.as_deref(), shown.as_deref(), payload, version) {
        return;
    }
    if let Err(code) = registry::set(&key, &utf16z(VALUE), &data, size) {
        crate::flow::log(logs, &failure(version, code));
    }
}

/// Other systems have no registry; only Windows targets call [`record`].
#[cfg(not(windows))]
mod registry {
    pub(super) fn read(_key: &[u16], _value: &[u16]) -> Result<Option<Vec<u16>>, u32> {
        Ok(None)
    }

    pub(super) fn set(_key: &[u16], _value: &[u16], _data: &[u8], _size: u32) -> Result<(), u32> {
        Ok(())
    }
}

#[cfg(windows)]
mod registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW,
        RegOpenKeyExW, RegSetValueExW,
    };

    /// The `REG_SZ` value `value` (NUL-terminated UTF-16) of the key `key`
    /// under `HKEY_CURRENT_USER`, as UTF-16 with its NUL; `None` when the
    /// key or the value does not exist. The error is the Windows error code
    /// (`ERROR_MORE_DATA` for a value longer than the longest path).
    pub(super) fn read(key: &[u16], value: &[u16]) -> Result<Option<Vec<u16>>, u32> {
        let mut data = vec![0u16; 32_768];
        let mut size = u32::try_from(data.len() * 2).expect("64 KiB fits in a u32");
        // SAFETY: `key` and `value` are NUL-terminated UTF-16 that outlive
        // the call; `data` holds `size` bytes and `size` is a valid place
        // for the length written; no type is asked for (null).
        let got = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                data.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if got == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if got != ERROR_SUCCESS {
            return Err(got);
        }
        data.truncate(size as usize / 2);
        Ok(Some(data))
    }

    /// Sets the `REG_SZ` value `value` (NUL-terminated UTF-16) of the
    /// existing key `key` under `HKEY_CURRENT_USER` to `data`, of `size`
    /// bytes (counted before the key is opened, so nothing between the open
    /// and the close can panic); nothing when the key does not exist. The
    /// error is the Windows error code.
    pub(super) fn set(key: &[u16], value: &[u16], data: &[u8], size: u32) -> Result<(), u32> {
        let mut handle: HKEY = std::ptr::null_mut();
        // SAFETY: `key` is NUL-terminated UTF-16 that outlives the call, and
        // `handle` is a valid place for the opened key.
        let opened = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut handle,
            )
        };
        if opened == ERROR_FILE_NOT_FOUND {
            return Ok(());
        }
        if opened != ERROR_SUCCESS {
            return Err(opened);
        }
        // SAFETY: `handle` was opened above with KEY_SET_VALUE; `value` is
        // NUL-terminated UTF-16 and `data` holds `size` bytes, both
        // outliving the call.
        let set = unsafe { RegSetValueExW(handle, value.as_ptr(), 0, REG_SZ, data.as_ptr(), size) };
        // SAFETY: `handle` is the key opened above, closed once.
        unsafe { RegCloseKey(handle) };
        if set == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(set)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The installed copy's entry names the folder that was swapped (as the
    /// canonical path gives it, with `\\?\`); a second copy marked `direct`
    /// elsewhere does not match.
    #[test]
    fn the_entry_is_the_swapped_installs_only() {
        let installed = r"C:\Users\a\AppData\Local\Programs\mcbench-lite";
        for path in [
            r"\\?\C:\Users\a\AppData\Local\Programs\mcbench-lite",
            r"C:\Users\a\AppData\Local\Programs\mcbench-lite",
            r"c:\users\A\appdata\local\programs\MCBENCH-LITE\",
            "C:/Users/A/AppData/Local/Programs/mcbench-lite",
        ] {
            assert!(names_install(installed, Path::new(path)), "{path}");
            assert!(
                names_install(&format!("{installed}\\"), Path::new(path)),
                "{path}"
            );
        }
        for path in [
            r"\\?\D:\tools\mcbench-lite",
            r"C:\Users\a\AppData\Local\Programs\mcbench-lite-2",
            r"C:\Users\a\AppData\Local\Programs",
        ] {
            assert!(!names_install(installed, Path::new(path)), "{path}");
        }
    }

    #[test]
    fn written_only_when_it_is_this_install_and_differs() {
        let here = Path::new(r"\\?\C:\P\mcbench-lite");
        let location = Some(r"C:\P\mcbench-lite");
        assert!(wanted(location, Some("1.0.0"), here, "1.1.0"));
        assert!(wanted(location, None, here, "1.1.0"));
        assert!(!wanted(location, Some("1.1.0"), here, "1.1.0"));
        assert!(!wanted(None, Some("1.0.0"), here, "1.1.0"));
        assert!(!wanted(Some(r"D:\other"), Some("1.0.0"), here, "1.1.0"));
    }

    #[test]
    fn what_is_written() {
        assert_eq!(
            KEY,
            "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\mcbench-lite"
        );
        assert_eq!(VALUE, "DisplayVersion");
        assert_eq!(utf16z("Display"), [68, 105, 115, 112, 108, 97, 121, 0]);
        assert_eq!(
            reg_sz("1.2.0"),
            [b'1', 0, b'.', 0, b'2', 0, b'.', 0, b'0', 0, 0, 0]
        );
        assert_eq!(LOCATION, "InstallLocation");
        assert_eq!(from_utf16z(&[67, 58, 92, 0, 0, 0]), "C:\\");
        assert_eq!(from_utf16z(&utf16z("D:\\x")), "D:\\x");
        assert_eq!(
            failure("1.2.0", 5),
            "The installed-apps entry HKEY_CURRENT_USER\\Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\mcbench-lite could not be given DisplayVersion 1.2.0 (Windows error 5); the update itself is installed."
        );
    }
}
