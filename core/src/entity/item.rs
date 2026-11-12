// ItemEntity: its constructors and tick, Entity.baseTick as an item meets
// it, merging with nearby items; and the drop paths that make them
// (Block.popResource, Block.dropResources).

use super::damage::Dmg;
use super::fluid;
use super::motion::{self, set_pos, overrides};
use super::{add_fresh_entity, item_entities_in, remove, Entity, Kind};
use crate::collision;
use crate::blocks::container::Stack;
use crate::dir::{Axis, Dir, Pos};
use crate::level::{refuse, Level};
use crate::blocks::container::same_item_same_components;
use crate::loot::{self, ItemStack};
use crate::phys::{self, Vec3};
use crate::registry::State;

// ItemEntity's fields (age counts up to the despawn rate).
pub struct ItemData {
    pub stack: ItemStack,
    pub age: i32,
    pub pickup_delay: i32,
    pub health: i32,
    pub despawn_rate: i32,
    // ItemEntity.target (Owner): the one that may pick the item up; items
    // merge only with those that have the same.
    pub target: Option<(i64, i64)>,
}

const INFINITE_PICKUP_DELAY: i32 = 32767;
const INFINITE_LIFETIME: i32 = -32768;
// Spigot's item-despawn-rate.
const ITEM_DESPAWN_RATE: i32 = 6000;
// Spigot's merge-radius.item.
const ITEM_MERGE: f64 = 0.5;

fn data(e: &Entity) -> &ItemData {
    match &e.kind {
        Kind::Item(d) => d,
        _ => panic!("not an item entity"),
    }
}

fn data_mut(e: &mut Entity) -> &mut ItemData {
    match &mut e.kind {
        Kind::Item(d) => d,
        _ => panic!("not an item entity"),
    }
}

impl ItemData {
    fn is_empty(&self) -> bool {
        self.stack.count <= 0
    }
}

// ItemEntity(Level, x, y, z, stack) and, with a delta, the longer
// constructor: every draw from the shared random, in order. Not added yet.
pub(super) fn new_item(l: &mut Level, x: f64, y: f64, z: f64, stack: ItemStack, delta: Option<Vec3>) -> Box<Entity> {
    let placeholder = ItemData { stack: ItemStack::empty(), age: 0, pickup_delay: 0, health: 5, despawn_rate: ITEM_DESPAWN_RATE, target: None };
    let mut e = Box::new(Entity::new(l, Kind::Item(placeholder), 0.25f32, 0.25f32, 0.2125f32));
    // bobOffs = nextFloat() * PI * 2, then setYRot(nextFloat() * 360).
    let _bob = l.shared_random.next_float() * std::f32::consts::PI * 2.0f32;
    e.y_rot = l.shared_random.next_float() * 360.0f32;
    set_pos(l, &mut e, x, y, z);
    e.delta = match delta {
        Some(d) => d,
        None => {
            let dx = l.shared_random.next_double() * 0.2 - 0.1;
            let dz = l.shared_random.next_double() * 0.2 - 0.1;
            Vec3::new(dx, 0.2, dz)
        }
    };
    set_item(&mut e, stack);
    e
}

// ItemEntity(EntityType, Level): what the constructor draws, for an entity
// whose saved data is loaded next.
pub(super) fn blank_item(l: &mut Level) -> Box<Entity> {
    let placeholder = ItemData { stack: ItemStack::empty(), age: 0, pickup_delay: 0, health: 5, despawn_rate: ITEM_DESPAWN_RATE, target: None };
    let mut e = Box::new(Entity::new(l, Kind::Item(placeholder), 0.25f32, 0.25f32, 0.2125f32));
    // bobOffs = nextFloat() * PI * 2, then setYRot(nextFloat() * 360).
    let _bob = l.shared_random.next_float() * std::f32::consts::PI * 2.0f32;
    e.y_rot = l.shared_random.next_float() * 360.0f32;
    e
}

