// The VoxelShape operations block code reads, ported from Paper 1.21.10's
// own VoxelShape (getFaceShape through calculateFaceDirect) and moonrise's
// CollisionUtil (sliceShape, isJoinNonEmpty and the coordinate merge it
// runs), on the shapes the registry holds (Registry::shapes). Shapes are
// held at offset 0 (a shape moved by a position offset is not modelled).

use crate::dir::{Axis, Dir};
use crate::phys::{jmax, jmin};
use crate::level::{refuse, Level};
use crate::registry::{Shape, State, Support, DYNAMIC_SHAPE, OFFSET};

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

// The shape Shapes.empty() is.
pub fn empty() -> Shape {
    Shape::new([vec![0.0], vec![0.0], vec![0.0]], [0, 0, 0], Vec::new(), None, false)
}

// Shapes.block().
pub fn block() -> Shape {
    Shape::new([vec![0.0, 1.0], vec![0.0, 1.0], vec![0.0, 1.0]], [1, 1, 1], vec![true], Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]), true)
}

// Shapes.box(x1, y1, z1, x2, y2, z2).
pub fn make_box(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Shape {
    Shape::new([vec![x1, x2], vec![y1, y2], vec![z1, z2]], [1, 1, 1], vec![true], Some([x1, y1, z1, x2, y2, z2]), false)
}

// Block.box(x1, y1, z1, x2, y2, z2) in sixteenths.
pub fn block_box(x1: f64, y1: f64, z1: f64, x2: f64, y2: f64, z2: f64) -> Shape {
    make_box(x1 / 16.0, y1 / 16.0, z1 / 16.0, x2 / 16.0, y2 / 16.0, z2 / 16.0)
}

// Mth.getSeed(x, y, z).
fn get_seed(x: i32, y: i32, z: i32) -> i64 {
    let l = (x.wrapping_mul(3129871) as i64) ^ (z as i64).wrapping_mul(116129781) ^ (y as i64);
    l.wrapping_mul(l).wrapping_mul(42317861).wrapping_add(l.wrapping_mul(11)) >> 16
}

// BlockBehaviour.OffsetType.XZ's offset of a block at a position, with the
// block's getMaxHorizontalOffset.
pub fn xz_offset(pos: crate::dir::Pos, max_horizontal: f32) -> (f64, f64) {
    let seed = get_seed(pos.x(), 0, pos.z());
    let clamp = |v: f64| v.max(-(max_horizontal as f64)).min(max_horizontal as f64);
    let x = clamp(((((seed & 15) as f32) / 15.0f32) as f64 - 0.5) * 0.5);
    let z = clamp(((((seed >> 8 & 15) as f32) / 15.0f32) as f64 - 0.5) * 0.5);
    (x, z)
}

// The largest horizontal offset (Block.getMaxHorizontalOffset) of a block
// whose shape is offset by position, by class.
fn max_horizontal_offset(class: &str) -> Option<f32> {
    match class {
        // BambooStalkBlock: OffsetType.XZ, the default maximum offset.
        "BambooStalkBlock" => Some(0.25f32),
        // PointedDripstoneBlock: OffsetType.XZ, the base's first x (2 sixteenths).
        "PointedDripstoneBlock" => Some(0.125f32),
        _ => None,
    }
}

// The shapes of states that offset their shape by position: the block's
// root shape (the registry holds the shape at the origin, offset included,
// which for these blocks is exact: the offset at the origin is the
// negative maximum) and its offset limit.
fn offset_root(l: &Level, state: State) -> (Shape, f32) {
    let reg = l.reg;
    let class = reg.blocks[l.block(state) as usize].class.as_str();
    let shape = &reg.shapes[reg.states[state as usize].collision_shape as usize];
    match max_horizontal_offset(class) {
        Some(max) => {
            let (ox, oz) = xz_offset(crate::dir::Pos::new(0, 0, 0), max);
            (shape.moved(-ox, 0.0, -oz), max)
        }
        None => refuse(format!("collision shape of {} offset by its position", reg.describe(state))),
    }
}

// The shape a state offset by position has before the offset (VoxelShape.move
// keeps the root coordinates and adds to the offset: the sum of the block's
// offset and the position is what a shape's coordinates are moved by), and the
// block's offset at the position.
pub fn offset_parts(l: &Level, state: State, pos: crate::dir::Pos) -> (Shape, f64, f64) {
    let (root, max) = offset_root(l, state);
    let (ox, oz) = xz_offset(pos, max);
    (root, ox, oz)
}

// The shape of a state's getCollisionShape(level, pos): the registry's
// shape at the origin, moved by the block's offset at the position; a moving
// piston's is its block entity's.
pub fn collision_shape_at(l: &Level, state: State, pos: crate::dir::Pos) -> std::borrow::Cow<'static, Shape> {
    let reg = l.reg;
    if reg.flags(state) & DYNAMIC_SHAPE != 0 && reg.blocks[l.block(state) as usize].class == "MovingPistonBlock" {
        return std::borrow::Cow::Owned(crate::blocks::moving_piston::shape_at(l, pos));
    }
    let shape: &'static Shape = &reg.shapes[reg.states[state as usize].collision_shape as usize];
    if reg.flags(state) & OFFSET == 0 || shape.is_empty() {
        return std::borrow::Cow::Borrowed(shape);
    }
    let (root, max) = offset_root(l, state);
    let (ox, oz) = xz_offset(pos, max);
    std::borrow::Cow::Owned(root.moved(ox, 0.0, oz))
}

