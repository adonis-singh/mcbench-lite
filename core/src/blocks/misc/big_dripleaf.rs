// BigDripleafBlock: the leaf that tilts under an item that lands on it and
// rights itself, on scheduled ticks. onProjectileHit is not reached (no
// projectiles).

use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::Entity;
use crate::in_tag;
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::property;
use crate::registry::{BlockId, State};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    tick: Some(tick),
    can_survive: Some(can_survive),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// BigDripleafBlock.TILT: the values none, unstable, partial, full.
fn tilt(l: &Level) -> u16 {
    property!(l, "tilt", ["none", "unstable", "partial", "full"])
}

const NONE_TILT: u16 = 0;
const UNSTABLE: u16 = 1;
const PARTIAL: u16 = 2;
const FULL: u16 = 3;

// BigDripleafBlock.DELAY_UNTIL_NEXT_TILT_STATE (-1 for none).
fn delay_until_next_tilt_state(tilt: u16) -> i32 {
    match tilt {
        UNSTABLE | PARTIAL => 10,
        FULL => 100,
        _ => -1,
    }
}

// BigDripleafBlock.canSurvive: a leaf, a stem or a placeable block below.
fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    l.block(below) == l.block(state) || is_block!(l, below, "minecraft:big_dripleaf_stem") || in_tag!(l, below, "minecraft:big_dripleaf_placeable")
}

// BigDripleafBlock.updateShape: a leaf with another leaf above it becomes a
// stem (Block.withPropertiesOf copies the facing and waterlogged values).
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if direction == Dir::Down && !can_survive(l, state, pos) {
        return l.known.air;
    }
    // A waterlogged leaf schedules a water tick.
    schedule_water_tick(l, state, pos);
    if direction == Dir::Up && l.block(neighbor) == l.block(state) {
        let stem = l.reg.default_state(crate::block_id!(l, "minecraft:big_dripleaf_stem"));
        let facing = l.reg.property_of(state, "facing");
        let waterlogged = l.reg.property_of(state, "waterlogged");
        let stem = l.reg.with(stem, facing, l.reg.get(state, facing));
        l.reg.with(stem, waterlogged, l.reg.get(state, waterlogged))
    } else {
        state
    }
}

// BigDripleafBlock.entityInside: an entity standing on a leaf that is not
// tilted, above the leaf's top edge, with no signal at the leaf, tilts it
// (the event it calls first has no listener, and nothing cancels the
// EntityInteractEvent or the EntityChangeBlockEvent).
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if l.reg.get(state, tilt(l)) == NONE_TILT && can_entity_tilt(pos, entity) && !l.has_neighbor_signal(pos) {
        set_tilt_and_schedule_tick(l, state, pos, UNSTABLE, false);
    }
}

// BigDripleafBlock.canEntityTilt.
fn can_entity_tilt(pos: Pos, entity: &Entity) -> bool {
    entity.on_ground && entity.pos.y > (pos.y() as f32 + 0.6875f32) as f64
}

// BigDripleafBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if l.has_neighbor_signal(pos) {
        reset_tilt(l, state, pos);
    } else {
        match l.reg.get(state, tilt(l)) {
            UNSTABLE => set_tilt_and_schedule_tick(l, state, pos, PARTIAL, true),
            PARTIAL => set_tilt_and_schedule_tick(l, state, pos, FULL, true),
            FULL => reset_tilt(l, state, pos),
            _ => {}
        }
    }
}

// BigDripleafBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, state: State, pos: Pos, _neighbor_block: BlockId, _moved: bool) {
    if l.has_neighbor_signal(pos) {
        reset_tilt(l, state, pos);
    }
}

// BigDripleafBlock.playTiltSound: Mth.randomBetween(level.random, 0.8F, 1.2F)
// draws one float; the sound reaches nobody.
fn play_tilt_sound(l: &mut Level) {
    l.random.next_float();
}

// BigDripleafBlock.setTiltAndScheduleTick (setTilt always succeeds: the
// EntityChangeBlockEvent has no listener).
fn set_tilt_and_schedule_tick(l: &mut Level, state: State, pos: Pos, new_tilt: u16, sound: bool) {
    set_tilt(l, state, pos, new_tilt);
    if sound {
        play_tilt_sound(l);
    }
    let delay = delay_until_next_tilt_state(new_tilt);
    if delay != -1 {
        l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
    }
}

// BigDripleafBlock.resetTilt: the sound plays if the leaf (as it was passed)
// was tilted.
fn reset_tilt(l: &mut Level, state: State, pos: Pos) {
    set_tilt(l, state, pos, NONE_TILT);
    if l.reg.get(state, tilt(l)) != NONE_TILT {
        play_tilt_sound(l);
    }
}

// Tilt.causesVibration: every tilt but unstable.
fn causes_vibration(tilt: u16) -> bool {
    tilt != UNSTABLE
}

// BigDripleafBlock.setTilt: setBlock(pos, state.setValue(TILT, tilt), 2), then
// a block change event with no source that sculk sensors hear when the new
// tilt causes a vibration and differs from the old one.
fn set_tilt(l: &mut Level, state: State, pos: Pos, new_tilt: u16) {
    let old_tilt = l.reg.get(state, tilt(l));
    let tilted = l.reg.with(state, tilt(l), new_tilt);
    l.set(pos, tilted, crate::level::UPDATE_CLIENTS);
    if causes_vibration(new_tilt) && new_tilt != old_tilt {
        l.game_event_at(None, crate::game_event!(l, "block_change"), pos);
    }
}
