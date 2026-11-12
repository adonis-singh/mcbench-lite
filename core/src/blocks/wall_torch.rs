// RedstoneWallTorchBlock: RedstoneTorchBlock with its own canSurvive,
// updateShape, hasNeighborSignal and getSignal. onPlace and
// affectNeighborsAfterRemoval are the torch's (randomOrientation is null).

use super::torch;
use super::Behaviour;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::{BlockId, State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(torch::get_direct_signal),
    can_survive: Some(can_survive),
    ..torch::BEHAVIOUR
};

fn facing(l: &Level) -> u16 {
    property!(l, "facing", ["north", "south", "west", "east"])
}

// WallTorchBlock.canSurvive(level, pos, facing): the block behind has a
// sturdy face towards the torch.
pub fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let facing = l.reg.get_dir(state, facing(l));
    let behind = pos.relative(facing.opposite());
    let behind_state = l.get(behind);
    crate::shape_ops::is_face_sturdy(l, behind_state, behind, facing, Support::Full)
}

pub fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if dir.opposite() == l.reg.get_dir(state, facing(l)) && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

fn has_neighbor_signal(l: &mut Level, pos: Pos, state: State) -> bool {
    let opposite = l.reg.get_dir(state, facing(l)).opposite();
    l.has_signal(pos.relative(opposite), opposite)
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if l.reg.get_bool(state, torch::lit(l)) && l.reg.get_dir(state, facing(l)) != side {
        15
    } else {
        0
    }
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    torch::tick_with(l, state, pos, has_neighbor_signal);
}

fn neighbor_changed(l: &mut Level, state: State, pos: Pos, neighbor: BlockId, moved: bool) {
    torch::neighbor_changed_with(l, state, pos, neighbor, moved, has_neighbor_signal);
}
