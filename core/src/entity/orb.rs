// ExperienceOrb as Paper runs it: the orbs a block's experience
// (Block.popExperience) and a furnace's recipes give
// (ExperienceOrb.awardWithDirection, with the merge into an orb already there
// and the split of the amount into orb values), their making (the shared
// random's rotation and velocity, the push out of a block it is made in:
// unstuckIfPossible and CollisionGetter.findFreePosition), their tick (gravity,
// water and lava, the scan that merges orbs of a value, the despawn), what
// fire, lava and explosions do to them (hurtServer), and their saved data
// (Paper.ExpData). No orb has a player to follow: the level holds none (the
// verifier's input player is made in no level's player list).
//
// Paper's configuration is at its defaults: misc.xp-orb-groups-per-area is
// the vanilla 40, and Spigot's merge-radius.exp is -1, so an orb added to the
// level is not merged by CraftEventFactory.doEntityAddEventCalling (and
// entities.behavior.experience-merge-max-value is never read). The Bukkit and
// Paper events an orb fires (ExperienceOrbMergeEvent, EntitySpawnEvent,
// BlockExpEvent) have no listener.

use super::damage::Dmg;
use super::motion::{self, set_pos};
use super::{add_fresh_entity, entities_in, fluid, remove, Entity, Kind, Only};
use crate::codec::{fail, Input, R};
use crate::dir::Pos;
use crate::level::{refuse, Level};
use crate::nbt::{Compound, Tag};
use crate::phys::{self, Aabb, Vec3};

// ExperienceOrb's fields: the value (DATA_VALUE), how many orbs of the value
// it stands for (count), its age and health, and Paper's ExpData (the
// entities it came from and why).
pub struct OrbData {
    pub value: i32,
    pub count: i32,
    pub age: i32,
    pub health: i32,
    pub source: Option<(i64, i64)>,
    pub trigger: Option<(i64, i64)>,
    pub reason: &'static str,
}

const LIFETIME: i32 = 6000;
// GlobalConfiguration.misc.xpOrbGroupsPerArea.or(40) at its default.
const ORB_GROUPS_PER_AREA: i32 = 40;

// org.bukkit.entity.ExperienceOrb.SpawnReason.
const SPAWN_REASONS: [&str; 11] = ["PLAYER_DEATH", "ENTITY_DEATH", "FURNACE", "BREED", "VILLAGER_TRADE", "FISHING", "BLOCK_BREAK", "CUSTOM", "EXP_BOTTLE", "GRINDSTONE", "UNKNOWN"];

pub fn data(e: &Entity) -> &OrbData {
    match &e.kind {
        Kind::Orb(d) => d,
        _ => panic!("not an experience orb"),
    }
}

fn data_mut(e: &mut Entity) -> &mut OrbData {
    match &mut e.kind {
        Kind::Orb(d) => d,
        _ => panic!("not an experience orb"),
    }
}

// ExperienceOrb(EntityType, Level): what Entity's constructor draws (the id
// and the UUID).
pub fn blank_orb(l: &mut Level) -> Box<Entity> {
    let t = l.reg.entity_types["minecraft:experience_orb"].clone();
    let orb = OrbData { value: 0, count: 1, age: 0, health: 5, source: None, trigger: None, reason: "UNKNOWN" };
    Box::new(Entity::new(l, Kind::Orb(orb), t.width, t.height, t.eye_height))
}

// ExperienceOrb(level, pos, Vec3.ZERO, value, reason, triggerId, null): the
// rotation and the velocity from the shared random, and the push out of a
// block the orb is made in. Not added yet.
fn new_orb(l: &mut Level, pos: Vec3, value: i32, reason: &'static str, trigger: Option<(i64, i64)>) -> Box<Entity> {
    let mut e = blank_orb(l);
    {
        let d = data_mut(&mut e);
        d.trigger = trigger;
        d.reason = reason;
    }
    set_pos(l, &mut e, pos.x, pos.y, pos.z);
    e.y_rot = l.shared_random.next_float() * 360.0f32;
    let x = (l.shared_random.next_double() * 0.2 - 0.1) * 2.0;
    let y = l.shared_random.next_double() * 0.2 * 2.0;
    let z = (l.shared_random.next_double() * 0.2 - 0.1) * 2.0;
    // The direction is Vec3.ZERO: the velocity is not turned, and the
    // position moves by the direction's normal (zero) scaled.
    let size = (e.bb.xsize() + e.bb.ysize() + e.bb.zsize()) / 3.0;
    let at = pos.add_vec(Vec3::ZERO.scale(size * 0.5));
    set_pos(l, &mut e, at.x, at.y, at.z);
    e.delta = Vec3::new(x, y, z);
    if !super::collide::no_collision(l, &e, &e.bb) {
        unstuck_if_possible(l, &mut e, size);
    }
    data_mut(&mut e).value = value;
    e
}

