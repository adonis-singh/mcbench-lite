// What the blocks a player is in or stands on do to its movement:
// Entity.applyEffectsFromBlocks and checkInsideBlocks (the blocks the box
// touched on its way), BlockBehaviour.entityInside of cobwebs, powder snow,
// sweet berry bushes, honey and bubble columns, and SlimeBlock.stepOn. What
// the other blocks do (a pressure plate's press, a cactus's prick, a portal)
// the server does, or hurts a player who cannot be hurt.

use super::{read, Movement, Player};
use crate::dir::Pos;
use crate::level::{refuse, Level};
use crate::phys::{self, Aabb, Vec3};
use crate::registry::{State, AIR};
use std::collections::HashSet;

// Block classes whose entityInside does nothing to a creative player on the
// client: arrows, ravagers, bees, boats and falling blocks are what they act
// on, or the server does the work (pressure plates, tripwires, hoppers,
// detector rails, tilting leaves, portals), or the player cannot be hurt
// (fire, cacti, campfires, lava, wither roses) or its fire is the server's.
const NO_MOVEMENT_EFFECT: [&str; 25] = [
    "ButtonBlock",
    "CropBlock",
    "CarrotBlock",
    "PotatoBlock",
    "BeetrootBlock",
    "TorchflowerCropBlock",
    "PitcherCropBlock",
    "CampfireBlock",
    "EyeblossomBlock",
    "FrogspawnBlock",
    "WitherRoseBlock",
    "WaterlilyBlock",
    "CactusBlock",
    "HopperBlock",
    "NetherPortalBlock",
    "EndPortalBlock",
    "EndGatewayBlock",
    "PressurePlateBlock",
    "WeightedPressurePlateBlock",
    "TripWireBlock",
    "DetectorRailBlock",
    "BigDripleafBlock",
    "FireBlock",
    "SoulFireBlock",
    "LavaCauldronBlock",
];

impl Player {
    // Entity.applyEffectsFromBlocks().
    pub(super) fn apply_effects_from_blocks(&mut self, l: &Level) {
        let mut movements: Vec<Movement> = self.movements_this_tick.drain(..).collect();
        let old_position = Vec3::new(self.xo, self.yo, self.zo);
        if movements.is_empty() {
            movements.push(Movement { from: old_position, to: self.pos, axis_dependent_original: None });
        } else if movements.last().unwrap().to.distance_to_sqr(self.pos) > 9.9999994E-11f32 as f64 {
            let last = movements.last().unwrap().to;
            movements.push(Movement { from: last, to: self.pos, axis_dependent_original: None });
        }
        self.apply_effects_for(l, &movements);
    }

    // Entity.applyEffectsFromBlocks(Vec3, Vec3).
    pub(crate) fn apply_effects_between(&mut self, l: &Level, from: Vec3, to: Vec3) {
        self.apply_effects_for(l, &[Movement { from, to, axis_dependent_original: None }]);
    }

    // Entity.applyEffectsFromBlocks(List<Movement>).
    fn apply_effects_for(&mut self, l: &Level, movements: &[Movement]) {
        // isAffectedByBlocks.
        if self.on_ground {
            let on_pos = self.on_pos_legacy(l);
            let state = read::state(l, on_pos);
            self.step_on(l, state);
        }
        self.check_inside_blocks(l, movements);
    }

    // Entity.removeLatestMovementRecording.
    pub(crate) fn remove_latest_movement_recording(&mut self) {
        self.movements_this_tick.pop_back();
    }

    // SlimeBlock.stepOn (the other blocks' stepOn reach the server only).
    fn step_on(&mut self, l: &Level, state: State) {
        let class = l.reg.blocks[l.block(state) as usize].class.as_str();
        if class == "SlimeBlock" {
            let d = self.delta.y.abs();
            if d < 0.1 && !self.is_shift_key_down() {
                let f = 0.4 + d * 0.2;
                self.delta = self.delta.multiply(f, 1.0, f);
            }
        } else if crate::entity::motion::overrides(l, state, "stepOn") && !matches!(class, "MagmaBlock" | "SculkSensorBlock" | "CalibratedSculkSensorBlock" | "SculkShriekerBlock" | "RedStoneOreBlock" | "TurtleEggBlock") {
            refuse(format!("{class}.stepOn ({})", l.reg.describe(state)));
        }
    }

