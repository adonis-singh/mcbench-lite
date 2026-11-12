// SnifferEggBlock: onPlace schedules the hatching tick, which first cracks
// the egg twice and then hatches a sniffer. Paper's sniffer hatch times are
// at their defaults (24000 ticks, 12000 boosted by the block below).

use crate::blocks::shapes::common::play_sound_with_random_pitch;
use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::game_event::Context;
use crate::in_tag;
use crate::level::{refuse, Level, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour { on_place: Some(on_place), tick: Some(tick), ..NONE };

// SnifferEggBlock.HATCH.
fn hatch(l: &Level) -> u16 {
    property!(l, "hatch", ["0", "1", "2"])
}

// SnifferEggBlock.onPlace: hatchBoost (the block below is in the hatch-boost
// tag); the level event reaches nobody; the block place game event is heard by
// sculk sensors; the tick is scheduled at a third of the hatch time plus
// level.random.nextInt(300).
fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    let below = l.get(pos.below());
    let time = if in_tag!(l, below, "minecraft:sniffer_egg_hatch_boost") { 12000 } else { 24000 };
    l.game_event_in(crate::game_event!(l, "block_place"), pos, Context::of_state(state));
    let delay = time / 3 + l.random.next_int_bound(300);
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
}

// SnifferEggBlock.tick: a crack (the block grow event has no listener), or the
// hatching, which removes the egg and spawns a sniffer.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let level = l.reg.get_int(state, hatch(l));
    if level != 2 {
        l.set(pos, l.reg.with_int(state, hatch(l), level + 1), UPDATE_CLIENTS);
        play_sound_with_random_pitch(l);
    } else {
        play_sound_with_random_pitch(l);
        l.destroy_block(pos, false, 512);
        refuse(format!("a sniffer hatching at {pos}"));
    }
}
