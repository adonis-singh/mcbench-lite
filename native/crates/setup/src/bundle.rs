//! The shipped data's bundle and its lock file
//! (`native/tools/extract/README.md`, The bundle): a POSIX ustar archive of
//! the files [`FILES`] names, in that order, every header's mode 0644,
//! owner 0, time 0, compressed by the `zstd` program at level 19 with its
//! frame checksum; `native/minecraft-data.lock` records the Minecraft
//! version, the bundle's sha256 and size, each file's sha256 and size, and
//! where the build fetches the bundle from.

use std::io::Read;

use sha2::Digest;

use crate::jar::hex;

/// The files of the shipped data, in the bundle's order: the engine's
/// three tables, the block table, the render layers, the tints, the
/// mesher's pack, the entity capture, BlueMap's two settings files, the
/// block registry build programs are checked against (Minecraft's data
/// generator's blocks report written as today's `blocks.json`,
/// [`crate::registry`]), the client-jar manifest and Minecraft's night
/// (`server/McbenchNight.java`).
pub const FILES: [&str; 13] = [
    "blocks.txt",
    "loot.json",
    "datafix.txt",
    "block-table.tsv",
    "render-layers.tsv",
    "tints.tsv",
    "mcbench-pack.bin",
    "mcbench-entities.json",
    "settings.json",
    "map-settings.json",
    "blocks.json",
    "client-jar.json",
    "night.json",
];

/// The lock file of this checkout (`native/minecraft-data.lock`).
pub const LOCK: &str = include_str!("../../../minecraft-data.lock");

/// What a lock file records.
#[derive(Clone, Debug, PartialEq)]
pub struct Lock {
    /// The Minecraft version.
    pub minecraft: String,
    /// The bundle's sha256.
    pub sha256: String,
    /// The bundle's size in bytes.
    pub size: u64,
    /// Where the build fetches the bundle from.
    pub url: String,
    /// The `zstd` program's version the bundle was compressed with.
    pub zstd: String,
    /// Each file of [`FILES`]: its name, sha256 and size.
    pub files: Vec<(String, String, u64)>,
}

impl Lock {
    /// Reads a lock file's text.
    pub fn parse(text: &str) -> Result<Lock, String> {
        let mut minecraft = None;
        let mut bundle = None;
        let mut url = None;
        let mut zstd = None;
        let mut files = Vec::new();
        for line in text
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let fields: Vec<&str> = line.split('\t').collect();
            let size = |field: &str| {
                field
                    .parse::<u64>()
                    .map_err(|_| format!("The lock file's line {line:?} has no size"))
            };
            match fields[..] {
                ["minecraft", version] => minecraft = Some(version.to_string()),
                ["bundle", sha256, bytes] => bundle = Some((sha256.to_string(), size(bytes)?)),
                ["url", address] => url = Some(address.to_string()),
                ["zstd", version] => zstd = Some(version.to_string()),
                ["file", name, sha256, bytes] => {
                    files.push((name.to_string(), sha256.to_string(), size(bytes)?))
                }
                _ => return Err(format!("The lock file has a line it cannot read: {line:?}")),
            }
        }
        let (sha256, size) = bundle.ok_or("The lock file names no bundle")?;
        let names: Vec<&str> = files.iter().map(|(name, _, _)| name.as_str()).collect();
        if names != FILES {
            return Err(format!(
                "The lock file names the files {names:?}, not {FILES:?}"
            ));
        }
        Ok(Lock {
            minecraft: minecraft.ok_or("The lock file names no Minecraft version")?,
            sha256,
            size,
            url: url.ok_or("The lock file names no address")?,
            zstd: zstd.ok_or("The lock file names no zstd version")?,
            files,
        })
    }

    /// The lock file's text for these values.
    pub fn text(&self) -> String {
        let mut out = String::from(
            "# The Minecraft data the native app ships (native/tools/extract/README.md),\n\
             # written by native/tools/extract/bundle.sh from one run of the extractor and\n\
             # never edited by hand. mcbench-setup's build embeds the bundle that\n\
             # MCBENCH_DATA_BUNDLE names once its sha256 and size are these.\n",
        );
        out.push_str(&format!("minecraft\t{}\n", self.minecraft));
        out.push_str(&format!("bundle\t{}\t{}\n", self.sha256, self.size));
        out.push_str(&format!("url\t{}\n", self.url));
        out.push_str(&format!("zstd\t{}\n", self.zstd));
        for (name, sha256, size) in &self.files {
            out.push_str(&format!("file\t{name}\t{sha256}\t{size}\n"));
        }
        out
    }
}

/// The sha256 of `bytes` in hexadecimal.
pub fn sha256(bytes: &[u8]) -> String {
    hex(&sha2::Sha256::digest(bytes))
}

