// SporeBlossomBlock: hangs from a block that supports its center, and not in
// water.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::level::Level;
use crate::registry::{State, Support};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// SporeBlossomBlock.canSurvive: Block.canSupportCenter(level, pos.above(),
// DOWN) && !level.isWaterAt(pos). The fluid at pos is read only when the
// center is supported.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let above = l.get(pos.above());
    if in_tag!(l, above, "minecraft:unstable_bottom_center") || !shape_ops::is_face_sturdy(l, above, pos.above(), Dir::Down, Support::Center) {
        return false;
    }
    // !level.isWaterAt(pos).
    let fluid = l.fluid_state(pos);
    !l.reg.fluids[fluid as usize].fluid.is_water()
}

// SporeBlossomBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if direction == Dir::Up && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}
