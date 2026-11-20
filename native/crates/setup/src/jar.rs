//! Mojang's client jar as the texture fetch knows it: the client-jar
//! manifest the extractor ships (`client-jar.json`: where each entry the
//! app fetches lies in the jar and what its bytes must be), reading a whole
//! jar's central directory (release time), and an entry's bytes checked
//! against the manifest.

use std::io::Read;

use serde::{Deserialize, Serialize};
use sha2::Digest;

/// The client-jar manifest (`client-jar.json` in the shipped data).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The Minecraft version.
    pub minecraft: String,
    /// The jar's address on Mojang's CDN (its version file's
    /// `downloads.client.url`).
    pub url: String,
    /// The jar's size in bytes.
    pub size: u64,
    /// The jar's sha1, as Mojang's version file names it.
    pub sha1: String,
    /// Every entry the app fetches, by name.
    pub entries: Vec<Entry>,
    /// By the pack's material index, its texture (BlueMap's
    /// `textures.json` `resourcePath`).
    pub materials: Vec<String>,
    /// The textures the entity capture's atlas places
    /// (`mcbench-entities.json` `atlas.rects`).
    #[serde(rename = "entityAtlas")]
    pub entity_atlas: Vec<String>,
    /// By block of the block table (`minecraft:stone`), the texture
    /// Minecraft breaks it into particles with (`block/stone`), the icon's
    /// texture (`server/block-icon.js` particleTexture).
    pub particles: Vec<(String, String)>,
}

/// One entry of the jar.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Its name in the jar.
    pub name: String,
    /// Where its local header begins.
    pub offset: u64,
    /// Where its stored bytes begin.
    pub data: u64,
    /// Its stored (compressed) size.
    pub stored: u64,
    /// Its size.
    pub size: u64,
    /// Its compression: 0 stored, 8 deflated.
    pub method: u16,
    /// Its CRC32, as the jar's directory names it.
    pub crc32: u32,
    /// The sha256 of its bytes.
    pub sha256: String,
}

impl Entry {
    /// Its bytes from its stored bytes, which must be the entry the
    /// manifest names: inflated, then its size, CRC32 and sha256 checked.
    pub fn unpack(&self, stored: &[u8]) -> Result<Vec<u8>, String> {
        if stored.len() as u64 != self.stored {
            return Err(format!(
                "{} is {} stored bytes, but the client-jar manifest names {}",
                self.name,
                stored.len(),
                self.stored
            ));
        }
        let bytes = match self.method {
            0 => stored.to_vec(),
            8 => {
                let mut out = Vec::with_capacity(self.size as usize);
                flate2::read::DeflateDecoder::new(stored)
                    .read_to_end(&mut out)
                    .map_err(|e| format!("{} does not inflate: {e}", self.name))?;
                out
            }
            other => return Err(format!("{} has compression method {other}", self.name)),
        };
        if bytes.len() as u64 != self.size || crc32fast::hash(&bytes) != self.crc32 {
            return Err(format!(
                "{} is not the entry the client-jar manifest names (its size or CRC32 differs)",
                self.name
            ));
        }
        if hex(&sha2::Sha256::digest(&bytes)) != self.sha256 {
            return Err(format!(
                "{} is not the entry the client-jar manifest names (its sha256 differs)",
                self.name
            ));
        }
        Ok(bytes)
    }
}

/// Lower-case hexadecimal.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// Every entry of a whole jar in its central directory's order, each one's
/// stored bytes' start read from its local header, its sha256 left empty
/// (release time; the client jar has no zip64 records).
pub fn entries(jar: &[u8]) -> Result<Vec<Entry>, String> {
    if jar.len() < 22 {
        return Err("The jar is shorter than a zip's end record".into());
    }
    let tail = jar.len().saturating_sub(65_557);
    let eocd = (tail..=jar.len() - 22)
        .rev()
        .find(|&i| u32le(jar, i) == 0x0605_4b50)
        .ok_or("The jar has no end of central directory record")?;
    let count = u16le(jar, eocd + 10) as usize;
    let mut at = u32le(jar, eocd + 16) as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32le(jar, at) != 0x0201_4b50 {
            return Err(format!("The jar has no central directory entry at {at}"));
        }
        let method = u16le(jar, at + 10);
        let crc32 = u32le(jar, at + 16);
        let stored = u32le(jar, at + 20) as u64;
        let size = u32le(jar, at + 24) as u64;
        let name_len = u16le(jar, at + 28) as usize;
        let extra = u16le(jar, at + 30) as usize;
        let comment = u16le(jar, at + 32) as usize;
        let offset = u32le(jar, at + 42) as u64;
        let name = String::from_utf8(jar[at + 46..at + 46 + name_len].to_vec())
            .map_err(|_| format!("The jar has an entry name at {at} that is not UTF-8"))?;
        let local = offset as usize;
        if u32le(jar, local) != 0x0403_4b50 {
            return Err(format!("The jar has no local header for {name}"));
        }
        let data =
            (local + 30 + u16le(jar, local + 26) as usize + u16le(jar, local + 28) as usize) as u64;
        out.push(Entry {
            name,
            offset,
            data,
            stored,
            size,
            method,
            crc32,
            sha256: String::new(),
        });
        at += 46 + name_len + extra + comment;
    }
    Ok(out)
}

