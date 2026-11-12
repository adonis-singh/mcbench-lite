// BaseFireBlock, FireBlock and SoulFireBlock, in a level whose doFireTick
// game rule is off (McbenchLevel and the live server's world both turn it
// off): a fire block only reschedules its tick, and neither burns nor
// spreads. Placing one looks for a nether portal frame to light, and fire
// breaks when it has nothing to burn or stand on. An entity inside fire is
// set alight and hurt (BaseFireBlock.entityInside).

use super::portal_shape;
use crate::blocks::{Behaviour, NONE};
use crate::dir::{Axis, Dir, Pos};
use crate::entity::inside::{self, Applier, Effect};
use crate::entity::Entity;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::property;
use crate::registry::{State, Support};

pub const FIRE: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    on_place: Some(on_place),
    tick: Some(tick),
    can_survive: Some(can_survive),
    entity_inside: Some(entity_inside),
    all_entities: true,
    ..NONE
};

pub const SOUL_FIRE: Behaviour = Behaviour {
    update_shape: Some(soul_update_shape),
    on_place: Some(base_on_place),
    can_survive: Some(soul_can_survive),
    entity_inside: Some(soul_entity_inside),
    all_entities: true,
    ..NONE
};

// BaseFireBlock.entityInside (EntityInsideBlockEvent has no listener): the
// entity's freezing is cleared, it is set alight and, once alight, hurt by
// the block's fire damage.
fn base_entity_inside(applier: &mut Applier, fire_damage: f32) {
    applier.apply(inside::CLEAR_FREEZE);
    applier.apply(inside::FIRE_IGNITE);
    applier.run_after(inside::FIRE_IGNITE, Effect::FireHurt(fire_damage));
}

// FireBlock passes 1.0F as the fire damage.
fn entity_inside(_l: &mut Level, _state: State, _pos: Pos, _entity: &mut Entity, applier: &mut Applier, _past_edges: bool) {
    base_entity_inside(applier, 1.0f32);
}

// SoulFireBlock passes 2.0F.
fn soul_entity_inside(_l: &mut Level, _state: State, _pos: Pos, _entity: &mut Entity, applier: &mut Applier, _past_edges: bool) {
    base_entity_inside(applier, 2.0f32);
}

fn age_property(l: &Level) -> u16 {
    property!(l, "age", ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13", "14", "15"])
}

fn side_property(l: &Level, dir: Dir) -> u16 {
    match dir {
        Dir::North => property!(l, "north", ["true", "false"]),
        Dir::East => property!(l, "east", ["true", "false"]),
        Dir::South => property!(l, "south", ["true", "false"]),
        Dir::West => property!(l, "west", ["true", "false"]),
        Dir::Up => property!(l, "up", ["true", "false"]),
        Dir::Down => panic!("fire has no down side"),
    }
}

fn waterlogged_property(l: &Level) -> u16 {
    property!(l, "waterlogged", ["true", "false"])
}

// FireBlock.getIgniteOdds(BlockState): a waterlogged block does not catch.
fn ignite_odds(l: &Level, state: State) -> i32 {
    let block = &l.reg.blocks[l.block(state) as usize];
    if block.properties.iter().any(|&(id, _)| id == waterlogged_property(l)) && l.reg.get_bool(state, waterlogged_property(l)) {
        0
    } else {
        l.reg.fire_odds.get(&l.block(state)).map_or(0, |&(ignite, _)| ignite)
    }
}

// FireBlock.canBurn.
fn can_burn(l: &Level, state: State) -> bool {
    ignite_odds(l, state) > 0
}

// SoulFireBlock.canSurviveOnBlock.
fn soul_fire_base(l: &Level, state: State) -> bool {
    l.reg.in_tag(state, "minecraft:soul_fire_base_blocks")
}

// FireBlock.isValidFireLocation: any neighbour can burn.
fn is_valid_fire_location(l: &mut Level, pos: Pos) -> bool {
    for dir in crate::dir::ALL {
        let state = l.get(pos.relative(dir));
        if can_burn(l, state) {
            return true;
        }
    }
    false
}

// FireBlock.canSurvive.
fn can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    crate::shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Full) || is_valid_fire_location(l, pos)
}

