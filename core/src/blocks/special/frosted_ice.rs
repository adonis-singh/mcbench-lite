// FrostedIceBlock (and IceBlock.melt). Frosted ice schedules its tick when
// placed and ages or melts into water when it ticks, as the light there
// allows.

use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL, UPDATE_CLIENTS};
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour { neighbor_changed: Some(neighbor_changed), on_place: Some(on_place), tick: Some(tick), ..NONE };

// Paper's environment.frosted-ice.delay (min and max).
const DELAY_MIN: i32 = 20;
const DELAY_MAX: i32 = 40;

// Mth.nextInt(random, min, max).
fn next_int(l: &mut Level, min: i32, max: i32) -> i32 {
    if min >= max {
        min
    } else {
        l.random.next_int_bound(max - min + 1) + min
    }
}

// FrostedIceBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    let delay = next_int(l, 60, 120);
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
}

// FrostedIceBlock.fewerNeigboursThan.
fn fewer_neighbours_than(l: &mut Level, state: State, pos: Pos, required: i32) -> bool {
    let mut count = 0;
    for dir in crate::dir::ALL {
        let neighbour = l.get(pos.relative(dir));
        if l.block(neighbour) == l.block(state) {
            count += 1;
            if count >= required {
                return false;
            }
        }
    }
    true
}

// FrostedIceBlock.slightlyMelt: whether the block melted away (it was at age
// 3); otherwise it ages, without a neighbour update.
fn slightly_melt(l: &mut Level, state: State, pos: Pos) -> bool {
    let age = l.reg.property_of(state, "age");
    let value = l.reg.get_int(state, age);
    if value < 3 {
        let aged = l.reg.with_int(state, age, value + 1);
        l.set_block(pos, aged, UPDATE_CLIENTS, 512);
        false
    } else {
        melt(l, pos);
        true
    }
}

// FrostedIceBlock.tick (with Paper's frostedIce.enabled on), in the overworld.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.random.next_int_bound(3) == 0 || fewer_neighbours_than(l, state, pos, 4) {
        // Level.getMaxLocalRawBrightness(pos).
        let sky_darken = l.sky_darken;
        let brightness = l.raw_brightness(pos, sky_darken);
        let age = l.reg.get_int(state, l.reg.property_of(state, "age"));
        if brightness > 11 - age - l.reg.states[state as usize].light_block as i32 && slightly_melt(l, state, pos) {
            for dir in crate::dir::ALL {
                let neighbour_pos = pos.relative(dir);
                let neighbour = l.get(neighbour_pos);
                if l.block(neighbour) == l.block(state) && !slightly_melt(l, neighbour, neighbour_pos) {
                    let delay = next_int(l, DELAY_MIN, DELAY_MAX);
                    l.schedule_tick(neighbour_pos, l.block(state), delay, PRIORITY_NORMAL);
                }
            }
            return;
        }
    }
    let delay = next_int(l, DELAY_MIN, DELAY_MAX);
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
}

// FrostedIceBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, neighbor: BlockId, _moved: bool) {
    if neighbor == l.block(state) && fewer_neighbours_than(l, state, pos, 2) {
        melt(l, pos);
    }
}

// IceBlock.melt in the overworld (the fade event has no listener): the block
// becomes water, and its neighbours hear of the water.
fn melt(l: &mut Level, pos: Pos) {
    let water = l.reg.block_key("minecraft:water");
    l.set(pos, l.reg.default_state(water), UPDATE_ALL);
    l.neighbor_changed(pos, water);
}
