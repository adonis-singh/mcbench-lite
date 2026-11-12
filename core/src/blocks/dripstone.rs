// PointedDripstoneBlock: what keeps it up and how its thickness follows its
// neighbours (updateShape, canSurvive), and its tick: a stalagmite that
// cannot stand breaks, a stalactite falls (spawnFallingStalactite). Random
// ticks are off (the random tick speed is 0), and the player and projectile
// paths are not inputs. Its shape is offset by its position
// (crate::shape_ops::collision_shape_at). An entity that lands on a stalagmite's
// tip falls two and a half blocks further.

use super::{Behaviour, NONE};
use crate::dir::{Dir, Pos};
use crate::entity::falling;
use crate::level::{Level, PRIORITY_NORMAL};
use crate::fluid::WATER_TICK_DELAY;
use crate::registry::{Fluid, State, Support};

pub const BEHAVIOUR: Behaviour = Behaviour { update_shape: Some(update_shape), tick: Some(tick), can_survive: Some(can_survive), fall_on: Some(fall_on), all_entities: true, ..NONE };

// PointedDripstoneBlock.fallOn: an entity landing on the tip of a stalagmite
// takes the fall plus 2.5 (Entity.causeFallDamage with the stalagmite damage and
// a multiplier of 2.0, which no kind of entity here reads); else Block.fallOn.
fn fall_on(l: &mut Level, state: State, _pos: Pos, entity: &mut crate::entity::Entity, fall_distance: f64) {
    let distance = if is_stalagmite(l, state) && thickness(l, state) == "tip" { fall_distance + 2.5 } else { fall_distance };
    crate::entity::motion::cause_fall_damage(l, entity, distance);
}

fn tip_direction(l: &Level, state: State) -> Dir {
    l.reg.get_dir(state, l.reg.property_of(state, "vertical_direction"))
}

fn thickness(l: &Level, state: State) -> &str {
    l.reg.value_name(state, l.reg.property_of(state, "thickness"))
}

fn is_dripstone(l: &Level, state: State) -> bool {
    l.reg.blocks[l.block(state) as usize].key == "minecraft:pointed_dripstone"
}

fn is_pointed_dripstone_with_direction(l: &Level, state: State, dir: Dir) -> bool {
    is_dripstone(l, state) && tip_direction(l, state) == dir
}

fn is_stalactite(l: &Level, state: State) -> bool {
    is_pointed_dripstone_with_direction(l, state, Dir::Down)
}

fn is_stalagmite(l: &Level, state: State) -> bool {
    is_pointed_dripstone_with_direction(l, state, Dir::Up)
}

fn is_tip(l: &Level, state: State, is_tip_merge: bool) -> bool {
    is_dripstone(l, state) && {
        let t = thickness(l, state);
        t == "tip" || (is_tip_merge && t == "tip_merge")
    }
}

// isValidPointedDripstonePlacement.
fn is_valid_placement(l: &mut Level, pos: Pos, dir: Dir) -> bool {
    let block_pos = pos.relative(dir.opposite());
    let state = l.get(block_pos);
    crate::shape_ops::is_face_sturdy(l, state, block_pos, dir, Support::Full) || is_pointed_dripstone_with_direction(l, state, dir)
}

fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    let dir = tip_direction(l, state);
    is_valid_placement(l, pos, dir)
}

// calculateDripstoneThickness.
fn calculate_thickness(l: &mut Level, pos: Pos, dir: Dir, is_tip_merge: bool) -> &'static str {
    let opposite = dir.opposite();
    let state = l.get(pos.relative(dir));
    if !is_pointed_dripstone_with_direction(l, state, opposite) {
        if !is_pointed_dripstone_with_direction(l, state, dir) {
            "tip"
        } else {
            let t = thickness(l, state);
            if t != "tip" && t != "tip_merge" {
                let behind = l.get(pos.relative(opposite));
                if !is_pointed_dripstone_with_direction(l, behind, dir) {
                    "base"
                } else {
                    "middle"
                }
            } else {
                "frustum"
            }
        }
    } else if !is_tip_merge && thickness(l, state) != "tip_merge" {
        "tip"
    } else {
        "tip_merge"
    }
}

fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, _neighbor_pos: Pos, _neighbor: State) -> State {
    if l.reg.get_bool(state, l.reg.property_of(state, "waterlogged")) {
        l.schedule_fluid_tick(pos, Fluid::Water, WATER_TICK_DELAY);
    }
    if dir != Dir::Up && dir != Dir::Down {
        return state;
    }
    let tip = tip_direction(l, state);
    let block = l.block(state);
    if tip == Dir::Down && l.has_scheduled_tick(pos, block) {
        state
    } else if dir == tip.opposite() && !can_survive(l, state, pos) {
        l.schedule_tick(pos, block, if tip == Dir::Down { 2 } else { 1 }, PRIORITY_NORMAL);
        state
    } else {
        let merge = thickness(l, state) == "tip_merge";
        let new = calculate_thickness(l, pos, tip, merge);
        l.reg.with_name(state, l.reg.property_of(state, "thickness"), new)
    }
}

fn tick(l: &mut Level, state: State, pos: Pos) {
    if is_stalagmite(l, state) && !can_survive(l, state, pos) {
        l.destroy_block(pos, true, 512);
    } else {
        spawn_falling_stalactite(l, state, pos);
    }
}

// spawnFallingStalactite: each stalactite block from here down to its tip
// becomes a falling block; the tip's hurts what it lands on.
fn spawn_falling_stalactite(l: &mut Level, state: State, pos: Pos) {
    let mut mutable = pos;
    let mut state = state;
    while is_stalactite(l, state) {
        let entity = falling::fall(l, mutable, state);
        if is_tip(l, state, true) {
            let max = (1 + pos.y() - mutable.y()).max(6);
            falling::set_hurts_entities(l, entity, 1.0f32 * max as f32, 40);
            break;
        }
        mutable = mutable.below();
        state = l.get(mutable);
    }
}
