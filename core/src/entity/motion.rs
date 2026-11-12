// Entity movement: Entity.move and what it calls, with moonrise's
// collisions (collision.rs), for an entity that no vehicle, rider, freezing
// or gravity switch acts on (what fluids do to it is in fluid.rs and
// item.rs). Whatever block behaviour this port does not model refuses.

use super::inside::{self, Applier};
use super::{fluid, item, on_move, Entity, Kind, Movement};
use crate::clip;
use crate::collision::{self, Collected};
use crate::dir::Pos;
use crate::level::{refuse, Level};
use crate::phys::{self, Aabb, Vec3};
use crate::registry::{Fluid, State, AIR};

// Whether the state's block class overrides a behaviour method.
pub fn overrides(l: &Level, state: State, name: &str) -> bool {
    let bit = crate::blocks::BEHAVIOUR.iter().position(|&m| m == name).unwrap_or_else(|| panic!("no behaviour method {name}"));
    l.blocks.overrides[l.block(state) as usize] & (1 << bit) != 0
}

// A block behaviour the entity reaches that this port does not model.
fn unported(l: &Level, state: State, name: &str) -> ! {
    refuse(format!("{}.{} ({})", l.reg.blocks[l.block(state) as usize].class, name, l.reg.describe(state)))
}

// Entity.setPos(x, y, z): setPosRaw with the bounding box made again.
pub fn set_pos(l: &mut Level, e: &mut Entity, x: f64, y: f64, z: f64) {
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return;
    }
    if e.pos.x != x || e.pos.y != y || e.pos.z != z {
        e.pos = Vec3::new(x, y, z);
        let (fx, fy, fz) = (phys::floor(x), phys::floor(y), phys::floor(z));
        if fx != e.block_pos.x() || fy != e.block_pos.y() || fz != e.block_pos.z() {
            e.block_pos = Pos::new(fx, fy, fz);
            // setPosRaw forgets Entity.inBlockState when the block position changes.
            if let Kind::ArmorStand(data) = &mut e.kind {
                data.in_block_state = None;
            }
        }
        on_move(l, e);
    }
    e.bb = e.make_bounding_box(e.pos);
    if let Some(t) = &mut l.entities.ticking {
        if t.id == e.id {
            t.bb = e.bb;
        }
    }
}

// Entity.teleportRelative: teleportTo, whose snapTo sets the position, the
// old position and rotation and makes the box again. A hanging entity's setPos
// takes the block it hangs at from the position and puts the entity at the
// center of its box (BlockAttachedEntity.setPos); a painting's snapTo is only
// that setPos, an item frame's is Entity.snapTo (setPosRaw, the old position,
// then reapplyPosition).
pub fn teleport_relative(l: &mut Level, e: &mut Entity, dx: f64, dy: f64, dz: f64) {
    let (x, y, z) = (e.pos.x + dx, e.pos.y + dy, e.pos.z + dz);
    match &e.kind {
        Kind::Painting(_) => super::frame::frame_set_pos(l, e, x, y, z),
        Kind::Frame(_) => {
            super::frame::set_raw(l, e, x, y, z);
            e.set_old_pos_and_rot();
            super::frame::reapply_position(l, e);
        }
        _ => {
            set_pos(l, e, x, y, z);
            e.set_old_pos_and_rot();
        }
    }
}

pub fn set_pos_vec(l: &mut Level, e: &mut Entity, p: Vec3) {
    set_pos(l, e, p.x, p.y, p.z);
}

// Entity.getOnPos(yOffset).
pub fn get_on_pos(l: &mut Level, e: &Entity, y_offset: f32) -> Pos {
    if let Some(supporting) = e.main_supporting_block {
        // getChunkIfLoadedImmediately.
        if l.has_chunk_at(supporting) {
            if !(y_offset > 1.0E-5f32) {
                return supporting;
            }
            let state = l.get(supporting);
            let fence = (!(y_offset as f64 <= 0.5) || !l.reg.in_tag(state, "minecraft:fences")) && !l.reg.in_tag(state, "minecraft:walls");
            let gate = l.reg.blocks[l.block(state) as usize].class == "FenceGateBlock";
            return if fence && !gate { Pos::new(supporting.x(), phys::floor(e.pos.y - y_offset as f64), supporting.z()) } else { supporting };
        }
    }
    Pos::new(phys::floor(e.pos.x), phys::floor(e.pos.y - y_offset as f64), phys::floor(e.pos.z))
}

pub fn on_pos_legacy(l: &mut Level, e: &Entity) -> Pos {
    get_on_pos(l, e, 0.2f32)
}

