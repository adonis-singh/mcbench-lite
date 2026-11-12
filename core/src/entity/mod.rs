// The entity layer: the entities of a level and how the level keeps them
// (moonrise's EntityLookup and ChunkEntitySlices behind ServerLevel's
// callbacks, its EntityTickList), ported from the pinned server's code.
// Item entities, falling blocks and primed TNT exist. What an entity does is
// in item.rs, falling.rs, tnt.rs and motion.rs.

pub mod armor_stand;
pub mod falling;
pub mod fluid;
pub mod frame;
pub mod golem;
pub mod inside;
pub mod boat;
pub mod collide;
pub mod damage;
pub mod item;
pub mod load;
pub mod minecart;
pub mod orb;
pub mod painting;
pub mod motion;
pub mod tnt;
pub mod vehicle;

use crate::blocks::container::Stack;
use crate::dir::{chunk_key, Pos};
use crate::fast::FastMap;
use crate::level::Level;
use crate::phys::{self, Aabb, Vec3};
use crate::world::{HEIGHT, MIN_Y};
use std::collections::VecDeque;

pub type EntityId = i32;

// Entity.ENTITY_COUNTER of a seeded run starts here (McbenchExperiment).
pub const SEEDED_ENTITY_IDS: i32 = 1 << 24;

// FullChunkStatus from FULL up: what a chunk entities are kept in is at.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ChunkStatus {
    Full,
    BlockTicking,
    EntityTicking,
}

// net.minecraft.world.level.entity.Visibility.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Visibility {
    Hidden,
    Tracked,
    Ticking,
}

impl Visibility {
    fn accessible(self) -> bool {
        self != Visibility::Hidden
    }
    fn ticking(self) -> bool {
        self == Visibility::Ticking
    }
}

pub fn visibility(status: Option<ChunkStatus>) -> Visibility {
    match status {
        None => Visibility::Hidden,
        Some(ChunkStatus::EntityTicking) => Visibility::Ticking,
        Some(_) => Visibility::Tracked,
    }
}

// PortalProcessor: the portal block the entity is in (a nether portal, an end
// portal or an end gateway), where it came in, how long it has been in and
// whether it is in it this tick.
#[derive(Clone, Copy)]
pub struct PortalProcess {
    pub portal: crate::registry::BlockId,
    pub entry: Pos,
    pub time: i32,
    pub inside_this_tick: bool,
}

// Entity.Movement.
#[derive(Clone, Copy)]
pub struct Movement {
    pub from: Vec3,
    pub to: Vec3,
    pub axis_dependent_original: Option<Vec3>,
}

pub enum Kind {
    Item(item::ItemData),
    Falling(falling::FallingData),
    Tnt(tnt::TntData),
    Frame(frame::FrameData),
    Minecart(minecart::MinecartData),
    Boat(boat::BoatData),
    Painting(painting::PaintingData),
    ArmorStand(armor_stand::StandData),
    Orb(orb::OrbData),
    Golem(golem::GolemData),
}

pub struct Entity {
    pub id: EntityId,
    // The uuid's most and least significant bits (Entity.uuid).
    pub uuid: (i64, i64),
    pub kind: Kind,
    // removalReason != null.
    pub removed: bool,
    // The level callback is the lookup's (the entity was added to it and not removed).
    pub in_level: bool,
    pub pos: Vec3,
    pub block_pos: Pos,
    pub bb: Aabb,
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub delta: Vec3,
    pub y_rot: f32,
    pub x_rot: f32,
    pub y_rot_o: f32,
    pub x_rot_o: f32,
    pub xo: f64,
    pub yo: f64,
    pub zo: f64,
    pub x_old: f64,
    pub y_old: f64,
    pub z_old: f64,
    pub on_ground: bool,
    pub horizontal_collision: bool,
    pub vertical_collision: bool,
    pub vertical_collision_below: bool,
    pub minor_horizontal_collision: bool,
    pub fall_distance: f64,
    // Entity.stuckSpeedMultiplier (makeStuckInBlock).
    pub stuck_speed_multiplier: Vec3,
    // Entity.getTicksFrozen (DATA_TICKS_FROZEN) and isInPowderSnow (freezeLocked
    // is Paper.FreezeLock, which an import refuses).
    pub ticks_frozen: i32,
    pub in_powder_snow: bool,
    pub tick_count: i32,
    pub first_tick: bool,
    pub no_physics: bool,
    pub has_impulse: bool,
    pub portal_cooldown: i32,
    // Entity.portalProcess.
    pub portal_process: Option<PortalProcess>,
    pub main_supporting_block: Option<Pos>,
    pub on_ground_no_blocks: bool,
    pub movement_this_tick: VecDeque<Movement>,
    // Entity.pistonDeltas and pistonDeltasGameTime: how far pistons have moved
    // the entity along each axis in the game time they last pushed it.
    pub piston_deltas: [f64; 3],
    pub piston_deltas_game_time: i64,
    // Entity.wasTouchingWater, fluidHeight (water, lava), lastLavaContact and
    // remainingFireTicks.
    pub was_touching_water: bool,
    pub fluid_height: [f64; 2],
    // Entity.fluidOnEyes (water, lava) and wasEyeInWater.
    pub fluid_on_eyes: [bool; 2],
    pub was_eye_in_water: bool,
    // Entity.totalEntityAge (Spigot.ticksLived): the ticks lived; a living
    // entity loads it.
    pub total_age: i32,
    pub last_lava_contact: Option<Pos>,
    pub remaining_fire_ticks: i32,
    // Entity.airSupply, invulnerable and noGravity (loaded from saved data;
    // an entity this level makes has the defaults).
    pub air_supply: i32,
    pub invulnerable: bool,
    pub no_gravity: bool,
    // What saved data gave an entity that nothing here acts on, as the tags
    // it saves back (CustomName, Silent and the like).
    pub extra: crate::nbt::Compound,
    // Entity.moveDist, flyDist and nextStep: what movement emissions count.
    pub move_dist: f32,
    pub fly_dist: f32,
    pub next_step: f32,
    // Paper.Origin and Paper.OriginWorld, and the spawn reason, as an entity
    // saves them (set when the verifier imports it).
    pub origin: Option<(Vec3, (i64, i64))>,
    pub spawn_reason: &'static str,
    // moonrise$getSectionX/Y/Z (none: Integer.MIN_VALUE) and the chunk status.
    pub section: Option<(i32, i32, i32)>,
    pub chunk_status: Option<ChunkStatus>,
}

