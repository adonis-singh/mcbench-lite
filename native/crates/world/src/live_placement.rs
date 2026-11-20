//! How a world becomes the vanilla commands that place it
//! (`server/live-placement.js`): which chunks differ from what Minecraft
//! already has, and each chunk's blocks as exact cuboids, in three passes
//! (removals, blocks that stay as placed alone, the others), each column by
//! column from the bottom up.

use std::collections::HashMap;
use std::sync::Arc;

use crate::chunk::{CHUNK_SIZE, CHUNK_VOLUME};
use crate::facts::LastPass;
use crate::js::locale_cmp;
use crate::world::coords_of;
use crate::{AIR, BlockState, Palette, PlaceCommands, World};

/// A state as a command names it, its properties in `localeCompare` order
/// (`blockSpec`).
pub fn block_spec(entry: &BlockState) -> Result<String, String> {
    let mut sorted: Vec<&(String, String)> = entry.properties.iter().collect();
    let mut failure = None;
    sorted.sort_by(|a, b| {
        locale_cmp(&a.0, &b.0).unwrap_or_else(|e| {
            failure = Some(e);
            std::cmp::Ordering::Equal
        })
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if sorted.is_empty() {
        return Ok(entry.id.clone());
    }
    let pairs: Vec<String> = sorted.iter().map(|(k, v)| format!("{k}={v}")).collect();
    Ok(format!("{}[{}]", entry.id, pairs.join(",")))
}

fn cube_command(spec: &str, from: [i32; 3], to: [i32; 3]) -> String {
    if from == to {
        return format!("setblock {} {} {} {spec}", from[0], from[1], from[2]);
    }
    format!(
        "fill {} {} {} {} {} {} {spec}",
        from[0], from[1], from[2], to[0], to[1], to[2]
    )
}

/// One chunk's commands by pass (`compileChunk`'s result).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChunkPlan {
    /// The chunk's key.
    pub key: u32,
    /// Cuboids of air.
    pub removed: Vec<String>,
    /// Cuboids of blocks that stay as placed alone.
    pub alone: Vec<String>,
    /// Cuboids of blocks that need what is around them.
    pub supported: Vec<String>,
}

struct Specs<'a> {
    palette: &'a Palette,
    cache: HashMap<u16, String>,
}

impl<'a> Specs<'a> {
    fn new(palette: &'a Palette) -> Specs<'a> {
        Specs {
            palette,
            cache: HashMap::from([(0, AIR.to_string())]),
        }
    }

    fn at(&mut self, index: u16) -> Result<&str, String> {
        if !self.cache.contains_key(&index) {
            let entry = self
                .palette
                .get(index as usize)
                .ok_or_else(|| format!("palette index {index} has no entry"))?;
            self.cache.insert(index, block_spec(entry)?);
        }
        Ok(&self.cache[&index])
    }
}

/// Greedy exact cuboids inside one chunk (`compileChunk`): what takes
/// `previous`'s chunk at `key` (none when `None`) to `world`'s. `last` is
/// the import's last pass by palette index ([`LastPass`] of the palette the
/// engine places, `placed`).
pub fn compile_chunk(
    world: &World,
    key: u32,
    y_offset: i32,
    previous: Option<&World>,
    last: &[u8],
) -> Result<ChunkPlan, String> {
    let chunk = world.chunk(key);
    let previous_chunk = previous.and_then(|p| p.chunk(key));
    let current = match chunk {
        Some(chunk) => chunk.view().into_owned(),
        None => [0u16; CHUNK_VOLUME],
    };
    let before = previous_chunk.map(|c| c.view().into_owned());
    let same_palette = previous.is_some_and(|p| Arc::ptr_eq(p.palette(), world.palette()));
    let mut specs = Specs::new(world.palette());
    let mut old_specs = previous.map(|p| Specs::new(p.palette()));
    let mut blocks = [-1i32; CHUNK_VOLUME];
    for i in 0..CHUNK_VOLUME {
        let old = before.as_ref().map_or(0, |b| b[i]);
        let next = current[i];
        if (same_palette || previous_chunk.is_none()) && old == next {
            continue;
        }
        if previous_chunk.is_some() {
            let old_spec = old_specs
                .as_mut()
                .expect("a previous world")
                .at(old)?
                .to_string();
            if specs.at(next)? == old_spec {
                continue;
            }
        }
        blocks[i] = next as i32;
    }
    let index_of = |x: i32, y: i32, z: i32| ((y * CHUNK_SIZE + z) * CHUNK_SIZE + x) as usize;
    let mut visited = [false; CHUNK_VOLUME];
    let [cx, cy, cz] = coords_of(key);
    let (ox, oy, oz) = (cx * CHUNK_SIZE, cy * CHUNK_SIZE, cz * CHUNK_SIZE);
    let mut plan = ChunkPlan {
        key,
        removed: Vec::new(),
        alone: Vec::new(),
        supported: Vec::new(),
    };
    for y in 0..CHUNK_SIZE {
        for z in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                let at = index_of(x, y, z);
                let value = blocks[at];
                if value == -1 || visited[at] {
                    continue;
                }
                let free =
                    |i: usize, visited: &[bool; CHUNK_VOLUME]| !visited[i] && blocks[i] == value;
                let mut x2 = x;
                while x2 + 1 < CHUNK_SIZE && free(index_of(x2 + 1, y, z), &visited) {
                    x2 += 1;
                }
                let mut z2 = z;
                while z2 + 1 < CHUNK_SIZE
                    && (x..=x2).all(|xx| free(index_of(xx, y, z2 + 1), &visited))
                {
                    z2 += 1;
                }
                let mut y2 = y;
                while y2 + 1 < CHUNK_SIZE
                    && (z..=z2)
                        .all(|zz| (x..=x2).all(|xx| free(index_of(xx, y2 + 1, zz), &visited)))
                {
                    y2 += 1;
                }
                for yy in y..=y2 {
                    for zz in z..=z2 {
                        for xx in x..=x2 {
                            visited[index_of(xx, yy, zz)] = true;
                        }
                    }
                }
                let spec = specs.at(value as u16)?;
                let command = cube_command(
                    spec,
                    [ox + x, oy + y + y_offset, oz + z],
                    [ox + x2, oy + y2 + y_offset, oz + z2],
                );
                if value == 0 {
                    plan.removed.push(command);
                } else if last.get(value as usize).copied().unwrap_or(0) != 0 {
                    plan.supported.push(command);
                } else {
                    plan.alone.push(command);
                }
            }
        }
    }
    Ok(plan)
}

