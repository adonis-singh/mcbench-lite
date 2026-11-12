// ArmorStand, a LivingEntity with no AI: loaded from saved data, ticking
// (Entity.baseTick, LivingEntity.baseTick and aiStep as a stand meets them:
// gravity and movement in air and fluids, the air supply, burning, the
// ticks of a dead stand), hurt by explosions, fire and the void, broken with
// its equipment dropped, and saved. Paper's configuration is at its defaults
// (entities.armor-stands.tick true, do-collision-entity-lookups true); the
// Bukkit events (EntityDeathEvent, EntityDamageEvent, EntityCombustEvent and
// the like) have no listener.
//
// What a stand's saved data may hold that this port does not model
// (attributes, effects, a brain, a team, leashes, a sleeping position,
// equipment with enchantments or attribute modifiers, a compass or a map that
// ticks) fails the load, and the caller refuses. The attributes a stand saves
// are the instances Minecraft has made by then (lazily, in an order that
// differs from one run to the next): none when it is imported, so a stand
// that has ticked does not save.

use super::damage::Dmg;
use super::motion::{self, set_pos};
use super::{fluid, item, remove, Entity, Kind, Only};
use crate::codec::{self, fail, Input, R};
use crate::components;
use crate::dir::Pos;
use crate::level::Level;
use crate::loot::ItemStack;
use crate::nbt::{Compound, Tag};
use crate::phys::{self, Aabb, Vec3};
use crate::registry::{State, EMPTY_FLUID};

// EquipmentSlot.VALUES, by the names the equipment is saved under.
pub const SLOT_NAMES: [&str; 8] = ["mainhand", "offhand", "feet", "legs", "chest", "head", "body", "saddle"];
pub const FEET: usize = 2;

// ArmorStand's poses (head, body, left arm, right arm, left leg, right leg)
// and their defaults.
type Rotations = [f32; 3];
const POSE_KEYS: [&str; 6] = ["Head", "Body", "LeftArm", "RightArm", "LeftLeg", "RightLeg"];
const DEFAULT_POSE: [Rotations; 6] = [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [-10.0, 0.0, -10.0], [-15.0, 0.0, 10.0], [-1.0, 0.0, -1.0], [1.0, 0.0, 1.0]];

pub struct StandData {
    // LivingEntity: health, the hurt time, the stamp of the last hurt by a mob
    // (saved), the ticks since it died, the absorption amount and the equipment
    // (EquipmentSlot.VALUES order).
    pub health: f32,
    pub hurt_time: i32,
    pub hurt_by_timestamp: i32,
    pub death_time: i32,
    pub absorption: f32,
    pub equipment: [ItemStack; 8],
    // ArmorStand: the flags (DATA_CLIENT_FLAGS and the invisibility), the
    // disabled slots, the pose, and the tick switch (Paper).
    pub invisible: bool,
    pub small: bool,
    pub show_arms: bool,
    pub no_base_plate: bool,
    pub marker: bool,
    pub disabled_slots: i32,
    pub pose: [Rotations; 6],
    pub can_tick: bool,
    pub can_tick_set_by_api: bool,
    // Entity.inBlockState: the block the entity is in, read once a tick.
    pub in_block_state: Option<State>,
}

pub fn data(e: &Entity) -> &StandData {
    match &e.kind {
        Kind::ArmorStand(d) => d,
        _ => panic!("not an armor stand"),
    }
}

pub fn data_mut(e: &mut Entity) -> &mut StandData {
    match &mut e.kind {
        Kind::ArmorStand(d) => d,
        _ => panic!("not an armor stand"),
    }
}

// LivingEntity.isAlive: not removed and with health.
pub fn is_alive(e: &Entity) -> bool {
    !e.removed && data(e).health > 0.0f32
}

// ArmorStand.hasPhysics: neither a marker nor without gravity.
fn has_physics(e: &Entity) -> bool {
    !data(e).marker && !e.no_gravity
}

