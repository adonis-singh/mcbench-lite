//! The parent's side of the build transfer (`server/build-transfer.js`): the
//! world sent to the build child ([`encode_world`]), and the child's changes
//! applied to a clone ([`apply_changes`]). [`decode_world`] and
//! [`encode_changes`] are the child's side, which today's JavaScript runs in
//! the child; they are here for the tests and the differential harness.
//!
//! `[palette json][record count][records][order count][order keys]`, little
//! endian; the order is present only in change sets. A record is `[key i32]
//! [mode u8]`, then, when summaries are written, `[count u16][local bbox 6 x
//! u8, 255 = unknown]`, then the dense (8192 bytes) or sparse (`u16` pairs,
//! then index and value pairs, in the chunk's insertion order) payload.

use std::sync::{Arc, OnceLock};

use crate::chunk::{CHUNK_VOLUME, Cells, Chunk, IntBuild, Sparse, air_cells};
use crate::chunk_map::ChunkMap;
use crate::js::push_json_string;
use crate::world::{coords_of, key_of};
use crate::{AIR, BlockState, MAX_BLOCKS, MAX_COORD, World};

const SPARSE: u8 = 0;
const DENSE: u8 = 1;
const REMOVED: u8 = 2;
const DENSE_BYTES: usize = CHUNK_VOLUME * 2;
const CHUNK_LIMIT: i32 = MAX_COORD >> 4;

fn palette_json(world: &World, start: usize) -> String {
    let palette = world.palette();
    let mut out = String::from("[");
    for index in start..palette.len() {
        let entry = palette.entry(index);
        if index > start {
            out.push(',');
        }
        out.push('[');
        push_json_string(&mut out, &entry.id);
        out.push_str(",{");
        for (at, (key, value)) in entry.properties.iter().enumerate() {
            if at > 0 {
                out.push(',');
            }
            push_json_string(&mut out, key);
            out.push(':');
            push_json_string(&mut out, value);
        }
        out.push_str("}]");
    }
    out.push(']');
    out
}

