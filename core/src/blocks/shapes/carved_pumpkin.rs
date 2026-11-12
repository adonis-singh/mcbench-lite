// CarvedPumpkinBlock (carved pumpkins and jack o'lanterns). onPlace looks
// for the golem patterns (BlockPattern.find) and spawns the golem of the
// first that matches (trySpawnGolem): the golem made and added, the pattern's
// blocks cleared and their neighbours told, a copper golem's copper block
// turned into a copper chest. What the golem does from then on is not ported
// (entity/golem.rs).

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, ALL};
use crate::entity::golem::{self, Golem};
use crate::level::{refuse, Level, UPDATE_CLIENTS};
use crate::registry::{BlockId, State};
use std::sync::OnceLock;
use crate::shape_tag;

pub const BEHAVIOUR: Behaviour = Behaviour { on_place: Some(on_place), ..NONE };

// What a pattern's character asks of the block there.
#[derive(Clone, Copy)]
enum Cell {
    // '^': PUMPKINS_PREDICATE.
    Pumpkin,
    // '#': snow block, iron block, or a block of BlockTags.COPPER.
    Body(Body),
    // '~': an air state (BlockInWorld.getState().isAir(), which throws for
    // a block in a chunk not loaded).
    Air,
    // ' ': any block.
    Any,
    // The wither's '#': a block of BlockTags.WITHER_SUMMON_BASE_BLOCKS.
    WitherBase,
}

#[derive(Clone, Copy)]
enum Body {
    Snow,
    Iron,
    Copper,
}

// A BlockPattern: its cells by [depth][height][width].
struct Pattern {
    cells: &'static [&'static [&'static [Cell]]],
}

use Cell::{Air, Any, Pumpkin, WitherBase};

const H: Cell = Cell::Body(Body::Snow);
const I: Cell = Cell::Body(Body::Iron);
const C: Cell = Cell::Body(Body::Copper);

// getOrCreateSnowGolemFull: aisle("^", "#", "#").
const SNOW_GOLEM: Pattern = Pattern { cells: &[&[&[Pumpkin], &[H], &[H]]] };
// getOrCreateIronGolemFull: aisle("~^~", "###", "~#~").
const IRON_GOLEM: Pattern = Pattern { cells: &[&[&[Air, Pumpkin, Air], &[I, I, I], &[Air, I, Air]]] };
// getOrCreateCopperGolemFull: aisle("^", "#").
const COPPER_GOLEM: Pattern = Pattern { cells: &[&[&[Pumpkin], &[C]]] };

// getOrCreateSnowGolemBase: aisle(" ", "#", "#").
const SNOW_BASE: Pattern = Pattern { cells: &[&[&[Any], &[H], &[H]]] };
// getOrCreateIronGolemBase: aisle("~ ~", "###", "~#~").
const IRON_BASE: Pattern = Pattern { cells: &[&[&[Air, Any, Air], &[I, I, I], &[Air, I, Air]]] };
// getOrCreateCopperGolemBase: aisle(" ", "#").
const COPPER_BASE: Pattern = Pattern { cells: &[&[&[Any], &[C]]] };
// WitherSkullBlock.getOrCreateWitherBase: aisle("   ", "###", "~#~").
const WITHER_BASE: Pattern = Pattern { cells: &[&[&[Any, Any, Any], &[WitherBase, WitherBase, WitherBase], &[Air, WitherBase, Air]]] };

// CarvedPumpkinBlock.canSpawnGolem.
pub fn can_spawn_golem(l: &mut Level, pos: Pos) -> bool {
    find(l, &SNOW_BASE, pos).is_some() || find(l, &IRON_BASE, pos).is_some() || find(l, &COPPER_BASE, pos).is_some()
}

// WitherSkullBlock.getOrCreateWitherBase().find(level, pos) != null.
pub fn wither_base_found(l: &mut Level, pos: Pos) -> bool {
    find(l, &WITHER_BASE, pos).is_some()
}

// CarvedPumpkinBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        try_spawn_golem(l, pos);
    }
}

