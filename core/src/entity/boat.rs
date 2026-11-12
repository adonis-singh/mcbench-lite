// AbstractBoat (Boat, Raft and their chest kinds, one class per entity type
// here): floating, the water and the ground it is in, being pushed, bubble
// columns, being destroyed. Paper's configuration is at its defaults
// (collisions.allow-vehicle-collisions true, only-players-collide false); the
// Bukkit events (VehicleUpdateEvent, VehicleMoveEvent, VehicleDamageEvent,
// VehicleDestroyEvent, VehicleEntityCollisionEvent) have no listener.
//
// Nothing rides a boat here: no player steers it, so its controls, paddles
// and passengers are not modeled (an entity that would board it refuses).

use super::damage::Dmg;
use super::fluid;
use super::motion::{self, set_pos};
use super::vehicle::{self, Vehicle};
use super::{remove, Entity, Kind, Only};
use crate::blocks::container::{self, empty_slots_n};
use crate::codec::{fail, Input, R};
use crate::collision::{self, Moved};
use crate::dir::Pos;
use crate::level::Level;
use crate::nbt::{Compound, Tag};
use crate::phys::{self, Aabb, Vec3};

// AbstractBoat.Status.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    InWater,
    UnderWater,
    UnderFlowingWater,
    OnLand,
    InAir,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::InWater => "IN_WATER",
            Status::UnderWater => "UNDER_WATER",
            Status::UnderFlowingWater => "UNDER_FLOWING_WATER",
            Status::OnLand => "ON_LAND",
            Status::InAir => "IN_AIR",
        }
    }
}

pub struct BoatData {
    // The entity type's key (it is also its drop item's).
    pub key: String,
    // AbstractChestBoat: 27 stacks.
    pub chest: bool,
    pub vehicle: Vehicle,
    pub status: Option<Status>,
    pub old_status: Option<Status>,
    pub water_level: f64,
    pub land_friction: f32,
    pub last_yd: f64,
    pub out_of_control_ticks: f32,
    pub delta_rotation: f32,
    pub is_above_bubble_column: bool,
    pub bubble_column_direction_is_down: bool,
    pub bubble_time: i32,
}

pub fn data(e: &Entity) -> &BoatData {
    match &e.kind {
        Kind::Boat(d) => d,
        _ => panic!("not a boat"),
    }
}

pub fn data_mut(e: &mut Entity) -> &mut BoatData {
    match &mut e.kind {
        Kind::Boat(d) => d,
        _ => panic!("not a boat"),
    }
}

// Whether an entity type's key is a boat's (boats and rafts, with and
// without a chest).
pub fn is_boat_type(key: &str) -> bool {
    key.ends_with("_boat") || key.ends_with("_raft")
}

// A new boat of the entity type (its constructor): not added to the level.
pub fn new_boat(l: &mut Level, key: &str) -> Box<Entity> {
    let t = l.reg.entity_types[key].clone();
    let chest = key.contains("chest_");
    let data = BoatData {
        key: key.to_string(),
        chest,
        vehicle: Vehicle::new(),
        status: None,
        old_status: None,
        water_level: 0.0,
        land_friction: 0.0f32,
        last_yd: 0.0,
        out_of_control_ticks: 0.0f32,
        delta_rotation: 0.0f32,
        is_above_bubble_column: false,
        bubble_column_direction_is_down: false,
        bubble_time: 0,
    };
    let e = Box::new(Entity::new(l, Kind::Boat(data), t.width, t.height, t.eye_height));
    if chest {
        l.entities.containers.insert(e.id, empty_slots_n(27));
    }
    e
}

