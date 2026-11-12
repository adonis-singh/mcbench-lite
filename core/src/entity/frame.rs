// ItemFrame and GlowItemFrame (HangingEntity, BlockAttachedEntity): loaded
// from saved data, ticking (the survival check every hanging-tick-frequency
// ticks), broken by an explosion with what they drop, read by comparators,
// and saved. Placing, rotating or emptying one is a player's, which this port
// does not have.

use super::{item, remove, Entity, Kind};
use crate::codec::{self, fail, items, Input, R};
use crate::collision;
use crate::components;
use crate::dir::{Axis, Dir, Pos, ALL};
use crate::level::Level;
use crate::loot::ItemStack;
use crate::nbt::{Compound, Tag};
use crate::phys::{Aabb, Vec3};

// Spigot's hanging-tick-frequency.
const TICK_FREQUENCY: i32 = 100;

// ItemFrame's fields: the direction it faces, the block it hangs on (pos),
// its item (one, or none), the rotation of the item, the chance the item
// drops, and the flags; checkInterval counts to the next survival check.
#[derive(Clone, Debug)]
pub struct FrameData {
    pub glow: bool,
    pub direction: Dir,
    pub pos: Pos,
    pub item: ItemStack,
    pub rotation: i32,
    pub drop_chance: f32,
    pub invisible: bool,
    pub fixed: bool,
    pub check_interval: i32,
}

fn data(e: &Entity) -> &FrameData {
    match &e.kind {
        Kind::Frame(d) => d,
        _ => panic!("not an item frame"),
    }
}

fn data_mut(e: &mut Entity) -> &mut FrameData {
    match &mut e.kind {
        Kind::Frame(d) => d,
        _ => panic!("not an item frame"),
    }
}

// The block a hanging entity (item frame or painting) hangs on, and the way
// it faces.
pub fn hanging_pos(e: &Entity) -> Pos {
    match &e.kind {
        Kind::Frame(d) => d.pos,
        Kind::Painting(p) => p.pos,
        _ => panic!("not a hanging entity"),
    }
}

fn hanging_pos_mut(e: &mut Entity) -> &mut Pos {
    match &mut e.kind {
        Kind::Frame(d) => &mut d.pos,
        Kind::Painting(p) => &mut p.pos,
        _ => panic!("not a hanging entity"),
    }
}

pub fn hanging_direction(e: &Entity) -> Dir {
    match &e.kind {
        Kind::Frame(d) => d.direction,
        Kind::Painting(p) => p.direction,
        _ => panic!("not a hanging entity"),
    }
}

fn check_interval_mut(e: &mut Entity) -> &mut i32 {
    match &mut e.kind {
        Kind::Frame(d) => &mut d.check_interval,
        Kind::Painting(p) => &mut p.check_interval,
        _ => panic!("not a hanging entity"),
    }
}

// Direction.get2DDataValue of a horizontal direction.
pub fn data_2d(direction: Dir) -> i32 {
    match direction {
        Dir::South => 0,
        Dir::West => 1,
        Dir::North => 2,
        Dir::East => 3,
        _ => panic!("no 2d data value for {}", direction.name()),
    }
}

// ItemFrame.calculateBoundingBoxStatic (without a framed map).
fn bounding_box(pos: Pos, direction: Dir) -> Aabb {
    let (sx, sy, sz) = direction.offset();
    // Vec3.atCenterOf(pos).relative(direction, -0.46875).
    let center = Vec3::new(pos.x() as f64 + 0.5 + sx as f64 * -0.46875, pos.y() as f64 + 0.5 + sy as f64 * -0.46875, pos.z() as f64 + 0.5 + sz as f64 * -0.46875);
    let side = 0.75f32 as f64;
    let axis = direction.axis();
    let dx = if axis == Axis::X { 0.0625 } else { side };
    let dy = if axis == Axis::Y { 0.0625 } else { side };
    let dz = if axis == Axis::Z { 0.0625 } else { side };
    // AABB.ofSize.
    Aabb::new(center.x - dx / 2.0, center.y - dy / 2.0, center.z - dz / 2.0, center.x + dx / 2.0, center.y + dy / 2.0, center.z + dz / 2.0)
}

// HangingEntity.recalculateBoundingBox: the position is the box's center.
fn recalculate_bounding_box(l: &mut Level, e: &mut Entity) {
    let aabb = match &e.kind {
        Kind::Frame(d) => bounding_box(d.pos, d.direction),
        Kind::Painting(p) => super::painting::bounding_box(l.reg, p),
        _ => panic!("not a hanging entity"),
    };
    let center = aabb.center();
    raw_pos(l, e, center.x, center.y, center.z);
    e.bb = aabb;
}

