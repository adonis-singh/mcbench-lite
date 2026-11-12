// LiquidBlock: water and lava. Their fluid state follows from the block's
// level (the dump has it per state); this is what the block does when it is
// placed, nudged or reshaped, and where lava meets water.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::fluid::tick_delay;
use crate::level::{Level, UPDATE_ALL};
use crate::registry::{BlockId, Fluid, State};
use std::sync::OnceLock;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), neighbor_changed: Some(neighbor_changed), on_place: Some(on_place), ..NONE };

// The blocks lava makes or reads.
struct Blocks {
    soul_soil: BlockId,
    blue_ice: BlockId,
    obsidian: State,
    cobblestone: State,
    basalt: State,
}

fn blocks(l: &Level) -> &'static Blocks {
    static BLOCKS: OnceLock<Blocks> = OnceLock::new();
    BLOCKS.get_or_init(|| {
        let reg = l.reg;
        Blocks {
            soul_soil: reg.block_key("minecraft:soul_soil"),
            blue_ice: reg.block_key("minecraft:blue_ice"),
            obsidian: reg.default_state(reg.block_key("minecraft:obsidian")),
            cobblestone: reg.default_state(reg.block_key("minecraft:cobblestone")),
            basalt: reg.default_state(reg.block_key("minecraft:basalt")),
        }
    })
}

// LiquidBlock.updateShape: a source here or beyond reschedules the fluid.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let fluid = l.fluid_of(state);
    let neighbor_fluid = l.fluid_of(neighbor);
    if l.reg.fluids[fluid as usize].source || l.reg.fluids[neighbor_fluid as usize].source {
        let kind = l.fluid_type(fluid);
        l.schedule_fluid_tick(pos, kind, tick_delay(kind));
    }
    state
}

// LiquidBlock.getFlowSpeed: water next to lava flows at Paper's
// water-over-lava-flow-speed (5), else at its fluid's tick delay.
fn flow_speed(l: &mut Level, kind: Fluid, pos: Pos) -> i32 {
    // Paper's environment.water-over-lava-flow-speed.
    const WATER_OVER_LAVA_FLOW_SPEED: i32 = 5;
    let is_lava = |l: &mut Level, at: Pos| l.fluid_if_loaded(at).map_or(false, |f| l.fluid_type(f).is_lava());
    if !kind.is_water() || (!is_lava(l, pos.relative(Dir::North)) && !is_lava(l, pos.relative(Dir::South)) && !is_lava(l, pos.relative(Dir::West)) && !is_lava(l, pos.relative(Dir::East))) {
        tick_delay(kind)
    } else {
        WATER_OVER_LAVA_FLOW_SPEED
    }
}

// LiquidBlock.shouldSpreadLiquid: lava next to water (or, on soul soil, blue
// ice) turns to stone and does not spread.
fn should_spread_liquid(l: &mut Level, pos: Pos, state: State) -> bool {
    let fluid = l.fluid_of(state);
    if l.fluid_type(fluid).is_lava() {
        let known = blocks(l);
        let below = l.get(pos.below());
        let is_soul_soil = l.block(below) == known.soul_soil;
        // POSSIBLE_FLOW_DIRECTIONS: down, south, north, east, west; the
        // neighbour read is on the opposite side.
        for direction in [Dir::Down, Dir::South, Dir::North, Dir::East, Dir::West] {
            let at = pos.relative(direction.opposite());
            let there = l.fluid_state(at);
            if l.fluid_type(there).is_water() {
                let here = l.fluid_state(pos);
                let block = if l.reg.fluids[here as usize].source { known.obsidian } else { known.cobblestone };
                // BlockFormEvent, then fizz (a level event that reaches nobody).
                l.set(pos, block, UPDATE_ALL);
                return false;
            }
            if is_soul_soil {
                let neighbor = l.get(at);
                if l.block(neighbor) == known.blue_ice {
                    l.set(pos, known.basalt, UPDATE_ALL);
                    return false;
                }
            }
        }
    }
    true
}

// LiquidBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    if should_spread_liquid(l, pos, state) {
        let fluid = l.fluid_of(state);
        let kind = l.fluid_type(fluid);
        let delay = flow_speed(l, kind, pos);
        l.schedule_fluid_tick(pos, kind, delay);
    }
}

// LiquidBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor_block: BlockId, _moved: bool) {
    on_place(l, state, pos, state, false);
}