// ArmorStand.isIgnoringBlockTriggers: a marker.
pub fn ignores_block_triggers(e: &Entity) -> bool {
    data(e).marker
}

// ArmorStand(EntityType, Level): the constructor draws what Entity does (the
// id and the UUID); the rotation LivingEntity draws from Math.random is
// replaced by the saved one.
pub fn new_stand(l: &mut Level) -> Box<Entity> {
    let t = l.reg.entity_types["minecraft:armor_stand"].clone();
    let stand = StandData {
        health: 20.0f32,
        hurt_time: 0,
        hurt_by_timestamp: 0,
        death_time: 0,
        absorption: 0.0f32,
        equipment: std::array::from_fn(|_| ItemStack::empty()),
        invisible: false,
        small: false,
        show_arms: false,
        no_base_plate: false,
        marker: false,
        disabled_slots: 0,
        pose: DEFAULT_POSE,
        can_tick: true,
        can_tick_set_by_api: false,
        in_block_state: None,
    };
    Box::new(Entity::new(l, Kind::ArmorStand(stand), t.width, t.height, t.eye_height))
}

// ArmorStand.getDefaultDimensions, scaled by LivingEntity.getDimensions (the
// scale attribute is 1.0): a marker has none, a small stand half the size with
// an eye height of its own.
fn dimensions(l: &Level, d: &StandData) -> (f32, f32, f32) {
    if d.marker {
        return (0.0, 0.0, 0.0f32 * 0.85f32);
    }
    let t = &l.reg.entity_types["minecraft:armor_stand"];
    if d.small {
        (t.width * 0.5f32, t.height * 0.5f32, 0.9875f32)
    } else {
        (t.width, t.height, t.eye_height)
    }
}

// ArmorStand.refreshDimensions: the dimensions follow the flags and the stand
// is placed again where it is. (It is not moved out of anything: a stand
// grown after its first tick would be, but the flags are set on load.)
fn refresh_dimensions(l: &mut Level, e: &mut Entity) {
    let (width, height, eye_height) = dimensions(l, data(e));
    e.width = width;
    e.height = height;
    e.eye_height = eye_height;
    let p = e.pos;
    set_pos(l, e, p.x, p.y, p.z);
}

// Rotations.CODEC: three floats, each modulo 360 (none infinite or not a number).
fn rotations(tag: &Tag) -> R<Rotations> {
    let list = codec::items(tag)?;
    if list.len() != 3 {
        return fail(format!("List must have 3 elements, but it had {}", list.len()));
    }
    let mut out = [0.0f32; 3];
    for (i, element) in list.iter().enumerate() {
        let v = codec::float(element)?;
        out[i] = if v.is_infinite() || v.is_nan() { 0.0 } else { v % 360.0f32 };
    }
    Ok(out)
}

// What equipment does that this port does not model: the compass and the map
// tick, enchantments and attribute modifiers change the entity's attributes.
fn check_equipment(l: &Level, stack: &ItemStack) -> R<()> {
    let key = l.reg.items[stack.item as usize].key.as_str();
    if matches!(key, "minecraft:compass" | "minecraft:filled_map") {
        return fail("an armor stand holding an item that ticks");
    }
    for component in ["minecraft:enchantments", "minecraft:attribute_modifiers"] {
        if stack.patch.get(component).is_some() {
            return fail("an armor stand holding an item with enchantments or attribute modifiers");
        }
    }
    Ok(())
}

// Mth.clamp(float, 0.0F, 20.0F): the maximum health is the attribute's default.
fn clamp_health(health: f32) -> f32 {
    if health < 0.0f32 {
        0.0f32
    } else if 20.0f32 < health {
        20.0f32
    } else {
        health
    }
}

