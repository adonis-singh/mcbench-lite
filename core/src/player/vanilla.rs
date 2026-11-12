// The collisions as the 1.21.10 client computes them: BlockCollisions (the
// blocks around a box in Cursor3D's order, a block whose shape is
// Shapes.block() itself tested with AABB.intersects, any other shape with
// Shapes.joinIsNotEmpty) and Shapes.collide with VoxelShape.collideX. Paper's
// moonrise computes the same for the cases the server meets (collision.rs);
// at the epsilon edges (a box that overlaps a full block by less than 1.0E-7)
// the two differ, and a player moves as the client does.

use super::Player;
use crate::collision::Ctx;
use crate::dir::Pos;
use crate::level::Level;
use crate::phys::{self, jmax, jmin, Aabb, Vec3};
use crate::registry::{Shape, AIR};
use crate::shape_ops;

fn and(a: bool, b: bool) -> bool {
    a && b
}

// BlockCollisions: the shapes (moved to their blocks) that collide with the
// box, in the order the client walks them, each with its block's position.
pub fn block_shapes(l: &Level, aabb: &Aabb, context: Ctx, only_suffocating: bool) -> Vec<(Pos, Shape)> {
    // (The world border's walls are not modelled.)
    if !(aabb.min_x > -2.9e7 && aabb.max_x < 2.9e7 && aabb.min_z > -2.9e7 && aabb.max_z < 2.9e7) {
        crate::level::refuse("a box at the world border");
    }
    let (min_x, max_x) = (phys::floor(aabb.min_x - 1.0E-7) - 1, phys::floor(aabb.max_x + 1.0E-7) + 1);
    let (min_y, max_y) = (phys::floor(aabb.min_y - 1.0E-7) - 1, phys::floor(aabb.max_y + 1.0E-7) + 1);
    let (min_z, max_z) = (phys::floor(aabb.min_z - 1.0E-7) - 1, phys::floor(aabb.max_z + 1.0E-7) + 1);
    let entity_shape = shape_ops::create(aabb.min_x, aabb.min_y, aabb.min_z, aabb.max_x, aabb.max_y, aabb.max_z);
    let mut out = Vec::new();
    // Cursor3D: x fastest, then y, then z.
    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let edges = (x == min_x || x == max_x) as i32 + (y == min_y || y == max_y) as i32 + (z == min_z || z == max_z) as i32;
                if edges == 3 {
                    continue;
                }
                let pos = Pos::new(x, y, z);
                let state = super::read::state(l, pos);
                if l.reg.flags(state) & AIR != 0 {
                    continue;
                }
                if (only_suffocating && !super::is_suffocating(l, state)) || !crate::collision::takes_part(l, state, edges) {
                    continue;
                }
                let shape = crate::collision::shape_of(l, state, pos, context);
                if super::is_block_shape(l, state) && !matches!(l.reg.blocks[l.block(state) as usize].class.as_str(), "ScaffoldingBlock" | "PowderSnowBlock") {
                    if aabb.intersects_box(x as f64, y as f64, z as f64, x as f64 + 1.0, y as f64 + 1.0, z as f64 + 1.0) {
                        out.push((pos, shape.moved(x as f64, y as f64, z as f64)));
                    }
                } else {
                    let moved = shape.moved(x as f64, y as f64, z as f64);
                    if !moved.is_empty() && shape_ops::join_is_not_empty(&moved, &entity_shape, and) {
                        out.push((pos, moved));
                    }
                }
            }
        }
    }
    out
}

// The index of the last coordinate at or before `d` (VoxelShape.findIndex on
// an ArrayVoxelShape: Mth.binarySearch for the first coordinate beyond it,
// less one).
fn find_index(coords: &[f64], d: f64) -> i32 {
    let (mut min, mut i) = (0i32, coords.len() as i32);
    while i > 0 {
        let j = i / 2;
        let k = min + j;
        if d < coords[k as usize] {
            i = j;
        } else {
            min = k + 1;
            i -= j + 1;
        }
    }
    min - 1
}