// Entity.load and AbstractBoat's readAdditionalSaveData (the leash is
// refused by the importer before) and the chest boat's, for a boat the
// constructor made.
pub fn load(l: &mut Level, tag: &Compound, key: &str) -> R<super::load::Loaded> {
    let input = Input(tag);
    let mut e = new_boat(l, key);
    super::load::load_common(l, &mut e, &input, false)?;
    if input.get("leash").is_some() {
        return fail("a boat's leash");
    }
    if data(&e).chest {
        if input.get("LootTable").is_some() {
            return fail("a boat's loot table");
        }
        input.long_or("LootTableSeed", 0)?;
        let mut items = empty_slots_n(27);
        crate::blocks::load::load_all_items(l.reg, &input, &mut items)?;
        l.entities.containers.insert(e.id, items);
    }
    let (origin, spawn_reason) = super::load::load_paper(l, &input)?;
    Ok(super::load::Loaded { entity: e, origin, spawn_reason })
}

// addAdditionalSaveData of AbstractBoat (a leash, none here) and the chest
// boat's.
pub fn save_fields(l: &Level, e: &Entity, out: &mut Compound) -> R<()> {
    let d = data(e);
    if d.chest {
        container::save_all_items(l.reg, &l.entities.containers[&e.id], true, out);
    }
    out.put_string("id", &d.key);
    Ok(())
}

// Containers.dropContents(level, this, this) for a chest boat: see
// minecart::drop_contents.
pub fn drop_contents(l: &mut Level, e: &Entity) {
    let stacks = l.entities.containers[&e.id].clone();
    for stack in &stacks {
        super::item::drop_item_stack(l, e.pos.x, e.pos.y, e.pos.z, stack);
    }
}

// AbstractBoat.getDefaultGravity, then Entity.getGravity.
fn gravity(e: &Entity) -> f64 {
    if e.no_gravity {
        0.0
    } else {
        0.04
    }
}

// Entity.getY(scale).
fn get_y(e: &Entity, scale: f64) -> f64 {
    e.pos.y + e.height as f64 * scale
}

// AbstractBoat.isUnderWater (the status).
pub fn is_under_water(e: &Entity) -> bool {
    matches!(data(e).status, Some(Status::UnderWater | Status::UnderFlowingWater))
}

// AbstractBoat.isUnderwater: UNDER_FLOWING_WATER, UNDER_WATER or none.
fn find_underwater(l: &mut Level, e: &Entity) -> Option<Status> {
    let bb = e.bb;
    let d = bb.max_y + 0.001;
    let floor = phys::floor(bb.min_x);
    let ceil = phys::ceil(bb.max_x);
    let floor1 = phys::floor(bb.max_y);
    let ceil1 = phys::ceil(d);
    let floor2 = phys::floor(bb.min_z);
    let ceil2 = phys::ceil(bb.max_z);
    let mut flag = false;
    for i in floor..ceil {
        for i1 in floor1..ceil1 {
            for i2 in floor2..ceil2 {
                let pos = Pos::new(i, i1, i2);
                let fluid_state = l.fluid_state(pos);
                if l.fluid_type(fluid_state).is_water() && d < (i1 as f32 + crate::fluid::height(l, fluid_state, pos)) as f64 {
                    if !l.reg.fluids[fluid_state as usize].source {
                        return Some(Status::UnderFlowingWater);
                    }
                    flag = true;
                }
            }
        }
    }
    flag.then_some(Status::UnderWater)
}

// AbstractBoat.checkInWater.
fn check_in_water(l: &mut Level, e: &mut Entity) -> bool {
    let bb = e.bb;
    let floor = phys::floor(bb.min_x);
    let ceil = phys::ceil(bb.max_x);
    let floor1 = phys::floor(bb.min_y);
    let ceil1 = phys::ceil(bb.min_y + 0.001);
    let floor2 = phys::floor(bb.min_z);
    let ceil2 = phys::ceil(bb.max_z);
    let mut flag = false;
    let mut water_level = -f64::MAX;
    for i in floor..ceil {
        for i1 in floor1..ceil1 {
            for i2 in floor2..ceil2 {
                let pos = Pos::new(i, i1, i2);
                let fluid_state = l.fluid_state(pos);
                if l.fluid_type(fluid_state).is_water() {
                    let f = i1 as f32 + crate::fluid::height(l, fluid_state, pos);
                    water_level = phys::jmax(f as f64, water_level);
                    flag |= bb.min_y < f as f64;
                }
            }
        }
    }
    data_mut(e).water_level = water_level;
    flag
}

