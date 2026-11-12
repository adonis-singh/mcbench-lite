// The level: McbenchLevel's world with Minecraft's block-update machinery,
// ported from the pinned server's own code (Level, LevelChunk, ServerLevel,
// CollectingNeighborUpdater, SignalGetter). Block behaviour is in blocks/.
//
// Anything this port does not model refuses (refuse): the request fails
// rather than going on from a world the server would not have made.

use crate::blocks::{self, BlockEntity, Blocks, Kind};
use crate::dir::{chunk_key, Dir, Pos, HORIZONTAL, UPDATE_ORDER, UPDATE_SHAPE_ORDER};
use crate::random::Random;
use crate::registry::{BlockId, Fluid, FluidId, Registry, State, AIR, ANALOG, BLOCK_ENTITY, CONDUCTOR, EMPTY_FLUID, SIGNAL_SOURCE};
use crate::ticks::{LevelTicks, Tick};
use crate::world::World;
use crate::entity::{self, ChunkStatus, Entities};
use crate::game_event::Listeners;
use crate::loot::RandomSequences;
use crate::fast::{FastMap, FastSet};
use crate::light::Lighting;
use std::collections::{HashMap, HashSet, VecDeque};

// What a refusal carries: what the port does not model.
pub struct Refusal(pub String);

pub fn refuse(what: impl Into<String>) -> ! {
    let what = what.into();
    // For finding where a refusal comes from while developing.
    if let Some(file) = std::env::var_os("MCBENCH_REFUSE_TRACE") {
        use std::io::Write;
        if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
            let _ = writeln!(out, "refusal: {what}\n{}", std::backtrace::Backtrace::force_capture());
        }
    }
    std::panic::panic_any(Refusal(what))
}

pub const UPDATE_NEIGHBORS: i32 = 1;
pub const UPDATE_CLIENTS: i32 = 2;
pub const UPDATE_IMMEDIATE: i32 = 8;
pub const UPDATE_ALL: i32 = 3;
pub const UPDATE_KNOWN_SHAPE: i32 = 16;
pub const UPDATE_SUPPRESS_DROPS: i32 = 32;
pub const UPDATE_MOVE_BY_PISTON: i32 = 64;
pub const UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE: i32 = 128;
pub const UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS: i32 = 256;
pub const UPDATE_SKIP_ON_PLACE: i32 = 512;

// Server properties' max-chained-neighbor-updates, at its default.
const MAX_CHAINED_NEIGHBOR_UPDATES: i32 = 1_000_000;
// Paper's world defaults: environment.max-block-ticks and max-fluid-ticks.
const MAX_BLOCK_TICKS: usize = 65536;
const MAX_FLUID_TICKS: usize = 65536;

// TickPriority values.
pub const PRIORITY_EXTREMELY_HIGH: i32 = -3;
pub const PRIORITY_VERY_HIGH: i32 = -2;
pub const PRIORITY_HIGH: i32 = -1;
pub const PRIORITY_NORMAL: i32 = 0;

// CollectingNeighborUpdater's entries.
#[derive(Clone, Copy)]
enum Updates {
    Shape { direction: Dir, neighbor_state: State, pos: Pos, neighbor_pos: Pos, flags: i32, recursion: i32 },
    // The Shape updates of updateNeighbourShapes, added one after another: `len` of
    // them, `idx` the next, as one entry (the order they run in is the same).
    ShapeSweep { origin: Pos, state: State, flags: i32, recursion: i32, idx: u8, len: u8 },
    Simple { pos: Pos, block: BlockId },
    Full { state: State, pos: Pos, block: BlockId, moved: bool },
    Multi { source: Pos, block: BlockId, skip: Option<Dir>, idx: usize },
}

