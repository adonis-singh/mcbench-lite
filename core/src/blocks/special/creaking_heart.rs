// CreakingHeartBlock and CreakingHeartBlockEntity. A heart wakes at night
// and spawns a creaking for a player near it; it is never night here (the
// daylight cycle is off at noon) and there is no player, so a heart only
// follows whether it has its two logs.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_analog_output_signal: Some(get_analog_output_signal),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// CreakingHeartBlockEntity: its ticks of existence and its tick countdown.
// It has no creaking, so the output signal and the particle emitter stay 0.
#[derive(Clone, Debug, Default)]
pub struct CreakingHeart {
    ticker: i32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Uprooted,
    Dormant,
    Awake,
}

fn axis_property(l: &Level) -> u16 {
    property!(l, "axis", ["x", "y", "z"])
}

fn state_property(l: &Level) -> u16 {
    property!(l, "creaking_heart_state", ["uprooted", "dormant", "awake"])
}

fn phase(l: &Level, state: State) -> Phase {
    match l.reg.value_name(state, state_property(l)) {
        "uprooted" => Phase::Uprooted,
        "dormant" => Phase::Dormant,
        "awake" => Phase::Awake,
        other => panic!("no creaking heart state {other}"),
    }
}

fn with_phase(l: &Level, state: State, phase: Phase) -> State {
    l.reg.with_name(
        state,
        state_property(l),
        match phase {
            Phase::Uprooted => "uprooted",
            Phase::Dormant => "dormant",
            Phase::Awake => "awake",
        },
    )
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::CreakingHeart(CreakingHeart::default()))
}

// CreakingHeartBlock.getTicker on the server: a heart that is not uprooted.
fn ticker(l: &Level, state: State) -> Option<fn(&mut Level, Pos, State)> {
    (phase(l, state) != Phase::Uprooted).then_some(server_tick as fn(&mut Level, Pos, State))
}

// CreakingHeartBlock.isNaturalNight: Level.isMoonVisible, with the day time
// at noon (the daylight cycle is off).
fn is_natural_night() -> bool {
    false
}

// CreakingHeartBlock.hasRequiredLogs: the two blocks on its axis are pale oak
// logs on that axis.
fn has_required_logs(l: &mut Level, state: State, pos: Pos) -> bool {
    let axis = l.reg.value_name(state, axis_property(l)).to_string();
    let directions = match axis.as_str() {
        "x" => [Dir::East, Dir::West],
        "y" => [Dir::Up, Dir::Down],
        "z" => [Dir::South, Dir::North],
        other => panic!("no axis {other}"),
    };
    for dir in directions {
        let at = l.get(pos.relative(dir));
        if !l.reg.in_tag(at, "minecraft:pale_oak_logs") || l.reg.value_name(at, axis_property(l)) != axis {
            return false;
        }
    }
    true
}

// CreakingHeartBlock.updateState.
fn update_state(l: &mut Level, state: State, pos: Pos) -> State {
    let required = has_required_logs(l, state, pos);
    if required && phase(l, state) == Phase::Uprooted {
        with_phase(l, state, if is_natural_night() { Phase::Awake } else { Phase::Dormant })
    } else {
        state
    }
}

// CreakingHeartBlock.updateShape: a tick is scheduled.
fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    state
}

// CreakingHeartBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let updated = update_state(l, state, pos);
    if updated != state {
        l.set(pos, updated, UPDATE_ALL);
    }
}

// Containers.updateNeighboursAfterDestroy.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, _moved: bool) {
    l.update_neighbour_for_output_signal(pos, l.block(state));
}

// CreakingHeartBlock.getAnalogOutputSignal: the block entity's output signal,
// which is 0 without a creaking.
fn get_analog_output_signal(l: &mut Level, state: State, pos: Pos, _dir: Dir) -> i32 {
    if phase(l, state) == Phase::Uprooted {
        return 0;
    }
    l.block_entity(pos);
    0
}

// CreakingHeartBlockEntity.serverTick.
fn server_tick(l: &mut Level, pos: Pos, state: State) {
    let heart = match l.block_entities.get_mut(&pos) {
        Some(BlockEntity::Special(Special::CreakingHeart(heart))) => heart,
        _ => panic!("no creaking heart at {pos}"),
    };
    let countdown = heart.ticker;
    heart.ticker -= 1;
    if countdown < 0 {
        let delay = l.random.next_int_bound(5) + 20;
        match l.block_entities.get_mut(&pos) {
            Some(BlockEntity::Special(Special::CreakingHeart(heart))) => heart.ticker = delay,
            _ => unreachable!(),
        }
        // updateCreakingState: with no creaking, uprooted without the logs,
        // else asleep.
        let updated = if !has_required_logs(l, state, pos) {
            with_phase(l, state, Phase::Uprooted)
        } else {
            with_phase(l, state, if is_natural_night() { Phase::Awake } else { Phase::Dormant })
        };
        if updated != state {
            l.set(pos, updated, UPDATE_ALL);
        }
        // (A creaking is spawned for a player near an awake heart: neither
        // can be here.)
    }
}
