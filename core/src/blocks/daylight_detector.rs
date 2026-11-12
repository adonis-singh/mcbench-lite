// DaylightDetectorBlock. Its ticker reads the sky light, the sky darkness
// and the sun's angle once every 20 ticks.

use super::entities::Entity;
use super::{BlockEntity, Behaviour, InputPlayer, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { get_signal: Some(get_signal), new_block_entity: Some(new_block_entity), ticker: Some(ticker), use_without_item: Some(use_without_item), ..NONE };

// DaylightDetectorBlock.POWER.
fn power_property(l: &Level) -> u16 {
    property!(l, "power", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15"])
}

// DaylightDetectorBlock.getSignal.
fn get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    l.reg.get_int(state, power_property(l))
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Entity(Entity::DaylightDetector)
}

// DaylightDetectorBlock.getTicker: the server of a dimension with sky light
// (the overworld) ticks DaylightDetectorBlock.tickEntity.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(tick_entity)
}

// DaylightDetectorBlock.tickEntity: updateSignalStrength every 20 game ticks.
fn tick_entity(l: &mut Level, pos: Pos, state: State) {
    if l.game_time % 20 == 0 {
        update_signal_strength(l, state, pos);
    }
}

// DaylightDetectorBlock.updateSignalStrength.
fn update_signal_strength(l: &mut Level, state: State, pos: Pos) {
    let mut i = l.sky_brightness(pos) - l.sky_darken;
    let mut sun_angle = crate::light::sun_angle(crate::light::DAY_TIME);
    let inverted = l.reg.get_bool(state, l.reg.property_of(state, "inverted"));
    if inverted {
        i = 15 - i;
    } else if i > 0 {
        let f = if sun_angle < std::f32::consts::PI { 0.0f32 } else { (std::f64::consts::PI * 2.0) as f32 };
        sun_angle += (f - sun_angle) * 0.2f32;
        // Math.round(float).
        i = (i as f32 * crate::light::mth_cos(sun_angle) + 0.5f32).floor() as i32;
    }
    i = i.clamp(0, 15);
    let power = power_property(l);
    if l.reg.get_int(state, power) != i {
        // CraftEventFactory.callRedstoneChange: no plugin changes the current.
        l.set(pos, l.reg.with_int(state, power, i), 3);
    }
}

// DaylightDetectorBlock.useWithoutItem: for a player who may build (the input
// player is a survival player), the block is inverted (flags 2), the game
// event BLOCK_CHANGE names the player and the new state, and the signal
// is recomputed.
fn use_without_item(l: &mut Level, state: State, pos: Pos, player: Option<InputPlayer>) -> bool {
    let inverted = l.reg.property_of(state, "inverted");
    let state = l.reg.cycle(state, inverted);
    l.set(pos, state, 2);
    let event = crate::game_event!(l, "block_change");
    l.game_event_in(event, pos, crate::game_event::Context::of(player.map(|p| p.source), state));
    update_signal_strength(l, state, pos);
    true
}
