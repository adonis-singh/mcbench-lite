// SnowLayerBlock: a layer survives on a block with a full top (or snow of
// eight layers, or a block of the can-survive tag), never on the
// cannot-survive tag. Its random tick (melting by block light) never runs.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::in_tag;
use crate::level::Level;
use crate::property;
use crate::registry::State;
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..NONE };

// BlockStateProperties.LAYERS.
fn layers(l: &Level) -> u16 {
    property!(l, "layers", ["1", "2", "3", "4", "5", "6", "7", "8"])
}

// SnowLayerBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    if in_tag!(l, below, "minecraft:snow_layer_cannot_survive_on") {
        return false;
    }
    if in_tag!(l, below, "minecraft:snow_layer_can_survive_on") {
        return true;
    }
    // Block.isFaceFull(blockState.getCollisionShape(level, pos.below()), UP):
    // the dumped shape is the one at the origin for no entity (a moving
    // piston's is its block entity's).
    shape_ops::collision_shape_at(l, below, pos.below()).face_shape(Dir::Up).is_full_block() || l.block(below) == l.block(state) && l.reg.get_int(below, layers(l)) == 8
}

// SnowLayerBlock.updateShape: a layer that cannot survive becomes air.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if can_survive(l, state, pos) {
        state
    } else {
        l.known.air
    }
}
