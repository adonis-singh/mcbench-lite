// AbstractMinecart and its kinds (Minecart, MinecartChest, MinecartHopper,
// MinecartFurnace, MinecartTNT) with the old minecart behaviour: the
// minecart_improvements feature is not enabled in this level
// (FeatureFlags.VANILLA_SET), so AbstractMinecart makes an
// OldMinecartBehavior. Paper's configuration is at its defaults
// (collisions.allow-vehicle-collisions true, only-players-collide false,
// fixes.tnt-entity-height-nerf disabled); the Bukkit events (VehicleUpdateEvent,
// VehicleMoveEvent, VehicleEntityCollisionEvent, VehicleDamageEvent,
// VehicleDestroyEvent, ExplosionPrimeEvent) have no listener.
//
// A minecart with a passenger, a command block minecart and a spawner
// minecart are not modeled (nothing here makes a rider).

use super::damage::Dmg;
use super::fluid;
use super::item;
use super::motion::{self, set_pos};
use super::vehicle::{self, push_by as push, Vehicle};
use super::{remove, Entity, Kind, Only};
use crate::blocks::container::{self, empty_slots_n, Stack};
use crate::blocks::rail::{self, RailShape};
use crate::codec::{fail, Input, R};
use crate::dir::Pos;
use crate::level::{refuse, Level};
use crate::nbt::{Compound, Tag};
use crate::phys::{self, clamp, jmin, Vec3};
use crate::registry::State;

// The classes of minecart, by entity type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Minecart,
    Chest,
    Hopper,
    Furnace,
    Tnt,
}

impl Class {
    pub fn key(self) -> &'static str {
        match self {
            Class::Minecart => "minecraft:minecart",
            Class::Chest => "minecraft:chest_minecart",
            Class::Hopper => "minecraft:hopper_minecart",
            Class::Furnace => "minecraft:furnace_minecart",
            Class::Tnt => "minecraft:tnt_minecart",
        }
    }

    pub fn by_key(key: &str) -> Option<Class> {
        [Class::Minecart, Class::Chest, Class::Hopper, Class::Furnace, Class::Tnt].into_iter().find(|c| c.key() == key)
    }

    // Container.getContainerSize of a container minecart.
    fn container_size(self) -> usize {
        match self {
            Class::Chest => 27,
            Class::Hopper => 5,
            _ => 0,
        }
    }

    // The item a destroyed minecart drops (VehicleEntity.getDropItem).
    fn drop_item(self) -> &'static str {
        match self {
            Class::Minecart => "minecraft:minecart",
            Class::Chest => "minecraft:chest_minecart",
            Class::Hopper => "minecraft:hopper_minecart",
            Class::Furnace => "minecraft:furnace_minecart",
            Class::Tnt => "minecraft:tnt_minecart",
        }
    }
}

// The fields of a minecart: VehicleEntity's hurt time, direction and damage,
// AbstractMinecart's, and the kinds'.
pub struct MinecartData {
    pub class: Class,
    pub vehicle: Vehicle,
    pub on_rails: bool,
    pub flipped: bool,
    pub display_offset: i32,
    // MinecartHopper: enabled, and whether it took an item this tick.
    pub enabled: bool,
    pub consumed_item_this_frame: bool,
    // MinecartFurnace: the fuel, the push, and the synched flag that the
    // fuel is lit (set by its tick).
    pub fuel: i32,
    pub push: Vec3,
    pub has_fuel: bool,
    // MinecartTNT: the fuse (-1 for none), the explosion's power and speed
    // factor, and whether an ignition source (a damage source) is held.
    pub fuse: i32,
    pub explosion_power_base: f32,
    pub explosion_speed_factor: f32,
    pub ignited: bool,
}

impl MinecartData {
    fn new(class: Class) -> MinecartData {
        MinecartData {
            class,
            vehicle: Vehicle::new(),
            on_rails: false,
            flipped: false,
            display_offset: default_display_offset(class),
            enabled: true,
            consumed_item_this_frame: false,
            fuel: 0,
            push: Vec3::ZERO,
            has_fuel: false,
            fuse: -1,
            explosion_power_base: 4.0f32,
            explosion_speed_factor: 1.0f32,
            ignited: false,
        }
    }

    pub fn is_container(&self) -> bool {
        matches!(self.class, Class::Chest | Class::Hopper)
    }
}

// AbstractMinecart.getDefaultDisplayOffset.
fn default_display_offset(class: Class) -> i32 {
    match class {
        Class::Chest => 8,
        Class::Hopper => 1,
        _ => 6,
    }
}

pub fn data(e: &Entity) -> &MinecartData {
    match &e.kind {
        Kind::Minecart(d) => d,
        _ => panic!("not a minecart"),
    }
}

