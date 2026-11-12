// Entity.move for the player and what it calls: the collision with the
// blocks (collision.rs), step-up, sneaking at an edge (Player.maybeBackOffFromEdge),
// the supporting block, fall distance, the block factors and the auto-jump
// (LocalPlayer.move and updateAutoJump).

use super::{read, signum, vanilla, Movement, Player};
use crate::clip;
use crate::collision::{Ctx, EntityCtx};
use crate::dir::Pos;
use crate::level::Level;
use crate::mth;
use crate::phys::{self, Aabb, Vec3};
use crate::registry::{Shape, State};

impl Player {
    // The CollisionContext.of(player) the shapes read.
    pub(super) fn context(&self) -> Ctx {
        Ctx::Entity(EntityCtx { bottom: self.pos.y, fall_distance: self.fall_distance, falling_block: false, walks_on_powder_snow: false, descending: self.is_shift_key_down() })
    }

    // Level.noCollision(player, box): no block collides with the box.
    pub(super) fn no_collision(&self, l: &Level, aabb: &Aabb) -> bool {
        self.block_shapes(l, aabb).is_empty() && self.entity_shapes(l, aabb).is_empty()
    }

    // CollisionGetter.collidesWithSuffocatingBlock.
    pub(super) fn collides_with_suffocating_block(&self, l: &Level, aabb: &Aabb) -> bool {
        !vanilla::block_shapes(l, aabb, self.context(), true).is_empty()
    }

    // Entity.getOnPos(offset).
    pub(super) fn get_on_pos(&self, l: &Level, y_offset: f32) -> Pos {
        if let Some(supporting) = self.main_supporting_block {
            if !(y_offset > 1.0E-5f32) {
                return supporting;
            }
            let state = read::state(l, supporting);
            let fence = (!(y_offset as f64 <= 0.5) || !l.reg.in_tag(state, "minecraft:fences")) && !l.reg.in_tag(state, "minecraft:walls");
            let gate = l.reg.blocks[l.block(state) as usize].class == "FenceGateBlock";
            return if fence && !gate { Pos::new(supporting.x(), phys::floor(self.pos.y - y_offset as f64), supporting.z()) } else { supporting };
        }
        Pos::new(phys::floor(self.pos.x), phys::floor(self.pos.y - y_offset as f64), phys::floor(self.pos.z))
    }

    pub(super) fn on_pos_legacy(&self, l: &Level) -> Pos {
        self.get_on_pos(l, 0.2f32)
    }

    pub(super) fn block_pos_below_that_affects_my_movement(&self, l: &Level) -> Pos {
        self.get_on_pos(l, 0.500001f32)
    }

    // Entity.getBlockJumpFactor.
    pub(super) fn block_jump_factor(&self, l: &Level) -> f32 {
        let here = l.reg.blocks[l.block(read::state(l, self.block_pos)) as usize].jump_factor;
        let below = l.reg.blocks[l.block(read::state(l, self.block_pos_below_that_affects_my_movement(l))) as usize].jump_factor;
        if here as f64 == 1.0 {
            below
        } else {
            here
        }
    }

    // Entity.getBlockSpeedFactor with Player's (1.0 while flying) and
    // LivingEntity's (the movement efficiency attribute, 0).
    fn block_speed_factor(&self, l: &Level) -> f32 {
        if self.flying {
            return 1.0f32;
        }
        let state = read::state(l, self.block_pos);
        let block = &l.reg.blocks[l.block(state) as usize];
        let speed = block.speed_factor;
        let factor = if block.key != "minecraft:water" && block.key != "minecraft:bubble_column" {
            if speed as f64 == 1.0 {
                let below = read::state(l, self.block_pos_below_that_affects_my_movement(l));
                l.reg.blocks[l.block(below) as usize].speed_factor
            } else {
                speed
            }
        } else {
            speed
        };
        // Mth.lerp(movement efficiency (0), factor, 1.0F).
        factor + 0.0f32 * (1.0f32 - factor)
    }

