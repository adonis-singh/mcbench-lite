// Entity loading and saving for the entities the verifier imports:
// EntityType.loadEntityRecursive (Entity.load and the entity's own
// readAdditionalSaveData) and Entity.saveAsPassenger, which the verifier
// writes into the report as the import's audit. What saved data sets that
// nothing here acts on is kept as the tags it saves back (Entity.extra);
// data this port does not model fails (the caller refuses).

use super::{item, Entity};
use crate::codec::{self, fail, items, Input, R};
use crate::components;
use crate::level::Level;
use crate::loot::ItemStack;
use crate::nbt::{Compound, Tag};
use crate::phys::Vec3;

// Codec.DOUBLE.listOf() of exactly three, and Vec3.CODEC.
fn vec3(tag: &Tag) -> R<Vec3> {
    let list = items(tag)?;
    if list.len() != 3 {
        return fail(format!("List must have 3 elements, but it had {}", list.len()));
    }
    Ok(Vec3::new(codec::double(&list[0])?, codec::double(&list[1])?, codec::double(&list[2])?))
}

// Vec2.CODEC: two floats.
fn vec2(tag: &Tag) -> R<(f32, f32)> {
    let list = items(tag)?;
    if list.len() != 2 {
        return fail(format!("List must have 2 elements, but it had {}", list.len()));
    }
    Ok((codec::float(&list[0])?, codec::float(&list[1])?))
}

// UUIDUtil.CODEC: four ints, as an int array or a list of numbers.
pub fn uuid(tag: &Tag) -> R<(i64, i64)> {
    let ints: Vec<i32> = match tag {
        Tag::IntArray(a) => a.clone(),
        other => items(other)?.iter().map(|t| t.box_int().ok_or_else(|| codec::Fail("Some elements are not numbers".into()))).collect::<R<Vec<i32>>>()?,
    };
    if ints.len() != 4 {
        return fail(format!("Expected uuid array of 4, but got {}", ints.len()));
    }
    Ok(((((ints[0] as i64) << 32) | (ints[1] as u32 as i64)), (((ints[2] as i64) << 32) | (ints[3] as u32 as i64))))
}

pub fn uuid_tag((hi, lo): (i64, i64)) -> Tag {
    Tag::IntArray(vec![(hi >> 32) as i32, hi as i32, (lo >> 32) as i32, lo as i32])
}

// MCUtil.sanitizeNanInf.
fn sanitize(v: f64) -> f64 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

// Mth.clamp(double, min, max).
fn clamp(v: f64, min: f64, max: f64) -> f64 {
    if v < min {
        min
    } else {
        v.min(max)
    }
}

// Paper.SpawnReason names (CreatureSpawnEvent.SpawnReason).
const SPAWN_REASONS: [&str; 46] = [
    "NATURAL", "JOCKEY", "CHUNK_GEN", "SPAWNER", "TRIAL_SPAWNER", "EGG", "SPAWNER_EGG", "LIGHTNING", "BUILD_SNOWMAN", "BUILD_IRONGOLEM", "BUILD_COPPERGOLEM", "BUILD_WITHER", "VILLAGE_DEFENSE",
    "VILLAGE_INVASION", "BREEDING", "SLIME_SPLIT", "REINFORCEMENTS", "NETHER_PORTAL", "DISPENSE_EGG", "INFECTION", "CURED", "OCELOT_BABY", "SILVERFISH_BLOCK", "MOUNT", "TRAP", "ENDER_PEARL",
    "SHOULDER_ENTITY", "DROWNED", "SHEARED", "EXPLOSION", "RAID", "PATROL", "BEEHIVE", "PIGLIN_ZOMBIFIED", "SPELL", "FROZEN", "METAMORPHOSIS", "DUPLICATION", "COMMAND", "ENCHANTMENT",
    "OMINOUS_ITEM_SPAWNER", "BUCKET", "POTION_EFFECT", "REANIMATE", "REHYDRATION", "CUSTOM",
];

// An entity loaded, with the Paper fields it saves back (Paper.Origin and
// Paper.OriginWorld, the spawn reason).
pub struct Loaded {
    pub entity: Box<Entity>,
    // Paper.Origin and Paper.OriginWorld: as the tag gave them, or as the
    // level sets them when the entity is added.
    pub origin: Option<(Vec3, (i64, i64))>,
    pub spawn_reason: &'static str,
}