pub fn data_mut(e: &mut Entity) -> &mut MinecartData {
    match &mut e.kind {
        Kind::Minecart(d) => d,
        _ => panic!("not a minecart"),
    }
}

// The minecart's display block state (AbstractMinecart.getDisplayBlockState
// with no custom one).
pub fn display_state(l: &Level, d: &MinecartData) -> State {
    let reg = l.reg;
    match d.class {
        Class::Minecart => l.known.air,
        Class::Chest => reg.default_state(reg.block_key("minecraft:chest")),
        Class::Hopper => reg.default_state(reg.block_key("minecraft:hopper")),
        Class::Furnace => {
            let state = reg.default_state(reg.block_key("minecraft:furnace"));
            reg.with_bool(state, reg.property_of(state, "lit"), d.has_fuel)
        }
        Class::Tnt => reg.default_state(reg.block_key("minecraft:tnt")),
    }
}

// A new minecart of the class (the entity type's constructor): not added to
// the level.
pub fn new_cart(l: &mut Level, class: Class) -> Box<Entity> {
    let t = l.reg.entity_types[class.key()].clone();
    let e = Box::new(Entity::new(l, Kind::Minecart(MinecartData::new(class)), t.width, t.height, t.eye_height));
    if class.container_size() > 0 {
        l.entities.containers.insert(e.id, empty_slots_n(class.container_size()));
    }
    e
}

// Mth.clamp(float, float, float).
fn clamp_f32(value: f32, min: f32, max: f32) -> f32 {
    if value < min {
        min
    } else if value != value {
        value
    } else if value <= max {
        value
    } else {
        max
    }
}

// Entity.load and AbstractMinecart's readAdditionalSaveData, the kind's own,
// for a minecart the constructor made.
pub fn load(l: &mut Level, tag: &Compound, class: Class) -> R<super::load::Loaded> {
    let input = Input(tag);
    let mut e = new_cart(l, class);
    super::load::load_common(l, &mut e, &input, false)?;
    // AbstractMinecart.readAdditionalSaveData.
    if input.get("DisplayState").is_some() {
        return fail("a minecart's display state");
    }
    let offset = input.int_or("DisplayOffset", default_display_offset(class))?;
    data_mut(&mut e).display_offset = offset;
    data_mut(&mut e).flipped = input.bool_or("FlippedRotation", false)?;
    e.first_tick = input.bool_or("HasTicked", false)?;
    if input.get("Paper.FrictionState").is_some() {
        return fail("a minecart's friction state");
    }
    if class.container_size() > 0 {
        // readChestVehicleSaveData: the loot table is unpacked when the
        // contents are first used, which is not modeled.
        if input.get("LootTable").is_some() {
            return fail("a minecart's loot table");
        }
        input.long_or("LootTableSeed", 0)?;
        let mut items = empty_slots_n(class.container_size());
        crate::blocks::load::load_all_items(l.reg, &input, &mut items)?;
        l.entities.containers.insert(e.id, items);
    }
    match class {
        Class::Hopper => data_mut(&mut e).enabled = input.bool_or("Enabled", true)?,
        Class::Furnace => {
            let push_x = input.double_or("PushX", 0.0)?;
            let push_z = input.double_or("PushZ", 0.0)?;
            let d = data_mut(&mut e);
            d.push = Vec3::new(push_x, 0.0, push_z);
            d.fuel = input.short_or("Fuel", 0)? as i32;
        }
        Class::Tnt => {
            let fuse = input.int_or("fuse", -1)?;
            let power = clamp_f32(input.float_or("explosion_power", 4.0f32)?, 0.0f32, 128.0f32);
            let factor = clamp_f32(input.float_or("explosion_speed_factor", 1.0f32)?, 0.0f32, 128.0f32);
            let d = data_mut(&mut e);
            d.fuse = fuse;
            d.explosion_power_base = power;
            d.explosion_speed_factor = factor;
        }
        Class::Minecart | Class::Chest => {}
    }
    let (origin, spawn_reason) = super::load::load_paper(l, &input)?;
    Ok(super::load::Loaded { entity: e, origin, spawn_reason })
}

// addAdditionalSaveData of AbstractMinecart and the kinds.
pub fn save_fields(l: &Level, e: &Entity, out: &mut Compound) -> R<()> {
    let d = data(e);
    if d.display_offset != default_display_offset(d.class) {
        out.put_int("DisplayOffset", d.display_offset);
    }
    out.put_boolean("FlippedRotation", d.flipped);
    out.put_boolean("HasTicked", e.first_tick);
    if d.is_container() {
        // addChestVehicleSaveData: no loot table, and the items.
        container::save_all_items(l.reg, &l.entities.containers[&e.id], true, out);
    }
    match d.class {
        Class::Hopper => out.put_boolean("Enabled", d.enabled),
        Class::Furnace => {
            out.put("PushX", Tag::double(d.push.x));
            out.put("PushZ", Tag::double(d.push.z));
            out.put("Fuel", Tag::Short(d.fuel as i16));
        }
        Class::Tnt => {
            out.put_int("fuse", d.fuse);
            if d.explosion_power_base != 4.0f32 {
                out.put("explosion_power", Tag::float(d.explosion_power_base));
            }
            if d.explosion_speed_factor != 1.0f32 {
                out.put("explosion_speed_factor", Tag::float(d.explosion_speed_factor));
            }
        }
        Class::Minecart | Class::Chest => {}
    }
    out.put_string("id", d.class.key());
    Ok(())
}