// Entity.setPosRaw: the position and the block it is in, the entity moving
// between sections; the box stays.
pub(super) fn raw_pos(l: &mut Level, e: &mut Entity, x: f64, y: f64, z: f64) {
    if !(x.is_finite() && y.is_finite() && z.is_finite()) {
        return;
    }
    if e.pos.x != x || e.pos.y != y || e.pos.z != z {
        e.pos = Vec3::new(x, y, z);
        let (fx, fy, fz) = (crate::phys::floor(x), crate::phys::floor(y), crate::phys::floor(z));
        if fx != e.block_pos.x() || fy != e.block_pos.y() || fz != e.block_pos.z() {
            e.block_pos = Pos::new(fx, fy, fz);
        }
        super::on_move(l, e);
    }
}

// BlockAttachedEntity.setPos.
pub(super) fn frame_set_pos(l: &mut Level, e: &mut Entity, x: f64, y: f64, z: f64) {
    *hanging_pos_mut(e) = Pos::new(crate::phys::floor(x), crate::phys::floor(y), crate::phys::floor(z));
    recalculate_bounding_box(l, e);
    e.has_impulse = true;
}

// ItemFrame.setDirection.
fn set_direction(l: &mut Level, e: &mut Entity, direction: Dir) {
    data_mut(e).direction = direction;
    if direction.is_horizontal() {
        e.x_rot = 0.0;
        e.y_rot = (data_2d(direction) * 90) as f32;
    } else {
        // getAxisDirection().getStep(): -1 down, 1 up.
        e.x_rot = (-90 * if direction == Dir::Up { 1 } else { -1 }) as f32;
        e.y_rot = 0.0;
    }
    e.x_rot_o = e.x_rot;
    e.y_rot_o = e.y_rot;
    recalculate_bounding_box(l, e);
}

// ItemFrame(EntityType, Level): the constructor draws what Entity does.
fn blank_frame(l: &mut Level, glow: bool) -> Box<Entity> {
    // EntityType.ITEM_FRAME and GLOW_ITEM_FRAME: 0.5 by 0.5, eye height 0.425.
    let frame = FrameData { glow, direction: Dir::South, pos: Pos::new(0, 0, 0), item: ItemStack::empty(), rotation: 0, drop_chance: 1.0, invisible: false, fixed: false, check_interval: 0 };
    let mut e = Box::new(Entity::new(l, Kind::Frame(frame), 0.5f32, 0.5f32, 0.425f32));
    // BlockAttachedEntity: checkInterval = getId() % hangingTickFrequency.
    data_mut(&mut e).check_interval = e.id % TICK_FREQUENCY;
    e
}

// BlockPos.CODEC: three ints, as an int array or a list of numbers.
pub(super) fn block_pos(tag: &Tag) -> R<Pos> {
    let ints: Vec<i32> = match tag {
        Tag::IntArray(a) => a.clone(),
        other => items(other)?.iter().map(|t| t.box_int().ok_or_else(|| codec::Fail("Some elements are not numbers".into()))).collect::<R<Vec<i32>>>()?,
    };
    if ints.len() != 3 {
        return fail(format!("Expected an array of 3 ints, but got {}", ints.len()));
    }
    Ok(Pos::new(ints[0], ints[1], ints[2]))
}

