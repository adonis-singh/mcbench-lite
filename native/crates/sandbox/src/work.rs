//! The host-side file bridge to a chat's `/work` (`sandbox.js` `workDir`,
//! `resolveWorkPath`, `readWorkFile`, `writeWorkFile`, `copyIntoWork`,
//! `workFileExists`, `makeReadFile`, `syncImages`, `syncAttachments`,
//! `workError`).
//!
//! `/work` is writable from inside the box, so the model controls every
//! symlink under it. Host-side access resolves real paths, refuses anything
//! outside the real work directory, and checks the opened inode against the
//! resolved path so a link swapped after resolution cannot redirect access.
//!
//! Every error is the message a model reads. A file error never names the
//! host path (which holds the data folder and so the user's name): it is
//! told by the `/work` path the model gave and what its code means, in
//! libuv's words as Node tells them (`contract/uv-errors.json`), for example
//! `/work/f.txt/g: not a directory (ENOTDIR)`.
//!
//! On Windows the chats' folders stay on Windows' own disk (the WSL2
//! distro reaches them by drvfs), so this bridge works on them there: a
//! file is opened without following its last part when that is a reparse
//! point (`FILE_FLAG_OPEN_REPARSE_POINT`; Windows' symbolic links and
//! junctions, and WSL's own Linux symbolic links, which Windows cannot
//! follow at all), the opened file is compared with the resolved path by
//! volume and file index, and an OS error is told by the name libuv gives
//! its Windows code (`uv_translate_sys_error`), as Node on Windows does.

use std::path::{Component, Path, PathBuf};

use crate::Workspace;

/// `mc.readFile`'s limit (`makeReadFile`).
pub const READ_FILE_MAX_BYTES: u64 = 8 * 1024 * 1024;

/// A failure inside the bridge: an OS error (told by [`work_error`]) or one
/// of the bridge's own messages.
#[derive(Debug)]
enum Failure {
    Os(std::io::Error),
    Message(String),
}

impl From<std::io::Error> for Failure {
    fn from(error: std::io::Error) -> Self {
        Failure::Os(error)
    }
}

type Result<T> = std::result::Result<T, Failure>;

/// `workPath`: the `/work` path of what the model wrote.
pub fn work_path(rel: &str) -> String {
    format!("/work/{}", strip_work(rel))
}

/// `String(relPath).replace(/^\/?work(?:\/|$)/, '')`.
fn strip_work(rel: &str) -> &str {
    let rest = rel.strip_prefix('/').unwrap_or(rel);
    match rest.strip_prefix("work") {
        Some("") => "",
        Some(after) if after.starts_with('/') => &after[1..],
        _ => rel,
    }
}

/// The name and libuv message of an OS error, as Node's `error.code` and
/// `util.getSystemErrorMessage(error.errno)` give them.
fn uv_error(error: &std::io::Error) -> Option<(&'static str, String)> {
    static UV: std::sync::LazyLock<serde_json::Map<String, serde_json::Value>> =
        std::sync::LazyLock::new(|| {
            serde_json::from_str(include_str!("../contract/uv-errors.json"))
                .expect("contract/uv-errors.json is JSON")
        });
    let name = errno_name(error.raw_os_error()?)?;
    let message = UV.get(name)?.as_str()?.to_string();
    Some((name, message))
}