// --- Containers ---

// Containers.dropContents(level, this, this): every slot dropped from the
// entity's position (the draws of the level's random come first for a slot,
// even an empty one).
pub fn drop_contents(l: &mut Level, e: &Entity) {
    let stacks: Vec<Stack> = l.entities.containers[&e.id].clone();
    for stack in &stacks {
        item::drop_item_stack(l, e.pos.x, e.pos.y, e.pos.z, stack);
    }
}

// --- Ticking ---

// AbstractMinecart.getMaxSpeed(level): the behaviour's, and the furnace's.
fn max_speed(e: &Entity) -> f64 {
    let base = if e.was_touching_water { 0.2 } else { 0.4 };
    match data(e).class {
        Class::Furnace => {
            if e.was_touching_water {
                base * 0.75
            } else {
                base * 0.5
            }
        }
        _ => base,
    }
}

// AbstractMinecart.getDefaultGravity, then Entity.applyGravity.
fn apply_gravity(e: &mut Entity) {
    let gravity = if e.no_gravity {
        0.0
    } else if e.was_touching_water {
        0.005
    } else {
        0.04
    };
    if gravity != 0.0 {
        e.delta = e.delta.add(0.0, -gravity, 0.0);
    }
}

// AbstractMinecart.getCurrentBlockPosOrRailBelow (old movement).
fn block_pos_or_rail_below(l: &mut Level, e: &Entity) -> Pos {
    let (x, mut y, z) = (phys::floor(e.pos.x), phys::floor(e.pos.y), phys::floor(e.pos.z));
    let below = l.get(Pos::new(x, y - 1, z));
    if l.reg.in_tag(below, "minecraft:rails") {
        y -= 1;
    }
    Pos::new(x, y, z)
}

// AbstractMinecart.EXITS.
fn exits(shape: RailShape) -> ((i32, i32, i32), (i32, i32, i32)) {
    const WEST: (i32, i32, i32) = (-1, 0, 0);
    const EAST: (i32, i32, i32) = (1, 0, 0);
    const NORTH: (i32, i32, i32) = (0, 0, -1);
    const SOUTH: (i32, i32, i32) = (0, 0, 1);
    let below = |v: (i32, i32, i32)| (v.0, v.1 - 1, v.2);
    match shape {
        RailShape::NorthSouth => (NORTH, SOUTH),
        RailShape::EastWest => (WEST, EAST),
        RailShape::AscendingEast => (below(WEST), EAST),
        RailShape::AscendingWest => (WEST, below(EAST)),
        RailShape::AscendingNorth => (NORTH, below(SOUTH)),
        RailShape::AscendingSouth => (below(NORTH), SOUTH),
        RailShape::SouthEast => (SOUTH, EAST),
        RailShape::SouthWest => (SOUTH, WEST),
        RailShape::NorthWest => (NORTH, WEST),
        RailShape::NorthEast => (NORTH, EAST),
    }
}

// OldMinecartBehavior.getPos(x, y, z): the point on the track under the
// position, none if there is no rail.
fn track_point(l: &mut Level, x: f64, y: f64, z: f64) -> Option<Vec3> {
    let (fx, mut fy, fz) = (phys::floor(x), phys::floor(y), phys::floor(z));
    let below = l.get(Pos::new(fx, fy - 1, fz));
    if l.reg.in_tag(below, "minecraft:rails") {
        fy -= 1;
    }
    let state = l.get(Pos::new(fx, fy, fz));
    if !crate::blocks::is_rail(l, state) {
        return None;
    }
    let shape = rail::shape(l, state);
    let (v1, v2) = exits(shape);
    let d = fx as f64 + 0.5 + v1.0 as f64 * 0.5;
    let d1 = fy as f64 + 0.0625 + v1.1 as f64 * 0.5;
    let d2 = fz as f64 + 0.5 + v1.2 as f64 * 0.5;
    let d3 = fx as f64 + 0.5 + v2.0 as f64 * 0.5;
    let d4 = fy as f64 + 0.0625 + v2.1 as f64 * 0.5;
    let d5 = fz as f64 + 0.5 + v2.2 as f64 * 0.5;
    let d6 = d3 - d;
    let d7 = (d4 - d1) * 2.0;
    let d8 = d5 - d2;
    let d9 = if d6 == 0.0 {
        z - fz as f64
    } else if d8 == 0.0 {
        x - fx as f64
    } else {
        let d10 = x - d;
        let d11 = z - d2;
        (d10 * d6 + d11 * d8) * 2.0
    };
    let x = d + d6 * d9;
    let mut y = d1 + d7 * d9;
    let z = d2 + d8 * d9;
    if d7 < 0.0 {
        y += 1.0;
    } else if d7 > 0.0 {
        y += 0.5;
    }
    Some(Vec3::new(x, y, z))
}

