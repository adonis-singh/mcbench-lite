// NoteBlock.

use super::{Behaviour, InputPlayer, NONE};
use crate::dir::{Axis, Dir, Pos};
use crate::level::{Level, UPDATE_ALL};
use crate::property;
use crate::registry::{BlockId, State};
use crate::game_event::Source;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    neighbor_changed: Some(neighbor_changed),
    trigger_event: Some(trigger_event),
    use_without_item: Some(use_without_item),
    ..NONE
};

fn instrument(l: &Level) -> u16 {
    property!(
        l,
        "instrument",
        [
            "harp", "basedrum", "snare", "hat", "bass", "flute", "bell", "guitar", "chime", "xylophone", "iron_xylophone", "cow_bell", "didgeridoo", "bit", "banjo",
            "pling", "zombie", "skeleton", "creeper", "dragon", "wither_skeleton", "piglin", "custom_head"
        ]
    )
}

fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

// NoteBlockInstrument.worksAboveNoteBlock: every type but BASE_BLOCK, the
// mob heads and the custom head.
fn works_above(name: &str) -> bool {
    matches!(name, "zombie" | "skeleton" | "creeper" | "dragon" | "wither_skeleton" | "piglin" | "custom_head")
}

// NoteBlock.setInstrument: the block above's instrument if it works above a
// note block, else the block below's (harp if that one works above).
fn set_instrument(l: &mut Level, pos: Pos, state: State) -> State {
    let reg = l.reg;
    let above = l.get(pos.above());
    let name = reg.instrument(above);
    if works_above(name) {
        return reg.with_name(state, instrument(l), name);
    }
    let below = l.get(pos.below());
    let below_name = reg.instrument(below);
    reg.with_name(state, instrument(l), if works_above(below_name) { "harp" } else { below_name })
}

// NoteBlock.updateShape; BlockBehaviour's returns the state. Paper's
// disableNoteblockUpdates is at its default, off.
fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if dir.axis() == Axis::Y {
        set_instrument(l, pos, state)
    } else {
        state
    }
}

// NoteBlock.neighborChanged.
fn neighbor_changed(l: &mut Level, mut state: State, pos: Pos, _neighbor: BlockId, _moved: bool) {
    let has_neighbor_signal = l.has_neighbor_signal(pos);
    if has_neighbor_signal != l.reg.get_bool(state, powered(l)) {
        if has_neighbor_signal {
            play_note(l, state, pos, None);
            state = l.get(pos);
        }
        l.set(pos, l.reg.with_bool(state, powered(l), has_neighbor_signal), UPDATE_ALL);
    }
}

// NoteBlock.playNote by an entity or none.
fn play_note(l: &mut Level, state: State, pos: Pos, entity: Option<Source>) {
    if works_above(l.reg.value_name(state, instrument(l))) || {
        let above = l.get(pos.above());
        l.is_air(above)
    } {
        l.block_event(pos, l.block(state), 0, 0);
        let event = crate::game_event!(l, "note_block_play");
        l.game_event_at(entity, event, pos);
    }
}

// NoteBlock.triggerEvent. The note play event has no listener. A tunable
// instrument spawns a particle and pitches the sound, neither of which
// reaches a block. The sound is seeded from the level's random, which is
// drawn either way unless a custom head has no sound.
fn trigger_event(l: &mut Level, state: State, pos: Pos, _id: i32, _param: i32) -> bool {
    if l.reg.value_name(state, instrument(l)) == "custom_head" {
        // getCustomSoundId: the block above must be a skull block entity,
        // which this port does not model (it refuses to make one).
        let _ = l.block_entity(pos.above());
        return false;
    }
    l.random.next_long();
    true
}

fn note(l: &Level) -> u16 {
    property!(l, "note", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24"])
}

// NoteBlock.useWithoutItem: the note goes to the next (Paper's
// disableNoteblockUpdates is at its default, off), then the note plays for the
// player. The statistic the player is awarded changes no block.
fn use_without_item(l: &mut Level, state: State, pos: Pos, player: Option<InputPlayer>) -> bool {
    let state = l.reg.cycle(state, note(l));
    l.set(pos, state, UPDATE_ALL);
    play_note(l, state, pos, player.map(|p| p.source));
    true
}
