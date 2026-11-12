// Clipping a segment against blocks: VoxelShape.clip as moonrise patches it,
// the shape's boxes (VoxelShape.toAabbs), and BlockGetter.traverseBlocks, which
// ClipContext-based clips (a fall that resets fall damage, a falling block's
// look for water) and an explosion's rays build on. Only whether the segment
// hits is asked of the results, never where or on which face.

use crate::collision::find_floor;
use crate::dir::Pos;
use crate::phys::{self, Aabb, Vec3};
use crate::registry::Shape;

// DiscreteVoxelShape.forAllBoxes(consumer, true) as BitSetDiscreteVoxelShape
// does it with moonrise's flat bitset: the boxes (as voxel indices
// minX, minY, minZ, maxX, maxY, maxZ) that cover the full voxels, merged
// along z, then x, then y.
fn voxel_boxes(shape: &Shape) -> Vec<[usize; 6]> {
    let [size_x, size_y, size_z] = shape.size;
    let inc_y = size_z;
    let inc_x = size_z * size_y;
    let mut bits = shape.full.clone();
    let first_set = |bits: &[bool], from: usize, to: usize| (from..to).find(|&i| bits[i]);
    let first_clear = |bits: &[bool], from: usize, to: usize| (from..to).find(|&i| !bits[i]);
    let is_range_set = |bits: &[bool], from: usize, to: usize| first_clear(bits, from, to).is_none();
    let clear_range = |bits: &mut [bool], from: usize, to: usize| {
        for bit in &mut bits[from..to] {
            *bit = false;
        }
    };
    let mut boxes = Vec::new();
    for y in 0..size_y {
        let index_y = y * inc_y;
        for x in 0..size_x {
            let index_x = index_y + x * inc_x;
            let end_index = index_x + size_z;
            let mut z_index = index_x;
            while z_index < end_index {
                let Some(first_set_z) = first_set(&bits, z_index, end_index) else { break };
                let last_set_z = first_clear(&bits, first_set_z, end_index).unwrap_or(end_index);
                clear_range(&mut bits, first_set_z, last_set_z);
                let mut end_x = x + 1;
                let mut neighbour_start = first_set_z + inc_x;
                let mut neighbour_end = last_set_z + inc_x;
                while end_x < size_x && is_range_set(&bits, neighbour_start, neighbour_end) {
                    end_x += 1;
                    clear_range(&mut bits, neighbour_start, neighbour_end);
                    neighbour_start += inc_x;
                    neighbour_end += inc_x;
                }
                let mut end_y = y + 1;
                let mut first_zy = first_set_z + inc_y;
                let mut last_zy = last_set_z + inc_y;
                'merge: while end_y < size_y {
                    let (mut test_x, mut start, mut end) = (x, first_zy, last_zy);
                    while test_x < end_x {
                        if !is_range_set(&bits, start, end) {
                            break 'merge;
                        }
                        test_x += 1;
                        start += inc_x;
                        end += inc_x;
                    }
                    end_y += 1;
                    let (mut test_x, mut start, mut end) = (x, first_zy, last_zy);
                    while test_x < end_x {
                        clear_range(&mut bits, start, end);
                        test_x += 1;
                        start += inc_x;
                        end += inc_x;
                    }
                    first_zy += inc_y;
                    last_zy += inc_y;
                }
                boxes.push([x, y, first_set_z - index_x, end_x, end_y, last_set_z - index_x]);
                z_index = last_set_z;
            }
        }
    }
    boxes
}

impl Shape {
    // VoxelShape.toAabbs: min x, y, z, max x, y, z of each box.
    pub fn boxes(&self) -> &[[f64; 6]] {
        self.boxes.get_or_init(|| {
            if let Some(single) = self.single {
                return vec![single];
            }
            voxel_boxes(self)
                .into_iter()
                .map(|[min_x, min_y, min_z, max_x, max_y, max_z]| {
                    let (x, y, z) = (&self.coords[0], &self.coords[1], &self.coords[2]);
                    [x[min_x], y[min_y], z[min_z], x[max_x], y[max_y], z[max_z]]
                })
                .collect()
        })
    }
}

// CollisionUtil.strictlyContains(shape, x, y, z) for a shape at the origin.
fn strictly_contains(shape: &Shape, x: f64, y: f64, z: f64) -> bool {
    if let Some(single) = shape.single {
        return x >= single[0] && x < single[3] && y >= single[1] && y < single[4] && z >= single[2] && z < single[5];
    }
    if shape.is_empty() {
        return false;
    }
    let [size_x, size_y, size_z] = shape.size;
    let index_x = find_floor(&shape.coords[0], 0.0, x, 0, size_x);
    if index_x < 0 || index_x as usize >= size_x {
        return false;
    }
    let index_y = find_floor(&shape.coords[1], 0.0, y, 0, size_y);
    if index_y < 0 || index_y as usize >= size_y {
        return false;
    }
    let index_z = find_floor(&shape.coords[2], 0.0, z, 0, size_z);
    if index_z < 0 || index_z as usize >= size_z {
        return false;
    }
    shape.is_full(index_x as usize, index_y as usize, index_z as usize)
}

