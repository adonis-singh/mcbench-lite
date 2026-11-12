// LayeredCauldronBlock (the water and powder snow cauldrons) and
// LavaCauldronBlock: their analog outputs and what an entity inside does.
// CauldronBlock overrides nothing this level reaches (its output is Block's
// 0). Precipitation and random ticks do not run here, and a cauldron's tick is
// scheduled only by a pointed dripstone.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::inside::{self, Applier, Effect};
use crate::entity::Entity;
use crate::game_event::Context;
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{Shape, State};
use crate::{block_id, shape_ops};

pub const LAYERED: Behaviour = Behaviour {
    get_analog_output_signal: Some(layered_analog_output_signal),
    entity_inside: Some(layered_entity_inside),
    all_entities: true,
    ..NONE
};
pub const LAVA: Behaviour = Behaviour {
    get_analog_output_signal: Some(lava_analog_output_signal),
    entity_inside: Some(lava_entity_inside),
    all_entities: true,
    ..NONE
};

// LayeredCauldronBlock.LEVEL.
fn level_property(l: &Level) -> u16 {
    property!(l, "level", ["1", "2", "3"])
}

// LayeredCauldronBlock.getAnalogOutputSignal.
fn layered_analog_output_signal(l: &mut Level, state: State, _pos: Pos, _direction: Dir) -> i32 {
    l.reg.get_int(state, level_property(l))
}

// LavaCauldronBlock.getAnalogOutputSignal.
fn lava_analog_output_signal(_l: &mut Level, _state: State, _pos: Pos, _direction: Dir) -> i32 {
    3
}

// LayeredCauldronBlock.getEntityInsideCollisionShape: FILLED_SHAPES[level - 1],
// the cauldron's collision shape (AbstractCauldronBlock.SHAPE) and a column
// (12 wide, from 4) up to 6 + 3 * level sixteenths.
pub fn layered_inside_shape(l: &Level, state: State) -> Shape {
    let level = l.reg.get_int(state, level_property(l));
    shape_ops::or(&hollow(l, state), &shape_ops::block_box(2.0, 4.0, 2.0, 14.0, 6.0 + level as f64 * 3.0, 14.0))
}

// LavaCauldronBlock.FILLED_SHAPE: SHAPE or SHAPE_INSIDE, a column from 4 to 15.
pub fn lava_inside_shape(l: &Level, state: State) -> Shape {
    shape_ops::or(&hollow(l, state), &shape_ops::block_box(2.0, 4.0, 2.0, 14.0, 15.0, 14.0))
}

// AbstractCauldronBlock.SHAPE, which is the cauldron's collision shape.
fn hollow(l: &Level, state: State) -> Shape {
    l.reg.shapes[l.reg.states[state as usize].collision_shape as usize].clone()
}

// LayeredCauldronBlock.entityInside: EntityInsideBlockEvent has no listener;
// the server level runs, before the entity is extinguished, the lowering of
// the fill level for an entity that is on fire.
fn layered_entity_inside(l: &mut Level, state: State, pos: Pos, _entity: &mut Entity, applier: &mut Applier, _past_edges: bool) {
    applier.run_before(inside::EXTINGUISH, Effect::CauldronExtinguish(pos, state));
}

// The runBefore of LayeredCauldronBlock.entityInside, once the entity burns
// (the game rule mobGriefing is on and nothing but a player is barred by
// mayInteract): handleEntityOnFireInside lowers the fill level, a powder snow
// cauldron's as the water cauldron of that level. Whether it did.
pub fn handle_entity_on_fire_inside(l: &mut Level, state: State, pos: Pos) -> bool {
    let level = l.reg.get_int(state, level_property(l));
    let base = if l.reg.blocks[l.block(state) as usize].key == "minecraft:powder_snow_cauldron" {
        let water = l.reg.default_state(block_id!(l, "minecraft:water_cauldron"));
        l.reg.with_int(water, level_property(l), level)
    } else {
        state
    };
    // lowerFillLevel.
    let lowered = level - 1;
    let target = if lowered == 0 { l.reg.default_state(block_id!(l, "minecraft:cauldron")) } else { l.reg.with_int(base, level_property(l), lowered) };
    // changeLevel: CauldronLevelChangeEvent has no listener; the state is placed
    // with flags 3, then BLOCK_CHANGE.
    l.set(pos, target, UPDATE_ALL);
    l.game_event_in(crate::game_event!(l, "block_change"), pos, Context::of_state(target));
    true
}

// LavaCauldronBlock.entityInside: CLEAR_FREEZE, LAVA_IGNITE, and after the
// ignition the lava's hurt.
fn lava_entity_inside(_l: &mut Level, _state: State, pos: Pos, _entity: &mut Entity, applier: &mut Applier, _past_edges: bool) {
    applier.apply(inside::CLEAR_FREEZE);
    applier.apply(inside::LAVA_IGNITE);
    applier.run_after(inside::LAVA_IGNITE, Effect::LavaHurt(pos));
}
