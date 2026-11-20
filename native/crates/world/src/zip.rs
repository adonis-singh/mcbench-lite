//! Files of a zip (or jar), read through its central directory
//! (`server/zip-entry.js`): the names of its entries, one file's bytes
//! without unpacking the rest, the bytes of many, or every file unpacked
//! into a folder with its Unix mode and symbolic links.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use indexmap::IndexMap;

const S_IFMT: u32 = 0o170000;
const S_IFLNK: u32 = 0o120000;

/// One entry of the central directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZipEntry {
    /// Its name.
    pub name: String,
    /// Its compression method (0 stored, 8 deflated).
    pub method: u16,
    /// Its compressed size.
    pub compressed: u32,
    /// Its Unix mode, 0 when the archive was not made on Unix.
    pub mode: u32,
    /// Where its local header is.
    pub header: u32,
}

fn le16(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "The zip's directory ends early".to_string())
}

fn le32(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "The zip's directory ends early".to_string())
}

fn read_at(file: &mut File, at: u64, length: usize, name: &Path) -> Result<Vec<u8>, String> {
    let mut out = vec![0; length];
    file.seek(SeekFrom::Start(at))
        .map_err(|e| format!("Cannot read {}: {e}", name.display()))?;
    let mut got = 0;
    while got < length {
        let n = file
            .read(&mut out[got..])
            .map_err(|e| format!("Cannot read {}: {e}", name.display()))?;
        if n == 0 {
            break;
        }
        got += n;
    }
    Ok(out)
}

/// The archive's entries from its central directory (`entries`).
fn entries(file: &mut File, path: &Path) -> Result<Vec<ZipEntry>, String> {
    let size = file
        .metadata()
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?
        .len();
    let tail_length = size.min(65_557);
    let tail = read_at(file, size - tail_length, tail_length as usize, path)?;
    let end = tail
        .windows(4)
        .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| format!("{} is not a zip archive", path.display()))?;
    let count = le16(&tail, end + 10)?;
    let directory_size = le32(&tail, end + 12)?;
    let directory = read_at(
        file,
        le32(&tail, end + 16)? as u64,
        directory_size as usize,
        path,
    )?;
    let mut list = Vec::with_capacity(count as usize);
    let mut at = 0;
    for _ in 0..count {
        let name_length = le16(&directory, at + 28)? as usize;
        let name = directory
            .get(at + 46..at + 46 + name_length)
            .ok_or("The zip's directory ends early")?;
        list.push(ZipEntry {
            name: String::from_utf8_lossy(name).into_owned(),
            method: le16(&directory, at + 10)?,
            compressed: le32(&directory, at + 20)?,
            mode: if directory.get(at + 5) == Some(&3) {
                le32(&directory, at + 38)? >> 16
            } else {
                0
            },
            header: le32(&directory, at + 42)?,
        });
        at += 46
            + name_length
            + le16(&directory, at + 30)? as usize
            + le16(&directory, at + 32)? as usize;
    }
    Ok(list)
}

fn data_start(file: &mut File, entry: &ZipEntry, path: &Path) -> Result<u64, String> {
    let local = read_at(file, entry.header as u64, 30, path)?;
    Ok(entry.header as u64 + 30 + le16(&local, 26)? as u64 + le16(&local, 28)? as u64)
}

fn read_data(file: &mut File, path: &Path, entry: &ZipEntry) -> Result<Vec<u8>, String> {
    let start = data_start(file, entry, path)?;
    let data = read_at(file, start, entry.compressed as usize, path)?;
    match entry.method {
        0 => Ok(data),
        8 => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(&data[..])
                .read_to_end(&mut out)
                .map_err(|e| {
                    format!("{} in {} does not inflate: {e}", entry.name, path.display())
                })?;
            Ok(out)
        }
        method => Err(format!(
            "{} in {} uses unsupported compression {method}",
            entry.name,
            path.display()
        )),
    }
}