// Entity.getBlockPosBelowThatAffectsMyMovement (0.500001), which
// ItemEntity and ExperienceOrb override with 0.999999.
pub fn block_pos_below(l: &mut Level, e: &Entity) -> Pos {
    get_on_pos(l, e, if matches!(e.kind, Kind::Item(_) | Kind::Orb(_)) { 0.999999f32 } else { 0.500001f32 })
}

// Entity.checkSupportingBlock.
fn check_supporting_block(l: &Level, e: &mut Entity, on_ground: bool, movement: Option<Vec3>) {
    if on_ground {
        let bb = e.bb;
        let aabb = Aabb::new(bb.min_x, bb.min_y - 1.0E-6, bb.min_z, bb.max_x, bb.min_y, bb.max_z);
        let ctx = super::collide::context(l, e);
        let mut found = collision::find_supporting_block(l, e.pos, ctx, &aabb);
        if found.is_some() || e.on_ground_no_blocks {
            e.main_supporting_block = found;
        } else if let Some(m) = movement {
            found = collision::find_supporting_block(l, e.pos, ctx, &aabb.mv(-m.x, 0.0, -m.z));
            e.main_supporting_block = found;
        }
        e.on_ground_no_blocks = found.is_none();
    } else {
        e.on_ground_no_blocks = false;
        e.main_supporting_block = None;
    }
}

// Entity.collide for an entity with no step height and no hard-colliding
// entity around.
fn collide(l: &Level, e: &Entity, movement: Vec3) -> Vec3 {
    let (x_zero, y_zero, z_zero) = (movement.x == 0.0, movement.y == 0.0, movement.z == 0.0);
    if x_zero & y_zero & z_zero {
        return movement;
    }
    let current = e.bb;
    let initial = if x_zero & z_zero {
        if movement.y < 0.0 {
            collision::cut_downwards(&current, movement.y)
        } else {
            collision::cut_upwards(&current, movement.y)
        }
    } else {
        current.expand_towards(movement.x, movement.y, movement.z)
    };
    // CollisionUtil.getEntityHardCollisions, then the blocks'; the entities'
    // boxes follow the blocks' single boxes.
    let entity_boxes = super::collide::hard_collisions(l, e, &initial);
    let mut collected = Collected::default();
    collision::block_collisions(l, &initial, super::collide::context(l, e), false, &mut collected);
    collected.boxes.extend(entity_boxes);
    collision::perform_collisions(movement, current, &collected)
}

// Entity.getBlockSpeedFactor.
fn block_speed_factor(l: &mut Level, e: &Entity) -> f32 {
    let state = l.get(e.block_pos);
    // AbstractMinecart.getBlockSpeedFactor: 1.0 on a rail.
    if matches!(e.kind, Kind::Minecart(_)) && l.reg.in_tag(state, "minecraft:rails") {
        return 1.0f32;
    }
    let speed = l.reg.blocks[l.block(state) as usize].speed_factor;
    let key = &l.reg.blocks[l.block(state) as usize].key;
    if key != "minecraft:water" && key != "minecraft:bubble_column" {
        if speed as f64 == 1.0 {
            let below = block_pos_below(l, e);
            let below_state = l.get(below);
            l.reg.blocks[l.block(below_state) as usize].speed_factor
        } else {
            speed
        }
    } else {
        speed
    }
}

// Entity.getDimensionChangingDelay: the portal cooldown an entity gets from
// going through a portal (300; VehicleEntity's is 10).
fn dimension_changing_delay(e: &Entity) -> i32 {
    match e.kind {
        Kind::Minecart(_) | Kind::Boat(_) => 10,
        _ => 300,
    }
}

// Entity.setAsInsidePortal(portal, pos).
pub fn set_as_inside_portal(e: &mut Entity, portal: crate::registry::BlockId, pos: Pos) {
    if e.portal_cooldown > 0 {
        e.portal_cooldown = dimension_changing_delay(e);
    } else {
        match &mut e.portal_process {
            Some(process) if process.portal == portal => {
                if !process.inside_this_tick {
                    process.entry = pos;
                    process.inside_this_tick = true;
                }
            }
            _ => e.portal_process = Some(super::PortalProcess { portal, entry: pos, time: 0, inside_this_tick: true }),
        }
    }
}