// AbstractBoat.getGroundFriction: the average friction of the blocks the box
// just under the boat touches (NaN where it touches none).
fn ground_friction(l: &mut Level, e: &Entity) -> f32 {
    let bb = e.bb;
    let aabb = Aabb::new(bb.min_x, bb.min_y - 0.001, bb.min_z, bb.max_x, bb.min_y, bb.max_z);
    let i = phys::floor(aabb.min_x) - 1;
    let i1 = phys::ceil(aabb.max_x) + 1;
    let i2 = phys::floor(aabb.min_y) - 1;
    let i3 = phys::ceil(aabb.max_y) + 1;
    let i4 = phys::floor(aabb.min_z) - 1;
    let i5 = phys::ceil(aabb.max_z) + 1;
    let mut f = 0.0f32;
    let mut i6 = 0;
    for i7 in i..i1 {
        for i8 in i4..i5 {
            let i9 = (if i7 != i && i7 != i1 - 1 { 0 } else { 1 }) + (if i8 != i4 && i8 != i5 - 1 { 0 } else { 1 });
            if i9 != 2 {
                for i10 in i2..i3 {
                    if i9 <= 0 || i10 != i2 && i10 != i3 - 1 {
                        let pos = Pos::new(i7, i10, i8);
                        let state = l.get(pos);
                        if l.reg.blocks[l.block(state) as usize].class != "WaterlilyBlock" && collision_overlaps(l, state, pos, &aabb) {
                            f += l.reg.blocks[l.block(state) as usize].friction;
                            i6 += 1;
                        }
                    }
                }
            }
        }
    }
    f / i6 as f32
}

// Shapes.joinIsNotEmpty(state.getCollisionShape(level, pos).move(pos), Shapes.create(aabb), AND):
// the block's collision shape overlaps the box.
fn collision_overlaps(l: &Level, state: crate::registry::State, pos: Pos, aabb: &Aabb) -> bool {
    let shape = collision::shape_of(l, state, pos, collision::Ctx::Empty);
    if shape.is_empty() {
        return false;
    }
    if let Some(single) = shape.single {
        let moved = Aabb::new(single[0] + pos.x() as f64, single[1] + pos.y() as f64, single[2] + pos.z() as f64, single[3] + pos.x() as f64, single[4] + pos.y() as f64, single[5] + pos.z() as f64);
        collision::boxes_overlap(&moved, aabb)
    } else {
        collision::voxels_overlap(&Moved { shape, off: [pos.x() as f64, pos.y() as f64, pos.z() as f64] }, aabb)
    }
}

// AbstractBoat.getStatus.
fn get_status(l: &mut Level, e: &mut Entity) -> Status {
    if let Some(status) = find_underwater(l, e) {
        data_mut(e).water_level = e.bb.max_y;
        return status;
    }
    if check_in_water(l, e) {
        return Status::InWater;
    }
    let ground_friction = ground_friction(l, e);
    if ground_friction > 0.0f32 {
        data_mut(e).land_friction = ground_friction;
        Status::OnLand
    } else {
        Status::InAir
    }
}

// AbstractBoat.getWaterLevelAbove.
fn water_level_above(l: &mut Level, e: &Entity) -> f32 {
    let bb = e.bb;
    let floor = phys::floor(bb.min_x);
    let ceil = phys::ceil(bb.max_x);
    let floor1 = phys::floor(bb.max_y);
    let ceil1 = phys::ceil(bb.max_y - data(e).last_yd);
    let floor2 = phys::floor(bb.min_z);
    let ceil2 = phys::ceil(bb.max_z);
    'layers: for i in floor1..ceil1 {
        let mut f = 0.0f32;
        let mut last_y = i;
        for i1 in floor..ceil {
            for i2 in floor2..ceil2 {
                let pos = Pos::new(i1, i, i2);
                last_y = pos.y();
                let fluid_state = l.fluid_state(pos);
                if l.fluid_type(fluid_state).is_water() {
                    let height = crate::fluid::height(l, fluid_state, pos);
                    f = if f.is_nan() || height.is_nan() { f32::NAN } else { f.max(height) };
                }
                if f >= 1.0f32 {
                    continue 'layers;
                }
            }
        }
        if f < 1.0f32 {
            return last_y as f32 + f;
        }
    }
    (ceil1 + 1) as f32
}

