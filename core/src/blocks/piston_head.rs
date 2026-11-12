// PistonHeadBlock.

use super::piston::{self, extended, facing, piston_type};
use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    can_survive: Some(can_survive),
    ..NONE
};

// PistonHeadBlock.isFittingBase: the extended piston of the head's type
// facing as the head does.
fn is_fitting_base(l: &Level, base_state: State, extended_state: State) -> bool {
    let block = if l.reg.value_name(base_state, piston_type(l)) == "normal" { piston::piston(l) } else { piston::sticky_piston(l) };
    l.block(extended_state) == block
        && l.reg.get_bool(extended_state, extended(l))
        && l.reg.get_dir(extended_state, facing(l)) == l.reg.get_dir(base_state, facing(l))
}

// PistonHeadBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    let block_pos = pos.relative(l.reg.get_dir(state, facing(l)).opposite());
    let base = l.get(block_pos);
    if is_fitting_base(l, state, base) {
        l.destroy_block(block_pos, true, 512);
    }
}

// PistonHeadBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction.opposite() == l.reg.get_dir(state, facing(l)) && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// PistonHeadBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let block_state = l.get(pos.relative(l.reg.get_dir(state, facing(l)).opposite()));
    is_fitting_base(l, state, block_state)
        || l.block(block_state) == piston::moving_piston_block(l) && l.reg.get_dir(block_state, facing(l)) == l.reg.get_dir(state, facing(l))
}

// PistonHeadBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, neighbor_block: BlockId, _moved: bool) {
    if can_survive(l, state, pos) {
        l.neighbor_changed(pos.relative(l.reg.get_dir(state, facing(l)).opposite()), neighbor_block);
    }
}
