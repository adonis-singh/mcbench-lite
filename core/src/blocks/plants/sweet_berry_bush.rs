// SweetBerryBushBlock: entityInside sticks a living entity (the armor stand; no
// fox or bee is here) in the bush, and hurts it if the bush has berries and
// the entity moved.

use crate::blocks::vegetation::PLAIN;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::entity::damage::Dmg;
use crate::entity::inside::Applier;
use crate::entity::{Entity, Kind};
use crate::level::Level;
use crate::phys::Vec3;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { entity_inside: Some(entity_inside), all_entities: true, ..PLAIN };

// SweetBerryBushBlock.AGE.
fn age(l: &Level) -> u16 {
    property!(l, "age", ["0", "1", "2", "3"])
}

// SweetBerryBushBlock.entityInside (EntityInsideBlockEvent has no listener).
fn entity_inside(l: &mut Level, state: State, _pos: Pos, entity: &mut Entity, _applier: &mut Applier, _past_edges: bool) {
    // (An armor stand is the only LivingEntity.)
    if !matches!(entity.kind, Kind::ArmorStand(_)) {
        return;
    }
    entity.make_stuck_in_block(Vec3::new(0.800000011920929, 0.75, 0.800000011920929));
    if l.reg.get_int(state, age(l)) != 0 {
        // Not client authoritative: the way it came.
        let moved = entity.old_position().subtract(entity.pos);
        if moved.horizontal_distance_sqr() > 0.0 && (moved.x.abs() >= 0.003000000026077032 || moved.z.abs() >= 0.003000000026077032) {
            crate::entity::hurt(l, entity, Dmg::SweetBerryBush, 1.0f32);
        }
    }
}