// A run of entries in `entries` from `start` to `end`, `cursor` the next to run.
#[derive(Clone, Copy)]
struct Group {
    start: usize,
    cursor: usize,
    end: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct BlockEvent {
    pub(crate) pos: Pos,
    block: BlockId,
    pub(crate) a: i32,
    pub(crate) b: i32,
    // Queued by block_event_at_cursor: not in block_event_set.
    ghost: bool,
}

// Well-known blocks and states.
pub struct Known {
    pub air: State,
    pub void_air: State,
    pub redstone_wire: BlockId,
    pub redstone_block: BlockId,
    pub comparator: BlockId,
    pub repeater: BlockId,
    pub hopper: BlockId,
}

pub struct Level {
    pub reg: &'static Registry,
    pub blocks: &'static Blocks,
    pub known: Known,
    pub world: World,
    pub random: Random,
    pub game_time: i64,
    sub_tick: i64,
    pub block_ticks: LevelTicks<BlockId>,
    pub fluid_ticks: LevelTicks<Fluid>,
    pub(crate) block_events: VecDeque<BlockEvent>,
    // The queued block events whose position is a BlockPos.MutableBlockPos still
    // being moved along (cursor_moved).
    cursor_events: Vec<usize>,
    block_event_set: FastSet<BlockEvent>,
    // CollectingNeighborUpdater's stack and added list, as one list of
    // entries run in place (see run_updates).
    entries: Vec<Updates>,
    groups: Vec<Group>,
    count: i32,
    // Chunk tickets (McbenchLevel.ticket): ticketed columns and the chunks
    // whose blocks tick.
    columns: FastSet<i64>,
    ticking: FastSet<i64>,
    // A live import level (McbenchLevel.create(seed)): its commands ticket
    // the columns they touch.
    pub imports: bool,
    pub block_entities: FastMap<Pos, BlockEntity>,
    // The components and persistent data the block entities were loaded
    // with (none where a block entity was made empty).
    pub block_entity_extras: FastMap<Pos, blocks::Extras>,
    // The game event listeners (block entities) by section.
    pub listeners: Listeners,
    // ServerLevel.handlingTick: true from a tick's start through its block
    // events.
    pub handling_tick: bool,
    // The living entities whose experience a sculk catalyst has taken
    // (LivingEntity.skipDropExperience), by entity id.
    pub experience_skipped: Vec<i32>,
    // Block entity tickers (Level.blockEntityTickers): each a wrapper
    // (LevelChunk.RebindableTickingBlockEntityWrapper) bound to the block
    // entity of a generation at its position, or to none (removed).
    generations: FastMap<Pos, u64>,
    next_generation: u64,
    wrappers: Vec<(Pos, Option<u64>)>,
    tickers: Vec<usize>,
    pending_tickers: Vec<usize>,
    ticking_block_entities: bool,
    tickers_in_level: FastMap<Pos, usize>,
    // The live world's changes since its last frame: each position and the
    // block it held before, in the order first changed.
    pub changes: Option<(Vec<Pos>, FastMap<Pos, State>)>,
    // Paper's Level.redstoneUpdateInfos: the redstone torch toggles of the
    // last 60 ticks (position, game time), oldest first, kept per level for
    // torch burnout (RedstoneTorchBlock.Toggle). Java's field is null until
    // the first check, which behaves as an empty deque.
    pub redstone_toggles: VecDeque<(Pos, i64)>,
    // RedStoneWireBlock.shouldSignal: the wire block's one field, false only
    // while it reads its neighbours' signals (blocks/wire.rs).
    pub wire_should_signal: bool,
    // The entities (ServerLevel's entity lookup and tick list), the one
    // random they share (Entity.SHARED_RANDOM) and the loot random
    // sequences (RandomSequences) of this level.
    pub entities: Entities,
    pub shared_random: Random,
    pub sequences: RandomSequences,
    // Spigot's currentPrimedTnt: the primed TNT ticks this server tick so far
    // (reset when the block entities have ticked).
    pub current_primed_tnt: i32,
    // An error Minecraft reported and went on from (McbenchLevel.serverException):
    // the request it happened in fails with it.
    pub reported: Option<String>,
    // The light engine: each chunk's light and the changes gathered since the
    // last flush (McbenchLevel.Light), and Level.skyDarken.
    pub light: Lighting,
    pub sky_darken: i32,
    // Each chunk's block entity map's key layout (see jmap.rs), whose order
    // a saved chunk lists its block entities in.
    pub block_entity_keys: FastMap<i64, crate::jmap::OpenHashKeys>,
    // BlockEntity.ignoreBlockEntityUpdates: set while a hopper puts a stack
    // into a slot, which then tells nothing.
    pub ignore_block_entity_updates: bool,
    // The world's UUID (most and least significant bits): a seeded run's is
    // the seed twice.
    pub world_uuid: (i64, i64),
    // The client player walking in this level, if there is one (crate::player):
    // the level's moving pistons push it as they push the entities.
    pub player: Option<Box<crate::player::Player>>,
}

// The positions of the level's beacons, in the order the state writes them.
fn positions_of_beacons(l: &Level) -> Vec<Pos> {
    let mut positions: Vec<Pos> =
        l.block_entities.iter().filter(|(_, e)| matches!(e, BlockEntity::Special(blocks::special::Special::Beacon(_)))).map(|(pos, _)| *pos).collect();
    positions.sort();
    positions
}

impl Level {
    pub fn new(reg: &'static Registry, blocks: &'static Blocks, seed: i64, imports: bool) -> Level {
        let air_states = (0..reg.states.len()).map(|s| reg.flags(s as State) & AIR != 0).collect();
        let known = Known {
            air: reg.default_state(reg.block_key("minecraft:air")),
            void_air: reg.default_state(reg.block_key("minecraft:void_air")),
            redstone_wire: reg.block_key("minecraft:redstone_wire"),
            redstone_block: reg.block_key("minecraft:redstone_block"),
            comparator: reg.block_key("minecraft:comparator"),
            repeater: reg.block_key("minecraft:repeater"),
            hopper: reg.block_key("minecraft:hopper"),
        };
        Level {
            reg,
            blocks,
            known,
            world: World::new(air_states),
            random: Random::new(seed),
            game_time: 0,
            sub_tick: 0,
            block_ticks: LevelTicks::new(),
            fluid_ticks: LevelTicks::new(),
            block_events: VecDeque::new(),
            cursor_events: Vec::new(),
            block_event_set: FastSet::default(),
            entries: Vec::new(),
            groups: Vec::new(),
            count: 0,
            columns: FastSet::default(),
            ticking: FastSet::default(),
            imports,
            block_entities: FastMap::default(),
            block_entity_extras: FastMap::default(),
            listeners: Listeners::default(),
            handling_tick: false,
            experience_skipped: Vec::new(),
            generations: FastMap::default(),
            next_generation: 0,
            wrappers: Vec::new(),
            tickers: Vec::new(),
            pending_tickers: Vec::new(),
            ticking_block_entities: false,
            tickers_in_level: FastMap::default(),
            changes: None,
            redstone_toggles: VecDeque::new(),
            wire_should_signal: true,
            entities: Entities::new(),
            shared_random: Random::new(seed),
            sequences: RandomSequences::new(seed),
            current_primed_tnt: 0,
            reported: None,
            light: Lighting::new(),
            sky_darken: 0,
            block_entity_keys: FastMap::default(),
            ignore_block_entity_updates: false,
            world_uuid: (seed, seed),
            player: None,
        }
    }

    // The chunk at x, z, made when first asked for, with its tick queue.
    #[inline]
    pub fn chunk(&mut self, x: i32, z: i32) {
        if !self.world.is_last(x, z) {
            self.chunk_other(x, z);
        }
    }

    // A chunk other than the last asked for, made if new.
    #[inline(never)]
    fn chunk_other(&mut self, x: i32, z: i32) {
        if self.world.touch(x, z) {
            // The light a new chunk is generated with (ChunkStatus.LIGHT).
            self.light.light_chunk(self.reg, &self.world, x, z);
            self.block_ticks.add_container(chunk_key(x, z));
            self.fluid_ticks.add_container(chunk_key(x, z));
            // The chunk's entity slices are made with it (McbenchLevel.chunk).
            let status = self.chunk_status(chunk_key(x, z));
            self.entities.chunk_created(x, z, status);
        }
    }

    // McbenchLevel.status.
    pub fn chunk_status(&self, key: i64) -> ChunkStatus {
        if self.columns.contains(&key) {
            ChunkStatus::EntityTicking
        } else if self.ticking.contains(&key) {
            ChunkStatus::BlockTicking
        } else {
            ChunkStatus::Full
        }
    }

    // McbenchLevel.ticket: the column entity ticking, the ring around it
    // block ticking, the next ring loaded.
    pub fn ticket(&mut self, x: i32, z: i32) {
        if self.columns.contains(&chunk_key(x, z)) {
            return;
        }
        // The status each chunk that exists has before the ticket.
        let mut before = Vec::new();
        for dx in -2..=2 {
            for dz in -2..=2 {
                if self.world.has_chunk(x + dx, z + dz) {
                    let key = chunk_key(x + dx, z + dz);
                    before.push((key, self.chunk_status(key)));
                }
            }
        }
        self.columns.insert(chunk_key(x, z));
        for dx in -2..=2 {
            for dz in -2..=2 {
                if dx * dx <= 1 && dz * dz <= 1 {
                    self.ticking.insert(chunk_key(x + dx, z + dz));
                }
            }
        }
        for dx in -2..=2 {
            for dz in -2..=2 {
                self.chunk(x + dx, z + dz);
                let key = chunk_key(x + dx, z + dz);
                if let Some(&(_, old)) = before.iter().find(|(k, _)| *k == key) {
                    let status = self.chunk_status(key);
                    if old != status {
                        entity::chunk_status_change(self, x + dx, z + dz, status);
                    }
                }
            }
        }
    }

    #[inline]
    pub fn ticks_at(&self, pos: Pos) -> bool {
        self.ticking.contains(&pos.chunk())
    }