// AbstractMinecart.isRedstoneConductor(pos).
fn is_redstone_conductor(l: &mut Level, pos: Pos) -> bool {
    let state = l.get(pos);
    l.is_conductor(state)
}

// AbstractMinecart.applyNaturalSlowdown(speed).
fn apply_natural_slowdown(l: &mut Level, e: &mut Entity, speed: Vec3) -> Vec3 {
    match data(e).class {
        Class::Chest | Class::Hopper => {
            // AbstractMinecartContainer: no loot table here.
            let mut f = 0.98f32;
            let signal = container::redstone_signal(l, &container::Cont::Entity(e.id));
            let i = 15 - signal;
            f += i as f32 * 0.001f32;
            if e.was_touching_water {
                f *= 0.95f32;
            }
            speed.multiply(f as f64, 0.0, f as f64)
        }
        Class::Furnace => {
            let d = data_mut(e);
            let vec3 = if d.push.length_sqr() > 1.0E-7 {
                // calculateNewPushAlong(speed).
                let push = d.push;
                d.push = if push.horizontal_distance_sqr() > 1.0E-4 && speed.horizontal_distance_sqr() > 0.001 {
                    push.projected_on(speed).normalize().scale(push.length())
                } else {
                    push
                };
                let mut v = speed.multiply(0.8, 0.0, 0.8).add_vec(d.push);
                if e.was_touching_water {
                    v = v.scale(0.1);
                }
                v
            } else {
                speed.multiply(0.98, 0.0, 0.98)
            };
            slowdown(e, vec3)
        }
        Class::Minecart | Class::Tnt => slowdown(e, speed),
    }
}

// AbstractMinecart.applyNaturalSlowdown: the behaviour's slowdown factor
// (a minecart without a rider and with slowWhenEmpty), and the water's.
fn slowdown(e: &Entity, speed: Vec3) -> Vec3 {
    let factor = 0.96;
    let mut vec3 = speed.multiply(factor, 0.0, factor);
    if e.was_touching_water {
        vec3 = vec3.scale(0.95f32 as f64);
    }
    vec3
}

// AbstractMinecart.move(SELF, movement) with the old behaviour: Entity.move,
// then the effects of the blocks it is in.
fn cart_move(l: &mut Level, e: &mut Entity, movement: Vec3) {
    motion::move_entity(l, e, movement);
    apply_effects_at_position(l, e);
}

// AbstractMinecart.applyEffectsFromBlocks() with the old behaviour:
// applyEffectsFromBlocks(position(), position()), then the movements are
// cleared.
fn apply_effects_at_position(l: &mut Level, e: &mut Entity) {
    let movements = vec![super::Movement { from: e.pos, to: e.pos, axis_dependent_original: None }];
    motion::apply_effects_for_movements(l, e, &movements);
    e.movement_this_tick.clear();
}