// Entity.handlePortal on the server: the portal cooldown runs down; an entity
// in a portal (PortalProcessor.processPortalTeleportation) that has been in it
// for the portal's transition time (none, for anything but a player) gets the
// portal cooldown, and goes where the portal leads, which is nowhere: the level
// is the only one the server has (every portal's getPortalDestination finds no
// level to go to, or an end gateway no exit portal).
pub fn handle_portal(e: &mut Entity) {
    // processPortalCooldown.
    if e.portal_cooldown > 0 {
        e.portal_cooldown -= 1;
    }
    // canUsePortal(false): nothing rides here, so the entity must be alive.
    let can_use = super::is_alive(e);
    let delay = dimension_changing_delay(e);
    let Some(process) = &mut e.portal_process else { return };
    let teleports = if !process.inside_this_tick {
        // decayTick.
        process.time = (process.time - 4).max(0);
        false
    } else {
        process.inside_this_tick = false;
        if can_use {
            let before = process.time;
            process.time += 1;
            before >= 0
        } else {
            false
        }
    };
    if teleports {
        e.portal_cooldown = delay;
    } else if process.time <= 0 {
        // hasExpired.
        e.portal_process = None;
    }
}

// BlockGetter.clip(ClipContext(from, to, FALLDAMAGE_RESETTING, WATER, entity))
// is not a miss: the segment meets a block that resets fall damage (a full
// block for it) or water before it leaves the loaded chunks.
fn clip_resets_fall_damage(l: &mut Level, from: Vec3, to: Vec3) -> bool {
    let result = clip::traverse_blocks(from, to, |pos| {
        let Some(state) = l.get_if_loaded(pos) else { return Some(false) };
        if l.is_air(state) {
            return None;
        }
        if l.reg.in_tag(state, "minecraft:fall_damage_resetting") && clip::block_clip_hits(from, to, pos) {
            return Some(true);
        }
        // ClipContext.Fluid.WATER picks the water's shape.
        let fluid_state = l.fluid_of(state);
        if l.fluid_type(fluid_state).is_water() {
            let height = crate::fluid::height(l, fluid_state, pos);
            if clip::box_clip_hits(height as f64, from, to, pos) {
                return Some(true);
            }
        }
        None
    });
    result == Some(true)
}

// Entity.causeFallDamage (whatever the damage multiplier: no kind reads it).
pub fn cause_fall_damage(l: &mut Level, e: &mut Entity, fall_distance: f64) {
    match e.kind {
        Kind::Falling(_) => super::falling::cause_fall_damage(l, e, fall_distance),
        Kind::Minecart(_) => super::minecart::cause_fall_damage(l, e, fall_distance),
        Kind::Boat(_) => unreachable!("a boat's checkFallDamage is its own"),
        // Entity.causeFallDamage: no passengers.
        Kind::Item(_) | Kind::Tnt(_) | Kind::Frame(_) | Kind::Painting(_) | Kind::Orb(_) => {}
        Kind::Golem(_) => super::golem::unported(e, "falls"),
        // LivingEntity.causeFallDamage: the fall damage the stand takes does nothing to it.
        Kind::ArmorStand(_) => {}
    }
}

// Entity.checkFallDamage: `y` is the movement that was made.
fn check_fall_damage(l: &mut Level, e: &mut Entity, y: f64, state: State, pos: Pos) {
    if matches!(e.kind, Kind::Boat(_)) {
        super::boat::check_fall_damage(l, e, y);
        return;
    }
    if matches!(e.kind, Kind::ArmorStand(_)) {
        super::armor_stand::before_fall_damage(l, e);
    }
    if !e.was_touching_water && y < 0.0 {
        e.fall_distance -= (y as f32) as f64;
    }
    if e.on_ground {
        if e.fall_distance > 0.0 {
            // Block.fallOn: the entity takes the fall (causeFallDamage).
            if overrides(l, state, "fallOn") {
                let fall_distance = e.fall_distance;
                crate::blocks::fall_on(l, state, pos, e, fall_distance);
            } else {
                cause_fall_damage(l, e, e.fall_distance);
            }
            // Level.gameEvent(HIT_GROUND, position, Context.of(this, the
            // supporting block's state, or the state landed on)).
            let affected = match e.main_supporting_block {
                Some(supporting) => l.get(supporting),
                None => state,
            };
            let source = crate::game_event::Source::of(l, e);
            let event = crate::game_event!(l, "hit_ground");
            l.game_event(event, e.pos, crate::game_event::Context::of(Some(source), affected));
        }
        e.fall_distance = 0.0;
    }
}

// Entity.isOnRails.
fn is_on_rails(e: &Entity) -> bool {
    match &e.kind {
        Kind::Minecart(c) => c.on_rails,
        _ => false,
    }
}