// ExperienceOrb.unstuckIfPossible: the orb moves to the point nearest its
// center of the cube of its size that the blocks around it leave free.
fn unstuck_if_possible(l: &mut Level, e: &mut Entity, size: f64) {
    let center = e.pos.add(0.0, e.height as f64 / 2.0, 0.0);
    let cube = crate::shape_ops::create(center.x - size / 2.0, center.y - size / 2.0, center.z - size / 2.0, center.x + size / 2.0, center.y + size / 2.0, center.z + size / 2.0);
    if let Some(free) = find_free_position(l, e, &cube, center, e.width as f64, e.height as f64, e.width as f64) {
        let to = free.add(0.0, -(e.height as f64) / 2.0, 0.0);
        set_pos(l, e, to.x, to.y, to.z);
    }
}

// CollisionGetter.findFreePosition(entity, shape, pos, x, y, z): the blocks
// that collide with the shape's bounds grown by x, y, z (BlockCollisions, in
// its order), each of their boxes grown by half of that, joined; the point of
// the shape outside them nearest to `pos`. The world border is far from any
// box here.
fn find_free_position(l: &Level, e: &Entity, shape: &crate::registry::Shape, pos: Vec3, x: f64, y: f64, z: f64) -> Option<Vec3> {
    use crate::shape_ops;
    if shape.is_empty() {
        return None;
    }
    let b = shape.bounds();
    let aabb = Aabb::new(b[0] - x, b[1] - y, b[2] - z, b[3] + x, b[4] + y, b[5] + z);
    let mut blocks = shape_ops::empty();
    for boxes in crate::collision::vanilla_block_boxes(l, &aabb, super::collide::context(l, e)) {
        for b in boxes {
            let grown = shape_ops::create(b[0] - x / 2.0, b[1] - y / 2.0, b[2] - z / 2.0, b[3] + x / 2.0, b[4] + y / 2.0, b[5] + z / 2.0);
            blocks = shape_ops::or(&blocks, &grown);
        }
    }
    let free = shape_ops::optimize(&shape_ops::join_unoptimized(shape, &blocks, shape_ops::only_first));
    closest_point_to(&free, pos)
}

// VoxelShape.closestPointTo (moonrise's): the point of the shape's boxes
// (toAabbs) nearest to `point`, the first of equals.
fn closest_point_to(shape: &crate::registry::Shape, point: Vec3) -> Option<Vec3> {
    if shape.is_empty() {
        return None;
    }
    let mut found = None;
    let mut best = f64::MAX;
    for b in shape.boxes() {
        let x = phys::clamp(point.x, b[0], b[3]);
        let y = phys::clamp(point.y, b[1], b[4]);
        let z = phys::clamp(point.z, b[2], b[5]);
        let dist = point.distance_to_sqr(Vec3::new(x, y, z));
        if dist < best {
            found = Some(Vec3::new(x, y, z));
            best = dist;
        }
    }
    found
}

// ExperienceOrb.getExperienceValue: the largest orb value an amount makes.
fn experience_value(value: i32) -> i32 {
    if value > 162670129 {
        return value - 100000;
    }
    for (above, worth) in [(81335063, 81335063), (40667527, 40667527), (20333759, 20333759), (10166857, 10166857), (5083423, 5083423), (2541701, 2541701), (1270849, 1270849), (635413, 635413), (317701, 317701), (158849, 158849), (79423, 79423), (39709, 39709), (19853, 19853), (9923, 9923), (4957, 4957)] {
        if value > above {
            return worth;
        }
    }
    for worth in [2477, 1237, 617, 307, 149, 73, 37, 17, 7, 3] {
        if value >= worth {
            return worth;
        }
    }
    1
}

// ExperienceOrb.awardWithDirection(level, pos, Vec3.ZERO, amount, reason,
// triggerId, null): orbs of the amount's values, each merged into an orb
// there of its value when one is found, else added.
pub fn award(l: &mut Level, pos: Vec3, amount: i32, reason: &'static str, trigger: Option<(i64, i64)>) {
    let mut amount = amount;
    while amount > 0 {
        let value = experience_value(amount);
        amount -= value;
        if !try_merge_to_existing(l, pos, value) {
            let e = new_orb(l, pos, value, reason, trigger);
            add_fresh_entity(l, e);
        }
    }
}

