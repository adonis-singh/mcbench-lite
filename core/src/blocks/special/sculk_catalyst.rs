// SculkCatalystBlock and SculkCatalystBlockEntity. A catalyst blooms when a
// living entity dies near it, and none exists here; its spreader has no
// cursor to update.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::game_event::{Context, GameEvent};
use crate::level::{refuse, Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    tick: Some(tick),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(super::idle_ticker),
    ..NONE
};

fn bloom_property(l: &Level) -> u16 {
    property!(l, "bloom", ["true", "false"])
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::SculkCatalyst)
}

// SculkCatalystBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_bool(state, bloom_property(l)) {
        l.set(pos, l.reg.with_bool(state, bloom_property(l), false), UPDATE_ALL);
    }
}

// CatalystListener.handleGameEvent: a living entity's death whose experience is
// not taken yet is taken (a stand's is nothing: LivingEntity.getBaseExperienceReward
// is 0, so no cursor is added) and the catalyst blooms.
pub fn handle_game_event(l: &mut Level, pos: Pos, event: GameEvent, context: Context) {
    if event != crate::game_event!(l, "entity_die") {
        return;
    }
    let Some(source) = context.entity.filter(|source| source.living) else { return };
    if source.is_input_player() {
        refuse("a player dies beside a sculk catalyst");
    }
    if l.experience_skipped.contains(&source.id) {
        return;
    }
    l.experience_skipped.push(source.id);
    // SculkCatalystBlockEntity.bloom: the pulse, its end, the soul particles
    // and the sound, whose pitch draws from the level's random.
    let state = l.get(pos);
    l.set(pos, l.reg.with_bool(state, bloom_property(l), true), 3);
    l.schedule_tick(pos, l.block(state), 8, PRIORITY_NORMAL);
    l.random.next_float();
}