impl Entity {
    // Entity.makeStuckInBlock.
    pub fn make_stuck_in_block(&mut self, multiplier: Vec3) {
        self.fall_distance = 0.0;
        self.stuck_speed_multiplier = multiplier;
    }

    // Entity's constructor for a type with these dimensions: the id, then
    // the uuid from the shared random (Mth.createInsecureUUID).
    pub fn new(l: &mut Level, kind: Kind, width: f32, height: f32, eye_height: f32) -> Entity {
        l.entities.counter += 1;
        let id = l.entities.counter;
        let random = &mut l.shared_random;
        let hi = random.next_long() & -61441i64 | 16384i64;
        let lo = random.next_long() & 4611686018427387903i64 | i64::MIN;
        let mut e = Entity {
            id,
            uuid: (hi, lo),
            kind,
            removed: false,
            in_level: false,
            pos: Vec3::ZERO,
            block_pos: Pos::new(0, 0, 0),
            bb: Aabb::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
            width,
            height,
            eye_height,
            delta: Vec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            y_rot_o: 0.0,
            x_rot_o: 0.0,
            xo: 0.0,
            yo: 0.0,
            zo: 0.0,
            x_old: 0.0,
            y_old: 0.0,
            z_old: 0.0,
            on_ground: false,
            horizontal_collision: false,
            vertical_collision: false,
            vertical_collision_below: false,
            minor_horizontal_collision: false,
            fall_distance: 0.0,
            stuck_speed_multiplier: Vec3::ZERO,
            ticks_frozen: 0,
            in_powder_snow: false,
            tick_count: 0,
            first_tick: true,
            no_physics: false,
            has_impulse: false,
            portal_cooldown: 0,
            portal_process: None,
            main_supporting_block: None,
            on_ground_no_blocks: false,
            movement_this_tick: VecDeque::new(),
            piston_deltas: [0.0; 3],
            piston_deltas_game_time: 0,
            was_touching_water: false,
            fluid_height: [0.0, 0.0],
            fluid_on_eyes: [false, false],
            was_eye_in_water: false,
            total_age: 0,
            last_lava_contact: None,
            remaining_fire_ticks: 0,
            air_supply: 300,
            invulnerable: false,
            no_gravity: false,
            extra: crate::nbt::Compound::new(),
            move_dist: 0.0,
            fly_dist: 0.0,
            next_step: 1.0,
            origin: None,
            spawn_reason: "DEFAULT",
            section: None,
            chunk_status: None,
        };
        // Entity's constructor ends with setPos(0, 0, 0).
        e.bb = e.make_bounding_box(Vec3::ZERO);
        e
    }

    // EntityDimensions.makeBoundingBox.
    pub fn make_bounding_box(&self, p: Vec3) -> Aabb {
        let half = self.width / 2.0f32;
        let height = self.height;
        Aabb::new(p.x - half as f64, p.y, p.z - half as f64, p.x + half as f64, p.y + height as f64, p.z + half as f64)
    }

    pub fn eye_y(&self) -> f64 {
        self.pos.y + self.eye_height as f64
    }

    pub fn set_old_pos_and_rot(&mut self) {
        self.xo = self.pos.x;
        self.x_old = self.pos.x;
        self.yo = self.pos.y;
        self.y_old = self.pos.y;
        self.zo = self.pos.z;
        self.z_old = self.pos.z;
        self.y_rot_o = self.y_rot;
        self.x_rot_o = self.x_rot;
    }

    pub fn old_position(&self) -> Vec3 {
        Vec3::new(self.x_old, self.y_old, self.z_old)
    }
}