// CarvedPumpkinBlock.trySpawnGolem: the snow, iron and copper golem
// patterns in turn; a match spawns its golem.
fn try_spawn_golem(l: &mut Level, pos: Pos) {
    if let Some(found) = find(l, &SNOW_GOLEM, pos) {
        let e = golem::new_golem(l, Golem::Snow);
        spawn_golem_in_world(l, &found, e, found.pos(0, 2));
        return;
    }
    if let Some(found) = find(l, &IRON_GOLEM, pos) {
        let mut e = golem::new_golem(l, Golem::Iron);
        golem::data_mut(&mut e).player_created = true;
        spawn_golem_in_world(l, &found, e, found.pos(1, 2));
        return;
    }
    if let Some(found) = find(l, &COPPER_GOLEM, pos) {
        let e = golem::new_golem(l, Golem::Copper);
        let id = spawn_golem_in_world(l, &found, e, found.pos(0, 0));
        // (The golem was added, so it is valid.) replaceCopperBlockWithChest:
        // the copper block's copper chest, facing as the pumpkin does, from the
        // states the pattern read.
        let (copper, pumpkin) = (found.state(0, 1), found.state(0, 0));
        let direction = l.reg.get_dir(pumpkin, l.reg.property_of(pumpkin, "facing"));
        let chest = crate::blocks::block_entities::copper_chest::from_copper_block(l, copper, direction, found.pos(0, 1));
        l.set(found.pos(0, 1), chest, UPDATE_CLIENTS);
        // CopperGolem.spawn(getWeatherStateFromPattern): the copper block's
        // weather (a waxed block's is its unwaxed block's); the spawn sound
        // reaches no one.
        let weather = copper_weather(l, copper);
        let added = l.entities.all.get_mut(&id).expect("the copper golem just added");
        golem::data_mut(added).weather = weather;
    }
}

// The weather state (WeatheringCopper.WeatherState's ordinal) of a block of
// BlockTags.COPPER: the copper blocks and their waxed blocks.
fn copper_weather(l: &Level, state: State) -> u8 {
    match l.reg.blocks[l.block(state) as usize].key.trim_start_matches("minecraft:").trim_start_matches("waxed_") {
        "copper_block" => 0,
        "exposed_copper" => 1,
        "weathered_copper" => 2,
        "oxidized_copper" => 3,
        other => panic!("not a copper block: {other}"),
    }
}

// CarvedPumpkinBlock.spawnGolemInWorld: the golem put at the block's center
// (snapTo, 0.05 up) and added (the spawn event has no listener), the
// pattern's blocks cleared (setBlock with UPDATE_CLIENTS; the level event
// that shows them breaking reaches no one), no player near to trigger an
// advancement, and the neighbours of each cleared block told
// (updatePatternBlocks). Returns the golem's id.
fn spawn_golem_in_world(l: &mut Level, found: &Match, mut e: Box<crate::entity::Entity>, at: Pos) -> crate::entity::EntityId {
    golem::snap_to(l, &mut e, at.x() as f64 + 0.5, at.y() as f64 + 0.05, at.z() as f64 + 0.5);
    e.spawn_reason = match golem::data(&e).golem {
        Golem::Snow => "BUILD_SNOWMAN",
        Golem::Iron => "BUILD_IRONGOLEM",
        Golem::Copper => "BUILD_COPPERGOLEM",
    };
    let id = e.id;
    crate::entity::add_fresh_entity(l, e);
    let air = l.known.air;
    for i in 0..found.width {
        for j in 0..found.height {
            l.set(found.pos(i, j), air, UPDATE_CLIENTS);
        }
    }
    let air_block = l.block(air);
    for i in 0..found.width {
        for j in 0..found.height {
            l.update_neighbors_at(found.pos(i, j), air_block);
        }
    }
    id
}

// BlockPattern.BlockPatternMatch: the corner and directions a pattern matched
// with, its size, and the states it read there (BlockInWorld's cache, which
// the golem's spawn reads after it changed the blocks).
struct Match {
    corner: Pos,
    finger: Dir,
    thumb: Dir,
    width: i32,
    height: i32,
    states: Vec<State>,
}

impl Match {
    // getBlock(palmOffset, thumbOffset, 0).getPos().
    fn pos(&self, palm: i32, thumb: i32) -> Pos {
        translate_and_rotate(self.corner, self.finger, self.thumb, palm, thumb, 0)
    }

    // getBlock(palmOffset, thumbOffset, 0).getState(), as it was read.
    fn state(&self, palm: i32, thumb: i32) -> State {
        self.states[(palm * self.height + thumb) as usize]
    }
}

// BlockInWorld.getState: none in a chunk not loaded.
fn state_at(l: &mut Level, pos: Pos) -> Option<State> {
    if l.has_chunk_at(pos) {
        Some(l.get(pos))
    } else {
        None
    }
}

struct Ids {
    carved_pumpkin: BlockId,
    jack_o_lantern: BlockId,
    snow_block: BlockId,
    iron_block: BlockId,
}

