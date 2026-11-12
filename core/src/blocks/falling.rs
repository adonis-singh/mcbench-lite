// FallingBlock and its subclasses: SandBlock and ColoredFallingBlock (gravel),
// ConcretePowderBlock, AnvilBlock and DragonEggBlock. A falling block schedules
// its tick when placed and when a neighbour changes; its tick lets it fall if
// nothing holds it up (FallingBlockEntity.fall). The player paths (an anvil's
// menu, the dragon egg's teleport) are not inputs.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::falling::{self, is_free, touches_liquid};
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;
use crate::world::MIN_Y;

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), on_place: Some(on_place), tick: Some(tick), ..NONE };

pub const CONCRETE_POWDER: Behaviour = Behaviour { update_shape: Some(concrete_update_shape), ..BEHAVIOUR };

// FallingBlock.getDelayAfterPlace: 2, the dragon egg's 5.
fn delay(l: &Level, state: State) -> i32 {
    if l.reg.blocks[l.block(state) as usize].class == "DragonEggBlock" {
        5
    } else {
        2
    }
}

fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    l.schedule_tick(pos, l.block(state), delay(l, state), PRIORITY_NORMAL);
}

fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    l.schedule_tick(pos, l.block(state), delay(l, state), PRIORITY_NORMAL);
    state
}

// ConcretePowderBlock.updateShape: where it touches water it turns to
// concrete (BlockFormEvent has no listener).
fn concrete_update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if touches_liquid(l, pos) {
        return falling::concrete_of(l, l.block(state));
    }
    update_shape(l, state, pos, dir, neighbor_pos, neighbor)
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    let below = l.get(pos.below());
    if is_free(l, below) && pos.y() >= MIN_Y {
        let entity = falling::fall(l, pos, state);
        // FallingBlock.falling: an anvil hurts what it lands on.
        if l.reg.blocks[l.block(state) as usize].class == "AnvilBlock" {
            falling::set_hurts_entities(l, entity, 2.0f32, 40);
        }
    }
}
