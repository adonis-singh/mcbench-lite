//! A chunk as `see_build`'s page draws it (`server/world.js`
//! `chunkSnapshot`, `shownVersion`): its blocks less those hidden on every
//! side, grouped into LOD proxies where a render LOD is set, with the
//! version that names what it is made from.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::block_states::PaletteStates;
use crate::chunk::{CHUNK_SIZE, CHUNK_VOLUME, Chunk};
use crate::world::{coords_of, key_of};
use crate::{BlockState, World};

/// The boundary layers told apart in this process, numbered by content: the
/// layer of none is 0 (`LAYERS`).
fn layer_number(bits: [u8; 32]) -> usize {
    static LAYERS: OnceLock<Mutex<HashMap<[u8; 32], usize>>> = OnceLock::new();
    let mut layers = LAYERS
        .get_or_init(|| Mutex::new(HashMap::from([([0u8; 32], 0)])))
        .lock()
        .expect("layers lock");
    let next = layers.len();
    *layers.entry(bits).or_insert(next)
}

/// A chunk's six boundary layers (x = 0, x = 15, y = 0, y = 15, z = 0,
/// z = 15) by `occluding` (`boundaryLayers`).
fn boundary_layers(chunk: &Chunk, occluding: &[u8]) -> [usize; 6] {
    let dense = chunk.view();
    let mut bits = [[0u8; 32]; 6];
    let occludes = |value: u16| occluding.get(value as usize).copied().unwrap_or(0) != 0;
    for a in 0..16usize {
        for b in 0..16usize {
            let byte = (a << 1) | (b >> 3);
            let bit = 1u8 << (b & 7);
            let cells = [
                (a << 8) | (b << 4),
                (a << 8) | (b << 4) | 15,
                (a << 4) | b,
                (15 << 8) | (a << 4) | b,
                (a << 8) | b,
                (a << 8) | (15 << 4) | b,
            ];
            for (side, cell) in cells.into_iter().enumerate() {
                if occludes(dense[cell]) {
                    bits[side][byte] |= bit;
                }
            }
        }
    }
    bits.map(layer_number)
}

/// One block of a snapshot: local x, y, z, its index in the snapshot's
/// palette, and its LOD scale when grouped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotBlock {
    /// Local coordinates.
    pub at: [i32; 3],
    /// Index into [`ChunkSnapshot::palette`].
    pub palette: usize,
    /// The proxy's scale along x, y, z, for a grouped block.
    pub scale: Option<[i32; 3]>,
}

/// A chunk as the page is sent it (`chunkSnapshot`'s result).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkSnapshot {
    /// The chunk's key.
    pub key: u32,
    /// Its origin.
    pub origin: [i32; 3],
    /// [`World::shown_version`].
    pub version: String,
    /// The entries its blocks use: the world's entry and its state id.
    pub palette: Vec<(BlockState, u16)>,
    /// Its blocks.
    pub blocks: Vec<SnapshotBlock>,
}

struct Record {
    coords: [i32; 3],
    index: u16,
    opaque: bool,
    scale: Option<[i32; 3]>,
}

impl World {
    /// What [`World::chunk_snapshot`]'s chunk is made from (`shownVersion`):
    /// its version and the number of the boundary layer each of the six
    /// chunks across its faces turns to it.
    pub fn shown_version(&self, key: u32, states: &PaletteStates) -> String {
        let [cx, cy, cz] = coords_of(key);
        let layer = |near: u32, side: usize| {
            self.chunk(near)
                .map_or(0, |chunk| boundary_layers(chunk, &states.occluding)[side])
        };
        format!(
            "{} {} {} {} {} {} {}",
            self.chunk(key).expect("a chunk of the world").version(),
            layer(key_of(cx + 1, cy, cz), 0),
            layer(key_of(cx - 1, cy, cz), 1),
            layer(key_of(cx, cy + 1, cz), 2),
            layer(key_of(cx, cy - 1, cz), 3),
            layer(key_of(cx, cy, cz + 1), 4),
            layer(key_of(cx, cy, cz - 1), 5)
        )
    }