#[cfg(unix)]
fn errno_name(errno: i32) -> Option<&'static str> {
    // The errno names of the platform, by value; libuv names them alike.
    const NAMES: &[(i32, &str)] = &[
        (libc::E2BIG, "E2BIG"),
        (libc::EACCES, "EACCES"),
        (libc::EAGAIN, "EAGAIN"),
        (libc::EBADF, "EBADF"),
        (libc::EBUSY, "EBUSY"),
        (libc::EDQUOT, "EDQUOT"),
        (libc::EEXIST, "EEXIST"),
        (libc::EFAULT, "EFAULT"),
        (libc::EFBIG, "EFBIG"),
        (libc::EINTR, "EINTR"),
        (libc::EINVAL, "EINVAL"),
        (libc::EIO, "EIO"),
        (libc::EISDIR, "EISDIR"),
        (libc::ELOOP, "ELOOP"),
        (libc::EMFILE, "EMFILE"),
        (libc::EMLINK, "EMLINK"),
        (libc::ENAMETOOLONG, "ENAMETOOLONG"),
        (libc::ENFILE, "ENFILE"),
        (libc::ENODEV, "ENODEV"),
        (libc::ENOENT, "ENOENT"),
        (libc::ENOMEM, "ENOMEM"),
        (libc::ENOSPC, "ENOSPC"),
        (libc::ENOSYS, "ENOSYS"),
        (libc::ENOTDIR, "ENOTDIR"),
        (libc::ENOTEMPTY, "ENOTEMPTY"),
        (libc::ENOTSUP, "ENOTSUP"),
        (libc::ENXIO, "ENXIO"),
        (libc::EPERM, "EPERM"),
        (libc::EROFS, "EROFS"),
        (libc::ESPIPE, "ESPIPE"),
        (libc::ESTALE, "ESTALE"),
        (libc::ETIMEDOUT, "ETIMEDOUT"),
        (libc::ETXTBSY, "ETXTBSY"),
        (libc::EXDEV, "EXDEV"),
    ];
    NAMES
        .iter()
        .find(|(value, _)| *value == errno)
        .map(|(_, name)| *name)
}

#[cfg(windows)]
fn errno_name(code: i32) -> Option<&'static str> {
    use windows_sys::Win32::Foundation::*;
    // libuv's uv_translate_sys_error (src/win/error.c), for the file
    // system's codes.
    const NAMES: &[(u32, &str)] = &[
        (ERROR_ELEVATION_REQUIRED, "EACCES"),
        (ERROR_CANT_ACCESS_FILE, "EACCES"),
        (ERROR_NO_DATA, "EAGAIN"),
        (ERROR_INVALID_FLAGS, "EBADF"),
        (ERROR_INVALID_HANDLE, "EBADF"),
        (ERROR_LOCK_VIOLATION, "EBUSY"),
        (ERROR_PIPE_BUSY, "EBUSY"),
        (ERROR_SHARING_VIOLATION, "EBUSY"),
        (ERROR_ALREADY_EXISTS, "EEXIST"),
        (ERROR_FILE_EXISTS, "EEXIST"),
        (ERROR_NOACCESS, "EFAULT"),
        (ERROR_INVALID_PARAMETER, "EINVAL"),
        (ERROR_SYMLINK_NOT_SUPPORTED, "EINVAL"),
        (ERROR_OPEN_FAILED, "EIO"),
        (ERROR_CANT_RESOLVE_FILENAME, "ELOOP"),
        (ERROR_TOO_MANY_OPEN_FILES, "EMFILE"),
        (ERROR_BUFFER_OVERFLOW, "ENAMETOOLONG"),
        (ERROR_FILENAME_EXCED_RANGE, "ENAMETOOLONG"),
        (ERROR_BAD_PATHNAME, "ENOENT"),
        (ERROR_DIRECTORY, "ENOENT"),
        (ERROR_FILE_NOT_FOUND, "ENOENT"),
        (ERROR_INVALID_NAME, "ENOENT"),
        (ERROR_INVALID_DRIVE, "ENOENT"),
        (ERROR_INVALID_REPARSE_DATA, "ENOENT"),
        (ERROR_PATH_NOT_FOUND, "ENOENT"),
        (ERROR_NOT_ENOUGH_MEMORY, "ENOMEM"),
        (ERROR_OUTOFMEMORY, "ENOMEM"),
        (ERROR_CANNOT_MAKE, "ENOSPC"),
        (ERROR_DISK_FULL, "ENOSPC"),
        (ERROR_EA_TABLE_FULL, "ENOSPC"),
        (ERROR_END_OF_MEDIA, "ENOSPC"),
        (ERROR_HANDLE_DISK_FULL, "ENOSPC"),
        (ERROR_DIR_NOT_EMPTY, "ENOTEMPTY"),
        (ERROR_NOT_SUPPORTED, "ENOTSUP"),
        (ERROR_BROKEN_PIPE, "EOF"),
        (ERROR_ACCESS_DENIED, "EPERM"),
        (ERROR_PRIVILEGE_NOT_HELD, "EPERM"),
        (ERROR_WRITE_PROTECT, "EROFS"),
        (ERROR_SEM_TIMEOUT, "ETIMEDOUT"),
        (ERROR_NOT_SAME_DEVICE, "EXDEV"),
        (ERROR_INVALID_FUNCTION, "EISDIR"),
    ];
    NAMES
        .iter()
        .find(|(value, _)| *value as i32 == code)
        .map(|(_, name)| *name)
}