// moonrise's EntityList: a list with a position per id, whose removal
// moves the last entry into the gap.
#[derive(Default)]
pub struct IdList {
    items: Vec<EntityId>,
    index: FastMap<EntityId, usize>,
}

impl IdList {
    pub fn add(&mut self, id: EntityId) -> bool {
        if self.index.contains_key(&id) {
            return false;
        }
        self.index.insert(id, self.items.len());
        self.items.push(id);
        true
    }

    pub fn remove(&mut self, id: EntityId) -> bool {
        let Some(at) = self.index.remove(&id) else { return false };
        let last = self.items.pop().expect("a list with an entry");
        if at < self.items.len() {
            self.items[at] = last;
            self.index.insert(last, at);
        }
        true
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn ids(&self) -> &[EntityId] {
        &self.items
    }
}

// ServerLevel's EntityTickList: moonrise's IteratorSafeOrderedReferenceSet,
// whose iteration order is the order entities were added in.
#[derive(Default)]
pub struct TickList {
    slots: Vec<Option<EntityId>>,
    index: FastMap<EntityId, usize>,
    iterators: u32,
}

impl TickList {
    pub fn add(&mut self, id: EntityId) -> bool {
        if self.index.contains_key(&id) {
            return false;
        }
        self.index.insert(id, self.slots.len());
        self.slots.push(Some(id));
        true
    }

    pub fn remove(&mut self, id: EntityId) -> bool {
        let Some(at) = self.index.remove(&id) else { return false };
        self.slots[at] = None;
        if self.iterators == 0 {
            self.defrag_if_needed();
        }
        true
    }

    fn defrag_if_needed(&mut self) {
        if 1.0 - self.index.len() as f64 / self.slots.len() as f64 >= 0.2 {
            self.slots.retain(Option::is_some);
            for (at, slot) in self.slots.iter().enumerate() {
                self.index.insert(slot.unwrap(), at);
            }
        }
    }

    // Starts an iteration over the entities there are now.
    pub fn begin(&mut self) -> usize {
        self.iterators += 1;
        self.slots.len()
    }

    // The entity at or after `at`, up to the iteration's end.
    pub fn next(&self, at: &mut usize, end: usize) -> Option<EntityId> {
        let end = end.min(self.slots.len());
        while *at < end {
            let slot = self.slots[*at];
            *at += 1;
            if slot.is_some() {
                return slot;
            }
        }
        None
    }

    pub fn finish(&mut self) {
        self.iterators -= 1;
        if self.iterators == 0 {
            self.defrag_if_needed();
        }
    }
}

// ChunkEntitySlices: a chunk's entities, in all and by section.
pub struct Slices {
    pub status: ChunkStatus,
    entities: IdList,
    sections: Vec<Vec<EntityId>>,
}

const MIN_SECTION: i32 = MIN_Y >> 4;
const SECTIONS: i32 = HEIGHT >> 4;
const MAX_SECTION: i32 = MIN_SECTION + SECTIONS - 1;

impl Slices {
    pub fn new(status: ChunkStatus) -> Slices {
        Slices { status, entities: IdList::default(), sections: (0..SECTIONS).map(|_| Vec::new()).collect() }
    }

    fn add(&mut self, e: &mut Entity, section_y: i32) -> bool {
        if !self.entities.add(e.id) {
            return false;
        }
        e.chunk_status = Some(self.status);
        let list = &mut self.sections[(section_y - MIN_SECTION) as usize];
        if !list.contains(&e.id) {
            list.push(e.id);
        }
        true
    }

    fn remove(&mut self, e: &mut Entity, section_y: i32) -> bool {
        if !self.entities.remove(e.id) {
            return false;
        }
        e.chunk_status = None;
        let list = &mut self.sections[(section_y - MIN_SECTION) as usize];
        if let Some(at) = list.iter().position(|&id| id == e.id) {
            list.remove(at);
        }
        true
    }

    pub fn is_empty(&self) -> bool {
        self.entities.len() == 0
    }
}

pub struct Entities {
    // Entity.ENTITY_COUNTER.
    pub counter: i32,
    // The entities not being ticked (the one that is, is out of this map).
    pub all: FastMap<EntityId, Box<Entity>>,
    // EntityLookup.accessibleEntities, the order the entity list is written in.
    pub accessible: IdList,
    pub tick_list: TickList,
    pub slices: FastMap<i64, Slices>,
    // The item frames in the level.
    pub frames: FastMap<EntityId, ()>,
    // The entity being ticked, which is out of `all` meanwhile.
    pub ticking: Option<Ticking>,
    // What container entities hold, by id: a container entity is searched
    // while it is being ticked or hurt, when it is out of `all`.
    pub containers: FastMap<EntityId, Vec<Stack>>,
}

impl Entities {
    pub fn new() -> Entities {
        Entities { counter: SEEDED_ENTITY_IDS, all: FastMap::default(), accessible: IdList::default(), tick_list: TickList::default(), slices: FastMap::default(), frames: FastMap::default(), ticking: None, containers: FastMap::default() }
    }

    pub fn count(&self) -> usize {
        self.accessible.len()
    }

