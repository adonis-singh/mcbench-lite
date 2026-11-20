//! Blueprints (`server/blueprint.js`): what the mesher draws, in
//! `McbenchEngine.write`'s format, big endian: `i32` palette entries, each a
//! UTF id, `i32` properties, each a UTF name and value (entry 0 is air);
//! `i32` min x, y, z, max x, y, z of the blocks; `i32` sections, each `i32`
//! section x, y, z and 4096 `u16` palette indices in Minecraft's order (y,
//! z, x).

use std::collections::HashMap;
use std::sync::Arc;

use crate::chunk::{CHUNK_VOLUME, Cells, Chunk, DENSE_THRESHOLD, Sparse, air_cells};
use crate::js::code_unit_cmp;
use crate::palette::Palette;
use crate::world::key_of;
use crate::{AIR, BlockState, Blueprint, MAX_BLOCKS, MAX_COORD, World};

/// `encodeBlueprint(palette, sections, bounds)`: `palette` as state names
/// (`id[key=value,...]`), air first; `sections` as their origin and 4096
/// palette indices.
pub fn encode_blueprint(
    palette: &[String],
    sections: &[([i32; 3], &[u16; CHUNK_VOLUME])],
    bounds: [i32; 6],
) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(64 + palette.len() * 40 + sections.len() * (12 + 8192));
    let text = |out: &mut Vec<u8>, value: &str| -> Result<(), String> {
        let length = u16::try_from(value.len())
            .map_err(|_| format!("A blueprint text of {} bytes is past 65535", value.len()))?;
        out.extend_from_slice(&length.to_be_bytes());
        out.extend_from_slice(value.as_bytes());
        Ok(())
    };
    out.extend_from_slice(&(palette.len() as i32).to_be_bytes());
    for name in palette {
        let (id, pairs) = match name.find('[') {
            None => (name.as_str(), Vec::new()),
            Some(open) => {
                let inner = &name[open + 1..name.len() - 1];
                let mut pairs = Vec::new();
                for pair in inner.split(',') {
                    let mut parts = pair.split('=');
                    let key = parts.next().expect("split yields one part");
                    let value = parts.next().ok_or_else(|| {
                        format!("Blueprint state {name} has a property without a value")
                    })?;
                    pairs.push((key, value));
                }
                (&name[..open], pairs)
            }
        };
        text(&mut out, id)?;
        out.extend_from_slice(&(pairs.len() as i32).to_be_bytes());
        for (key, value) in pairs {
            text(&mut out, key)?;
            text(&mut out, value)?;
        }
    }
    for value in bounds {
        out.extend_from_slice(&value.to_be_bytes());
    }
    out.extend_from_slice(&(sections.len() as i32).to_be_bytes());
    for (origin, indices) in sections {
        for value in origin {
            out.extend_from_slice(&value.to_be_bytes());
        }
        for value in indices.iter() {
            out.extend_from_slice(&value.to_be_bytes());
        }
    }
    Ok(out)
}

/// Gathers blocks into sections and a palette (`collector`): `add` for one
/// block that is not air, then `encode`. An empty world spans one position.
pub struct Collector {
    palette: Vec<String>,
    index: HashMap<String, u16>,
    sections: Vec<([i32; 3], Box<[u16; CHUNK_VOLUME]>)>,
    at: HashMap<[i32; 3], usize>,
    last: Option<([i32; 3], usize)>,
    bounds: [i32; 6],
}

impl Default for Collector {
    fn default() -> Collector {
        Collector::new()
    }
}

impl Collector {
    /// A collector holding nothing.
    pub fn new() -> Collector {
        Collector {
            palette: vec![AIR.to_string()],
            index: HashMap::from([(AIR.to_string(), 0)]),
            sections: Vec::new(),
            at: HashMap::new(),
            last: None,
            bounds: [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN],
        }
    }

    /// The palette index of state `name`, added when new.
    pub fn intern(&mut self, name: &str) -> u16 {
        if let Some(&value) = self.index.get(name) {
            return value;
        }
        let value = self.palette.len() as u16;
        self.index.insert(name.to_string(), value);
        self.palette.push(name.to_string());
        value
    }

    /// Adds a block by its palette index (from [`Collector::intern`]).
    #[inline]
    pub fn add_index(&mut self, x: i32, y: i32, z: i32, value: u16) {
        let key = [x >> 4, y >> 4, z >> 4];
        let slot = match self.last {
            Some((last, slot)) if last == key => slot,
            _ => {
                let slot = match self.at.get(&key) {
                    Some(&slot) => slot,
                    None => {
                        self.sections.push((key, air_cells()));
                        self.at.insert(key, self.sections.len() - 1);
                        self.sections.len() - 1
                    }
                };
                self.last = Some((key, slot));
                slot
            }
        };
        self.sections[slot].1[(((y & 15) << 8) | ((z & 15) << 4) | (x & 15)) as usize] = value;
        let b = &mut self.bounds;
        b[0] = b[0].min(x);
        b[1] = b[1].min(y);
        b[2] = b[2].min(z);
        b[3] = b[3].max(x);
        b[4] = b[4].max(y);
        b[5] = b[5].max(z);
    }

