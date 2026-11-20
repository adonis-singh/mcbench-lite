//! One world holding every build of a bench round side by side along x
//! (`server/bench-world.js`). Builds move by whole chunks: a chunk whose
//! source is unchanged keeps its object across updates.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::chunk::{CHUNK_SIZE, Cells, Chunk, Sparse, air_cells};
use crate::chunk_map::ChunkMap;
use crate::world::{KEY_OFFSET, coords_of, key_of};
use crate::{BlockBox, Palette, World};

const GAP_CHUNKS: i32 = 2;
const EMPTY_CHUNKS: i32 = 2;

/// Where one build sits in the composed world (a `layout` entry).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placed {
    /// The member's id.
    pub id: String,
    /// The lowest corner of its build there.
    pub min: [i32; 3],
    /// The highest corner.
    pub max: [i32; 3],
}

/// One message of a member's run, as [`world_at_round`] reads it.
#[derive(Clone, Debug)]
pub struct RoundMessage {
    /// Its id.
    pub id: String,
    /// `user` or `assistant`.
    pub role: String,
    /// Whether it is a `see_build` image message.
    pub see_build: bool,
}

/// The world a model's run had at the end of `round` (1-based): the
/// checkpoint taken before its next round's prompt, or its current world
/// (`worldAtRound`).
pub fn world_at_round<'a>(
    messages: &[RoundMessage],
    checkpoints: &'a HashMap<String, World>,
    world: &'a World,
    round: usize,
) -> Option<&'a World> {
    let prompts: Vec<&RoundMessage> = messages
        .iter()
        .filter(|m| m.role == "user" && !m.see_build)
        .collect();
    match prompts.get(round) {
        Some(next) => checkpoints.get(&next.id),
        None => Some(world),
    }
}

fn chunk_span(bounds: Option<BlockBox>) -> Option<([i32; 2], [i32; 2])> {
    let b = bounds?;
    Some((
        [b.min[0] >> 4, b.min[2] >> 4],
        [b.max[0] >> 4, b.max[2] >> 4],
    ))
}

/// `Math.round`: halves go up.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

/// The composer of one bench (`BenchComposer`).
#[derive(Debug)]
pub struct BenchComposer {
    palette: Arc<Palette>,
    remaps: HashMap<u64, Vec<Option<u16>>>,
    chunks: HashMap<String, (Arc<Chunk>, Arc<Chunk>)>,
    used: HashSet<String>,
    revision: i64,
}

impl Default for BenchComposer {
    fn default() -> BenchComposer {
        BenchComposer::new()
    }
}

impl BenchComposer {
    /// A composer with an empty palette.
    pub fn new() -> BenchComposer {
        BenchComposer {
            palette: Arc::new(Palette::new()),
            remaps: HashMap::new(),
            chunks: HashMap::new(),
            used: HashSet::new(),
            revision: 0,
        }
    }

    fn remap(&mut self, palette: &Palette, index: u16) -> Result<u16, String> {
        let table = self.remaps.entry(palette.uid()).or_default();
        if table.len() <= index as usize {
            table.resize(index as usize + 1, None);
        }
        if let Some(found) = table[index as usize] {
            return Ok(found);
        }
        let entry = palette.entry(index as usize);
        let found = self.palette.intern(&entry.id, &entry.properties)?;
        self.remaps.get_mut(&palette.uid()).expect("made above")[index as usize] = Some(found);
        Ok(found)
    }

    fn moved(
        &mut self,
        member: &str,
        world: &World,
        key: u32,
        source: &Arc<Chunk>,
        dx: i32,
        dz: i32,
    ) -> Result<Arc<Chunk>, String> {
        let cache_key = format!("{member}:{key}:{dx}:{dz}");
        self.used.insert(cache_key.clone());
        if let Some((cached_source, chunk)) = self.chunks.get(&cache_key)
            && Arc::ptr_eq(cached_source, source)
        {
            return Ok(chunk.clone());
        }
        let cells = match source.cells() {
            Cells::Dense(dense) => {
                let mut out = air_cells();
                for (slot, &value) in out.iter_mut().zip(dense.iter()) {
                    if value != 0 {
                        *slot = self.remap(world.palette(), value)?;
                    }
                }
                Cells::Dense(out)
            }
            Cells::Sparse(sparse) => {
                let mut out = Sparse::default();
                for (&index, &value) in sparse {
                    out.insert(index, self.remap(world.palette(), value)?);
                }
                Cells::Sparse(out)
            }
        };
        let chunk = Arc::new(Chunk::of(cells, source.count()));
        self.chunks
            .insert(cache_key, (source.clone(), chunk.clone()));
        Ok(chunk)
    }

    /// The composed world of `entries` (member id and world, in display
    /// order) and where each build sits (`compose`).
    pub fn compose(
        &mut self,
        entries: &[(String, &World)],
    ) -> Result<(World, Vec<Placed>), String> {
        let spans: Vec<_> = entries
            .iter()
            .map(|(_, world)| chunk_span(world.bounds()))
            .collect();
        let widths: Vec<i32> = spans
            .iter()
            .map(|span| span.map_or(EMPTY_CHUNKS, |(min, max)| max[0] - min[0] + 1))
            .collect();
        let total = widths.iter().sum::<i32>() + GAP_CHUNKS * (entries.len() as i32 - 1);
        let mut cursor = -((total as f64) / 2.0).floor() as i32;
        self.used = HashSet::new();
        let mut chunks = ChunkMap::default();
        let mut layout = Vec::new();
        let mut block_count = 0;
        for (slot, (id, world)) in entries.iter().enumerate() {
            match spans[slot] {
                Some((min, max)) => {
                    let dx = cursor - min[0];
                    let dz = -js_round((min[1] + max[1]) as f64 / 2.0) as i32;
                    for (key, source) in world.chunks() {
                        let [cx, cy, cz] = coords_of(key);
                        let (x, z) = (cx + dx, cz + dz);
                        if !(-KEY_OFFSET..KEY_OFFSET).contains(&x)
                            || !(-KEY_OFFSET..KEY_OFFSET).contains(&z)
                        {
                            return Err("These builds are too large to show side by side".into());
                        }
                        let moved = self.moved(id, world, key, source, dx, dz)?;
                        chunks.insert(key_of(x, cy, z), moved);
                    }
                    let bounds = world.bounds().expect("a span has bounds");
                    layout.push(Placed {
                        id: id.clone(),
                        min: [
                            bounds.min[0] + dx * CHUNK_SIZE,
                            bounds.min[1],
                            bounds.min[2] + dz * CHUNK_SIZE,
                        ],
                        max: [
                            bounds.max[0] + dx * CHUNK_SIZE,
                            bounds.max[1],
                            bounds.max[2] + dz * CHUNK_SIZE,
                        ],
                    });
                    block_count += world.size();
                }
                None => {
                    let x = cursor * CHUNK_SIZE;
                    layout.push(Placed {
                        id: id.clone(),
                        min: [x, 0, -CHUNK_SIZE],
                        max: [x + EMPTY_CHUNKS * CHUNK_SIZE - 1, 0, CHUNK_SIZE - 1],
                    });
                }
            }
            cursor += widths[slot] + GAP_CHUNKS;
        }
        let used = std::mem::take(&mut self.used);
        self.chunks.retain(|key, _| used.contains(key));
        self.revision += 1;
        Ok((
            World::assemble(
                chunks,
                self.palette.clone(),
                block_count,
                self.revision,
                None,
            ),
            layout,
        ))
    }
}