// Entity.load and LivingEntity's and ArmorStand's readAdditionalSaveData, for a
// stand the constructor made.
pub fn load(l: &mut Level, tag: &Compound) -> R<super::load::Loaded> {
    let input = Input(tag);
    let mut e = new_stand(l);
    super::load::load_common(l, &mut e, &input, false)?;
    // LivingEntity.readAdditionalSaveData.
    let mut absorption = input.float_or("AbsorptionAmount", 0.0f32)?;
    if absorption.is_nan() {
        absorption = 0.0;
    }
    if input.string("Paper.FrictionState")?.is_some() {
        return fail("a living entity's friction state");
    }
    match input.get("attributes") {
        None => {}
        Some(Tag::List(list)) if list.is_empty() => {}
        Some(_) => return fail("a living entity's attributes"),
    }
    match input.get("active_effects") {
        None => {}
        Some(Tag::List(list)) if list.is_empty() => {}
        Some(_) => return fail("a living entity's effects"),
    }
    if input.get("Bukkit.MaxHealth").is_some() {
        return fail("a living entity's maximum health");
    }
    // setHealth(Health, or the maximum): not a number is the maximum, else clamped.
    let health = input.float_or("Health", 20.0f32)?;
    let health = if health.is_nan() { 20.0f32 } else { clamp_health(health) };
    let hurt_time = input.short_or("HurtTime", 0)? as i32;
    let death_time = input.short_or("DeathTime", 0)? as i32;
    // lastHurtByMobTimestamp is read twice; the second read is the one that stays.
    input.int_or("HurtByTimestamp", 0)?;
    if input.get("Team").is_some() {
        return fail("a living entity's team");
    }
    if input.bool_or("FallFlying", false)? {
        return fail("a living entity that glides");
    }
    if input.get("sleeping_pos").is_some() {
        return fail("a living entity that sleeps");
    }
    match input.get("Brain") {
        None => {}
        Some(Tag::Compound(c)) if c.len() == 1 && matches!(c.get("memories"), Some(Tag::Compound(m)) if m.is_empty()) => {}
        Some(_) => return fail("a living entity's brain"),
    }
    if input.get("last_hurt_by_player").is_some() || input.get("last_hurt_by_mob").is_some() {
        return fail("a living entity hurt by another");
    }
    if input.int_or("last_hurt_by_player_memory_time", 0)? != 0 {
        return fail("a living entity hurt by a player");
    }
    let hurt_by_timestamp = input.int_or("ticks_since_last_hurt_by_mob", 0)?.wrapping_add(e.tick_count);
    let mut equipment: [ItemStack; 8] = std::array::from_fn(|_| ItemStack::empty());
    if let Some(tag) = input.get("equipment") {
        for (key, value) in &codec::map(tag)?.0 {
            let Some(slot) = SLOT_NAMES.iter().position(|name| name == key) else { return fail(format!("No such equipment slot {key}")) };
            let stack = components::decode_stack(l.reg, value)?;
            check_equipment(l, &stack)?;
            equipment[slot] = stack;
        }
    }
    if input.get("locator_bar_icon").is_some() {
        return fail("a living entity's locator bar icon");
    }
    {
        let d = data_mut(&mut e);
        d.absorption = absorption;
        d.health = health;
        d.hurt_time = hurt_time;
        d.death_time = death_time;
        d.hurt_by_timestamp = hurt_by_timestamp;
        d.equipment = equipment;
    }
    // ArmorStand.readAdditionalSaveData: the flags are one synched byte, and a
    // change of it refreshes the dimensions; the marker is set last, so the
    // dimensions end up those of the final flags.
    data_mut(&mut e).invisible = input.bool_or("Invisible", false)?;
    let small = input.bool_or("Small", false)?;
    let arms = input.bool_or("ShowArms", false)?;
    data_mut(&mut e).disabled_slots = input.int_or("DisabledSlots", 0)?;
    let no_base_plate = input.bool_or("NoBasePlate", false)?;
    let marker = input.bool_or("Marker", false)?;
    {
        let d = data_mut(&mut e);
        d.small = small;
        d.show_arms = arms;
        d.no_base_plate = no_base_plate;
        d.marker = marker;
    }
    if small || arms || no_base_plate || marker {
        refresh_dimensions(l, &mut e);
    }
    e.no_physics = !has_physics(&e);
    if let Some(pose) = input.get("Pose") {
        let map = codec::map(pose)?;
        let mut poses = DEFAULT_POSE;
        for (i, key) in POSE_KEYS.iter().enumerate() {
            if let Some(tag) = map.get(key) {
                poses[i] = rotations(tag)?;
            }
        }
        data_mut(&mut e).pose = poses;
    }
    if input.int("Paper.CanTickOverride")?.is_some() {
        let can_tick = input.bool_or("Paper.CanTickOverride", true)?;
        data_mut(&mut e).can_tick = can_tick;
        data_mut(&mut e).can_tick_set_by_api = true;
    }
    // Entity.load: repositionEntityAfterLoad, then a living entity's age.
    let p = e.pos;
    set_pos(l, &mut e, p.x, p.y, p.z);
    e.total_age = input.int_or("Spigot.ticksLived", 0)?;
    let (origin, spawn_reason) = super::load::load_paper(l, &input)?;
    Ok(super::load::Loaded { entity: e, origin, spawn_reason })
}

