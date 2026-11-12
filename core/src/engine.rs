// McbenchEngine's requests on this port's level: placing an import's
// commands, writing the blueprint, ticking the live world and writing what
// changed, and a player's inputs. Files are byte for byte the Java engine's.

use crate::blocks;
use crate::dir::{chunk_key, Pos, UPDATE_SHAPE_ORDER};
use crate::level::{Level, UPDATE_CLIENTS, UPDATE_KNOWN_SHAPE, UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS};
use crate::registry::State;
use crate::world::{MIN_Y, SECTIONS};
use std::collections::{BTreeMap, HashMap};

// The live world's stream since its last base (McbenchEngine.Stream): the
// palette its patches index, and the blocks that are not air counted by
// plane along x, y and z (by section coordinate, its 16 planes) and by chunk
// column, which the bounds and the built columns follow as blocks change.
pub struct Stream {
    palette: Vec<State>,
    index: HashMap<State, u32>,
    planes: [BTreeMap<i32, [i32; 16]>; 3],
    columns: HashMap<i64, i32>,
}

impl Stream {
    // One block that is not air more (`count` 1) or fewer (-1) at `at`.
    fn count(&mut self, at: [i32; 3], count: i32) {
        for axis in 0..3 {
            let counts = self.planes[axis].entry(at[axis] >> 4).or_insert([0; 16]);
            counts[(at[axis] & 15) as usize] += count;
            if counts.iter().all(|&n| n == 0) {
                self.planes[axis].remove(&(at[axis] >> 4));
            }
        }
        let column = chunk_key(at[0] >> 4, at[2] >> 4);
        let held = self.columns.entry(column).or_insert(0);
        *held += count;
        if *held == 0 {
            self.columns.remove(&column);
        }
    }

    // The bounds of the blocks (min x, y, z, max x, y, z); None for no block.
    fn bounds(&self) -> Option<[i32; 6]> {
        let mut bounds = [0; 6];
        for axis in 0..3 {
            let (&low, counts) = self.planes[axis].first_key_value()?;
            bounds[axis] = (low << 4) + counts.iter().position(|&n| n != 0).unwrap() as i32;
            let (&high, counts) = self.planes[axis].last_key_value().unwrap();
            bounds[axis + 3] = (high << 4) + counts.iter().rposition(|&n| n != 0).unwrap() as i32;
        }
        Some(bounds)
    }
}

// McbenchEngine.placeAll, which the engine's requests end by failing with an
// error Minecraft reported.
pub fn place_all(level: &mut Level, lines: &[String], strict: bool) -> Result<(), String> {
    place_commands(level, lines, strict);
    level.check_reported()
}

// McbenchEngine.placeAll itself: the verifier's levels go on from an error
// Minecraft reported and fail at their first tick.
pub fn place_commands(level: &mut Level, lines: &[String], strict: bool) {
    let flags = UPDATE_CLIENTS | if strict { 816 } else { UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS };
    let commands: Vec<(Pos, Pos, State, Vec<u16>)> = lines
        .iter()
        .map(|line| {
            let p: Vec<&str> = line.split(' ').collect();
            let fill = p[0] == "fill";
            let n = |i: usize| p[i].parse::<i32>().unwrap_or_else(|_| panic!("bad command {line}"));
            let from = Pos::new(n(1), n(2), n(3));
            let to = if fill { Pos::new(n(4), n(5), n(6)) } else { from };
            let (state, named) = level.reg.parse_state(if fill { p[7] } else { p[4] });
            (from, to, state, named)
        })
        .collect();
    if level.imports {
        for (from, to, _, _) in &commands {
            for x in (from.x() >> 4) - 1..=(to.x() >> 4) + 1 {
                for z in (from.z() >> 4) - 1..=(to.z() >> 4) + 1 {
                    level.ticket(x, z);
                }
            }
        }
    }
    for (from, to, state, named) in &commands {
        let mut changed = Vec::new();
        for z in from.z()..=to.z() {
            for y in from.y()..=to.y() {
                for x in from.x()..=to.x() {
                    let pos = Pos::new(x, y, z);
                    let old = level.get(pos);
                    if place(level, *state, named, pos, flags) && !strict {
                        changed.push((pos, old));
                    }
                }
            }
        }
        for (pos, old) in changed {
            level.update_neighbours_on_block_set(pos, old);
        }
    }
}