// ExperienceOrb.tryMergeToExisting: an orb in the block around `pos` of this
// value and of the group the level's random picks counts one more.
fn try_merge_to_existing(l: &mut Level, pos: Vec3, amount: i32) -> bool {
    let aabb = Aabb::new(pos.x - 0.5, pos.y - 0.5, pos.z - 0.5, pos.x + 0.5, pos.y + 0.5, pos.z + 0.5);
    let group = l.random.next_int_bound(ORB_GROUPS_PER_AREA);
    if matches!(l.entities.ticking, Some(t) if t.classes & 1 << Only::Orbs as u8 != 0) {
        refuse("experience orbs given while an orb ticks");
    }
    let found = entities_in(l, None, None, Only::Orbs, &aabb, |orb| can_merge_with(orb, group, amount));
    let Some(&first) = found.first() else { return false };
    let orb = data_mut(l.entities.all.get_mut(&first).expect("an orb in the level"));
    orb.count += 1;
    orb.age = 0;
    true
}

// ExperienceOrb.canMerge(orb, id, value): not removed, of the group, of the value.
fn can_merge_with(orb: &Entity, id: i32, value: i32) -> bool {
    !orb.removed && (orb.id.wrapping_sub(id)) % ORB_GROUPS_PER_AREA == 0 && data(orb).value == value
}

// Entity.fireImmune: the orb's type is not.
pub fn fire_immune() -> bool {
    false
}

// ExperienceOrb.hurtServer: an orb takes any damage it is not invulnerable
// to (no damage here bypasses invulnerability but the void's), and dies
// when its health runs out. No game event.
pub fn hurt(l: &mut Level, e: &mut Entity, dmg: Dmg, amount: f32) -> bool {
    // isInvulnerableToBase: removed, invulnerable (the void bypasses it); no
    // fire source hurts a fire immune orb, and none is.
    if e.removed || (e.invulnerable && !dmg.bypasses_invulnerability()) {
        return false;
    }
    let d = data_mut(e);
    d.health = (d.health as f32 - amount) as i32;
    if d.health <= 0 {
        remove(l, e);
    }
    true
}

// Entity.lavaHurt(pos): lava's damage, and the burn sound's pitch from the
// shared random unless the orb is silent.
pub fn lava_hurt(l: &mut Level, e: &mut Entity) {
    if hurt(l, e, Dmg::Lava, 4.0f32) && !super::minecart::is_silent(e) {
        l.shared_random.next_float();
    }
}

// Entity.baseTick for an orb.
fn base_tick(l: &mut Level, e: &mut Entity) {
    e.in_powder_snow = false;
    fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    fluid::update_fluid_on_eyes(l, e);
    if e.remaining_fire_ticks > 0 {
        if e.remaining_fire_ticks % 20 == 0 && !fluid::is_in_lava(e) {
            hurt(l, e, Dmg::OnFire, 1.0f32);
        }
        e.remaining_fire_ticks -= 1;
    }
    if fluid::is_in_lava(e) {
        e.fall_distance *= 0.5;
    } else {
        e.last_lava_contact = None;
    }
    // checkBelowWorld: void damage is on, 64 blocks under the world
    // (onBelowWorld discards the orb).
    if e.pos.y < (crate::world::MIN_Y as f64) + -64.0 {
        remove(l, e);
    }
    e.first_tick = false;
}

// ExperienceOrb.getDefaultGravity, which Entity.getGravity makes 0 for an orb
// without gravity.
fn gravity(e: &Entity) -> f64 {
    if e.no_gravity {
        0.0
    } else {
        0.03
    }
}

// ExperienceOrb.tick. An orb removed on the way (below the world, burnt to
// death) goes on to the end of its tick, as Java's does.
pub fn tick(l: &mut Level, e: &mut Entity) {
    // Entity.tick: Paper's despawn time is not set.
    base_tick(l, e);
    let stuck = !super::collide::no_collision(l, e, &e.bb);
    if e.fluid_on_eyes[0] {
        // setUnderwaterMovement.
        let dm = e.delta;
        e.delta = Vec3::new(dm.x * 0.99f32 as f64, phys::jmin(dm.y + 5.0E-4f32 as f64, 0.06f32 as f64), dm.z * 0.99f32 as f64);
    } else if !stuck {
        // applyGravity.
        let g = gravity(e);
        if g != 0.0 {
            e.delta = e.delta.add(0.0, -g, 0.0);
        }
    }
    let fluid_state = l.fluid_state(e.block_pos);
    if l.fluid_type(fluid_state).is_lava() {
        let x = (l.shared_random.next_float() - l.shared_random.next_float()) * 0.2f32;
        let z = (l.shared_random.next_float() - l.shared_random.next_float()) * 0.2f32;
        e.delta = Vec3::new(x as f64, 0.2f32 as f64, z as f64);
    }
    if e.tick_count % 20 == 1 {
        scan_for_merges(l, e);
    }
    // followNearbyPlayer: no player to follow.
    if stuck {
        let moved = e.bb.mv(e.delta.x, e.delta.y, e.delta.z);
        if !super::collide::no_collision(l, e, &moved) {
            motion::move_towards_closest_space(l, e, e.pos.x, (e.bb.min_y + e.bb.max_y) / 2.0, e.pos.z);
            e.has_impulse = true;
        }
    }
    let d = e.delta.y;
    motion::move_entity(l, e, e.delta);
    motion::apply_effects_from_blocks(l, e);
    let mut f = 0.98f32;
    if e.on_ground {
        // getBlockPosBelowThatAffectsMyMovement: getOnPos(0.999999F).
        let below = motion::get_on_pos(l, e, 0.999999f32);
        let state = l.get(below);
        f = l.reg.blocks[l.block(state) as usize].friction * 0.98f32;
    }
    e.delta = e.delta.scale(f as f64);
    if e.vertical_collision_below && d < -gravity(e) {
        e.delta = Vec3::new(e.delta.x, -d * 0.4, e.delta.z);
    }
    let orb = data_mut(e);
    orb.age += 1;
    if orb.age >= LIFETIME {
        remove(l, e);
    }
}

