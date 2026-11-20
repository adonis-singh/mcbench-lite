//! Where the app keeps its data (docs/native-app.md, Data folders).

use std::path::PathBuf;

/// The platform's data folder: `~/Library/Application Support/mcbench-lite`
/// on macOS, `%LOCALAPPDATA%\mcbench-lite` on Windows,
/// `$XDG_DATA_HOME/mcbench-lite` on Linux (`~/.local/share` when
/// `XDG_DATA_HOME` is unset or not absolute, as the XDG base directory
/// specification says); `MCBENCH_DATA_DIR` names another.
pub fn data_folder() -> Result<PathBuf, String> {
    data_folder_of(&|name| std::env::var_os(name))
}

/// [`data_folder`] with the environment `env`.
pub fn data_folder_of(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    if let Some(dir) = env("MCBENCH_DATA_DIR").filter(|v| !v.is_empty()) {
        return std::path::absolute(PathBuf::from(dir))
            .map_err(|e| format!("MCBENCH_DATA_DIR: {e}"));
    }
    platform(env).map(|base| base.join("mcbench-lite"))
}

#[cfg(target_os = "macos")]
fn platform(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    let home = env("HOME")
        .filter(|v| !v.is_empty())
        .ok_or("HOME is not set, so the data folder is not known")?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support"))
}

#[cfg(windows)]
fn platform(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    env("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "LOCALAPPDATA is not set, so the data folder is not known".to_string())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform(env: &dyn Fn(&str) -> Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    if let Some(xdg) = env("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    {
        return Ok(xdg);
    }
    let home = env("HOME")
        .filter(|v| !v.is_empty())
        .ok_or("HOME is not set, so the data folder is not known")?;
    Ok(PathBuf::from(home).join(".local").join("share"))
}

/// The data folder's `logs`, created if missing: the interface's crash log
/// goes there.
pub fn logs_folder() -> Result<PathBuf, String> {
    let logs = data_folder()?.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| format!("{}: {e}", logs.display()))?;
    Ok(logs)
}

/// Where the pinned programs are installed (the provider CLIs): the data
/// folder's `tools`, or the folder `MCBENCH_TOOLS_DIR` names.
pub fn tools_folder(data_folder: &std::path::Path) -> Result<PathBuf, String> {
    match std::env::var_os("MCBENCH_TOOLS_DIR").filter(|v| !v.is_empty()) {
        Some(dir) => {
            std::path::absolute(PathBuf::from(dir)).map_err(|e| format!("MCBENCH_TOOLS_DIR: {e}"))
        }
        None => Ok(data_folder.join("tools")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::ffi::OsString;

    fn with(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<OsString> {
        let map: HashMap<String, OsString> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), OsString::from(v)))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn mcbench_data_dir_names_the_data_folder() {
        let root = std::env::temp_dir().join("named-data");
        assert_eq!(
            data_folder_of(&with(&[
                ("MCBENCH_DATA_DIR", root.to_str().unwrap()),
                ("HOME", "/home/Ada")
            ]))
            .unwrap(),
            root
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_mac_keeps_it_in_application_support() {
        assert_eq!(
            data_folder_of(&with(&[("HOME", "/Users/Ada")])).unwrap(),
            PathBuf::from("/Users/Ada/Library/Application Support/mcbench-lite")
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn linux_keeps_it_in_xdg_data_home_or_local_share() {
        assert_eq!(
            data_folder_of(&with(&[
                ("XDG_DATA_HOME", "/x/data"),
                ("HOME", "/home/Ada")
            ]))
            .unwrap(),
            PathBuf::from("/x/data/mcbench-lite")
        );
        assert_eq!(
            data_folder_of(&with(&[
                ("XDG_DATA_HOME", "relative"),
                ("HOME", "/home/Ada")
            ]))
            .unwrap(),
            PathBuf::from("/home/Ada/.local/share/mcbench-lite")
        );
        assert_eq!(
            data_folder_of(&with(&[("HOME", "/home/Ada")])).unwrap(),
            PathBuf::from("/home/Ada/.local/share/mcbench-lite")
        );
        assert!(data_folder_of(&with(&[])).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn windows_keeps_it_in_local_app_data() {
        assert_eq!(
            data_folder_of(&with(&[("LOCALAPPDATA", r"C:\Users\a\AppData\Local")])).unwrap(),
            PathBuf::from(r"C:\Users\a\AppData\Local\mcbench-lite")
        );
    }
}