// Entity.vibrationAndSoundEffectsFromBlock: the step event of an entity
// moving on (or in) a block.
fn vibration_effects_from_block(l: &mut Level, e: &Entity, state: State, play_step_sound: bool, broadcast_game_event: bool) -> bool {
    if l.is_air(state) {
        return false;
    }
    let climbable = l.reg.in_tag(state, "minecraft:climbable") || l.reg.blocks[l.block(state) as usize].key == "minecraft:powder_snow";
    // (No entity here crouches or swims.)
    if e.on_ground || climbable || is_on_rails(e) {
        // The step sound reaches no one, but a crystal block's chimes draw from
        // the shared random (and keep a count of their own).
        if play_step_sound && l.reg.in_tag(state, "minecraft:crystal_sound_blocks") && e.tick_count >= 20 {
            refuse("an entity stepping on a block that chimes");
        }
        if broadcast_game_event {
            let source = crate::game_event::Source::of(l, e);
            let event = crate::game_event!(l, "step");
            l.game_event(event, e.pos, crate::game_event::Context::of(Some(source), state));
        }
        true
    } else {
        false
    }
}

// Entity.applyMovementEmissionAndPlaySound: the movement counts steps, and the
// block under the entity may be stepped on (STEP, SWIM events).
fn apply_movement_emission(l: &mut Level, e: &mut Entity, emits_sounds: bool, emits_events: bool, movement: Vec3, pos: Pos, state: State) {
    let f1 = (movement.length() * 0.6f32 as f64) as f32;
    let f2 = (movement.horizontal_distance() * 0.6f32 as f64) as f32;
    let on_pos = get_on_pos(l, e, 1.0E-5f32);
    let block_state = l.get(on_pos);
    let climbable = l.reg.in_tag(block_state, "minecraft:climbable") || l.reg.blocks[l.block(block_state) as usize].key == "minecraft:powder_snow";
    e.move_dist += if climbable { f1 } else { f2 };
    e.fly_dist += f1;
    if e.move_dist > e.next_step && !l.is_air(block_state) {
        let flag = on_pos == pos;
        let mut flag1 = vibration_effects_from_block(l, e, state, emits_sounds, flag);
        if !flag {
            flag1 |= vibration_effects_from_block(l, e, block_state, false, emits_events);
        }
        if flag1 {
            e.next_step = (e.move_dist as i32 + 1) as f32;
        } else if e.was_touching_water {
            e.next_step = (e.move_dist as i32 + 1) as f32;
            if emits_sounds {
                // waterSwimSound: the pitch is 1.0F + (nextFloat() - nextFloat()) * 0.4F.
                l.shared_random.next_float();
                l.shared_random.next_float();
            }
            if emits_events {
                let source = crate::game_event::Source::of(l, e);
                let event = crate::game_event!(l, "swim");
                l.game_event(event, e.pos, crate::game_event::Context::of_entity(Some(source)));
            }
        }
    }
}

// MoverType: how an entity is moved. Only the two this port reaches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mover {
    SelfMove,
    Piston,
}

// Vec3.equals(Vec3.ZERO): Double.compare of each component with 0.0.
fn is_zero_vec(v: Vec3) -> bool {
    v.x.to_bits() == 0 && v.y.to_bits() == 0 && v.z.to_bits() == 0
}

// Entity.limitPistonMovement.
fn limit_piston_movement(l: &Level, e: &mut Entity, pos: Vec3) -> Vec3 {
    if pos.length_sqr() <= 1.0E-7 {
        return pos;
    }
    let game_time = l.game_time;
    if game_time != e.piston_deltas_game_time {
        e.piston_deltas = [0.0; 3];
        e.piston_deltas_game_time = game_time;
    }
    let limit = 1.0E-5f32 as f64;
    if pos.x != 0.0 {
        let d = restrict(e, 0, pos.x);
        if d.abs() <= limit { Vec3::ZERO } else { Vec3::new(d, 0.0, 0.0) }
    } else if pos.y != 0.0 {
        let d = restrict(e, 1, pos.y);
        if d.abs() <= limit { Vec3::ZERO } else { Vec3::new(0.0, d, 0.0) }
    } else if pos.z != 0.0 {
        let d = restrict(e, 2, pos.z);
        if d.abs() <= limit { Vec3::ZERO } else { Vec3::new(0.0, 0.0, d) }
    } else {
        Vec3::ZERO
    }
}

// Entity.applyPistonMovementRestriction: the distance, clamped so that the
// total along the axis in this game time stays within 0.51 either way.
fn restrict(e: &mut Entity, axis: usize, distance: f64) -> f64 {
    let d = phys::clamp(distance + e.piston_deltas[axis], -0.51, 0.51);
    let distance = d - e.piston_deltas[axis];
    e.piston_deltas[axis] = d;
    distance
}

