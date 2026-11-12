// Block collisions as Paper's moonrise patches compute them
// (ca.spottedleaf.moonrise.patches.collisions.CollisionUtil, and the level's
// findSupportingBlock): which blocks around a box collide with it, the
// move that is left of a movement against them, and the block that holds
// an entity up. The arithmetic, epsilons and tie-breaks are theirs.
//
// A chunk that is not loaded is read as no collision and is not made, as
// the level's chunk cache answers `getChunk(x, z, FULL, false)`.

use crate::dir::Pos;
use crate::level::{refuse, Level};
use crate::phys::{jmax, jmin, Aabb, Vec3};
use crate::registry::{Shape, State, AIR, DYNAMIC_SHAPE};
use crate::world::{HEIGHT, MIN_Y};

// CollisionUtil.COLLISION_EPSILON-based comparisons use 1.0E-7.
const EPS: f64 = 1.0E-7;

// A shape of the registry moved by a block position.
#[derive(Clone)]
pub struct Moved<'a> {
    pub shape: std::borrow::Cow<'a, Shape>,
    pub off: [f64; 3],
}

// What a box collides with: shapes with more than a box, and boxes.
#[derive(Default)]
pub struct Collected<'a> {
    pub voxels: Vec<Moved<'a>>,
    pub boxes: Vec<Aabb>,
}

// The CollisionContext a shape is asked in: CollisionContext.empty(), or an
// entity's (EntityCollisionContext, which holds the entity's y at the moment
// it is made, as the box test of the shapes that read it needs). Only
// scaffolding and powder snow read it; every other class's collision shape
// is the dumped one (for the empty context).
// (BambooStalkBlock and PointedDripstoneBlock are not among them: their
// collision shape is the dumped one moved by their offset at the position,
// and their isCollisionShapeFullBlock is false, which the dump holds. A
// moving piston's follows its block entity: moving_piston::shape_at. A
// shulker box's follows its block entity's lid, which only a player opening
// it moves: closed, it is the full block the dump has. A liquid block's is
// empty for an entity that cannot stand on fluids, as every entity of this
// port is, and so is the dumped shape.)
#[derive(Clone, Copy)]
pub enum Ctx {
    Empty,
    Entity(EntityCtx),
}

// EntityCollisionContext's fields the shapes read, and the entity's fields
// they read: Entity.getY (entityBottom), fallDistance, whether it is a
// falling block and PowderSnowBlock.canEntityWalkOnPowderSnow. (isDescending
// is Entity.isShiftKeyDown, a shared flag no entity of this level sets, and
// an entity of this port holds nothing a scaffolding block's isHoldingItem
// would see.)
#[derive(Clone, Copy)]
pub struct EntityCtx {
    pub bottom: f64,
    pub fall_distance: f64,
    pub falling_block: bool,
    pub walks_on_powder_snow: bool,
    // Entity.isDescending (Entity.isShiftKeyDown): a sneaking player.
    pub descending: bool,
}

// ScaffoldingBlock.SHAPE_UNSTABLE_BOTTOM: Block.column(16, 0, 2).
static SCAFFOLDING_UNSTABLE_BOTTOM: std::sync::LazyLock<Shape> = std::sync::LazyLock::new(|| crate::shape_ops::block_box(0.0, 0.0, 0.0, 16.0, 2.0, 16.0));
// PowderSnowBlock.FALLING_COLLISION_SHAPE: Shapes.box(0, 0, 0, 1, 0.9f, 1).
static POWDER_SNOW_FALLING: std::sync::LazyLock<Shape> = std::sync::LazyLock::new(|| crate::shape_ops::make_box(0.0, 0.0, 0.0, 1.0, 0.8999999761581421, 1.0));
// Shapes.block().
static FULL_BLOCK: std::sync::LazyLock<Shape> = std::sync::LazyLock::new(crate::shape_ops::block);
static EMPTY: std::sync::LazyLock<Shape> = std::sync::LazyLock::new(crate::shape_ops::empty);

