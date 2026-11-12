// Painting (a HangingEntity, BlockAttachedEntity): loaded from saved data,
// ticking (the survival check every hanging-tick-frequency ticks), broken by
// an explosion with what it drops, and saved. Placing one is a player's.

use super::{frame, remove, Entity, Kind, Only};
use crate::codec::{self, Input, R};
use crate::dir::{Axis, Dir, Pos};
use crate::level::Level;
use crate::loot::ItemStack;
use crate::nbt::{Compound, Tag};
use crate::phys::{Aabb, Vec3};
use crate::registry::Registry;

// Painting's fields: the variant (an index in the data pack's painting
// variants), the direction it faces, the block it hangs at, and the counter
// to its next survival check.
#[derive(Clone, Debug)]
pub struct PaintingData {
    pub variant: usize,
    pub direction: Dir,
    pub pos: Pos,
    pub check_interval: i32,
}

pub fn data(e: &Entity) -> &PaintingData {
    match &e.kind {
        Kind::Painting(d) => d,
        _ => panic!("not a painting"),
    }
}

pub fn data_mut(e: &mut Entity) -> &mut PaintingData {
    match &mut e.kind {
        Kind::Painting(d) => d,
        _ => panic!("not a painting"),
    }
}

// Painting.offsetForPaintingSize.
fn offset_for_size(size: i32) -> f64 {
    if size % 2 == 0 {
        0.5
    } else {
        0.0
    }
}

// Painting.calculateBoundingBoxStatic.
pub fn bounding_box(reg: &Registry, p: &PaintingData) -> Aabb {
    let variant = &reg.paintings[p.variant];
    let (width, height) = (variant.width, variant.height);
    let direction = p.direction;
    let (sx, sy, sz) = direction.offset();
    // Vec3.atCenterOf(pos).relative(direction, -0.46875).
    let c = Vec3::new(p.pos.x() as f64 + 0.5 + -0.46875 * sx as f64, p.pos.y() as f64 + 0.5 + -0.46875 * sy as f64, p.pos.z() as f64 + 0.5 + -0.46875 * sz as f64);
    let d = offset_for_size(width);
    let d1 = offset_for_size(height);
    let (cx, cy, cz) = direction.counter_clockwise().offset();
    let vec3 = Vec3::new(c.x + d * cx as f64, c.y + d * cy as f64, c.z + d * cz as f64);
    // .relative(Direction.UP, d1).
    let center = Vec3::new(vec3.x + d1 * 0.0, vec3.y + d1 * 1.0, vec3.z + d1 * 0.0);
    let axis = direction.axis();
    let d2 = if axis == Axis::X { 0.0625 } else { width as f64 };
    let d3 = height as f64;
    let d4 = if axis == Axis::Z { 0.0625 } else { width as f64 };
    // AABB.ofSize.
    Aabb::new(center.x - d2 / 2.0, center.y - d3 / 2.0, center.z - d4 / 2.0, center.x + d2 / 2.0, center.y + d3 / 2.0, center.z + d4 / 2.0)
}

// Painting(EntityType, Level): the constructor draws what Entity does; the
// variant is the data pack's first.
pub fn blank_painting(l: &mut Level) -> Box<Entity> {
    let t = l.reg.entity_types["minecraft:painting"].clone();
    let data = PaintingData { variant: 0, direction: Dir::South, pos: Pos::new(0, 0, 0), check_interval: 0 };
    let mut e = Box::new(Entity::new(l, Kind::Painting(data), t.width, t.height, t.eye_height));
    // BlockAttachedEntity: checkInterval = getId() % hangingTickFrequency.
    data_mut(&mut e).check_interval = e.id % 100;
    e
}

// HangingEntity.setDirection.
fn set_direction(l: &mut Level, e: &mut Entity, direction: Dir) {
    data_mut(e).direction = direction;
    e.y_rot = (frame::data_2d(direction) * 90) as f32;
    e.y_rot_o = e.y_rot;
    recalculate(l, e);
}

// HangingEntity.recalculateBoundingBox: the position is the box's center.
fn recalculate(l: &mut Level, e: &mut Entity) {
    let aabb = bounding_box(l.reg, data(e));
    let center = aabb.center();
    frame::raw_pos(l, e, center.x, center.y, center.z);
    e.bb = aabb;
}

// Direction.LEGACY_ID_CODEC_2D: a byte, read as the 2d data value
// (Mth.abs(value % 4)).
fn legacy_direction(tag: &Tag) -> R<Dir> {
    let b = codec::byte(tag)? as i32;
    let value = (b % 4).abs();
    Ok(match value {
        0 => Dir::South,
        1 => Dir::West,
        2 => Dir::North,
        _ => Dir::East,
    })
}