// OldMinecartBehavior.moveAlongTrack.
fn move_along_track(l: &mut Level, e: &mut Entity) {
    let rail_pos = block_pos_or_rail_below(l, e);
    let state = l.get(rail_pos);
    e.fall_distance = 0.0;
    let (mut x, mut y, mut z) = (e.pos.x, e.pos.y, e.pos.z);
    let pos = track_point(l, x, y, z);
    y = rail_pos.y() as f64;
    let mut powered = false;
    let mut unpowered = false;
    if l.reg.blocks[l.block(state) as usize].key == "minecraft:powered_rail" {
        powered = rail::powered(l, state);
        unpowered = !powered;
    }
    let mut d = 0.0078125;
    if e.was_touching_water {
        d *= 0.2;
    }
    let mut delta = e.delta;
    let shape = rail::shape(l, state);
    match shape {
        RailShape::AscendingEast => {
            e.delta = delta.add(-d, 0.0, 0.0);
            y += 1.0;
        }
        RailShape::AscendingWest => {
            e.delta = delta.add(d, 0.0, 0.0);
            y += 1.0;
        }
        RailShape::AscendingNorth => {
            e.delta = delta.add(0.0, 0.0, d);
            y += 1.0;
        }
        RailShape::AscendingSouth => {
            e.delta = delta.add(0.0, 0.0, -d);
            y += 1.0;
        }
        _ => {}
    }
    delta = e.delta;
    let (v1, v2) = exits(shape);
    let mut d1 = (v2.0 - v1.0) as f64;
    let mut d2 = (v2.2 - v1.2) as f64;
    let square_root = (d1 * d1 + d2 * d2).sqrt();
    let d3 = delta.x * d1 + delta.z * d2;
    if d3 < 0.0 {
        d1 = -d1;
        d2 = -d2;
    }
    let min = jmin(2.0, delta.horizontal_distance());
    delta = Vec3::new(min * d1 / square_root, delta.y, min * d2 / square_root);
    e.delta = delta;
    // (No passenger steers the minecart.)
    if unpowered {
        let d5 = e.delta.horizontal_distance();
        if d5 < 0.03 {
            e.delta = Vec3::ZERO;
        } else {
            e.delta = e.delta.multiply(0.5, 0.0, 0.5);
        }
    }
    let d5 = rail_pos.x() as f64 + 0.5 + v1.0 as f64 * 0.5;
    let d6 = rail_pos.z() as f64 + 0.5 + v1.2 as f64 * 0.5;
    let d7 = rail_pos.x() as f64 + 0.5 + v2.0 as f64 * 0.5;
    let d8 = rail_pos.z() as f64 + 0.5 + v2.2 as f64 * 0.5;
    d1 = d7 - d5;
    d2 = d8 - d6;
    let d9 = if d1 == 0.0 {
        z - rail_pos.z() as f64
    } else if d2 == 0.0 {
        x - rail_pos.x() as f64
    } else {
        let d10 = x - d5;
        let d11 = z - d6;
        (d10 * d1 + d11 * d2) * 2.0
    };
    x = d5 + d1 * d9;
    z = d6 + d2 * d9;
    set_pos(l, e, x, y, z);
    // isVehicle() is false: no rider slows it.
    let d10 = 1.0;
    let d11 = max_speed(e);
    delta = e.delta;
    cart_move(l, e, Vec3::new(clamp(d10 * delta.x, -d11, d11), 0.0, clamp(d10 * delta.z, -d11, d11)));
    if v1.1 != 0 && phys::floor(e.pos.x) - rail_pos.x() == v1.0 && phys::floor(e.pos.z) - rail_pos.z() == v1.2 {
        set_pos(l, e, e.pos.x, e.pos.y + v1.1 as f64, e.pos.z);
    } else if v2.1 != 0 && phys::floor(e.pos.x) - rail_pos.x() == v2.0 && phys::floor(e.pos.z) - rail_pos.z() == v2.2 {
        set_pos(l, e, e.pos.x, e.pos.y + v2.1 as f64, e.pos.z);
    }
    e.delta = apply_natural_slowdown(l, e, e.delta);
    let pos1 = track_point(l, e.pos.x, e.pos.y, e.pos.z);
    if let (Some(pos1), Some(pos)) = (pos1, pos) {
        let d12 = (pos.y - pos1.y) * 0.05;
        let delta1 = e.delta;
        let d13 = delta1.horizontal_distance();
        if d13 > 0.0 {
            e.delta = delta1.multiply((d13 + d12) / d13, 1.0, (d13 + d12) / d13);
        }
        set_pos(l, e, e.pos.x, pos1.y, e.pos.z);
    }
    let floor = phys::floor(e.pos.x);
    let floor1 = phys::floor(e.pos.z);
    if floor != rail_pos.x() || floor1 != rail_pos.z() {
        let delta1 = e.delta;
        let d13 = delta1.horizontal_distance();
        e.delta = Vec3::new(d13 * (floor - rail_pos.x()) as f64, delta1.y, d13 * (floor1 - rail_pos.z()) as f64);
    }
    if powered {
        let delta1 = e.delta;
        let d13 = delta1.horizontal_distance();
        if d13 > 0.01 {
            e.delta = delta1.add(delta1.x / d13 * 0.06, 0.0, delta1.z / d13 * 0.06);
        } else {
            let delta2 = e.delta;
            let mut d15 = delta2.x;
            let mut d16 = delta2.z;
            if shape == RailShape::EastWest {
                if is_redstone_conductor(l, rail_pos.relative(crate::dir::Dir::West)) {
                    d15 = 0.02;
                } else if is_redstone_conductor(l, rail_pos.relative(crate::dir::Dir::East)) {
                    d15 = -0.02;
                }
            } else {
                if shape != RailShape::NorthSouth {
                    return;
                }
                if is_redstone_conductor(l, rail_pos.relative(crate::dir::Dir::North)) {
                    d16 = 0.02;
                } else if is_redstone_conductor(l, rail_pos.relative(crate::dir::Dir::South)) {
                    d16 = -0.02;
                }
            }
            e.delta = Vec3::new(d15, delta2.y, d16);
        }
    }
}