// Whether AABB.clip(boxes, start, end, pos) finds a box: some box is entered
// through a face within the segment.
fn boxes_hit(boxes: &[[f64; 6]], start: Vec3, end: Vec3, pos: Pos) -> bool {
    boxes.iter().any(|b| {
        let moved = Aabb::new(b[0] + pos.x() as f64, b[1] + pos.y() as f64, b[2] + pos.z() as f64, b[3] + pos.x() as f64, b[4] + pos.y() as f64, b[5] + pos.z() as f64);
        phys::clip_box([moved.min_x, moved.min_y, moved.min_z], [moved.max_x, moved.max_y, moved.max_z], start, end).is_some()
    })
}

// VoxelShape.clip(start, end, pos) != null, for a shape at the origin.
pub fn shape_clip_hits(shape: &Shape, start: Vec3, end: Vec3, pos: Pos) -> bool {
    if shape.is_empty() {
        return false;
    }
    let direction = end.subtract(start);
    if direction.length_sqr() < 1.0E-7 {
        return false;
    }
    let from_behind = start.add_vec(direction.scale(0.001));
    let (x, y, z) = (from_behind.x - pos.x() as f64, from_behind.y - pos.y() as f64, from_behind.z - pos.z() as f64);
    strictly_contains(shape, x, y, z) || boxes_hit(shape.boxes(), start, end, pos)
}

// Shapes.box(0, 0, 0, 1, height, 1).clip(start, end, pos) != null (Shapes.block()
// for a height of 1: a fluid's shape, a full block).
pub fn box_clip_hits(height: f64, start: Vec3, end: Vec3, pos: Pos) -> bool {
    let direction = end.subtract(start);
    if direction.length_sqr() < 1.0E-7 {
        return false;
    }
    let from_behind = start.add_vec(direction.scale(0.001));
    let (x, y, z) = (from_behind.x - pos.x() as f64, from_behind.y - pos.y() as f64, from_behind.z - pos.z() as f64);
    (x >= 0.0 && x < 1.0 && y >= 0.0 && y < height && z >= 0.0 && z < 1.0) || boxes_hit(&[[0.0, 0.0, 0.0, 1.0, height, 1.0]], start, end, pos)
}

// Shapes.block().clip(start, end, pos) != null.
pub fn block_clip_hits(start: Vec3, end: Vec3, pos: Pos) -> bool {
    box_clip_hits(1.0, start, end, pos)
}

// BlockGetter.traverseBlocks: the blocks the segment passes through, the
// first for which `test` gives a result; None where none does (the fail
// function's result is the caller's). Segments that start where they end give
// none.
pub fn traverse_blocks<T>(from: Vec3, to: Vec3, mut test: impl FnMut(Pos) -> Option<T>) -> Option<T> {
    if from.x.to_bits() == to.x.to_bits() && from.y.to_bits() == to.y.to_bits() && from.z.to_bits() == to.z.to_bits() {
        return None;
    }
    let d = phys::lerp(-1.0E-7, to.x, from.x);
    let d1 = phys::lerp(-1.0E-7, to.y, from.y);
    let d2 = phys::lerp(-1.0E-7, to.z, from.z);
    let d3 = phys::lerp(-1.0E-7, from.x, to.x);
    let d4 = phys::lerp(-1.0E-7, from.y, to.y);
    let d5 = phys::lerp(-1.0E-7, from.z, to.z);
    let (mut x, mut y, mut z) = (phys::floor(d3), phys::floor(d4), phys::floor(d5));
    if let Some(found) = test(Pos::new(x, y, z)) {
        return Some(found);
    }
    let d6 = d - d3;
    let d7 = d1 - d4;
    let d8 = d2 - d5;
    let (i, i1, i2) = (phys::sign(d6), phys::sign(d7), phys::sign(d8));
    let d9 = if i == 0 { f64::MAX } else { i as f64 / d6 };
    let d10 = if i1 == 0 { f64::MAX } else { i1 as f64 / d7 };
    let d11 = if i2 == 0 { f64::MAX } else { i2 as f64 / d8 };
    let mut d12 = d9 * if i > 0 { 1.0 - phys::frac(d3) } else { phys::frac(d3) };
    let mut d13 = d10 * if i1 > 0 { 1.0 - phys::frac(d4) } else { phys::frac(d4) };
    let mut d14 = d11 * if i2 > 0 { 1.0 - phys::frac(d5) } else { phys::frac(d5) };
    while d12 <= 1.0 || d13 <= 1.0 || d14 <= 1.0 {
        if d12 < d13 {
            if d12 < d14 {
                x += i;
                d12 += d9;
            } else {
                z += i2;
                d14 += d11;
            }
        } else if d13 < d14 {
            y += i1;
            d13 += d10;
        } else {
            z += i2;
            d14 += d11;
        }
        if let Some(found) = test(Pos::new(x, y, z)) {
            return Some(found);
        }
    }
    None
}