    // Entity.checkInsideBlocks(List<Movement>, collector).
    fn check_inside_blocks(&mut self, l: &Level, movements: &[Movement]) {
        let mut visited: HashSet<i64> = HashSet::new();
        for movement in movements {
            let mut from = movement.from;
            let travel = movement.to.subtract(movement.from);
            let mut left = 16;
            match movement.axis_dependent_original {
                Some(original) if travel.length_sqr() > 0.0 => {
                    for axis in axis_step_order(original) {
                        let d = travel.get(axis);
                        if d != 0.0 {
                            let next = from.relative(axis, true, d);
                            left -= self.check_inside_blocks_between(l, &mut visited, from, next, left);
                            from = next;
                        }
                    }
                }
                _ => left -= self.check_inside_blocks_between(l, &mut visited, movement.from, movement.to, 16),
            }
            if left <= 0 {
                self.check_inside_blocks_between(l, &mut visited, movement.to, movement.to, 1);
            }
        }
    }

    // Entity.checkInsideBlocks(from, to, collector, visited, maxSteps).
    fn check_inside_blocks_between(&mut self, l: &Level, visited: &mut HashSet<i64>, from: Vec3, to: Vec3, max_steps: i32) -> i32 {
        let aabb = self.make_bounding_box(to, self.width, self.height).deflate(1.0E-5f32 as f64);
        let far = from.distance_to_sqr(to) > 0.9999900000002526f64 * 0.9999900000002526f64;
        let mut last = 0;
        for_each_block_intersected_between(from, to, &aabb, &mut |pos, index| {
            if index >= max_steps {
                return false;
            }
            last = index;
            let state = read::state(l, pos);
            if l.reg.flags(state) & AIR != 0 {
                return true;
            }
            // BlockBehaviour.getEntityInsideCollisionShape: the block, but for
            // powder snow's (the shape it collides with) and the shapes of
            // blocks that act on no movement.
            let in_block = self.inside_shape_hit(l, state, pos, from, to);
            if in_block && visited.insert(pos.as_long()) {
                let past_edges = far || aabb.intersects_box(pos.x() as f64, pos.y() as f64, pos.z() as f64, pos.x() as f64 + 1.0, pos.y() as f64 + 1.0, pos.z() as f64 + 1.0);
                self.entity_inside(l, state, pos, past_edges);
            }
            true
        });
        last + 1
    }

    // `getEntityInsideCollisionShape == Shapes.block() || collidedWithShapeMovingFrom(...)`.
    fn inside_shape_hit(&self, l: &Level, state: State, pos: Pos, from: Vec3, to: Vec3) -> bool {
        let class = l.reg.blocks[l.block(state) as usize].class.as_str();
        if class != "PowderSnowBlock" {
            return true;
        }
        // PowderSnowBlock: the collision shape for this player, the block if it is empty.
        let shape = crate::collision::shape_of(l, state, pos, self.context());
        if shape.is_empty() {
            return true;
        }
        let boxes: Vec<Aabb> = shape
            .boxes()
            .iter()
            .map(|b| Aabb::new(b[0] + pos.x() as f64, b[1] + pos.y() as f64, b[2] + pos.z() as f64, b[3] + pos.x() as f64, b[4] + pos.y() as f64, b[5] + pos.z() as f64))
            .collect();
        self.make_bounding_box(from, self.width, self.height).collided_along_vector(to.subtract(from), &boxes)
    }