// ItemEntity.readAdditionalSaveData's results: health, age, pickup delay, the
// owner and the stack (setItem sets the despawn rate).
pub(super) fn set_loaded(e: &mut Entity, stack: ItemStack, health: i32, age: i32, pickup_delay: i32, target: Option<(i64, i64)>) {
    {
        let d = data_mut(e);
        d.health = health;
        d.age = age;
        d.pickup_delay = pickup_delay;
        d.target = target;
    }
    set_item(e, stack);
}

// ItemEntity.setItem: the despawn rate is the configured one.
pub fn set_item(e: &mut Entity, stack: ItemStack) {
    let d = data_mut(e);
    d.stack = stack;
    d.despawn_rate = ITEM_DESPAWN_RATE;
}

// ItemEntity.setDefaultPickUpDelay.
pub(super) fn set_default_pickup_delay(e: &mut Entity) {
    data_mut(e).pickup_delay = 10;
}

// Block.popResource(level, pos, stack).
pub fn pop_resource(l: &mut Level, pos: Pos, stack: ItemStack) {
    // EntityType.ITEM.getHeight() / 2.0.
    let d = 0.25f32 as f64 / 2.0;
    let offset = |l: &mut Level| l.random.next_double() * 0.5 + -0.25;
    let d1 = pos.x() as f64 + 0.5 + offset(l);
    let d2 = pos.y() as f64 + 0.5 + offset(l) - d;
    let d3 = pos.z() as f64 + 0.5 + offset(l);
    // The position is drawn before popResource checks the stack is empty.
    if stack.count <= 0 {
        return;
    }
    let mut e = new_item(l, d1, d2, d3, stack, None);
    // setDefaultPickUpDelay.
    data_mut(&mut e).pickup_delay = 10;
    add_fresh_entity(l, e);
}

// Containers.dropItemStack: the stack comes out in pieces of 10 to 30, each an
// item entity with the random velocity of a dropped container's contents.
// The position is drawn from the level's random before the stack is looked at.
pub fn drop_item_stack(l: &mut Level, x: f64, y: f64, z: f64, stack: &Stack) {
    // EntityType.ITEM.getWidth().
    let d = 0.25f32 as f64;
    let d1 = 1.0 - d;
    let d2 = d / 2.0;
    let d3 = phys::floor(x) as f64 + l.random.next_double() * d1 + d2;
    let d4 = phys::floor(y) as f64 + l.random.next_double() * d1;
    let d5 = phys::floor(z) as f64 + l.random.next_double() * d1 + d2;
    while stack.borrow().count > 0 {
        let size = l.random.next_int_bound(21) + 10;
        let split = crate::blocks::container::split(stack, size);
        let piece = split.borrow().clone();
        let mut e = new_item(l, d3, d4, d5, piece, None);
        let dx = l.random.triangle(0.0, 0.11485000171139836);
        let dy = l.random.triangle(0.2, 0.11485000171139836);
        let dz = l.random.triangle(0.0, 0.11485000171139836);
        e.delta = Vec3::new(dx, dy, dz);
        add_fresh_entity(l, e);
    }
}

// DefaultDispenseItemBehavior.spawnItem(level, stack, speed, facing,
// position, blockSource) when no BlockDispenseEvent listener cancels or
// changes it: the item entity leaves the position with the dispenser's
// random velocity.
pub fn spawn_dispensed(l: &mut Level, stack: ItemStack, speed: i32, facing: Dir, position: Vec3) {
    let y = if facing.axis() == Axis::Y { position.y - 0.125 } else { position.y - 0.15625 };
    let mut e = new_item(l, position.x, y, position.z, stack, None);
    let d3 = l.random.next_double() * 0.1 + 0.2;
    let (step_x, _, step_z) = facing.offset();
    let deviation = 0.0172275 * speed as f64;
    let dx = l.random.triangle(step_x as f64 * d3, deviation);
    let dy = l.random.triangle(0.2, deviation);
    let dz = l.random.triangle(step_z as f64 * d3, deviation);
    e.delta = Vec3::new(dx, dy, dz);
    add_fresh_entity(l, e);
}