// Whether a shape from shape_of is the one Shapes.block() is (an identity the
// entity inside check relies on): powder snow's, for a falling block or an entity
// that walks on it.
pub fn is_shapes_block(shape: &std::borrow::Cow<'static, Shape>) -> bool {
    std::ptr::eq(&**shape, &*FULL_BLOCK)
}

// EntityCollisionContext.isAbove(shape, pos, defaultValue): the entity's y is
// above the shape's top at the position (the empty context answers the
// default).
fn is_above(ctx: Ctx, top: f64, pos: Pos, default: bool) -> bool {
    match ctx {
        Ctx::Empty => default,
        Ctx::Entity(e) => e.bottom > pos.y() as f64 + top - 9.999999747378752E-6,
    }
}

// ScaffoldingBlock.getCollisionShape. The dumped shape is SHAPE_STABLE (the
// empty context is above everything and not descending).
fn scaffolding_shape(l: &Level, state: State, ctx: Ctx, pos: Pos) -> std::borrow::Cow<'static, Shape> {
    let descending = matches!(ctx, Ctx::Entity(e) if e.descending);
    if is_above(ctx, 1.0, pos, true) && !descending {
        return std::borrow::Cow::Borrowed(&l.reg.shapes[l.reg.states[state as usize].collision_shape as usize]);
    }
    let distance = l.reg.get_int(state, l.reg.property_of(state, "distance"));
    let bottom = l.reg.get_bool(state, l.reg.property_of(state, "bottom"));
    // SHAPE_BELOW_BLOCK is the block moved down one: its top is at 0.
    if distance != 0 && bottom && is_above(ctx, 0.0, pos, true) {
        std::borrow::Cow::Borrowed(&*SCAFFOLDING_UNSTABLE_BOTTOM)
    } else {
        std::borrow::Cow::Borrowed(&*EMPTY)
    }
}

// PowderSnowBlock.getCollisionShape. The dumped shape (the empty context has
// no entity) is empty.
fn powder_snow_shape(ctx: Ctx, pos: Pos) -> std::borrow::Cow<'static, Shape> {
    let Ctx::Entity(e) = ctx else { return std::borrow::Cow::Borrowed(&*EMPTY) };
    if e.fall_distance > 2.5 {
        return std::borrow::Cow::Borrowed(&*POWDER_SNOW_FALLING);
    }
    // (Block.getCollisionShape is BlockState.getShape: Shapes.block().)
    if e.falling_block || (e.walks_on_powder_snow && is_above(ctx, 1.0, pos, false)) {
        std::borrow::Cow::Borrowed(&*FULL_BLOCK)
    } else {
        std::borrow::Cow::Borrowed(&*EMPTY)
    }
}