// BlockStateBase.isFaceSturdy(level, pos, face, supportType). The state
// cache answers it (the dump), except for a block with a dynamic shape,
// whose support shape (getBlockSupportShape, its collision shape) is read
// at the position: bamboo and pointed dripstone (offset by position) and the
// moving piston (its block entity's shape); the others refuse.
pub fn is_face_sturdy(l: &Level, state: State, pos: crate::dir::Pos, face: Dir, support: Support) -> bool {
    if l.reg.flags(state) & DYNAMIC_SHAPE != 0 {
        match l.reg.blocks[l.block(state) as usize].class.as_str() {
            "BambooStalkBlock" | "PointedDripstoneBlock" => return shape_is_face_sturdy(&collision_shape_at(l, state, pos), face, support),
            "MovingPistonBlock" => return shape_is_face_sturdy(&crate::blocks::moving_piston::shape_at(l, pos), face, support),
            _ => {}
        }
    }
    l.reg.sturdy(state, face, support)
}

// BlockStateBase.isCollisionShapeFullBlock(level, pos): the dump's flag, but
// for a moving piston, whose collision shape is read at the position.
pub fn is_collision_shape_full_block(l: &Level, state: State, pos: crate::dir::Pos) -> bool {
    if l.reg.blocks[l.block(state) as usize].class == "MovingPistonBlock" {
        return crate::blocks::moving_piston::shape_at(l, pos).is_full_block();
    }
    l.reg.flags(state) & crate::registry::COLLISION_FULL != 0
}

// SupportType.isSupporting on a support shape.
pub fn shape_is_face_sturdy(shape: &Shape, face: Dir, support: Support) -> bool {
    match support {
        // Block.isFaceFull.
        Support::Full => shape.face_shape(face).is_full_block(),
        Support::Center => !join_is_not_empty(&shape.face_shape(face), &block_box(7.0, 0.0, 7.0, 9.0, 10.0, 9.0), only_second),
        Support::Rigid => {
            // Shapes.join(Shapes.block(), Block.column(12.0, 0.0, 16.0), ONLY_FIRST):
            // the block without its central column.
            let ring = Shape::new(
                [vec![0.0, 0.125, 0.875, 1.0], vec![0.0, 1.0], vec![0.0, 0.125, 0.875, 1.0]],
                [3, 1, 3],
                vec![true, true, true, true, false, true, true, true, true],
                None,
                false,
            );
            !join_is_not_empty(&shape.face_shape(face), &ring, only_second)
        }
    }
}

fn fuzzy_equals(a: f64, b: f64, tolerance: f64) -> bool {
    (a - b).abs() <= tolerance || a == b
}

// CollisionUtil.findFloor with a zero offset.
fn find_floor(values: &[f64], value: f64, start: i32, end: i32) -> i32 {
    let (mut start, mut end) = (start, end);
    loop {
        let middle = ((start + end) as u32 >> 1) as i32;
        let middle_val = values[middle as usize];
        if value < middle_val {
            end = middle - 1;
        } else {
            start = middle + 1;
        }
        if start > end {
            break;
        }
    }
    start - 1
}