    // Level.shouldTickBlocksAt(ChunkPos.asLong(x, z)).
    pub fn ticks_in_chunk(&self, x: i32, z: i32) -> bool {
        self.ticking.contains(&chunk_key(x, z))
    }

    // Which generation of block entity is at the position (a block entity
    // replaced at the position is another).
    pub fn block_entity_generation(&self, pos: Pos) -> Option<u64> {
        self.generations.get(&pos).copied()
    }

    // --- Reading blocks ---

    #[inline]
    pub fn get(&mut self, pos: Pos) -> State {
        if let Some(state) = self.world.get_last(pos) {
            return state;
        }
        self.get_elsewhere(pos)
    }

    // A block outside the chunk last asked for (or outside the world).
    #[inline(never)]
    fn get_elsewhere(&mut self, pos: Pos) -> State {
        if World::outside(pos) {
            return self.known.void_air;
        }
        self.chunk(pos.x() >> 4, pos.z() >> 4);
        self.world.get(pos)
    }

    // Level.getBlockStateIfLoaded.
    pub fn get_if_loaded(&mut self, pos: Pos) -> Option<State> {
        if World::outside(pos) {
            return Some(self.known.void_air);
        }
        if !self.world.has_chunk(pos.x() >> 4, pos.z() >> 4) {
            return None;
        }
        Some(self.world.get(pos))
    }

    pub fn has_chunk_at(&self, pos: Pos) -> bool {
        self.world.has_chunk(pos.x() >> 4, pos.z() >> 4)
    }

    #[inline]
    pub fn block(&self, state: State) -> BlockId {
        self.reg.block(state)
    }

    #[inline]
    pub fn kind(&self, state: State) -> Kind {
        self.blocks.state_kinds[state as usize]
    }

    #[inline]
    pub fn is_air(&self, state: State) -> bool {
        self.reg.flags(state) & AIR != 0
    }

    #[inline]
    pub fn is_conductor(&self, state: State) -> bool {
        self.reg.flags(state) & CONDUCTOR != 0
    }

    #[inline]
    pub fn is_signal_source(&self, state: State) -> bool {
        self.reg.flags(state) & SIGNAL_SOURCE != 0
    }

    #[inline]
    pub fn has_analog_output(&self, state: State) -> bool {
        self.reg.flags(state) & ANALOG != 0
    }

    #[inline]
    pub fn has_block_entity(&self, state: State) -> bool {
        self.reg.flags(state) & BLOCK_ENTITY != 0
    }

    // BlockState.getFluidState.
    #[inline]
    pub fn fluid_of(&self, state: State) -> FluidId {
        self.reg.fluid(state)
    }

    // Level.getFluidState.
    #[inline]
    pub fn fluid_state(&mut self, pos: Pos) -> FluidId {
        if World::outside(pos) {
            return EMPTY_FLUID;
        }
        let state = self.get(pos);
        self.fluid_of(state)
    }

    // Level.getFluidIfLoaded: none for a chunk that is not loaded.
    pub fn fluid_if_loaded(&mut self, pos: Pos) -> Option<FluidId> {
        if !self.world.has_chunk(pos.x() >> 4, pos.z() >> 4) {
            return None;
        }
        if World::outside(pos) {
            return Some(EMPTY_FLUID);
        }
        Some(self.fluid_of(self.world.get(pos)))
    }

    // The fluid state's type.
    #[inline]
    pub fn fluid_type(&self, fluid: FluidId) -> Fluid {
        self.reg.fluids[fluid as usize].fluid
    }

    // --- Signals (SignalGetter) ---

    #[inline]
    pub fn direct_signal(&mut self, pos: Pos, dir: Dir) -> i32 {
        let state = self.get(pos);
        blocks::get_direct_signal(self, state, pos, dir)
    }

    pub fn direct_signal_to(&mut self, pos: Pos) -> i32 {
        let mut i = 0;
        for dir in crate::dir::ALL {
            i = i.max(self.direct_signal(pos.relative(dir), dir));
            if i >= 15 {
                return i;
            }
        }
        i
    }

    pub fn control_input_signal(&mut self, pos: Pos, dir: Dir, diodes_only: bool) -> i32 {
        let state = self.get(pos);
        if diodes_only {
            return if blocks::is_diode(self, state) { self.direct_signal(pos, dir) } else { 0 };
        }
        if self.block(state) == self.known.redstone_block {
            return 15;
        }
        if self.block(state) == self.known.redstone_wire {
            return blocks::wire_power(self, state);
        }
        if self.is_signal_source(state) {
            self.direct_signal(pos, dir)
        } else {
            0
        }
    }

    pub fn signal(&mut self, pos: Pos, dir: Dir) -> i32 {
        let state = self.get(pos);
        let signal = blocks::get_signal(self, state, pos, dir);
        if self.is_conductor(state) {
            signal.max(self.direct_signal_to(pos))
        } else {
            signal
        }
    }

    pub fn has_signal(&mut self, pos: Pos, dir: Dir) -> bool {
        self.signal(pos, dir) > 0
    }

    pub fn has_neighbor_signal(&mut self, pos: Pos) -> bool {
        crate::dir::ALL.iter().any(|&dir| self.signal(pos.relative(dir), dir) > 0)
    }

    pub fn best_neighbor_signal(&mut self, pos: Pos) -> i32 {
        let mut best = 0;
        for dir in crate::dir::ALL {
            let signal = self.signal(pos.relative(dir), dir);
            if signal >= 15 {
                return 15;
            }
            best = best.max(signal);
        }
        best
    }

    // --- Changing blocks ---

    pub fn set(&mut self, pos: Pos, state: State, flags: i32) -> bool {
        self.set_block(pos, state, flags, 512)
    }

    // Level.setBlock, with McbenchLevel's change tracking.
    pub fn set_block(&mut self, pos: Pos, state: State, flags: i32, recursion: i32) -> bool {
        if self.changes.is_some() && !World::outside(pos) {
            let before = self.get(pos);
            let (order, changes) = self.changes.as_mut().unwrap();
            if let std::collections::hash_map::Entry::Vacant(entry) = changes.entry(pos) {
                entry.insert(before);
                order.push(pos);
            }
        }
        if World::outside(pos) {
            return false;
        }
        self.chunk(pos.x() >> 4, pos.z() >> 4);
        let Some(old) = self.chunk_set_block_state(pos, state, flags) else { return false };
        let current = self.world.get(pos);
        self.notify_and_update_physics(pos, old, state, current, flags, recursion);
        true
    }