    // Level.findSupportingBlock: the block under the player whose shape meets
    // `aabb`, nearest to the player's position (the greater position among equals).
    fn find_supporting_block(&self, l: &Level, aabb: &Aabb) -> Option<Pos> {
        let mut selected: Option<Pos> = None;
        let mut selected_distance = f64::MAX;
        for (pos, _) in self.block_shapes(l, aabb) {
            let dx = pos.x() as f64 + 0.5 - self.pos.x;
            let dy = pos.y() as f64 + 0.5 - self.pos.y;
            let dz = pos.z() as f64 + 0.5 - self.pos.z;
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
            if distance < selected_distance || distance == selected_distance && (selected.is_none() || compare(selected.unwrap(), pos) < 0) {
                selected = Some(pos);
                selected_distance = distance;
            }
        }
        selected
    }

    // Entity.checkSupportingBlock.
    fn check_supporting_block(&mut self, l: &Level, on_ground: bool, movement: Option<Vec3>) {
        if on_ground {
            let bb = self.bb;
            let aabb = Aabb::new(bb.min_x, bb.min_y - 1.0E-6, bb.min_z, bb.max_x, bb.min_y, bb.max_z);
            let mut found = self.find_supporting_block(l, &aabb);
            if found.is_some() || self.on_ground_no_blocks {
                self.main_supporting_block = found;
            } else if let Some(m) = movement {
                found = self.find_supporting_block(l, &aabb.mv(-m.x, 0.0, -m.z));
                self.main_supporting_block = found;
            }
            self.on_ground_no_blocks = found.is_none();
        } else {
            self.on_ground_no_blocks = false;
            self.main_supporting_block = None;
        }
    }

    // Entity.setOnGroundWithMovement.
    fn set_on_ground_with_movement(&mut self, l: &Level, on_ground: bool, horizontal_collision: bool, movement: Vec3) {
        self.on_ground = on_ground;
        self.horizontal_collision = horizontal_collision;
        self.check_supporting_block(l, on_ground, Some(movement));
    }

    // Entity.addMovementThisTick.
    fn add_movement_this_tick(&mut self, movement: Movement) {
        if self.movements_this_tick.len() >= 100 {
            let first = self.movements_this_tick.pop_front().unwrap();
            let second = self.movements_this_tick.pop_front().unwrap();
            self.movements_this_tick.push_front(Movement { from: first.from, to: second.to, axis_dependent_original: None });
        }
        self.movements_this_tick.push_back(movement);
    }

    // LocalPlayer.move: Entity.move, then the auto-jump from how far it went.
    pub(super) fn move_entity(&mut self, l: &Level, movement: Vec3) {
        let (old_x, old_z) = (self.pos.x, self.pos.z);
        self.entity_move(l, movement, false);
        let dx = (self.pos.x - old_x) as f32;
        let dz = (self.pos.z - old_z) as f32;
        self.update_auto_jump(l, dx, dz);
    }

    // LocalPlayer.move(MoverType.PISTON, movement): a moving piston carries the player.
    pub(crate) fn move_by_piston(&mut self, l: &Level, movement: Vec3) {
        let (old_x, old_z) = (self.pos.x, self.pos.z);
        self.entity_move(l, movement, true);
        let dx = (self.pos.x - old_x) as f32;
        let dz = (self.pos.z - old_z) as f32;
        self.update_auto_jump(l, dx, dz);
    }

    // Entity.limitPistonMovement.
    fn limit_piston_movement(&mut self, l: &Level, pos: Vec3) -> Vec3 {
        if pos.length_sqr() <= 1.0E-7 {
            return pos;
        }
        if l.game_time != self.piston_deltas_game_time {
            self.piston_deltas = [0.0; 3];
            self.piston_deltas_game_time = l.game_time;
        }
        let limit = 1.0E-5f32 as f64;
        // applyPistonMovementRestriction: the total along the axis in this game time stays within 0.51 either way.
        let mut restrict = |axis: usize, distance: f64| {
            let d = phys::clamp(distance + self.piston_deltas[axis], -0.51, 0.51);
            let distance = d - self.piston_deltas[axis];
            self.piston_deltas[axis] = d;
            distance
        };
        if pos.x != 0.0 {
            let d = restrict(0, pos.x);
            if d.abs() <= limit { Vec3::ZERO } else { Vec3::new(d, 0.0, 0.0) }
        } else if pos.y != 0.0 {
            let d = restrict(1, pos.y);
            if d.abs() <= limit { Vec3::ZERO } else { Vec3::new(0.0, d, 0.0) }
        } else if pos.z != 0.0 {
            let d = restrict(2, pos.z);
            if d.abs() <= limit { Vec3::ZERO } else { Vec3::new(0.0, 0.0, d) }
        } else {
            Vec3::ZERO
        }
    }

