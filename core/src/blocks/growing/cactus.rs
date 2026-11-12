// CactusBlock: stands on cactus or sand with nothing solid or lava beside it.
// A cactus that cannot survive schedules a tick (updateShape) and breaks in
// it. An item entity inside it is hurt.

use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::entity::Entity;
use crate::in_tag;
use crate::is_block;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::registry::{State, LIQUID, SOLID};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), can_survive: Some(can_survive), tick: Some(tick), entity_inside: Some(entity_inside), all_entities: true, ..NONE };

// CactusBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    if !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    }
}

// CactusBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, _direction: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        l.schedule_tick(pos, l.block(state), 1, PRIORITY_NORMAL);
    }
    state
}

// CactusBlock.canSurvive: a side that is solid or holds lava decides first.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    for dir in HORIZONTAL {
        let side = l.get(pos.relative(dir));
        if l.reg.flags(side) & SOLID != 0 || l.reg.fluids[l.fluid_of(side) as usize].fluid.is_lava() {
            return false;
        }
    }
    let below = l.get(pos.below());
    if !(is_block!(l, below, "minecraft:cactus") || in_tag!(l, below, "minecraft:sand")) {
        return false;
    }
    let above = l.get(pos.above());
    l.reg.flags(above) & LIQUID == 0
}

// CactusBlock.entityInside: EntityInsideBlockEvent (no listener) and
// entity.hurt(cactus damage, 1.0F).
fn entity_inside(l: &mut Level, _state: State, _pos: Pos, entity: &mut Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    crate::entity::hurt(l, entity, crate::entity::damage::Dmg::Cactus, 1.0f32);
}