/// `workError(error, relPath)`: an OS error told by its `/work` path; any
/// other message as it is.
fn tell(failure: Failure, rel: &str) -> String {
    match failure {
        Failure::Message(message) => message,
        Failure::Os(error) => match uv_error(&error) {
            Some((name, message)) => format!("{}: {message} ({name})", work_path(rel)),
            None => panic!("an OS error with no libuv name reached the /work bridge: {error}"),
        },
    }
}

/// `workError(error, relPath)` of an OS error met outside this module (a
/// listing of `/work/verification` by `test_build`'s report pruning).
pub fn work_error(error: std::io::Error, rel: &str) -> String {
    tell(Failure::Os(error), rel)
}

/// `inWork`: runs a bridge function and tells its errors.
fn in_work<T>(rel: &str, run: impl FnOnce() -> Result<T>) -> std::result::Result<T, String> {
    run().map_err(|failure| tell(failure, rel))
}

/// `workDir`: the chat's `/work` with its `images/` and `attachments/`
/// folders, made if missing (the model can put a file where one should be).
pub fn work_dir(workspace: &Workspace) -> std::result::Result<PathBuf, String> {
    for folder in ["images", "attachments"] {
        in_work(folder, || {
            Ok(std::fs::create_dir_all(workspace.host.join(folder))?)
        })?;
    }
    Ok(workspace.host.clone())
}

fn escape(rel: &str) -> Failure {
    Failure::Message(format!("Path escapes /work: {rel}"))
}

fn inside(root: &Path, file: &Path) -> bool {
    file.starts_with(root)
}

/// `path.resolve(root, rel)`: `rel` joined and `.` and `..` folded
/// lexically; an absolute `rel` replaces the root. A Windows root keeps its
/// prefix (`\\?\C:`), which `..` does not pop.
fn resolve(root: &Path, rel: &str) -> PathBuf {
    let mut out = PathBuf::from("/");
    let joined = if rel.starts_with('/') {
        PathBuf::from(rel)
    } else {
        root.join(rel)
    };
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(name) => out.push(name),
            Component::Prefix(prefix) => out = PathBuf::from(prefix.as_os_str()),
            Component::RootDir => out.push(part.as_os_str()),
            Component::CurDir => {}
        }
    }
    out
}