// Entity.move(MoverType.SELF, movement).
pub fn move_entity(l: &mut Level, e: &mut Entity, movement: Vec3) {
    move_entity_as(l, e, Mover::SelfMove, movement);
}

// Entity.move(type, movement).
pub fn move_entity_as(l: &mut Level, e: &mut Entity, mover: Mover, movement: Vec3) {
    // ItemFrame.move and BlockAttachedEntity.move: a frame is never moved.
    if let Kind::Frame(_) = e.kind {
        super::frame::moved(l, e, movement);
        return;
    }
    if e.no_physics {
        set_pos(l, e, e.pos.x + movement.x, e.pos.y + movement.y, e.pos.z + movement.z);
        e.horizontal_collision = false;
        e.vertical_collision = false;
        e.vertical_collision_below = false;
        e.minor_horizontal_collision = false;
        return;
    }
    let mut movement = movement;
    if mover == Mover::Piston {
        movement = limit_piston_movement(l, e, movement);
        if is_zero_vec(movement) {
            return;
        }
    }
    // Entity.stuckSpeedMultiplier (makeStuckInBlock).
    if e.stuck_speed_multiplier.length_sqr() > 1.0E-7 {
        if mover != Mover::Piston {
            movement = movement.multiply(e.stuck_speed_multiplier.x, e.stuck_speed_multiplier.y, e.stuck_speed_multiplier.z);
        }
        e.stuck_speed_multiplier = Vec3::ZERO;
        e.delta = Vec3::ZERO;
    }
    let collided = collide(l, e, movement);
    let d = collided.length_sqr();
    if d > 1.0E-7 || movement.length_sqr() - d < 1.0E-7 {
        if e.fall_distance != 0.0 && d >= 1.0 {
            let min = phys::jmin(collided.length(), 8.0);
            let to = e.pos.add_vec(collided.normalize().scale(min));
            if clip_resets_fall_damage(l, e.pos, to) {
                e.fall_distance = 0.0;
            }
        }
        let from = e.pos;
        let to = from.add_vec(collided);
        assert!(e.movement_this_tick.len() < 100, "a hundred movements in a tick");
        e.movement_this_tick.push_back(Movement { from, to, axis_dependent_original: Some(movement) });
        set_pos_vec(l, e, to);
    }
    let flag = !phys::equal(movement.x, collided.x);
    let flag1 = !phys::equal(movement.z, collided.z);
    e.horizontal_collision = flag || flag1;
    e.vertical_collision = movement.y != collided.y;
    e.vertical_collision_below = e.vertical_collision && movement.y < 0.0;
    e.on_ground = e.vertical_collision_below;
    check_supporting_block(l, e, e.on_ground, Some(collided));
    e.minor_horizontal_collision = false;
    let on_pos = on_pos_legacy(l, e);
    let state = l.get(on_pos);
    check_fall_damage(l, e, collided.y, state, on_pos);
    if e.removed {
        return;
    }
    if e.horizontal_collision {
        let delta = e.delta;
        e.delta = Vec3::new(if flag { 0.0 } else { delta.x }, delta.y, if flag1 { 0.0 } else { delta.z });
    }
    if movement.y != collided.y {
        crate::blocks::update_entity_movement_after_fall_on(l, state, e);
    }
    // Entity.getMovementEmission: a minecart and a boat emit events only, a
    // stand sounds and events; the rest emit nothing.
    match e.kind {
        Kind::Minecart(_) | Kind::Boat(_) => apply_movement_emission(l, e, false, true, collided, on_pos, state),
        Kind::ArmorStand(_) => apply_movement_emission(l, e, true, true, collided, on_pos, state),
        _ => {}
    }
    let factor = block_speed_factor(l, e);
    e.delta = e.delta.multiply(factor as f64, 1.0, factor as f64);
}

// Entity.applyEffectsFromBlocks().
pub fn apply_effects_from_blocks(l: &mut Level, e: &mut Entity) {
    let mut movements: Vec<Movement> = e.movement_this_tick.drain(..).collect();
    if movements.is_empty() {
        movements.push(Movement { from: e.old_position(), to: e.pos, axis_dependent_original: None });
    } else if movements.last().unwrap().to.distance_to_sqr(e.pos) > 9.9999994E-11f32 as f64 {
        let last = movements.last().unwrap().to;
        movements.push(Movement { from: last, to: e.pos, axis_dependent_original: None });
    }
    apply_effects_for_movements(l, e, &movements);
}