// Entity.load and Painting's readAdditionalSaveData, for a painting the
// constructor made.
pub fn load(l: &mut Level, tag: &Compound) -> R<super::load::Loaded> {
    let input = Input(tag);
    let mut e = blank_painting(l);
    super::load::load_common(l, &mut e, &input, true)?;
    let direction = input.read("facing", legacy_direction)?.unwrap_or(Dir::South);
    // BlockAttachedEntity.readAdditionalSaveData: the block it hangs on, if
    // within 16 blocks of the one the entity is in.
    if let Some(at) = input.read("block_pos", frame::block_pos)? {
        let (dx, dy, dz) = ((at.x() - e.block_pos.x()) as f64, (at.y() - e.block_pos.y()) as f64, (at.z() - e.block_pos.z()) as f64);
        if dx * dx + dy * dy + dz * dz < 16.0 * 16.0 {
            data_mut(&mut e).pos = at;
        }
    }
    set_direction(l, &mut e, direction);
    // VariantUtils.readVariant: a location the registry holds sets the
    // variant, another leaves the first.
    if let Some(key) = input.read("variant", codec::location)? {
        if let Some(at) = l.reg.paintings.iter().position(|v| v.key == key) {
            data_mut(&mut e).variant = at;
            recalculate(l, &mut e);
        }
    }
    let (origin, spawn_reason) = super::load::load_paper(l, &input)?;
    Ok(super::load::Loaded { entity: e, origin, spawn_reason })
}

// Painting.addAdditionalSaveData.
pub fn save_fields(l: &Level, e: &Entity, out: &mut Compound) -> R<()> {
    let d = data(e);
    out.put("facing", Tag::Byte(frame::data_2d(d.direction) as i8));
    out.put("block_pos", Tag::IntArray(vec![d.pos.x(), d.pos.y(), d.pos.z()]));
    out.put_string("variant", &l.reg.paintings[d.variant].key);
    out.put_string("id", "minecraft:painting");
    Ok(())
}

// HangingEntity.survives.
pub fn survives(l: &mut Level, e: &Entity) -> bool {
    let d = data(e);
    // getPopBox: the bounding box.
    let pop = e.bb;
    if !super::collide::no_collision(l, e, &pop) {
        return false;
    }
    // calculateSupportBox: the box moved half a block into the wall, a bit smaller.
    let (sx, sy, sz) = d.direction.offset();
    let support = e.bb.mv((sx as f32 * -0.5f32) as f64, (sy as f32 * -0.5f32) as f64, (sz as f32 * -0.5f32) as f64).deflate(1.0E-7);
    let (min_x, min_y, min_z) = (crate::phys::floor(support.min_x), crate::phys::floor(support.min_y), crate::phys::floor(support.min_z));
    let (max_x, max_y, max_z) = (crate::phys::floor(support.max_x), crate::phys::floor(support.max_y), crate::phys::floor(support.max_z));
    for x in min_x..=max_x {
        for y in min_y..=max_y {
            for z in min_z..=max_z {
                let state = l.get(Pos::new(x, y, z));
                if l.reg.flags(state) & crate::registry::SOLID == 0 && !crate::blocks::is_diode(l, state) {
                    return false;
                }
            }
        }
    }
    // canCoexist(false): no other hanging entity of this type, or facing
    // this way, in the box.
    super::entities_in(l, Some(e), Some(e.id), Only::Hanging, &pop, |other| matches!(other.kind, Kind::Painting(_)) || frame::hanging_direction(other) == d.direction).is_empty()
}

// Painting.dropItem(level, null): the painting item (doEntityDrops is on).
pub fn drop_item(l: &mut Level, e: &mut Entity) {
    let item = ItemStack::new(l.reg.item_by_key["minecraft:painting"], 1);
    frame::spawn_at_location(l, e, item);
}

// BlockAttachedEntity.hurtServer for an explosion's damage (the entity is
// out of the level's map): it is killed and drops.
pub fn hurt_by_explosion(l: &mut Level, e: &mut Entity) {
    if e.invulnerable || e.removed {
        return;
    }
    remove(l, e);
    let die = crate::game_event!(l, "entity_die");
    l.game_event(die, e.pos, crate::game_event::Context::of_entity(Some(crate::game_event::Source::of(l, e))));
    drop_item(l, e);
}