// VoxelShape.collide(axis, box, movement).
fn voxel_collide(axis: usize, shape: &Shape, bb: &Aabb, movement: f64) -> f64 {
    if shape.is_empty() {
        return movement;
    }
    if movement.abs() < 1.0E-7 {
        return 0.0;
    }
    // The cycle of axes from the movement's: along, then the two across.
    let (a, b, c) = match axis {
        0 => (0, 1, 2),
        1 => (1, 2, 0),
        _ => (2, 0, 1),
    };
    let max = bb.max(a);
    let min = bb.min(a);
    let first = find_index(&shape.coords[a], min + 1.0E-7);
    let last = find_index(&shape.coords[a], max - 1.0E-7);
    let b0 = 0.max(find_index(&shape.coords[b], bb.min(b) + 1.0E-7));
    let b1 = (shape.size[b] as i32).min(find_index(&shape.coords[b], bb.max(b) - 1.0E-7) + 1);
    let c0 = 0.max(find_index(&shape.coords[c], bb.min(c) + 1.0E-7));
    let c1 = (shape.size[c] as i32).min(find_index(&shape.coords[c], bb.max(c) - 1.0E-7) + 1);
    let size = shape.size[a] as i32;
    let full = |k: i32, i: i32, j: i32| {
        let mut at = [0usize; 3];
        at[a] = k as usize;
        at[b] = i as usize;
        at[c] = j as usize;
        shape.is_full(at[0], at[1], at[2])
    };
    let mut movement = movement;
    if movement > 0.0 {
        for k in (last + 1)..size {
            for i in b0..b1 {
                for j in c0..c1 {
                    if full(k, i, j) {
                        let d = shape.coords[a][k as usize] - max;
                        if d >= -1.0E-7 {
                            movement = jmin(movement, d);
                        }
                        return movement;
                    }
                }
            }
        }
    } else if movement < 0.0 {
        for k in (0..first).rev() {
            for i in b0..b1 {
                for j in c0..c1 {
                    if full(k, i, j) {
                        let d = shape.coords[a][k as usize + 1] - min;
                        if d <= 1.0E-7 {
                            movement = jmax(movement, d);
                        }
                        return movement;
                    }
                }
            }
        }
    }
    movement
}

// Shapes.collide(axis, box, shapes, movement).
fn collide_axis(axis: usize, bb: &Aabb, shapes: &[&Shape], movement: f64) -> f64 {
    let mut movement = movement;
    for shape in shapes {
        if movement.abs() < 1.0E-7 {
            return 0.0;
        }
        movement = voxel_collide(axis, shape, bb, movement);
    }
    movement
}

// Direction.axisStepOrder(movement).
fn axis_step_order(v: Vec3) -> [usize; 3] {
    if v.x.abs() < v.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    }
}

// Entity.collideWithShapes.
pub fn collide_with_shapes(movement: Vec3, bb: &Aabb, shapes: &[&Shape]) -> Vec3 {
    if shapes.is_empty() {
        return movement;
    }
    let mut moved = Vec3::ZERO;
    for axis in axis_step_order(movement) {
        let d = movement.get(axis);
        if d != 0.0 {
            let r = collide_axis(axis, &bb.mv(moved.x, moved.y, moved.z), shapes, d);
            moved = moved.with(axis, r);
        }
    }
    moved
}

impl Player {
    // The block shapes colliding with a box, for this player.
    pub(super) fn block_shapes(&self, l: &Level, aabb: &Aabb) -> Vec<(Pos, Shape)> {
        block_shapes(l, aabb, self.context(), false)
    }

    // EntityGetter.getEntityCollisions(player, box): the boxes of the entities
    // the player collides with, boats (Entity.canCollideWith; no other
    // entity of the level can be collided with), in the order of their ids.
    pub(super) fn entity_shapes(&self, l: &Level, aabb: &Aabb) -> Vec<Shape> {
        use crate::entity::{entities_in, Kind, Only};
        // AABB.getSize.
        if (aabb.xsize() + aabb.ysize() + aabb.zsize()) / 3.0 < 1.0E-7 {
            return Vec::new();
        }
        let mut ids = entities_in(l, None, None, Only::Any, &aabb.inflate(1.0E-7, 1.0E-7, 1.0E-7), |e| matches!(e.kind, Kind::Boat(_)));
        ids.sort();
        ids.into_iter()
            .map(|id| {
                let b = l.entities.get(None, id).bb;
                shape_ops::create(b.min_x, b.min_y, b.min_z, b.max_x, b.max_y, b.max_z)
            })
            .collect()
    }
}
