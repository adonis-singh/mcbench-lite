// SpongeBlock and WetSpongeBlock. A sponge soaks up the water around it:
// a breadth first search over a BlockStateListPopulator (a level view that
// captures its setBlock calls), then the captured blocks are placed in the
// level in the order they were last captured, after the water blocks that
// can be picked up are picked up in the level itself.
//
// The populator is stood in for by writing the captured states into the
// world for the length of the search (the search reads them back as the
// populator answers them) and restoring the world afterwards.

use super::special;
use super::{Behaviour, NONE};
use crate::dir::{Pos, ALL};
use crate::fluid::named;
use crate::level::{refuse, Level, UPDATE_ALL, UPDATE_CLIENTS};
use crate::registry::{BlockId, State, BLOCK_ENTITY};
use std::collections::{HashSet, VecDeque};

pub const BEHAVIOUR: Behaviour = Behaviour { on_place: Some(on_place), neighbor_changed: Some(neighbor_changed), ..NONE };
pub const WET_BEHAVIOUR: Behaviour = Behaviour { on_place: Some(wet_on_place), ..NONE };

// BlockStateListPopulator: the captured blocks (position, state, flags) in
// the order of their last capture, and the states the world held.
struct Populator {
    captured: Vec<(Pos, State, i32)>,
    original: Vec<(Pos, State)>,
}

impl Populator {
    // BlockStateListPopulator.setBlock.
    fn set(&mut self, l: &mut Level, pos: Pos, state: State, flags: i32) {
        self.captured.retain(|&(at, _, _)| at != pos);
        if !self.original.iter().any(|&(at, _)| at == pos) {
            let before = l.get(pos);
            self.original.push((pos, before));
        }
        l.world.set(pos, state);
        self.captured.push((pos, state, flags));
    }

    fn restore(&mut self, l: &mut Level) {
        for (pos, state) in self.original.drain(..) {
            l.world.set(pos, state);
        }
    }
}

// Where a block change goes: the level, or the populator.
enum Sink<'a> {
    Level,
    Populator(&'a mut Populator),
}

fn set_block(l: &mut Level, sink: &mut Sink, pos: Pos, state: State, flags: i32) {
    match sink {
        Sink::Level => {
            l.set_block(pos, state, flags, 512);
        }
        Sink::Populator(populator) => populator.set(l, pos, state, flags),
    }
}

// LevelAccessor.destroyBlock(pos, true) (the populator's: the block it
// holds is replaced by the fluid's legacy block).
fn destroy_block(l: &mut Level, sink: &mut Sink, pos: Pos) {
    match sink {
        Sink::Level => {
            l.destroy_block(pos, true, 512);
        }
        Sink::Populator(populator) => {
            let state = l.get(pos);
            if !l.is_air(state) {
                let fluid = l.fluid_of(state);
                let legacy = l.reg.fluids[fluid as usize].legacy;
                populator.set(l, pos, legacy, UPDATE_ALL);
            }
        }
    }
}

fn supers(l: &Level, state: State, name: &str) -> bool {
    l.reg.blocks[l.block(state) as usize].supers.iter().any(|s| s == name)
}

// BucketPickup.pickupBlock(null, level, pos, state) for a block that is one:
// whether it gave a bucket (the result is not empty).
fn pickup_block(l: &mut Level, sink: &mut Sink, pos: Pos, state: State) -> bool {
    let class = l.reg.blocks[l.block(state) as usize].class.as_str();
    match class {
        "LiquidBlock" => {
            // LiquidBlock.pickupBlock: a source block.
            if l.reg.get_int(state, l.reg.property_of(state, "level")) == 0 {
                set_block(l, sink, pos, l.known.air, 11);
                true
            } else {
                false
            }
        }
        "BubbleColumnBlock" => {
            set_block(l, sink, pos, l.known.air, 11);
            true
        }
        _ if supers(l, state, "SimpleWaterloggedBlock") => {
            if l.reg.blocks[l.block(state) as usize].overrides.iter().any(|name| name == "pickupBlock") || class == "BarrierBlock" {
                refuse(format!("{class}.pickupBlock ({})", l.reg.describe(state)));
            }
            // SimpleWaterloggedBlock.pickupBlock.
            let waterlogged = l.reg.property_of(state, "waterlogged");
            if l.reg.get_bool(state, waterlogged) {
                set_block(l, sink, pos, l.reg.with_bool(state, waterlogged, false), UPDATE_ALL);
                if !super::can_survive(l, state, pos) {
                    destroy_block(l, sink, pos);
                }
                true
            } else {
                false
            }
        }
        _ => refuse(format!("{class}.pickupBlock ({})", l.reg.describe(state))),
    }
}