    // BlockState.entityInside for the player.
    fn entity_inside(&mut self, l: &Level, state: State, pos: Pos, past_edges: bool) {
        if !crate::entity::motion::overrides(l, state, "entityInside") {
            return;
        }
        let reg = l.reg;
        let class = reg.blocks[l.block(state) as usize].class.as_str();
        match class {
            "WebBlock" => self.make_stuck_in_block(Vec3::new(0.25, 0.05f32 as f64, 0.25)),
            "SweetBerryBushBlock" => self.make_stuck_in_block(Vec3::new(0.8f32 as f64, 0.75, 0.8f32 as f64)),
            "PowderSnowBlock" => {
                // (A living entity is stuck only in the block it is in.)
                let in_block = self.in_block_state(l);
                if reg.blocks[l.block(in_block) as usize].key == "minecraft:powder_snow" {
                    self.make_stuck_in_block(Vec3::new(0.9f32 as f64, 1.5, 0.9f32 as f64));
                }
            }
            "HoneyBlock" => {
                if self.is_sliding_down(pos) {
                    self.do_slide_movement();
                }
            }
            "BubbleColumnBlock" => {
                if past_edges {
                    let above = read::state(l, pos.above());
                    // above.getCollisionShape(level, pos) with no entity.
                    let empty = crate::collision::shape_of(l, above, pos, crate::collision::Ctx::Empty).is_empty() && l.fluid_of(above) == crate::registry::EMPTY_FLUID;
                    let drag_down = reg.get_bool(state, reg.property_of(state, "drag"));
                    if empty {
                        self.on_above_bubble_column(drag_down);
                    } else {
                        self.on_inside_bubble_column(drag_down);
                    }
                }
            }
            other => {
                if !NO_MOVEMENT_EFFECT.contains(&other) && !reg.blocks[l.block(state) as usize].supers.iter().any(|s| NO_MOVEMENT_EFFECT.contains(&s.as_str())) && other != "LayeredCauldronBlock" {
                    refuse(format!("{other}.entityInside ({})", reg.describe(state)));
                }
            }
        }
    }

    // Entity.makeStuckInBlock.
    fn make_stuck_in_block(&mut self, multiplier: Vec3) {
        // Player.makeStuckInBlock: a flying player is not stuck.
        if self.flying {
            return;
        }
        self.fall_distance = 0.0;
        self.stuck_speed_multiplier = multiplier;
    }

    // Player.onAboveBubbleColumn: a flying player is not pulled.
    fn on_above_bubble_column(&mut self, drag_down: bool) {
        if self.flying {
            return;
        }
        let d = self.delta;
        let y = if drag_down { phys::jmax(-0.9, d.y - 0.03) } else { phys::jmin(1.8, d.y + 0.1) };
        self.delta = Vec3::new(d.x, y, d.z);
    }

    // Player.onInsideBubbleColumn.
    fn on_inside_bubble_column(&mut self, drag_down: bool) {
        if self.flying {
            return;
        }
        let d = self.delta;
        let y = if drag_down { phys::jmax(-0.3, d.y - 0.03) } else { phys::jmin(0.7, d.y + 0.06) };
        self.delta = Vec3::new(d.x, y, d.z);
        self.fall_distance = 0.0;
    }

    // HoneyBlock.isSlidingDown.
    fn is_sliding_down(&self, pos: Pos) -> bool {
        if self.on_ground {
            false
        } else if self.pos.y > pos.y() as f64 + 0.9375 - 1.0E-7 {
            false
        } else if old_delta_y(self.delta.y) >= -0.08 {
            false
        } else {
            let d = (pos.x() as f64 + 0.5 - self.pos.x).abs();
            let d1 = (pos.z() as f64 + 0.5 - self.pos.z).abs();
            let d2 = 0.4375 + (self.width / 2.0f32) as f64;
            d + 1.0E-7 > d2 || d1 + 1.0E-7 > d2
        }
    }

