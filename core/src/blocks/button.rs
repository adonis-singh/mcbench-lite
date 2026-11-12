// ButtonBlock.

use super::lever::{affect_neighbors_after_removal, can_survive, get_direct_signal, get_signal, powered, update_neighbours, update_shape};
use super::{Behaviour, InputPlayer, NONE};
use crate::dir::Pos;
use crate::level::{refuse, Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    can_survive: Some(can_survive),
    use_without_item: Some(press),
    ..NONE
};

// ButtonBlock.ticksToStayPressed, the constructor argument Blocks gives each
// button: stone and polished blackstone 20, the wood types 30.
fn ticks_to_stay_pressed(l: &Level, state: State) -> i32 {
    match l.reg.blocks[l.block(state) as usize].key.as_str() {
        "minecraft:stone_button" | "minecraft:polished_blackstone_button" => 20,
        "minecraft:oak_button" | "minecraft:spruce_button" | "minecraft:birch_button" | "minecraft:jungle_button" | "minecraft:acacia_button"
        | "minecraft:cherry_button" | "minecraft:dark_oak_button" | "minecraft:pale_oak_button" | "minecraft:mangrove_button" | "minecraft:bamboo_button"
        | "minecraft:crimson_button" | "minecraft:warped_button" => 30,
        other => refuse(format!("ticks to stay pressed of {other}")),
    }
}

// ButtonBlock.press with no player: playSound changes nothing.
fn press(l: &mut Level, state: State, pos: Pos, _player: Option<InputPlayer>) -> bool {
    l.set(pos, l.reg.with_bool(state, powered(l), true), UPDATE_ALL);
    update_neighbours(l, state, pos);
    let delay = ticks_to_stay_pressed(l, state);
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
    let event = crate::game_event!(l, "block_activate");
    l.game_event_at(None, event, pos);
    true
}

// ButtonBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_bool(state, powered(l)) {
        check_pressed(l, state, pos);
    }
}

// ButtonBlock.checkPressed. This port has no entities, so the arrow query
// (getEntitiesOfClass over the button's box, a lookup that changes nothing)
// finds none, whether or not the button type lets arrows press it. Only a
// powered button is checked here (tick), so it releases: the redstone change
// event has no listener.
fn check_pressed(l: &mut Level, state: State, pos: Pos) {
    let pressed = false;
    let powered_value = l.reg.get_bool(state, powered(l));
    if pressed != powered_value {
        l.set(pos, l.reg.with_bool(state, powered(l), pressed), UPDATE_ALL);
        update_neighbours(l, state, pos);
        // gameEvent(arrow, flag ? BLOCK_ACTIVATE : BLOCK_DEACTIVATE): no arrow.
        let event = crate::game_event!(l, "block_deactivate");
        l.game_event_at(None, event, pos);
    }
    // The reschedule is for a button an arrow holds: none here.
}