    // The entity with this id; `me` stands for the one being ticked.
    pub fn get<'a>(&'a self, me: Option<&'a Entity>, id: EntityId) -> &'a Entity {
        match me {
            Some(m) if m.id == id => m,
            _ => self.all.get(&id).unwrap_or_else(|| panic!("no entity {id}")),
        }
    }

    // A new chunk's entity slices (McbenchLevel.chunk).
    pub fn chunk_created(&mut self, x: i32, z: i32, status: ChunkStatus) {
        self.slices.insert(chunk_key(x, z), Slices::new(status));
    }
}

fn section_of(pos: Pos) -> (i32, i32, i32) {
    (pos.x() >> 4, (pos.y() >> 4).clamp(MIN_SECTION, MAX_SECTION), pos.z() >> 4)
}

// EntityLookup.entityStatusChange, with ServerLevel's callbacks.
fn status_change(l: &mut Level, e: &mut Entity, old: Visibility, new: Visibility, created: bool) {
    if created {
        e.set_old_pos_and_rot();
    }
    if old != new {
        if new > old {
            if !old.accessible() && new.accessible() {
                l.entities.accessible.add(e.id);
                // EntityCallbacks.onTrackingStart sets the origin the entity lacks.
                if e.origin.is_none() {
                    e.origin = Some((e.pos, l.world_uuid));
                }
            }
            if !old.ticking() && new.ticking() {
                l.entities.tick_list.add(e.id);
            }
        } else {
            if old.ticking() && !new.ticking() {
                l.entities.tick_list.remove(e.id);
            }
            if old.accessible() && !new.accessible() {
                l.entities.accessible.remove(e.id);
            }
        }
    }
}

// ServerLevel.addFreshEntity of an entity no event cancels: EntityLookup.addEntity.
// The entity goes into the lookup; the caller keeps it (to put it back in
// `all` once it is not being ticked) or lets it go.
pub fn add_fresh_entity(l: &mut Level, mut e: Box<Entity>) {
    let (sx, sy, sz) = section_of(e.block_pos);
    e.section = Some((sx, sy, sz));
    l.chunk(sx, sz);
    let key = chunk_key(sx, sz);
    let slices = l.entities.slices.get_mut(&key).expect("entity slices of a chunk");
    assert!(slices.add(&mut e, sy), "entity added twice");
    e.in_level = true;
    if matches!(e.kind, Kind::Frame(_)) {
        l.entities.frames.insert(e.id, ());
    }
    let new = visibility(e.chunk_status);
    status_change(l, &mut e, Visibility::Hidden, new, true);
    l.entities.all.insert(e.id, e);
}

// The entity's position changed (EntityCallback.onMove): if it is in
// another section it moves to its lists.
pub fn on_move(l: &mut Level, e: &mut Entity) {
    if !e.in_level {
        return;
    }
    let old_visibility = visibility(e.chunk_status);
    let (sx, sy, sz) = e.section.expect("an entity in a section");
    let (nx, ny, nz) = section_of(e.block_pos);
    if (nx, ny, nz) == (sx, sy, sz) {
        return;
    }
    // getOrCreateChunk of the new section's chunk makes the chunk if new.
    l.chunk(nx, nz);
    let old = l.entities.slices.get_mut(&chunk_key(sx, sz)).expect("the old slices");
    assert!(old.remove(e, sy), "entity missing from its section");
    let slices = l.entities.slices.get_mut(&chunk_key(nx, nz)).expect("the new slices");
    assert!(slices.add(e, ny), "entity already in its new section");
    e.section = Some((nx, ny, nz));
    let new = visibility(e.chunk_status);
    status_change(l, e, old_visibility, new, false);
}

// Entity.discard / setRemoved: the entity leaves the lookup. The caller
// drops it afterwards.
pub fn remove(l: &mut Level, e: &mut Entity) {
    // AbstractMinecartContainer.remove: a container's contents drop first.
    match &e.kind {
        Kind::Minecart(c) if c.is_container() => minecart::drop_contents(l, e),
        // AbstractChestBoat.remove likewise.
        Kind::Boat(b) if b.chest => boat::drop_contents(l, e),
        _ => {}
    }
    e.removed = true;
    if let Some(t) = &mut l.entities.ticking {
        if t.id == e.id {
            t.removed = true;
        }
    }
    if !e.in_level {
        return;
    }
    let ticking_state = visibility(e.chunk_status);
    let (sx, sy, sz) = e.section.expect("an entity in a section");
    let slices = l.entities.slices.get_mut(&chunk_key(sx, sz)).expect("the entity's slices");
    assert!(slices.remove(e, sy), "entity missing from its section");
    e.section = None;
    status_change(l, e, ticking_state, Visibility::Hidden, false);
    e.in_level = false;
    if matches!(e.kind, Kind::Frame(_)) {
        l.entities.frames.remove(&e.id);
    }
}

// Slices.updateStatus for a chunk whose status changed (a ticket).
pub fn chunk_status_change(l: &mut Level, x: i32, z: i32, status: ChunkStatus) {
    let key = chunk_key(x, z);
    let ids: Vec<EntityId> = {
        let slices = l.entities.slices.get_mut(&key).expect("slices of a chunk");
        slices.status = status;
        slices.entities.ids().to_vec()
    };
    for id in ids {
        let mut e = l.entities.all.remove(&id).expect("an entity of a chunk");
        let old = visibility(e.chunk_status);
        e.chunk_status = Some(status);
        let new = visibility(e.chunk_status);
        status_change(l, &mut e, old, new, false);
        l.entities.all.insert(id, e);
    }
}

// Which entities a lookup visits: every one, or the Class version's.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Only {
    Any,
    Items,
    Frames,
    // HangingEntity: item frames and paintings.
    Hanging,
    // AbstractMinecart.
    Carts,
    // The entities that are a Container: container minecarts.
    Containers,
    // LivingEntity.
    Living,
    // ExperienceOrb.
    Orbs,
}

