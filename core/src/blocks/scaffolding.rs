// ScaffoldingBlock: it holds as long as a chain of scaffolding reaches the
// ground, else it falls (FallingBlockEntity.fall) or breaks. Its shape
// depends on the colliding entity, so entities that move near it refuse.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::entity::falling;
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::fluid::WATER_TICK_DELAY;
use crate::registry::{Fluid, State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), on_place: Some(on_place), tick: Some(tick), can_survive: Some(can_survive), ..NONE };

fn distance(l: &Level) -> u16 {
    property!(l, "distance", ["0", "1", "2", "3", "4", "5", "6", "7"])
}

fn bottom(l: &Level) -> u16 {
    property!(l, "bottom", ["true", "false"])
}

fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
}

fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if l.reg.get_bool(state, l.reg.property_of(state, "waterlogged")) {
        l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
    }
    l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    state
}

fn is_bottom(l: &mut Level, pos: Pos, distance: i32, block: u16) -> bool {
    if distance <= 0 {
        return false;
    }
    let below = l.get(pos.below());
    l.block(below) != block
}

// ScaffoldingBlock.getDistance.
fn get_distance(l: &mut Level, pos: Pos) -> i32 {
    let below = pos.below();
    let state = l.get(below);
    let scaffolding = l.reg.block_key("minecraft:scaffolding");
    let mut i = 7;
    if l.block(state) == scaffolding {
        i = l.reg.get_int(state, distance(l));
    } else if crate::shape_ops::is_face_sturdy(l, state, below, Dir::Up, Support::Full) {
        return 0;
    }
    for dir in HORIZONTAL {
        let neighbour = l.get(pos.relative(dir));
        if l.block(neighbour) == scaffolding {
            i = i.min(l.reg.get_int(neighbour, distance(l)) + 1);
            if i == 1 {
                break;
            }
        }
    }
    i
}

fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    get_distance(l, pos) < 7
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    let dist = get_distance(l, pos);
    let with_distance = l.reg.with_int(state, distance(l), dist);
    let bottom_value = is_bottom(l, pos, dist, l.block(state));
    let new_state = l.reg.with_bool(with_distance, bottom(l), bottom_value);
    // The fade event (BlockFadeEvent) has no listener.
    if l.reg.get_int(new_state, distance(l)) == 7 {
        if l.reg.get_int(state, distance(l)) == 7 {
            falling::fall(l, pos, new_state);
        } else {
            l.destroy_block(pos, true, 512);
        }
    } else if state != new_state {
        l.set(pos, new_state, UPDATE_ALL);
    }
}