// Entity.load for the fields every kind has.
pub fn load_common(l: &mut Level, e: &mut Entity, input: &Input, frame: bool) -> R<()> {
    let pos = input.read("Pos", vec3)?.unwrap_or(Vec3::ZERO);
    let motion = input.read("Motion", vec3)?.unwrap_or(Vec3::ZERO);
    let motion = Vec3::new(sanitize(motion.x), sanitize(motion.y), sanitize(motion.z));
    let (rot_y, rot_x) = input.read("Rotation", vec2)?.unwrap_or((0.0, 0.0));
    let limit = |v: f64| if v.abs() > 10.0 { 0.0 } else { v };
    e.delta = Vec3::new(limit(motion.x), limit(motion.y), limit(motion.z));
    e.has_impulse = true;
    let (x, y, z) = (clamp(pos.x, -3.0000512E7, 3.0000512E7), clamp(pos.y, -2.0E7, 2.0E7), clamp(pos.z, -3.0000512E7, 3.0000512E7));
    if frame {
        super::frame::set_raw(l, e, x, y, z);
    } else {
        super::motion::set_pos(l, e, x, y, z);
    }
    // setYRot and setXRot ignore a rotation that is not finite.
    if rot_y.is_finite() {
        e.y_rot = rot_y;
    }
    if rot_x.is_finite() {
        e.x_rot = (rot_x % 360.0f32).clamp(-90.0f32, 90.0f32);
    }
    e.set_old_pos_and_rot();
    e.fall_distance = input.double_or("fall_distance", 0.0)?;
    e.remaining_fire_ticks = input.short_or("Fire", 0)? as i32;
    e.air_supply = input.int_or("Air", 300)?;
    e.on_ground = input.bool_or("OnGround", false)?;
    e.invulnerable = input.bool_or("Invulnerable", false)?;
    e.portal_cooldown = input.int_or("PortalCooldown", 0)?;
    if let Some(uuid) = input.read("UUID", uuid)? {
        e.uuid = uuid;
    }
    if !(e.pos.x.is_finite() && e.pos.y.is_finite() && e.pos.z.is_finite()) || !e.y_rot.is_finite() || !e.x_rot.is_finite() {
        return fail("Entity has invalid position or rotation");
    }
    // reapplyPosition, then setRot (both angles taken modulo 360).
    let p = e.pos;
    if frame {
        super::frame::reapply_position(l, e);
    } else {
        super::motion::set_pos(l, e, p.x, p.y, p.z);
    }
    e.y_rot = e.y_rot % 360.0f32;
    e.x_rot = (e.x_rot % 360.0f32).clamp(-90.0f32, 90.0f32);
    if let Some(name) = input.read("CustomName", |tag| crate::text::component(l.reg, tag))? {
        e.extra.put("CustomName", name);
    }
    for (key, saved) in [("CustomNameVisible", "CustomNameVisible"), ("Silent", "Silent"), ("NoGravity", "NoGravity"), ("Glowing", "Glowing")] {
        if input.bool_or(key, false)? {
            e.extra.put_boolean(saved, true);
            if key == "NoGravity" {
                e.no_gravity = true;
            }
        }
    }
    e.ticks_frozen = input.int_or("TicksFrozen", 0)?;
    if input.get("Paper.FireOverride").is_some() || input.get("HasVisualFire").is_some() {
        return fail("an entity's visual fire");
    }
    if input.get("data").is_some() {
        return fail("an entity's custom data");
    }
    match input.get("Tags") {
        None => {}
        Some(Tag::List(tags)) if tags.is_empty() => {}
        Some(_) => return fail("an entity's tags"),
    }
    Ok(())
}

// The Bukkit and Paper fields Entity.load reads after the kind's own: only
// their defaults are modeled.
pub fn load_paper(l: &Level, input: &Input) -> R<(Option<(Vec3, (i64, i64))>, &'static str)> {
    if !input.bool_or("Bukkit.persist", true)? || !input.bool_or("Bukkit.visibleByDefault", true)? || input.int_or("Bukkit.MaxAirSupply", 300)? != 300 {
        return fail("an entity's Bukkit settings");
    }
    if input.get("PublicBukkitValues").is_some() || input.get("BukkitValues").is_some() {
        return fail("an entity's Bukkit persistent data");
    }
    if let Some(t) = input.get("Bukkit.invisible") {
        if codec::boolean(t)? {
            return fail("an invisible entity (Bukkit)");
        }
    }
    let origin = match input.read("Paper.Origin", vec3)? {
        Some(origin) => {
            let world = input.read("Paper.OriginWorld", uuid)?.unwrap_or(l.world_uuid);
            Some((origin, world))
        }
        None => None,
    };
    if input.bool_or("Paper.FromMobSpawner", false)? || input.bool_or("Paper.FromNetherPortal", false)? || input.bool_or("Paper.FreezeLock", false)? {
        return fail("an entity's Paper flags");
    }
    let reason = match input.string("Paper.SpawnReason")? {
        Some(name) => match SPAWN_REASONS.iter().find(|r| **r == name) {
            Some(r) => r,
            // An unknown name is logged and the reason is DEFAULT.
            None => "DEFAULT",
        },
        None => "DEFAULT",
    };
    Ok((origin, reason))
}

// EntityType.loadEntityRecursive for an entity the verifier imports (an item
// or an item frame; without passengers).
pub fn load_entity(l: &mut Level, id: &str, tag: &Compound) -> R<Loaded> {
    match id {
        "minecraft:item" => load_item(l, tag),
        "minecraft:item_frame" => super::frame::load(l, tag, false),
        "minecraft:glow_item_frame" => super::frame::load(l, tag, true),
        "minecraft:painting" => super::painting::load(l, tag),
        "minecraft:armor_stand" => super::armor_stand::load(l, tag),
        "minecraft:experience_orb" => super::orb::load(l, tag),
        other => match super::minecart::Class::by_key(other) {
            Some(class) => super::minecart::load(l, tag, class),
            None if super::boat::is_boat_type(other) && l.reg.entity_types.contains_key(other) => super::boat::load(l, tag, other),
            None => fail(format!("the entity {other}")),
        },
    }
}

