// PowderSnowBlock: an entity inside it is slowed and freezes, a burning one
// breaks it, and its collision shape depends on the entity
// (crate::collision). Its fallOn only makes a sound for a living entity
// that falls four blocks or more, and does not hurt the fall.

use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::inside::{self, Applier, Effect};
use crate::entity::{Entity, Kind};
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { entity_inside: Some(entity_inside), fall_on: Some(fall_on), all_entities: true, ..NONE };

// PowderSnowBlock.entityInside. EntityInsideBlockEvent has no listener. An
// entity that is not living (every kind but the armor stand), or a living
// one that was not already inside this block when its tick began
// (Entity.getInBlockState), is stuck in the snow; a burning entity breaks
// the block before it is extinguished, and the entity freezes.
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut Entity, applier: &mut Applier, _past_edges: bool) {
    let living = matches!(entity.kind, Kind::ArmorStand(_));
    let stuck = !living || {
        let in_state = crate::entity::armor_stand::in_block_state(l, entity);
        l.block(in_state) == l.block(state)
    };
    if stuck {
        // (The particles are the client's.)
        entity.make_stuck_in_block(Vec3::new(0.8999999761581421, 1.5, 0.8999999761581421));
    }
    applier.run_before(inside::EXTINGUISH, Effect::PowderSnowBreak(pos));
    applier.apply(inside::FREEZE);
    applier.apply(inside::EXTINGUISH);
}

// PowderSnowBlock.fallOn: the fall sound for a living entity that fell four
// blocks or more, which reaches no one; Block.fallOn is not called.
fn fall_on(_l: &mut Level, _state: State, _pos: Pos, _entity: &mut Entity, _fall_distance: f64) {}
