// DoorBlock (and WeatheringCopperDoorBlock, whose additions are random
// ticks). Events (BlockRedstoneEvent) have no listener: the new current
// stands.

use super::common::{bukkit_block_power, can_open_by_hand, open, play_sound_with_random_pitch, powered};
use crate::blocks::{Behaviour, InputPlayer, NONE};
use crate::dir::{Axis, Dir, Pos};
use crate::level::{Level, UPDATE_CLIENTS, UPDATE_IMMEDIATE};
use crate::property;
use crate::registry::{BlockId, State, Support};
use crate::shape_classes;

pub const BEHAVIOUR: Behaviour =
    Behaviour { update_shape: Some(update_shape), neighbor_changed: Some(neighbor_changed), can_survive: Some(can_survive), use_without_item: Some(use_without_item), ..NONE };

// DoubleBlockHalf's indices in its property's values.
const LOWER: u16 = 1;

fn half(l: &Level) -> u16 {
    property!(l, "half", ["upper", "lower"])
}

// instanceof DoorBlock.
pub fn is_door(l: &Level, state: State) -> bool {
    shape_classes!(l, ["DoorBlock", "WeatheringCopperDoorBlock"])[l.block(state) as usize]
}

// DoorBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    let door_half = l.reg.get(state, half(l));
    if direction.axis() == Axis::Y && (door_half == LOWER) == (direction == Dir::Up) {
        if is_door(l, neighbor) && l.reg.get(neighbor, half(l)) != door_half {
            l.reg.with(neighbor, half(l), door_half)
        } else {
            l.known.air
        }
    } else if door_half == LOWER && direction == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// DoorBlock.canSurvive.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let below_pos = pos.below();
    let below = l.get(below_pos);
    if l.reg.get(state, half(l)) == LOWER {
        crate::shape_ops::is_face_sturdy(l, below, below_pos, Dir::Up, Support::Full)
    } else {
        l.block(below) == l.block(state)
    }
}

// DoorBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let other_half = pos.relative(if l.reg.get(state, half(l)) == LOWER { Dir::Up } else { Dir::Down });
    let mut power = bukkit_block_power(l, pos);
    let power_top = bukkit_block_power(l, other_half);
    if power_top > power {
        power = power_top;
    }
    let old_power = if l.reg.get_bool(state, powered(l)) { 15 } else { 0 };
    if (old_power == 0) ^ (power == 0) {
        let now_open = power > 0;
        if now_open != l.reg.get_bool(state, open(l)) {
            play_sound_with_random_pitch(l);
            let event = if now_open { crate::game_event!(l, "block_open") } else { crate::game_event!(l, "block_close") };
            l.game_event_at(None, event, pos);
        }
        let new_state = l.reg.with_bool(l.reg.with_bool(state, powered(l), now_open), open(l), now_open);
        l.set(pos, new_state, UPDATE_CLIENTS);
    }
}

// DoorBlock.useWithoutItem: a type that cannot be opened by hand (iron) passes;
// else the clicked half's open state flips (flags 10: the other half follows
// by its shape update), the sound draws its pitch, and the player is the
// source of the game event.
fn use_without_item(l: &mut Level, state: State, pos: Pos, player: Option<InputPlayer>) -> bool {
    if !can_open_by_hand(l, state) {
        return false;
    }
    let state = l.reg.cycle(state, open(l));
    l.set(pos, state, UPDATE_CLIENTS | UPDATE_IMMEDIATE);
    play_sound_with_random_pitch(l);
    let event = if l.reg.get_bool(state, open(l)) { crate::game_event!(l, "block_open") } else { crate::game_event!(l, "block_close") };
    l.game_event_at(player.map(|p| p.source), event, pos);
    true
}