// Entity.applyEffectsFromBlocks(oldPosition, position).
pub fn apply_effects_between(l: &mut Level, e: &mut Entity, old_position: Vec3, position: Vec3) {
    apply_effects_for_movements(l, e, &[Movement { from: old_position, to: position, axis_dependent_original: None }]);
}

// Entity.applyEffectsFromBlocks(List<Movement>).
pub fn apply_effects_for_movements(l: &mut Level, e: &mut Entity, movements: &[Movement]) {
    // isAffectedByBlocks.
    if e.removed || e.no_physics {
        return;
    }
    if e.on_ground {
        let on_pos = on_pos_legacy(l, e);
        let state = l.get(on_pos);
        if crate::blocks::has_step_on(l, state) {
            crate::blocks::step_on(l, on_pos, state, e);
        } else if overrides(l, state, "stepOn") && l.reg.blocks[l.block(state) as usize].class != "MagmaBlock" {
            // (MagmaBlock.stepOn hurts living entities only.)
            unported(l, state, "stepOn");
        }
    }
    let was_on_fire = item::is_on_fire(l, e);
    // Entity.isFreezing: some ticks frozen.
    let was_freezing = e.ticks_frozen > 0;
    let fire_ticks = e.remaining_fire_ticks;
    let mut collector = Applier::default();
    check_inside_blocks(l, e, movements, &mut collector);
    collector.apply_and_clear(l, e);
    // isInRain: no rain falls in this world.
    if (was_on_fire && !item::is_on_fire(l, e)) || (was_freezing && e.ticks_frozen <= 0) {
        // playEntityOnFireExtinguishedSound.
        l.shared_random.next_float();
        l.shared_random.next_float();
    }
    let increased = e.remaining_fire_ticks > fire_ticks;
    if !item::is_on_fire(l, e) && !increased {
        e.remaining_fire_ticks = 0;
    }
}

// Axis step order: Direction.axisStepOrder(vector).
fn axis_step_order(v: Vec3) -> [usize; 3] {
    if v.x.abs() < v.z.abs() {
        [1, 2, 0]
    } else {
        [1, 0, 2]
    }
}

// Classes without an entityInside hook whose entityInside does nothing to an
// entity of this level: it acts on arrows (buttons), ravagers (crops), bees
// (eyeblossoms) or boats (waterlilies, below) only. (The event it calls first
// has no listener.)
const INSIDE_NO_EFFECT: [&str; 10] = [
    "ButtonBlock",
    "CropBlock",
    "CarrotBlock",
    "PotatoBlock",
    "BeetrootBlock",
    "TorchflowerCropBlock",
    "PitcherCropBlock",
    "EyeblossomBlock",
    "WaterlilyBlock",
    "BubbleColumnBlock",
];

// Entity.checkInsideBlocks(List<Movement>, collector).
fn check_inside_blocks(l: &mut Level, e: &mut Entity, movements: &[Movement], collector: &mut Applier) {
    let mut visited = std::collections::HashSet::new();
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
                        left -= check_inside_blocks_between(l, e, collector, &mut visited, from, next, left);
                        from = next;
                    }
                }
            }
            _ => left -= check_inside_blocks_between(l, e, collector, &mut visited, movement.from, movement.to, 16),
        }
        if left <= 0 {
            check_inside_blocks_between(l, e, collector, &mut visited, movement.to, movement.to, 1);
        }
    }
}

// Entity.collidedWithFluid.
fn collided_with_fluid(l: &mut Level, e: &Entity, fluid_state: crate::registry::FluidId, pos: Pos, from: Vec3, to: Vec3) -> bool {
    if fluid_state == crate::registry::EMPTY_FLUID {
        return false;
    }
    // Fluid.getAABB.
    let height = crate::fluid::height(l, fluid_state, pos);
    let aabb = Aabb::new(pos.x() as f64, pos.y() as f64, pos.z() as f64, pos.x() as f64 + 1.0, (pos.y() as f32 + height) as f64, pos.z() as f64 + 1.0);
    // collidedWithShapeMovingFrom.
    let start = e.make_bounding_box(from);
    start.collided_along_vector(to.subtract(from), &[aabb])
}

