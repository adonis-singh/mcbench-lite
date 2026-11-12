// HoneyBlock: a block that sticks to what a piston moves with it, but not to
// slime blocks (PistonStructureResolver, piston_resolver.rs), and slows what
// slides down its sides (entityInside), for every kind of entity the port
// has. Its fallOn only makes sounds and an entity event, and hurts as
// Block.fallOn does (Entity.causeFallDamage with another multiplier, which
// no kind reads).

use super::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::{motion, Entity, Kind};
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { fall_on: Some(fall_on), entity_inside: Some(entity_inside), all_entities: true, ..NONE };

// HoneyBlock.fallOn.
fn fall_on(l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, fall_distance: f64) {
    motion::cause_fall_damage(l, entity, fall_distance);
}

// HoneyBlock.getOldDeltaY and getNewDeltaY (0.98F, widened).
fn old_delta_y(delta_y: f64) -> f64 {
    delta_y / 0.98f32 as f64 + 0.08
}

fn new_delta_y(delta_y: f64) -> f64 {
    (delta_y - 0.08) * 0.98f32 as f64
}

// HoneyBlock.isSlidingDown.
fn is_sliding_down(pos: Pos, entity: &Entity) -> bool {
    if entity.on_ground {
        false
    } else if entity.pos.y > pos.y() as f64 + 0.9375 - 1.0E-7 {
        false
    } else if old_delta_y(entity.delta.y) >= -0.08 {
        false
    } else {
        let abs = (pos.x() as f64 + 0.5 - entity.pos.x).abs();
        let abs1 = (pos.z() as f64 + 0.5 - entity.pos.z).abs();
        let d = 0.4375 + (entity.width / 2.0f32) as f64;
        abs + 1.0E-7 > d || abs1 + 1.0E-7 > d
    }
}

// HoneyBlock.entityInside (the EntityInsideBlockEvent has no listener):
// HoneyBlock.doSlideMovement and maybeDoSlideEffects.
fn entity_inside(l: &mut Level, _state: State, pos: Pos, entity: &mut Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if is_sliding_down(pos, entity) {
        let delta = entity.delta;
        if old_delta_y(delta.y) < -0.13 {
            let d = -0.05 / old_delta_y(delta.y);
            entity.delta = Vec3::new(delta.x * d, new_delta_y(-0.05), delta.z * d);
        } else {
            entity.delta = Vec3::new(delta.x, new_delta_y(-0.05), delta.z);
        }
        entity.fall_distance = 0.0;
        // doesEntityDoHoneyBlockSlideEffects: primed TNT (of these kinds); its
        // two draws decide a sound and an entity event.
        if matches!(entity.kind, Kind::Tnt(_)) {
            l.random.next_int_bound(5);
            l.random.next_int_bound(5);
        }
    }
}