// AbstractBoat.floatBoat.
fn float_boat(l: &mut Level, e: &mut Entity) {
    let mut d = -gravity(e);
    let mut d1 = 0.0;
    let mut f = 0.05f32;
    let old = data(e).old_status;
    let status = data(e).status.expect("a boat with a status in its tick");
    if old == Some(Status::InAir) && status != Status::InAir && status != Status::OnLand {
        let level = get_y(e, 1.0);
        data_mut(e).water_level = level;
        let d2 = (water_level_above(l, e) - e.height) as f64 + 0.101;
        let moved = e.bb.mv(0.0, d2 - e.pos.y, 0.0);
        if super::collide::no_collision(l, e, &moved) {
            motion::move_entity(l, e, Vec3::new(0.0, d2 - e.pos.y, 0.0));
            e.delta = e.delta.multiply(1.0, 0.0, 1.0);
            data_mut(e).last_yd = 0.0;
        }
        data_mut(e).status = Some(Status::InWater);
    } else {
        match status {
            Status::InWater => {
                d1 = (data(e).water_level - e.pos.y) / e.height as f64;
                f = 0.9f32;
            }
            Status::UnderFlowingWater => {
                d = -7.0E-4;
                f = 0.9f32;
            }
            Status::UnderWater => {
                d1 = 0.01f32 as f64;
                f = 0.45f32;
            }
            Status::InAir => f = 0.9f32,
            // (No player steers it: the land friction stays.)
            Status::OnLand => f = data(e).land_friction,
        }
        let delta = e.delta;
        e.delta = Vec3::new(delta.x * f as f64, delta.y + d, delta.z * f as f64);
        data_mut(e).delta_rotation *= f;
        if d1 > 0.0 {
            let delta1 = e.delta;
            e.delta = Vec3::new(delta1.x, (delta1.y + d1 * (0.04 / 0.65)) * 0.75, delta1.z);
        }
    }
}

// AbstractBoat.checkFallDamage.
pub fn check_fall_damage(l: &mut Level, e: &mut Entity, y: f64) {
    data_mut(e).last_yd = e.delta.y;
    if e.on_ground {
        e.fall_distance = 0.0;
    } else {
        let below = l.fluid_state(Pos::new(e.block_pos.x(), e.block_pos.y() - 1, e.block_pos.z()));
        if !l.fluid_type(below).is_water() && y < 0.0 {
            e.fall_distance -= (y as f32) as f64;
        }
    }
}

// AbstractBoat.tickBubbleColumn (on a server).
fn tick_bubble_column(e: &mut Entity) {
    if !data(e).is_above_bubble_column {
        data_mut(e).bubble_time = 0;
    }
    let mut bubble_time = data(e).bubble_time;
    if bubble_time > 0 {
        bubble_time -= 1;
        data_mut(e).bubble_time = bubble_time;
        let i = 60 - bubble_time - 1;
        if i > 0 && bubble_time == 0 {
            data_mut(e).bubble_time = 0;
            let delta = e.delta;
            if data(e).bubble_column_direction_is_down {
                e.delta = delta.add(0.0, -0.7, 0.0);
            } else {
                // (A player passenger would make it 2.7.)
                e.delta = Vec3::new(delta.x, 0.6, delta.z);
            }
        }
        data_mut(e).is_above_bubble_column = false;
    }
}