// The verifier's drop input: ItemEntity(level, x, y, z, stack, dx, dy, dz)
// with setDefaultPickUpDelay, added to the level. Returns its uuid.
pub fn supply_item(l: &mut Level, x: f64, y: f64, z: f64, stack: ItemStack, delta: Vec3) -> (i64, i64) {
    let mut e = new_item(l, x, y, z, stack, Some(delta));
    data_mut(&mut e).pickup_delay = 10;
    let uuid = e.uuid;
    add_fresh_entity(l, e);
    uuid
}

// An item entity of the stack with the default pickup delay, added to the
// level (LecternBlockEntity.preRemoveSideEffects, HangingEntity.spawnAtLocation).
pub fn spawn_with_default_delay(l: &mut Level, x: f64, y: f64, z: f64, stack: ItemStack) {
    let mut e = new_item(l, x, y, z, stack, None);
    data_mut(&mut e).pickup_delay = 10;
    add_fresh_entity(l, e);
}

// An item entity made and offered to the level with an empty stack, which
// ServerLevel.addEntity rejects: only its making shows (an id, the shared
// random's draws).
pub fn reject_empty_item(l: &mut Level, x: f64, y: f64, z: f64) {
    let mut e = new_item(l, x, y, z, ItemStack::empty(), None);
    data_mut(&mut e).pickup_delay = 10;
}

// ItemStack.onDestroyed: BundleItem drops what a bundle holds and BlockItem
// what a container item holds (ItemUtils.onContainerDestroyed: an item
// entity of each, at the destroyed one's position).
fn on_destroyed(l: &mut Level, e: &Entity) {
    let stack = data(e).stack.clone();
    let reg = l.reg;
    let key = if reg.items[stack.item as usize].block.is_some() {
        "minecraft:container"
    } else if reg.item_component(stack.item, "minecraft:bundle_contents").is_some() {
        "minecraft:bundle_contents"
    } else {
        return;
    };
    let Some(contents) = stack.component(reg, key) else { return };
    let crate::nbt::Tag::List(entries) = contents else { return };
    let mut dropped: Vec<ItemStack> = Vec::new();
    for entry in &entries {
        let tag = if key == "minecraft:container" {
            match crate::codec::map(entry).and_then(|m| crate::codec::required(m, "item")) {
                Ok(item) => item.clone(),
                Err(fail) => refuse(format!("the contents of a destroyed item: {}", fail.0)),
            }
        } else {
            entry.clone()
        };
        match crate::components::decode_stack(reg, &tag) {
            Ok(stack) => dropped.push(stack),
            Err(fail) => refuse(format!("the contents of a destroyed item: {}", fail.0)),
        }
    }
    for stack in dropped {
        let dropped = new_item(l, e.pos.x, e.pos.y, e.pos.z, stack, None);
        add_fresh_entity(l, dropped);
    }
}

// Two classes have a getDrops of their own: the decorated pot's (below) and
// the beehive's, which empties the hive of its bees for an explosion's TNT
// before the loot table rolls; a hive here never holds bees (one loaded with
// any refuses), so its drops are the table's.

