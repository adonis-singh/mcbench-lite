//! The chunks of a Minecraft dimension's region files (`server/region.js`;
//! Anvil, 1.18+ sections): NBT with long arrays kept as their big-endian
//! bytes, the slots of a region file, and every block that is not air.

use std::io::Read;
use std::path::Path;

use indexmap::IndexMap;

use crate::facts::MinecraftFacts;
use crate::js::code_unit_cmp;

/// An NBT value as `readNbt` reads it.
#[derive(Clone, Debug, PartialEq)]
pub enum Nbt {
    /// Tag 1.
    Byte(i8),
    /// Tag 2.
    Short(i16),
    /// Tag 3.
    Int(i32),
    /// Tag 4.
    Long(i64),
    /// Tag 5.
    Float(f32),
    /// Tag 6.
    Double(f64),
    /// Tag 7.
    ByteArray(Vec<u8>),
    /// Tag 8.
    String(String),
    /// Tag 9.
    List(Vec<Nbt>),
    /// Tag 10, keys in order (a key repeated keeps its first place and its
    /// last value, as an object's).
    Compound(IndexMap<String, Nbt>),
    /// Tag 11.
    IntArray(Vec<i32>),
    /// Tag 12, as its big-endian bytes.
    LongArray(Vec<u8>),
}

impl Nbt {
    /// The child `name` of a compound.
    pub fn get(&self, name: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(map) => map.get(name),
            _ => None,
        }
    }

    /// An integer tag's value.
    pub fn as_i64(&self) -> Option<i64> {
        match *self {
            Nbt::Byte(v) => Some(v as i64),
            Nbt::Short(v) => Some(v as i64),
            Nbt::Int(v) => Some(v as i64),
            Nbt::Long(v) => Some(v),
            _ => None,
        }
    }

    /// A string tag's value.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::String(text) => Some(text),
            _ => None,
        }
    }

    /// A list tag's values.
    pub fn as_list(&self) -> Option<&[Nbt]> {
        match self {
            Nbt::List(list) => Some(list),
            _ => None,
        }
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&e| e <= self.bytes.len())
            .ok_or("NBT ends early")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn text(&mut self) -> Result<String, String> {
        let n = u16::from_be_bytes(self.take(2)?.try_into().expect("two bytes")) as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn count(&mut self) -> Result<usize, String> {
        let n = self.i32()?;
        Ok(n.max(0) as usize)
    }

    fn payload(&mut self, tag: u8) -> Result<Nbt, String> {
        Ok(match tag {
            1 => Nbt::Byte(self.u8()? as i8),
            2 => Nbt::Short(i16::from_be_bytes(
                self.take(2)?.try_into().expect("two bytes"),
            )),
            3 => Nbt::Int(self.i32()?),
            4 => Nbt::Long(i64::from_be_bytes(
                self.take(8)?.try_into().expect("eight bytes"),
            )),
            5 => Nbt::Float(f32::from_be_bytes(
                self.take(4)?.try_into().expect("four bytes"),
            )),
            6 => Nbt::Double(f64::from_be_bytes(
                self.take(8)?.try_into().expect("eight bytes"),
            )),
            7 => {
                let n = self.count()?;
                Nbt::ByteArray(self.take(n)?.to_vec())
            }
            8 => Nbt::String(self.text()?),
            9 => {
                let element = self.u8()?;
                let n = self.count()?;
                let mut list = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    list.push(self.payload(element)?);
                }
                Nbt::List(list)
            }
            10 => {
                let mut out = IndexMap::new();
                loop {
                    let child = self.u8()?;
                    if child == 0 {
                        break;
                    }
                    let name = self.text()?;
                    let value = self.payload(child)?;
                    out.insert(name, value);
                }
                Nbt::Compound(out)
            }
            11 => {
                let n = self.count()?;
                let mut out = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    out.push(self.i32()?);
                }
                Nbt::IntArray(out)
            }
            12 => {
                let n = self.count()?;
                Nbt::LongArray(self.take(n * 8)?.to_vec())
            }
            other => return Err(format!("Unknown NBT tag {other}")),
        })
    }
}

