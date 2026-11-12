// BasePressurePlateBlock, PressurePlateBlock and WeightedPressurePlateBlock
// (which differ in what the signal counts: weighted_pressure_plate.rs). A plate is pressed by the
// entities in the box over it: the wooden types by every entity, the stone
// types by living ones only (the level has none). It is checked when an
// entity is inside it and by its own tick while pressed.

use super::weighted_pressure_plate;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::{self, Entity};
use crate::game_event::Source;
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_CLIENTS};
use crate::phys::Aabb;
use crate::property;
use crate::registry::{State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    get_signal: Some(get_signal),
    get_direct_signal: Some(get_direct_signal),
    can_survive: Some(can_survive),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// PressurePlateBlock.POWERED.
fn powered(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

fn is_weighted(l: &Level, state: State) -> bool {
    l.reg.plates[&l.block(state)].1.is_some()
}

// getSignalForState.
pub fn signal_for_state(l: &Level, state: State) -> i32 {
    if is_weighted(l, state) {
        weighted_pressure_plate::signal_for_state(l, state)
    } else if l.reg.get_bool(state, powered(l)) {
        15
    } else {
        0
    }
}

// setSignalForState.
fn set_signal_for_state(l: &Level, state: State, strength: i32) -> State {
    if is_weighted(l, state) {
        weighted_pressure_plate::set_signal_for_state(l, state, strength)
    } else {
        l.reg.with_bool(state, powered(l), strength > 0)
    }
}

// getPressedTime.
fn pressed_time(l: &Level, state: State) -> i32 {
    if is_weighted(l, state) {
        10
    } else {
        20
    }
}

// BasePressurePlateBlock.TOUCH_AABB moved to the position.
fn touch_box(pos: Pos) -> Aabb {
    Aabb::new(0.0625 + pos.x() as f64, pos.y() as f64, 0.0625 + pos.z() as f64, 0.9375 + pos.x() as f64, 0.25 + pos.y() as f64, 0.9375 + pos.z() as f64)
}

// The ids of the entities BasePressurePlateBlock.getEntities finds, none of
// which is a spectator or ignores block triggers (a marker armor stand does);
// `me` is the entity being ticked, if one is. A plate that senses only mobs
// counts the living ones.
fn pressing_entities(l: &Level, state: State, pos: Pos, me: Option<&Entity>) -> usize {
    let only = if l.reg.plates[&l.block(state)].0 { entity::Only::Living } else { entity::Only::Any };
    entity::entities_in(l, me, None, only, &touch_box(pos), |e| !matches!(&e.kind, entity::Kind::ArmorStand(_)) || !entity::armor_stand::ignores_block_triggers(e)).len()
}

// getSignalStrength. The interaction events have no listener.
fn signal_strength(l: &Level, state: State, pos: Pos, me: Option<&Entity>) -> i32 {
    let count = pressing_entities(l, state, pos, me);
    match l.reg.plates[&l.block(state)].1 {
        None => {
            if count > 0 {
                15
            } else {
                0
            }
        }
        Some(max_weight) => weighted_pressure_plate::signal_strength(count as i32, max_weight),
    }
}

// BasePressurePlateBlock.updateShape.
fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if dir == Dir::Down && !can_survive(l, state, pos) {
        l.known.air
    } else {
        state
    }
}

// BasePressurePlateBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    // canSupportRigidBlock, or canSupportCenter(level, pos, UP) (the
    // direction is not DOWN, so the unstable-bottom-center tag is not read).
    crate::shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Rigid) || crate::shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Center)
}

// BasePressurePlateBlock.tick.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let signal = signal_for_state(l, state);
    if signal > 0 {
        check_pressed(l, None, pos, state, signal, None);
    }
}

// BasePressurePlateBlock.entityInside.
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut Entity, _applier: &mut crate::entity::inside::Applier, _past_edges: bool) {
    // (EntityInsideBlockEvent has no listener.)
    let signal = signal_for_state(l, state);
    if signal == 0 {
        let source = Source::of(l, entity);
        check_pressed(l, Some(source), pos, state, signal, Some(&*entity));
    }
}

// BasePressurePlateBlock.checkPressed. The redstone event
// (BlockRedstoneEvent) has no listener: the strength stands.
fn check_pressed(l: &mut Level, entity: Option<Source>, pos: Pos, state: State, current: i32, me: Option<&Entity>) {
    let strength = signal_strength(l, state, pos, me);
    let was_pressed = current > 0;
    let pressed = strength > 0;
    if current != strength {
        let changed = set_signal_for_state(l, state, strength);
        l.set(pos, changed, UPDATE_CLIENTS);
        update_neighbours(l, pos, state);
    }
    if !pressed && was_pressed {
        let event = crate::game_event!(l, "block_deactivate");
        l.game_event_at(entity, event, pos);
    } else if pressed && !was_pressed {
        let event = crate::game_event!(l, "block_activate");
        l.game_event_at(entity, event, pos);
    }
    if pressed {
        let (block, time) = (l.block(state), pressed_time(l, state));
        l.schedule_tick(pos, block, time, PRIORITY_NORMAL);
    }
}

// BasePressurePlateBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved && signal_for_state(l, state) > 0 {
        update_neighbours(l, pos, state);
    }
}

// BasePressurePlateBlock.updateNeighbours.
fn update_neighbours(l: &mut Level, pos: Pos, state: State) {
    let block = l.block(state);
    l.update_neighbors_at(pos, block);
    l.update_neighbors_at(pos.below(), block);
}

fn get_signal(l: &mut Level, state: State, _pos: Pos, _side: Dir) -> i32 {
    signal_for_state(l, state)
}

fn get_direct_signal(l: &mut Level, state: State, _pos: Pos, side: Dir) -> i32 {
    if side == Dir::Up {
        signal_for_state(l, state)
    } else {
        0
    }
}