// Block.getDrops(state, level, pos, blockEntity...) with no tool, no entity.
pub(crate) fn get_drops(l: &mut Level, state: State, pos: Pos, this_entity: bool) -> Vec<ItemStack> {
    let reg = l.reg;
    // The loot context's block entity (Level.getBlockEntity makes it if
    // missing), as its collected components.
    let components = if l.has_block_entity(state) {
        let own = l.block_entity_extras.get(&pos).map(|extras| extras.components.clone());
        l.block_entity(pos).map(|entity| crate::blocks::collect_components(reg, entity, own.as_ref()))
    } else {
        None
    };
    // MovingPistonBlock.getDrops: the moving piston at the loot's origin
    // gives the drops of the block it moves (BlockState.getDrops with the
    // same parameters, so the loot's block entity stays the moving piston's),
    // and none without its block entity.
    if crate::blocks::moving_piston::is_moving_piston(l, state) {
        let moved = match l.block_entity(pos) {
            Some(crate::blocks::BlockEntity::Piston(moving)) => moving.moved_state(),
            _ => return Vec::new(),
        };
        if crate::blocks::moving_piston::is_moving_piston(l, moved) {
            refuse("the drops of a moving piston that moves a moving piston (MovingPistonBlock.getDrops calls itself)");
        }
        return state_drops(l, moved, pos, components, this_entity);
    }
    state_drops(l, state, pos, components, this_entity)
}

// BlockState.getDrops(params) of a state at a position, the loot's block
// entity given as its collected components.
fn state_drops(l: &mut Level, state: State, pos: Pos, components: Option<crate::nbt::Compound>, this_entity: bool) -> Vec<ItemStack> {
    let reg = l.reg;
    let class = reg.blocks[l.block(state) as usize].class.as_str();
    // LiquidBlock.getDrops: Collections.emptyList().
    if class == "LiquidBlock" {
        return Vec::new();
    }
    // The loot's location checks read the level, so the random sequences are
    // taken out of it while the table rolls.
    let mut sequences = std::mem::replace(&mut l.sequences, loot::RandomSequences::new(0));
    // DecoratedPotBlock.getDrops: a pot's block entity gives its sherds
    // (PotDecorations.ordered, each item's default instance) as the dynamic
    // drop "sherds".
    let dynamic: Vec<(&str, Vec<ItemStack>)> = match l.block_entities.get(&pos) {
        Some(crate::blocks::BlockEntity::Entity(crate::blocks::entities::Entity::DecoratedPot(pot))) if class == "DecoratedPotBlock" => {
            vec![("minecraft:sherds", pot.sherds.iter().map(|sherd| ItemStack::new(reg.item_by_key[sherd.as_str()], 1)).collect())]
        }
        _ => Vec::new(),
    };
    let drops = loot::block_drops(reg, &mut sequences, state, components.as_ref(), &dynamic, this_entity, &mut |dx, dy, dz| l.get_if_loaded(Pos::new(pos.x() + dx, pos.y() + dy, pos.z() + dz)));
    l.sequences = sequences;
    drops
}

// Block.spawnAfterBreak(state, level, pos, EMPTY, dropExperience), the
// class's own: the classes with experience (DropExperienceBlock, SculkBlock,
// RedStoneOreBlock, the sculk sensors, shrieker and catalyst, SpawnerBlock)
// only call Block's, which does nothing (Paper gives their experience through
// getExpDrop); an infested block spawns a silverfish, a mob this port does
// not model.
fn class_spawn_after_break(l: &Level, state: State) {
    let class = l.reg.blocks[l.block(state) as usize].class.as_str();
    if overrides(l, state, "spawnAfterBreak") && !EXPERIENCE_CLASSES.contains(&class) {
        refuse(format!("{class}.spawnAfterBreak ({})", l.reg.describe(state)));
    }
}

// The classes whose spawnAfterBreak only calls Block's, and whose getExpDrop
// gives experience.
const EXPERIENCE_CLASSES: [&str; 8] = ["DropExperienceBlock", "SculkBlock", "RedStoneOreBlock", "SculkSensorBlock", "CalibratedSculkSensorBlock", "SculkShriekerBlock", "SculkCatalystBlock", "SpawnerBlock"];

