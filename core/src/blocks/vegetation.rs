// VegetationBlock: updateShape and canSurvive through the class's mayPlaceOn,
// and the helpers the plant classes share (a block by key, a tag's members).

use super::Behaviour;
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

// A block's id by key, looked up once.
#[macro_export]
macro_rules! block_id {
    ($l:expr, $key:literal) => {{
        static ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
        *ID.get_or_init(|| $l.reg.block_key($key))
    }};
}

// BlockState.is(Block): the state is of the block with this key.
#[macro_export]
macro_rules! is_block {
    ($l:expr, $state:expr, $key:literal) => {
        $l.block($state) == $crate::block_id!($l, $key)
    };
}

// BlockState.is(TagKey<Block>).
#[macro_export]
macro_rules! in_tag {
    ($l:expr, $state:expr, $name:literal) => {
        $crate::shape_tag!($l, $name)[$l.block($state) as usize]
    };
}

// VegetationBlock.mayPlaceOn: dirt or farmland below.
pub fn may_place_on(l: &Level, state: State) -> bool {
    crate::in_tag!(l, state, "minecraft:dirt") || crate::is_block!(l, state, "minecraft:farmland")
}

// VegetationBlock.canSurvive: mayPlaceOn the block below, which the class
// decides (it reads the state, and for a few classes the level and position).
pub fn can_survive(l: &mut Level, pos: Pos, may_place_on: fn(&mut Level, State, Pos) -> bool) -> bool {
    let below = pos.below();
    let state = l.get(below);
    may_place_on(l, state, below)
}

// VegetationBlock.updateShape (BlockBehaviour.updateShape returns the
// state; Paper's BlockPhysicsEvent has no listener): the plant that cannot
// survive becomes air. canSurvive is the class's own.
pub fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if super::can_survive(l, state, pos) {
        state
    } else {
        l.known.air
    }
}

// The behaviour of a VegetationBlock that adds nothing: a class whose only
// override is mayPlaceOn (canSurvive's argument).
pub const fn behaviour(can_survive: fn(&mut Level, State, Pos) -> bool) -> Behaviour {
    Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), ..super::NONE }
}

// Level.getFluidState is empty at a state.
pub fn fluid_is_empty(l: &Level, state: State) -> bool {
    l.fluid_of(state) == crate::registry::EMPTY_FLUID
}

// A VegetationBlock class that adds no engine behaviour (BushBlock,
// FlowerBlock, TallGrassBlock, SaplingBlock, FlowerBedBlock, EyeblossomBlock,
// SweetBerryBushBlock, ...): the dirt or farmland below.
pub const PLAIN: Behaviour = behaviour(plain_can_survive);

fn plain_can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    can_survive(l, pos, |l, state, _| may_place_on(l, state))
}