    // HoneyBlock.doSlideMovement.
    fn do_slide_movement(&mut self) {
        let d = self.delta;
        if old_delta_y(d.y) < -0.13 {
            let f = -0.05 / old_delta_y(d.y);
            self.delta = Vec3::new(d.x * f, new_delta_y(-0.05), d.z * f);
        } else {
            self.delta = Vec3::new(d.x, new_delta_y(-0.05), d.z);
        }
        self.fall_distance = 0.0;
    }
}

fn old_delta_y(d: f64) -> f64 {
    d / 0.98f32 as f64 + 0.08
}

fn new_delta_y(d: f64) -> f64 {
    (d - 0.08) * 0.98f32 as f64
}

// Direction.axisStepOrder(vector).
fn axis_step_order(v: Vec3) -> [usize; 3] {
    if v.x.abs() < v.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    }
}

// BlockPos.betweenCornersInDirection(x1, y1, z1, x2, y2, z2, direction).
fn between_corners_in_direction(c: [i32; 6], direction: Vec3) -> Vec<Pos> {
    let min = [c[0].min(c[3]), c[1].min(c[4]), c[2].min(c[5])];
    let max = [c[0].max(c[3]), c[1].max(c[4]), c[2].max(c[5])];
    let span = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
    let start = [if direction.x >= 0.0 { min[0] } else { max[0] }, if direction.y >= 0.0 { min[1] } else { max[1] }, if direction.z >= 0.0 { min[2] } else { max[2] }];
    let order = axis_step_order(direction);
    let step = |axis: usize| if direction.get(axis) >= 0.0 { 1 } else { -1 };
    let mut out = Vec::new();
    for first in 0..=span[order[0]] {
        for second in 0..=span[order[1]] {
            for third in 0..=span[order[2]] {
                let mut at = start;
                at[order[0]] += step(order[0]) * first;
                at[order[1]] += step(order[1]) * second;
                at[order[2]] += step(order[2]) * third;
                out.push(Pos::new(at[0], at[1], at[2]));
            }
        }
    }
    out
}

// BlockGetter.forEachBlockIntersectedBetween.
fn for_each_block_intersected_between(from: Vec3, to: Vec3, bb: &Aabb, visit: &mut dyn FnMut(Pos, i32) -> bool) -> bool {
    let travel = to.subtract(from);
    if travel.length_sqr() < (1.0E-5f32 as f64) * (1.0E-5f32 as f64) {
        // BlockPos.betweenClosed(box): x fastest, then y, then z.
        let (min_x, min_y, min_z) = (phys::floor(bb.min_x), phys::floor(bb.min_y), phys::floor(bb.min_z));
        let (max_x, max_y, max_z) = (phys::floor(bb.max_x), phys::floor(bb.max_y), phys::floor(bb.max_z));
        for z in min_z.min(max_z)..=min_z.max(max_z) {
            for y in min_y.min(max_y)..=min_y.max(max_y) {
                for x in min_x.min(max_x)..=min_x.max(max_x) {
                    if !visit(Pos::new(x, y, z), 0) {
                        return false;
                    }
                }
            }
        }
        return true;
    }
    let mut set = HashSet::new();
    let moved_back = bb.mv(-travel.x, -travel.y, -travel.z);
    for pos in between_corners_in_direction(corners(&moved_back), travel) {
        if !visit(pos, 0) {
            return false;
        }
        set.insert(pos);
    }
    let i = add_collisions_along_travel(&mut set, travel, bb, visit);
    if i < 0 {
        return false;
    }
    for pos in between_corners_in_direction(corners(bb), travel) {
        if set.insert(pos) && !visit(pos, i + 1) {
            return false;
        }
    }
    true
}

fn corners(bb: &Aabb) -> [i32; 6] {
    [phys::floor(bb.min_x), phys::floor(bb.min_y), phys::floor(bb.min_z), phys::floor(bb.max_x), phys::floor(bb.max_y), phys::floor(bb.max_z)]
}