// AbstractBoat.onAboveBubbleColumn(downwards, pos).
pub fn on_above_bubble_column(l: &mut Level, e: &mut Entity, downwards: bool) {
    data_mut(e).is_above_bubble_column = true;
    data_mut(e).bubble_column_direction_is_down = downwards;
    if data(e).bubble_time == 0 {
        data_mut(e).bubble_time = 60;
    }
    if !is_under_water(e) && l.shared_random.next_int_bound(100) == 0 {
        // The splash sound's pitch and the particle's offsets.
        l.shared_random.next_float();
        l.shared_random.next_float();
        l.shared_random.next_float();
        let event = crate::game_event!(l, "splash");
        l.game_event(event, e.pos, crate::game_event::Context::of_entity(None));
    }
}

// Entity.baseTick for a boat in the level.
fn base_tick(l: &mut Level, e: &mut Entity) {
    e.in_powder_snow = false;
    fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    fluid::update_fluid_on_eyes(l, e);
    if e.remaining_fire_ticks > 0 {
        if super::item::fire_immune(l, e) {
            fluid::clear_fire(e);
        } else {
            if e.remaining_fire_ticks % 20 == 0 && !fluid::is_in_lava(e) {
                vehicle::hurt(l, e, Dmg::OnFire, 1.0f32);
            }
            e.remaining_fire_ticks -= 1;
        }
    }
    if fluid::is_in_lava(e) {
        e.fall_distance *= 0.5;
    } else {
        e.last_lava_contact = None;
    }
    // checkBelowWorld: void damage is on, 64 blocks under the world.
    if e.pos.y < (crate::world::MIN_Y as f64) + -64.0 {
        remove(l, e);
    }
    e.first_tick = false;
}

// AbstractBoat.tick.
pub fn tick(l: &mut Level, e: &mut Entity) {
    {
        let d = data_mut(e);
        d.old_status = d.status;
    }
    let status = get_status(l, e);
    data_mut(e).status = Some(status);
    if status != Status::UnderWater && status != Status::UnderFlowingWater {
        data_mut(e).out_of_control_ticks = 0.0f32;
    } else {
        data_mut(e).out_of_control_ticks += 1.0f32;
    }
    // (ejectPassengers at 60 ticks under water: nobody rides.)
    data_mut(e).vehicle.wear_down();
    base_tick(l, e);
    float_boat(l, e);
    motion::move_entity(l, e, e.delta);
    motion::apply_effects_from_blocks(l, e);
    motion::apply_effects_from_blocks(l, e);
    tick_bubble_column(e);
    // The entities pushed by the boat (EntitySelector.pushableBy).
    let aabb = e.bb.inflate(0.2f32 as f64, -0.01f32 as f64, 0.2f32 as f64);
    let others = super::entities_in(l, Some(e), Some(e.id), Only::Any, &aabb, |o| super::collide::is_collidable(o) && super::collide::is_pushable(o));
    for id in others {
        // (An entity that is a LivingEntity could board; none here is.)
        let mut other = l.entities.all.remove(&id).expect("an entity the boat pushes");
        push(e, &mut other);
        if !other.removed {
            l.entities.all.insert(id, other);
        }
    }
}

// AbstractBoat.push(entity): the boat is pushed by (and pushes) the entity.
pub fn push(this: &mut Entity, entity: &mut Entity) {
    if matches!(entity.kind, Kind::Boat(_)) {
        if entity.bb.min_y < this.bb.max_y {
            vehicle::push_entity(this, entity);
        }
    } else if entity.bb.min_y <= this.bb.min_y {
        vehicle::push_entity(this, entity);
    }
}

// AbstractBoat.destroy and AbstractChestBoat.destroy.
pub fn destroy(l: &mut Level, e: &mut Entity) {
    let key = data(e).key.clone();
    vehicle::destroy_with_item(l, e, &key);
    if data(e).chest {
        // chestVehicleDestroyed: the contents drop again.
        drop_contents(l, e);
    }
}

#[allow(dead_code)]
fn unused(_: Tag, _: Pos) {
    let _ = set_pos;
}