/// The ustar archive of `files` (name, bytes), in order.
pub fn tar(files: &[(&str, &[u8])]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for (name, bytes) in files {
        if name.len() > 100 || !name.is_ascii() {
            return Err(format!("{name} is not a short ASCII name"));
        }
        let mut header = [0u8; 512];
        header[..name.len()].copy_from_slice(name.as_bytes());
        let octal = |field: &mut [u8], value: u64| {
            let text = format!("{value:0width$o}", width = field.len() - 1);
            field[..text.len()].copy_from_slice(text.as_bytes());
        };
        octal(&mut header[100..108], 0o644);
        octal(&mut header[108..116], 0);
        octal(&mut header[116..124], 0);
        octal(&mut header[124..136], bytes.len() as u64);
        octal(&mut header[136..148], 0);
        header[156] = b'0';
        header[257..263].copy_from_slice(b"ustar\0");
        header[263..265].copy_from_slice(b"00");
        header[148..156].copy_from_slice(b"        ");
        let sum: u32 = header.iter().map(|&b| b as u32).sum();
        let text = format!("{sum:06o}\0 ");
        header[148..156].copy_from_slice(text.as_bytes());
        out.extend_from_slice(&header);
        out.extend_from_slice(bytes);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.resize(out.len() + 1024, 0);
    Ok(out)
}

/// The files of a ustar archive (name, bytes), in order.
pub fn untar(archive: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut files = Vec::new();
    let mut at = 0;
    while at + 512 <= archive.len() {
        let header = &archive[at..at + 512];
        if header.iter().all(|&b| b == 0) {
            return Ok(files);
        }
        if &header[257..263] != b"ustar\0" {
            return Err(format!("The archive has no ustar header at {at}"));
        }
        let field = |range: std::ops::Range<usize>| -> Result<u64, String> {
            let text = std::str::from_utf8(&header[range])
                .map_err(|_| format!("The archive's header at {at} is not text"))?;
            u64::from_str_radix(text.trim_matches(|c| c == '\0' || c == ' '), 8)
                .map_err(|_| format!("The archive's header at {at} has a field {text:?}"))
        };
        let mut blank = [0u8; 512];
        blank.copy_from_slice(header);
        blank[148..156].copy_from_slice(b"        ");
        if field(148..156)? != blank.iter().map(|&b| b as u64).sum::<u64>() {
            return Err(format!("The archive's header at {at} has a wrong checksum"));
        }
        if header[156] != b'0' {
            return Err(format!("The archive's entry at {at} is not a file"));
        }
        let end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
        let name = String::from_utf8(header[..end].to_vec())
            .map_err(|_| format!("The archive's name at {at} is not UTF-8"))?;
        let size = field(124..136)? as usize;
        let start = at + 512;
        if start + size > archive.len() {
            return Err(format!("The archive ends inside {name}"));
        }
        files.push((name, archive[start..start + size].to_vec()));
        at = start + size.div_ceil(512) * 512;
    }
    Err("The archive has no end".into())
}

/// The bundle's archive: the zstd frame decompressed by a pure-Rust
/// decoder, which checks the frame's checksum.
pub fn decompress(bundle: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(bundle)
        .map_err(|e| format!("The bundle is not a zstd frame: {e}"))?;
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| format!("The bundle does not decompress: {e}"))?;
    Ok(out)
}

/// The bundle's files, which must be the ones `lock` names: its sha256 and
/// size checked, then decompressed and read in [`FILES`]' order.
pub fn open(bundle: &[u8], lock: &Lock) -> Result<Vec<(String, Vec<u8>)>, String> {
    let got = sha256(bundle);
    if got != lock.sha256 || bundle.len() as u64 != lock.size {
        return Err(format!(
            "The Minecraft data bundle has sha256 {got} and {} bytes, but the lock file names {} and {} bytes",
            bundle.len(),
            lock.sha256,
            lock.size
        ));
    }
    let files = untar(&decompress(bundle)?)?;
    let names: Vec<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    if names != FILES {
        return Err(format!(
            "The Minecraft data bundle holds {names:?}, not {FILES:?}"
        ));
    }
    for ((name, bytes), (_, _, size)) in files.iter().zip(&lock.files) {
        if bytes.len() as u64 != *size {
            return Err(format!(
                "{name} in the bundle is {} bytes, but the lock file names {size}",
                bytes.len()
            ));
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_archive_reads_back_as_its_files_in_order() {
        let long = vec![7u8; 1500];
        let archive = tar(&[
            ("blocks.txt", b"block\tminecraft:stone"),
            ("pack.bin", &long),
            ("empty", b""),
        ])
        .unwrap();
        assert_eq!(archive.len() % 512, 0);
        let files = untar(&archive).unwrap();
        assert_eq!(
            files.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["blocks.txt", "pack.bin", "empty"]
        );
        assert_eq!(files[1].1, long);
        assert!(files[2].1.is_empty());
        let mut broken = archive.clone();
        broken[5] ^= 1;
        assert!(untar(&broken).unwrap_err().contains("wrong checksum"));
    }

    #[test]
    fn the_lock_file_reads_back_as_written_and_names_every_file() {
        let lock = Lock {
            minecraft: "1.21.10".into(),
            sha256: "a".repeat(64),
            size: 12,
            url: "https://example.invalid/bundle".into(),
            zstd: "1.5.7".into(),
            files: FILES
                .iter()
                .map(|n| (n.to_string(), "b".repeat(64), 3))
                .collect(),
        };
        assert_eq!(Lock::parse(&lock.text()).unwrap(), lock);
        let short = lock
            .text()
            .replace("file\tclient-jar.json", "file\tother.json");
        assert!(
            Lock::parse(&short)
                .unwrap_err()
                .starts_with("The lock file names the files")
        );
    }

    #[test]
    fn a_bundle_that_is_not_the_locks_is_refused() {
        let lock = Lock {
            minecraft: "1.21.10".into(),
            sha256: "0".repeat(64),
            size: 3,
            url: String::new(),
            zstd: String::new(),
            files: Vec::new(),
        };
        assert_eq!(
            open(b"abc", &lock).unwrap_err(),
            format!(
                "The Minecraft data bundle has sha256 {} and 3 bytes, but the lock file names {} and 3 bytes",
                sha256(b"abc"),
                "0".repeat(64)
            )
        );
    }
}