impl Shape {
    // VoxelShape.move: every coordinate offset (the shape's own offset is
    // kept in the coordinates).
    pub fn moved(&self, dx: f64, dy: f64, dz: f64) -> Shape {
        if self.is_empty() {
            return empty();
        }
        let shift = |coords: &Vec<f64>, d: f64| coords.iter().map(|c| c + d).collect::<Vec<f64>>();
        let coords = [shift(&self.coords[0], dx), shift(&self.coords[1], dy), shift(&self.coords[2], dz)];
        let single = self.single.map(|_| [coords[0][0], coords[1][0], coords[2][0], coords[0][1], coords[1][1], coords[2][1]]);
        Shape::new(coords, self.size, self.full.clone(), single, false)
    }

    // moonrise's VoxelShape.isFullBlock.
    pub fn is_full_block(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        if let Some(b) = self.single {
            return b[0].abs() <= 1.0E-7 && b[1].abs() <= 1.0E-7 && b[2].abs() <= 1.0E-7 && (1.0 - b[3]).abs() <= 1.0E-7 && (1.0 - b[4]).abs() <= 1.0E-7 && (1.0 - b[5]).abs() <= 1.0E-7;
        }
        let mut min = self.size;
        let mut max = [0usize; 3];
        for x in 0..self.size[0] {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    if self.voxel(x, y, z) {
                        for (a, v) in [x, y, z].into_iter().enumerate() {
                            min[a] = min[a].min(v);
                            max[a] = max[a].max(v + 1);
                        }
                    }
                }
            }
        }
        let spans = (0..3).all(|a| self.coords[a][min[a]].abs() <= 1.0E-7 && (1.0 - self.coords[a][max[a]]).abs() <= 1.0E-7);
        spans && (min[0]..max[0]).all(|x| (min[1]..max[1]).all(|y| (min[2]..max[2]).all(|z| self.voxel(x, y, z))))
    }
    fn voxel(&self, x: usize, y: usize, z: usize) -> bool {
        self.full[(x * self.size[1] + y) * self.size[2] + z]
    }

    // VoxelShape.getFaceShape.
    pub fn face_shape(&self, side: Dir) -> Shape {
        if self.is_empty() {
            return self.clone();
        }
        let axis = axis_index(side.axis());
        let coords = &self.coords[axis];
        if coords.len() == 2 && fuzzy_equals(coords[0], 0.0, 1.0E-7) && fuzzy_equals(coords[1], 1.0, 1.0E-7) {
            return self.clone();
        }
        let positive = matches!(side, Dir::Up | Dir::South | Dir::East);
        let index = find_floor(coords, if positive { 0.9999999 } else { 1.0E-7 }, 0, coords.len() as i32 - 1);
        self.slice(axis, index)
    }

    // CollisionUtil.sliceShapeOptimised.
    fn slice(&self, axis: usize, index: i32) -> Shape {
        let size = self.size;
        if index < 0 || index as usize >= size[axis] {
            return empty();
        }
        let index = index as usize;
        let cube = |a: usize| self.coords[a].len() == 2 && self.coords[a][0] == 0.0 && self.coords[a][1] == 1.0;
        if cube(axis) {
            return self.clone();
        }
        let others: Vec<usize> = (0..3).filter(|&a| a != axis).collect();
        if cube(others[0]) && cube(others[1]) {
            let mut at = [0usize; 3];
            at[axis] = index;
            return if self.voxel(at[0], at[1], at[2]) { block() } else { empty() };
        }
        let mut local = size;
        local[axis] = 1;
        let mut full = Vec::with_capacity(local[0] * local[1] * local[2]);
        for x in 0..local[0] {
            for y in 0..local[1] {
                for z in 0..local[2] {
                    let mut at = [x, y, z];
                    at[axis] += index;
                    full.push(self.voxel(at[0], at[1], at[2]));
                }
            }
        }
        if !full.iter().any(|&f| f) {
            return empty();
        }
        let mut coords = self.coords.clone();
        coords[axis] = vec![0.0, 1.0];
        let single = (local == [1, 1, 1]).then(|| [coords[0][0], coords[1][0], coords[2][0], coords[0][1], coords[1][1], coords[2][1]]);
        Shape::new(coords, local, full, single, false)
    }

    // The shape's bounds (VoxelShape.bounds): min and max of its full voxels.
    pub fn bounds(&self) -> [f64; 6] {
        assert!(!self.is_empty(), "No bounds for empty shape.");
        if let Some(single) = self.single {
            return single;
        }
        let mut min = self.size;
        let mut max = [0usize; 3];
        for x in 0..self.size[0] {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    if self.voxel(x, y, z) {
                        for (a, v) in [x, y, z].into_iter().enumerate() {
                            min[a] = min[a].min(v);
                            max[a] = max[a].max(v + 1);
                        }
                    }
                }
            }
        }
        [self.coords[0][min[0]], self.coords[1][min[1]], self.coords[2][min[2]], self.coords[0][max[0]], self.coords[1][max[1]], self.coords[2][max[2]]]
    }

    // VoxelShape.max(axis): the top of the highest full voxel along the axis,
    // or negative infinity for a shape with none.
    pub fn max_along(&self, axis: Axis) -> f64 {
        let a = axis_index(axis);
        let mut top = 0usize;
        for x in 0..self.size[0] {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    if self.voxel(x, y, z) {
                        top = top.max([x, y, z][a] + 1);
                    }
                }
            }
        }
        if top == 0 {
            f64::NEG_INFINITY
        } else {
            self.coords[a][top]
        }
    }
}

