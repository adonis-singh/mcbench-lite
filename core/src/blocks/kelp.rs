// KelpBlock (the head of a kelp plant) and KelpPlantBlock (its body), on
// GrowingPlantBlock, GrowingPlantHeadBlock and GrowingPlantBodyBlock. They
// hold water (their fluid state is the dump's). Their random growth never
// runs (no random ticks), nor does bonemeal.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::fluid::{named, WATER_TICK_DELAY};
use crate::level::Level;
use crate::registry::{Fluid, State, Support};

pub const HEAD_BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(head_update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };
pub const BODY_BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(body_update_shape), can_survive: Some(can_survive), tick: Some(tick), ..NONE };

// KelpBlock.canAttachTo (and KelpPlantBlock's, which is the head's): anything
// but a magma block.
fn can_attach_to(l: &Level, state: State) -> bool {
    l.block(state) != named(l).magma_block
}

// GrowingPlantBlock.canSurvive for a plant that grows up.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below_pos = pos.below();
    let below = l.get(below_pos);
    let known = named(l);
    let block = l.block(below);
    can_attach_to(l, below) && (block == known.kelp || block == known.kelp_plant || crate::shape_ops::is_face_sturdy(l, below, below_pos, Dir::Up, Support::Full))
}

// GrowingPlantBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}

// GrowingPlantHeadBlock.updateShape for the kelp head (growth direction up,
// fluid ticks scheduled).
fn head_update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let known = named(l);
    if direction == Dir::Down {
        if !can_survive(l, state, pos) {
            l.schedule_tick(pos, l.block(state), 1, crate::level::PRIORITY_NORMAL);
        } else {
            let above = l.get(pos.above());
            let block = l.block(above);
            if block == known.kelp || block == known.kelp_plant {
                return l.reg.default_state(known.kelp_plant);
            }
        }
    }
    let neighbor_block = l.block(neighbor);
    if direction != Dir::Up || (neighbor_block != known.kelp && neighbor_block != known.kelp_plant) {
        l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
        state
    } else {
        l.reg.default_state(known.kelp_plant)
    }
}

// GrowingPlantBodyBlock.updateShape for the kelp plant: a body with no
// plant above becomes a head of a random age.
fn body_update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let known = named(l);
    if direction == Dir::Down && !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, crate::level::PRIORITY_NORMAL);
    }
    let neighbor_block = l.block(neighbor);
    if direction == Dir::Up && neighbor_block != known.kelp_plant && neighbor_block != known.kelp {
        // GrowingPlantHeadBlock.getStateForPlacement(random): a random age.
        let age = l.random.next_int_bound(25);
        let head = l.reg.default_state(known.kelp);
        l.reg.with_int(head, l.reg.property_of(head, "age"), age)
    } else {
        l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
        state
    }
}
