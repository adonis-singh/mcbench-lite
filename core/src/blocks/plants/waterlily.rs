// WaterlilyBlock: stands on water (or ice) with nothing wet above.
// entityInside acts on boats.

use crate::blocks::vegetation;
use crate::blocks::Behaviour;
use crate::dir::Pos;
use crate::level::Level;
use crate::registry::{Fluid, State, EMPTY_FLUID};
use crate::shape_classes;

pub const BEHAVIOUR: Behaviour = vegetation::behaviour(can_survive);

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    vegetation::can_survive(l, pos, may_place_on)
}

// WaterlilyBlock.mayPlaceOn: (the fluid at pos is water || the state is an
// IceBlock) && the fluid above it is empty. `pos` is the block below the
// lily.
fn may_place_on(l: &mut Level, state: State, pos: Pos) -> bool {
    let fluid = l.fluid_state(pos);
    let fluid_above = l.fluid_state(pos.above());
    (l.reg.fluids[fluid as usize].fluid == Fluid::Water || shape_classes!(l, ["IceBlock", "FrostedIceBlock"])[l.block(state) as usize]) && fluid_above == EMPTY_FLUID
}
