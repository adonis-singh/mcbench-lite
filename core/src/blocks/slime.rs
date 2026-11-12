// SlimeBlock: a block that sticks to what a piston moves with it
// (PistonStructureResolver, piston_resolver.rs), and bounces what falls on
// it and slows what walks on it (fallOn, updateEntityMovementAfterFallOn,
// stepOn), for every kind of entity the port has: items, falling blocks,
// primed TNT, item frames and armor stands. None of them sneaks
// (isSuppressingBounce, isSteppingCarefully are false).

use super::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::{motion, Entity};
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    fall_on: Some(fall_on),
    update_entity_movement_after_fall_on: Some(update_entity_movement_after_fall_on),
    step_on_mut: Some(step_on),
    all_entities: true,
    ..NONE
};

// SlimeBlock.fallOn: entity.causeFallDamage(fallDistance, 0.0F, ...).
fn fall_on(l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, fall_distance: f64) {
    motion::cause_fall_damage(l, entity, fall_distance);
}

// SlimeBlock.updateEntityMovementAfterFallOn: bounceUp, by 1.0 for a living
// entity (an armor stand) and 0.8 for the others.
fn update_entity_movement_after_fall_on(_l: &mut Level, entity: &mut Entity) {
    let delta = entity.delta;
    if delta.y < 0.0 {
        let factor = if matches!(entity.kind, crate::entity::Kind::ArmorStand(_)) { 1.0 } else { 0.8 };
        entity.delta = Vec3::new(delta.x, -delta.y * factor, delta.z);
    }
}

// SlimeBlock.stepOn: what is not falling fast is slowed along the ground.
fn step_on(_l: &mut Level, _pos: Pos, _state: State, entity: &mut Entity) {
    let abs = entity.delta.y.abs();
    if abs < 0.1 {
        let d = 0.4 + abs * 0.2;
        entity.delta = entity.delta.multiply(d, 1.0, d);
    }
}
