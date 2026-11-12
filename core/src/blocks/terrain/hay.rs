// HayBlock: fallOn hurts the fall less (Entity.causeFallDamage with a
// multiplier of 0.2, which no kind of entity here reads).

use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::{motion, Entity};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { fall_on: Some(fall_on), all_entities: true, ..NONE };

// HayBlock.fallOn.
fn fall_on(l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, fall_distance: f64) {
    motion::cause_fall_damage(l, entity, fall_distance);
}