// BlockInput.place for a command's block (McbenchEngine.place).
fn place(level: &mut Level, state: State, named: &[u16], pos: Pos, flags: i32) -> bool {
    let mut shaped = if flags & UPDATE_KNOWN_SHAPE != 0 { state } else { update_from_neighbour_shapes(level, state, pos) };
    if level.is_air(shaped) {
        shaped = state;
    }
    // BlockInput.overwriteWithDefinedProperties: unchanged if the shape left
    // the state as parsed, else each named property copied where the shaped
    // block has it (trySetValue: an attached stem shaped into a stem has no
    // facing to copy).
    if shaped != state {
        for &property in named {
            if level.reg.has(shaped, property) {
                shaped = level.reg.with(shaped, property, level.reg.get(state, property));
            }
        }
    }
    level.set(pos, shaped, flags)
}

// Block.updateFromNeighbourShapes.
pub fn update_from_neighbour_shapes(level: &mut Level, mut state: State, pos: Pos) -> State {
    for dir in UPDATE_SHAPE_ORDER {
        let neighbor = pos.relative(dir);
        let neighbor_state = level.get(neighbor);
        state = blocks::update_shape(level, state, pos, dir, neighbor, neighbor_state);
    }
    state
}

struct Out(Vec<u8>);

impl Out {
    fn int(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn short(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_be_bytes());
    }
    fn utf(&mut self, s: &str) {
        assert!(s.is_ascii());
        self.short(s.len() as u16);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn state(&mut self, level: &Level, state: State) {
        self.utf(&level.reg.blocks[level.block(state) as usize].key);
        let values = level.reg.values(state);
        self.int(values.len() as i32);
        for (name, value) in values {
            self.utf(name);
            self.utf(value);
        }
    }
}

// McbenchEngine.write: the level's blocks, every chunk's sections in order,
// as the blueprint's bytes. Fails as the Java engine does when no block is
// placed.
pub fn write(level: &mut Level) -> Result<(Stream, Vec<u8>), String> {
    let air = level.known.air;
    let mut palette = vec![air];
    let mut index: HashMap<State, u32> = HashMap::from([(air, 0)]);
    let mut sections: Vec<((i32, i32, i32), Box<[u16; 4096]>)> = Vec::new();
    let mut planes: [BTreeMap<i32, [i32; 16]>; 3] = Default::default();
    let mut columns = HashMap::new();
    let mut bounds = [i32::MAX, i32::MAX, i32::MAX, i32::MIN, i32::MIN, i32::MIN];
    let mut keys: Vec<i64> = level.world.chunk_keys().collect();
    keys.sort();
    for key in keys {
        let chunk = level.world.chunk(key);
        for i in 0..SECTIONS {
            let Some(section) = &chunk.sections[i] else { continue };
            if section.non_air == 0 {
                continue;
            }
            let (sx, sy, sz) = (chunk.x, (MIN_Y >> 4) + i as i32, chunk.z);
            let mut values: Option<Box<[u16; 4096]>> = None;
            // The section's blocks by plane along x, y and z, and all of them.
            let mut counts = [[0i32; 16]; 3];
            let mut held = 0;
            for at in 0..4096usize {
                let state = section.blocks[at];
                if level.reg.is_air(state) {
                    continue;
                }
                let value = *index.entry(state).or_insert_with(|| {
                    palette.push(state);
                    (palette.len() - 1) as u32
                });
                values.get_or_insert_with(|| Box::new([0; 4096]))[at] = value as u16;
                let local = [at & 15, at >> 8, (at >> 4) & 15];
                for axis in 0..3 {
                    counts[axis][local[axis]] += 1;
                }
                held += 1;
                let pos = [sx * 16 + (at & 15) as i32, sy * 16 + (at >> 8) as i32, sz * 16 + ((at >> 4) & 15) as i32];
                for axis in 0..3 {
                    bounds[axis] = bounds[axis].min(pos[axis]);
                    bounds[axis + 3] = bounds[axis + 3].max(pos[axis]);
                }
            }
            if let Some(values) = values {
                for (axis, s) in [sx, sy, sz].into_iter().enumerate() {
                    let plane = planes[axis].entry(s).or_insert([0; 16]);
                    for n in 0..16 {
                        plane[n] += counts[axis][n];
                    }
                }
                *columns.entry(chunk_key(sx, sz)).or_insert(0) += held;
                sections.push(((sx, sy, sz), values));
            }
        }
    }
    if sections.is_empty() {
        return Err("java.lang.IllegalStateException: The import places no blocks".into());
    }
    let mut out = Out(Vec::with_capacity(16 + sections.len() * 8204));
    out.int(palette.len() as i32);
    for &state in &palette {
        out.state(level, state);
    }
    for v in bounds {
        out.int(v);
    }
    out.int(sections.len() as i32);
    for ((x, y, z), values) in &sections {
        out.int(*x);
        out.int(*y);
        out.int(*z);
        for &v in values.iter() {
            out.short(v);
        }
    }
    level.changes = Some((Vec::new(), Default::default()));
    Ok((Stream { palette, index, planes, columns }, out.0))
}

// McbenchEngine.tick's result: the patch it wrote and the entities' list.
pub struct Ticked {
    pub changes: Vec<u8>,
    pub entities: Vec<u8>,
}

// McbenchEngine.tick: the ticks, then every block that changed as a patch
// with the bounds and the count of built columns after it, which a block
// placed past the bounds, or in a column that had none, grows, and air on
// the bounds' edge, or in a column's last block, shrinks. `light` is whether
// to light what the ticks changed before answering (the stock request). The
// light is lit when anything reads it, so a caller that will only read
// blocks after the ticks (a settle: nothing runs after them) asks for none.
pub fn tick(level: &mut Level, stream: &mut Option<Stream>, ticks: i32, light: bool) -> Result<Ticked, String> {
    for _ in 0..ticks {
        level.tick();
        level.check_reported()?;
    }
    // Light what the ticks changed, as Paper's light worker does between ticks.
    if light {
        level.flush_light();
    }
    // A world without a base fails after its ticks and light, where
    // McbenchEngine.tick meets its null changes.
    if level.changes.is_none() {
        return Err("java.lang.NullPointerException: no base written".into());
    }
    let stream = stream.as_mut().expect("a level with changes and no stream");
    let (order, before) = level.changes.replace((Vec::new(), Default::default())).unwrap();
    let mut changes = Vec::new();
    for pos in order {
        let now = level.get(pos);
        let was = before[&pos];
        if now != was {
            changes.push(pos);
            let (air, was_air) = (level.is_air(now), level.is_air(was));
            if air != was_air {
                stream.count([pos.x(), pos.y(), pos.z()], if air { -1 } else { 1 });
            }
        }
    }
    let entities = crate::entity::json(level).into_bytes();
    let Some(bounds) = stream.bounds() else {
        return Err("java.lang.IllegalStateException: The import places no blocks".into());
    };
    let known = stream.palette.len();
    let mut indices = Vec::with_capacity(changes.len());
    for &pos in &changes {
        let state = level.world.get(pos);
        let palette = &mut stream.palette;
        let value = *stream.index.entry(state).or_insert_with(|| {
            palette.push(state);
            (palette.len() - 1) as u32
        });
        indices.push(value);
    }
    let mut out = Out(Vec::new());
    out.int((stream.palette.len() - known) as i32);
    for &state in &stream.palette[known..] {
        out.state(level, state);
    }
    for v in bounds {
        out.int(v);
    }
    out.int(stream.columns.len() as i32);
    out.int(changes.len() as i32);
    for (i, pos) in changes.iter().enumerate() {
        out.int(pos.x());
        out.int(pos.y());
        out.int(pos.z());
        out.short(indices[i] as u16);
    }
    Ok(Ticked { changes: out.0, entities })
}

// McbenchEngine.input: a lever pulled, a button pressed unless it is pressed
// already, or a block used by the input player looking with `yaw`.
pub fn input(level: &mut Level, kind: &str, pos: Pos, yaw: f32) -> Result<(), String> {
    let state = level.get(pos);
    if !blocks::input(level, kind, state, pos, yaw) {
        return Err(format!("java.lang.IllegalArgumentException: No {kind} input at {pos}: {}", level.reg.describe(state)));
    }
    level.check_reported()
}
