// BrushableBlock (suspicious sand and gravel) and BrushableBlockEntity: it
// schedules a tick when it is placed or its neighbours change, and the tick
// lets it fall when nothing is under it: a falling block entity that does
// not drop an item when it lands (disableDrop; it breaks and posts
// block_destroy, see entity::falling). Nothing brushes it, so its brush
// count stays 0.

use super::Special;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    on_place: Some(on_place),
    tick: Some(tick),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ..NONE
};

// BrushableBlock.TICK_DELAY.
const TICK_DELAY: i32 = 2;

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::BrushableBlock { item: None })
}

fn on_place(l: &mut Level, state: State, pos: Pos, _old: State, _moved: bool) {
    l.schedule_tick(pos, l.block(state), TICK_DELAY, PRIORITY_NORMAL);
}

fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    l.schedule_tick(pos, l.block(state), TICK_DELAY, PRIORITY_NORMAL);
    state
}

// FallingBlock.isFree.
pub fn is_free(l: &Level, state: State) -> bool {
    l.is_air(state) || l.reg.in_tag(state, "minecraft:fire") || l.reg.flags(state) & crate::registry::LIQUID != 0 || l.reg.flags(state) & crate::registry::REPLACEABLE != 0
}

// BrushableBlock.tick: BrushableBlockEntity.checkReset (a block nobody
// brushed has nothing to reset), then falling.
fn tick(l: &mut Level, state: State, pos: Pos) {
    l.block_entity(pos);
    let below = l.get(pos.below());
    if is_free(l, below) && pos.y() >= crate::world::MIN_Y {
        let entity = crate::entity::falling::fall(l, pos, state);
        crate::entity::falling::disable_drop(l, entity);
    }
}