    /// Adds block `name` at `(x, y, z)` (`add`).
    pub fn add(&mut self, x: i32, y: i32, z: i32, name: &str) {
        let value = self.intern(name);
        self.add_index(x, y, z, value);
    }

    /// The blueprint's bytes (`encode`).
    pub fn encode(&self) -> Result<Vec<u8>, String> {
        let sections: Vec<([i32; 3], &[u16; CHUNK_VOLUME])> =
            self.sections.iter().map(|(o, s)| (*o, &**s)).collect();
        encode_blueprint(
            &self.palette,
            &sections,
            if self.sections.is_empty() {
                [0; 6]
            } else {
                self.bounds
            },
        )
    }
}

/// Blocks given as `(x, y, z, state name)` at Minecraft coordinates
/// (`blocksBlueprint`).
pub fn blocks_blueprint(blocks: &[(i32, i32, i32, String)]) -> Result<Vec<u8>, String> {
    let mut collected = Collector::new();
    for (x, y, z, name) in blocks {
        collected.add(*x, *y, *z, name);
    }
    collected.encode()
}

/// A palette entry's state name with its properties sorted (UTF-16 order),
/// as `worldBlueprint` names it.
pub fn state_name(entry: &BlockState) -> String {
    if entry.properties.is_empty() {
        return entry.id.clone();
    }
    let mut sorted: Vec<&(String, String)> = entry.properties.iter().collect();
    sorted.sort_by(|a, b| code_unit_cmp(&a.0, &b.0));
    let pairs: Vec<String> = sorted.iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{}[{}]", entry.id, pairs.join(","))
}

/// A world's blueprint at Minecraft height: browser y + `y_offset`
/// (`worldBlueprint`).
pub fn world_blueprint(world: &World, y_offset: i32) -> Result<Vec<u8>, String> {
    let mut collected = Collector::new();
    let palette = world.palette();
    let mut names: Vec<Option<u16>> = vec![None; palette.len()];
    for (key, chunk) in world.chunks() {
        let [cx, cy, cz] = crate::world::coords_of(key);
        let (ox, oy, oz) = (cx * 16, cy * 16 + y_offset, cz * 16);
        chunk.for_each(|index, value| {
            let value = value as usize;
            if value >= names.len() {
                names.resize(value + 1, None);
            }
            let local = match names[value] {
                Some(local) => local,
                None => {
                    let local = collected.intern(&state_name(palette.entry(value)));
                    names[value] = Some(local);
                    local
                }
            };
            collected.add_index(
                ox + (index & 15) as i32,
                oy + (index >> 8) as i32,
                oz + ((index >> 4) & 15) as i32,
                local,
            );
        });
    }
    collected.encode()
}