// CollisionUtil.findFloor.
pub fn find_floor(values: &[f64], offset: f64, value: f64, start: usize, end: usize) -> i32 {
    assert!(end < values.len(), "findFloor range");
    let (mut start, mut end) = (start as i32, end as i32);
    loop {
        let middle = (start + end) >> 1;
        let middle_val = values[middle as usize] + offset;
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

// CollisionUtil.collideX/Y/Z(AABB target, AABB source, move) along `axis`.
fn collide_box(axis: usize, target: &Aabb, source: &Aabb, source_move: f64) -> f64 {
    let (a, b) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    if !(source.min(a) - target.max(a) < -EPS) || !(source.max(a) - target.min(a) > EPS) || !(source.min(b) - target.max(b) < -EPS) || !(source.max(b) - target.min(b) > EPS) {
        return source_move;
    }
    if source_move >= 0.0 {
        let max_move = target.min(axis) - source.max(axis);
        if max_move < -EPS {
            source_move
        } else {
            jmin(max_move, source_move)
        }
    } else {
        let max_move = target.max(axis) - source.min(axis);
        if max_move > EPS {
            source_move
        } else {
            jmax(max_move, source_move)
        }
    }
}

// The range of voxels along `axis` that `source` spans, as the voxel
// collisions compute it: the first, and one past the last. None where it
// spans none.
fn span(target: &Moved, axis: usize, source: &Aabb) -> Option<(usize, usize)> {
    let coords = &target.shape.coords[axis];
    let size = target.shape.size[axis];
    let floor_min = find_floor(coords, target.off[axis], source.min(axis) + EPS, 0, size).max(0);
    if floor_min as usize >= size {
        return None;
    }
    let ceil_max = (size as i32).min(find_floor(coords, target.off[axis], source.max(axis) - EPS, floor_min as usize, size) + 1);
    if floor_min >= ceil_max {
        return None;
    }
    Some((floor_min as usize, ceil_max as usize))
}

// CollisionUtil.collideX/Y/Z(VoxelShape target, AABB source, move).
pub fn collide_voxels(axis: usize, target: &Moved, source: &Aabb, source_move: f64) -> f64 {
    assert!(target.shape.single.is_none(), "a single box among the voxel shapes");
    let (a, b) = match axis {
        0 => (1, 2),
        1 => (0, 2),
        _ => (0, 1),
    };
    let Some((a_min, a_max)) = span(target, a, source) else { return source_move };
    let Some((b_min, b_max)) = span(target, b, source) else { return source_move };
    let shape: &Shape = &target.shape;
    let size = shape.size;
    let coords = &shape.coords[axis];
    let off = target.off[axis];
    // The voxel at `along` on the axis and `p`, `q` on the other two.
    let full = |along: usize, p: usize, q: usize| {
        let mut at = [0usize; 3];
        at[axis] = along;
        at[a] = p;
        at[b] = q;
        shape.is_full(at[0], at[1], at[2])
    };
    let slab_full = |along: usize| (a_min..a_max).any(|p| (b_min..b_max).any(|q| full(along, p, q)));
    let along_size = size[axis];
    if source_move > 0.0 {
        let source_max = source.max(axis);
        let ceil_max = find_floor(coords, off, source_max - EPS, 0, along_size) + 1;
        let mut curr = ceil_max;
        while (curr as usize) < along_size {
            let mut max_dist = coords[curr as usize] + off - source_max;
            if max_dist >= source_move {
                return source_move;
            }
            if max_dist >= -EPS {
                max_dist = jmin(max_dist, source_move);
            }
            if slab_full(curr as usize) {
                return max_dist;
            }
            curr += 1;
        }
        source_move
    } else {
        let source_min = source.min(axis);
        let floor_min = find_floor(coords, off, source_min + EPS, 0, along_size);
        let mut curr = floor_min - 1;
        while curr >= 0 {
            let mut max_dist = coords[curr as usize + 1] + off - source_min;
            if max_dist <= source_move {
                return source_move;
            }
            if max_dist <= EPS {
                max_dist = jmax(max_dist, source_move);
            }
            if slab_full(curr as usize) {
                return max_dist;
            }
            curr -= 1;
        }
        source_move
    }
}

// CollisionUtil.voxelShapeIntersect(AABB box1, AABB box2).
fn intersect(box1: &Aabb, box2: &Aabb) -> bool {
    box1.min_x - box2.max_x < -EPS
        && box1.max_x - box2.min_x > EPS
        && box1.min_y - box2.max_y < -EPS
        && box1.max_y - box2.min_y > EPS
        && box1.min_z - box2.max_z < -EPS
        && box1.max_z - box2.min_z > EPS
}

// CollisionUtil.voxelShapeIntersectNoEmpty(VoxelShape, AABB).
fn intersect_voxels(target: &Moved, aabb: &Aabb) -> bool {
    if target.shape.is_empty() {
        return false;
    }
    let Some((x0, x1)) = span(target, 0, aabb) else { return false };
    let Some((y0, y1)) = span(target, 1, aabb) else { return false };
    let Some((z0, z1)) = span(target, 2, aabb) else { return false };
    for x in x0..x1 {
        for y in y0..y1 {
            for z in z0..z1 {
                if target.shape.is_full(x, y, z) {
                    return true;
                }
            }
        }
    }
    false
}

// Every block position the collision queries look at around a box, with
// their edge count: CollisionUtil.getCollisionsForBlocksOrWorldBorder walks
// the box grown by a block on every side, and takes a block on the edge
// only if its shape is larger than a block (on one edge) or it is a moving
// piston (on two); the corners never.
fn candidates(aabb: &Aabb, mut visit: impl FnMut(Pos, i32) -> bool) {
    let min_x = crate::phys::floor(aabb.min_x - EPS) - 1;
    let max_x = crate::phys::floor(aabb.max_x + EPS) + 1;
    let min_y = ((MIN_Y >> 4 << 4) - 1).max(crate::phys::floor(aabb.min_y - EPS) - 1);
    let max_y = ((MIN_Y + HEIGHT >> 4 << 4) + 16).min(crate::phys::floor(aabb.max_y + EPS) + 1);
    let min_z = crate::phys::floor(aabb.min_z - EPS) - 1;
    let max_z = crate::phys::floor(aabb.max_z + EPS) + 1;
    if min_y > max_y {
        return;
    }
    // The chunks in the order the code walks them (z, then x), each section
    // from the bottom, then y, z, x within it.
    for chunk_z in (min_z >> 4)..=(max_z >> 4) {
        for chunk_x in (min_x >> 4)..=(max_x >> 4) {
            for chunk_y in (min_y >> 4)..=(max_y >> 4) {
                let section = chunk_y - (MIN_Y >> 4);
                if section < 0 || section >= HEIGHT >> 4 {
                    continue;
                }
                for y in (chunk_y << 4).max(min_y)..=((chunk_y << 4) + 15).min(max_y) {
                    for z in (chunk_z << 4).max(min_z)..=((chunk_z << 4) + 15).min(max_z) {
                        for x in (chunk_x << 4).max(min_x)..=((chunk_x << 4) + 15).min(max_x) {
                            let edges = (x == min_x || x == max_x) as i32 + (y == min_y || y == max_y) as i32 + (z == min_z || z == max_z) as i32;
                            if edges != 3 && visit(Pos::new(x, y, z), edges) {
                                return;
                            }
                        }
                    }
                }
            }
        }
    }
}

// Whether the block takes part given its edge count (see `candidates`).
pub fn takes_part(l: &Level, state: State, edges: i32) -> bool {
    let large = l.reg.flags(state) & crate::registry::LARGE_COLLISION != 0;
    let piston = l.reg.blocks[l.block(state) as usize].class == "MovingPistonBlock";
    edges == 0 || ((edges != 1 || large) && (edges != 2 || piston))
}

// The block at a position in a chunk that exists, else none.
fn loaded(l: &Level, pos: Pos) -> Option<State> {
    if pos.y() < MIN_Y || pos.y() >= MIN_Y + HEIGHT || !l.world.has_chunk(pos.x() >> 4, pos.z() >> 4) {
        return None;
    }
    Some(l.world.get(pos))
}

// The state's getCollisionShape(level, pos, context).
pub fn shape_of(l: &Level, state: State, pos: Pos, ctx: Ctx) -> std::borrow::Cow<'static, Shape> {
    if l.reg.flags(state) & DYNAMIC_SHAPE != 0 {
        match l.reg.blocks[l.block(state) as usize].class.as_str() {
            "ScaffoldingBlock" => return scaffolding_shape(l, state, ctx, pos),
            "PowderSnowBlock" => return powder_snow_shape(ctx, pos),
            _ => {}
        }
    }
    // A shape offset by the block's position (bamboo) is the dumped one, which
    // is at the origin, moved.
    crate::shape_ops::collision_shape_at(l, state, pos)
}

// CollisionUtil.getCollisionsForBlocksOrWorldBorder without the border (a
// box nowhere near it) for a box moving or standing in the level: what
// collides, or with `check_only` whether anything does.
pub fn block_collisions<'a>(l: &'a Level, aabb: &Aabb, ctx: Ctx, check_only: bool, into: &mut Collected<'a>) -> bool {
    assert!(
        aabb.min_x > -2.9e7 && aabb.max_x < 2.9e7 && aabb.min_z > -2.9e7 && aabb.max_z < 2.9e7,
        "a box at the world border"
    );
    let mut found = false;
    let reg: &'a crate::registry::Registry = l.reg;
    candidates(aabb, |pos, edges| {
        let Some(state) = loaded(l, pos) else { return false };
        if reg.flags(state) & AIR != 0 || !takes_part(l, state, edges) {
            return false;
        }
        let shape = shape_of(l, state, pos, ctx);
        if let Some(single) = shape.single {
            let moved = Aabb::new(single[0] + pos.x() as f64, single[1] + pos.y() as f64, single[2] + pos.z() as f64, single[3] + pos.x() as f64, single[4] + pos.y() as f64, single[5] + pos.z() as f64);
            if intersect(aabb, &moved) {
                found = true;
                if check_only {
                    return true;
                }
                into.boxes.push(moved);
            }
        } else if !shape.is_empty() {
            // An offset shape of more than a box has the root's coordinates and,
            // as its offset, the block's offset plus the position (VoxelShape.move
            // adds to the offset: root + (offset + position)). A moving piston's
            // shape is its own, offset by the block's position.
            let moved = if reg.flags(state) & crate::registry::OFFSET != 0 && matches!(shape, std::borrow::Cow::Owned(_)) {
                let (root, ox, oz) = crate::shape_ops::offset_parts(l, state, pos);
                Moved { shape: std::borrow::Cow::Owned(root), off: [ox + pos.x() as f64, 0.0 + pos.y() as f64, oz + pos.z() as f64] }
            } else {
                Moved { shape, off: [pos.x() as f64, pos.y() as f64, pos.z() as f64] }
            };
            if intersect_voxels(&moved, aabb) {
                found = true;
                if check_only {
                    return true;
                }
                into.voxels.push(moved);
            }
        }
        false
    });
    found
}