// Entity.checkInsideBlocks(from, to, collector, visited, maxSteps): the
// blocks the entity's box touches along the way, a step apart, and the
// fluids in them; the number of steps taken.
fn check_inside_blocks_between(
    l: &mut Level,
    e: &mut Entity,
    collector: &mut Applier,
    visited: &mut std::collections::HashSet<i64>,
    from: Vec3,
    to: Vec3,
    max_steps: i32,
) -> i32 {
    let aabb = e.make_bounding_box(to).deflate(1.0E-5f32 as f64);
    let far = from.distance_to_sqr(to) > 0.9999900000002526f64 * 0.9999900000002526f64;
    let mut last = 0;
    for_each_block_intersected_between(from, to, &aabb, &mut |l, pos, index| {
        if !super::is_alive(e) {
            return false;
        }
        if index >= max_steps {
            return false;
        }
        last = index;
        let state = l.get(pos);
        if l.reg.flags(state) & AIR != 0 {
            return true;
        }
        let fluid_state = l.fluid_of(state);
        // getEntityInsideCollisionShape: Shapes.block() but for the classes that
        // override it; a shape the entity's box meets moving from `from` to `to`.
        let inside = match inside::inside_shape(l, e, state, pos) {
            None => true,
            Some(shape) => inside::collided_with_shape(e, from, to, &shape, pos),
        };
        let in_fluid = collided_with_fluid(l, e, fluid_state, pos, from, to);
        if (inside || in_fluid) && visited.insert(pos.as_long()) {
            if inside {
                let past_edges = far || aabb.intersects_box(pos.x() as f64, pos.y() as f64, pos.z() as f64, pos.x() as f64 + 1.0, pos.y() as f64 + 1.0, pos.z() as f64 + 1.0);
                collector.advance_step(index, pos);
                entity_inside(l, e, state, pos, collector, past_edges);
            }
            if in_fluid {
                collector.advance_step(index, pos);
                match l.fluid_type(fluid_state) {
                    Fluid::FlowingWater | Fluid::Water => collector.apply(inside::EXTINGUISH),
                    Fluid::FlowingLava | Fluid::Lava => {
                        collector.apply(inside::CLEAR_FREEZE);
                        collector.apply(inside::LAVA_IGNITE);
                        collector.run_after(inside::LAVA_IGNITE, inside::Effect::LavaHurt(pos));
                    }
                    Fluid::Empty => {}
                }
            }
        }
        true
    }, l);
    last + 1
}