// Whether an entity of this kind is of the class a lookup asks for.
pub fn is_class(kind: &Kind, only: Only) -> bool {
    match only {
        Only::Any => true,
        Only::Items => matches!(kind, Kind::Item(_)),
        Only::Frames => matches!(kind, Kind::Frame(_)),
        Only::Hanging => matches!(kind, Kind::Frame(_) | Kind::Painting(_)),
        Only::Carts => matches!(kind, Kind::Minecart(_)),
        Only::Containers => matches!(kind, Kind::Minecart(c) if c.is_container()) || matches!(kind, Kind::Boat(b) if b.chest),
        Only::Living => matches!(kind, Kind::ArmorStand(_) | Kind::Golem(_)),
        Only::Orbs => matches!(kind, Kind::Orb(_)),
    }
}

// EntityLookup.getEntities(Class, null, box, into, predicate) for the item
// entities: those in the box in the lookup's order, for which `accept` holds.
pub fn item_entities_in(l: &Level, me: Option<&Entity>, aabb: &Aabb, accept: impl FnMut(&Entity) -> bool) -> Vec<EntityId> {
    entities_in(l, me, None, Only::Items, aabb, accept)
}

// The ids of the entities in the sections of the chunks around a box, in
// the order the lookup visits them (chunk regions, then each chunk's
// sections from the lowest, each section's list in order).
pub(super) fn ids_around(l: &Level, aabb: &Aabb) -> Vec<EntityId> {
    let mut out = Vec::new();
    let min_chunk_x = phys::floor(aabb.min_x) - 2 >> 4;
    let min_chunk_z = phys::floor(aabb.min_z) - 2 >> 4;
    let max_chunk_x = phys::floor(aabb.max_x) + 2 >> 4;
    let max_chunk_z = phys::floor(aabb.max_z) + 2 >> 4;
    let (min_region_x, min_region_z) = (min_chunk_x >> 5, min_chunk_z >> 5);
    let (max_region_x, max_region_z) = (max_chunk_x >> 5, max_chunk_z >> 5);
    for region_z in min_region_z..=max_region_z {
        let min_z = if region_z == min_region_z { min_chunk_z & 31 } else { 0 };
        let max_z = if region_z == max_region_z { max_chunk_z & 31 } else { 31 };
        for region_x in min_region_x..=max_region_x {
            let min_x = if region_x == min_region_x { min_chunk_x & 31 } else { 0 };
            let max_x = if region_x == max_region_x { max_chunk_x & 31 } else { 31 };
            for z in min_z..=max_z {
                for x in min_x..=max_x {
                    let Some(slices) = l.entities.slices.get(&chunk_key(region_x << 5 | x, region_z << 5 | z)) else { continue };
                    // chunk.status.isOrAfter(FULL): every chunk with slices.
                    let min = (phys::floor(aabb.min_y - 2.0) >> 4).clamp(MIN_SECTION, MAX_SECTION);
                    let max = (phys::floor(aabb.max_y + 2.0) >> 4).clamp(MIN_SECTION, MAX_SECTION);
                    for section in min..=max {
                        out.extend_from_slice(&slices.sections[(section - MIN_SECTION) as usize]);
                    }
                }
            }
        }
    }
    out
}

// EntityLookup.getEntities(except, box, into, predicate) (every entity), or
// the Class version for item entities or item frames: those in the box in
// the lookup's order, but `except`, for which `accept` holds. `me` stands
// for the entity being ticked, which is not among the level's.
pub fn entities_in(l: &Level, me: Option<&Entity>, except: Option<EntityId>, only: Only, aabb: &Aabb, mut accept: impl FnMut(&Entity) -> bool) -> Vec<EntityId> {
    let mut out = Vec::new();
    for id in ids_around(l, aabb) {
        if Some(id) == except {
            continue;
        }
        // The Class versions only visit the entities of that class
        // (an entity out of the level's map is being ticked).
        if only == Only::Frames && !l.entities.frames.contains_key(&id) {
            continue;
        }
        // The entity being ticked is out of the map: a search that does not
        // stand for it knows only its class.
        if me.is_none() {
            if let Some(t) = l.entities.ticking {
                if t.id == id {
                    assert!(t.classes & 1 << only as u8 == 0, "a search reached the entity being ticked");
                    continue;
                }
            }
        }
        let e = l.entities.get(me, id);
        if !is_class(&e.kind, only) {
            continue;
        }
        if e.bb.intersects(aabb) && accept(e) {
            if let Kind::Golem(_) = e.kind {
                golem::unported(e, "is found by a search of the level's entities");
            }
            out.push(id);
        }
    }
    out
}