// BlockCollisions(level, context, box, false) as CollisionGetter's
// getBlockCollisions walks it (Paper's level does not replace this iterator):
// the blocks around the box in Cursor3D's order (x fastest, then y, then z),
// a block whose collision shape is Shapes.block() itself tested with
// AABB.intersects, any other shape with Shapes.joinIsNotEmpty; for each
// block that collides, the boxes of its shape moved to it (toAabbs: the
// shape's boxes moved by its offset, moonrise's root plus offset and
// position). A moving piston's shape is its block entity's, which this port
// has not as a shape object, so one among the blocks refuses.
pub fn vanilla_block_boxes(l: &Level, aabb: &Aabb, ctx: Ctx) -> Vec<Vec<[f64; 6]>> {
    assert!(
        aabb.min_x > -2.9e7 && aabb.max_x < 2.9e7 && aabb.min_z > -2.9e7 && aabb.max_z < 2.9e7,
        "a box at the world border"
    );
    fn and(a: bool, b: bool) -> bool {
        a && b
    }
    let (min_x, max_x) = (crate::phys::floor(aabb.min_x - 1.0E-7) - 1, crate::phys::floor(aabb.max_x + 1.0E-7) + 1);
    let (min_y, max_y) = (crate::phys::floor(aabb.min_y - 1.0E-7) - 1, crate::phys::floor(aabb.max_y + 1.0E-7) + 1);
    let (min_z, max_z) = (crate::phys::floor(aabb.min_z - 1.0E-7) - 1, crate::phys::floor(aabb.max_z + 1.0E-7) + 1);
    let entity_shape = crate::shape_ops::create(aabb.min_x, aabb.min_y, aabb.min_z, aabb.max_x, aabb.max_y, aabb.max_z);
    let mut out = Vec::new();
    for z in min_z..=max_z {
        for y in min_y..=max_y {
            for x in min_x..=max_x {
                let edges = (x == min_x || x == max_x) as i32 + (y == min_y || y == max_y) as i32 + (z == min_z || z == max_z) as i32;
                if edges == 3 {
                    continue;
                }
                let pos = Pos::new(x, y, z);
                // getChunkForCollisions: a chunk not loaded is skipped; above
                // and below the world the block is void air.
                let Some(state) = loaded(l, pos) else { continue };
                if l.reg.flags(state) & AIR != 0 || !takes_part(l, state, edges) {
                    continue;
                }
                let class = l.reg.blocks[l.block(state) as usize].class.as_str();
                if class == "MovingPistonBlock" {
                    crate::level::refuse("the collision shape of a moving piston beside an experience orb's free position");
                }
                let shape = shape_of(l, state, pos, ctx);
                let identity = match class {
                    "PowderSnowBlock" => is_shapes_block(&shape),
                    "ScaffoldingBlock" => false,
                    _ => crate::player::is_block_shape(l, state),
                };
                if identity {
                    if aabb.intersects_box(x as f64, y as f64, z as f64, x as f64 + 1.0, y as f64 + 1.0, z as f64 + 1.0) {
                        out.push(vec![[x as f64, y as f64, z as f64, x as f64 + 1.0, y as f64 + 1.0, z as f64 + 1.0]]);
                    }
                    continue;
                }
                if shape.is_empty() {
                    continue;
                }
                // The shape's root and its offset with the position.
                let (root, off) = if l.reg.flags(state) & crate::registry::OFFSET != 0 && matches!(shape, std::borrow::Cow::Owned(_)) {
                    let (root, ox, oz) = crate::shape_ops::offset_parts(l, state, pos);
                    (std::borrow::Cow::Owned(root), [ox + x as f64, 0.0 + y as f64, oz + z as f64])
                } else {
                    (shape, [x as f64, y as f64, z as f64])
                };
                let moved = root.moved(off[0], off[1], off[2]);
                if crate::shape_ops::join_is_not_empty(&moved, &entity_shape, and) {
                    out.push(root.boxes().iter().map(|b| [b[0] + off[0], b[1] + off[1], b[2] + off[2], b[3] + off[0], b[4] + off[1], b[5] + off[2]]).collect());
                }
            }
        }
    }
    out
}

