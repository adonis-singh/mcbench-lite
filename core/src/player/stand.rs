// Where the player can stand on a build: the place player mode stands it
// as it starts (web/live-world.mjs standBeside). Each column's top is tried
// with Minecraft's own test of a place a player stands up at, the one a bed
// or a vehicle uses (DismountHelper.findSafeDismountLocation), so the player
// stands on the blocks as Minecraft would let it and never inside them. Only
// a column's top is tried: the player stands under the open sky, on the
// build and not in one of its rooms.

use super::{read, Pose};
use crate::collision::{self, Ctx};
use crate::dir::Pos;
use crate::level::Level;
use crate::phys::Aabb;

// The open spot on top of a column of the box `min`..`max` nearest to `near`
// (the feet's x, y, z): the feet of a player standing there, or none when no
// column has one. Columns are tried in rings around `near`, nearest first,
// until no column further out can be nearer than the spot found.
pub fn standing_spot(l: &Level, min: [i32; 3], max: [i32; 3], near: [f64; 3]) -> Option<[f64; 3]> {
    let (cx, cz) = (near[0].floor() as i32, near[2].floor() as i32);
    let reach = [min[0] - cx, max[0] - cx, min[2] - cz, max[2] - cz].into_iter().map(i32::abs).max().unwrap();
    let mut best: Option<(f64, [f64; 3])> = None;
    // Takes column x, z's spot if it is nearer than the nearest yet.
    let consider = |best: &mut Option<(f64, [f64; 3])>, x: i32, z: i32| {
        if x < min[0] || x > max[0] || z < min[2] || z > max[2] {
            return;
        }
        let Some(spot) = column_spot(l, x, z, min[1], max[1]) else { return };
        let distance = ((spot[0] - near[0]).powi(2) + (spot[1] - near[1]).powi(2) + (spot[2] - near[2]).powi(2)).sqrt();
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            *best = Some((distance, spot));
        }
    };
    for r in 0..=reach {
        // A column of ring r has its middle at least r - 0.5 across from `near`.
        if best.is_some_and(|(nearest, _)| r as f64 - 0.5 >= nearest) {
            break;
        }
        if r == 0 {
            consider(&mut best, cx, cz);
            continue;
        }
        for x in cx - r..=cx + r {
            consider(&mut best, x, cz - r);
            consider(&mut best, x, cz + r);
        }
        for z in cz - r + 1..cz + r {
            consider(&mut best, cx - r, z);
            consider(&mut best, cx + r, z);
        }
    }
    best.map(|(_, spot)| spot)
}

// The feet of a player standing on the top of column x, z between y `bottom`
// and `top`: in the top block (one lower than a full block, a slab) or on it.
fn column_spot(l: &Level, x: i32, z: i32, bottom: i32, top: i32) -> Option<[f64; 3]> {
    let highest = (bottom..=top).rev().find(|&y| {
        let pos = Pos::new(x, y, z);
        !collision::shape_of(l, read::state(l, pos), pos, Ctx::Empty).is_empty()
    })?;
    find_safe_dismount_location(l, Pos::new(x, highest, z)).or_else(|| find_safe_dismount_location(l, Pos::new(x, highest + 1, z)))
}

// DismountHelper.findSafeDismountLocation(EntityType.PLAYER, level, pos,
// false): a creative player takes no harm from the blocks it stands on, so
// none is refused as dangerous. The world border is millions of blocks from
// any build.
fn find_safe_dismount_location(l: &Level, pos: Pos) -> Option<[f64; 3]> {
    // BlockGetter.getBlockFloorHeight.
    let floor = match non_climbable_top(l, pos) {
        Some(top) => top,
        None => match non_climbable_top(l, pos.below()) {
            Some(below) if below >= 1.0 => below - 1.0,
            _ => f64::NEG_INFINITY,
        },
    };
    // DismountHelper.isBlockFloorValid.
    if floor.is_infinite() || floor >= 1.0 {
        return None;
    }
    // Vec3.upFromBottomCenterOf, and EntityDimensions.makeBoundingBox.
    let feet = [pos.x() as f64 + 0.5, pos.y() as f64 + floor, pos.z() as f64 + 0.5];
    let (width, height, _) = Pose::Standing.dimensions();
    let half = (width / 2.0f32) as f64;
    let aabb = Aabb::new(feet[0] - half, feet[1], feet[2] - half, feet[0] + half, feet[1] + height as f64, feet[2] + half);
    if !collision::no_collision(l, &aabb, Ctx::Empty) {
        return None;
    }
    let invalid = |pos: Pos| l.reg.in_tag(read::state(l, pos), "minecraft:invalid_spawn_inside");
    if invalid(pos) || invalid(pos.above()) {
        return None;
    }
    Some(feet)
}

// VoxelShape.max(Axis.Y) of DismountHelper.nonClimbableShape: the top of the
// block's collision shape, none for an empty one, a climbable block's or an
// open trapdoor's.
fn non_climbable_top(l: &Level, pos: Pos) -> Option<f64> {
    let state = read::state(l, pos);
    let reg = l.reg;
    if reg.in_tag(state, "minecraft:climbable") || (reg.blocks[l.block(state) as usize].class == "TrapDoorBlock" && reg.get_bool(state, reg.property_of(state, "open"))) {
        return None;
    }
    let shape = collision::shape_of(l, state, pos, Ctx::Empty);
    (!shape.is_empty()).then(|| super::autojump::shape_max_y(&shape))
}