// AbstractMinecart.comeOffTrack.
fn come_off_track(l: &mut Level, e: &mut Entity) {
    let max_speed = max_speed(e);
    let delta = e.delta;
    e.delta = Vec3::new(clamp(delta.x, -max_speed, max_speed), delta.y, clamp(delta.z, -max_speed, max_speed));
    if e.on_ground {
        // derailedX, Y, Z: 0.5.
        e.delta = Vec3::new(e.delta.x * 0.5, e.delta.y * 0.5, e.delta.z * 0.5);
    }
    cart_move(l, e, e.delta);
    if !e.on_ground {
        // flyingX, Y, Z: 0.95.
        e.delta = Vec3::new(e.delta.x * 0.95, e.delta.y * 0.95, e.delta.z * 0.95);
    }
}

// AbstractMinecart.activateMinecart(x, y, z, receivingPower).
fn activate(l: &mut Level, e: &mut Entity, receiving_power: bool) {
    match data(e).class {
        Class::Minecart => {
            if receiving_power && data(e).vehicle.hurt_time == 0 {
                let v = &mut data_mut(e).vehicle;
                v.hurt_dir = -v.hurt_dir;
                v.hurt_time = 10;
                v.damage = 50.0f32;
            }
        }
        Class::Hopper => {
            let flag = !receiving_power;
            if flag != data(e).enabled {
                data_mut(e).enabled = flag;
            }
        }
        Class::Tnt => {
            if receiving_power && data(e).fuse < 0 {
                prime_fuse(l, e, false);
            }
        }
        Class::Chest | Class::Furnace => {}
    }
}

// OldMinecartBehavior.tick, on a server.
fn behaviour_tick(l: &mut Level, e: &mut Entity) {
    apply_gravity(e);
    let rail_pos = block_pos_or_rail_below(l, e);
    let state = l.get(rail_pos);
    let is_rail = crate::blocks::is_rail(l, state);
    data_mut(e).on_rails = is_rail;
    if is_rail {
        move_along_track(l, e);
        if l.reg.blocks[l.block(state) as usize].key == "minecraft:activator_rail" {
            activate(l, e, rail::powered(l, state));
        }
    } else {
        come_off_track(l, e);
    }
    apply_effects_at_position(l, e);
    e.x_rot = 0.0f32;
    let d = e.xo - e.pos.x;
    let d1 = e.zo - e.pos.z;
    if d * d + d1 * d1 > 0.001 {
        e.y_rot = (crate::mth::atan2(l.reg, d1, d) * 180.0 / std::f64::consts::PI) as f32;
        if data(e).flipped {
            e.y_rot += 180.0f32;
        }
    }
    let d2 = crate::mth::wrap_degrees(e.y_rot - e.y_rot_o) as f64;
    if d2 < -170.0 || d2 >= 170.0 {
        e.y_rot += 180.0f32;
        let d = data_mut(e);
        d.flipped = !d.flipped;
    }
    e.x_rot = e.x_rot % 360.0f32;
    e.y_rot = e.y_rot % 360.0f32;
    push_and_pickup_entities(l, e);
}

// OldMinecartBehavior.pushAndPickupEntities (the minecart has no rider).
fn push_and_pickup_entities(l: &mut Level, e: &mut Entity) {
    let aabb = e.bb.inflate(0.2f32 as f64, 0.0, 0.2f32 as f64);
    let rideable = data(e).class == Class::Minecart;
    if rideable && e.delta.horizontal_distance_sqr() >= 0.01 {
        // EntitySelector.pushableBy(minecart): the other entities that are
        // collidable and pushable.
        let others = super::entities_in(l, Some(e), Some(e.id), Only::Any, &aabb, |o| super::collide::is_collidable(o) && super::collide::is_pushable(o));
        for id in others {
            let is_cart = matches!(l.entities.all[&id].kind, Kind::Minecart(_));
            if !is_cart {
                // entity.startRiding(minecart): not modeled.
                refuse("an entity pushed onto a minecart boards it");
            }
            let mut other = l.entities.all.remove(&id).expect("a pushable minecart");
            push_cart(l, &mut other, e);
            if !other.removed {
                l.entities.all.insert(id, other);
            }
        }
    } else {
        let others = super::entities_in(l, Some(e), Some(e.id), Only::Carts, &aabb, |o| super::collide::is_pushable(o));
        for id in others {
            let mut other = l.entities.all.remove(&id).expect("a pushable minecart");
            push_cart(l, &mut other, e);
            if !other.removed {
                l.entities.all.insert(id, other);
            }
        }
    }
}