/// Reads an NBT document: its root's payload (`readNbt`).
pub fn read_nbt(bytes: &[u8]) -> Result<Nbt, String> {
    let mut reader = Reader { bytes, at: 0 };
    let tag = reader.u8()?;
    reader.text()?;
    reader.payload(tag)
}

/// One chunk slot of a region file: its index and stored (compressed)
/// bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 0..1024.
    pub index: usize,
    /// The stored bytes: the compression byte, then the data.
    pub stored: Vec<u8>,
}

/// The chunk slots of one region file (`regionSlots`).
pub fn region_slots(file: &Path) -> Result<Vec<Slot>, String> {
    let data = std::fs::read(file).map_err(|e| format!("Cannot read {}: {e}", file.display()))?;
    let mut slots = Vec::new();
    if data.len() < 8192 {
        return Ok(slots);
    }
    for i in 0..1024 {
        let sector = ((data[i * 4] as usize) << 16)
            | ((data[i * 4 + 1] as usize) << 8)
            | data[i * 4 + 2] as usize;
        if sector == 0 {
            continue;
        }
        let at = sector * 4096;
        let head = data
            .get(at..at + 4)
            .ok_or_else(|| format!("{} slot {i} points past the file", file.display()))?;
        let length = i32::from_be_bytes(head.try_into().expect("four bytes"));
        let start = (at + 4).min(data.len());
        let end = (at as i64 + 4 + length as i64).clamp(start as i64, data.len() as i64) as usize;
        slots.push(Slot {
            index: i,
            stored: data[start..end].to_vec(),
        });
    }
    Ok(slots)
}

/// A slot's chunk NBT (`readChunk`): zlib (2), gzip (1) or raw.
pub fn read_chunk(stored: &[u8]) -> Result<Nbt, String> {
    let kind = stored.first().copied().ok_or("An empty chunk slot")?;
    let raw = &stored[1..];
    let mut out = Vec::new();
    let inflated = match kind {
        2 => flate2::read::ZlibDecoder::new(raw)
            .read_to_end(&mut out)
            .map(|_| &out[..]),
        1 => flate2::read::GzDecoder::new(raw)
            .read_to_end(&mut out)
            .map(|_| &out[..]),
        _ => Ok(raw),
    };
    read_nbt(inflated.map_err(|e| format!("A chunk does not inflate: {e}"))?)
}