/// An entry's stored bytes in a whole jar.
pub fn stored<'a>(jar: &'a [u8], entry: &Entry) -> &'a [u8] {
    &jar[entry.data as usize..(entry.data + entry.stored) as usize]
}

/// The jar entry of a texture id (`minecraft:block/stone`, or `block/stone`
/// in Minecraft's namespace): `assets/minecraft/textures/block/stone.png`.
pub fn texture_path(id: &str) -> String {
    let (space, name) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("assets/{space}/textures/{name}.png")
}

/// A zip of `entries` with their stored bytes as the jar holds them (the
/// renderer reads the entity textures from it as from the jar: `Assets`'s
/// `client_jar`), in the given order.
pub fn write_zip(entries: &[(&Entry, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (entry, stored) in entries {
        let offset = out.len() as u32;
        let name = entry.name.as_bytes();
        let header = |out: &mut Vec<u8>, signature: u32| {
            out.extend_from_slice(&signature.to_le_bytes());
        };
        header(&mut out, 0x0403_4b50);
        out.extend_from_slice(&20u16.to_le_bytes()); // version needed
        out.extend_from_slice(&0u16.to_le_bytes()); // flags
        out.extend_from_slice(&entry.method.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes()); // time and date
        out.extend_from_slice(&entry.crc32.to_le_bytes());
        out.extend_from_slice(&(entry.stored as u32).to_le_bytes());
        out.extend_from_slice(&(entry.size as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // extra
        out.extend_from_slice(name);
        out.extend_from_slice(stored);
        header(&mut central, 0x0201_4b50);
        central.extend_from_slice(&20u16.to_le_bytes()); // made by
        central.extend_from_slice(&20u16.to_le_bytes()); // version needed
        central.extend_from_slice(&0u16.to_le_bytes()); // flags
        central.extend_from_slice(&entry.method.to_le_bytes());
        central.extend_from_slice(&0u32.to_le_bytes()); // time and date
        central.extend_from_slice(&entry.crc32.to_le_bytes());
        central.extend_from_slice(&(entry.stored as u32).to_le_bytes());
        central.extend_from_slice(&(entry.size as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name);
    }
    let directory = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0; 4]); // disk numbers
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes()); // comment
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, bytes: &[u8], method: u16, stored: &[u8]) -> Entry {
        Entry {
            name: name.into(),
            offset: 0,
            data: 0,
            stored: stored.len() as u64,
            size: bytes.len() as u64,
            method,
            crc32: crc32fast::hash(bytes),
            sha256: hex(&sha2::Sha256::digest(bytes)),
        }
    }

    #[test]
    fn an_entry_is_kept_only_when_its_size_crc_and_sha256_are_the_manifests() {
        let bytes = b"the texture's bytes".repeat(20);
        let mut deflated = Vec::new();
        flate2::read::DeflateEncoder::new(&bytes[..], flate2::Compression::default())
            .read_to_end(&mut deflated)
            .unwrap();
        let e = entry("a.png", &bytes, 8, &deflated);
        assert_eq!(e.unpack(&deflated).unwrap(), bytes);
        let mut other = deflated.clone();
        other[3] ^= 1;
        assert!(e.unpack(&other).is_err());
        let mut wrong = e.clone();
        wrong.sha256 = "0".repeat(64);
        assert_eq!(
            wrong.unpack(&deflated).unwrap_err(),
            "a.png is not the entry the client-jar manifest names (its sha256 differs)"
        );
        assert_eq!(
            e.unpack(&deflated[1..]).unwrap_err(),
            format!(
                "a.png is {} stored bytes, but the client-jar manifest names {}",
                deflated.len() - 1,
                deflated.len()
            )
        );
    }

    #[test]
    fn a_written_zip_reads_back_as_its_entries_where_their_bytes_lie() {
        let a = b"first".to_vec();
        let b = b"second entry".to_vec();
        let ea = entry("assets/minecraft/textures/a.png", &a, 0, &a);
        let eb = entry("assets/minecraft/textures/b.png", &b, 0, &b);
        let zip = write_zip(&[(&ea, &a), (&eb, &b)]);
        let read = entries(&zip).unwrap();
        assert_eq!(read.len(), 2);
        assert_eq!(read[1].name, eb.name);
        let mut found = read[1].clone();
        found.sha256 = eb.sha256.clone();
        assert_eq!(found.unpack(stored(&zip, &read[1])).unwrap(), b);
    }
}