// What the level knows of the entity being ticked, which is out of its map:
// what a search for minecarts reads of it.
#[derive(Clone, Copy)]
pub struct Ticking {
    pub id: EntityId,
    pub bb: Aabb,
    pub removed: bool,
    // The lookups' classes (Only, one bit each) the entity is of.
    pub classes: u8,
    // What a collision with it reads (collide.rs): whether it is hard
    // colliding, can be collided with, is pushable.
    pub hard: bool,
    pub collided_with: bool,
    pub pushable: bool,
}

// The bits of the lookups' classes an entity of this kind is of.
fn class_bits(kind: &Kind) -> u8 {
    [Only::Any, Only::Items, Only::Frames, Only::Hanging, Only::Carts, Only::Containers, Only::Living, Only::Orbs].into_iter().filter(|only| is_class(kind, *only)).fold(0, |bits, only| bits | 1 << only as u8)
}

// EntityLookup.getEntities(AbstractMinecart, box, predicate): the minecarts in
// the box (all of them, or the container ones), in the lookup's order. The
// entity being ticked is one of them if it is a minecart.
pub fn minecarts_in(l: &Level, aabb: &Aabb, only: Only) -> Vec<EntityId> {
    let mut out = Vec::new();
    for id in ids_around(l, aabb) {
        match l.entities.ticking {
            Some(t) if t.id == id => {
                if t.classes & 1 << only as u8 != 0 && !t.removed && t.bb.intersects(aabb) {
                    out.push(id);
                }
            }
            _ => {
                // An entity a push has taken out of the level (a piston's) is
                // still in the lookup; what kind it is cannot be told here.
                let Some(e) = l.entities.all.get(&id) else { crate::level::refuse("an entity out of the level while a rail looks for minecarts") };
                if matches!(e.kind, Kind::Minecart(_)) && is_class(&e.kind, only) && !e.removed && e.bb.intersects(aabb) {
                    out.push(id);
                }
            }
        }
    }
    out
}

// ServerLevel.tickNonPassenger for an entity that is always active
// (ActivationRange: the ranges are 0, and falling blocks and primed TNT are
// always active).
pub fn tick_non_passenger(l: &mut Level, e: &mut Entity) {
    e.set_old_pos_and_rot();
    e.tick_count += 1;
    e.total_age += 1;
    l.entities.ticking = Some(Ticking {
        id: e.id,
        bb: e.bb,
        removed: e.removed,
        classes: class_bits(&e.kind),
        hard: collide::is_hard_colliding(&e.kind),
        collided_with: collide::can_be_collided_with(e),
        pushable: collide::is_pushable(e),
    });
    match e.kind {
        Kind::Item(_) => item::tick(l, e),
        Kind::Falling(_) => falling::tick(l, e),
        Kind::Tnt(_) => tnt::tick(l, e),
        Kind::Frame(_) | Kind::Painting(_) => frame::tick(l, e),
        Kind::Minecart(_) => minecart::tick(l, e),
        Kind::Boat(_) => boat::tick(l, e),
        Kind::ArmorStand(_) => armor_stand::tick(l, e),
        Kind::Orb(_) => orb::tick(l, e),
        Kind::Golem(_) => golem::unported(e, "ticks"),
    }
    // postTick: handlePortal for an entity that is alive.
    if is_alive(e) {
        motion::handle_portal(e);
    }
    l.entities.ticking = None;
}