// BlockState.entityInside for an item entity: a block that has one does
// something to an item only if it is ported here.
fn entity_inside(l: &mut Level, e: &mut Entity, state: State, pos: Pos, applier: &mut Applier, past_edges: bool) {
    if !crate::blocks::overrides_entity_inside(l, state) {
        return;
    }
    let class = l.reg.blocks[l.block(state) as usize].class.as_str();
    if class == "BubbleColumnBlock" {
        // BubbleColumnBlock.entityInside (EntityInsideBlockEvent has no listener).
        if past_edges {
            let above = l.get(pos.above());
            // (getCollisionShape(level, pos): the column's own position, which a
            // block whose shape is read at its position sees as its own.)
            let above_shape = crate::shape_ops::collision_shape_at(l, above, pos);
            let above_fluid = l.fluid_of(above);
            let drag_down = l.reg.get_bool(state, l.reg.property_of(state, "drag"));
            if above_shape.is_empty() && above_fluid == crate::registry::EMPTY_FLUID {
                if matches!(e.kind, Kind::Boat(_)) {
                    // AbstractBoat.onAboveBubbleColumn replaces the pull.
                    super::boat::on_above_bubble_column(l, e, drag_down);
                } else {
                    fluid::on_above_bubble_column(l, e, drag_down);
                }
            } else {
                fluid::on_inside_bubble_column(e, drag_down);
            }
        }
    } else if let Some(inside) = crate::blocks::entity_inside_of(l, state) {
        // The ported entityInside methods model item entities, unless the
        // class's are for every kind.
        if !matches!(e.kind, Kind::Item(_)) && !crate::blocks::all_entities(l, state) {
            unported(l, state, "entityInside");
        }
        inside(l, state, pos, e, applier, past_edges);
    } else if class == "WaterlilyBlock" {
        // WaterlilyBlock.entityInside: a boat destroys it, with its drops
        // (EntityChangeBlockEvent has no listener).
        if matches!(e.kind, Kind::Boat(_)) {
            let source = crate::game_event::Source::of(l, e);
            l.destroy_block_by(pos, true, 512, Some(source));
        }
    } else if !INSIDE_NO_EFFECT.contains(&class) {
        unported(l, state, "entityInside");
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
    // firstIndex runs slowest, thirdIndex fastest.
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
fn for_each_block_intersected_between(from: Vec3, to: Vec3, bb: &Aabb, visit: &mut dyn FnMut(&mut Level, Pos, i32) -> bool, l: &mut Level) -> bool {
    let walked = walk_blocks_between(from, to, bb, visit, l);
    l.cursor_end();
    walked
}

// The walk of forEachBlockIntersectedBetween. Each of its iterators yields one
// BlockPos.MutableBlockPos that it moves along (Level.cursor_moved): what keeps
// that position, as a block event queued by an end gateway does, follows it.
fn walk_blocks_between(from: Vec3, to: Vec3, bb: &Aabb, visit: &mut dyn FnMut(&mut Level, Pos, i32) -> bool, l: &mut Level) -> bool {
    let travel = to.subtract(from);
    if travel.length_sqr() < (1.0E-5f32 as f64) * (1.0E-5f32 as f64) {
        // BlockPos.betweenClosed(box): x fastest, then y, then z.
        let (min_x, min_y, min_z) = (phys::floor(bb.min_x), phys::floor(bb.min_y), phys::floor(bb.min_z));
        let (max_x, max_y, max_z) = (phys::floor(bb.max_x), phys::floor(bb.max_y), phys::floor(bb.max_z));
        l.cursor_begin();
        for z in min_z.min(max_z)..=min_z.max(max_z) {
            for y in min_y.min(max_y)..=min_y.max(max_y) {
                for x in min_x.min(max_x)..=min_x.max(max_x) {
                    l.cursor_moved(Pos::new(x, y, z));
                    if !visit(l, Pos::new(x, y, z), 0) {
                        return false;
                    }
                }
            }
        }
        return true;
    }
    let mut set = std::collections::HashSet::new();
    let moved_back = bb.mv(-travel.x, -travel.y, -travel.z);
    l.cursor_begin();
    for pos in between_corners_in_direction(corners(&moved_back), travel) {
        l.cursor_moved(pos);
        if !visit(l, pos, 0) {
            return false;
        }
        set.insert(pos);
    }
    let i = add_collisions_along_travel(&mut set, travel, bb, visit, l);
    if i < 0 {
        return false;
    }
    l.cursor_begin();
    for pos in between_corners_in_direction(corners(bb), travel) {
        l.cursor_moved(pos);
        if set.insert(pos) && !visit(l, pos, i + 1) {
            return false;
        }
    }
    true
}

// The corners betweenCornersInDirection(AABB, ...) takes: the floors of the
// minimum and the maximum position.
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
fn add_collisions_along_travel(
    output: &mut std::collections::HashSet<Pos>,
    travel: Vec3,
    bb: &Aabb,
    visit: &mut dyn FnMut(&mut Level, Pos, i32) -> bool,
    l: &mut Level,
) -> i32 {
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
            l.cursor_begin();
            for pos in between_corners_in_direction([fx, fy, fz, f3, f4, f5], travel) {
                l.cursor_moved(pos);
                if output.insert(pos) && !visit(l, pos, i3) {
                    return -1;
                }
            }
        }
    }
    i3
}

// Entity.moveTowardsClosestSpace.
pub fn move_towards_closest_space(l: &mut Level, e: &mut Entity, x: f64, y: f64, z: f64) {
    let block = Pos::new(phys::floor(x), phys::floor(y), phys::floor(z));
    let within = Vec3::new(x - block.x() as f64, y - block.y() as f64, z - block.z() as f64);
    // (direction, axis, positive).
    use crate::dir::Dir::*;
    let mut direction = Up;
    let mut best = f64::MAX;
    for dir in [North, South, West, East, Up] {
        let at = block.relative(dir);
        let state = l.get(at);
        if !crate::shape_ops::is_collision_shape_full_block(l, state, at) {
            let axis = match dir.axis() {
                crate::dir::Axis::X => 0,
                crate::dir::Axis::Y => 1,
                crate::dir::Axis::Z => 2,
            };
            let d1 = within.get(axis);
            let positive = matches!(dir, Up | South | East);
            let d2 = if positive { 1.0 - d1 } else { d1 };
            if d2 < best {
                best = d2;
                direction = dir;
            }
        }
    }
    let f = l.shared_random.next_float() * 0.2f32 + 0.1f32;
    let positive = matches!(direction, Up | South | East);
    let f1 = if positive { 1.0f32 } else { -1.0f32 };
    let vec31 = e.delta.scale(0.75);
    e.delta = match direction.axis() {
        crate::dir::Axis::X => Vec3::new((f1 * f) as f64, vec31.y, vec31.z),
        crate::dir::Axis::Y => Vec3::new(vec31.x, (f1 * f) as f64, vec31.z),
        crate::dir::Axis::Z => Vec3::new(vec31.x, vec31.y, (f1 * f) as f64),
    };
}