// LivingEntity.addAdditionalSaveData and ArmorStand's.
pub fn save_fields(l: &Level, e: &Entity, out: &mut Compound) -> R<()> {
    // The attribute instances a stand has made by its first tick are saved in
    // no fixed order, and are not modeled.
    if e.tick_count > 0 {
        return fail("an armor stand that has ticked");
    }
    let d = data(e);
    out.put("Health", Tag::float(d.health));
    out.put("HurtTime", Tag::Short(d.hurt_time as i16));
    out.put_int("HurtByTimestamp", d.hurt_by_timestamp);
    out.put("DeathTime", Tag::Short(d.death_time as i16));
    out.put("AbsorptionAmount", Tag::float(d.absorption));
    out.put("attributes", Tag::List(Vec::new()));
    out.put_boolean("FallFlying", false);
    let mut brain = Compound::new();
    brain.put("memories", Tag::Compound(Compound::new()));
    out.put("Brain", Tag::Compound(brain));
    if d.equipment.iter().any(|s| s.count > 0) {
        let mut equipment = Compound::new();
        for (i, name) in SLOT_NAMES.iter().enumerate() {
            if d.equipment[i].count > 0 {
                equipment.put(name, components::encode_stack(l.reg, &d.equipment[i])?);
            }
        }
        out.put("equipment", Tag::Compound(equipment));
    }
    out.put_boolean("Invisible", d.invisible);
    out.put_boolean("Small", d.small);
    out.put_boolean("ShowArms", d.show_arms);
    out.put_int("DisabledSlots", d.disabled_slots);
    out.put_boolean("NoBasePlate", d.no_base_plate);
    if d.marker {
        out.put_boolean("Marker", true);
    }
    // ArmorStandPose.CODEC: a part that is its default is left out.
    let mut pose = Compound::new();
    for (i, key) in POSE_KEYS.iter().enumerate() {
        if d.pose[i].map(f32::to_bits) != DEFAULT_POSE[i].map(f32::to_bits) {
            pose.put(key, Tag::List(d.pose[i].iter().map(|v| Tag::float(*v)).collect()));
        }
    }
    out.put("Pose", Tag::Compound(pose));
    if d.can_tick_set_by_api {
        out.put_boolean("Paper.CanTickOverride", d.can_tick);
    }
    out.put_string("id", "minecraft:armor_stand");
    Ok(())
}

// --- Ticking ---

// ArmorStand.tick: with ticking off, nothing (the equipment is only looked at);
// else LivingEntity.tick: Entity.tick (baseTick), then aiStep unless removed.
// The rotations of the head and body, and the ranges they are kept in, are not
// observed.
pub fn tick(l: &mut Level, e: &mut Entity) {
    if !data(e).can_tick {
        return;
    }
    base_tick(l, e);
    if !e.removed {
        ai_step(l, e);
    }
}

// Entity.getTicksRequiredToFreeze.
pub const FREEZE_TICKS: i32 = 140;

