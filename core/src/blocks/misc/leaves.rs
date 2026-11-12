// LeavesBlock (and TintedParticleLeavesBlock, UntintedParticleLeavesBlock and
// MangroveLeavesBlock, which add particles and bonemeal only): the distance
// to the nearest log, kept by scheduled ticks. randomTick (decay) never runs.

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, ALL};
use crate::in_tag;
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), tick: Some(tick), ..NONE };

// LeavesBlock.DISTANCE.
fn distance(l: &Level) -> u16 {
    property!(l, "distance", ["1", "2", "3", "4", "5", "6", "7"])
}

// LeavesBlock.TICK_DELAY.
const TICK_DELAY: i32 = 1;

// LeavesBlock.getOptionalDistanceAt orElse 7 (LeavesBlock.getDistanceAt): a
// log is 0, a state with the DISTANCE property (a leaves block) has its
// value, any other 7. (Scaffolding's distance is another property.)
fn distance_at(l: &Level, neighbor: State) -> i32 {
    if in_tag!(l, neighbor, "minecraft:logs") {
        0
    } else if l.reg.has(neighbor, distance(l)) {
        l.reg.get_int(neighbor, distance(l))
    } else {
        7
    }
}

// LeavesBlock.updateShape: a waterlogged leaf schedules a water tick. The block ticks to recompute its distance unless the
// neighbour is a log and the distance is already 1.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    let i = distance_at(l, neighbor) + 1;
    if i != 1 || l.reg.get_int(state, distance(l)) != i {
        l.schedule_tick(pos, l.block(state), TICK_DELAY, PRIORITY_NORMAL);
    }
    state
}

// LeavesBlock.updateDistance.
fn update_distance(l: &mut Level, state: State, pos: Pos) -> State {
    let mut i = 7;
    for dir in ALL {
        let neighbor = l.get(pos.relative(dir));
        i = i.min(distance_at(l, neighbor) + 1);
        if i == 1 {
            break;
        }
    }
    l.reg.with_int(state, distance(l), i)
}

// LeavesBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let updated = update_distance(l, state, pos);
    l.set(pos, updated, UPDATE_ALL);
}
