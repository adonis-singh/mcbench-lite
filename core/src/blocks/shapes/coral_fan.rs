// CoralFanBlock and CoralPlantBlock (the same code).

use super::coral;
use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(coral::update_shape),
    on_place: Some(on_place),
    tick: Some(coral::tick),
    can_survive: Some(coral::can_survive),
    ..NONE
};

// CoralFanBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    coral::try_schedule_die_tick(l, state, pos);
}