// ExperienceOrb.scanForMerges: the orbs of its group and value around it
// join it (ExperienceOrb.merge, the merge event has no listener).
fn scan_for_merges(l: &mut Level, e: &mut Entity) {
    let (id, value) = (e.id, data(e).value);
    let area = e.bb.inflate(0.5, 0.5, 0.5);
    let found = entities_in(l, Some(e), None, Only::Orbs, &area, |orb| orb.id != id && can_merge_with(orb, id, value));
    for other_id in found {
        let mut other = l.entities.all.remove(&other_id).expect("an orb in the level");
        let (count, age) = (data(&other).count, data(&other).age);
        let me = data_mut(e);
        me.count += count;
        me.age = me.age.min(age);
        remove(l, &mut other);
    }
}

// ExperienceOrb.readAdditionalSaveData (and Entity.load's common fields) for
// an orb the verifier imports.
pub fn load(l: &mut Level, tag: &Compound) -> R<super::load::Loaded> {
    let input = Input(tag);
    let mut e = blank_orb(l);
    super::load::load_common(l, &mut e, &input, false)?;
    let health = input.short_or("Health", 5)? as i32;
    let age = input.short_or("Age", 0)? as i32;
    let value = input.int_or("Value", 0)?;
    // ExtraCodecs.POSITIVE_INT.
    let count = input.read("Count", |t| {
        let v = crate::codec::int(t)?;
        if v > 0 {
            Ok(v)
        } else {
            fail(format!("Value must be positive: {v}"))
        }
    })?;
    let (mut source, mut trigger, mut reason) = (None, None, "UNKNOWN");
    // loadPaperNBT: the uuids read quietly; a reason Bukkit does not know is
    // logged through the server, which this level has not.
    if let Some(exp) = input.read("Paper.ExpData", |t| crate::codec::map(t))? {
        source = exp.get("source").and_then(|t| super::load::uuid(t).ok());
        trigger = exp.get("trigger").and_then(|t| super::load::uuid(t).ok());
        if let Some(Tag::String(name)) = exp.get("reason") {
            match SPAWN_REASONS.iter().find(|r| **r == name.as_str()) {
                Some(r) => reason = r,
                None => return fail(format!("an experience orb's spawn reason {name}")),
            }
        }
    }
    {
        let d = data_mut(&mut e);
        d.health = health;
        d.age = age;
        d.value = value;
        d.count = count.unwrap_or(1);
        d.source = source;
        d.trigger = trigger;
        d.reason = reason;
    }
    let (origin, spawn_reason) = super::load::load_paper(l, &input)?;
    Ok(super::load::Loaded { entity: e, origin, spawn_reason })
}

// ExperienceOrb.addAdditionalSaveData and savePaperNBT.
pub fn save_fields(e: &Entity, out: &mut Compound) {
    let d = data(e);
    out.put("Health", Tag::Short(d.health as i16));
    out.put("Age", Tag::Short(d.age as i16));
    out.put_int("Value", d.value);
    out.put_int("Count", d.count);
    let mut exp = Compound::new();
    if let Some(source) = d.source {
        exp.put("source", super::load::uuid_tag(source));
    }
    if let Some(trigger) = d.trigger {
        exp.put("trigger", super::load::uuid_tag(trigger));
    }
    if d.reason != "UNKNOWN" {
        exp.put_string("reason", d.reason);
    }
    out.put("Paper.ExpData", Tag::Compound(exp));
    out.put_string("id", "minecraft:experience_orb");
}

// What a block position is, as BlockPos the orbs of a broken block are
// given at the center of (Vec3.atCenterOf).
pub fn center_of(pos: Pos) -> Vec3 {
    Vec3::new(pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5)
}
