// GrowingPlantBodyBlock (WeepingVinesPlantBlock, TwistingVinesPlantBlock and
// CaveVinesPlantBlock add only fields and bonemeal): updateShape.
// scheduleFluidTicks is false for these classes.

use super::growing_plant::{self, plant};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(growing_plant::can_survive), tick: Some(growing_plant::tick), ..NONE };

// updateHeadAfterConvertedFromBody(state, headBlock.getStateForPlacement(
// random)): a head of a random age, which CaveVinesPlantBlock gives the
// body's BERRIES.
fn converted_to_head(l: &mut Level, body: State) -> State {
    let plant = plant(l, body);
    let head = l.reg.default_state(plant.head);
    let head = l.reg.with(head, growing_plant::age(l), l.random.next_int_bound(25) as u16);
    if plant.berries {
        let berries = growing_plant::berries(l);
        l.reg.with(head, berries, l.reg.get(body, berries))
    } else {
        head
    }
}

// GrowingPlantBodyBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let plant = plant(l, state);
    let block = l.block(state);
    if direction == plant.growth.opposite() && !crate::blocks::can_survive(l, state, pos) {
        l.schedule_tick(pos, block, 1, PRIORITY_NORMAL);
    }
    if direction == plant.growth && l.block(neighbor) != block && l.block(neighbor) != plant.head {
        converted_to_head(l, state)
    } else {
        state
    }
}