    /// The chunk at `key` as `see_build`'s page draws it (`chunkSnapshot`):
    /// `states` is [`crate::block_states::BlockStates::of_palette`] of this
    /// world's palette, `lod` [`World::render_lod`].
    pub fn chunk_snapshot(
        &self,
        key: u32,
        states: &PaletteStates,
        lod: Option<(usize, i32)>,
    ) -> Option<ChunkSnapshot> {
        let chunk = self.chunk(key)?;
        let [cx, cy, cz] = coords_of(key);
        let (ox, oy, oz) = (cx * CHUNK_SIZE, cy * CHUNK_SIZE, cz * CHUNK_SIZE);
        let occluding = &states.occluding;
        let occ = |value: u16| occluding.get(value as usize).copied() == Some(1);
        let dense = chunk.view();
        let px = self.chunk(key_of(cx + 1, cy, cz));
        let nx = self.chunk(key_of(cx - 1, cy, cz));
        let py = self.chunk(key_of(cx, cy + 1, cz));
        let ny = self.chunk(key_of(cx, cy - 1, cz));
        let pz = self.chunk(key_of(cx, cy, cz + 1));
        let nz = self.chunk(key_of(cx, cy, cz - 1));
        let across = |neighbor: Option<&std::sync::Arc<Chunk>>, index: usize| {
            neighbor.is_some_and(|n| occ(n.get(index)))
        };
        let mut records = Vec::new();
        for index in 0..CHUNK_VOLUME {
            let value = dense[index];
            if value == 0 {
                continue;
            }
            let (lx, lz, ly) = (index & 15, (index >> 4) & 15, index >> 8);
            let opaque = occ(value);
            if opaque
                && (if lx < 15 {
                    occ(dense[index + 1])
                } else {
                    across(px, index - 15)
                })
                && (if lx > 0 {
                    occ(dense[index - 1])
                } else {
                    across(nx, index + 15)
                })
                && (if ly < 15 {
                    occ(dense[index + 256])
                } else {
                    across(py, index - 15 * 256)
                })
                && (if ly > 0 {
                    occ(dense[index - 256])
                } else {
                    across(ny, index + 15 * 256)
                })
                && (if lz < 15 {
                    occ(dense[index + 16])
                } else {
                    across(pz, index - 15 * 16)
                })
                && (if lz > 0 {
                    occ(dense[index - 16])
                } else {
                    across(nz, index + 15 * 16)
                })
            {
                continue;
            }
            records.push(Record {
                coords: [lx as i32, ly as i32, lz as i32],
                index: value,
                opaque,
                scale: None,
            });
        }
        if let Some((axis, scale)) = lod.filter(|(_, scale)| *scale > 1) {
            let plane: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
            let mut groups: indexmap::IndexMap<(i32, i32, i32), Vec<Record>> =
                indexmap::IndexMap::new();
            for record in records {
                let c = record.coords;
                groups
                    .entry((
                        c[axis],
                        c[plane[0]].div_euclid(scale),
                        c[plane[1]].div_euclid(scale),
                    ))
                    .or_default()
                    .push(record);
            }
            let mut compressed = Vec::new();
            for group in groups.into_values() {
                if group.len() != (scale * scale) as usize || !group.iter().all(|r| r.opaque) {
                    compressed.extend(group);
                    continue;
                }
                let mut counts: HashMap<u16, usize> = HashMap::new();
                let mut representative = 0;
                for (at, record) in group.iter().enumerate() {
                    let count = counts.get(&record.index).copied().unwrap_or(0) + 1;
                    counts.insert(record.index, count);
                    if count
                        > counts
                            .get(&group[representative].index)
                            .copied()
                            .unwrap_or(0)
                    {
                        representative = at;
                    }
                }
                let chosen = &group[representative];
                let mut coords = chosen.coords;
                coords[plane[0]] = coords[plane[0]].div_euclid(scale) * scale;
                coords[plane[1]] = coords[plane[1]].div_euclid(scale) * scale;
                let mut size = [1, 1, 1];
                size[plane[0]] = scale;
                size[plane[1]] = scale;
                compressed.push(Record {
                    coords,
                    index: chosen.index,
                    opaque: chosen.opaque,
                    scale: Some(size),
                });
            }
            records = compressed;
        }
        let mut palette = Vec::new();
        let mut local: HashMap<u16, usize> = HashMap::new();
        let mut blocks = Vec::with_capacity(records.len());
        for record in records {
            let at = *local.entry(record.index).or_insert_with(|| {
                palette.push((
                    self.palette().entry(record.index as usize).clone(),
                    states.ids[record.index as usize],
                ));
                palette.len() - 1
            });
            blocks.push(SnapshotBlock {
                at: record.coords,
                palette: at,
                scale: record.scale,
            });
        }
        Some(ChunkSnapshot {
            key,
            origin: [ox, oy, oz],
            version: self.shown_version(key, states),
            palette,
            blocks,
        })
    }
}
