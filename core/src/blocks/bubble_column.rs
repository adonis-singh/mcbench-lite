// BubbleColumnBlock, and what soul sand and magma blocks do to the water
// above them (SoulSandBlock, MagmaBlock: they tick the column above after
// twenty ticks whenever water meets them).

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::fluid::{named, WATER_TICK_DELAY};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::registry::{Fluid, State};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// SoulSandBlock and MagmaBlock: the same onPlace, updateShape and tick.
pub const COLUMN_BASE_BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(base_update_shape), on_place: Some(base_on_place), tick: Some(base_tick), ..NONE };

// BubbleColumnBlock.canExistIn: a bubble column, or a source water block.
fn can_exist_in(l: &Level, state: State) -> bool {
    let block = l.block(state);
    let known = named(l);
    if block == known.bubble_column {
        return true;
    }
    if block != known.water {
        return false;
    }
    let fluid = &l.reg.fluids[l.fluid_of(state) as usize];
    fluid.amount >= 8 && fluid.source
}

// BubbleColumnBlock.getColumnState: a column rises from soul sand, falls
// onto magma, and is water above anything else.
fn column_state(l: &Level, state: State) -> State {
    let known = named(l);
    let block = l.block(state);
    let drag = l.reg.property_of(l.reg.default_state(known.bubble_column), "drag");
    if block == known.bubble_column {
        state
    } else if block == known.soul_sand {
        l.reg.with_bool(l.reg.default_state(known.bubble_column), drag, false)
    } else if block == known.magma_block {
        l.reg.with_bool(l.reg.default_state(known.bubble_column), drag, true)
    } else {
        l.reg.default_state(known.water)
    }
}

// BubbleColumnBlock.updateColumn(level, pos, fluid, state).
pub fn update_column(l: &mut Level, pos: Pos, fluid: State, state: State) {
    if can_exist_in(l, fluid) {
        let column = column_state(l, state);
        l.set(pos, column, UPDATE_CLIENTS);
        let mut at = pos.above();
        loop {
            let there = l.get(at);
            if !can_exist_in(l, there) {
                break;
            }
            if !l.set(at, column, UPDATE_CLIENTS) {
                return;
            }
            at = at.above();
        }
    }
}

// BubbleColumnBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let below = l.get(pos.below());
    update_column(l, pos, state, below);
}

// BubbleColumnBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
    let known = named(l);
    if !can_survive(l, state, pos) || direction == Dir::Down || direction == Dir::Up && l.block(neighbor) != known.bubble_column && can_exist_in(l, neighbor) {
        l.schedule_tick(pos, l.block(state), 5, PRIORITY_NORMAL);
    }
    state
}

// BubbleColumnBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    let known = named(l);
    let block = l.block(below);
    block == known.bubble_column || block == known.magma_block || block == known.soul_sand
}

// SoulSandBlock.updateShape and MagmaBlock.updateShape: water above it
// schedules the column's check.
fn base_update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Up && l.block(neighbor) == named(l).water {
        l.schedule_tick(pos, l.block(state), 20, PRIORITY_NORMAL);
    }
    state
}

// SoulSandBlock.onPlace and MagmaBlock.onPlace.
fn base_on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    l.schedule_tick(pos, l.block(state), 20, PRIORITY_NORMAL);
}

// SoulSandBlock.tick and MagmaBlock.tick: BubbleColumnBlock.updateColumn(
// level, pos.above(), state).
fn base_tick(l: &mut Level, state: State, pos: Pos) {
    let above = pos.above();
    let fluid = l.get(above);
    update_column(l, above, fluid, state);
}