    // Entity.move(type, movement): MoverType.SELF, or PISTON.
    fn entity_move(&mut self, l: &Level, movement: Vec3, piston: bool) {
        let mut movement = movement;
        if piston {
            movement = self.limit_piston_movement(l, movement);
            // Vec3.equals(Vec3.ZERO): Double.compare of each component with 0.0.
            if movement.x.to_bits() == 0 && movement.y.to_bits() == 0 && movement.z.to_bits() == 0 {
                return;
            }
        }
        if self.stuck_speed_multiplier.length_sqr() > 1.0E-7 {
            if !piston {
                movement = movement.multiply(self.stuck_speed_multiplier.x, self.stuck_speed_multiplier.y, self.stuck_speed_multiplier.z);
            }
            self.stuck_speed_multiplier = Vec3::ZERO;
            self.delta = Vec3::ZERO;
        }
        if !piston {
            movement = self.maybe_back_off_from_edge(l, movement);
        }
        let collided = self.collide(l, movement);
        let d = collided.length_sqr();
        if d > 1.0E-7 || movement.length_sqr() - d < 1.0E-7 {
            if self.fall_distance != 0.0 && d >= 1.0 {
                let min = phys::jmin(collided.length(), 8.0);
                let to = self.pos.add_vec(collided.normalize().scale(min));
                if self.clip_resets_fall_damage(l, self.pos, to) {
                    self.fall_distance = 0.0;
                }
            }
            let from = self.pos;
            let to = from.add_vec(collided);
            self.add_movement_this_tick(Movement { from, to, axis_dependent_original: Some(movement) });
            self.set_pos_vec(to);
        }
        let flag = !phys::equal(movement.x, collided.x);
        let flag1 = !phys::equal(movement.z, collided.z);
        let horizontal_collision = flag || flag1;
        // (A local player moves the way it is authoritative on.)
        self.vertical_collision = movement.y != collided.y;
        self.vertical_collision_below = self.vertical_collision && movement.y < 0.0;
        self.set_on_ground_with_movement(l, self.vertical_collision_below, horizontal_collision, collided);
        if self.horizontal_collision {
            self.minor_horizontal_collision = self.is_horizontal_collision_minor(collided);
        } else {
            self.minor_horizontal_collision = false;
        }
        let on_pos = self.on_pos_legacy(l);
        let state = read::state(l, on_pos);
        self.check_fall_damage(l, collided.y);
        if self.horizontal_collision {
            let d = self.delta;
            self.delta = Vec3::new(if flag { 0.0 } else { d.x }, d.y, if flag1 { 0.0 } else { d.z });
        }
        if movement.y != collided.y {
            self.update_entity_movement_after_fall_on(l, state);
        }
        let factor = self.block_speed_factor(l);
        self.delta = self.delta.multiply(factor as f64, 1.0, factor as f64);
    }

    // Entity.checkFallDamage for a player: the distance counts up while it
    // is not in water; landing ends it (the block's fallOn reaches no one:
    // a creative player takes no damage).
    fn check_fall_damage(&mut self, l: &Level, y: f64) {
        // LivingEntity.checkFallDamage looks for water first.
        if !self.is_in_water() {
            self.update_in_water_state_and_do_water_current_pushing(l);
        }
        if !self.is_in_water() && y < 0.0 {
            self.fall_distance -= (y as f32) as f64;
        }
        if self.on_ground {
            self.fall_distance = 0.0;
        }
    }