// CollisionUtil.voxelShapeIntersect(AABB, AABB); a box is min x, y, z, max x, y, z.
fn boxes_intersect(a: [f64; 6], b: [f64; 6]) -> bool {
    a[0] - b[3] < -1.0E-7 && a[3] - b[0] > 1.0E-7 && a[1] - b[4] < -1.0E-7 && a[4] - b[1] > 1.0E-7 && a[2] - b[5] < -1.0E-7 && a[5] - b[2] > 1.0E-7
}

// CollisionUtil.voxelShapeIntersectNoEmpty.
fn shape_intersects_box(shape: &Shape, aabb: [f64; 6]) -> bool {
    if shape.is_empty() {
        return false;
    }
    let size = shape.size;
    let mut range = [(0usize, 0usize); 3];
    for a in 0..3 {
        let floor_min = find_floor(&shape.coords[a], aabb[a] + 1.0E-7, 0, size[a] as i32).max(0);
        if floor_min as usize >= size[a] {
            return false;
        }
        let ceil_max = (size[a] as i32).min(find_floor(&shape.coords[a], aabb[a + 3] - 1.0E-7, floor_min, size[a] as i32) + 1);
        if floor_min >= ceil_max {
            return false;
        }
        range[a] = (floor_min as usize, ceil_max as usize);
    }
    for x in range[0].0..range[0].1 {
        for y in range[1].0..range[1].1 {
            for z in range[2].0..range[2].1 {
                if shape.voxel(x, y, z) {
                    return true;
                }
            }
        }
    }
    false
}

// CollisionUtil.MergedVoxelCoordinateList.
struct Merged {
    first: Vec<i32>,
    second: Vec<i32>,
    // The merged coordinates (MergedVoxelCoordinateList.coordinates), the first
    // `voxels + 1` of which a joined shape keeps.
    coords: Vec<f64>,
    voxels: usize,
}

// MergedVoxelCoordinateList.merge, for two coordinate lists that are not
// the same array.
fn merge(first: &[f64], second: &[f64], ft: bool, tf: bool) -> Option<Merged> {
    let (first_count, second_count) = (first.len() as i32, second.len() as i32);
    let (voxels_first, voxels_second) = (first_count - 1, second_count - 1);
    let max_count = (first_count + second_count) as usize;
    let mut first_indices = vec![0i32; max_count];
    let mut second_indices = vec![0i32; max_count];
    let mut coordinates = vec![0.0f64; max_count];
    let (not_tf, not_ft) = (!tf, !ft);
    let (mut first_index, mut second_index) = (0i32, 0i32);
    let mut result_size = 0usize;
    let mut last = f64::NAN;
    loop {
        let none_left_first = first_index >= first_count;
        let none_left_second = second_index >= second_count;
        if none_left_first && none_left_second || none_left_second && not_tf || none_left_first && not_ft {
            return if result_size <= 1 { None } else { Some(Merged { first: first_indices, second: second_indices, coords: coordinates, voxels: result_size - 1 }) };
        }
        let first_zero = first_index == 0;
        let second_zero = second_index == 0;
        let select;
        if none_left_first {
            select = second[second_index as usize];
            second_index += 1;
        } else if none_left_second {
            select = first[first_index as usize];
            first_index += 1;
        } else {
            let break_first = not_tf && second_zero;
            let break_second = not_ft && first_zero;
            let f = first[first_index as usize];
            let s = second[second_index as usize];
            let use_first = f < s + 1.0E-7;
            let cont = use_first && break_first || !use_first && break_second;
            select = if use_first { f } else { s };
            first_index += use_first as i32;
            second_index += 1 ^ use_first as i32;
            if cont {
                continue;
            }
        }
        let mut prev_first = first_index - 1;
        if prev_first >= voxels_first {
            prev_first = -1;
        }
        let mut prev_second = second_index - 1;
        if prev_second >= voxels_second {
            prev_second = -1;
        }
        if last >= select - 1.0E-7 {
            first_indices[result_size - 1] = prev_first;
            second_indices[result_size - 1] = prev_second;
        } else {
            first_indices[result_size] = prev_first;
            second_indices[result_size] = prev_second;
            coordinates[result_size] = select;
            result_size += 1;
            last = select;
        }
    }
}