// BlockGetter.getFurthestCorner.
fn furthest_corner(v: Vec3) -> [i32; 3] {
    let abs = (1.0 * v.x + 0.0 * v.y + 0.0 * v.z).abs();
    let abs1 = (0.0 * v.x + 1.0 * v.y + 0.0 * v.z).abs();
    let abs2 = (0.0 * v.x + 0.0 * v.y + 1.0 * v.z).abs();
    let i = if v.x >= 0.0 { 1 } else { -1 };
    let i1 = if v.y >= 0.0 { 1 } else { -1 };
    let i2 = if v.z >= 0.0 { 1 } else { -1 };
    if abs <= abs1 && abs <= abs2 {
        [-i, -i2, i1]
    } else if abs1 <= abs2 {
        [i2, -i1, -i]
    } else {
        [-i1, i, -i2]
    }
}

// BlockGetter.addCollisionsAlongTravel: the number of steps, or -1 if the
// visitor stopped.
fn add_collisions_along_travel(output: &mut HashSet<Pos>, travel: Vec3, bb: &Aabb, visit: &mut dyn FnMut(Pos, i32) -> bool) -> i32 {
    let (xsize, ysize, zsize) = (bb.xsize(), bb.ysize(), bb.zsize());
    let corner = furthest_corner(travel);
    let center = bb.center();
    let vec3 = Vec3::new(center.x + xsize * 0.5 * corner[0] as f64, center.y + ysize * 0.5 * corner[1] as f64, center.z + zsize * 0.5 * corner[2] as f64);
    let vec31 = vec3.subtract(travel);
    let (mut fx, mut fy, mut fz) = (phys::floor(vec31.x), phys::floor(vec31.y), phys::floor(vec31.z));
    let (i, i1, i2) = (phys::sign(travel.x), phys::sign(travel.y), phys::sign(travel.z));
    let d = if i == 0 { f64::MAX } else { i as f64 / travel.x };
    let d1 = if i1 == 0 { f64::MAX } else { i1 as f64 / travel.y };
    let d2 = if i2 == 0 { f64::MAX } else { i2 as f64 / travel.z };
    let mut d3 = d * if i > 0 { 1.0 - phys::frac(vec31.x) } else { phys::frac(vec31.x) };
    let mut d4 = d1 * if i1 > 0 { 1.0 - phys::frac(vec31.y) } else { phys::frac(vec31.y) };
    let mut d5 = d2 * if i2 > 0 { 1.0 - phys::frac(vec31.z) } else { phys::frac(vec31.z) };
    let mut i3 = 0;
    while d3 <= 1.0 || d4 <= 1.0 || d5 <= 1.0 {
        if d3 < d4 {
            if d3 < d5 {
                fx += i;
                d3 += d;
            } else {
                fz += i2;
                d5 += d2;
            }
        } else if d4 < d5 {
            fy += i1;
            d4 += d1;
        } else {
            fz += i2;
            d5 += d2;
        }
        let hit = phys::clip_box([fx as f64, fy as f64, fz as f64], [(fx + 1) as f64, (fy + 1) as f64, (fz + 1) as f64], vec31, vec3);
        if let Some(hit) = hit {
            i3 += 1;
            let d6 = phys::clamp(hit.x, (fx as f32 + 1.0E-5f32) as f64, fx as f64 + 1.0 - 1.0E-5f32 as f64);
            let d7 = phys::clamp(hit.y, (fy as f32 + 1.0E-5f32) as f64, fy as f64 + 1.0 - 1.0E-5f32 as f64);
            let d8 = phys::clamp(hit.z, (fz as f32 + 1.0E-5f32) as f64, fz as f64 + 1.0 - 1.0E-5f32 as f64);
            let f3 = phys::floor(d6 - xsize * corner[0] as f64);
            let f4 = phys::floor(d7 - ysize * corner[1] as f64);
            let f5 = phys::floor(d8 - zsize * corner[2] as f64);
            for pos in between_corners_in_direction([fx, fy, fz, f3, f4, f5], travel) {
                if output.insert(pos) && !visit(pos, i3) {
                    return -1;
                }
            }
        }
    }
    i3
}