// LivingEntity.canFreeze: not wearing freeze immune armor (the armor slot
// group is head, chest, legs and feet); no armor stand is a spectator or of
// a freeze immune type.
pub fn can_freeze(l: &Level, e: &Entity) -> bool {
    let immune = &l.reg.item_tags["minecraft:freeze_immune_wearables"];
    ![5usize, 4, 3, 2].iter().any(|&slot| {
        let stack = &data(e).equipment[slot];
        stack.count > 0 && immune[stack.item as usize]
    })
}

// Entity.getInBlockState: the block the stand is in, read once a tick.
pub fn in_block_state(l: &mut Level, e: &mut Entity) -> State {
    if let Some(state) = data(e).in_block_state {
        return state;
    }
    let state = l.get(e.block_pos);
    data_mut(e).in_block_state = Some(state);
    state
}

// LivingEntity.baseTick: Entity.baseTick, then the living part.
fn base_tick(l: &mut Level, e: &mut Entity) {
    // Entity.baseTick.
    data_mut(e).in_block_state = None;
    e.in_powder_snow = false;
    fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    fluid::update_fluid_on_eyes(l, e);
    if e.remaining_fire_ticks > 0 {
        if item::fire_immune(l, e) {
            fluid::clear_fire(e);
        } else {
            if e.remaining_fire_ticks % 20 == 0 && !fluid::is_in_lava(e) {
                hurt(l, e, Dmg::OnFire, 1.0f32);
            }
            e.remaining_fire_ticks -= 1;
        }
    }
    if fluid::is_in_lava(e) {
        e.fall_distance *= 0.5;
    } else {
        e.last_lava_contact = None;
    }
    // checkBelowWorld: void damage is on, 64 blocks under the world;
    // LivingEntity.onBelowWorld hurts it with the void's damage (4.0).
    if e.pos.y < (crate::world::MIN_Y as f64) + -64.0 {
        hurt(l, e, Dmg::OutOfWorld, 4.0f32);
    }
    e.first_tick = false;
    // LivingEntity.baseTick: a stand that is alive. (isInWall hurts a stand
    // in a wall, which does nothing to it.) A stand breathes under water
    // (can_breathe_under_water), so its air comes back wherever it is.
    if is_alive(e) && e.air_supply < 300 {
        e.air_supply = (e.air_supply + 4).min(300);
    }
    if data(e).hurt_time > 0 {
        data_mut(e).hurt_time -= 1;
    }
    // isDeadOrDying: tickDeath removes it after twenty ticks (no loot, no event).
    if data(e).health <= 0.0f32 {
        data_mut(e).death_time += 1;
        if data(e).death_time >= 20 && !e.removed {
            remove(l, e);
        }
    }
}

// LivingEntity.aiStep as a stand meets it.
fn ai_step(l: &mut Level, e: &mut Entity) {
    // The motion that is under 0.003 is dropped.
    let delta = e.delta;
    let x = if delta.x.abs() < 0.003 { 0.0 } else { delta.x };
    let z = if delta.z.abs() < 0.003 { 0.0 } else { delta.z };
    let y = if delta.y.abs() < 0.003 { 0.0 } else { delta.y };
    e.delta = Vec3::new(x, y, z);
    // (serverAiStep does nothing, nothing jumps, nothing rides.) ArmorStand
    // only travels with physics.
    if has_physics(e) {
        travel(l, e);
    }
    motion::apply_effects_from_blocks(l, e);
    // The freezing: the ticks wear off two a tick unless the stand is in
    // powder snow and can freeze (freezeLocked is never set; removeFrost and
    // tryAddFrost change a movement speed attribute no stand reads), and a
    // frozen stand is hurt every forty ticks (the freeze damage does nothing
    // to it).
    if !(e.in_powder_snow && can_freeze(l, e)) {
        e.ticks_frozen = (e.ticks_frozen - 2).max(0);
    }
    if e.tick_count % 40 == 0 && e.ticks_frozen >= FREEZE_TICKS && can_freeze(l, e) {
        hurt(l, e, Dmg::Freeze, 1.0f32);
    }
    push_entities(l, e);
}