// DropExperienceBlock's experience (its xpRange), by block: the minimum and
// the maximum of the IntProvider (a ConstantInt has them equal).
const DROP_EXPERIENCE: [(&str, i32, i32); 16] = [
    ("minecraft:gold_ore", 0, 0),
    ("minecraft:deepslate_gold_ore", 0, 0),
    ("minecraft:iron_ore", 0, 0),
    ("minecraft:deepslate_iron_ore", 0, 0),
    ("minecraft:coal_ore", 0, 2),
    ("minecraft:deepslate_coal_ore", 0, 2),
    ("minecraft:nether_gold_ore", 0, 1),
    ("minecraft:lapis_ore", 2, 5),
    ("minecraft:deepslate_lapis_ore", 2, 5),
    ("minecraft:diamond_ore", 3, 7),
    ("minecraft:deepslate_diamond_ore", 3, 7),
    ("minecraft:emerald_ore", 3, 7),
    ("minecraft:deepslate_emerald_ore", 3, 7),
    ("minecraft:nether_quartz_ore", 2, 5),
    ("minecraft:copper_ore", 0, 0),
    ("minecraft:deepslate_copper_ore", 0, 0),
];

// Block.getExpDrop(state, level, pos, EMPTY, true) (Paper's): the experience a
// block broken without a tool gives. Block.tryDropExperience samples the
// class's IntProvider from the level's random (a ConstantInt draws nothing,
// a UniformInt draws Mth.randomBetweenInclusive) and passes it through
// EnchantmentHelper.processBlockExperience, which an empty stack leaves as it
// is; a spawner draws its own.
pub(crate) fn exp_drop(l: &mut Level, state: State) -> i32 {
    let block = &l.reg.blocks[l.block(state) as usize];
    let (min, max) = match block.class.as_str() {
        "DropExperienceBlock" => {
            let &(_, min, max) = DROP_EXPERIENCE.iter().find(|(key, _, _)| *key == block.key).unwrap_or_else(|| panic!("the experience of {}", block.key));
            (min, max)
        }
        "SculkBlock" => (1, 1),
        "RedStoneOreBlock" => (1, 5),
        "SculkSensorBlock" | "CalibratedSculkSensorBlock" | "SculkShriekerBlock" | "SculkCatalystBlock" => (5, 5),
        "SpawnerBlock" => return 15 + l.random.next_int_bound(15) + l.random.next_int_bound(15),
        _ => return 0,
    };
    let sampled = if min == max { min } else { l.random.next_int_bound(max - min + 1) + min };
    sampled.max(0)
}

// Block.popExperience(level, pos, amount, entity): the orbs at the block's
// center (doBlockDrops is on).
pub(crate) fn pop_experience(l: &mut Level, pos: Pos, amount: i32, trigger: Option<(i64, i64)>) {
    super::orb::award(l, super::orb::center_of(pos), amount, "BLOCK_BREAK", trigger);
}

// BlockStateBase.spawnAfterBreak(level, pos, EMPTY, false), as an explosion's
// blocks meet it (no player caused the explosion): the class's own.
pub(crate) fn spawn_after_break_no_experience(l: &mut Level, state: State) {
    class_spawn_after_break(l, state);
}

// ItemEntity.hurtServer of an explosion's damage (the entity `e` is not in
// the level's map). Nothing resists it but a nether star; the damage is never
// zero.
pub(crate) fn hurt_by_explosion(l: &mut Level, e: &mut Entity, amount: f32) {
    // isInvulnerableToBase: an invulnerable entity takes no explosion damage.
    if e.invulnerable {
        return;
    }
    // canBeHurtBy: DAMAGE_RESISTANT names the damage type tag the item resists.
    match l.reg.items[data(e).stack.item as usize].damage_resistant.as_deref() {
        Some("minecraft:is_explosion") => return,
        None | Some("minecraft:is_fire") => {}
        Some(other) => refuse(format!("an item that resists {other}")),
    }
    let d = data_mut(e);
    d.health = (d.health as f32 - amount) as i32;
    let destroyed = d.health <= 0;
    // gameEvent(ENTITY_DAMAGE, damageSource.getEntity()): the explosion's
    // causing entity is the owner of the TNT, which is none here.
    let event = crate::game_event!(l, "entity_damage");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(None));
    if destroyed {
        on_destroyed(l, e);
        discard(l, e);
    }
}