fn lexists(file: &Path) -> Result<bool> {
    match std::fs::symlink_metadata(file) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn resolve_inner(workspace: &Workspace, rel: &str, write: bool) -> Result<PathBuf> {
    let root = std::fs::canonicalize(work_dir(workspace).map_err(Failure::Message)?)?;
    let file = resolve(&root, strip_work(rel));
    if !inside(&root, &file) {
        return Err(escape(rel));
    }
    let mut existing = file.clone();
    while !lexists(&existing)? {
        existing.pop();
    }
    if write && existing == file && std::fs::symlink_metadata(&file)?.file_type().is_symlink() {
        return Err(Failure::Message(format!(
            "Refusing to write through a symlink in /work: {rel}"
        )));
    }
    let real = match std::fs::canonicalize(&existing) {
        Ok(real) => real,
        Err(error) if is(&error, Errno::NoEnt) || is(&error, Errno::Loop) => {
            return Err(escape(rel));
        }
        Err(error) => return Err(error.into()),
    };
    if !inside(&root, &real) {
        return Err(escape(rel));
    }
    let rest = file
        .strip_prefix(&existing)
        .expect("existing is an ancestor of file");
    Ok(if rest.as_os_str().is_empty() {
        real
    } else {
        real.join(rest)
    })
}

/// `resolveWorkPath(conversationId, relPath, { write })`: the host path of a
/// `/work` path, refused when it or a link in it leads out of `/work`, and
/// for a write, when it is itself a link.
pub fn resolve_work_path(
    workspace: &Workspace,
    rel: &str,
    write: bool,
) -> std::result::Result<PathBuf, String> {
    in_work(rel, || resolve_inner(workspace, rel, write))
}

enum Errno {
    NoEnt,
    Loop,
    Exist,
}

#[cfg(unix)]
fn is(error: &std::io::Error, errno: Errno) -> bool {
    error.raw_os_error()
        == Some(match errno {
            Errno::NoEnt => libc::ENOENT,
            Errno::Loop => libc::ELOOP,
            Errno::Exist => libc::EEXIST,
        })
}

/// As libuv names the code; a path through one of WSL's Linux links, which
/// Windows cannot follow (`ERROR_CANT_ACCESS_FILE`), is a loop: the link
/// leads nowhere the host may go.
#[cfg(windows)]
fn is(error: &std::io::Error, errno: Errno) -> bool {
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    let name = errno_name(code);
    match errno {
        Errno::NoEnt => name == Some("ENOENT"),
        Errno::Loop => {
            name == Some("ELOOP")
                || code == windows_sys::Win32::Foundation::ERROR_CANT_ACCESS_FILE as i32
        }
        Errno::Exist => name == Some("EEXIST"),
    }
}

#[cfg(unix)]
fn open(file: &Path, flags: i32, mode: u32) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    #[cfg(test)]
    let file = &tests_hook::redirected(file);
    let access = flags & libc::O_ACCMODE;
    std::fs::OpenOptions::new()
        .read(access == libc::O_RDONLY)
        .write(access == libc::O_WRONLY)
        .create(flags & libc::O_CREAT != 0)
        .create_new(flags & libc::O_EXCL != 0)
        .custom_flags(flags & (libc::O_NOFOLLOW))
        .mode(mode)
        .open(file)
}

/// [`READ_ONLY`], [`CREATE`] or [`WRITE`], not following the last part:
/// a reparse point is refused with `ERROR_CANT_RESOLVE_FILENAME` (libuv's
/// `ELOOP`, what `O_NOFOLLOW` gives on a link).
#[cfg(windows)]
fn open(file: &Path, flags: i32, _: u32) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    };
    let mut options = std::fs::OpenOptions::new();
    match flags {
        READ_ONLY => options.read(true),
        CREATE => options.write(true).create_new(true),
        _ => options.write(true),
    };
    let fd = options
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(file)?;
    if fd.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(std::io::Error::from_raw_os_error(
            windows_sys::Win32::Foundation::ERROR_CANT_RESOLVE_FILENAME as i32,
        ));
    }
    Ok(fd)
}

/// `checkedDescriptor`: the opened file is the one the path names, inside
/// `/work`.
#[cfg(unix)]
fn checked(workspace: &Workspace, rel: &str, fd: &std::fs::File, file: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let opened = std::fs::canonicalize(file)?;
    if !inside(&std::fs::canonicalize(&workspace.host)?, &opened) {
        return Err(escape(rel));
    }
    let expected = std::fs::metadata(&opened)?;
    let actual = fd.metadata()?;
    if expected.dev() != actual.dev() || expected.ino() != actual.ino() {
        return Err(escape(rel));
    }
    Ok(())
}