// LivingEntity.travel with no input (no input is Vec3.ZERO, which moveRelative
// adds to the motion).
fn travel(l: &mut Level, e: &mut Entity) {
    if e.was_touching_water || fluid::is_in_lava(e) {
        travel_in_fluid(l, e);
    } else {
        travel_in_air(l, e);
    }
}

// LivingEntity.getEffectiveGravity: the gravity attribute (0.08), none without
// gravity.
fn effective_gravity(e: &Entity) -> f64 {
    if e.no_gravity {
        0.0
    } else {
        0.08
    }
}

// LivingEntity.onClimbable.
fn on_climbable(l: &mut Level, e: &mut Entity) -> bool {
    let state = in_block_state(l, e);
    if l.reg.in_tag(state, "minecraft:climbable") {
        return true;
    }
    // TrapDoorBlock, and LivingEntity.trapdoorUsableAsLadder: open, over a
    // ladder that faces the same way.
    if l.reg.blocks[l.block(state) as usize].class == "TrapDoorBlock" {
        if !l.reg.get_bool(state, l.reg.property_of(state, "open")) {
            return false;
        }
        let below = l.get(Pos::new(e.block_pos.x(), e.block_pos.y() - 1, e.block_pos.z()));
        if l.reg.blocks[l.block(below) as usize].key != "minecraft:ladder" {
            return false;
        }
        return l.reg.get_dir(below, l.reg.property_of(below, "facing")) == l.reg.get_dir(state, l.reg.property_of(state, "facing"));
    }
    false
}

// LivingEntity.handleOnClimbable.
fn handle_on_climbable(l: &mut Level, e: &mut Entity, delta: Vec3) -> Vec3 {
    if on_climbable(l, e) {
        e.fall_distance = 0.0;
        let x = phys::clamp(delta.x, -0.15f32 as f64, 0.15f32 as f64);
        let z = phys::clamp(delta.z, -0.15f32 as f64, 0.15f32 as f64);
        let y = phys::jmax(delta.y, -0.15f32 as f64);
        return Vec3::new(x, y, z);
    }
    delta
}

// LivingEntity.travelInAir.
fn travel_in_air(l: &mut Level, e: &mut Entity) {
    let below = motion::block_pos_below(l, e);
    let friction = if e.on_ground {
        let state = l.get(below);
        l.reg.blocks[l.block(state) as usize].friction
    } else {
        1.0f32
    };
    let f1 = friction * 0.91f32;
    // handleRelativeFrictionAndCalculateMovement: moveRelative, handleOnClimbable, move.
    e.delta = e.delta.add_vec(Vec3::ZERO);
    let delta = e.delta;
    e.delta = handle_on_climbable(l, e, delta);
    motion::move_entity(l, e, e.delta);
    let mut moved = e.delta;
    if e.horizontal_collision && on_climbable(l, e) {
        moved = Vec3::new(moved.x, 0.2, moved.z);
    }
    let y = moved.y - effective_gravity(e);
    e.delta = Vec3::new(moved.x * f1 as f64, y * 0.98f32 as f64, moved.z * f1 as f64);
}

// Level.containsAnyLiquid.
fn contains_any_liquid(l: &mut Level, aabb: &Aabb) -> bool {
    for x in phys::floor(aabb.min_x)..phys::ceil(aabb.max_x) {
        for y in phys::floor(aabb.min_y)..phys::ceil(aabb.max_y) {
            for z in phys::floor(aabb.min_z)..phys::ceil(aabb.max_z) {
                let state = l.get(Pos::new(x, y, z));
                if l.fluid_of(state) != EMPTY_FLUID {
                    return true;
                }
            }
        }
    }
    false
}

// Entity.isFree(box): nothing collides with the box, and no fluid is in it.
fn is_free(l: &mut Level, e: &Entity, aabb: &Aabb) -> bool {
    super::collide::no_collision(l, e, aabb) && !contains_any_liquid(l, aabb)
}