// Block.dropResources(state, level, pos) and the (state, level, pos,
// blockEntity) form: the drops pop out, then BlockStateBase.spawnAfterBreak
// with experience (the class's own, then popExperience of getExpDrop).
pub fn drop_resources(l: &mut Level, state: State, pos: Pos) {
    for stack in get_drops(l, state, pos, false) {
        pop_resource(l, pos, stack);
    }
    class_spawn_after_break(l, state);
    let xp = exp_drop(l, state);
    pop_experience(l, pos, xp, None);
}

// Block.dropResources(state, level, pos, blockEntity, entity, EMPTY, false),
// Level.destroyBlock's drop (which took the experience before and pops it
// after): the drops pop out, then the class's spawnAfterBreak.
pub fn drop_resources_no_experience(l: &mut Level, state: State, pos: Pos, this_entity: bool) {
    for stack in get_drops(l, state, pos, this_entity) {
        pop_resource(l, pos, stack);
    }
    class_spawn_after_break(l, state);
}

// Paper's Block.dropResources(state, level, pos, blockEntity, source), what a
// piston and flowing water destroy: the drops, the experience
// (BlockBreakBlockEvent has no listener), the drops popped out, the class's
// spawnAfterBreak without experience, then the experience's orbs.
pub fn drop_resources_by_block(l: &mut Level, state: State, pos: Pos) {
    let drops = get_drops(l, state, pos, false);
    let xp = exp_drop(l, state);
    for stack in drops {
        pop_resource(l, pos, stack);
    }
    class_spawn_after_break(l, state);
    pop_experience(l, pos, xp, None);
}

// --- Ticking ---

fn is_mergable(l: &Level, e: &Entity) -> bool {
    let d = data(e);
    !e.removed && d.pickup_delay != INFINITE_PICKUP_DELAY && d.age != INFINITE_LIFETIME && d.age < d.despawn_rate && d.stack.count < d.stack.max_stack_size(l.reg)
}

// ItemEntity.discard(cause): the item leaves the level.
pub fn discard(l: &mut Level, e: &mut Entity) {
    remove(l, e);
}

// ItemStack.canBeHurtBy(a fire damage source): DAMAGE_RESISTANT names the
// damage type tag the item resists, and fire (in_fire, on_fire, lava) is in
// is_fire only.
fn can_be_hurt_by_fire(l: &Level, e: &Entity) -> bool {
    match l.reg.items[data(e).stack.item as usize].damage_resistant.as_deref() {
        None | Some("minecraft:is_explosion") => true,
        Some("minecraft:is_fire") => false,
        Some(other) => refuse(format!("an item that resists {other}")),
    }
}

// Entity.fireImmune: an item fire cannot hurt (the item entity type is not
// fire immune), primed TNT (its type is), never a falling block.
pub fn fire_immune(l: &Level, e: &Entity) -> bool {
    match e.kind {
        Kind::Item(_) => !can_be_hurt_by_fire(l, e),
        Kind::Tnt(_) => true,
        // (EntityType.fireImmune of an orb's type and the golems' is false.)
        Kind::Falling(_) | Kind::Frame(_) | Kind::Minecart(_) | Kind::Boat(_) | Kind::Painting(_) | Kind::ArmorStand(_) | Kind::Orb(_) | Kind::Golem(_) => false,
    }
}

// Entity.isOnFire (on the server).
pub fn is_on_fire(l: &Level, e: &Entity) -> bool {
    !fire_immune(l, e) && e.remaining_fire_ticks > 0
}