    // LevelChunk.setBlockState: the state before, or none if nothing changed
    // (or the block was replaced again while the old one was removed).
    fn chunk_set_block_state(&mut self, pos: Pos, state: State, flags: i32) -> Option<State> {
        let only_air = self.world.section_has_only_air(pos);
        if only_air && self.is_air(state) {
            return None;
        }
        let old = self.world.set(pos, state);
        if old == state {
            return None;
        }
        self.world.update_heightmaps(self.reg, pos, state);
        let has_only_air = self.world.section_has_only_air(pos);
        if only_air != has_only_air {
            self.light.update_section_status(pos.x() >> 4, pos.y() >> 4, pos.z() >> 4, has_only_air);
        }
        if self.has_different_light_properties(old, state) {
            self.light.check_block(pos);
        }
        let block = self.block(state);
        let block_changed = self.block(old) != block;
        let moved = flags & UPDATE_MOVE_BY_PISTON != 0;
        let side_effects = flags & UPDATE_SKIP_BLOCK_ENTITY_SIDEEFFECTS == 0;
        if block_changed && self.has_block_entity(old) && !blocks::keeps_block_entity(self, state, old) {
            if side_effects {
                if let Some(entity) = self.block_entities.get(&pos).cloned() {
                    blocks::pre_remove_side_effects(self, entity, pos, old);
                }
            }
            self.remove_block_entity_in(pos, old);
        }
        if (block_changed || blocks::is_rail(self, state)) && (flags & UPDATE_NEIGHBORS != 0 || moved) {
            blocks::affect_neighbors_after_removal(self, old, pos, moved);
        }
        if self.block(self.world.get(pos)) != block {
            return None;
        }
        if flags & UPDATE_SKIP_ON_PLACE == 0 {
            blocks::on_place(self, state, pos, old, moved);
        }
        if self.has_block_entity(state) {
            match self.block_entities.get(&pos) {
                Some(entity) if !blocks::block_entity_valid(self, entity, state) => {
                    self.remove_block_entity_in(pos, old);
                    if let Some(entity) = blocks::new_block_entity(self, state, pos) {
                        self.register_block_entity(pos, entity, state);
                    }
                }
                // BlockEntity.setBlockState, then a new ticker for the state.
                Some(_) => self.update_block_entity_ticker(pos, state),
                None => {
                    if let Some(entity) = blocks::new_block_entity(self, state, pos) {
                        self.register_block_entity(pos, entity, state);
                    }
                }
            }
        }
        Some(old)
    }

    // Light.flush: lights what has changed since, which Paper's light worker
    // does between ticks.
    pub fn flush_light(&mut self) {
        self.light.flush(self.reg, &self.world);
    }

    // The columns lit again from their blocks (StarLightInterface.relightChunks).
    pub fn relight(&mut self, columns: &[(i32, i32)]) -> usize {
        self.light.relight(self.reg, &self.world, columns)
    }

    // Level.getBrightness(LightLayer.SKY, pos): the light engine's layer
    // listener lights what is gathered first (Light.getLayerListener).
    pub fn sky_brightness(&mut self, pos: Pos) -> i32 {
        self.flush_light();
        self.light.sky_light_value(pos)
    }

    // Level.getBrightness(LightLayer.BLOCK, pos).
    #[allow(dead_code)]
    pub fn block_brightness(&mut self, pos: Pos) -> i32 {
        self.flush_light();
        self.light.block_light_value(pos)
    }

    // Level.getRawBrightness(pos, darkening) (Light.getRawBrightness).
    #[allow(dead_code)]
    pub fn raw_brightness(&mut self, pos: Pos, darkening: i32) -> i32 {
        self.flush_light();
        self.light.raw_brightness(pos, darkening)
    }

    // LightEngine.hasDifferentLightProperties.
    fn has_different_light_properties(&self, old: State, new: State) -> bool {
        use crate::registry::USE_SHAPE_FOR_LIGHT;
        new != old
            && (self.reg.light_properties(new) != self.reg.light_properties(old) || self.reg.flags(new) & USE_SHAPE_FOR_LIGHT != 0 || self.reg.flags(old) & USE_SHAPE_FOR_LIGHT != 0)
    }

    // LevelChunk.setBlockEntity (through addAndRegisterBlockEntity): a block
    // entity made for `state`, for the block now at its position. If that
    // block does not take it, Minecraft reports an error and does not keep
    // it (the block was replaced while it was placed); if it is another
    // block that does, it reports the mismatch and keeps it for that one.
    fn register_block_entity(&mut self, pos: Pos, entity: BlockEntity, state: State) {
        let current = self.world.get(pos);
        let mut state = state;
        if current != state {
            if !blocks::block_entity_valid(self, &entity, current) {
                self.report(format!("Trying to set block entity at {pos}, but state {} does not allow it", self.reg.describe(current)));
                return;
            }
            if self.block(current) != self.block(state) {
                self.report(format!("Block state mismatch on block entity in position {pos}"));
            }
            state = current;
        }
        self.add_block_entity(pos, entity, state);
    }

    // An error Minecraft logs and goes on from: the first one a request
    // meets fails it (McbenchLevel.reported).
    pub fn report(&mut self, what: String) {
        if self.reported.is_none() {
            self.reported = Some(what);
        }
    }

    // McbenchLevel.reported: the request fails with the error if there is one.
    pub fn check_reported(&mut self) -> Result<(), String> {
        match self.reported.take() {
            Some(what) => Err(format!("java.lang.IllegalStateException: Minecraft reported an error, which Paper logs and goes on from: {what}")),
            None => Ok(()),
        }
    }

    // Level.getBlockEntity: made for a block that has one if missing.
    pub fn block_entity(&mut self, pos: Pos) -> Option<&mut BlockEntity> {
        if World::outside(pos) {
            return None;
        }
        let state = self.get(pos);
        if !self.block_entities.contains_key(&pos) {
            if !self.has_block_entity(state) {
                return None;
            }
            let entity = blocks::new_block_entity(self, state, pos)?;
            self.add_block_entity(pos, entity, state);
        }
        self.block_entities.get_mut(&pos)
    }

    // Level.setBlockEntity: LevelChunk.setBlockEntity, which reports an
    // error and keeps nothing for a state without a block entity.
    pub fn set_block_entity(&mut self, pos: Pos, entity: BlockEntity) {
        if World::outside(pos) {
            return;
        }
        let state = self.get(pos);
        if !self.has_block_entity(state) {
            self.report(format!("Trying to set block entity at {pos}, but state {} does not allow it", self.reg.describe(state)));
            return;
        }
        self.register_block_entity(pos, entity, state);
    }

    // LevelChunk.addAndRegisterBlockEntity: the block entity of `state`,
    // replacing any at its position, and its ticker.
    fn add_block_entity(&mut self, pos: Pos, entity: BlockEntity, state: State) {
        // LevelChunk.addGameEventListener.
        if blocks::special::is_listener(&entity) {
            self.listeners.register(pos);
        }
        self.block_entities.insert(pos, entity);
        self.block_entity_extras.remove(&pos);
        self.block_entity_keys.entry(pos.chunk()).or_insert_with(crate::jmap::OpenHashKeys::new).put(pos);
        self.generations.insert(pos, self.next_generation);
        self.next_generation += 1;
        self.update_block_entity_ticker(pos, state);
    }

