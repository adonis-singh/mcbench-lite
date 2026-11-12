// WitherRoseBlock: netherrack, soul sand and soul soil as well as dirt.
// entityInside gives a living entity the wither effect, unless the level is
// peaceful, which this one is.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::is_block;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { entity_inside: Some(entity_inside), all_entities: true, ..vegetation::behaviour(can_survive) };

fn entity_inside(_l: &mut Level, _state: State, _pos: Pos, _entity: &mut crate::entity::Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {}

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// WitherRoseBlock.mayPlaceOn.
fn may_place_on(l: &mut Level, state: State, _pos: Pos) -> bool {
    vegetation::may_place_on(l, state) || is_block!(l, state, "minecraft:netherrack") || is_block!(l, state, "minecraft:soul_sand") || is_block!(l, state, "minecraft:soul_soil")
}