// Shapes.joinIsNotEmpty(first, second, operator), through moonrise's
// CollisionUtil.isJoinNonEmpty.
pub fn join_is_not_empty(first: &Shape, second: &Shape, operator: fn(bool, bool) -> bool) -> bool {
    if operator(false, false) {
        panic!("Ambiguous operator: (false, false) -> true");
    }
    let (first_empty, second_empty) = (first.is_empty(), second.is_empty());
    if first_empty || second_empty {
        return operator(!first_empty, !second_empty);
    }
    let tt = operator(true, true);
    // (Identical shapes are never joined here: `first == second` is not modelled.)
    let ft = operator(false, true);
    let tf = operator(true, false);
    match (first.single, second.single) {
        (Some(a), Some(b)) => {
            if !boxes_intersect(a, b) {
                return tf | ft;
            }
            if tt {
                return true;
            }
        }
        (Some(a), None) => {
            if !shape_intersects_box(second, a) {
                return tf | ft;
            }
            if tt {
                return true;
            }
        }
        (None, Some(b)) => {
            if !shape_intersects_box(first, b) {
                return tf | ft;
            }
            if tt {
                return true;
            }
        }
        (None, None) => {
            if !boxes_intersect(first.bounds(), second.bounds()) {
                return tf | ft;
            }
        }
    }
    let Some(merged_x) = merge(&first.coords[0], &second.coords[0], ft, tf) else { return false };
    let Some(merged_y) = merge(&first.coords[1], &second.coords[1], ft, tf) else { return false };
    let Some(merged_z) = merge(&first.coords[2], &second.coords[2], ft, tf) else { return false };
    let op = (ft as u32) << 1 | (tf as u32) << 2 | (tt as u32) << 3;
    // isMergeEmpty.
    for x in 0..merged_x.voxels {
        let (s1x, s2x) = (merged_x.first[x], merged_x.second[x]);
        for y in 0..merged_y.voxels {
            let (s1y, s2y) = (merged_y.first[y], merged_y.second[y]);
            for z in 0..merged_z.voxels {
                let (s1z, s2z) = (merged_z.first[z], merged_z.second[z]);
                let full1 = (s1x | s1y | s1z) >= 0 && first.voxel(s1x as usize, s1y as usize, s1z as usize);
                let full2 = (s2x | s2y | s2z) >= 0 && second.voxel(s2x as usize, s2y as usize, s2z as usize);
                if op & 1 << (full2 as u32 | (full1 as u32) << 1) != 0 {
                    return true;
                }
            }
        }
    }
    false
}

impl Shape {
    // VoxelShape.min(axis): the bottom of the lowest full voxel along the
    // axis, or positive infinity for a shape with none.
    pub fn min_along(&self, axis: Axis) -> f64 {
        let a = axis_index(axis);
        let mut bottom = self.size[a];
        for x in 0..self.size[0] {
            for y in 0..self.size[1] {
                for z in 0..self.size[2] {
                    if self.voxel(x, y, z) {
                        bottom = bottom.min([x, y, z][a]);
                    }
                }
            }
        }
        if bottom >= self.size[a] {
            f64::INFINITY
        } else {
            self.coords[a][bottom]
        }
    }

    // moonrise$getFaceShapeClamped.
    fn face_shape_clamped(&self, side: Dir) -> Shape {
        if self.is_empty() {
            return self.clone();
        }
        if self.single == Some([0.0, 0.0, 0.0, 1.0, 1.0, 1.0]) {
            // (Shapes.block().)
            return self.clone();
        }
        let axis = side.axis();
        let a = axis_index(axis);
        if matches!(side, Dir::Up | Dir::South | Dir::East) {
            if fuzzy_equals(self.max_along(axis), 1.0, 1.0E-7) {
                self.slice(a, self.size[a] as i32 - 1)
            } else {
                empty()
            }
        } else if fuzzy_equals(self.min_along(axis), 0.0, 1.0E-7) {
            self.slice(a, 0)
        } else {
            empty()
        }
    }