// The entity list (McbenchEngine.writeEntities).
pub fn json(l: &Level) -> String {
    use crate::jfmt::{double_to_string as d, float_to_string as f};
    let mut out = String::from("[");
    for (i, &id) in l.entities.accessible.ids().iter().enumerate() {
        let e = &l.entities.all[&id];
        if i > 0 {
            out.push(',');
        }
        let kind = match &e.kind {
            Kind::Item(_) => "minecraft:item",
            Kind::Falling(_) => "minecraft:falling_block",
            Kind::Tnt(_) => "minecraft:tnt",
            Kind::Frame(data) => {
                if data.glow {
                    "minecraft:glow_item_frame"
                } else {
                    "minecraft:item_frame"
                }
            }
            Kind::Minecart(c) => c.class.key(),
            Kind::Boat(b) => b.key.as_str(),
            Kind::Painting(_) => "minecraft:painting",
            Kind::ArmorStand(_) => "minecraft:armor_stand",
            Kind::Orb(_) => "minecraft:experience_orb",
            Kind::Golem(g) => g.golem.key(),
        };
        out.push_str(&format!(
            "{{\"id\":{},\"type\":\"{kind}\",\"pos\":[{},{},{}],\"yaw\":{},\"pitch\":{},\"width\":{},\"height\":{}",
            e.id,
            d(e.pos.x),
            d(e.pos.y),
            d(e.pos.z),
            f(e.y_rot),
            f(e.x_rot),
            f(e.width),
            f(e.height)
        ));
        match &e.kind {
            Kind::Item(data) => {
                if data.stack.count > 0 {
                    out.push_str(&format!(",\"item\":\"{}\",\"count\":{}", l.reg.items[data.stack.item as usize].key, data.stack.count));
                }
            }
            Kind::Falling(data) => out.push_str(&format!(",\"block\":\"{}\"", l.reg.describe(data.state))),
            Kind::Tnt(data) => out.push_str(&format!(",\"block\":\"{}\",\"fuse\":{}", l.reg.describe(data.state), data.fuse)),
            // The frame's rotation, its item and the way it faces.
            Kind::Frame(data) => {
                out.push_str(&format!(",\"rotation\":{}", data.rotation));
                if data.item.count > 0 {
                    out.push_str(&format!(",\"item\":\"{}\",\"count\":{}", l.reg.items[data.item.item as usize].key, data.item.count));
                }
                out.push_str(&format!(",\"facing\":\"{}\"", data.direction.name()));
            }
            // The block a minecart shows.
            Kind::Minecart(c) => out.push_str(&format!(",\"block\":\"{}\"", l.reg.describe(minecart::display_state(l, c)))),
            Kind::Boat(_) | Kind::Orb(_) | Kind::Golem(_) => {}
            Kind::Painting(p) => out.push_str(&format!(",\"facing\":\"{}\",\"variant\":\"{}\"", p.direction.name(), l.reg.paintings[p.variant].key)),
            // The item in the main hand.
            Kind::ArmorStand(s) => {
                if s.equipment[0].count > 0 {
                    out.push_str(&format!(",\"item\":\"{}\",\"count\":{}", l.reg.items[s.equipment[0].item as usize].key, s.equipment[0].count));
                }
            }
        }
        out.push('}');
    }
    out.push(']');
    out
}

// The entities' fields as McbenchEngine.state writes them, in the entity
// list's order (doubles and floats as raw bits in hex).
pub fn state_text(l: &Level) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for &id in l.entities.accessible.ids() {
        let e = &l.entities.all[&id];
        write!(out, "entity {} {} {}", e.id, e.uuid.0, e.uuid.1).unwrap();
        for v in [e.pos.x, e.pos.y, e.pos.z, e.delta.x, e.delta.y, e.delta.z, e.xo, e.yo, e.zo] {
            write!(out, " {:x}", v.to_bits()).unwrap();
        }
        write!(out, " {:x} {:x}", e.y_rot.to_bits(), e.x_rot.to_bits()).unwrap();
        write!(out, " {}{}{}{}{} {} {}", e.on_ground as u8, e.horizontal_collision as u8, e.vertical_collision as u8, e.has_impulse as u8, e.removed as u8, e.tick_count, e.portal_cooldown).unwrap();
        match e.main_supporting_block {
            Some(p) => write!(out, " {},{},{}", p.x(), p.y(), p.z()).unwrap(),
            None => out.push_str(" -"),
        }
        match &e.kind {
            Kind::Item(data) => write!(out, " item {} {} {} {} {}", l.reg.items[data.stack.item as usize].key, data.stack.count, data.age, data.pickup_delay, data.health).unwrap(),
            Kind::Falling(data) => write!(
                out,
                " falling {} {} {}{}{} {:x} {} {},{},{}",
                l.reg.describe(data.state),
                data.time,
                data.drop_item as u8,
                data.cancel_drop as u8,
                data.hurt_entities as u8,
                data.fall_damage_per_distance.to_bits(),
                data.fall_damage_max,
                data.start_pos.x(),
                data.start_pos.y(),
                data.start_pos.z()
            )
            .unwrap(),
            Kind::Tnt(data) => write!(out, " tnt {} {:x} {}", data.fuse, data.explosion_power.to_bits(), l.reg.describe(data.state)).unwrap(),
            Kind::Frame(_) => {}
            Kind::Minecart(c) => {
                write!(out, " cart {} {}{}{} {} {} {:x}", c.class.key(), c.on_rails as u8, c.flipped as u8, e.first_tick as u8, c.vehicle.hurt_time, c.vehicle.hurt_dir, c.vehicle.damage.to_bits()).unwrap();
                match c.class {
                    minecart::Class::Hopper => write!(out, " hopper {}{}", c.enabled as u8, c.consumed_item_this_frame as u8).unwrap(),
                    minecart::Class::Furnace => write!(out, " furnace {} {:x} {:x} {}", c.fuel, c.push.x.to_bits(), c.push.z.to_bits(), c.has_fuel as u8).unwrap(),
                    minecart::Class::Tnt => write!(out, " tntcart {} {:x} {:x} {}", c.fuse, c.explosion_power_base.to_bits(), c.explosion_speed_factor.to_bits(), c.ignited as u8).unwrap(),
                    minecart::Class::Minecart | minecart::Class::Chest => {}
                }
                write!(out, " move {:x} {:x} {:x}", e.move_dist.to_bits(), e.fly_dist.to_bits(), e.next_step.to_bits()).unwrap();
            }
            Kind::Painting(p) => write!(out, " painting {} {} {}", l.reg.paintings[p.variant].key, p.direction.name(), p.check_interval).unwrap(),
            Kind::Boat(b) => {
                let status = |s: Option<boat::Status>| s.map_or("-", |s| s.name());
                write!(
                    out,
                    " boat {} {} {} {:x} {:x} {:x} {:x} {:x} {} {}{} {} {} {:x}",
                    status(b.status),
                    status(b.old_status),
                    e.first_tick as u8,
                    b.water_level.to_bits(),
                    b.land_friction.to_bits(),
                    b.last_yd.to_bits(),
                    b.out_of_control_ticks.to_bits(),
                    b.delta_rotation.to_bits(),
                    b.bubble_time,
                    b.is_above_bubble_column as u8,
                    b.bubble_column_direction_is_down as u8,
                    b.vehicle.hurt_time,
                    b.vehicle.hurt_dir,
                    b.vehicle.damage.to_bits()
                )
                .unwrap();
                write!(out, " move {:x} {:x} {:x}", e.move_dist.to_bits(), e.fly_dist.to_bits(), e.next_step.to_bits()).unwrap();
            }
            Kind::Orb(o) => write!(out, " orb {} {} {} {}", o.value, o.count, o.age, o.health).unwrap(),
            Kind::Golem(_) => {}
            Kind::ArmorStand(s) => {
                write!(out, " stand {:x} {} {} {} {}{}{}{}{}", s.health.to_bits(), s.hurt_time, s.death_time, e.air_supply, s.small as u8, s.show_arms as u8, s.no_base_plate as u8, s.marker as u8, s.invisible as u8).unwrap();
                write!(out, " move {:x} {:x} {:x}", e.move_dist.to_bits(), e.fly_dist.to_bits(), e.next_step.to_bits()).unwrap();
            }
        }
        write!(out, " fd {:x} fire {} {} frozen {} {}", e.fall_distance.to_bits(), e.remaining_fire_ticks, e.was_touching_water as u8, e.ticks_frozen, e.in_powder_snow as u8).unwrap();
        match &e.portal_process {
            None => writeln!(out, " portal -").unwrap(),
            Some(p) => writeln!(out, " portal {} {} {},{},{}", p.time, p.inside_this_tick as u8, p.entry.x(), p.entry.y(), p.entry.z()).unwrap(),
        }
    }
    out
}

