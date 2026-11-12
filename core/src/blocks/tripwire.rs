// TripWireBlock, with Paper's world configuration at the defaults
// (block-updates.disable-tripwire-updates off). An entity inside a tripwire
// presses it, and it checks itself ten ticks on while it is pressed.

use super::tripwire_hook::{self, calculate_state, hook_block, tripwire_block};
use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::inside::Applier;
use crate::entity::{self, Entity};
use crate::level::{Level, PRIORITY_NORMAL, UPDATE_ALL};
use crate::phys::Aabb;
use crate::property;
use crate::registry::{Shape, State};
use crate::shape_ops;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    on_place: Some(on_place),
    affect_neighbors_after_removal: Some(affect_neighbors_after_removal),
    tick: Some(tick),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

// TripWireBlock.ATTACHED.
fn attached_property(l: &Level) -> u16 {
    property!(l, "attached", ["true", "false"])
}

// TripWireBlock.getShape: SHAPE_ATTACHED (Block.column(16, 1, 2.5)) or
// SHAPE_NOT_ATTACHED (Block.column(16, 0, 8)); also its entity inside shape.
pub fn outline_shape(l: &Level, state: State) -> Shape {
    if l.reg.get_bool(state, attached_property(l)) {
        shape_ops::block_box(0.0, 1.0, 0.0, 16.0, 2.5, 16.0)
    } else {
        shape_ops::block_box(0.0, 0.0, 0.0, 16.0, 8.0, 16.0)
    }
}

// TripWireBlock.entityInside: an entity in an unpowered tripwire presses it.
fn entity_inside(l: &mut Level, state: State, pos: Pos, entity: &mut Entity, _applier: &mut Applier, _past_edges: bool) {
    if !l.reg.get_bool(state, powered_property(l)) {
        // (EntityInsideBlockEvent has no listener.)
        check_pressed(l, pos, Some(&*entity));
    }
}

// TripWireBlock.tick: a powered tripwire checks the entities in its box.
fn tick(l: &mut Level, _state: State, pos: Pos) {
    let state = l.get(pos);
    if l.reg.get_bool(state, powered_property(l)) {
        check_pressed(l, pos, None);
    }
}

// TripWireBlock.checkPressed(level, pos) and (level, pos, entities): the
// entities are the one given (`inside`, which is out of the level's lookup
// while it is ticked), or those in the tripwire's box (getEntities(null, box):
// not spectators). One that does not ignore block triggers (a marker armor
// stand does) presses it; EntityInteractEvent has no listener, so it is never
// cancelled.
fn check_pressed(l: &mut Level, pos: Pos, inside: Option<&Entity>) {
    let state = l.get(pos);
    let was_powered = l.reg.get_bool(state, powered_property(l));
    let pressed = match inside {
        Some(entity) => !ignores_block_triggers(entity),
        None => {
            let shape = outline_shape(l, state);
            let b = shape.single.expect("a box as the tripwire's shape");
            let bounds = Aabb::new(b[0] + pos.x() as f64, b[1] + pos.y() as f64, b[2] + pos.z() as f64, b[3] + pos.x() as f64, b[4] + pos.y() as f64, b[5] + pos.z() as f64);
            !entity::entities_in(l, None, None, entity::Only::Any, &bounds, |e| !ignores_block_triggers(e)).is_empty()
        }
    };
    let mut state = state;
    if pressed != was_powered {
        state = l.reg.with_bool(state, powered_property(l), pressed);
        l.set(pos, state, UPDATE_ALL);
        update_source(l, pos, state);
    }
    if pressed {
        let block = l.block(state);
        l.schedule_tick(pos, block, 10, PRIORITY_NORMAL);
    }
}

// Entity.isIgnoringBlockTriggers: a marker armor stand.
fn ignores_block_triggers(e: &Entity) -> bool {
    matches!(&e.kind, entity::Kind::ArmorStand(_)) && entity::armor_stand::ignores_block_triggers(e)
}

// TripWireBlock.POWERED.
fn powered_property(l: &Level) -> u16 {
    property!(l, "powered", ["true", "false"])
}

// TripWireBlock.PROPERTY_BY_DIRECTION.
fn side_property(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::North => property!(l, "north", ["true", "false"]),
        Dir::East => property!(l, "east", ["true", "false"]),
        Dir::South => property!(l, "south", ["true", "false"]),
        Dir::West => property!(l, "west", ["true", "false"]),
        _ => panic!("{dir:?} has no tripwire side"),
    }
}

// TripWireBlock.shouldConnectTo.
fn should_connect_to(l: &Level, state: State, direction: Dir) -> bool {
    let block = l.block(state);
    if block == hook_block(l) {
        l.reg.get_dir(state, tripwire_hook::facing_property(l)) == direction.opposite()
    } else {
        block == tripwire_block(l)
    }
}

// TripWireBlock.updateShape; BlockBehaviour's returns the state.
fn update_shape(l: &mut Level, state: State, _pos: Pos, direction: Dir, _neighbor_pos: Pos, neighbor: State) -> State {
    if direction.is_horizontal() {
        l.reg.with_bool(state, side_property(l, direction), should_connect_to(l, neighbor, direction))
    } else {
        state
    }
}

// TripWireBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        update_source(l, pos, state);
    }
}

// TripWireBlock.affectNeighborsAfterRemoval.
fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    if !moved {
        update_source(l, pos, l.reg.with_bool(state, powered_property(l), true));
    }
}

// TripWireBlock.updateSource: the hook at the end of the wire to the south
// and to the west, if it faces this way, recalculates with this wire as the
// state at its distance.
fn update_source(l: &mut Level, pos: Pos, state: State) {
    let this = l.block(state);
    for direction in [Dir::South, Dir::West] {
        for i in 1..42 {
            let block_pos = pos.relative_n(direction, i);
            let block_state = l.get(block_pos);
            if l.block(block_state) == hook_block(l) {
                if l.reg.get_dir(block_state, tripwire_hook::facing_property(l)) == direction.opposite() {
                    calculate_state(l, block_pos, block_state, false, true, i, Some(state));
                }
                break;
            }
            if l.block(block_state) != this {
                break;
            }
        }
    }
}