fn open(path: &Path) -> Result<File, String> {
    File::open(path).map_err(|e| format!("Cannot open {}: {e}", path.display()))
}

/// The names of the archive's entries in the central directory's order
/// (`zipNames`).
pub fn zip_names(path: &Path) -> Result<Vec<String>, String> {
    Ok(entries(&mut open(path)?, path)?
        .into_iter()
        .map(|e| e.name)
        .collect())
}

/// The entries of the archive (`entries`).
pub fn zip_entries(path: &Path) -> Result<Vec<ZipEntry>, String> {
    entries(&mut open(path)?, path)
}

/// One entry's bytes, unpacked (`readZipEntry`).
pub fn read_zip_entry(path: &Path, name: &str) -> Result<Vec<u8>, String> {
    let mut file = open(path)?;
    let list = entries(&mut file, path)?;
    let entry = list
        .iter()
        .find(|e| e.name == name)
        .ok_or_else(|| format!("{} has no {name}", path.display()))?;
    read_data(&mut file, path, entry)
}

/// The bytes of every entry `wanted` accepts, by name in the central
/// directory's order (`readZipEntries`).
pub fn read_zip_entries(
    path: &Path,
    mut wanted: impl FnMut(&str) -> bool,
) -> Result<IndexMap<String, Vec<u8>>, String> {
    let mut file = open(path)?;
    let mut out = IndexMap::new();
    for entry in entries(&mut file, path)? {
        if wanted(&entry.name) {
            let data = read_data(&mut file, path, &entry)?;
            out.insert(entry.name, data);
        }
    }
    Ok(out)
}

/// Unpacks every entry into `folder`, keeping each file's mode and making
/// its symbolic links (`extractZip`).
pub fn extract_zip(path: &Path, folder: &Path) -> Result<(), String> {
    let mut file = open(path)?;
    for entry in entries(&mut file, path)? {
        let target = folder.join(&entry.name);
        let make = |dir: &Path| {
            std::fs::create_dir_all(dir).map_err(|e| format!("Cannot make {}: {e}", dir.display()))
        };
        if entry.name.ends_with('/') {
            make(&target)?;
            continue;
        }
        make(target.parent().expect("an entry in a folder"))?;
        if entry.method != 0 && entry.method != 8 {
            return Err(format!(
                "{} in {} uses unsupported compression {}",
                entry.name,
                path.display(),
                entry.method
            ));
        }
        let bytes = if entry.compressed == 0 {
            Vec::new()
        } else {
            read_data(&mut file, path, &entry)?
        };
        if entry.mode & S_IFMT == S_IFLNK {
            let link = String::from_utf8_lossy(&bytes).into_owned();
            symlink(&link, &target)?;
            continue;
        }
        std::fs::write(&target, &bytes)
            .map_err(|e| format!("Cannot write {}: {e}", target.display()))?;
        if entry.mode & 0o777 != 0 {
            set_mode(&target, entry.mode & 0o777)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink(link: &str, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(link, target)
        .map_err(|e| format!("Cannot link {}: {e}", target.display()))
}

#[cfg(windows)]
fn symlink(link: &str, target: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_file(link, target)
        .map_err(|e| format!("Cannot link {}: {e}", target.display()))
}

#[cfg(unix)]
fn set_mode(target: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(target, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("Cannot set the mode of {}: {e}", target.display()))
}

#[cfg(windows)]
fn set_mode(target: &Path, mode: u32) -> Result<(), String> {
    // Node's chmod on Windows sets or clears only the read-only flag, from
    // the owner's write bit.
    let mut permissions = std::fs::metadata(target)
        .map_err(|e| format!("Cannot read {}: {e}", target.display()))?
        .permissions();
    permissions.set_readonly(mode & 0o200 == 0);
    std::fs::set_permissions(target, permissions)
        .map_err(|e| format!("Cannot set the mode of {}: {e}", target.display()))
}