// LivingEntity.getFluidFallingAdjustedMovement.
fn fluid_falling_adjusted_movement(gravity: f64, is_falling: bool, delta: Vec3) -> Vec3 {
    if gravity != 0.0 {
        let y = if is_falling && (delta.y - 0.005).abs() >= 0.003 && (delta.y - gravity / 16.0).abs() < 0.003 { -0.003 } else { delta.y - gravity / 16.0 };
        Vec3::new(delta.x, y, delta.z)
    } else {
        delta
    }
}

// LivingEntity.travelInFluid.
fn travel_in_fluid(l: &mut Level, e: &mut Entity) {
    let falling = e.delta.y <= 0.0;
    let y = e.pos.y;
    let gravity = effective_gravity(e);
    if e.was_touching_water {
        // The water slow down (0.8F); the water movement efficiency is zero.
        e.delta = e.delta.add_vec(Vec3::ZERO);
        motion::move_entity(l, e, e.delta);
        let mut delta = e.delta;
        if e.horizontal_collision && on_climbable(l, e) {
            delta = Vec3::new(delta.x, 0.2, delta.z);
        }
        delta = delta.multiply(0.8f32 as f64, 0.8f32 as f64, 0.8f32 as f64);
        e.delta = fluid_falling_adjusted_movement(gravity, falling, delta);
    } else {
        e.delta = e.delta.add_vec(Vec3::ZERO);
        motion::move_entity(l, e, e.delta);
        // getFluidJumpThreshold: 0.4 for eyes at least that high.
        let threshold = if (e.eye_height as f64) < 0.4 { 0.0 } else { 0.4 };
        if fluid::lava_height(e) <= threshold {
            e.delta = e.delta.multiply(0.5, 0.8f32 as f64, 0.5);
            e.delta = fluid_falling_adjusted_movement(gravity, falling, e.delta);
        } else {
            e.delta = e.delta.scale(0.5);
        }
        if gravity != 0.0 {
            e.delta = e.delta.add(0.0, -gravity / 4.0, 0.0);
        }
    }
    let delta = e.delta;
    if e.horizontal_collision {
        let aabb = e.bb.mv(delta.x, delta.y + 0.6f32 as f64 - e.pos.y + y, delta.z);
        if is_free(l, e, &aabb) {
            e.delta = Vec3::new(delta.x, 0.3f32 as f64, delta.z);
        }
    }
}

// ArmorStand.pushEntities: the ridable minecarts in the stand's box that are
// near it are pushed by it. (Nothing else is, and nothing pushes it.)
fn push_entities(l: &mut Level, e: &mut Entity) {
    let aabb = e.bb;
    let pos = e.pos;
    let carts = super::entities_in(l, Some(e), None, Only::Carts, &aabb, |o| matches!(&o.kind, Kind::Minecart(c) if c.class == super::minecart::Class::Minecart));
    for id in carts {
        if l.entities.all[&id].pos.distance_to_sqr(pos) <= 0.2 {
            let mut cart = l.entities.all.remove(&id).expect("a minecart the stand pushes");
            super::minecart::push_cart(l, &mut cart, e);
            if !cart.removed {
                l.entities.all.insert(id, cart);
            }
        }
    }
}

// LivingEntity.checkFallDamage's part before Entity's: a stand that is not in
// water looks again for the water it touches.
pub fn before_fall_damage(l: &mut Level, e: &mut Entity) {
    if !e.was_touching_water {
        fluid::update_in_water_state_and_do_water_current_pushing(l, e);
    }
}

// --- Hurting and breaking ---

// What Minecraft draws from the shared random when a stand dies of an
// EntityDeathEvent (CraftEventFactory.populateFields reads the voice pitch:
// two floats).
fn death_event(l: &mut Level) {
    l.shared_random.next_float();
    l.shared_random.next_float();
}