    // BlockEntity.saveWithoutMetadata of the block entity at the position:
    // its fields, and the components and persistent data it was loaded with.
    pub fn block_entity_saved_tag(&self, pos: Pos) -> (&'static str, crate::nbt::Compound) {
        let (key, mut data) = self.block_entities[&pos].saved_tag(self.reg);
        if let Some(extras) = self.block_entity_extras.get(&pos) {
            if !extras.components.is_empty() {
                data.put("components", crate::nbt::Tag::Compound(extras.components.clone()));
            }
            if !extras.bukkit.is_empty() {
                data.put("PublicBukkitValues", crate::nbt::Tag::Compound(extras.bukkit.clone()));
            }
        }
        (key, data)
    }

    // The components and persistent data a block entity was loaded with.
    pub fn set_block_entity_extras(&mut self, pos: Pos, extras: blocks::Extras) {
        if extras.is_empty() {
            self.block_entity_extras.remove(&pos);
        } else {
            self.block_entity_extras.insert(pos, extras);
        }
    }

    // Its type's key and saved data as SNBT.
    pub fn block_entity_saved(&self, pos: Pos) -> (&'static str, String) {
        let (key, data) = self.block_entity_saved_tag(pos);
        (key, data.to_string())
    }

    // BlockEntity.saveWithFullMetadata: the saved data with the type's id and
    // the position.
    pub fn block_entity_saved_with_metadata(&self, pos: Pos) -> String {
        let (key, mut data) = self.block_entity_saved_tag(pos);
        data.put_string("id", key);
        data.put_int("x", pos.x());
        data.put_int("y", pos.y());
        data.put_int("z", pos.z());
        data.to_string()
    }

    // LevelChunk.removeBlockEntity.
    pub fn remove_block_entity(&mut self, pos: Pos) {
        let state = self.get(pos);
        self.remove_block_entity_in(pos, state);
    }

    // LevelChunk.removeBlockEntity of an entity whose own state (the one
    // BlockEntity.getBlockState gives) is `entity_state`.
    pub fn remove_block_entity_in(&mut self, pos: Pos, entity_state: State) {
        // LevelChunk.removeGameEventListener, then BlockEntity.setRemoved.
        self.block_entity_extras.remove(&pos);
        if let Some(entity) = self.block_entities.remove(&pos) {
            self.block_entity_keys.get_mut(&pos.chunk()).expect("a chunk with block entities").remove(pos);
            if blocks::special::is_listener(&entity) {
                self.listeners.unregister(pos);
            }
            // JukeboxBlockEntity.setRemoved: the song stops for whoever listens.
            if matches!(entity, BlockEntity::Entity(blocks::entities::Entity::Jukebox(_))) {
                let event = crate::game_event!(self, "jukebox_stop_play");
                self.game_event_in(event, pos, crate::game_event::Context::of_state(entity_state));
            }
        }
        self.generations.remove(&pos);
        self.remove_block_entity_ticker(pos);
    }

    // LevelChunk.updateBlockEntityTicker: a block entity whose state ticks
    // gets a wrapper, kept in place when it had one.
    fn update_block_entity_ticker(&mut self, pos: Pos, state: State) {
        if !blocks::has_ticker(self, state) {
            self.remove_block_entity_ticker(pos);
            return;
        }
        let generation = self.generations[&pos];
        match self.tickers_in_level.get(&pos) {
            Some(&wrapper) => self.wrappers[wrapper].1 = Some(generation),
            None => {
                self.wrappers.push((pos, Some(generation)));
                let wrapper = self.wrappers.len() - 1;
                self.tickers_in_level.insert(pos, wrapper);
                if self.ticking_block_entities {
                    self.pending_tickers.push(wrapper);
                } else {
                    self.tickers.push(wrapper);
                }
            }
        }
    }

    // LevelChunk.removeBlockEntityTicker: the wrapper is rebound to no
    // block entity, and leaves the list on the next pass.
    fn remove_block_entity_ticker(&mut self, pos: Pos) {
        if let Some(wrapper) = self.tickers_in_level.remove(&pos) {
            self.wrappers[wrapper].1 = None;
        }
    }

    fn ticker_removed(&self, wrapper: usize) -> bool {
        let (pos, generation) = self.wrappers[wrapper];
        generation.is_none() || self.generations.get(&pos).copied() != generation
    }

    // What the level holds besides its blocks, as McbenchEngine.state
    // writes it.
    pub fn state_text(&mut self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        writeln!(out, "time {}", self.game_time).unwrap();
        writeln!(out, "random {}", self.random.state()).unwrap();
        writeln!(out, "subtick {}", self.sub_tick).unwrap();
        for tick in self.block_ticks.all() {
            let p = tick.pos;
            writeln!(out, "tick {} {} {} {} {} {} {}", p.x(), p.y(), p.z(), self.reg.blocks[tick.kind as usize].key, tick.trigger, tick.priority, tick.sub).unwrap();
        }
        for tick in self.fluid_ticks.all() {
            let p = tick.pos;
            writeln!(out, "fluidtick {} {} {} {} {} {} {}", p.x(), p.y(), p.z(), tick.kind.key(), tick.trigger, tick.priority, tick.sub).unwrap();
        }
        for event in &self.block_events {
            let p = event.pos;
            writeln!(out, "event {} {} {} {} {} {}", p.x(), p.y(), p.z(), self.reg.blocks[event.block as usize].key, event.a, event.b).unwrap();
        }
        let mut positions: Vec<&Pos> = self.block_entities.keys().collect();
        positions.sort();
        for pos in positions {
            let (kind, data) = self.block_entity_saved_tag(*pos);
            let mut text = String::new();
            crate::nbt::Tag::Compound(data).write_engine_snbt(&mut text);
            writeln!(out, "blockentity {} {} {} {kind} {text}", pos.x(), pos.y(), pos.z()).unwrap();
        }
        for (i, list) in [&self.tickers, &self.pending_tickers].into_iter().enumerate() {
            for &wrapper in list {
                if self.ticker_removed(wrapper) {
                    writeln!(out, "ticker removed").unwrap();
                } else {
                    let p = self.wrappers[wrapper].0;
                    writeln!(out, "ticker {} {} {}", p.x(), p.y(), p.z()).unwrap();
                }
            }
            if i == 0 {
                writeln!(out, "pending").unwrap();
            }
        }
        // The game event listeners and the beacons' beams.
        for line in self.listeners.state_lines() {
            writeln!(out, "{line}").unwrap();
        }
        for pos in &positions_of_beacons(self) {
            if let Some(BlockEntity::Special(blocks::special::Special::Beacon(beacon))) = self.block_entities.get(pos) {
                writeln!(out, "{}", beacon.state_text(*pos)).unwrap();
            }
        }
        // The entity layer's hidden state.
        writeln!(out, "entityid {}", self.entities.counter).unwrap();
        writeln!(out, "sharedrandom {}", self.shared_random.state()).unwrap();
        for (key, lo, hi) in self.sequences.states() {
            writeln!(out, "sequence {key} {lo} {hi}").unwrap();
        }
        out.push_str(&entity::state_text(self));
        // The light engine: what is gathered, then (once lit) each chunk's
        // light and heightmaps.
        writeln!(out, "skydarken {}", self.sky_darken).unwrap();
        out.push_str(&self.light.pending_text());
        self.flush_light();
        out.push_str(&self.light.light_text());
        let mut keys: Vec<i64> = self.world.chunk_keys().collect();
        keys.sort_by_key(|key| (*key as i32, (*key >> 32) as i32));
        for key in keys {
            let chunk = self.world.chunk(key);
            writeln!(out, "heightmaps {} {}", chunk.x, chunk.z).unwrap();
            for map in &chunk.heightmaps {
                let longs: Vec<String> = map.raw().iter().map(|l| format!("{l:x}")).collect();
                writeln!(out, "{} {}", map.kind.key(), longs.join(",")).unwrap();
            }
        }
        out
    }