// ItemEntity.hurtServer for damage no entity causes (fire of every kind, and the
// cactus; the damage event has no listener, no damage here is zero and the
// source is no mob's): whether it hurt.
pub fn hurt(l: &mut Level, e: &mut Entity, dmg: Dmg, amount: f32) -> bool {
    assert!(dmg.is_fire() || dmg == Dmg::Cactus, "damage an item takes from a block or fire");
    // isInvulnerableToBase: removed, invulnerable, or a fire source and fireImmune.
    if e.removed || e.invulnerable || (dmg.is_fire() && fire_immune(l, e)) {
        return false;
    }
    // canBeHurtBy: DAMAGE_RESISTANT covers fire (fire_immune above) and
    // explosions; no item resists the cactus.
    let d = data_mut(e);
    d.health = (d.health as f32 - amount) as i32;
    // gameEvent(ENTITY_DAMAGE, damageSource.getEntity()): a block's or fire's damage has no entity.
    let event = crate::game_event!(l, "entity_damage");
    l.game_event(event, e.pos, crate::game_event::Context::of_entity(None));
    if data(e).health <= 0 {
        on_destroyed(l, e);
        discard(l, e);
    }
    true
}

// Entity.lavaHurt(pos): lava's damage, and a burn sound for the item every
// tenth tick or once it is dead (the pitch draws from the shared random).
pub fn lava_hurt(l: &mut Level, e: &mut Entity, _pos: Pos) {
    // A falling block takes no damage (hurtServer is false) and primed TNT is
    // fire immune: no sound.
    if !matches!(e.kind, Kind::Item(_)) {
        return;
    }
    if !fire_immune(l, e) && hurt(l, e, Dmg::Lava, 4.0) && (data(e).health <= 0 || e.tick_count % 10 == 0) {
        l.shared_random.next_float();
    }
}