// Entity.load and ItemFrame's own fields (BlockAttachedEntity.readAdditionalSaveData
// first), for an item frame the constructor made.
pub fn load(l: &mut Level, tag: &Compound, glow: bool) -> R<super::load::Loaded> {
    let input = Input(tag);
    let mut e = blank_frame(l, glow);
    super::load::load_common(l, &mut e, &input, true)?;
    // BlockAttachedEntity.readAdditionalSaveData: the block it hangs on, if
    // within 16 blocks of the one the entity is in.
    if let Some(at) = input.read("block_pos", block_pos)? {
        let (dx, dy, dz) = ((at.x() - e.block_pos.x()) as f64, (at.y() - e.block_pos.y()) as f64, (at.z() - e.block_pos.z()) as f64);
        if dx * dx + dy * dy + dz * dz < 16.0 * 16.0 {
            data_mut(&mut e).pos = at;
        }
    }
    // The item: one of it; changing it makes the box again.
    let stack = input.read("Item", |t| components::decode_stack(l.reg, t))?;
    let item = stack.map(|mut s| {
        s.count = 1;
        s
    });
    data_mut(&mut e).item = item.unwrap_or_else(ItemStack::empty);
    recalculate_bounding_box(l, &mut e);
    // setRotation(ItemRotation, false): the rotation modulo 8 (Java's, which keeps the sign).
    data_mut(&mut e).rotation = input.byte_or("ItemRotation", 0)? as i32 % 8;
    data_mut(&mut e).drop_chance = input.float_or("ItemDropChance", 1.0)?;
    let direction = match input.read("Facing", codec::byte)? {
        Some(b) => ALL[((b as i32) % 6).unsigned_abs() as usize],
        None => Dir::Down,
    };
    set_direction(l, &mut e, direction);
    data_mut(&mut e).invisible = input.bool_or("Invisible", false)?;
    data_mut(&mut e).fixed = input.bool_or("Fixed", false)?;
    let (origin, spawn_reason) = super::load::load_paper(l, &input)?;
    Ok(super::load::Loaded { entity: e, origin, spawn_reason })
}

// The reapplied position of a loaded frame (Entity.load calls reapplyPosition,
// which BlockAttachedEntity.setPos takes).
pub fn reapply_position(l: &mut Level, e: &mut Entity) {
    let p = e.pos;
    frame_set_pos(l, e, p.x, p.y, p.z);
}

// The position a loaded entity is set to first (setPosRaw).
pub fn set_raw(l: &mut Level, e: &mut Entity, x: f64, y: f64, z: f64) {
    raw_pos(l, e, x, y, z);
}

// ItemFrame.addAdditionalSaveData and the tags after the common ones.
pub fn save_fields(l: &Level, e: &Entity, out: &mut Compound) -> R<()> {
    let d = data(e);
    out.put("block_pos", Tag::IntArray(vec![d.pos.x(), d.pos.y(), d.pos.z()]));
    if d.item.count > 0 {
        out.put("Item", components::encode_stack(l.reg, &d.item)?);
    }
    out.put("ItemRotation", Tag::Byte(d.rotation as i8));
    out.put("ItemDropChance", Tag::float(d.drop_chance));
    out.put("Facing", Tag::Byte(ALL.iter().position(|dir| *dir == d.direction).unwrap() as i8));
    out.put_boolean("Invisible", d.invisible);
    out.put_boolean("Fixed", d.fixed);
    out.put_string("id", if d.glow { "minecraft:glow_item_frame" } else { "minecraft:item_frame" });
    Ok(())
}

// The frame's block (BlockAttachedEntity.getPos).
pub fn attachment(e: &Entity) -> Pos {
    hanging_pos(e)
}

// ItemFrame.getAnalogOutput.
pub fn analog_output(e: &Entity) -> i32 {
    let d = data(e);
    if d.item.count <= 0 {
        0
    } else {
        d.rotation % 8 + 1
    }
}

// The item frames facing this way whose boxes meet the block at a position
// (ComparatorBlock.getItemFrame).
pub fn comparator_frame(l: &Level, direction: Dir, pos: Pos) -> Option<i32> {
    if l.entities.frames.is_empty() {
        return None;
    }
    let aabb = Aabb::new(pos.x() as f64, pos.y() as f64, pos.z() as f64, (pos.x() + 1) as f64, (pos.y() + 1) as f64, (pos.z() + 1) as f64);
    let found = super::entities_in(l, None, None, super::Only::Frames, &aabb, |e| matches!(&e.kind, Kind::Frame(f) if f.direction == direction) && !e.removed);
    if found.len() == 1 {
        Some(analog_output(&l.entities.all[&found[0]]))
    } else {
        None
    }
}

// ItemFrame.survives.
fn survives(l: &mut Level, e: &Entity) -> bool {
    let d = data(e);
    if d.fixed {
        return true;
    }
    // getPopBox: the box without a framed map (the same as its own).
    let pop = bounding_box(d.pos, d.direction);
    if !super::collide::no_collision(l, e, &pop) {
        return false;
    }
    let behind = d.pos.relative(d.direction.opposite());
    let state = l.get(behind);
    let supported = l.reg.flags(state) & crate::registry::SOLID != 0 || d.direction.is_horizontal() && crate::blocks::is_diode(l, state);
    // canCoexist(true): no other hanging entity facing this way in the box.
    supported && super::entities_in(l, Some(e), Some(e.id), super::Only::Hanging, &pop, |other| hanging_direction(other) == d.direction).is_empty()
}

