// CampfireBlock and CampfireBlockEntity: four empty slots, so neither its
// cooking tick (lit) nor its cooling tick (unlit) has anything to do, and
// no item is put on it. An entity inside it is burnt only if living (the armor
// stand).

use super::Special;
use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(super::idle_ticker),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

fn lit(l: &Level) -> u16 {
    property!(l, "lit", ["true", "false"])
}

// CampfireBlock.entityInside: a lit campfire hurts a living entity with the
// campfire damage (1 for a campfire, 2 for a soul campfire).
fn entity_inside(l: &mut Level, state: State, _pos: Pos, entity: &mut crate::entity::Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    if l.reg.get_bool(state, lit(l)) && matches!(entity.kind, crate::entity::Kind::ArmorStand(_)) {
        let damage = if l.reg.blocks[l.block(state) as usize].key == "minecraft:soul_campfire" { 2.0f32 } else { 1.0f32 };
        crate::entity::hurt(l, entity, crate::entity::damage::Dmg::Campfire, damage);
    }
}

fn signal_fire(l: &Level) -> u16 {
    property!(l, "signal_fire", ["true", "false"])
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::Campfire)
}

// CampfireBlock.updateShape: a waterlogged campfire schedules a water tick;
// signal fire follows whether hay is below.
fn update_shape(l: &mut Level, state: State, pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    if direction == Dir::Down {
        let hay = l.reg.blocks[l.block(neighbor) as usize].key == "minecraft:hay_block";
        l.reg.with_bool(state, signal_fire(l), hay)
    } else {
        state
    }
}