/// Whether two worlds hold the same blocks in chunk `key` (`chunksEqual`).
pub fn chunks_equal(
    left_world: Option<&World>,
    right_world: &World,
    key: u32,
) -> Result<bool, String> {
    let left = left_world.and_then(|w| w.chunk(key));
    let right = right_world.chunk(key);
    match (left, right) {
        (None, None) => return Ok(true),
        (Some(l), Some(r)) if Arc::ptr_eq(l, r) => return Ok(true),
        (Some(l), Some(r)) if l.count() == r.count() => {}
        _ => return Ok(false),
    }
    let (left, right, left_world) = (
        left.expect("matched"),
        right.expect("matched"),
        left_world.expect("matched"),
    );
    if let (Some(a), Some(b)) = (left.persisted(), right.persisted())
        && a == b
        && Arc::ptr_eq(left_world.palette(), right_world.palette())
    {
        return Ok(true);
    }
    let mut left_specs = Specs::new(left_world.palette());
    let mut right_specs = Specs::new(right_world.palette());
    let (left_view, right_view) = (left.view(), right.view());
    for (&l, &r) in left_view.iter().zip(right_view.iter()) {
        if l == 0 && r == 0 {
            continue;
        }
        if l == 0 || r == 0 {
            return Ok(false);
        }
        if left_world.palette().get(l as usize).is_none()
            || right_world.palette().get(r as usize).is_none()
        {
            return Ok(false);
        }
        if left_specs.at(l)? != right_specs.at(r)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The chunks whose blocks differ between `applied` and `world`, the
/// applied world's keys first (`changedKeys`).
pub fn changed_keys(applied: Option<&World>, world: &World) -> Result<Vec<u32>, String> {
    let mut keys: indexmap::IndexSet<u32> = indexmap::IndexSet::new();
    if let Some(applied) = applied {
        keys.extend(applied.chunks().map(|(key, _)| key));
    }
    keys.extend(world.chunks().map(|(key, _)| key));
    let mut out = Vec::new();
    for key in keys {
        if !chunks_equal(applied, world, key)? {
            out.push(key);
        }
    }
    Ok(out)
}

/// Plans by column, in order of first appearance, each column's plans from
/// the bottom up (`groupPlansByColumn`).
pub fn group_plans_by_column(plans: Vec<ChunkPlan>) -> Vec<Vec<ChunkPlan>> {
    let mut columns: indexmap::IndexMap<(i32, i32), Vec<ChunkPlan>> = indexmap::IndexMap::new();
    for plan in plans {
        let [cx, _, cz] = coords_of(plan.key);
        columns.entry((cx, cz)).or_default().push(plan);
    }
    columns
        .into_values()
        .map(|mut column| {
            column.sort_by_key(|plan| coords_of(plan.key)[1]);
            column
        })
        .collect()
}

/// The keys of the chunks that differ, and every command that takes
/// `applied` to `world` in the order the live import runs them
/// (`importCommands`). `placed` is the palette the engine places (the
/// world's after Minecraft's data fix of its version; the world's own when
/// `None`).
pub fn import_commands(
    world: &World,
    applied: Option<&World>,
    y_offset: i32,
    placed: Option<&Palette>,
    facts: &dyn LastPass,
) -> Result<(Vec<u32>, Vec<String>), String> {
    let last = facts.last_pass(placed.unwrap_or(world.palette()))?;
    let keys = changed_keys(applied, world)?;
    let plans = keys
        .iter()
        .map(|&key| compile_chunk(world, key, y_offset, applied, &last))
        .collect::<Result<Vec<_>, _>>()?;
    let columns = group_plans_by_column(plans);
    let mut commands = Vec::new();
    for pass in 0..3 {
        for column in &columns {
            for plan in column {
                commands.extend(
                    match pass {
                        0 => &plan.removed,
                        1 => &plan.alone,
                        _ => &plan.supported,
                    }
                    .iter()
                    .cloned(),
                );
            }
        }
    }
    Ok((keys, commands))
}

impl World {
    /// The live import's commands that take `applied` (nothing when `None`)
    /// to this world, a line each (`importCommands(...).commands`, as
    /// `live-views.js` joins them).
    pub fn place_commands(
        &self,
        applied: Option<&World>,
        y_offset: i32,
        placed: Option<&Palette>,
        facts: &dyn LastPass,
    ) -> Result<PlaceCommands, String> {
        let (_, commands) = import_commands(self, applied, y_offset, placed, facts)?;
        let mut text = String::with_capacity(commands.iter().map(|c| c.len() + 1).sum());
        for command in commands {
            text.push_str(&command);
            text.push('\n');
        }
        Ok(PlaceCommands(text))
    }
}