    // moonrise$occludesFullBlock.
    pub fn occludes_full(&self) -> bool {
        if self.is_empty() {
            return false;
        }
        if self.is_full_block() {
            return true;
        }
        if let Some(b) = self.single {
            return b[1] <= 1.0E-7 && b[4] >= 0.9999999 && b[0] <= 1.0E-7 && b[3] >= 0.9999999 && b[2] <= 1.0E-7 && b[5] >= 0.9999999;
        }
        !join_is_not_empty(&block(), self, only_first)
    }
}

// Shapes.mergedMayOccludeBlock.
fn merged_may_occlude_block(first: &Shape, second: &Shape) -> bool {
    let (a, b) = (first.bounds(), second.bounds());
    let min_x = jmin(a[0], b[0]);
    let min_y = jmin(a[1], b[1]);
    let min_z = jmin(a[2], b[2]);
    let max_x = jmax(a[3], b[3]);
    let max_y = jmax(a[4], b[4]);
    let max_z = jmax(a[5], b[5]);
    min_x <= 1.0E-7 && max_x >= 0.9999999 && min_y <= 1.0E-7 && max_y >= 0.9999999 && min_z <= 1.0E-7 && max_z >= 0.9999999
}

// Shapes.mergedFaceOccludes. (The "if cached" shortcuts it starts with and
// repeats on the face shapes answer true only where the computation after
// them does, so they are not kept.)
pub fn merged_face_occludes(first: &Shape, second: &Shape, direction: Dir) -> bool {
    if first.is_empty() & second.is_empty() {
        return false;
    }
    let new_first = first.face_shape_clamped(direction);
    let new_second = second.face_shape_clamped(direction.opposite());
    let first_empty = new_first.is_empty();
    let second_empty = new_second.is_empty();
    if first_empty & second_empty {
        false
    } else if first_empty | second_empty {
        if second_empty {
            new_first.occludes_full()
        } else {
            new_second.occludes_full()
        }
    } else {
        merged_may_occlude_block(&new_first, &new_second) && or_unoptimized(&new_first, &new_second).occludes_full()
    }
}

// moonrise$orUnoptimized.
fn or_unoptimized(first: &Shape, second: &Shape) -> Shape {
    if first.is_empty() {
        return second.clone();
    }
    if second.is_empty() {
        return first.clone();
    }
    join_unoptimized(first, second, or_op)
}

// BooleanOp.OR.
pub fn or_op(first: bool, second: bool) -> bool {
    first || second
}

// Math.round(double).
fn java_round(a: f64) -> i64 {
    let bits = a.to_bits() as i64;
    let biased_exp = (bits & 0x7FF0_0000_0000_0000u64 as i64) >> 52;
    let shift = (53 - 2 + 1023) - biased_exp;
    if (shift & -64) == 0 {
        let mut r = (bits & 0x000F_FFFF_FFFF_FFFFi64) | (0x000F_FFFF_FFFF_FFFFi64 + 1);
        if bits < 0 {
            r = -r;
        }
        ((r >> shift) + 1) >> 1
    } else {
        a as i64
    }
}

// Shapes.findBits.
fn find_bits(min_bits: f64, max_bits: f64) -> i32 {
    if !(min_bits < -1.0E-7) && !(max_bits > 1.0000001) {
        for i in 0..=3 {
            let i1 = 1i32 << i;
            let d = min_bits * i1 as f64;
            let d1 = max_bits * i1 as f64;
            let flag = (d - java_round(d) as f64).abs() < 1.0E-7 * i1 as f64;
            let flag1 = (d1 - java_round(d1) as f64).abs() < 1.0E-7 * i1 as f64;
            if flag && flag1 {
                return i;
            }
        }
        -1
    } else {
        -1
    }
}

// Shapes.generateCubeParts.
fn cube_parts(parts: usize) -> Vec<f64> {
    let inc = 1.0 / parts as f64;
    let mut out = Vec::with_capacity(parts + 1);
    let mut val = 0.0;
    for _ in 0..=parts {
        out.push(val);
        val += inc;
    }
    out
}