// Level.noCollision(entity, box) for an entity that no hard-colliding entity
// stands in the way of (the level holds only items).
pub fn no_collision(l: &Level, aabb: &Aabb, ctx: Ctx) -> bool {
    !block_collisions(l, aabb, ctx, true, &mut Collected::default())
}

fn perform_boxes(axis: usize, bb: &Aabb, mut value: f64, boxes: &[Aabb]) -> f64 {
    for target in boxes {
        value = collide_box(axis, target, bb, value);
    }
    value
}

fn perform_voxels(axis: usize, bb: &Aabb, mut value: f64, voxels: &[Moved]) -> f64 {
    for target in voxels {
        value = collide_voxels(axis, target, bb, value);
    }
    value
}

fn offset(bb: &Aabb, axis: usize, d: f64) -> Aabb {
    match axis {
        0 => Aabb::new(bb.min_x + d, bb.min_y, bb.min_z, bb.max_x + d, bb.max_y, bb.max_z),
        1 => Aabb::new(bb.min_x, bb.min_y + d, bb.min_z, bb.max_x, bb.max_y + d, bb.max_z),
        _ => Aabb::new(bb.min_x, bb.min_y, bb.min_z + d, bb.max_x, bb.max_y, bb.max_z + d),
    }
}

// CollisionUtil.performCollisions: the movement left of `movement` for a
// box against what it collides with, y first, then the smaller of x and z.
pub fn perform_collisions(movement: Vec3, bb: Aabb, c: &Collected) -> Vec3 {
    let (mut x, mut y, mut z) = (movement.x, movement.y, movement.z);
    let mut bb = bb;
    if y != 0.0 {
        y = perform_boxes(1, &bb, y, &c.boxes);
        y = perform_voxels(1, &bb, y, &c.voxels);
        if y != 0.0 {
            bb = offset(&bb, 1, y);
        }
    }
    let x_smaller = x.abs() < z.abs();
    if x_smaller && z != 0.0 {
        z = perform_boxes(2, &bb, z, &c.boxes);
        z = perform_voxels(2, &bb, z, &c.voxels);
        if z != 0.0 {
            bb = offset(&bb, 2, z);
        }
    }
    if x != 0.0 {
        x = perform_boxes(0, &bb, x, &c.boxes);
        x = perform_voxels(0, &bb, x, &c.voxels);
        if !x_smaller && x != 0.0 {
            bb = offset(&bb, 0, x);
        }
    }
    if !x_smaller && z != 0.0 {
        z = perform_boxes(2, &bb, z, &c.boxes);
        z = perform_voxels(2, &bb, z, &c.voxels);
    }
    Vec3::new(x, y, z)
}