    // Level.tickBlockEntities.
    pub(crate) fn tick_block_entities(&mut self) {
        self.ticking_block_entities = true;
        let pending = std::mem::take(&mut self.pending_tickers);
        self.tickers.extend(pending);
        let mut removed = FastSet::default();
        let mut i = 0;
        while i < self.tickers.len() {
            let wrapper = self.tickers[i];
            if self.ticker_removed(wrapper) {
                removed.insert(wrapper);
            } else {
                let pos = self.wrappers[wrapper].0;
                // LevelChunk.BoundTickingBlockEntity.tick.
                if self.ticks_at(pos) {
                    let state = self.get(pos);
                    if blocks::block_entity_valid(self, &self.block_entities[&pos], state) {
                        blocks::tick_block_entity(self, state, pos);
                    } else {
                        self.remove_block_entity(pos);
                    }
                }
            }
            i += 1;
        }
        if !removed.is_empty() {
            self.tickers.retain(|wrapper| !removed.contains(wrapper));
        }
        self.ticking_block_entities = false;
    }

    pub fn notify_and_update_physics(&mut self, pos: Pos, old: State, new: State, current: State, flags: i32, recursion: i32) {
        if current != new {
            return;
        }
        if flags & UPDATE_NEIGHBORS != 0 {
            self.update_neighbors_at(pos, self.block(old));
            if self.has_analog_output(new) {
                self.update_neighbour_for_output_signal(pos, self.block(new));
            }
        }
        if flags & UPDATE_KNOWN_SHAPE == 0 && recursion > 0 {
            let i = flags & !(UPDATE_NEIGHBORS | UPDATE_SUPPRESS_DROPS);
            blocks::update_indirect_neighbour_shapes(self, old, pos, i, recursion - 1);
            self.update_neighbour_shapes(new, pos, i, recursion - 1);
            blocks::update_indirect_neighbour_shapes(self, new, pos, i, recursion - 1);
        }
    }

    // BlockStateBase.updateNeighbourShapes.
    pub fn update_neighbour_shapes(&mut self, state: State, pos: Pos, flags: i32, recursion: i32) {
        if self.count == 0 {
            // Not run from an update: each runs as soon as it is added.
            for dir in UPDATE_SHAPE_ORDER {
                self.neighbor_shape_changed(dir.opposite(), pos.relative(dir), pos, state, flags, recursion);
            }
            return;
        }
        // Run from an update: the six are added (those past the limit of
        // chained updates dropped), to run after it.
        let admitted = (MAX_CHAINED_NEIGHBOR_UPDATES - self.count).clamp(0, UPDATE_SHAPE_ORDER.len() as i32);
        self.count += UPDATE_SHAPE_ORDER.len() as i32;
        if admitted > 0 {
            self.entries.push(Updates::ShapeSweep { origin: pos, state, flags, recursion, idx: 0, len: admitted as u8 });
        }
    }

    // Level.neighborShapeChanged.
    pub fn neighbor_shape_changed(&mut self, direction: Dir, pos: Pos, neighbor_pos: Pos, neighbor_state: State, flags: i32, recursion: i32) {
        if self.count == 0 {
            self.count = 1;
            self.shape_step(direction, neighbor_state, pos, neighbor_pos, flags, recursion);
            self.finish_top();
        } else {
            self.add_nested(Updates::Shape { direction, neighbor_state, pos, neighbor_pos, flags, recursion });
        }
    }

    pub fn update_neighbors_at(&mut self, pos: Pos, block: BlockId) {
        if self.count == 0 {
            self.multi_top(pos, block, None, 0);
        } else {
            self.add_nested(Updates::Multi { source: pos, block, skip: None, idx: 0 });
        }
    }

    pub fn update_neighbors_at_except_from_facing(&mut self, pos: Pos, block: BlockId, skip: Dir) {
        let idx = if UPDATE_ORDER[0] == skip { 1 } else { 0 };
        if self.count == 0 {
            self.multi_top(pos, block, Some(skip), idx);
        } else {
            self.add_nested(Updates::Multi { source: pos, block, skip: Some(skip), idx });
        }
    }

    // Level.neighborChanged(pos, block, orientation).
    pub fn neighbor_changed(&mut self, pos: Pos, block: BlockId) {
        if self.count == 0 {
            self.count = 1;
            self.simple_step(pos, block);
            self.finish_top();
        } else {
            self.add_nested(Updates::Simple { pos, block });
        }
    }

    // Level.neighborChanged(state, pos, block, orientation, movedByPiston).
    pub fn neighbor_changed_with(&mut self, state: State, pos: Pos, block: BlockId, moved: bool) {
        if self.count == 0 {
            self.count = 1;
            blocks::neighbor_changed(self, state, pos, block, moved);
            self.finish_top();
        } else {
            self.add_nested(Updates::Full { state, pos, block, moved });
        }
    }

    pub fn update_neighbour_for_output_signal(&mut self, pos: Pos, block: BlockId) {
        for dir in HORIZONTAL {
            let mut at = pos.relative(dir);
            if !self.has_chunk_at(at) {
                continue;
            }
            let mut state = self.get(at);
            if self.block(state) == self.known.comparator {
                self.neighbor_changed_with(state, at, block, false);
            } else if self.is_conductor(state) {
                at = at.relative(dir);
                state = self.get(at);
                if self.block(state) == self.known.comparator {
                    self.neighbor_changed_with(state, at, block, false);
                }
            }
        }
    }

    // ServerLevel.updateNeighboursOnBlockSet.
    pub fn update_neighbours_on_block_set(&mut self, pos: Pos, old: State) {
        let state = self.get(pos);
        let block = self.block(state);
        if self.block(old) != block {
            blocks::affect_neighbors_after_removal(self, old, pos, false);
        }
        self.update_neighbors_at(pos, block);
        if self.has_analog_output(state) {
            self.update_neighbour_for_output_signal(pos, block);
        }
    }