// EntityType.loadEntityRecursive for an item entity (without passengers).
pub fn load_item(l: &mut Level, tag: &Compound) -> R<Loaded> {
    let input = Input(tag);
    let mut e = item::blank_item(l);
    load_common(l, &mut e, &input, false)?;
    // ItemEntity.readAdditionalSaveData.
    let health = input.short_or("Health", 5)? as i32;
    let age = input.short_or("Age", 0)? as i32;
    let pickup_delay = input.short_or("PickupDelay", 0)? as i32;
    let target = input.read("Owner", uuid)?;
    if input.get("Thrower").is_some() {
        return fail("an item thrown by an entity");
    }
    let stack = input.read("Item", |t| components::decode_stack(l.reg, t))?;
    if input.get("Paper.FrictionState").is_some() {
        return fail("an item's friction state");
    }
    let empty = stack.is_none();
    item::set_loaded(&mut e, stack.unwrap_or_else(ItemStack::empty), health, age, pickup_delay, target);
    if empty {
        // discard(null).
        super::remove(l, &mut e);
    }
    let (origin, spawn_reason) = load_paper(l, &input)?;
    Ok(Loaded { entity: e, origin, spawn_reason })
}

// Entity.saveAsPassenger for an entity that was added to the level: its
// saved data (None if it does not save: removed for good, or not persistent).
pub fn save_entity(l: &Level, e: &Entity, origin: Option<(Vec3, (i64, i64))>, spawn_reason: &str) -> R<Option<Compound>> {
    if e.removed {
        return Ok(None);
    }
    let mut out = Compound::new();
    save_common(l, e, origin, spawn_reason, &mut out);
    match &e.kind {
        super::Kind::Item(data) => {
            out.put("Health", Tag::Short(data.health as i16));
            out.put("Age", Tag::Short(data.age as i16));
            out.put("PickupDelay", Tag::Short(data.pickup_delay as i16));
            if let Some(target) = data.target {
                out.put("Owner", uuid_tag(target));
            }
            if data.stack.count > 0 {
                out.put("Item", components::encode_stack(l.reg, &data.stack)?);
            }
            out.put_string("id", "minecraft:item");
        }
        super::Kind::Frame(_) => super::frame::save_fields(l, e, &mut out)?,
        super::Kind::Minecart(_) => super::minecart::save_fields(l, e, &mut out)?,
        super::Kind::Boat(_) => super::boat::save_fields(l, e, &mut out)?,
        super::Kind::Painting(_) => super::painting::save_fields(l, e, &mut out)?,
        super::Kind::ArmorStand(_) => super::armor_stand::save_fields(l, e, &mut out)?,
        super::Kind::Orb(_) => super::orb::save_fields(e, &mut out),
        _ => return fail("saving an entity of this kind"),
    }
    Ok(Some(out))
}

// Entity.saveWithoutId's common fields (with includeAll).
fn save_common(l: &Level, e: &Entity, origin: Option<(Vec3, (i64, i64))>, spawn_reason: &str, out: &mut Compound) {
    out.put("Pos", Tag::List(vec![Tag::double(e.pos.x), Tag::double(e.pos.y), Tag::double(e.pos.z)]));
    // MCUtil.sanitizeNanInf on the motion; the motion is finite here.
    out.put("Motion", Tag::List(vec![Tag::double(e.delta.x), Tag::double(e.delta.y), Tag::double(e.delta.z)]));
    out.put("Rotation", Tag::List(vec![Tag::float(e.y_rot), Tag::float(e.x_rot)]));
    out.put("fall_distance", Tag::double(e.fall_distance));
    out.put("Fire", Tag::Short(e.remaining_fire_ticks as i16));
    if e.ticks_frozen > 0 {
        out.put_int("TicksFrozen", e.ticks_frozen);
    }
    out.put("Air", Tag::Short(e.air_supply as i16));
    out.put_boolean("OnGround", e.on_ground);
    out.put_boolean("Invulnerable", e.invulnerable);
    out.put_int("PortalCooldown", e.portal_cooldown);
    out.put("UUID", uuid_tag(e.uuid));
    out.put("WorldUUIDLeast", Tag::Long(l.world_uuid.1));
    out.put("WorldUUIDMost", Tag::Long(l.world_uuid.0));
    out.put_int("Bukkit.updateLevel", 2);
    out.put_int("Spigot.ticksLived", e.total_age);
    out.merge(&e.extra);
    if let Some((origin, world)) = origin {
        out.put("Paper.OriginWorld", uuid_tag(world));
        out.put("Paper.Origin", Tag::List(vec![Tag::double(origin.x), Tag::double(origin.y), Tag::double(origin.z)]));
    }
    out.put_string("Paper.SpawnReason", spawn_reason);
}