// CollisionUtil.cutDownwards / cutUpwards.
pub fn cut_downwards(b: &Aabb, dy: f64) -> Aabb {
    Aabb::new(b.min_x, b.min_y + dy, b.min_z, b.max_x, b.min_y, b.max_z)
}

pub fn cut_upwards(b: &Aabb, dy: f64) -> Aabb {
    Aabb::new(b.min_x, b.max_y, b.min_z, b.max_x, b.max_y + dy, b.max_z)
}

// Level.findSupportingBlock: the block under an entity at `entity_pos` that
// collides with `aabb`, nearest to the entity's position (the greater
// position among equals).
pub fn find_supporting_block(l: &Level, entity_pos: Vec3, ctx: Ctx, aabb: &Aabb) -> Option<Pos> {
    let mut selected: Option<Pos> = None;
    let mut selected_distance = f64::MAX;
    let reg = l.reg;
    candidates(aabb, |pos, edges| {
        let Some(state) = loaded(l, pos) else { return false };
        let dx = pos.x() as f64 + 0.5 - entity_pos.x;
        let dy = pos.y() as f64 + 0.5 - entity_pos.y;
        let dz = pos.z() as f64 + 0.5 - entity_pos.z;
        let distance = dx * dx + dy * dy + dz * dz;
        // Vec3i.compareTo: y, then z, then x.
        let compare = |a: Pos, b: Pos| {
            if a.y() == b.y() {
                if a.z() == b.z() {
                    a.x().wrapping_sub(b.x())
                } else {
                    a.z().wrapping_sub(b.z())
                }
            } else {
                a.y().wrapping_sub(b.y())
            }
        };
        if !(distance > selected_distance) && (distance != selected_distance || compare(selected.unwrap(), pos) < 0) {
            if reg.flags(state) & AIR != 0 || !takes_part(l, state, edges) {
                return false;
            }
            let shape = shape_of(l, state, pos, ctx);
            let shifted = aabb.mv(-(pos.x() as f64), -(pos.y() as f64), -(pos.z() as f64));
            let hit = if let Some(single) = shape.single {
                intersect(&Aabb::new(single[0], single[1], single[2], single[3], single[4], single[5]), &shifted)
            } else if shape.is_empty() {
                false
            } else {
                intersect_voxels(&Moved { shape: std::borrow::Cow::Borrowed(&*shape), off: [0.0, 0.0, 0.0] }, &shifted)
            };
            if hit {
                selected = Some(pos);
                selected_distance = distance;
            }
        }
        false
    });
    selected
}

// Shapes.joinIsNotEmpty(first, second, AND) for a box and a box.
pub fn boxes_overlap(a: &Aabb, b: &Aabb) -> bool {
    intersect(a, b)
}

// The same for a shape of several boxes and a box.
pub fn voxels_overlap(a: &Moved, b: &Aabb) -> bool {
    intersect_voxels(a, b)
}