    // Block.updateEntityMovementAfterFallOn, and the bounce of slime and beds.
    fn update_entity_movement_after_fall_on(&mut self, l: &Level, state: State) {
        let class = l.reg.blocks[l.block(state) as usize].class.as_str();
        let bounce = |p: &mut Player, multiplier: f64| {
            let d = p.delta;
            if d.y < 0.0 {
                // (A living entity: 1.0.)
                p.delta = Vec3::new(d.x, -d.y * multiplier, d.z);
            }
        };
        match class {
            "SlimeBlock" => {
                if self.is_shift_key_down() {
                    self.delta = self.delta.multiply(1.0, 0.0, 1.0);
                } else {
                    bounce(self, 1.0);
                }
            }
            "BedBlock" => {
                if self.is_shift_key_down() {
                    self.delta = self.delta.multiply(1.0, 0.0, 1.0);
                } else {
                    let d = self.delta;
                    if d.y < 0.0 {
                        self.delta = Vec3::new(d.x, -d.y * 0.66f32 as f64 * 1.0, d.z);
                    }
                }
            }
            _ => self.delta = self.delta.multiply(1.0, 0.0, 1.0),
        }
    }

    // BlockGetter.clip(ClipContext(from, to, FALLDAMAGE_RESETTING, WATER)) is
    // not a miss: the segment meets a block that resets fall damage (a full
    // block for it) or water.
    fn clip_resets_fall_damage(&self, l: &Level, from: Vec3, to: Vec3) -> bool {
        let result = clip::traverse_blocks(from, to, |pos| {
            let state = read::state(l, pos);
            if l.is_air(state) {
                return None;
            }
            if l.reg.in_tag(state, "minecraft:fall_damage_resetting") && clip::block_clip_hits(from, to, pos) {
                return Some(true);
            }
            let fluid_state = l.fluid_of(state);
            if l.fluid_type(fluid_state).is_water() {
                let height = read::fluid_height(l, fluid_state, pos);
                if clip::box_clip_hits(height as f64, from, to, pos) {
                    return Some(true);
                }
            }
            None
        });
        result == Some(true)
    }

    // Player.maybeBackOffFromEdge.
    fn maybe_back_off_from_edge(&self, l: &Level, movement: Vec3) -> Vec3 {
        let step = self.max_up_step() as f64;
        if !self.flying && !(movement.y > 0.0) && self.is_shift_key_down() && self.is_above_ground(l, step as f32) {
            let mut d = movement.x;
            let mut d1 = movement.z;
            let d2 = signum(d) * 0.05;
            let d3 = signum(d1) * 0.05;
            while d != 0.0 && self.can_fall_at_least(l, d, 0.0, step) {
                if d.abs() <= 0.05 {
                    d = 0.0;
                    break;
                }
                d -= d2;
            }
            while d1 != 0.0 && self.can_fall_at_least(l, 0.0, d1, step) {
                if d1.abs() <= 0.05 {
                    d1 = 0.0;
                    break;
                }
                d1 -= d3;
            }
            while d != 0.0 && d1 != 0.0 && self.can_fall_at_least(l, d, d1, step) {
                if d.abs() <= 0.05 {
                    d = 0.0;
                } else {
                    d -= d2;
                }
                if d1.abs() <= 0.05 {
                    d1 = 0.0;
                } else {
                    d1 -= d3;
                }
            }
            Vec3::new(d, movement.y, d1)
        } else {
            movement
        }
    }

    // LivingEntity.maxUpStep: the step height attribute, 0.6.
    pub(super) fn max_up_step(&self) -> f32 {
        0.6f64 as f32
    }

    // Player.isAboveGround.
    fn is_above_ground(&self, l: &Level, step: f32) -> bool {
        self.on_ground || (self.fall_distance < step as f64 && !self.can_fall_at_least(l, 0.0, 0.0, step as f64 - self.fall_distance))
    }

    // Player.canFallAtLeast.
    fn can_fall_at_least(&self, l: &Level, x: f64, z: f64, y: f64) -> bool {
        let b = self.bb;
        let aabb = Aabb::new(b.min_x + 1.0E-7 + x, b.min_y - y - 1.0E-7, b.min_z + 1.0E-7 + z, b.max_x - 1.0E-7 + x, b.min_y, b.max_z - 1.0E-7 + z);
        self.no_collision(l, &aabb)
    }

