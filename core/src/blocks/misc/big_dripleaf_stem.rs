// BigDripleafStemBlock: holds the leaf up; a stem that loses its neighbour
// breaks on a scheduled tick.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), tick: Some(tick), can_survive: Some(can_survive), ..NONE };

// BigDripleafStemBlock.canSurvive: (the block below is a stem or a
// placeable block) && (the block above is a stem or a leaf).
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    let above = l.get(pos.above());
    (l.block(below) == l.block(state) || in_tag!(l, below, "minecraft:big_dripleaf_placeable")) && (l.block(above) == l.block(state) || is_block!(l, above, "minecraft:big_dripleaf"))
}

// BigDripleafStemBlock.updateShape: a waterlogged state schedules a water
// tick after the stem's own tick.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if (direction == Dir::Down || direction == Dir::Up) && !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    schedule_water_tick(l, state, pos);
    state
}

// BigDripleafStemBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}