// Entity.lavaHurt(pos): what lava does to an entity that stands in it.
pub fn lava_hurt(l: &mut Level, e: &mut Entity, pos: Pos) {
    match e.kind {
        Kind::Item(_) => item::lava_hurt(l, e, pos),
        Kind::Minecart(_) => minecart::lava_hurt_entry(l, e, pos),
        Kind::Boat(_) => boat_lava_hurt(l, e),
        // A falling block takes no damage and primed TNT is fire immune.
        Kind::ArmorStand(_) => armor_stand::lava_hurt(l, e),
        Kind::Orb(_) => orb::lava_hurt(l, e),
        Kind::Golem(_) => golem::unported(e, "is in lava"),
        Kind::Falling(_) | Kind::Tnt(_) | Kind::Frame(_) | Kind::Painting(_) => {}
    }
}

// Entity.hurt(source, amount) for damage no entity causes (what fire and the
// blocks do): whether the entity was hurt.
pub fn hurt(l: &mut Level, e: &mut Entity, dmg: damage::Dmg, amount: f32) -> bool {
    match e.kind {
        Kind::Item(_) => item::hurt(l, e, dmg, amount),
        Kind::Minecart(_) | Kind::Boat(_) => vehicle::hurt(l, e, dmg, amount),
        Kind::ArmorStand(_) => armor_stand::hurt(l, e, dmg, amount),
        Kind::Orb(_) => orb::hurt(l, e, dmg, amount),
        Kind::Golem(_) => golem::unported(e, "is hurt"),
        // FallingBlockEntity.hurtServer marks it hurt and primed TNT's does
        // nothing: both are false.
        Kind::Falling(_) | Kind::Tnt(_) => false,
        Kind::Frame(_) | Kind::Painting(_) => unreachable!("a hanging entity is in no block's effects"),
    }
}

// Entity.isAlive: not removed (a living entity also has health).
pub fn is_alive(e: &Entity) -> bool {
    match &e.kind {
        Kind::ArmorStand(_) => armor_stand::is_alive(e),
        _ => !e.removed,
    }
}

// The kinds the entity trace also writes the saved data of.
pub fn saves_in_trace(kind: &Kind) -> bool {
    matches!(kind, Kind::Minecart(_) | Kind::Boat(_) | Kind::Painting(_))
}

// Entity.lavaHurt() of a boat: the lava damage of 4.0 and its sound's pitch.
fn boat_lava_hurt(l: &mut Level, e: &mut Entity) {
    if item::fire_immune(l, e) {
        return;
    }
    if vehicle::hurt(l, e, damage::Dmg::Lava, 4.0f32) && !minecart::is_silent(e) {
        l.shared_random.next_float();
    }
}