    // Block.updateOrDestroy.
    pub fn update_or_destroy(&mut self, old: State, new: State, pos: Pos, flags: i32, recursion: i32) {
        if new == old {
            return;
        }
        if self.is_air(new) {
            self.destroy_block(pos, flags & UPDATE_SUPPRESS_DROPS == 0, recursion);
        } else {
            self.set_block(pos, new, flags & !UPDATE_SUPPRESS_DROPS, recursion);
        }
    }

    // Level.removeBlock(pos, movedByPiston): the block becomes the fluid it held.
    pub fn remove_block(&mut self, pos: Pos, moved: bool) -> bool {
        let fluid = self.fluid_state(pos);
        self.set(pos, self.reg.fluids[fluid as usize].legacy, UPDATE_ALL | if moved { UPDATE_MOVE_BY_PISTON } else { 0 })
    }

    // Level.destroyBlock with no entity.
    pub fn destroy_block(&mut self, pos: Pos, drop: bool, recursion: i32) -> bool {
        self.destroy_block_by(pos, drop, recursion, None)
    }

    // Level.destroyBlock(pos, drop, entity, recursion): the entity that
    // breaks the block is the source of its BLOCK_DESTROY event.
    pub fn destroy_block_by(&mut self, pos: Pos, drop: bool, recursion: i32, breaker: Option<crate::game_event::Source>) -> bool {
        let state = self.get(pos);
        if self.is_air(state) {
            return false;
        }
        let fluid = self.fluid_state(pos);
        // Paper takes the block's experience whether it drops or not
        // (BlockDestroyEvent has no listener).
        let xp = entity::item::exp_drop(self, state);
        if drop {
            // The block entity (made if missing) is only read by drops that
            // copy it, which refuse.
            if self.has_block_entity(state) {
                self.block_entity(pos);
            }
            entity::item::drop_resources_no_experience(self, state, pos, breaker.is_some());
            entity::item::pop_experience(self, pos, xp, breaker.map(|b| b.uuid));
        }
        let destroyed = self.set_block(pos, self.reg.fluids[fluid as usize].legacy, UPDATE_ALL, recursion);
        if destroyed {
            let event = crate::game_event!(self, "block_destroy");
            self.game_event_in(event, pos, crate::game_event::Context::of(breaker, state));
        }
        destroyed
    }

    // --- CollectingNeighborUpdater ---
    //
    // Java adds an entry to a stack and runs the stack when it is not being
    // run already (count is the number of entries added since). An entry run
    // that way is run here as soon as it is added, without going through the
    // list: `count` is 1 while it runs, and what it adds goes to `entries`.

    // An entry added while updates run (dropped past the limit of chained updates).
    fn add_nested(&mut self, updates: Updates) {
        let over = self.count >= MAX_CHAINED_NEIGHBOR_UPDATES;
        self.count += 1;
        if !over {
            self.entries.push(updates);
        }
    }

    // The end of an update run from the top: what it added runs, then all is as before.
    fn finish_top(&mut self) {
        if !self.entries.is_empty() {
            self.run_groups();
        }
        self.count = 0;
    }

    // Level.updateNeighborsAt from the top: the neighbours in order, what one
    // adds running before the next.
    fn multi_top(&mut self, source: Pos, block: BlockId, skip: Option<Dir>, mut idx: usize) {
        self.count = 1;
        loop {
            let dir = UPDATE_ORDER[idx];
            idx += 1;
            if idx < UPDATE_ORDER.len() && Some(UPDATE_ORDER[idx]) == skip {
                idx += 1;
            }
            let pos = source.relative(dir);
            let state = self.get(pos);
            blocks::neighbor_changed(self, state, pos, block, false);
            if !self.entries.is_empty() {
                self.run_groups();
            }
            if idx >= UPDATE_ORDER.len() {
                break;
            }
        }
        self.count = 0;
    }

    // CollectingNeighborUpdater.runUpdates for what was added while an
    // update ran. Java keeps a stack of entries and a list of those added
    // while one runs, which go on the stack in reverse when it has run (so the
    // first added runs first, then what it added, then the second, and an
    // entry with more to run resumes after all that). The entries here stay
    // where they were pushed: the list holds groups of entries (those added
    // during one step, in the order added) and each group a cursor, which is
    // the same order without moving an entry.
    fn run_groups(&mut self) {
        self.groups.push(Group { start: 0, cursor: 0, end: self.entries.len() });
        while let Some(&group) = self.groups.last() {
            if group.cursor == group.end {
                self.groups.pop();
                self.entries.truncate(group.start);
                continue;
            }
            let before = self.entries.len();
            let more = self.run_next(group.cursor);
            if !more {
                self.groups.last_mut().unwrap().cursor += 1;
            }
            if self.entries.len() > before {
                self.groups.push(Group { start: before, cursor: before, end: self.entries.len() });
            }
        }
    }

    fn shape_step(&mut self, direction: Dir, neighbor_state: State, pos: Pos, neighbor_pos: Pos, flags: i32, recursion: i32) {
        let state = self.get(pos);
        if flags & UPDATE_SKIP_SHAPE_UPDATE_ON_WIRE == 0 || self.block(state) != self.known.redstone_wire {
            let shaped = blocks::update_shape(self, state, pos, direction, neighbor_pos, neighbor_state);
            self.update_or_destroy(state, shaped, pos, flags, recursion);
        }
    }

    fn simple_step(&mut self, pos: Pos, block: BlockId) {
        let state = self.get(pos);
        blocks::neighbor_changed(self, state, pos, block, false);
    }

    // NeighborUpdates.runNext on the entry at `at`: whether it has more to run.
    fn run_next(&mut self, at: usize) -> bool {
        match self.entries[at] {
            Updates::Shape { direction, neighbor_state, pos, neighbor_pos, flags, recursion } => {
                self.shape_step(direction, neighbor_state, pos, neighbor_pos, flags, recursion);
                false
            }
            Updates::ShapeSweep { origin, state: neighbor_state, flags, recursion, idx, len } => {
                let dir = UPDATE_SHAPE_ORDER[idx as usize];
                let more = idx + 1 < len;
                if more {
                    if let Updates::ShapeSweep { idx, .. } = &mut self.entries[at] {
                        *idx += 1;
                    }
                }
                self.shape_step(dir.opposite(), neighbor_state, origin.relative(dir), origin, flags, recursion);
                more
            }
            Updates::Simple { pos, block } => {
                self.simple_step(pos, block);
                false
            }
            Updates::Full { state, pos, block, moved } => {
                blocks::neighbor_changed(self, state, pos, block, moved);
                false
            }
            Updates::Multi { source, block, skip, idx } => {
                let dir = UPDATE_ORDER[idx];
                let mut next = idx + 1;
                if next < UPDATE_ORDER.len() && Some(UPDATE_ORDER[next]) == skip {
                    next += 1;
                }
                let more = next < UPDATE_ORDER.len();
                if let Updates::Multi { idx, .. } = &mut self.entries[at] {
                    *idx = next;
                }
                self.simple_step(source.relative(dir), block);
                more
            }
        }
    }