fn ids(l: &Level) -> &'static Ids {
    static IDS: OnceLock<Ids> = OnceLock::new();
    IDS.get_or_init(|| Ids {
        carved_pumpkin: l.reg.block_key("minecraft:carved_pumpkin"),
        jack_o_lantern: l.reg.block_key("minecraft:jack_o_lantern"),
        snow_block: l.reg.block_key("minecraft:snow_block"),
        iron_block: l.reg.block_key("minecraft:iron_block"),
    })
}

// The cell's predicate on BlockInWorld.getState(); a pattern predicate that
// reads a block in a chunk not loaded throws in Minecraft, which refuses here.
fn test(l: &Level, cell: Cell, state: Option<State>, at: Pos) -> bool {
    match cell {
        Cell::Pumpkin => state.is_some_and(|s| l.block(s) == ids(l).carved_pumpkin || l.block(s) == ids(l).jack_o_lantern),
        Cell::Body(body) => match body {
            Body::Snow => state.is_some_and(|s| l.block(s) == ids(l).snow_block),
            Body::Iron => state.is_some_and(|s| l.block(s) == ids(l).iron_block),
            Body::Copper => match state {
                Some(s) => shape_tag!(l, "minecraft:copper")[l.block(s) as usize],
                None => refuse(format!("a golem pattern reads a block in an unloaded chunk at {at}")),
            },
        },
        Cell::Air => match state {
            Some(s) => l.is_air(s),
            None => refuse(format!("a golem pattern reads a block in an unloaded chunk at {at}")),
        },
        Cell::Any => true,
        Cell::WitherBase => match state {
            Some(s) => l.reg.in_tag(s, "minecraft:wither_summon_base_blocks"),
            None => refuse(format!("a wither pattern reads a block in an unloaded chunk at {at}")),
        },
    }
}

// BlockPattern.find: the match of the pattern with some corner in the cube
// from `pos`, some finger and some thumb direction.
fn find(l: &mut Level, pattern: &Pattern, pos: Pos) -> Option<Match> {
    let depth = pattern.cells.len();
    let height = pattern.cells[0].len();
    let width = pattern.cells[0][0].len();
    let max = width.max(height).max(depth) as i32;
    // BlockPos.betweenClosed(pos, pos.offset(max - 1, ...)): x fastest, then y, then z.
    for dz in 0..max {
        for dy in 0..max {
            for dx in 0..max {
                let corner = Pos::new(pos.x() + dx, pos.y() + dy, pos.z() + dz);
                for finger in ALL {
                    for thumb in ALL {
                        if thumb != finger && thumb != finger.opposite() && matches(l, pattern, corner, finger, thumb, (width, height, depth)) {
                            // The cells' states, read again: the level has not changed since.
                            let mut states = Vec::new();
                            for i in 0..width as i32 {
                                for j in 0..height as i32 {
                                    let at = translate_and_rotate(corner, finger, thumb, i, j, 0);
                                    states.push(state_at(l, at).expect("a matched block in a loaded chunk"));
                                }
                            }
                            return Some(Match { corner, finger, thumb, width: width as i32, height: height as i32, states });
                        }
                    }
                }
            }
        }
    }
    None
}

// BlockPattern.matches.
fn matches(l: &mut Level, pattern: &Pattern, pos: Pos, finger: Dir, thumb: Dir, (width, height, depth): (usize, usize, usize)) -> bool {
    for i in 0..width {
        for i1 in 0..height {
            for i2 in 0..depth {
                let at = translate_and_rotate(pos, finger, thumb, i as i32, i1 as i32, i2 as i32);
                let state = state_at(l, at);
                if !test(l, pattern.cells[i2][i1][i], state, at) {
                    return false;
                }
            }
        }
    }
    true
}

// BlockPattern.translateAndRotate.
fn translate_and_rotate(pos: Pos, finger: Dir, thumb: Dir, palm_offset: i32, thumb_offset: i32, finger_offset: i32) -> Pos {
    let (f, t) = (finger.offset(), thumb.offset());
    let cross = (f.1 * t.2 - f.2 * t.1, f.2 * t.0 - f.0 * t.2, f.0 * t.1 - f.1 * t.0);
    Pos::new(
        pos.x() + t.0 * -thumb_offset + cross.0 * palm_offset + f.0 * finger_offset,
        pos.y() + t.1 * -thumb_offset + cross.1 * palm_offset + f.1 * finger_offset,
        pos.z() + t.2 * -thumb_offset + cross.2 * palm_offset + f.2 * finger_offset,
    )
}
