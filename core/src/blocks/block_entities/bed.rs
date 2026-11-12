// BedBlock and BedBlockEntity. The two halves keep each other: updateShape
// toward the other half copies its occupied flag, or the bed breaks when
// the other half is not the other half of this bed. An entity that lands on a
// bed bounces. setPlacedBy (which places the head) is not reached by a
// setblock command.

use super::Plain;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::Entity;
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    new_block_entity: Some(new_block_entity),
    fall_on: Some(fall_on),
    update_entity_movement_after_fall_on: Some(update_entity_movement_after_fall_on),
    all_entities: true,
    ..NONE
};

// BedBlock.PART (head or foot), OCCUPIED, and HorizontalDirectionalBlock.FACING.
fn part(l: &Level) -> u16 {
    property!(l, "part", ["head", "foot"])
}

fn occupied(l: &Level) -> u16 {
    property!(l, "occupied", ["true", "false"])
}

fn is_head(l: &Level, state: State) -> bool {
    l.reg.value_name(state, part(l)) == "head"
}

// BedBlock.getNeighbourDirection.
fn neighbour_direction(l: &Level, state: State) -> Dir {
    let facing = crate::blocks::shapes::common::get_facing(l, state);
    if is_head(l, state) {
        facing.opposite()
    } else {
        facing
    }
}

// BedBlock.updateShape: toward the other half the occupied flag is the other
// half's, if that is a bed of this block in the other part; else the bed is
// gone. (BlockBehaviour.updateShape keeps the state for other directions.)
fn update_shape(l: &mut Level, state: State, _pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if direction != neighbour_direction(l, state) {
        return state;
    }
    if l.block(neighbor) == l.block(state) && l.reg.get(neighbor, part(l)) != l.reg.get(state, part(l)) {
        l.reg.with_bool(state, occupied(l), l.reg.get_bool(neighbor, occupied(l)))
    } else {
        l.known.air
    }
}

// BedBlock.newBlockEntity.
fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Plain(Plain::Bed)
}

// BedBlock.fallOn: Block.fallOn with half the distance, which is
// Entity.causeFallDamage.
fn fall_on(l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, fall_distance: f64) {
    crate::entity::motion::cause_fall_damage(l, entity, fall_distance * 0.5);
}

// BedBlock.updateEntityMovementAfterFallOn: nothing here sneaks (Entity.
// isSuppressingBounce is isShiftKeyDown), so bounceUp: a downward motion
// rebounds at 0.66 of its speed, times 1.0 for a living entity (the armor
// stand) and 0.8 for the others.
fn update_entity_movement_after_fall_on(_l: &mut Level, entity: &mut Entity) {
    let delta = entity.delta;
    if delta.y < 0.0 {
        let factor = if matches!(entity.kind, crate::entity::Kind::ArmorStand(_)) { 1.0 } else { 0.8 };
        entity.delta = crate::phys::Vec3::new(delta.x, -delta.y * 0.66f32 as f64 * factor, delta.z);
    }
}