/// The `.mca` files of a region folder in the order Node's `readdirSync`
/// lists them (libuv sorts names by their bytes).
fn region_files(folder: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map_err(|e| format!("Cannot list {}: {e}", folder.display()))?
        .map(|entry| {
            entry
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<_, _>>()?;
    names.sort();
    Ok(names
        .into_iter()
        .filter(|n| n.ends_with(".mca"))
        .map(|n| folder.join(n))
        .collect())
}

/// Every saved chunk's NBT in a region folder (`readChunks`).
pub fn read_chunks(folder: &Path) -> Result<Vec<Nbt>, String> {
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for file in region_files(folder)? {
        for slot in region_slots(&file)? {
            out.push(read_chunk(&slot.stored)?);
        }
    }
    Ok(out)
}

/// A palette entry as Minecraft names it, properties sorted (`stateName`).
pub fn state_name(entry: &Nbt) -> Result<String, String> {
    let name = entry
        .get("Name")
        .and_then(Nbt::as_str)
        .ok_or("A palette entry has no Name")?;
    let mut properties: Vec<(&String, String)> = match entry.get("Properties") {
        Some(Nbt::Compound(map)) => map
            .iter()
            .map(|(k, v)| {
                (
                    k,
                    v.as_str().map_or_else(|| format!("{v:?}"), str::to_string),
                )
            })
            .collect(),
        _ => Vec::new(),
    };
    if properties.is_empty() {
        return Ok(name.to_string());
    }
    properties.sort_by(|a, b| code_unit_cmp(a.0, b.0));
    Ok(format!(
        "{name}[{}]",
        properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",")
    ))
}

/// A section's 4096 palette indices in Minecraft's order, or `None` for a
/// section of one state (`sectionIndices`). Entries never straddle two
/// longs.
pub fn section_indices(section: &Nbt) -> Result<Option<Vec<u16>>, String> {
    let states = section
        .get("block_states")
        .ok_or("A section has no block_states")?;
    let palette = states
        .get("palette")
        .and_then(Nbt::as_list)
        .ok_or("A section has no palette")?;
    if palette.len() == 1 {
        return Ok(None);
    }
    let Some(Nbt::LongArray(data)) = states.get("data") else {
        return Err("A section of many states has no data".into());
    };
    let ceil_log2 = if palette.len() <= 1 {
        0
    } else {
        64 - (palette.len() as u64 - 1).leading_zeros()
    };
    let bits = ceil_log2.max(4);
    let per_long = 64 / bits;
    let mask = (1u64 << bits) - 1;
    let mut indices = vec![0u16; 4096];
    let (mut index, mut long) = (0usize, 0usize);
    while index < 4096 {
        let bytes = data
            .get(long * 8..long * 8 + 8)
            .ok_or("A section's data ends early")?;
        let value = u64::from_be_bytes(bytes.try_into().expect("eight bytes"));
        let mut k = 0;
        while k < per_long && index < 4096 {
            indices[index] = ((value >> (k * bits)) & mask) as u16;
            k += 1;
            index += 1;
        }
        long += 1;
    }
    Ok(Some(indices))
}

/// `"x,y,z"` to state name for every block that is not air to Minecraft
/// (`readBlockStates`).
pub fn read_block_states(
    folder: &Path,
    facts: &MinecraftFacts,
) -> Result<IndexMap<String, String>, String> {
    let mut states = IndexMap::new();
    for chunk in read_chunks(folder)? {
        let Some(sections) = chunk.get("sections").and_then(Nbt::as_list) else {
            continue;
        };
        let x = chunk
            .get("xPos")
            .and_then(Nbt::as_i64)
            .ok_or("A chunk has no xPos")?;
        let z = chunk
            .get("zPos")
            .and_then(Nbt::as_i64)
            .ok_or("A chunk has no zPos")?;
        for section in sections {
            let Some(palette) = section
                .get("block_states")
                .and_then(|s| s.get("palette"))
                .and_then(Nbt::as_list)
            else {
                continue;
            };
            if palette.is_empty() {
                continue;
            }
            let names = palette
                .iter()
                .map(state_name)
                .collect::<Result<Vec<_>, _>>()?;
            let air = palette
                .iter()
                .map(|entry| {
                    facts.is_air(
                        entry
                            .get("Name")
                            .and_then(Nbt::as_str)
                            .unwrap_or("undefined"),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let indices = section_indices(section)?;
            let y = section
                .get("Y")
                .and_then(Nbt::as_i64)
                .ok_or("A section has no Y")?;
            let origin = [x * 16, y * 16, z * 16];
            for i in 0..4096usize {
                let at = indices.as_ref().map_or(0, |v| v[i] as usize);
                let is_air = air
                    .get(at)
                    .copied()
                    .ok_or_else(|| format!("A section's index {at} is past its palette"))?;
                if !is_air {
                    states.insert(
                        format!(
                            "{},{},{}",
                            origin[0] + (i & 15) as i64,
                            origin[1] + (i >> 8) as i64,
                            origin[2] + ((i >> 4) & 15) as i64
                        ),
                        names[at].clone(),
                    );
                }
            }
        }
    }
    Ok(states)
}