// ArmorStand.hurtServer for damage of this type. It never returns true here.
pub fn hurt(l: &mut Level, e: &mut Entity, dmg: Dmg, _amount: f32) -> bool {
    if e.removed {
        return false;
    }
    // (The damage that reaches here is never zero: the event would cancel it.)
    if dmg.bypasses_invulnerability() {
        kill(l, e, true);
        return false;
    }
    // isInvulnerableTo: isInvulnerableToBase (no stand is fire immune, and
    // nothing here is fall damage), and no equipment makes it immune.
    if e.invulnerable || data(e).marker {
        return false;
    }
    let invisible = data(e).invisible;
    match dmg {
        Dmg::Explosion => {
            if !invisible {
                broken_by_anything(l, e);
                kill(l, e, false);
            }
        }
        // Burning (on_fire) is in burns_armor_stands: four points of damage to
        // a stand with health left. Lava is in neither list.
        Dmg::OnFire => {
            if !invisible && data(e).health > 0.5f32 {
                cause_damage(l, e, 4.0f32);
            }
        }
        // IGNITES_ARMOR_STANDS: a stand on fire takes 0.15, else catches fire
        // (igniteForSeconds(5.0F), whose EntityCombustEvent has no listener).
        Dmg::InFire | Dmg::Campfire => {
            if !invisible {
                if item::is_on_fire(l, e) {
                    cause_damage(l, e, 0.15f32);
                } else {
                    fluid::ignite_for_ticks(e, phys::floor(5.0f32 as f64 * 20.0f32 as f64));
                }
            }
        }
        // Damage no tag of the stand's lists names: the event is called, and
        // the stand takes nothing.
        Dmg::Lava | Dmg::Cactus | Dmg::SweetBerryBush | Dmg::HotFloor | Dmg::Freeze => {}
        Dmg::OutOfWorld => unreachable!("out_of_world bypasses invulnerability"),
    }
    false
}

// ArmorStand.causeDamage.
fn cause_damage(l: &mut Level, e: &mut Entity, amount: f32) {
    let health = data(e).health - amount;
    if health <= 0.5f32 {
        broken_by_anything(l, e);
        kill(l, e, false);
    } else {
        data_mut(e).health = clamp_health(health);
        // gameEvent(ENTITY_DAMAGE, damageSource.getEntity()): none of these has an entity.
        let event = crate::game_event!(l, "entity_damage");
        l.game_event(event, e.pos, crate::game_event::Context::of_entity(None));
    }
}

// ArmorStand.brokenByAnything: the equipment is dropped (LivingEntity.
// dropAllDeathLoot: the death event, then its drops, in the slots' order, at
// the block above the stand, and an empty loot table), and the stand stripped.
fn broken_by_anything(l: &mut Level, e: &mut Entity) {
    let stacks: Vec<ItemStack> = data(e).equipment.iter().filter(|s| s.count > 0).cloned().collect();
    death_event(l);
    let above = Pos::new(e.block_pos.x(), e.block_pos.y() + 1, e.block_pos.z());
    for stack in stacks {
        item::pop_resource(l, above, stack);
    }
    for slot in data_mut(e).equipment.iter_mut() {
        *slot = ItemStack::empty();
    }
}

// ArmorStand.kill(level, damageSource, callEvent): the death event if asked,
// then removed as killed, and ENTITY_DIE.
fn kill(l: &mut Level, e: &mut Entity, call_event: bool) {
    if call_event {
        death_event(l);
    }
    remove(l, e);
    let source = crate::game_event::Source::of(l, e);
    let event = crate::game_event!(l, "entity_die");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(Some(source)));
}

// Entity.lavaHurt() of a stand: lava's damage does nothing to it (no sound).
pub fn lava_hurt(l: &mut Level, e: &mut Entity) {
    if item::fire_immune(l, e) {
        return;
    }
    if hurt(l, e, Dmg::Lava, 4.0f32) && !super::minecart::is_silent(e) {
        l.shared_random.next_float();
    }
}

// ArmorStand.ignoreExplosion: an invisible stand (the explosion affects
// block-like entities: the game rule mobGriefing is on).
pub fn ignores_explosion(e: &Entity) -> bool {
    data(e).invisible
}
