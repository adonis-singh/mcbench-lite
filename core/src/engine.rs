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

    // The input each palette entry from `from` on takes
    // (blocks::input_of), a byte each: 0 for none, else the Input's number.
    pub fn inputs(&self, level: &Level, from: usize) -> Vec<u8> {
        self.palette[from..].iter().map(|&state| blocks::input_of(level, state).map_or(0, |kind| kind as u8)).collect()
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

// The blocks of the level that cannot survive where they are
// (BlockState.canSurvive with the blocks around them, which a player's
// placement asks and the updates of many blocks ask again): their positions
// as lines "x y z", in the order the level's chunks and sections hold them.
pub fn unsupported(level: &mut Level) -> String {
    let mut asked = Vec::new();
    for key in level.world.chunk_keys().collect::<Vec<_>>() {
        let chunk = level.world.chunk(key);
        for i in 0..SECTIONS {
            let Some(section) = &chunk.sections[i] else { continue };
            if section.non_air == 0 {
                continue;
            }
            let (sx, sy, sz) = (chunk.x, (MIN_Y >> 4) + i as i32, chunk.z);
            for at in 0..4096usize {
                let state = section.blocks[at];
                if blocks::may_not_survive(level, state) {
                    asked.push((Pos::new(sx * 16 + (at & 15) as i32, sy * 16 + (at >> 8) as i32, sz * 16 + ((at >> 4) & 15) as i32), state));
                }
            }
        }
    }
    let mut out = String::new();
    for (pos, state) in asked {
        if !blocks::can_survive(level, state, pos) {
            out.push_str(&format!("{} {} {}\n", pos.x(), pos.y(), pos.z()));
        }
    }
    out
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

// McbenchEngine.tick's result: the patch it wrote and the entities' list,
// and the input each palette entry the patch adds takes (Stream::inputs).
pub struct Ticked {
    pub changes: Vec<u8>,
    pub entities: Vec<u8>,
    pub inputs: Vec<u8>,
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
    Ok(Ticked { changes: out.0, entities, inputs: stream.inputs(level, known) })
}

// McbenchEngine.input: a lever pulled, a button pressed unless it is pressed
// already, or a block used by the input player looking with `yaw`.
pub fn input(level: &mut Level, kind: &str, pos: Pos, yaw: f32) -> Result<(), String> {
    let state = level.get(pos);
    if !blocks::Input::by_name(kind).is_some_and(|input| blocks::input(level, input, state, pos, yaw)) {
        return Err(format!("java.lang.IllegalArgumentException: No {kind} input at {pos}: {}", level.reg.describe(state)));
    }
    level.check_reported()
}

// A click on the block at `pos` by a player looking with `yaw`: the input
// its state takes (blocks::input_of), given as `input` gives it. Answers the
// input the block took, or none when it takes none or passes on it (a cross
// of dust all four of whose sides connect), which changes nothing.
pub fn click(level: &mut Level, pos: Pos, yaw: f32) -> Result<Option<blocks::Input>, String> {
    let state = level.get(pos);
    let Some(kind) = blocks::input_of(level, state) else { return Ok(None) };
    if !blocks::input(level, kind, state, pos, yaw) {
        return Ok(None);
    }
    level.check_reported()?;
    Ok(Some(kind))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocks::{input_of, Input};

    thread_local! {
        static BOOT: (&'static crate::registry::Registry, &'static blocks::Blocks) = crate::boot();
    }

    fn level() -> Level {
        let (reg, blocks) = BOOT.with(|boot| *boot);
        Level::new(reg, blocks, 0, true)
    }

    fn state(l: &Level, text: &str) -> State {
        l.reg.parse_state(text).0
    }

    // The input of each kind of block that takes one, and of some that take none.
    #[test]
    fn input_of_names_the_input_of_each_block() {
        let l = level();
        let of = |text: &str| input_of(&l, state(&l, text));
        assert_eq!(of("lever"), Some(Input::Toggle));
        let buttons: Vec<_> = l.reg.blocks.iter().filter(|block| block.class == "ButtonBlock").collect();
        assert!(buttons.len() >= 13);
        for button in buttons {
            assert_eq!(of(&button.key), Some(Input::Press), "{}", button.key);
            assert_eq!(of(&format!("{}[powered=true]", button.key)), Some(Input::Press), "{} pressed", button.key);
        }
        for used in ["oak_door", "copper_door", "waxed_oxidized_copper_door", "oak_trapdoor", "exposed_copper_trapdoor", "crimson_fence_gate", "repeater", "comparator", "note_block", "daylight_detector", "decorated_pot"] {
            assert_eq!(of(used), Some(Input::Use), "{used}");
        }
        for none in ["iron_door", "iron_trapdoor", "stone", "chest", "hopper", "piston", "redstone_lamp", "oak_fence", "air"] {
            assert_eq!(of(none), None, "{none}");
        }
        assert_eq!(of("redstone_wire[north=none,east=none,south=none,west=none]"), Some(Input::Use), "a dot");
        assert_eq!(of("redstone_wire[north=side,east=side,south=side,west=side]"), Some(Input::Use), "a cross");
        assert_eq!(of("redstone_wire[north=up,east=side,south=side,west=side,power=7]"), Some(Input::Use), "a cross climbing north");
        assert_eq!(of("redstone_wire[north=side,east=none,south=side,west=none]"), None, "a line");
        assert_eq!(of("redstone_wire[north=side,east=side,south=none,west=none]"), None, "a corner");
    }

    // BlockSetType.canOpenByHand, from the dump: of the doors and trapdoors,
    // the iron ones only are not opened by hand.
    #[test]
    fn only_iron_doors_and_trapdoors_are_not_opened_by_hand() {
        let l = level();
        let closed: Vec<&str> = l
            .reg
            .blocks
            .iter()
            .filter(|block| (block.is("DoorBlock") || block.is("TrapDoorBlock")) && !l.reg.flagged(block.default, "canOpenByHand"))
            .map(|block| block.key.as_str())
            .collect();
        assert_eq!(closed, ["minecraft:iron_door", "minecraft:iron_trapdoor"]);
    }

    // `input` takes a kind exactly when input_of names it, for every state of
    // every block: each state alone in a world on a block of stone (placed as
    // it is, without its onPlace), given each kind.
    #[test]
    fn input_takes_only_and_always_what_input_of_names() {
        let mut shared = level();
        let pos = Pos::new(0, 64, 0);
        for state in 0..level().reg.states.len() as State {
            for kind in [Input::Toggle, Input::Press, Input::Use] {
                let named = input_of(&shared, state) == Some(kind);
                // A state that takes nothing is refused before the world is read.
                let taken = if input_of(&shared, state).is_none() {
                    blocks::input(&mut shared, kind, state, pos, 0.0)
                } else {
                    let mut l = level();
                    l.set(pos.below(), self::state(&l, "stone"), crate::level::UPDATE_CLIENTS);
                    l.set(pos, state, crate::level::UPDATE_CLIENTS | crate::level::UPDATE_SKIP_ON_PLACE);
                    let placed = l.get(pos);
                    assert_eq!(placed, state, "{} placed", l.reg.describe(state));
                    blocks::input(&mut l, kind, placed, pos, 0.0)
                };
                assert_eq!(taken, named, "{} given {kind:?}", shared.reg.describe(state));
            }
        }
    }

    // A click takes the input input_of names; a cross of dust takes it only
    // when not all four of its sides connect, and otherwise nothing changes,
    // as the use input refuses.
    #[test]
    fn a_click_on_dust_takes_what_its_neighbours_let_it() {
        let mut l = level();
        let center = Pos::new(0, 64, 0);
        let dot = state(&l, "redstone_wire");
        let cross = state(&l, "redstone_wire[north=side,east=side,south=side,west=side]");
        l.set(Pos::new(0, 63, 0), state(&l, "stone"), crate::level::UPDATE_CLIENTS);
        l.set(center, dot, crate::level::UPDATE_CLIENTS);
        assert_eq!(click(&mut l, center, 0.0), Ok(Some(Input::Use)));
        assert_eq!(l.get(center), cross, "a dot becomes a cross");
        assert_eq!(click(&mut l, center, 0.0), Ok(Some(Input::Use)));
        assert_eq!(l.get(center), dot, "and the cross, connected to nothing, a dot");
        l.set(center, cross, crate::level::UPDATE_CLIENTS);
        for dir in [crate::dir::Dir::North, crate::dir::Dir::East, crate::dir::Dir::South, crate::dir::Dir::West] {
            l.set(center.relative(dir).below(), state(&l, "stone"), crate::level::UPDATE_CLIENTS);
            l.set(center.relative(dir), dot, 0);
        }
        assert_eq!(click(&mut l, center, 0.0), Ok(None), "a cross connected on all four sides passes");
        assert_eq!(l.get(center), cross);
        assert!(input(&mut l, "use", center, 0.0).unwrap_err().starts_with("java.lang.IllegalArgumentException: No use input at 0, 64, 0: minecraft:redstone_wire["));
        assert_eq!(click(&mut l, Pos::new(0, 63, 0), 0.0), Ok(None), "stone takes nothing");
        assert!(input(&mut l, "kick", center, 0.0).is_err());
    }

    // A pressed button takes the press and stays as it is; an iron door takes
    // no use, as before.
    #[test]
    fn a_pressed_button_takes_a_press_and_an_iron_door_no_use() {
        let mut l = level();
        let pos = Pos::new(0, 64, 0);
        let pressed = state(&l, "stone_button[face=floor,powered=true]");
        l.set(pos, pressed, crate::level::UPDATE_CLIENTS);
        assert_eq!(click(&mut l, pos, 0.0), Ok(Some(Input::Press)));
        assert_eq!(l.get(pos), pressed);
        let door = state(&l, "iron_door[half=lower]");
        l.set(pos, door, crate::level::UPDATE_CLIENTS);
        assert_eq!(click(&mut l, pos, 0.0), Ok(None));
        assert!(input(&mut l, "use", pos, 0.0).is_err());
        assert_eq!(l.get(pos), door);
    }

    // What each block state placed alone does, asked of the engine as
    // McbenchEngine.alone asks Minecraft, against what the table says
    // Minecraft does: its placement by the import, the shape updates from its
    // absent neighbours one after another, a neighbour update, and the block
    // and fluid ticks those updates leave it, each set at a spot of its own;
    // and where a block of stone alone lets a state that cannot survive
    // survive. A state of a block the port refuses is left out.
    #[test]
    fn each_state_placed_alone_does_what_the_table_says_minecraft_does() {
        use crate::level::UPDATE_CLIENTS;
        let (reg, _) = BOOT.with(|boot| *boot);
        assert_eq!(reg.alone.len(), reg.states.len());
        let mut wrong = Vec::new();
        let mut refused = 0;
        let mut l = level();
        let still = UPDATE_CLIENTS | 816;
        let stone = reg.parse_state("minecraft:stone").0;
        let air = l.known.air;
        let air_block = reg.block(air);
        for id in 0..reg.states.len() {
            let state = id as State;
            let at = Pos::new(4 * (id as i32 % 256), 64, 4 * (id as i32 / 256));
            let quiet = Pos::new(at.x(), 128, at.z());
            let block = reg.block(state);
            let probe = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                place_commands(&mut l, &[format!("setblock {} {} {} {}", at.x(), at.y(), at.z(), reg.describe(state))], false);
                let placed = l.get(at);
                let mut bits = if reg.block(placed) == block { 0 } else { 1 };
                l.set(quiet, state, still);
                let shaped = update_from_neighbour_shapes(&mut l, state, quiet);
                if reg.block(shaped) != block {
                    bits |= 2;
                }
                blocks::neighbor_changed(&mut l, state, quiet, air_block, false);
                let changed = l.get(quiet);
                if reg.block(changed) != block {
                    bits |= 4;
                }
                if bits == 0 && l.has_scheduled_tick(quiet, block) {
                    blocks::tick(&mut l, state, quiet);
                    let ticked = l.get(quiet);
                    if reg.block(ticked) != block {
                        bits |= 8;
                    }
                }
                let fluid = l.fluid_state(quiet);
                if bits == 0 && fluid != 0 && l.fluid_ticks.has_scheduled_tick(quiet, reg.fluids[fluid as usize].fluid) {
                    let here = l.get(quiet);
                    crate::fluid::tick(&mut l, quiet, here, fluid);
                    let flowed = l.get(quiet);
                    if reg.block(flowed) != block {
                        bits |= 16;
                    }
                }
                l.set(quiet, state, still);
                let mut held = 0;
                if !blocks::can_survive(&mut l, state, quiet) {
                    for (i, dir) in crate::dir::ALL.iter().enumerate() {
                        l.set(quiet.relative(*dir), stone, still);
                        if blocks::can_survive(&mut l, state, quiet) {
                            held |= 1 << i;
                        }
                        l.set(quiet.relative(*dir), air, still);
                    }
                }
                (bits, held)
            }));
            match probe {
                Ok((bits, held)) => {
                    if bits != reg.alone[id] || held != reg.held[id] {
                        wrong.push(format!("{}: {bits} {held}, the table {} {}", reg.describe(state), reg.alone[id], reg.held[id]));
                    }
                }
                Err(payload) if crate::is_refusal(&*payload) => {
                    refused += 1;
                    l = level();
                }
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
        assert!(refused < 400, "the port refuses {refused} states");
        let bits = |text: &str| reg.alone[reg.parse_state(text).0 as usize];
        assert_eq!((bits("minecraft:stone"), bits("minecraft:candle")), (0, 0), "stone and a candle stay");
        assert_eq!((bits("minecraft:torch"), bits("minecraft:sand"), bits("minecraft:water[level=1]")), (2, 8, 16));
        assert!(reg.alone.iter().filter(|&&bits| bits != 0).count() > 5000);
        assert!(wrong.is_empty(), "{} states: {:#?}", wrong.len(), &wrong[..wrong.len().min(40)]);
    }
}
