// WebBlock: an entity inside it is stuck (Entity.makeStuckInBlock).

use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::Entity;
use crate::level::Level;
use crate::phys::Vec3;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { entity_inside: Some(entity_inside), all_entities: true, ..NONE };

// WebBlock.entityInside: the event has no listener; no entity here (an armor
// stand is the only living one) has the weaving effect, so the multiplier is
// (0.25, 0.05F, 0.25).
fn entity_inside(_l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    entity.make_stuck_in_block(Vec3::new(0.25, 0.05f32 as f64, 0.25));
}