    // --- Scheduled ticks and block events ---

    pub fn schedule_tick(&mut self, pos: Pos, block: BlockId, delay: i32, priority: i32) {
        let sub = self.sub_tick;
        self.sub_tick += 1;
        self.block_ticks.schedule(Tick { pos, kind: block, trigger: self.game_time + delay as i64, priority, sub });
    }

    // McbenchExperiment.importScheduledTicks' queue work: the saved queue is
    // authoritative inside the box (from `min` to `max`), so the ticks there
    // go and the saved ones are scheduled, with their own sub-tick orders; new
    // ticks are ordered after all of them.
    pub fn restore_ticks(&mut self, min: Pos, max: Pos, ticks: Vec<Tick<BlockId>>, fluids: Vec<Tick<Fluid>>, largest_order: i64) {
        self.sub_tick = self.sub_tick.max(largest_order + 1);
        self.block_ticks.clear_area(min, max);
        self.fluid_ticks.clear_area(min, max);
        for tick in ticks {
            // A queue keeps ticks of one order apart by chance; this one
            // keeps them by order.
            if self.block_ticks.has_order(tick.pos.chunk(), tick.sub) {
                refuse("two scheduled ticks of one sub-tick order in a chunk");
            }
            self.block_ticks.schedule(tick);
        }
        for tick in fluids {
            if self.fluid_ticks.has_order(tick.pos.chunk(), tick.sub) {
                refuse("two scheduled ticks of one sub-tick order in a chunk");
            }
            self.fluid_ticks.schedule(tick);
        }
    }

    // ScheduledTickAccess.scheduleTick(pos, fluid, delay): the next sub-tick
    // order is shared with block ticks.
    pub fn schedule_fluid_tick(&mut self, pos: Pos, fluid: Fluid, delay: i32) {
        let sub = self.sub_tick;
        self.sub_tick += 1;
        self.fluid_ticks.schedule(Tick { pos, kind: fluid, trigger: self.game_time + delay as i64, priority: PRIORITY_NORMAL, sub });
    }

    pub fn has_scheduled_tick(&self, pos: Pos, block: BlockId) -> bool {
        self.block_ticks.has_scheduled_tick(pos, block)
    }

    pub fn will_tick_this_tick(&self, pos: Pos, block: BlockId) -> bool {
        self.block_ticks.will_tick_this_tick(pos, block)
    }

    pub fn block_event(&mut self, pos: Pos, block: BlockId, a: i32, b: i32) {
        let event = BlockEvent { pos, block, a, b, ghost: false };
        if self.block_event_set.insert(event) {
            self.block_events.push_back(event);
        }
    }

    // Level.blockEvent(pos, ...) with the position of an iterator that goes on
    // moving it (BlockPos.MutableBlockPos, which BlockEventData does not copy):
    // the event's position becomes the iterator's, step after step. Such an event
    // is never the same as another (its hash was taken at the position it started
    // at).
    pub fn block_event_at_cursor(&mut self, pos: Pos, block: BlockId, a: i32, b: i32) {
        self.cursor_events.push(self.block_events.len());
        self.block_events.push_back(BlockEvent { pos, block, a, b, ghost: true });
    }

    // The iterator yields a new position: the events that follow it move.
    pub fn cursor_moved(&mut self, pos: Pos) {
        for &index in &self.cursor_events {
            self.block_events[index].pos = pos;
        }
    }

    // A new iterator begins (or the walk is over): the events keep the position
    // they have.
    pub fn cursor_begin(&mut self) {
        self.cursor_events.clear();
    }

    pub fn cursor_end(&mut self) {
        self.cursor_events.clear();
    }

    fn run_block_events(&mut self) {
        let mut reschedule = Vec::new();
        while let Some(event) = self.block_events.pop_front() {
            if !event.ghost {
                self.block_event_set.remove(&event);
            }
            if self.ticks_at(event.pos) {
                let state = self.get(event.pos);
                if self.block(state) == event.block {
                    blocks::trigger_event(self, state, event.pos, event.a, event.b);
                }
            } else {
                reschedule.push(event);
            }
        }
        for event in reschedule {
            self.block_event(event.pos, event.block, event.a, event.b);
        }
    }

    // McbenchLevel.tick's entities: each in the tick list, in its order
    // (an entity added during the pass waits for the next).
    fn tick_entities(&mut self) {
        let end = self.entities.tick_list.begin();
        let mut at = 0;
        while let Some(id) = self.entities.tick_list.next(&mut at, end) {
            let mut e = self.entities.all.remove(&id).expect("a ticking entity");
            if !e.removed {
                entity::tick_non_passenger(self, &mut e);
            }
            if !e.removed {
                self.entities.all.insert(id, e);
            }
        }
        self.entities.tick_list.finish();
    }

    // McbenchLevel.tick: one server tick's parts that reach blocks.
    // One client tick of the level's player (crate::player::Player::tick): the
    // client ticks its entities before its block entities, so a front end calls
    // this and then `tick`.
    pub fn player_tick(&mut self, input: crate::player::Input) {
        let mut player = self.player.take().expect("a level without a player");
        player.tick(self, input);
        self.player = Some(player);
    }

    pub fn tick(&mut self) {
        self.handling_tick = true;
        self.sky_darken = crate::light::sky_darken(crate::light::DAY_TIME);
        self.game_time += 1;
        let ticking = &self.ticking;
        self.block_ticks.collect(self.game_time, MAX_BLOCK_TICKS, |chunk| ticking.contains(&chunk));
        while let Some(tick) = self.block_ticks.next_to_run() {
            let state = self.get(tick.pos);
            if self.block(state) == tick.kind {
                blocks::tick(self, state, tick.pos);
            }
        }
        // ServerLevel.tickFluid.
        let ticking = &self.ticking;
        self.fluid_ticks.collect(self.game_time, MAX_FLUID_TICKS, |chunk| ticking.contains(&chunk));
        while let Some(tick) = self.fluid_ticks.next_to_run() {
            let state = self.get(tick.pos);
            let fluid = self.fluid_of(state);
            if self.fluid_type(fluid) == tick.kind {
                crate::fluid::tick(self, tick.pos, state, fluid);
            }
        }
        self.random.next_long();
        self.run_block_events();
        self.handling_tick = false;
        self.tick_entities();
        self.tick_block_entities();
        // Level.tickBlockEntities ends by resetting the count of primed TNT ticks.
        self.current_primed_tnt = 0;
    }
}