// BlockAttachedEntity.tick, for an item frame or a painting.
pub fn tick(l: &mut Level, e: &mut Entity) {
    // checkBelowWorld.
    if e.pos.y < (crate::world::MIN_Y as f64) + -64.0 {
        remove(l, e);
    }
    let interval = *check_interval_mut(e);
    *check_interval_mut(e) += 1;
    if interval == TICK_FREQUENCY {
        *check_interval_mut(e) = 0;
        if !e.removed {
            let survives = match e.kind {
                Kind::Frame(_) => survives(l, e),
                _ => super::painting::survives(l, e),
            };
            if !survives {
                // discard(DROP), then dropItem(level, null).
                remove(l, e);
                match e.kind {
                    Kind::Frame(_) => drop_item(l, e),
                    _ => super::painting::drop_item(l, e),
                }
            }
        }
    }
}

// ItemFrame.dropItem(level, entity) with no entity: the break sound, then the
// frame and its item, then the block change event.
fn drop_item(l: &mut Level, e: &mut Entity) {
    drop_item_with(l, e, true);
    let source = crate::game_event::Context::of_entity(None);
    let event = crate::game_event!(l, "block_change");
    l.game_event(event, e.pos, source);
}

// Entity.kill(level): the entity is removed, killed, and the event says so.
fn kill(l: &mut Level, e: &mut Entity) {
    remove(l, e);
    let source = crate::game_event::Source::of(l, e);
    let event = crate::game_event!(l, "entity_die");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(Some(source)));
}

// ItemFrame.move and BlockAttachedEntity.move: a frame that is not fixed
// breaks when anything moves it (the HangingBreakEvent has no listener); a
// fixed one stays.
pub fn moved(l: &mut Level, e: &mut Entity, movement: Vec3) {
    if data(e).fixed {
        return;
    }
    if !e.removed && movement.length_sqr() > 0.0 {
        kill(l, e);
        drop_item(l, e);
    }
}

// ItemFrame.dropItem(level, entity, dropItem) with no entity.
fn drop_item_with(l: &mut Level, e: &mut Entity, drop_frame: bool) {
    if data(e).fixed {
        return;
    }
    let held = std::mem::replace(&mut data_mut(e).item, ItemStack::empty());
    // setItem(EMPTY) makes the box again and tells the comparators.
    recalculate_bounding_box(l, e);
    let pos = data(e).pos;
    let air = l.block(l.known.air);
    l.update_neighbour_for_output_signal(pos, air);
    if drop_frame {
        let glow = data(e).glow;
        let frame_item = if glow { "minecraft:glow_item_frame" } else { "minecraft:item_frame" };
        let frame = ItemStack::new(l.reg.item_by_key[frame_item], 1);
        spawn_at_location(l, e, frame);
    }
    if held.count > 0 {
        let held = held.clone();
        if l.shared_random.next_float() < data(e).drop_chance {
            spawn_at_location(l, e, held);
        }
    }
}

// ItemFrame.spawnAtLocation(level, stack) and HangingEntity.spawnAtLocation:
// an item entity a little in front of the frame.
pub(super) fn spawn_at_location(l: &mut Level, e: &Entity, stack: ItemStack) {
    let direction = hanging_direction(e);
    let y_offset = if direction == Dir::Down { -0.6f32 } else { 0.0f32 };
    let (sx, _, sz) = direction.offset();
    let x = e.pos.x + (sx as f32 * 0.15f32) as f64;
    let y = e.pos.y + y_offset as f64;
    let z = e.pos.z + (sz as f32 * 0.15f32) as f64;
    item::spawn_with_default_delay(l, x, y, z, stack);
}

// BlockAttachedEntity.ignoreExplosion: an explosion whose source is in water
// leaves a frame be.
pub fn ignores_explosion(source: &Entity) -> bool {
    source.was_touching_water
}

// ItemFrame.hurtServer for an explosion's damage (the entity `e` is out of
// the level's map): a frame that is not fixed breaks and drops.
pub fn hurt_by_explosion(l: &mut Level, e: &mut Entity) {
    if data(e).fixed || e.invulnerable || e.removed {
        return;
    }
    // kill(level), markHurt, dropItem(level, the damage source's entity: none).
    kill(l, e);
    drop_item(l, e);
}