// AbstractMinecart.push(entity): `this` is pushed by `entity`.
pub fn push_cart(l: &mut Level, this: &mut Entity, entity: &mut Entity) {
    // (A level on the server; the world configuration lets vehicles collide.)
    if entity.no_physics || this.no_physics {
        return;
    }
    let mut d = entity.pos.x - this.pos.x;
    let mut d1 = entity.pos.z - this.pos.z;
    let mut d2 = d * d + d1 * d1;
    if d2 >= 1.0E-4f32 as f64 {
        d2 = d2.sqrt();
        d /= d2;
        d1 /= d2;
        let mut d3 = 1.0 / d2;
        if d3 > 1.0 {
            d3 = 1.0;
        }
        d *= d3;
        d1 *= d3;
        d *= 0.1f32 as f64;
        d1 *= 0.1f32 as f64;
        d *= 0.5;
        d1 *= 0.5;
        if matches!(entity.kind, Kind::Minecart(_)) {
            push_other_minecart(this, entity, d, d1, l);
        } else {
            push(this, -d, 0.0, -d1);
            push(entity, d / 4.0, 0.0, d1 / 4.0);
        }
    }
}

// AbstractMinecart.pushOtherMinecart (old movement).
fn push_other_minecart(this: &mut Entity, other: &mut Entity, delta_x: f64, delta_z: f64, l: &Level) {
    let _ = l;
    let d = other.pos.x - this.pos.x;
    let d1 = other.pos.z - this.pos.z;
    let vec3 = Vec3::new(d, 0.0, d1).normalize();
    let rad = (std::f64::consts::PI / 180.0) as f32;
    let vec31 = Vec3::new(crate::mth::cos(this.y_rot * rad) as f64, 0.0, crate::mth::sin(this.y_rot * rad) as f64).normalize();
    let abs = vec3.dot(vec31).abs();
    if !(abs < 0.8f32 as f64) {
        let delta = this.delta;
        let delta1 = other.delta;
        let other_furnace = data(other).class == Class::Furnace;
        let this_furnace = data(this).class == Class::Furnace;
        if other_furnace && !this_furnace {
            this.delta = delta.multiply(0.2, 1.0, 0.2);
            push(this, delta1.x - delta_x, 0.0, delta1.z - delta_z);
            other.delta = delta1.multiply(0.95, 1.0, 0.95);
        } else if !other_furnace && this_furnace {
            other.delta = delta1.multiply(0.2, 1.0, 0.2);
            push(other, delta.x + delta_x, 0.0, delta.z + delta_z);
            this.delta = delta.multiply(0.95, 1.0, 0.95);
        } else {
            let d2 = (delta1.x + delta.x) / 2.0;
            let d3 = (delta1.z + delta.z) / 2.0;
            this.delta = delta.multiply(0.2, 1.0, 0.2);
            push(this, d2 - delta_x, 0.0, d3 - delta_z);
            other.delta = delta1.multiply(0.2, 1.0, 0.2);
            push(other, d2 + delta_x, 0.0, d3 + delta_z);
        }
    }
}

// AbstractMinecart.tick.
fn abstract_tick(l: &mut Level, e: &mut Entity) {
    data_mut(e).vehicle.wear_down();
    // checkBelowWorld: void damage is on, 64 blocks under the world.
    if e.pos.y < (crate::world::MIN_Y as f64) + -64.0 {
        remove(l, e);
    }
    behaviour_tick(l, e);
    fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    if fluid::is_in_lava(e) {
        // lavaIgnite(), lavaHurt().
        let immune = item::fire_immune(l, e);
        fluid::lava_ignite(e, immune);
        lava_hurt(l, e);
        e.fall_distance *= 0.5;
    }
    e.first_tick = false;
}

pub fn tick(l: &mut Level, e: &mut Entity) {
    match data(e).class {
        Class::Hopper => {
            data_mut(e).consumed_item_this_frame = false;
            abstract_tick(l, e);
            try_consume_items(l, e);
        }
        Class::Furnace => {
            abstract_tick(l, e);
            // The server's part of MinecartFurnace.tick.
            let d = data_mut(e);
            if d.fuel > 0 {
                d.fuel -= 1;
            }
            if d.fuel <= 0 {
                d.push = Vec3::ZERO;
            }
            d.has_fuel = d.fuel > 0;
            if d.has_fuel {
                // random.nextInt(4) == 0 spawns a smoke particle.
                l.shared_random.next_int_bound(4);
            }
        }
        Class::Tnt => {
            abstract_tick(l, e);
            tnt_tick(l, e);
        }
        Class::Minecart | Class::Chest => abstract_tick(l, e),
    }
}

// --- Fire and damage ---

// Entity.lavaHurt(): the lava damage of 4.0, and its sound's pitch.
fn lava_hurt(l: &mut Level, e: &mut Entity) {
    if item::fire_immune(l, e) {
        return;
    }
    if vehicle::hurt(l, e, Dmg::Lava, 4.0f32) {
        // serverLevel.playSound(..., 2.0F + random.nextFloat() * 0.4F), unless silent.
        if !is_silent(e) {
            l.shared_random.next_float();
        }
    }
}