// Whether the block is one the sponge takes plants from by hand (kelp and
// seagrass).
fn is_soakable_plant(l: &Level, state: State) -> bool {
    let known = named(l);
    let block = l.block(state);
    block == known.kelp || block == known.kelp_plant || block == known.seagrass || block == known.tall_seagrass
}

// SpongeBlock.removeWaterBreadthFirstSearch.
fn remove_water_breadth_first_search(l: &mut Level, origin: Pos) -> bool {
    let mut populator = Populator { captured: Vec::new(), original: Vec::new() };
    // BlockPos.breadthFirstTraversal(origin, 6, 65, ...).
    let mut queue: VecDeque<(Pos, i32)> = VecDeque::new();
    let mut seen: HashSet<i64> = HashSet::new();
    queue.push_back((origin, 0));
    let mut accepted = 0;
    while let Some((pos, depth)) = queue.pop_front() {
        if !seen.insert(pos.as_long()) {
            continue;
        }
        // The action: whether to accept the block (and go on from it).
        let accept = if pos == origin {
            true
        } else {
            let state = l.get(pos);
            let fluid = l.fluid_state(pos);
            if !l.fluid_type(fluid).is_water() {
                false
            } else if supers(l, state, "BucketPickup") && pickup_block(l, &mut Sink::Populator(&mut populator), pos, state) {
                true
            } else if l.reg.blocks[l.block(state) as usize].class == "LiquidBlock" {
                populator.set(l, pos, l.known.air, UPDATE_ALL);
                true
            } else if !is_soakable_plant(l, state) {
                false
            } else {
                populator.set(l, pos, l.known.air, UPDATE_ALL);
                true
            }
        };
        if accept {
            accepted += 1;
            if accepted >= 65 {
                break;
            }
            if depth < 6 {
                for dir in ALL {
                    queue.push_back((pos.relative(dir), depth + 1));
                }
            }
        }
    }
    populator.restore(l);
    if populator.captured.is_empty() {
        return false;
    }
    // SpongeAbsorbEvent has no listener. The snapshots are placed in order.
    // getSnapshotBlocks makes a CraftBlockEntityState of each captured block
    // that has a block entity first, saving that entity's data.
    let captured_blocks = std::mem::take(&mut populator.captured);
    let saved: Vec<Option<special::Special>> = captured_blocks
        .iter()
        .map(|&(pos, captured, _)| if l.reg.flags(captured) & BLOCK_ENTITY != 0 { special::snapshot_listener(l, pos) } else { None })
        .collect();
    for ((pos, captured, flags), saved) in captured_blocks.into_iter().zip(saved) {
        let state = l.get(pos);
        let fluid = l.fluid_state(pos);
        if l.fluid_type(fluid).is_water()
            && (!supers(l, state, "BucketPickup") || !pickup_block(l, &mut Sink::Level, pos, state))
            && l.reg.blocks[l.block(state) as usize].class != "LiquidBlock"
            && is_soakable_plant(l, state)
        {
            if l.reg.flags(state) & BLOCK_ENTITY != 0 {
                l.block_entity(pos);
            }
            if l.is_air(captured) {
                crate::entity::item::drop_resources(l, state, pos);
            }
        }
        l.set_block(pos, captured, flags, 512);
        // CraftBlockEntityState.place: the block entity now at the position
        // gets the saved data back, then setChanged.
        if l.reg.flags(captured) & BLOCK_ENTITY != 0 && l.block_entities.contains_key(&pos) {
            special::apply_snapshot(l, pos, saved);
            super::container::be_changed(l, pos);
        }
    }
    true
}

// SpongeBlock.tryAbsorbWater.
fn try_absorb_water(l: &mut Level, pos: Pos) {
    if remove_water_breadth_first_search(l, pos) {
        let wet = l.reg.default_state(named(l).wet_sponge);
        l.set(pos, wet, UPDATE_CLIENTS);
        // playSound: the seed comes from a random the port does not track.
    }
}

// SpongeBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        try_absorb_water(l, pos);
    }
}

// SpongeBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, _state: State, pos: Pos, _neighbor_block: BlockId, _moved: bool) {
    try_absorb_water(l, pos);
}

// WetSpongeBlock.onPlace: it dries only in a dimension that is ultra warm.
fn wet_on_place(_l: &mut Level, _state: State, _pos: Pos, _old: State, _moved: bool) {}
