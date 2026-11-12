// ConduitBlock and ConduitBlockEntity. Every 40 ticks the conduit looks for
// water in the 3x3x3 cube around it and the prismarine in the frame around
// that; an active one gives its power to players in range (none), attacks a
// hostile mob in the water near it (none exists) and makes its ambient
// sounds, whose timing draws from the level's random.

use super::Special;
use crate::blocks::shapes::common::schedule_water_tick;
use crate::blocks::{Behaviour, BlockEntity, NONE};
use crate::dir::{Dir, Pos};
use crate::level::Level;
use crate::registry::State;

pub const BEHAVIOUR: Behaviour = Behaviour {
    update_shape: Some(update_shape),
    trigger_event: Some(super::trigger_event),
    new_block_entity: Some(new_block_entity),
    ticker: Some(ticker),
    ..NONE
};

// ConduitBlockEntity: whether it is active and the game time its short ambient sound may next play after. (The tick count
// and the rotation only animate.)
#[derive(Clone, Debug, Default)]
pub struct Conduit {
    is_active: bool,
    next_ambient_sound_activation: i64,
}

fn new_block_entity(_l: &mut Level, _state: State, _pos: Pos) -> BlockEntity {
    BlockEntity::Special(Special::Conduit(Conduit::default()))
}

fn update_shape(l: &mut Level, state: State, pos: Pos, _dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    schedule_water_tick(l, state, pos);
    state
}

fn ticker(_l: &Level, _state: State) -> Option<fn(&mut Level, Pos, State)> {
    Some(server_tick)
}

// ConduitBlockEntity.updateShape: whether the cube is water all through and
// the frame has 16 blocks of the four kinds.
fn update_shape_cube(l: &mut Level, pos: Pos) -> bool {
    for i in -1..=1 {
        for i1 in -1..=1 {
            for i2 in -1..=1 {
                let fluid = l.fluid_state(Pos::new(pos.x() + i, pos.y() + i1, pos.z() + i2));
                // Level.isWaterAt: the fluid is in the water tag.
                if !l.fluid_type(fluid).is_water() {
                    return false;
                }
            }
        }
    }
    let mut count = 0;
    for i in -2..=2i32 {
        for i1 in -2..=2i32 {
            for i2 in -2..=2i32 {
                let (abs, abs1, abs2) = (i.abs(), i1.abs(), i2.abs());
                if (abs > 1 || abs1 > 1 || abs2 > 1) && (i == 0 && (abs1 == 2 || abs2 == 2) || i1 == 0 && (abs == 2 || abs2 == 2) || i2 == 0 && (abs == 2 || abs1 == 2)) {
                    let state = l.get(Pos::new(pos.x() + i, pos.y() + i1, pos.z() + i2));
                    let key = l.reg.blocks[l.block(state) as usize].key.as_str();
                    if matches!(key, "minecraft:prismarine" | "minecraft:prismarine_bricks" | "minecraft:sea_lantern" | "minecraft:dark_prismarine") {
                        count += 1;
                    }
                }
            }
        }
    }
    count >= 16
}

// ConduitBlockEntity.serverTick. The sounds reach no one; the effects reach
// no player; the attack finds no mob (selectNewTarget draws from the level's
// random only for a non-empty list).
fn server_tick(l: &mut Level, pos: Pos, _state: State) {
    let game_time = l.game_time;
    if game_time % 40 == 0 {
        let active = update_shape_cube(l, pos);
        let Some(BlockEntity::Special(Special::Conduit(conduit))) = l.block_entities.get_mut(&pos) else { panic!("no conduit at {pos}") };
        conduit.is_active = active;
    }
    let Some(BlockEntity::Special(Special::Conduit(conduit))) = l.block_entities.get_mut(&pos) else { panic!("no conduit at {pos}") };
    if conduit.is_active && game_time > conduit.next_ambient_sound_activation {
        let delay = l.random.next_int_bound(40);
        let Some(BlockEntity::Special(Special::Conduit(conduit))) = l.block_entities.get_mut(&pos) else { unreachable!() };
        conduit.next_ambient_sound_activation = game_time + 60 + delay as i64;
    }
}
