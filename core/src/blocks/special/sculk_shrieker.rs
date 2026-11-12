// SculkShriekerBlock and SculkShriekerBlockEntity. A shrieker shrieks for a
// player (a player that steps on it, or whose vibration it hears). The only
// player here is the input player (McbenchExperiment.newInputPlayer, made in
// the level, so SculkShriekerBlockEntity.tryGetPlayer takes it): a vibration
// it causes, heard directly or passed on by a sculk sensor, makes the
// shrieker shriek. Warning the player (WardenSpawnTracker.tryWarn) and
// responding (a warden, its reply sound, darkness) need canRespond, which is
// false in a peaceful level, so neither the player's warden spawn tracker
// nor the level's random is read.

use super::vibration::{self, Data};
use super::Special;
use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::Entity;
use crate::game_event::{Context, Source};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL, UPDATE_CLIENTS};
use crate::property;
use crate::registry::{Registry, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    tick: Some(tick),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    step_on: Some(step_on),
    ..NONE
};

// SculkShriekerBlockEntity: the vibration data and the warning level.
#[derive(Clone, Debug)]
pub struct Shrieker {
    pub vibration: Data,
    pub warning_level: i32,
}

impl Shrieker {
    pub fn saved(&self, reg: &Registry) -> (&'static str, String) {
        ("minecraft:sculk_shrieker", format!("{{components:{{}},{},warning_level:{}}}", self.vibration.saved(reg), self.warning_level))
    }
}

fn shrieking_property(l: &Level) -> u16 {
    property!(l, "shrieking", ["true", "false"])
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::SculkShrieker(Shrieker { vibration: Data::default(), warning_level: 0 }))
}

// getTicker on the server: VibrationSystem.Ticker.tick.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    fn tick_entity(l: &mut Level, pos: Pos, _state: State) {
        vibration::tick(l, pos);
    }
    Some(tick_entity)
}

fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

// SculkShriekerBlock.stepOn: only a player (or what one controls or threw)
// makes it shriek.
fn step_on(_l: &mut Level, _pos: Pos, _state: State, _entity: &Entity) {}

// SculkShriekerBlock.tick: the shriek ends, and the block entity responds if
// it can (it cannot).
fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.reg.get_bool(state, shrieking_property(l)) {
        l.set(pos, l.reg.with_bool(state, shrieking_property(l), false), UPDATE_ALL);
        // level.getBlockEntity(pos, SCULK_SHRIEKER) makes the block entity if
        // it is missing; tryRespond: canRespond is false in a peaceful level.
        l.block_entity(pos);
    }
}

// SculkShriekerBlockEntity.VibrationUser.onReceiveVibration: tryShriek for
// the player that caused the vibration (canReceiveVibration took only the
// input player, and the vibration keeps the entity it was made with).
pub fn on_receive_vibration(l: &mut Level, listener: Pos, entity: Option<Source>) {
    let player = entity.filter(|source| source.is_input_player()).expect("a sculk shrieker received a vibration of no player");
    try_shriek(l, listener, player);
}

// SculkShriekerBlockEntity.tryShriek: canRespond is false (a peaceful level),
// so the warning level goes to 0 and the shrieker shrieks.
fn try_shriek(l: &mut Level, pos: Pos, player: Source) {
    let state = l.get(pos);
    if l.reg.get_bool(state, shrieking_property(l)) {
        return;
    }
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::SculkShrieker(shrieker))) => shrieker.warning_level = 0,
        _ => panic!("no sculk shrieker at {pos}"),
    }
    shriek(l, pos, state, player);
}

// SculkShriekerBlockEntity.shriek: the level event (3007) goes to the players
// near it, none.
fn shriek(l: &mut Level, pos: Pos, state: State, player: Source) {
    l.set(pos, l.reg.with_bool(state, shrieking_property(l), true), UPDATE_CLIENTS);
    l.schedule_tick(pos, l.block(state), 90, PRIORITY_NORMAL);
    let event = crate::game_event!(l, "shriek");
    l.game_event_in(event, pos, Context::of_entity(Some(player)));
}