// Shapes.create(minX, minY, minZ, maxX, maxY, maxZ): a box on the voxel grid
// its coordinates fit (up to eighths), else the block's one voxel with the
// box's own coordinates.
pub fn create(min_x: f64, min_y: f64, min_z: f64, max_x: f64, max_y: f64, max_z: f64) -> Shape {
    if max_x - min_x < 1.0E-7 || max_y - min_y < 1.0E-7 || max_z - min_z < 1.0E-7 {
        return empty();
    }
    let (bits_x, bits_y, bits_z) = (find_bits(min_x, max_x), find_bits(min_y, max_y), find_bits(min_z, max_z));
    if bits_x < 0 || bits_y < 0 || bits_z < 0 {
        return make_box(min_x, min_y, min_z, max_x, max_y, max_z);
    }
    if bits_x == 0 && bits_y == 0 && bits_z == 0 {
        return block();
    }
    let (size_x, size_y, size_z) = (1usize << bits_x, 1usize << bits_y, 1usize << bits_z);
    let x0 = java_round(min_x * size_x as f64);
    let y0 = java_round(min_y * size_y as f64);
    let z0 = java_round(min_z * size_z as f64);
    let x1 = java_round(max_x * size_x as f64);
    let y1 = java_round(max_y * size_y as f64);
    let z1 = java_round(max_z * size_z as f64);
    let mut full = vec![false; size_x * size_y * size_z];
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                full[(x as usize * size_y + y as usize) * size_z + z as usize] = true;
            }
        }
    }
    Shape::new([cube_parts(size_x), cube_parts(size_y), cube_parts(size_z)], [size_x, size_y, size_z], full, None, false)
}

// The test joinUnoptimized makes before it merges, for an operator whose
// both-full result is false: whether the shapes' boxes meet.
fn bounds_meet(first: &Shape, second: &Shape) -> bool {
    match (first.single, second.single) {
        (Some(a), Some(b)) => boxes_intersect(a, b),
        (Some(a), None) => shape_intersects_box(second, a),
        (None, Some(b)) => shape_intersects_box(first, b),
        (None, None) => boxes_intersect(first.bounds(), second.bounds()),
    }
}

// Shapes.joinUnoptimized (CollisionUtil.joinUnoptimized): the shapes joined
// on the merged coordinates, not simplified. (The shapes are never the same
// object: Java returns one of them for that.)
pub fn join_unoptimized(first: &Shape, second: &Shape, operator: fn(bool, bool) -> bool) -> Shape {
    if operator(false, false) {
        panic!("Ambiguous operator: (false, false) -> true");
    }
    let tt = operator(true, true);
    let ft = operator(false, true);
    let tf = operator(true, false);
    if first.is_empty() {
        return if ft { second.clone() } else { empty() };
    }
    if second.is_empty() {
        return if tf { first.clone() } else { empty() };
    }
    if !tt && !bounds_meet(first, second) {
        if !tf & !ft {
            return empty();
        }
        if !tf | !ft {
            return if tf { first.clone() } else { second.clone() };
        }
    }
    let Some(merged_x) = merge(&first.coords[0], &second.coords[0], ft, tf) else { return empty() };
    let Some(merged_y) = merge(&first.coords[1], &second.coords[1], ft, tf) else { return empty() };
    let Some(merged_z) = merge(&first.coords[2], &second.coords[2], ft, tf) else { return empty() };
    let size = [merged_x.voxels, merged_y.voxels, merged_z.voxels];
    let op = (ft as u32) << 1 | (tf as u32) << 2 | (tt as u32) << 3;
    let mut full = Vec::with_capacity(size[0] * size[1] * size[2]);
    for x in 0..size[0] {
        let (s1x, s2x) = (merged_x.first[x], merged_x.second[x]);
        for y in 0..size[1] {
            let (s1y, s2y) = (merged_y.first[y], merged_y.second[y]);
            for z in 0..size[2] {
                let (s1z, s2z) = (merged_z.first[z], merged_z.second[z]);
                let full1 = (s1x | s1y | s1z) >= 0 && first.voxel(s1x as usize, s1y as usize, s1z as usize);
                let full2 = (s2x | s2y | s2z) >= 0 && second.voxel(s2x as usize, s2y as usize, s2z as usize);
                full.push(op & 1 << (full2 as u32 | (full1 as u32) << 1) != 0);
            }
        }
    }
    if !full.iter().any(|&f| f) {
        return empty();
    }
    let coords = [merged_x.coords[..=size[0]].to_vec(), merged_y.coords[..=size[1]].to_vec(), merged_z.coords[..=size[2]].to_vec()];
    let single = (size == [1, 1, 1]).then(|| [coords[0][0], coords[1][0], coords[2][0], coords[0][1], coords[1][1], coords[2][1]]);
    Shape::new(coords, size, full, single, false)
}

