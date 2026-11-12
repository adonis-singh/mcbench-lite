// GrowingPlantHeadBlock (WeepingVinesBlock, TwistingVinesBlock and
// CaveVinesBlock add only fields, growth and bonemeal): updateShape.
// scheduleFluidTicks is false for these classes.

use super::growing_plant::{self, plant};
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(growing_plant::can_survive), tick: Some(growing_plant::tick), ..NONE };

// updateBodyAfterConvertedFromHead(state, body.defaultBlockState()): the
// body, which CaveVinesBlock gives the head's BERRIES.
fn converted_to_body(l: &Level, head: State) -> State {
    let plant = plant(l, head);
    let body = l.reg.default_state(plant.body);
    if plant.berries {
        let berries = growing_plant::berries(l);
        l.reg.with(body, berries, l.reg.get(head, berries))
    } else {
        body
    }
}

// GrowingPlantHeadBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let plant = plant(l, state);
    let block = l.block(state);
    if direction == plant.growth.opposite() {
        if !crate::blocks::can_survive(l, state, pos) {
            l.schedule_tick(pos, block, 1, PRIORITY_NORMAL);
        } else {
            let ahead = l.get(pos.relative(plant.growth));
            if l.block(ahead) == block || l.block(ahead) == plant.body {
                return converted_to_body(l, state);
            }
        }
    }
    if direction != plant.growth || l.block(neighbor) != block && l.block(neighbor) != plant.body {
        state
    } else {
        converted_to_body(l, state)
    }
}
