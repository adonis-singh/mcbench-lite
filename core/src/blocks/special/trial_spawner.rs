// TrialSpawnerBlock and TrialSpawnerBlockEntity (TrialSpawner,
// TrialSpawnerState, TrialSpawnerStateData), with the default configuration
// a new trial spawner has (it has no spawn potentials), in a level without
// players: nothing is ever detected, so no mob is spawned and no reward is
// ejected. A spawner waiting for players or active asks whether it may spawn
// in the level (canSpawnInLevel): it may not, the level being peaceful with
// mob spawning off, so it only resets its statistics.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::Pos;
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// TrialSpawnerBlockEntity: whether the state data has made its next spawn
// data (a spawn data with an empty entity: there are no spawn potentials).
#[derive(Clone, Debug, Default)]
pub struct TrialSpawner {
    next_spawn_data: bool,
}

impl TrialSpawner {
    pub fn saved(&self) -> (&'static str, String) {
        ("minecraft:trial_spawner", if self.next_spawn_data { "{components:{},spawn_data:{entity:{}}}".into() } else { "{components:{}}".into() })
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Inactive,
    WaitingForPlayers,
    Active,
    WaitingForRewardEjection,
    EjectingReward,
    Cooldown,
}

fn state_property(l: &Level) -> u16 {
    property!(l, "trial_spawner_state", ["inactive", "waiting_for_players", "active", "waiting_for_reward_ejection", "ejecting_reward", "cooldown"])
}

fn ominous_property(l: &Level) -> u16 {
    property!(l, "ominous", ["true", "false"])
}

fn phase(l: &Level, state: State) -> Phase {
    match l.reg.value_name(state, state_property(l)) {
        "inactive" => Phase::Inactive,
        "waiting_for_players" => Phase::WaitingForPlayers,
        "active" => Phase::Active,
        "waiting_for_reward_ejection" => Phase::WaitingForRewardEjection,
        "ejecting_reward" => Phase::EjectingReward,
        "cooldown" => Phase::Cooldown,
        other => panic!("no trial spawner state {other}"),
    }
}

fn with_phase(l: &Level, state: State, phase: Phase) -> State {
    l.reg.with_name(
        state,
        state_property(l),
        match phase {
            Phase::Inactive => "inactive",
            Phase::WaitingForPlayers => "waiting_for_players",
            Phase::Active => "active",
            Phase::WaitingForRewardEjection => "waiting_for_reward_ejection",
            Phase::EjectingReward => "ejecting_reward",
            Phase::Cooldown => "cooldown",
        },
    )
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::TrialSpawner(TrialSpawner::default()))
}

// TrialSpawnerBlock.getTicker on the server: TrialSpawner.tickServer.
fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(tick_server)
}

// TrialSpawnerStateData.targetCooldownLength's default, and the cooldown
// end time every state keeps (0: nothing sets it without a player).
const TARGET_COOLDOWN_LENGTH: i64 = 36000;

// Sets whether the spawn data is made; whether it was not made before and is
// now.
fn set_data(l: &mut Level, pos: Pos, next_spawn_data: bool) -> bool {
    match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::TrialSpawner(spawner))) => {
            let made = next_spawn_data && !spawner.next_spawn_data;
            spawner.next_spawn_data = next_spawn_data;
            made
        }
        _ => panic!("no trial spawner at {pos}"),
    }
}

// TrialSpawner.tickServer. No mob is tracked, so none is untracked.
fn tick_server(l: &mut Level, pos: Pos, state: State) {
    let current = phase(l, state);
    let next = match current {
        // getOrCreateDisplayEntity: makes the spawn data (an empty one: there
        // are no spawn potentials, so the level's random is not drawn from),
        // and a spawn data without an entity id has no mob to show.
        Phase::Inactive => {
            // (markUpdated: setChanged, when the spawn data is made.)
            if set_data(l, pos, true) {
                super::set_changed(l, pos);
            }
            current
        }
        // canSpawnInLevel is false: the level is peaceful with mob spawning
        // off (the Java engine's server answers the spawner_blocks_enabled
        // it reads with Paper's default, true). resetStatistics clears
        // what no player or mob has set; an active spawner goes back to
        // waiting.
        Phase::WaitingForPlayers => current,
        Phase::Active => Phase::WaitingForPlayers,
        // isReadyToOpenShutter: the cooldown end time is 0.
        Phase::WaitingForRewardEjection => {
            if l.game_time as f32 >= (-TARGET_COOLDOWN_LENGTH) as f32 + 40.0f32 {
                Phase::EjectingReward
            } else {
                current
            }
        }
        // isReadyToEjectItems, with no player detected: the shutter closes.
        Phase::EjectingReward => {
            if (l.game_time + TARGET_COOLDOWN_LENGTH) as f32 % 30.0f32 == 0.0f32 {
                Phase::Cooldown
            } else {
                current
            }
        }
        // tryDetectPlayers finds no player, and the cooldown (ending at 0) is
        // over: the spawner stops being ominous and resets.
        Phase::Cooldown => {
            let reset = l.reg.with_bool(state, ominous_property(l), false);
            l.set(pos, reset, UPDATE_ALL);
            set_data(l, pos, false);
            Phase::WaitingForPlayers
        }
    };
    if next != current {
        // TrialSpawnerBlockEntity.setState: setChanged, then setBlockAndUpdate
        // on the entity's own (current) state.
        super::set_changed(l, pos);
        let now = l.get(pos);
        l.set(pos, with_phase(l, now, next), UPDATE_ALL);
    }
}
