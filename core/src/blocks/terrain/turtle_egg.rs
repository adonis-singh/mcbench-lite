// TurtleEggBlock: stepOn and fallOn break an egg (one of its eggs, or the block
// with the last) for a living entity (canDestroyEgg: the armor stand; a turtle or
// bat, a mob, is not here), at a chance of one in a hundred for a step and one
// in three for a fall; onPlace sends a level event; hatching is a random tick,
// which never runs.

use crate::blocks::{Behaviour, NONE};
use crate::dir::Pos;
use crate::entity::{motion, Entity, Kind};
use crate::game_event::Context;
use crate::level::{Level, UPDATE_CLIENTS};
use crate::property;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    on_place: Some(on_place),
    step_on: Some(step_on),
    fall_on: Some(fall_on),
    all_entities: true,
    ..NONE
};

// TurtleEggBlock.EGGS.
fn eggs(l: &Level) -> u16 {
    property!(l, "eggs", ["1", "2", "3", "4"])
}

// TurtleEggBlock.onPlace: levelEvent(2012) when the egg is on sand, which
// reaches clients only.
fn on_place(_l: &mut Level, _state: State, _pos: Pos, _old: State, _moved: bool) {}

// TurtleEggBlock.canDestroyEgg: a living entity that is not a turtle or a bat
// breaks an egg, when the game rule mobGriefing (on) allows it.
fn can_destroy_egg(entity: &Entity) -> bool {
    // (An item, a falling block, primed TNT, a vehicle and a hanging entity are
    // no LivingEntity.)
    matches!(entity.kind, Kind::ArmorStand(_))
}

// TurtleEggBlock.stepOn: destroyEgg(.., 100) unless the entity steps carefully
// (Entity.isSteppingCarefully is isShiftKeyDown, which nothing here is).
fn step_on(l: &mut Level, pos: Pos, state: State, entity: &Entity) {
    destroy_egg(l, state, pos, entity, 100);
}

// TurtleEggBlock.fallOn: destroyEgg(.., 3) (nothing here is a Zombie), then
// Block.fallOn.
fn fall_on(l: &mut Level, state: State, pos: Pos, entity: &mut Entity, fall_distance: f64) {
    destroy_egg(l, state, pos, entity, 3);
    motion::cause_fall_damage(l, entity, fall_distance);
}

// TurtleEggBlock.destroyEgg: the draw level.random.nextInt(chance) is made only
// for an entity that can destroy an egg; EntityInteractEvent has no listener.
fn destroy_egg(l: &mut Level, state: State, pos: Pos, entity: &Entity, chance: i32) {
    if can_destroy_egg(entity) && l.random.next_int_bound(chance) == 0 {
        decrease_eggs(l, pos, state);
    }
}

// TurtleEggBlock.decreaseEggs: the break sound draws a float for its pitch;
// the last egg breaks the block (no drops), else one egg goes (flags 2) with a
// block destroy event for the sensors.
fn decrease_eggs(l: &mut Level, pos: Pos, state: State) {
    l.random.next_float();
    let count = l.reg.get_int(state, eggs(l));
    if count <= 1 {
        l.destroy_block_by(pos, false, 512, None);
    } else {
        l.set(pos, l.reg.with_int(state, eggs(l), count - 1), UPDATE_CLIENTS);
        l.game_event_in(crate::game_event!(l, "block_destroy"), pos, Context::of_state(state));
    }
}