    // Entity.collide.
    fn collide(&self, l: &Level, movement: Vec3) -> Vec3 {
        let bb = self.bb;
        let expanded = bb.expand_towards(movement.x, movement.y, movement.z);
        let entities = self.entity_shapes(l, &expanded);
        let mut shapes: Vec<Shape> = entities.clone();
        shapes.extend(self.block_shapes(l, &expanded).into_iter().map(|(_, shape)| shape));
        let refs: Vec<&Shape> = shapes.iter().collect();
        let collided = if movement.length_sqr() == 0.0 { movement } else { vanilla::collide_with_shapes(movement, &bb, &refs) };
        let x_changed = movement.x != collided.x;
        let y_changed = movement.y != collided.y;
        let z_changed = movement.z != collided.z;
        let below = y_changed && movement.y < 0.0;
        let step = self.max_up_step();
        if step > 0.0f32 && (below || self.on_ground) && (x_changed || z_changed) {
            let base = if below { bb.mv(0.0, collided.y, 0.0) } else { bb };
            let mut area = base.expand_towards(movement.x, step as f64, movement.z);
            if !below {
                area = area.expand_towards(0.0, -1.0E-5f32 as f64, 0.0);
            }
            // (The entities are those of the first query, whatever the area.)
            let mut shapes: Vec<Shape> = entities;
            shapes.extend(self.block_shapes(l, &area).into_iter().map(|(_, shape)| shape));
            let refs: Vec<&Shape> = shapes.iter().collect();
            for h in candidate_step_up_heights(&base, &refs, step, collided.y as f32) {
                let stepped = vanilla::collide_with_shapes(Vec3::new(movement.x, h as f64, movement.z), &base, &refs);
                if stepped.horizontal_distance_sqr() > collided.horizontal_distance_sqr() {
                    let d = bb.min_y - base.min_y;
                    return stepped.subtract(Vec3::new(0.0, d, 0.0));
                }
            }
        }
        collided
    }

    // LocalPlayer.isHorizontalCollisionMinor.
    pub(super) fn is_horizontal_collision_minor(&self, v: Vec3) -> bool {
        let f = self.y_rot * (std::f64::consts::PI / 180.0) as f32;
        let d = mth::sin(f) as f64;
        let d1 = mth::cos(f) as f64;
        let d2 = self.xxa as f64 * d1 - self.zza as f64 * d;
        let d3 = self.zza as f64 * d1 + self.xxa as f64 * d;
        let d4 = d2 * d2 + d3 * d3;
        let d5 = v.x * v.x + v.z * v.z;
        if !(d4 < 1.0E-5f32 as f64) && !(d5 < 1.0E-5f32 as f64) {
            let d6 = d2 * v.x + d3 * v.z;
            let d7 = crate::jmath::acos(d6 / (d4 * d5).sqrt());
            d7 < 0.13962634f32 as f64
        } else {
            false
        }
    }

    // LocalPlayer.updateAutoJump.
    fn update_auto_jump(&mut self, l: &Level, dx: f32, dz: f32) {
        if !self.can_auto_jump(l) {
            return;
        }
        super::autojump::update(self, l, dx, dz);
    }

    // LocalPlayer.canAutoJump.
    fn can_auto_jump(&mut self, l: &Level) -> bool {
        self.auto_jump_enabled
            && self.auto_jump_time <= 0
            && self.on_ground
            && !self.is_shift_key_down()
            && self.move_vector.0 * self.move_vector.0 + self.move_vector.1 * self.move_vector.1 > 0.0f32
            && self.block_jump_factor(l) as f64 >= 1.0
    }
}

// Entity.collectCandidateStepUpHeights: the heights above the box's feet at
// which the shapes it meets have an edge, up to the step height, but not the
// height it already went.
fn candidate_step_up_heights(bb: &Aabb, shapes: &[&Shape], step: f32, went: f32) -> Vec<f32> {
    let mut heights: Vec<f32> = Vec::new();
    for shape in shapes {
        for &value in &shape.coords[1] {
            let h = (value - bb.min_y) as f32;
            if !(h < 0.0f32) && h != went {
                if h > step {
                    break;
                }
                if !heights.iter().any(|&known| known.to_bits() == h.to_bits()) {
                    heights.push(h);
                }
            }
        }
    }
    heights.sort_by(|a, b| a.total_cmp(b));
    heights
}