// VoxelShape.optimize: the shape as the box or boxes it is made of, joined
// again. The boxes (toAabbs) stay those of the shape it came from.
pub fn optimize(shape: &Shape) -> Shape {
    if shape.is_empty() {
        return empty();
    }
    if shape.single.is_some() {
        return if shape.is_full_block() { block() } else { shape.clone() };
    }
    let boxes: Vec<[f64; 6]> = shape.boxes().to_vec();
    if boxes.is_empty() {
        return empty();
    }
    let ret = if boxes.len() == 1 {
        let b = boxes[0];
        create(b[0], b[1], b[2], b[3], b[4], b[5])
    } else {
        let mut tmp: Vec<Shape> = boxes.iter().map(|b| create(b[0], b[1], b[2], b[3], b[4], b[5])).collect();
        let mut i = tmp.len();
        while i > 1 {
            let mut new_size = 0;
            let mut at = 0;
            while at < i {
                let next = at + 1;
                if next >= i {
                    tmp[new_size] = std::mem::replace(&mut tmp[at], empty());
                    new_size += 1;
                    break;
                }
                let joined = join_unoptimized(&tmp[at], &tmp[next], or_op);
                tmp[new_size] = joined;
                new_size += 1;
                at += 2;
            }
            i = new_size;
        }
        tmp.swap_remove(0)
    };
    // (ret.cachedToAABBs = this.cachedToAABBs where ret has none.)
    let _ = ret.boxes.set(boxes);
    ret
}

// Shapes.or(first, second).
pub fn or(first: &Shape, second: &Shape) -> Shape {
    optimize(&join_unoptimized(first, second, or_op))
}

// BooleanOp.ONLY_FIRST.
pub fn only_first(first: bool, second: bool) -> bool {
    first && !second
}

// BooleanOp.ONLY_SECOND.
pub fn only_second(first: bool, second: bool) -> bool {
    !first && second
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::Registry;

    // The registry's face sturdiness (Java's state cache) against shape_is_face_sturdy
    // on the dumped collision shape, for every state whose class does not
    // override getBlockSupportShape or a dynamic shape.
    #[test]
    fn sturdiness_against_the_dump() {
        let reg = Registry::parse(include_str!("../minecraft/1.21.10/blocks.txt"));
        let skip = ["MultifaceBlock", "ChorusFlowerBlock", "WallHangingSignBlock", "ShulkerBoxBlock", "CeilingHangingSignBlock", "MudBlock", "SnowLayerBlock", "FenceGateBlock", "SoulSandBlock", "MangroveLeavesBlock", "TintedParticleLeavesBlock", "UntintedParticleLeavesBlock"];
        let (mut compared, mut bad) = (0, std::collections::BTreeMap::new());
        for state in 0..reg.states.len() as State {
            let data = &reg.states[state as usize];
            let class = reg.blocks[data.block as usize].class.as_str();
            if data.flags & (DYNAMIC_SHAPE | OFFSET) != 0 || skip.contains(&class) || ["GlowLichenBlock", "VineBlock", "SculkVeinBlock", "MossyCarpetBlock", "ResinClumpBlock"].contains(&class) {
                continue;
            }
            let shape = &reg.shapes[data.collision_shape as usize];
            for face in crate::dir::ALL {
                for support in [Support::Full, Support::Center, Support::Rigid] {
                    compared += 1;
                    if shape_is_face_sturdy(shape, face, support) != reg.sturdy(state, face, support) {
                        *bad.entry(class.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
        println!("compared {compared}, differing by class: {bad:?}");
        assert!(bad.is_empty(), "{bad:?}");
    }

    // Shapes.mergedFaceOccludes (and moonrise's occludesFullBlock) on shape
    // objects, against the dump's table for every pair of its shapes.
    #[test]
    fn merged_face_occludes_against_the_dump() {
        let reg = Registry::parse(include_str!("../minecraft/1.21.10/blocks.txt"));
        for (at, shape) in reg.shapes.iter().enumerate() {
            assert_eq!(shape.occludes_full(), shape.occludes_full_block, "occludes full block of shape {at}");
        }
        let mut compared = 0;
        for first in 0..reg.shapes.len() {
            for second in 0..reg.shapes.len() {
                for direction in crate::dir::ALL {
                    compared += 1;
                    assert_eq!(
                        merged_face_occludes(&reg.shapes[first], &reg.shapes[second], direction),
                        reg.merged_face_occludes(first as u16, second as u16, direction),
                        "shapes {first} and {second} towards {direction:?}"
                    );
                }
            }
        }
        println!("compared {compared} pairs");
    }
}