#[cfg(windows)]
fn checked(workspace: &Workspace, rel: &str, fd: &std::fs::File, file: &Path) -> Result<()> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, GetFileInformationByHandle,
    };
    let opened = std::fs::canonicalize(file)?;
    if !inside(&std::fs::canonicalize(&workspace.host)?, &opened) {
        return Err(escape(rel));
    }
    let identity = |handle: &std::fs::File| -> std::io::Result<(u32, u32, u32)> {
        // SAFETY: an all-zero BY_HANDLE_FILE_INFORMATION is valid, and the
        // call fills it from a handle this function holds open.
        unsafe {
            let mut info: BY_HANDLE_FILE_INFORMATION = std::mem::zeroed();
            if GetFileInformationByHandle(handle.as_raw_handle() as _, &mut info) == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok((
                info.dwVolumeSerialNumber,
                info.nFileIndexHigh,
                info.nFileIndexLow,
            ))
        }
    };
    let expected = std::fs::OpenOptions::new()
        .access_mode(0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(&opened)?;
    if identity(&expected)? != identity(fd)? {
        return Err(escape(rel));
    }
    Ok(())
}

#[cfg(unix)]
const READ_ONLY: i32 = libc::O_RDONLY | libc::O_NOFOLLOW;
#[cfg(unix)]
const CREATE: i32 = libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW;
#[cfg(unix)]
const WRITE: i32 = libc::O_WRONLY | libc::O_NOFOLLOW;
#[cfg(windows)]
const READ_ONLY: i32 = 0;
#[cfg(windows)]
const CREATE: i32 = 1;
#[cfg(windows)]
const WRITE: i32 = 2;

/// `workFileExists`.
pub fn work_file_exists(workspace: &Workspace, rel: &str) -> std::result::Result<bool, String> {
    in_work(rel, || Ok(resolve_inner(workspace, rel, false)?.exists()))
}

/// `readWorkFile(conversationId, relPath, { maxBytes })`: the bytes of a
/// regular file in `/work`.
pub fn read_work_file(
    workspace: &Workspace,
    rel: &str,
    max_bytes: Option<u64>,
) -> std::result::Result<Vec<u8>, String> {
    in_work(rel, || {
        use std::io::Read;
        let file = resolve_inner(workspace, rel, false)?;
        let mut fd = match open(&file, READ_ONLY, 0) {
            Ok(fd) => fd,
            Err(error) if is(&error, Errno::NoEnt) => {
                return Err(Failure::Message(format!("No such file in /work: {rel}")));
            }
            Err(error) if is(&error, Errno::Loop) => return Err(escape(rel)),
            Err(error) => return Err(error.into()),
        };
        checked(workspace, rel, &fd, &file)?;
        let size = fd.metadata()?.len();
        if let Some(max) = max_bytes
            && size > max
        {
            return Err(Failure::Message(format!(
                "{rel} is {size} bytes (max {max})"
            )));
        }
        let mut data = Vec::with_capacity(size as usize);
        fd.read_to_end(&mut data)?;
        Ok(data)
    })
}

/// `makeReadFile(conversationId)(relPath)`: `mc.readFile` of a build
/// program, text of at most 8 MiB.
pub fn read_file_for_build(
    workspace: &Workspace,
    rel: &str,
) -> std::result::Result<String, String> {
    read_work_file(workspace, rel, Some(READ_FILE_MAX_BYTES))
        .map(|data| String::from_utf8_lossy(&data).into_owned())
        .map_err(|message| format!("mc.readFile: {message}"))
}