fn encode(
    world: &World,
    palette_start: usize,
    keys: &[u32],
    order: Option<&[u32]>,
    summaries: bool,
) -> Vec<u8> {
    let palette = palette_json(world, palette_start);
    let mut size = 4 + palette.len() + 4 + order.map_or(0, |o| 4 + o.len() * 4);
    for &key in keys {
        size += 5 + match world.chunk(key) {
            None => 0,
            Some(chunk) => {
                (if summaries { 8 } else { 0 })
                    + match &chunk.cells {
                        Cells::Dense(_) => DENSE_BYTES,
                        Cells::Sparse(sparse) => 2 + sparse.len() * 4,
                    }
            }
        };
    }
    let mut out = Vec::with_capacity(size);
    out.extend_from_slice(&(palette.len() as u32).to_le_bytes());
    out.extend_from_slice(palette.as_bytes());
    out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
    for &key in keys {
        out.extend_from_slice(&(key as i32).to_le_bytes());
        let Some(chunk) = world.chunk(key) else {
            out.push(REMOVED);
            continue;
        };
        out.push(if chunk.is_dense() { DENSE } else { SPARSE });
        if summaries {
            out.extend_from_slice(&(chunk.count as u16).to_le_bytes());
            match chunk.cached_bbox() {
                Some(bbox) => out.extend_from_slice(&bbox),
                None => out.extend_from_slice(&[255; 6]),
            }
        }
        match &chunk.cells {
            Cells::Dense(dense) => {
                let at = out.len();
                out.resize(at + DENSE_BYTES, 0);
                for (slot, value) in out[at..]
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(dense.iter())
                {
                    slot.copy_from_slice(&value.to_le_bytes());
                }
            }
            Cells::Sparse(sparse) => {
                out.extend_from_slice(&(sparse.len() as u16).to_le_bytes());
                for (&index, &value) in sparse {
                    out.extend_from_slice(&index.to_le_bytes());
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    if let Some(order) = order {
        out.extend_from_slice(&(order.len() as u32).to_le_bytes());
        for &key in order {
            out.extend_from_slice(&(key as i32).to_le_bytes());
        }
    }
    debug_assert_eq!(out.len(), size);
    out
}

/// The whole world as the build child reads it (`encodeWorld`).
pub fn encode_world(world: &World) -> Vec<u8> {
    let keys: Vec<u32> = world.chunks().map(|(key, _)| key).collect();
    encode(world, 1, &keys, None, true)
}

/// What the child sends back (`encodeChanges`): the palette past
/// `palette_start`, the changed chunks and the chunk order.
pub fn encode_changes(world: &World, palette_start: usize) -> Vec<u8> {
    let keys: Vec<u32> = world.changed_chunks().collect();
    let order: Vec<u32> = world.chunks().map(|(key, _)| key).collect();
    encode(world, palette_start, &keys, Some(&order), false)
}

struct Reader<'a> {
    buffer: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        if self.offset + length > self.buffer.len() {
            return Err("truncated".into());
        }
        let out = &self.buffer[self.offset..self.offset + length];
        self.offset += length;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn json(&mut self) -> Result<serde_json::Value, String> {
        let length = self.u32()? as usize;
        let text = String::from_utf8_lossy(self.take(length)?).into_owned();
        serde_json::from_str(&text).map_err(|e| format!("The build's palette is not JSON: {e}"))
    }
}

/// Reads the trusted direction: the parent's own world, palette indices,
/// counts and cached boxes taken as sent (`decodeWorld`).
pub fn decode_world(buffer: &[u8]) -> Result<World, String> {
    let mut reader = Reader { buffer, offset: 0 };
    let mut world = World::new();
    for entry in reader
        .json()?
        .as_array()
        .ok_or("The world's palette is not a list")?
    {
        let (id, properties) = entry_of(entry)?;
        world.palette().intern(&id, &properties)?;
    }
    let mut n = reader.u32()?;
    while n > 0 {
        let key = reader.i32()? as u32;
        let mode = reader.u8()?;
        let count = reader.u16()? as usize;
        let bbox: [u8; 6] = reader.take(6)?.try_into().expect("six bytes");
        let cells = if mode == DENSE {
            let bytes = reader.take(DENSE_BYTES)?;
            let mut dense = air_cells();
            for (slot, pair) in dense.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *slot = u16::from_le_bytes([pair[0], pair[1]]);
            }
            Cells::Dense(dense)
        } else {
            let mut sparse = Sparse::default();
            let mut i = reader.u16()?;
            while i > 0 {
                let index = reader.u16()?;
                let value = reader.u16()?;
                sparse.insert(index, value);
                i -= 1;
            }
            Cells::Sparse(sparse)
        };
        let chunk = Chunk::of(cells, count);
        if bbox[0] != 255 {
            chunk.bbox.set(bbox).expect("a new chunk has no box");
        }
        world.chunks.insert(key, Arc::new(chunk));
        world.block_count += count;
        n -= 1;
    }
    Ok(world)
}

/// A palette entry as the transfer writes it: `[id, states]`.
fn entry_of(entry: &serde_json::Value) -> Result<(String, Vec<(String, String)>), String> {
    let id = entry
        .get(0)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("palette entry {entry} has no id"))?;
    let mut properties = Vec::new();
    if let Some(states) = entry.get(1).and_then(|v| v.as_object()) {
        for (key, value) in states {
            let value = value.as_str().ok_or_else(|| {
                format!("palette entry {id} has state {key} = {value}, not a string")
            })?;
            properties.push((key.clone(), value.to_string()));
        }
    }
    Ok((id.to_string(), properties))
}

fn read_chunk(reader: &mut Reader<'_>, remap: &[u16]) -> Result<Option<Chunk>, String> {
    let mode = reader.u8()?;
    if mode == REMOVED {
        return Ok(None);
    }
    let (cells, count) = if mode == DENSE {
        let bytes = reader.take(DENSE_BYTES)?;
        let mut dense = air_cells();
        let mut count = 0;
        for (slot, pair) in dense.iter_mut().zip(bytes.as_chunks::<2>().0) {
            let value = u16::from_le_bytes([pair[0], pair[1]]) as usize;
            if value == 0 {
                continue;
            }
            if value >= remap.len() {
                return Err(format!("palette index {value} out of range"));
            }
            *slot = remap[value];
            count += 1;
        }
        (Cells::Dense(dense), count)
    } else if mode == SPARSE {
        let mut sparse = Sparse::default();
        let mut n = reader.u16()?;
        while n > 0 {
            let index = reader.u16()?;
            let value = reader.u16()?;
            if index as usize >= CHUNK_VOLUME || value == 0 || value as usize >= remap.len() {
                return Err(format!("invalid block {index}={value}"));
            }
            sparse.insert(index, remap[value as usize]);
            n -= 1;
        }
        let count = sparse.len();
        (Cells::Sparse(sparse), count)
    } else {
        return Err(format!("unknown chunk mode {mode}"));
    };
    Ok((count > 0).then(|| Chunk::of(cells, count)))
}

fn read_key(reader: &mut Reader<'_>) -> Result<u32, String> {
    let key = reader.i32()?;
    let [cx, cy, cz] = coords_of(key as u32);
    if key_of(cx, cy, cz) != key as u32
        || cx.abs() > CHUNK_LIMIT
        || cy.abs() > CHUNK_LIMIT
        || cz.abs() > CHUNK_LIMIT
    {
        return Err(format!("chunk key {key} out of range"));
    }
    Ok(key as u32)
}

/// Checks a palette entry the child sent (`normalizeBlock(block, 'palette
/// entry')`, `mc-dsl.js`'s block normalizer against the block registry):
/// given the entry's id and states as the child wrote them, the state the
/// world keeps, or the error.
pub type NormalizeBlock<'a> =
    dyn FnMut(&serde_json::Value, &serde_json::Value) -> Result<BlockState, String> + 'a;

/// Applies the untrusted direction (`applyChanges`). `world` is a fresh
/// clone of the world sent, whose palette had `palette_start` entries then.
/// On return the clone holds what an in-process run would have left: the
/// untouched chunks shared with the original, new ones for the keys
/// touched, the child's chunk order, changed set, palette indices and block
/// count. Everything is checked, the build's height included, before
/// anything changes: on an error the clone and its palette (shared with the
/// original) are as they were.
pub fn apply_changes(
    world: &mut World,
    buffer: &[u8],
    palette_start: usize,
    normalize: &mut NormalizeBlock<'_>,
    y_offset: i32,
) -> Result<(), String> {
    let mut reader = Reader { buffer, offset: 0 };
    let json = reader.json()?;
    let mut blocks = Vec::new();
    for entry in json.as_array().ok_or("The build's palette is not a list")? {
        let null = serde_json::Value::Null;
        let block = normalize(entry.get(0).unwrap_or(&null), entry.get(1).unwrap_or(&null))?;
        if block.id == AIR {
            return Err("air in palette".into());
        }
        blocks.push(block);
    }
    let mut remap: Vec<u16> = (0..palette_start as u16).collect();
    remap.extend(world.palette().plan(&blocks)?);
    let mut changed: indexmap::IndexMap<u32, Option<Arc<Chunk>>, IntBuild> =
        indexmap::IndexMap::default();
    let mut n = reader.u32()?;
    while n > 0 {
        let key = read_key(&mut reader)?;
        if changed.contains_key(&key) {
            return Err(format!("chunk {} repeated", key as i32));
        }
        let chunk = read_chunk(&mut reader, &remap)?.map(Arc::new);
        changed.insert(key, chunk);
        n -= 1;
    }
    let mut expected = world
        .chunks()
        .filter(|(key, _)| !changed.contains_key(key))
        .count();
    expected += changed.values().filter(|c| c.is_some()).count();
    let mut chunks = ChunkMap::with_capacity(expected);
    let mut block_count = 0;
    let mut n = reader.u32()?;
    while n > 0 {
        let raw = reader.i32()?;
        let key = raw as u32;
        let chunk = match changed.get(&key) {
            Some(chunk) => chunk.clone(),
            None => world.chunk(key).cloned(),
        };
        let Some(chunk) = chunk.filter(|_| !chunks.contains(key)) else {
            return Err(format!("chunk order lists {raw} wrongly"));
        };
        block_count += chunk.count;
        chunks.insert(key, chunk);
        n -= 1;
    }
    if reader.offset != buffer.len() {
        return Err("trailing bytes".into());
    }
    if chunks.len() != expected {
        return Err("chunk order is incomplete".into());
    }
    if block_count > MAX_BLOCKS {
        return Err(format!("{block_count} blocks exceed the limit"));
    }
    let probe = World::assemble(chunks, world.palette().clone(), block_count, 0, None);
    crate::dimension::validate_world_height(&probe, y_offset)?;
    for block in &blocks {
        world.palette().intern(&block.id, &block.properties)?;
    }
    world.chunks = probe.chunks;
    world.block_count = block_count;
    world.cached_bounds = OnceLock::new();
    for key in changed.keys() {
        world.owned.insert(*key);
        world.changed.insert(*key);
    }
    Ok(())
}

/// A normalizer that takes the child's entries as they are: an id string
/// and states whose values are strings. For tests and the differential
/// harness, whose change sets Node's own world wrote; the app passes the
/// block registry's normalizer.
pub fn take_as_sent(
    id: &serde_json::Value,
    states: &serde_json::Value,
) -> Result<BlockState, String> {
    let (id, properties) = entry_of(&serde_json::json!([id, states]))?;
    Ok(BlockState { id, properties })
}