// SoulFireBlock.canSurvive.
fn soul_can_survive(l: &mut Level, _state: State, pos: Pos) -> bool {
    let below = l.get(pos.below());
    soul_fire_base(l, below)
}

// FireBlock.getStateForPlacement(level, pos): a fire with the sides that
// have something to burn when it has neither a burnable block nor a sturdy
// top to stand on.
fn state_for_placement(l: &mut Level, pos: Pos) -> State {
    let fire = l.reg.block_key("minecraft:fire");
    let default = l.reg.default_state(fire);
    let below = l.get(pos.below());
    if !can_burn(l, below) && !crate::shape_ops::is_face_sturdy(l, below, pos.below(), Dir::Up, Support::Full) {
        let mut state = default;
        for dir in crate::dir::ALL {
            if dir == Dir::Down {
                continue;
            }
            let neighbour = l.get(pos.relative(dir));
            let burns = can_burn(l, neighbour);
            state = l.reg.with_bool(state, side_property(l, dir), burns);
        }
        state
    } else {
        default
    }
}

// BaseFireBlock.getState: soul fire over a soul fire base, else fire.
pub fn fire_state(l: &mut Level, pos: Pos) -> State {
    let below = l.get(pos.below());
    if soul_fire_base(l, below) {
        let soul = l.reg.block_key("minecraft:soul_fire");
        l.reg.default_state(soul)
    } else {
        state_for_placement(l, pos)
    }
}

// FireBlock.getStateWithAge.
fn state_with_age(l: &mut Level, pos: Pos, age: i32) -> State {
    let state = fire_state(l, pos);
    if l.reg.blocks[l.block(state) as usize].key == "minecraft:fire" {
        l.reg.with_int(state, age_property(l), age)
    } else {
        state
    }
}

// FireBlock.updateShape on the server. BlockFadeEvent has no listener.
fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if !can_survive(l, state, pos) {
        return l.known.air;
    }
    let age = l.reg.get_int(state, age_property(l));
    state_with_age(l, pos, age)
}

// SoulFireBlock.updateShape.
fn soul_update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if soul_can_survive(l, state, pos) {
        l.reg.default_state(l.block(state))
    } else {
        l.known.air
    }
}

// BaseFireBlock.onPlace: the level is the overworld, where fire placed in a
// frame lights the portal (and stops there); otherwise fire with nothing to
// stand on goes out. BlockFadeEvent has no listener.
fn base_on_place(l: &mut Level, state: State, pos: Pos, old: State, _moved: bool) {
    if l.block(old) != l.block(state) {
        if let Some(shape) = portal_shape::find_empty_portal_shape(l, pos, Axis::X) {
            shape.create_portal_blocks(l);
            return;
        }
        if !crate::blocks::can_survive(l, state, pos) {
            l.remove_block(pos, false);
        }
    }
}

// FireBlock.getFireTickDelay: Paper's fire-tick-delay (30) and 0 to 9 from
// the level's random.
fn fire_tick_delay(l: &mut Level) -> i32 {
    30 + l.random.next_int_bound(10)
}

// FireBlock.onPlace.
fn on_place(l: &mut Level, state: State, pos: Pos, old: State, moved: bool) {
    base_on_place(l, state, pos, old, moved);
    let delay = fire_tick_delay(l);
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
}

// FireBlock.tick: the next tick is scheduled; the rest is behind the
// doFireTick game rule, which is off.
fn tick(l: &mut Level, state: State, pos: Pos) {
    let delay = fire_tick_delay(l);
    l.schedule_tick(pos, l.block(state), delay, PRIORITY_NORMAL);
}