/// One palette entry of a blueprint: id, properties as written, and the
/// state name with its `name=value` pairs sorted.
pub type BlueprintEntry = (String, Vec<(String, String)>, String);

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(n)
            .filter(|&end| end <= self.bytes.len())
            .ok_or("The blueprint ends early")?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn int(&mut self) -> Result<i32, String> {
        Ok(i32::from_be_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn text(&mut self) -> Result<String, String> {
        let n = u16::from_be_bytes(self.take(2)?.try_into().expect("two bytes")) as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }

    fn palette(&mut self) -> Result<Vec<BlueprintEntry>, String> {
        let mut palette = Vec::new();
        let mut entries = self.int()?;
        while entries > 0 {
            let id = self.text()?;
            let mut properties = Vec::new();
            let mut pairs = Vec::new();
            let mut count = self.int()?;
            while count > 0 {
                let name = self.text()?;
                let value = self.text()?;
                pairs.push(format!("{name}={value}"));
                properties.push((name, value));
                count -= 1;
            }
            pairs.sort_by(|a, b| code_unit_cmp(a, b));
            let state = if pairs.is_empty() {
                id.clone()
            } else {
                format!("{id}[{}]", pairs.join(","))
            };
            palette.push((id, crate::js::object_order(properties), state));
            entries -= 1;
        }
        Ok(palette)
    }
}

/// `f(x, y, z, entry)` for each block of a blueprint that is not air
/// (`forEachBlueprintBlock`).
pub fn for_each_blueprint_block(
    bytes: &[u8],
    mut f: impl FnMut(i32, i32, i32, &BlueprintEntry),
) -> Result<(), String> {
    let mut reader = Reader { bytes, at: 0 };
    let palette = reader.palette()?;
    reader.take(24)?;
    let mut sections = reader.int()?;
    while sections > 0 {
        let (sx, sy, sz) = (reader.int()?, reader.int()?, reader.int()?);
        let cells = reader.take(8192)?;
        for i in 0..CHUNK_VOLUME {
            let value = u16::from_be_bytes([cells[i * 2], cells[i * 2 + 1]]) as usize;
            if value != 0 {
                let entry = palette.get(value).ok_or_else(|| {
                    format!(
                        "Blueprint palette index {value} is past its {} entries",
                        palette.len()
                    )
                })?;
                f(
                    sx * 16 + (i & 15) as i32,
                    sy * 16 + (i >> 8) as i32,
                    sz * 16 + ((i >> 4) & 15) as i32,
                    entry,
                );
            }
        }
        sections -= 1;
    }
    Ok(())
}

/// A blueprint's blocks as `"x,y,z"` to state name, properties sorted, in
/// the blueprint's order (`readBlueprint`).
pub fn read_blueprint(bytes: &[u8]) -> Result<indexmap::IndexMap<String, String>, String> {
    let mut states = indexmap::IndexMap::new();
    for_each_blueprint_block(bytes, |x, y, z, (_, _, state)| {
        states.insert(format!("{x},{y},{z}"), state.clone());
    })?;
    Ok(states)
}

/// The world of a blueprint's blocks at browser height y - `y_offset`,
/// interning states into `palette` (a new one when `None`) in order of
/// first appearance (`blueprintWorld`): what setting every block into a new
/// world would make, a section at a time.
pub fn blueprint_world(
    bytes: &[u8],
    y_offset: i32,
    palette: Option<Arc<Palette>>,
) -> Result<World, String> {
    let mut world = World::with_palette(palette.unwrap_or_default());
    let mut reader = Reader { bytes, at: 0 };
    let entries = reader.palette()?;
    reader.take(24)?;
    let mut target: Vec<i32> = vec![-1; entries.len().max(1)];
    target[0] = 0;
    let mut filled: indexmap::IndexMap<u32, (Box<[u16; CHUNK_VOLUME]>, usize)> =
        indexmap::IndexMap::new();
    let mut last_key = None;
    let mut last = 0usize;
    let mut sections = reader.int()?;
    while sections > 0 {
        let (sx, sy, sz) = (reader.int()?, reader.int()?, reader.int()?);
        let cells = reader.take(8192)?;
        let (ox, oy, oz) = (sx * 16, sy * 16 - y_offset, sz * 16);
        let inside = ox >= -MAX_COORD
            && ox + 15 <= MAX_COORD
            && oy >= -MAX_COORD
            && oy + 15 <= MAX_COORD
            && oz >= -MAX_COORD
            && oz + 15 <= MAX_COORD;
        for i in 0..CHUNK_VOLUME {
            let value = u16::from_be_bytes([cells[i * 2], cells[i * 2 + 1]]) as usize;
            if value == 0 {
                continue;
            }
            let slot = target.get_mut(value).ok_or_else(|| {
                format!(
                    "Blueprint palette index {value} is past its {} entries",
                    entries.len()
                )
            })?;
            if *slot == -1 {
                let (id, properties, _) = &entries[value];
                *slot = if id == AIR {
                    0
                } else {
                    world.palette().intern(id, properties)? as i32
                };
            }
            let index = *slot as u16;
            let (x, y, z) = (
                ox + (i & 15) as i32,
                oy + (i >> 8) as i32,
                oz + ((i >> 4) & 15) as i32,
            );
            if !inside {
                World::check_coord([x, y, z])?;
            }
            if index == 0 {
                continue;
            }
            let key = key_of(x >> 4, y >> 4, z >> 4);
            if last_key != Some(key) {
                last_key = Some(key);
                last = match filled.get_index_of(&key) {
                    Some(at) => at,
                    None => filled.insert_full(key, (air_cells(), 0)).0,
                };
            }
            let (dense, count) = filled.get_index_mut(last).expect("inserted").1;
            let at = (((y & 15) << 8) | ((z & 15) << 4) | (x & 15)) as usize;
            if dense[at] == 0 {
                *count += 1;
            }
            dense[at] = index;
        }
        sections -= 1;
    }
    for (key, (dense, count)) in filled {
        let cells = if count > DENSE_THRESHOLD {
            Cells::Dense(dense)
        } else {
            let mut sparse = Sparse::default();
            for (i, &value) in dense.iter().enumerate() {
                if value != 0 {
                    sparse.insert(i as u16, value);
                }
            }
            Cells::Sparse(sparse)
        };
        world.chunks.insert(key, Arc::new(Chunk::of(cells, count)));
        world.owned.insert(key);
        world.changed.insert(key);
        world.block_count += count;
    }
    if world.block_count > MAX_BLOCKS {
        return Err(format!("Build exceeds the {MAX_BLOCKS} block limit"));
    }
    Ok(world)
}

impl World {
    /// The world as the engine's blueprint at Minecraft height, browser y +
    /// `y_offset` (`worldBlueprint`).
    pub fn blueprint(&self, y_offset: i32) -> Result<Blueprint, String> {
        world_blueprint(self, y_offset).map(Blueprint)
    }

    /// The world a blueprint holds (`blueprintWorld`).
    pub fn from_blueprint(
        blueprint: &Blueprint,
        y_offset: i32,
        palette: Option<Arc<Palette>>,
    ) -> Result<World, String> {
        blueprint_world(&blueprint.0, y_offset, palette)
    }
}