// Entity.baseTick for an entity in the level of items.
fn base_tick(l: &mut Level, e: &mut Entity) {
    e.in_powder_snow = false;
    fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    fluid::update_fluid_on_eyes(l, e);
    if e.remaining_fire_ticks > 0 {
        if fire_immune(l, e) {
            fluid::clear_fire(e);
        } else {
            if e.remaining_fire_ticks % 20 == 0 && !fluid::is_in_lava(e) {
                hurt(l, e, Dmg::OnFire, 1.0);
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
        discard(l, e);
    }
    e.first_tick = false;
}

pub fn tick(l: &mut Level, e: &mut Entity) {
    if data(e).is_empty() {
        discard(l, e);
        return;
    }
    base_tick(l, e);
    {
        let d = data_mut(e);
        if d.pickup_delay > 0 && d.pickup_delay != INFINITE_PICKUP_DELAY {
            d.pickup_delay -= 1;
        }
    }
    e.xo = e.pos.x;
    e.yo = e.pos.y;
    e.zo = e.pos.z;
    let delta_movement = e.delta;
    if e.was_touching_water && fluid::water_height(e) > 0.1f32 as f64 {
        // setUnderwaterMovement.
        set_fluid_movement(e, 0.99f32 as f64);
    } else if fluid::is_in_lava(e) && fluid::lava_height(e) > 0.1f32 as f64 {
        // setUnderLavaMovement.
        set_fluid_movement(e, 0.95f32 as f64);
    } else {
        // applyGravity: Entity.getGravity is 0 for an entity without gravity.
        if !e.no_gravity {
            e.delta = e.delta.add(0.0, -0.04, 0.0);
        }
    }
    e.no_physics = !super::collide::no_collision(l, e, &e.bb.deflate(1.0E-7));
    if e.no_physics {
        motion::move_towards_closest_space(l, e, e.pos.x, (e.bb.min_y + e.bb.max_y) / 2.0, e.pos.z);
    }
    if !e.on_ground || e.delta.horizontal_distance_sqr() > 1.0E-5f32 as f64 || (e.tick_count + e.id) % 4 == 0 {
        motion::move_entity(l, e, e.delta);
        motion::apply_effects_from_blocks(l, e);
        let mut f = 0.98f32;
        if e.on_ground {
            let below = motion::block_pos_below(l, e);
            let state = l.get(below);
            f = l.reg.blocks[l.block(state) as usize].friction * 0.98f32;
        }
        e.delta = e.delta.multiply(f as f64, 0.98, f as f64);
        if e.on_ground {
            let d = e.delta;
            if d.y < 0.0 {
                e.delta = d.multiply(1.0, -0.5, 1.0);
            }
        }
    }
    let moved = phys::floor(e.xo) != phys::floor(e.pos.x) || phys::floor(e.yo) != phys::floor(e.pos.y) || phys::floor(e.zo) != phys::floor(e.pos.z);
    let interval = if moved { 2 } else { 40 };
    if e.tick_count % interval == 0 && is_mergable(l, e) {
        merge_with_neighbours(l, e);
    }
    {
        let d = data_mut(e);
        if d.age != INFINITE_LIFETIME {
            d.age += 1;
        }
    }
    e.has_impulse |= fluid::update_in_water_state_and_do_fluid_pushing(l, e);
    let change = e.delta.subtract(delta_movement).length_sqr();
    if change > 0.01 {
        e.has_impulse = true;
    }
    let d = data(e);
    if d.age >= d.despawn_rate {
        discard(l, e);
    }
}

// ItemEntity.setFluidMovement.
fn set_fluid_movement(e: &mut Entity, multiplier: f64) {
    let dm = e.delta;
    e.delta = Vec3::new(dm.x * multiplier, dm.y + (if dm.y < 0.06f32 as f64 { 5.0E-4f32 } else { 0.0f32 }) as f64, dm.z * multiplier);
}

// ItemEntity.mergeWithNeighbours.
fn merge_with_neighbours(l: &mut Level, e: &mut Entity) {
    if !is_mergable(l, e) {
        return;
    }
    let area = e.bb.inflate(ITEM_MERGE, ITEM_MERGE - 0.5, ITEM_MERGE);
    let me = e.id;
    let neighbours = item_entities_in(l, Some(e), &area, |n| n.id != me && is_mergable(l, n));
    for id in neighbours {
        let other = l.entities.all.get(&id).expect("a neighbour item");
        if !is_mergable(l, other) {
            continue;
        }
        try_to_merge(l, e, id);
        if e.removed {
            break;
        }
    }
}

// ItemEntity.tryToMerge(other).
fn try_to_merge(l: &mut Level, this: &mut Entity, other_id: i32) {
    let mut other = l.entities.all.remove(&other_id).expect("a neighbour item");
    let (a, b) = (data(this).stack.clone(), data(&other).stack.clone());
    // Objects.equals(target, target), then areMergable(item, item1): the stack
    // sizes add up to a stack, same item.
    if data(this).target == data(&other).target && b.count + a.count <= b.max_stack_size(l.reg) && same_item_same_components(&a, &b) {
        if b.count < a.count {
            merge_entities(l, this, &mut other);
        } else {
            merge_entities(l, &mut other, this);
        }
    }
    if other.removed {
        drop(other);
    } else {
        l.entities.all.insert(other_id, other);
    }
}

// ItemEntity.merge(destinationEntity, stack, originEntity, stack): ItemMergeEvent
// has no listener.
fn merge_entities(l: &mut Level, dest: &mut Entity, origin: &mut Entity) {
    let dest_max = data(dest).stack.max_stack_size(l.reg);
    // merge(destinationStack, originStack, 64): the destination's stack
    // copied with the count it grows to.
    let moved = (dest_max.min(64) - data(dest).stack.count).min(data(origin).stack.count);
    let mut merged = data(dest).stack.clone();
    merged.count += moved;
    data_mut(origin).stack.count -= moved;
    set_item(dest, merged);
    let pickup = data(dest).pickup_delay.max(data(origin).pickup_delay);
    data_mut(dest).pickup_delay = pickup;
    let age = data(dest).age.min(data(origin).age);
    data_mut(dest).age = age;
    if data(origin).stack.count <= 0 {
        discard(l, origin);
    }
}