/// `openWorkFileForWrite`: a regular file in `/work` opened for writing and
/// emptied, or `None` when `replace` is false and it exists.
fn open_for_write(
    workspace: &Workspace,
    rel: &str,
    replace: bool,
) -> Result<Option<std::fs::File>> {
    let host = work_dir(workspace).map_err(Failure::Message)?;
    // path.join(workDir, stripped): joined and folded, never replaced by an
    // absolute path.
    if !replace && lexists(&resolve(&host, strip_work(rel).trim_start_matches('/')))? {
        return Ok(None);
    }
    let file = resolve_inner(workspace, rel, true)?;
    std::fs::create_dir_all(file.parent().expect("a /work file has a parent"))?;
    let fd = match open(&file, CREATE, 0o644) {
        Ok(fd) => fd,
        Err(error) if is(&error, Errno::Exist) => {
            if !replace {
                return Ok(None);
            }
            match open(&file, WRITE, 0) {
                Ok(fd) => fd,
                Err(error) if is(&error, Errno::Loop) => {
                    return Err(Failure::Message(format!(
                        "Refusing to write through a symlink in /work: {rel}"
                    )));
                }
                Err(error) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    };
    checked(workspace, rel, &fd, &file)?;
    if !fd.metadata()?.is_file() {
        return Err(Failure::Message(format!(
            "Not a regular file in /work: {rel}"
        )));
    }
    fd.set_len(0)?;
    Ok(Some(fd))
}

/// `writeWorkFile`: `mc.writeFile` and `mc.dump` of a build program.
pub fn write_work_file(
    workspace: &Workspace,
    rel: &str,
    data: &[u8],
) -> std::result::Result<(), String> {
    in_work(rel, || {
        use std::io::Write;
        let mut fd = open_for_write(workspace, rel, true)?.expect("replace opens");
        fd.write_all(data)?;
        Ok(())
    })
}

/// `copyIntoWork`: a host file copied into `/work` without following
/// model-made links; with `replace` false an existing file is kept.
pub fn copy_into_work(
    workspace: &Workspace,
    rel: &str,
    source: &Path,
    replace: bool,
) -> std::result::Result<(), String> {
    in_work(rel, || {
        let Some(mut fd) = open_for_write(workspace, rel, replace)? else {
            return Ok(());
        };
        let mut input = std::fs::File::open(source)?;
        std::io::copy(&mut input, &mut fd)?;
        Ok(())
    })
}

/// `syncImages`: the chat's stored images (uploads and `see_build` renders,
/// not `view_image`'s `view-` files) mirrored into `/work/images`.
pub fn sync_images(workspace: &Workspace, images_dir: &Path) -> std::result::Result<(), String> {
    sync(workspace, images_dir, "images", |name| {
        !name.starts_with("view-")
    })
}

/// `syncAttachments`: the chat's attachments mirrored into
/// `/work/attachments`.
pub fn sync_attachments(
    workspace: &Workspace,
    attachments_dir: &Path,
) -> std::result::Result<(), String> {
    sync(workspace, attachments_dir, "attachments", |_| true)
}

fn sync(
    workspace: &Workspace,
    store_dir: &Path,
    folder: &str,
    keep: impl Fn(&str) -> bool,
) -> std::result::Result<(), String> {
    let source = store_dir.join(&workspace.chat);
    if !source.exists() {
        return Ok(());
    }
    // Node's readdirSync errors are not told in /work terms: this folder is
    // the store's, not the model's.
    let names = std::fs::read_dir(&source)
        .and_then(|entries| {
            entries
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<std::io::Result<Vec<_>>>()
        })
        .map_err(|error| error.to_string())?;
    for name in names {
        let name = name.to_string_lossy().into_owned();
        if keep(&name) {
            copy_into_work(
                workspace,
                &format!("{folder}/{name}"),
                &source.join(&name),
                false,
            )?;
        }
    }
    Ok(())
}

/// The test's open hook: a path opened as another (a descriptor swapped
/// after resolution).
#[cfg(all(test, unix))]
mod tests_hook {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};

    thread_local! {
        static REDIRECT: RefCell<Option<(PathBuf, PathBuf)>> = const { RefCell::new(None) };
    }

    pub(super) fn redirected(file: &Path) -> PathBuf {
        REDIRECT.with(|cell| match &*cell.borrow() {
            Some((from, to)) if from == file => to.clone(),
            _ => file.to_path_buf(),
        })
    }

    /// Opens `to` wherever `from` is opened, on this thread.
    pub(super) fn redirect(from: PathBuf, to: PathBuf) {
        REDIRECT.with(|cell| *cell.borrow_mut() = Some((from, to)));
    }

    pub(super) fn clear() {
        REDIRECT.with(|cell| *cell.borrow_mut() = None);
    }
}

#[cfg(all(test, unix))]
mod tests;
