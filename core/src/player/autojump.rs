// LocalPlayer.updateAutoJump: a player walking into a block a jump would
// clear jumps by itself (the option, off by default).

use super::{read, Player};
use crate::collision;
use crate::dir::Pos;
use crate::level::Level;
use crate::mth;
use crate::phys::{self, Aabb, Vec3};

// Level.getCollisions(player, box) as boxes: the shapes in the order
// BlockCollisions walks them, each shape's boxes in VoxelShape.toAabbs order.
fn collision_boxes(p: &Player, l: &Level, aabb: &Aabb) -> Vec<Aabb> {
    let mut out = Vec::new();
    for shape in p.entity_shapes(l, aabb) {
        for b in shape.boxes() {
            out.push(Aabb::new(b[0], b[1], b[2], b[3], b[4], b[5]));
        }
    }
    for (_, shape) in p.block_shapes(l, aabb) {
        for b in shape.boxes() {
            out.push(Aabb::new(b[0], b[1], b[2], b[3], b[4], b[5]));
        }
    }
    out
}

// Vec3.cross.
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x)
}

pub fn update(p: &mut Player, l: &Level, dx: f32, dz: f32) {
    let pos = p.pos;
    let target = pos.add(dx as f64, 0.0, dz as f64);
    let mut movement = Vec3::new(dx as f64, 0.0, dz as f64);
    let speed = p.speed();
    let mut length_sqr = movement.length_sqr() as f32;
    if length_sqr <= 0.001f32 {
        let (mx, my) = p.move_vector;
        let x = speed * mx;
        let y = speed * my;
        let sin = mth::sin(p.y_rot * (std::f64::consts::PI / 180.0) as f32);
        let cos = mth::cos(p.y_rot * (std::f64::consts::PI / 180.0) as f32);
        movement = Vec3::new((x * cos - y * sin) as f64, movement.y, (y * cos + x * sin) as f64);
        length_sqr = movement.length_sqr() as f32;
        if length_sqr <= 0.001f32 {
            return;
        }
    }
    // Mth.invSqrt(float): joml's 1.0F / (float)Math.sqrt(r).
    let inv = 1.0f32 / (length_sqr as f64).sqrt() as f32;
    let direction = movement.scale(inv as f64);
    // Entity.getForward: directionFromRotation(xRot, yRot).
    let forward = {
        let f2 = mth::cos(-p.y_rot * (std::f64::consts::PI / 180.0) as f32 - std::f32::consts::PI);
        let f3 = mth::sin(-p.y_rot * (std::f64::consts::PI / 180.0) as f32 - std::f32::consts::PI);
        let f4 = -mth::cos(-p.x_rot * (std::f64::consts::PI / 180.0) as f32);
        let f5 = mth::sin(-p.x_rot * (std::f64::consts::PI / 180.0) as f32);
        Vec3::new((f3 * f4) as f64, f5 as f64, (f2 * f4) as f64)
    };
    let dot = (forward.x * direction.x + forward.z * direction.z) as f32;
    if dot < -0.15f32 {
        return;
    }
    let context = p.context();
    let empty = |pos: Pos| collision::shape_of(l, read::state(l, pos), pos, context).is_empty();
    let mut at = Pos::new(phys::floor(p.pos.x), phys::floor(p.bb.max_y), phys::floor(p.pos.z));
    if !empty(at) {
        return;
    }
    at = at.above();
    if !empty(at) {
        return;
    }
    let max_jump = 1.2f32;
    let reach = jmaxf(speed * 7.0f32, 1.0f32 / inv);
    let mut far = target.add_vec(direction.scale(reach as f64));
    let width = p.width;
    let height = p.height;
    let area = Aabb::new(pos.x, pos.y, pos.z, far.x, far.y + height as f64, far.z).inflate(width as f64, 0.0, width as f64);
    let from = pos.add(0.0, 0.51f32 as f64, 0.0);
    far = far.add(0.0, 0.51f32 as f64, 0.0);
    let side = cross(direction, Vec3::new(0.0, 1.0, 0.0)).scale((width * 0.5f32) as f64);
    let from_left = from.subtract(side);
    let far_left = far.subtract(side);
    let from_right = from.add_vec(side);
    let far_right = far.add_vec(side);
    // Float.MIN_VALUE.
    let sentinel = f32::from_bits(1);
    let mut top = sentinel;
    for b in collision_boxes(p, l, &area) {
        if intersects_segment(&b, from_left, far_left) || intersects_segment(&b, from_right, far_right) {
            top = b.max_y as f32;
            let center = b.center();
            let center_block = Pos::new(phys::floor(center.x), phys::floor(center.y), phys::floor(center.z));
            let mut i = 1;
            while (i as f32) < max_jump {
                let above = Pos::new(center_block.x(), center_block.y() + i, center_block.z());
                let shape = collision::shape_of(l, read::state(l, above), above, context);
                if !shape.is_empty() {
                    top = shape_max_y(&shape) as f32 + above.y() as f32;
                    if top as f64 - p.pos.y > max_jump as f64 {
                        return;
                    }
                }
                if i > 1 {
                    at = at.above();
                    if !empty(at) {
                        return;
                    }
                }
                i += 1;
            }
            break;
        }
    }
    if top != sentinel {
        let rise = (top as f64 - p.pos.y) as f32;
        if !(rise <= 0.5f32) && !(rise > max_jump) {
            p.auto_jump_time = 1;
        }
    }
}

// AABB.intersects(Vec3, Vec3).
fn intersects_segment(b: &Aabb, v1: Vec3, v2: Vec3) -> bool {
    b.intersects_box(phys::jmin(v1.x, v2.x), phys::jmin(v1.y, v2.y), phys::jmin(v1.z, v2.z), phys::jmax(v1.x, v2.x), phys::jmax(v1.y, v2.y), phys::jmax(v1.z, v2.z))
}

// VoxelShape.max(Axis.Y).
fn shape_max_y(shape: &crate::registry::Shape) -> f64 {
    if let Some(single) = shape.single {
        return single[4];
    }
    let size = shape.size[1];
    let mut best = f64::NEG_INFINITY;
    for y in (0..size).rev() {
        let any = (0..shape.size[0]).any(|x| (0..shape.size[2]).any(|z| shape.is_full(x, y, z)));
        if any {
            best = shape.coords[1][y + 1];
            break;
        }
    }
    best
}

// Math.max(float, float).
fn jmaxf(a: f32, b: f32) -> f32 {
    if a != a {
        return a;
    }
    if a == 0.0 && b == 0.0 && a.is_sign_negative() {
        return b;
    }
    if a >= b {
        a
    } else {
        b
    }
}
