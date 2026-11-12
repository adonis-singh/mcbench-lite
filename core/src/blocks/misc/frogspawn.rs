// FrogspawnBlock: lives only on water. onPlace schedules the hatching
// with a delay drawn from the level's random; the tick destroys the block or
// hatches tadpoles (an entity spawn, which refuses). entityInside only
// destroys it for a falling block.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::inside::Applier;
use crate::entity::{Entity, Kind};
use crate::level::{refuse, Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    can_survive: Some(can_survive),
    on_place: Some(on_place),
    tick: Some(tick),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// FrogspawnBlock.entityInside: a falling block destroys it (Level.destroyBlock
// without drops).
fn entity_inside(l: &mut Level, _state: State, pos: Pos, entity: &mut Entity, _applier: &mut Applier, _past_edges: bool) {
    if matches!(entity.kind, Kind::Falling(_)) {
        l.destroy_block_by(pos, false, 512, None);
    }
}

// FrogspawnBlock.minHatchTickDelay and maxHatchTickDelay.
const MIN_HATCH_TICK_DELAY: i32 = 3600;
const MAX_HATCH_TICK_DELAY: i32 = 12000;

// FrogspawnBlock.canSurvive: mayPlaceOn(level, pos.below()) is water below
// and no fluid in the block's own position.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    // mayPlaceOn(level, pos.below()): water source there and nothing at pos.
    let fluid = l.fluid_state(pos.below());
    let above = l.fluid_state(pos);
    l.reg.fluids[fluid as usize].fluid == crate::registry::Fluid::Water && above == crate::registry::EMPTY_FLUID
}

// FrogspawnBlock.onPlace: getFrogspawnHatchDelay is
// random.nextInt(minHatchTickDelay, maxHatchTickDelay).
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    let delay = l.random.next_int_bound(MAX_HATCH_TICK_DELAY - MIN_HATCH_TICK_DELAY) + MIN_HATCH_TICK_DELAY;
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
}

// FrogspawnBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// FrogspawnBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, false, 512);
    } else {
        refuse(format!("hatching frogspawn at {pos}"));
    }
}