// Entity.isSilent: the saved Silent flag.
pub fn is_silent(e: &Entity) -> bool {
    e.extra.get("Silent").is_some()
}

pub fn lava_hurt_entry(l: &mut Level, e: &mut Entity, _pos: Pos) {
    lava_hurt(l, e);
}

// MinecartTNT.shouldSourceDestroy: damageSourceIgnitesTnt.
pub fn should_source_destroy(class: Class, dmg: Dmg) -> bool {
    class == Class::Tnt && (dmg.is_fire() || dmg.is_explosion())
}

// AbstractMinecart destroy(level, damageSource) of the classes.
pub fn destroy(l: &mut Level, e: &mut Entity, dmg: Dmg) {
    match data(e).class {
        Class::Tnt => {
            // MinecartTNT.destroy.
            let d = e.delta.horizontal_distance_sqr();
            let ignites = dmg.is_fire() || dmg.is_explosion();
            if !ignites && !(d >= 0.01f32 as f64) {
                destroy_with_item(l, e);
            } else if data(e).fuse < 0 {
                prime_fuse(l, e, true);
                let fuse = l.shared_random.next_int_bound(20) + l.shared_random.next_int_bound(20);
                data_mut(e).fuse = fuse;
            }
        }
        Class::Chest | Class::Hopper => {
            // AbstractMinecartContainer.destroy: the minecart, then
            // chestVehicleDestroyed drops the contents (again).
            destroy_with_item(l, e);
            drop_contents(l, e);
        }
        Class::Minecart | Class::Furnace => destroy_with_item(l, e),
    }
}

// --- Hopper minecart ---

// MinecartHopper.tryConsumeItems.
fn try_consume_items(l: &mut Level, e: &mut Entity) {
    if !e.removed && data(e).enabled && !data(e).consumed_item_this_frame && suck_in_items(l, e) {
        data_mut(e).consumed_item_this_frame = true;
    }
}

// MinecartHopper.suckInItems.
fn suck_in_items(l: &mut Level, e: &mut Entity) -> bool {
    if crate::blocks::hopper::suck_in_items_cart(l, e) {
        return true;
    }
    let aabb = e.bb.inflate(0.25, 0.0, 0.25);
    for id in super::item_entities_in(l, Some(e), &aabb, |o| !o.removed) {
        let Some(mut o) = l.entities.all.remove(&id) else { refuse("an item entity left the level during a hopper minecart's pickup") };
        let picked = crate::blocks::hopper::add_item_entity_to_cart(l, e, &mut o);
        if !o.removed {
            l.entities.all.insert(id, o);
        }
        if picked {
            return true;
        }
    }
    false
}

// --- TNT minecart ---

// MinecartTNT.primeFuse(damageSource): the fuse is lit, and a damage source
// is held the first time.
fn prime_fuse(_l: &mut Level, e: &mut Entity, with_source: bool) {
    // The game rule tntExplodes is on.
    let d = data_mut(e);
    d.fuse = 80;
    if with_source && !d.ignited {
        d.ignited = true;
    }
}

// MinecartTNT.tick, after AbstractMinecart's.
fn tnt_tick(l: &mut Level, e: &mut Entity) {
    let fuse = data(e).fuse;
    if fuse > 0 {
        data_mut(e).fuse -= 1;
    } else if fuse == 0 {
        explode(l, e, e.delta.horizontal_distance_sqr());
    }
    if e.horizontal_collision {
        let d = e.delta.horizontal_distance_sqr();
        if d >= 0.01f32 as f64 {
            explode(l, e, d);
        }
    }
}

// MinecartTNT.explode(damageSource, radiusModifier).
pub fn explode(l: &mut Level, e: &mut Entity, radius_modifier: f64) {
    let min = jmin(radius_modifier.sqrt(), 5.0);
    let d = data(e);
    let radius = (d.explosion_power_base as f64 + d.explosion_speed_factor as f64 * l.shared_random.next_double() * 1.5 * min) as f32;
    crate::explosion::explode(l, e, e.pos, radius, false);
    remove(l, e);
}

// MinecartTNT.causeFallDamage.
pub fn cause_fall_damage(l: &mut Level, e: &mut Entity, fall_distance: f64) {
    if data(e).class == Class::Tnt && fall_distance >= 3.0 {
        let d = fall_distance / 10.0;
        explode(l, e, d * d);
    }
}

// VehicleEntity.destroy(level, getDropItem()).
fn destroy_with_item(l: &mut Level, e: &mut Entity) {
    let key = data(e).class.drop_item();
    vehicle::destroy_with_item(l, e, key);
}
