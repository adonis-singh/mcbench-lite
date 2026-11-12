// DispenserBlock.getDispenseMethod and the DispenseItemBehaviors the server
// registers (DispenseItemBehavior.bootStrap), for a dispenser at a position:
// what each does to the world and to the stack. No plugin listens to the
// events they call (BlockDispenseEvent, BlockDispenseArmorEvent, BlockIgniteEvent,
// TNTPrimeEvent, BlockFertilizeEvent...), so each stands as it is made; a
// level event (a sound, a particle) reaches nothing here.
//
// This level has no living entities, no boats and no minecarts, no
// projectiles and no experience orbs. Where a behaviour looks for living
// entities in a block (armor, a chest for a horse, shears, a brush) it finds
// none, which is its code's own answer for none there. Where it would spawn
// an entity this port does not model (a projectile, a boat, a minecart, an
// armor stand, a mob), or place a block it cannot, it refuses.

use super::container::{self, count, is_empty, Cont, Stack};
use super::dispenser::{self, facing};
use crate::dir::{Dir, Pos};
use crate::game_event::Context;
use crate::level::{refuse, Level, UPDATE_ALL, UPDATE_CLIENTS};
use crate::loot::ItemStack;
use crate::nbt::Tag;
use crate::phys::{Aabb, Vec3};
use crate::registry::{Fluid, State};

// DispenserBlock.getDispenseMethod(level, item).dispense(blockSource, item):
// the stack that goes back into the slot.
pub fn dispense_method(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let reg = l.reg;
    let info = &reg.items[item.borrow().item as usize];
    // An item whose feature is disabled gets the default behaviour.
    if !info.dispense_enabled {
        return dispenser::dispense(l, state, pos, item);
    }
    let key = info.key.as_str();
    match info.dispense_behaviour.as_deref() {
        Some("net.minecraft.core.dispenser.DefaultDispenseItemBehavior") => dispenser::dispense(l, state, pos, item),
        Some("net.minecraft.core.dispenser.BoatDispenseItemBehavior") => boat(l, state, pos, item),
        Some("net.minecraft.core.dispenser.MinecartDispenseItemBehavior") => minecart(l, state, pos, item),
        Some("net.minecraft.core.dispenser.ProjectileDispenseBehavior") => refuse(format!("dispensing {key} (a projectile entity)")),
        Some("net.minecraft.core.dispenser.ShearsDispenseItemBehavior") => shears(l, state, pos, item),
        Some("net.minecraft.core.dispenser.ShulkerBoxDispenseBehavior") => shulker_box(l, state, pos, item),
        // The anonymous behaviours of DispenseItemBehavior.bootStrap, by their
        // number in the order they are written.
        Some(class) => match class.strip_prefix("net.minecraft.core.dispenser.DispenseItemBehavior$") {
            // Spawn eggs and armor stands spawn an entity.
            Some("1") => refuse(format!("dispensing {key} (a spawn egg)")),
            Some("2") => refuse(format!("dispensing {key} (an armor stand)")),
            // A chest: tamed chested horses without a chest are looked for, none exist.
            Some("3") => dispenser::dispense(l, state, pos, item),
            Some("4") => full_bucket(l, state, pos, item),
            Some("5") => empty_bucket(l, state, pos, item),
            Some("6") => flint_and_steel(l, state, pos, item),
            Some("7") => bone_meal(l, state, pos, item),
            Some("8") => tnt(l, state, pos, item),
            Some("9") => wither_skull(l, state, pos, item),
            Some("10") => carved_pumpkin(l, state, pos, item),
            Some("11") => glass_bottle(l, state, pos, item),
            Some("12") => glowstone(l, state, pos, item),
            // A brush looks for armadillos, none exist.
            Some("13") => item.clone(),
            Some("14") => honeycomb(l, state, pos, item),
            Some("15") => potion(l, state, pos, item),
            _ => refuse(format!("dispenser behaviour {class} ({key})")),
        },
        // An equippable item looks for a living entity to wear it, finds none
        // and is dispensed as it is (EquipmentDispenseItemBehavior).
        None => dispenser::dispense(l, state, pos, item),
    }
}

// --- Helpers ---

// BlockSource.pos().relative(facing).
fn front(l: &Level, state: State, pos: Pos) -> Pos {
    pos.relative(facing(l, state))
}

fn key_of(l: &Level, stack: &ItemStack) -> &'static str {
    &l.reg.items[stack.item as usize].key
}

fn is_block(l: &Level, state: State, key: &str) -> bool {
    l.reg.blocks[l.block(state) as usize].key == key
}

// DefaultDispenseItemBehavior.consumeWithRemainder: the stack shrinks; when
// that empties it the remainder takes its place, else the remainder goes into
// the dispenser (or out of it).
fn consume_with_remainder(l: &mut Level, state: State, pos: Pos, stack: &Stack, remainder: ItemStack) -> Stack {
    container::shrink(stack, 1);
    if is_empty(stack) {
        container::stack_of(remainder)
    } else {
        add_to_inventory_or_dispense(l, state, pos, remainder);
        stack.clone()
    }
}

// DefaultDispenseItemBehavior.addToInventoryOrDispense.
fn add_to_inventory_or_dispense(l: &mut Level, state: State, pos: Pos, remainder: ItemStack) {
    let leftover = insert_item(l, pos, container::stack_of(remainder));
    if !is_empty(&leftover) {
        let direction = facing(l, state);
        let (sx, sy, sz) = direction.offset();
        let position = Vec3::new(pos.x() as f64 + 0.5 + (0.7 * sx as f64 + 0.0), pos.y() as f64 + 0.5 + (0.7 * sy as f64 + 0.0), pos.z() as f64 + 0.5 + (0.7 * sz as f64 + 0.0));
        let stack = leftover.borrow().clone();
        crate::entity::item::spawn_dispensed(l, stack, 6, direction, position);
    }
}

// DispenserBlockEntity.insertItem.
fn insert_item(l: &mut Level, pos: Pos, stack: Stack) -> Stack {
    let max_stack_size = container::max_stack(l.reg, &stack).min(container::MAX_STACK);
    let size = container::slots(l, pos).len();
    for i in 0..size {
        let slot = container::slots(l, pos)[i].clone();
        if is_empty(&slot) || container::same_item(&stack, &slot) {
            let min = count(&stack).min(max_stack_size - count(&slot));
            if min > 0 {
                if is_empty(&slot) {
                    let piece = container::split(&stack, min);
                    Cont::Be(pos).set_item(l, i, piece);
                } else {
                    container::shrink(&stack, min);
                    container::grow(&slot, min);
                }
            }
            if is_empty(&stack) {
                break;
            }
        }
    }
    stack
}

// ItemStack.hurtAndBreak(amount, level, null, ...): the damage a stack with
// a durability takes, once the enchantments with an item_damage effect
// (unbreaking: the only one the server holds) have removed some by the
// level's random.
fn hurt_and_break(l: &mut Level, stack: &Stack, amount: i32) {
    let reg = l.reg;
    let mut s = stack.borrow_mut();
    let damageable = s.component(reg, "minecraft:max_damage").is_some() && s.component(reg, "minecraft:unbreakable").is_none() && s.component(reg, "minecraft:damage").is_some();
    if !damageable {
        return;
    }
    let amount = if amount > 0 { process_durability_change(l, &s, amount) } else { amount };
    if amount == 0 {
        return;
    }
    let max = match s.component(reg, "minecraft:max_damage") {
        Some(Tag::Int(max)) => max,
        _ => unreachable!(),
    };
    let current = match s.component(reg, "minecraft:damage") {
        Some(Tag::Int(damage)) => damage.clamp(0, max),
        _ => unreachable!(),
    };
    // applyDamage: setDamageValue, then the stack breaks if it is broken.
    let damage = (current + amount).clamp(0, max);
    s.set_component(reg, "minecraft:damage", Tag::Int(damage));
    if damage >= max {
        s.count -= 1;
    }
}

// EnchantmentHelper.processDurabilityChange: the amount as a float that each
// enchantment of the stack with an item_damage effect whose requirement holds
// changes (RemoveBinomial: each of the amount's units is removed with the
// effect's chance, one nextFloat of the level's random each), then its int.
// Unbreaking removes at the fraction (2 + 2 (level - 1)) / (10 + 5 (level - 1))
// from an item that is armor, else level / (level + 1).
fn process_durability_change(l: &mut Level, stack: &ItemStack, amount: i32) -> i32 {
    let reg = l.reg;
    let mut value = amount as f32;
    if let Some(Tag::Compound(enchantments)) = stack.component(reg, "minecraft:enchantments") {
        for (key, level) in &enchantments.0 {
            if key != "minecraft:unbreaking" {
                continue;
            }
            let Tag::Int(level) = level else { unreachable!("enchantment levels are ints") };
            let level = *level;
            let linear = |base: f32, per_level: f32| base + per_level * (level - 1) as f32;
            let armor = reg.item_tags["minecraft:enchantable/armor"][stack.item as usize];
            let (numerator, denominator) = if armor { (linear(2.0, 2.0), linear(10.0, 5.0)) } else { (linear(1.0, 1.0), linear(2.0, 1.0)) };
            let chance = if denominator == 0.0 { 0.0 } else { numerator / denominator };
            let mut removed = 0;
            let mut unit = 0;
            while (unit as f32) < value {
                if l.random.next_float() < chance {
                    removed += 1;
                }
                unit += 1;
            }
            value -= removed as f32;
        }
    }
    value as i32
}

// Block.isDestroyable of a state's block.
fn is_destroyable(l: &Level, state: State) -> bool {
    !matches!(
        l.reg.blocks[l.block(state) as usize].key.as_str(),
        "minecraft:barrier"
            | "minecraft:bedrock"
            | "minecraft:end_portal_frame"
            | "minecraft:end_portal"
            | "minecraft:end_gateway"
            | "minecraft:command_block"
            | "minecraft:repeating_command_block"
            | "minecraft:chain_command_block"
            | "minecraft:structure_block"
            | "minecraft:jigsaw"
    )
}

// BlockState.canBeReplaced(Fluid): Block.canBeReplaced(state, fluid).
fn can_be_replaced_by_fluid(l: &Level, state: State) -> bool {
    match l.reg.blocks[l.block(state) as usize].class.as_str() {
        "EndGatewayBlock" | "EndPortalBlock" => false,
        _ => l.reg.flags(state) & crate::registry::REPLACEABLE != 0 || l.reg.flags(state) & crate::registry::SOLID == 0,
    }
}

fn supers(l: &Level, state: State, name: &str) -> bool {
    l.reg.blocks[l.block(state) as usize].supers.iter().any(|s| s == name)
}

// --- Boats and minecarts ---

// Direction.toYRot: a quarter turn per horizontal direction from south; up and
// down have no turn of their own (their data2d, -1, masks to 3).
fn direction_y_rot(direction: Dir) -> f32 {
    match direction {
        Dir::South => 0.0,
        Dir::West => 90.0,
        Dir::North => 180.0,
        Dir::East | Dir::Up | Dir::Down => 270.0,
    }
}

// AbstractBoat.setInitialPos and AbstractMinecart.setInitialPos: the position,
// which is also where the entity was before.
fn set_initial_pos(l: &mut Level, e: &mut crate::entity::Entity, x: f64, y: f64, z: f64) {
    crate::entity::motion::set_pos(l, e, x, y, z);
    e.xo = x;
    e.yo = y;
    e.zo = z;
}

// EntityType.createDefaultStackConfig(level, stack, null): the entity takes the
// stack's custom name and entity data; this port leaves a stack that holds a
// name unported.
fn refuse_custom_name(l: &Level, stack: &ItemStack) {
    if stack.component(l.reg, "minecraft:custom_name").is_some() {
        refuse(format!("a dispensed {} with a custom name (the entity takes it)", key_of(l, stack)));
    }
}

// BoatDispenseItemBehavior: a boat is put on the water in front (or the water
// below an air block there), one block above it for the water in front; anything
// else is dispensed as an item.
fn boat(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let direction = facing(l, state);
    let at = front(l, state, pos);
    let key = key_of(l, &item.borrow());
    // 0.5625 + EntityType.getWidth() / 2.0
    let width = l.reg.entity_types[key].width;
    let distance = 0.5625 + width as f64 / 2.0;
    let (step_x, step_y, step_z) = direction.offset();
    let (x, y, z) = (pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5);
    let d1 = x + step_x as f64 * distance;
    let d2 = y + (step_y as f32 * 1.125f32) as f64;
    let d3 = z + step_z as f64 * distance;
    let fluid = l.fluid_state(at);
    let offset = if l.fluid_type(fluid).is_water() {
        1.0
    } else {
        let block = l.get(at);
        let below = l.fluid_state(at.below());
        if !l.is_air(block) || !l.fluid_type(below).is_water() {
            return dispenser::dispense(l, state, pos, item);
        }
        0.0
    };
    refuse_custom_name(l, &item.borrow());
    // EntityType.create (the constructor's uuid draw), setInitialPos, setYRot.
    let mut boat = crate::entity::boat::new_boat(l, key);
    set_initial_pos(l, &mut boat, d1, d2 + offset, d3);
    boat.y_rot = direction_y_rot(direction);
    crate::entity::add_fresh_entity(l, boat);
    container::shrink(item, 1);
    item.clone()
}

// MinecartDispenseItemBehavior: a minecart goes on a rail in front (a little
// above it), or on a rail below an air block in front (below it); anything
// else is dispensed as an item.
fn minecart(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let direction = facing(l, state);
    let at = front(l, state, pos);
    let (step_x, step_y, step_z) = direction.offset();
    let (x, y, z) = (pos.x() as f64 + 0.5, pos.y() as f64 + 0.5, pos.z() as f64 + 0.5);
    let d0 = x + step_x as f64 * 1.125;
    let d1 = y.floor() + step_y as f64;
    let d2 = z + step_z as f64 * 1.125;
    let block = l.get(at);
    let offset = if l.reg.in_tag(block, "minecraft:rails") {
        if super::rail::shape(l, block).is_slope() { 0.6 } else { 0.1 }
    } else {
        if !l.is_air(block) {
            return dispenser::dispense(l, state, pos, item);
        }
        let below = l.get(at.below());
        if !l.reg.in_tag(below, "minecraft:rails") {
            return dispenser::dispense(l, state, pos, item);
        }
        if direction != Dir::Down && super::rail::shape(l, below).is_slope() { -0.4 } else { -0.9 }
    };
    let key = key_of(l, &item.borrow());
    let Some(class) = crate::entity::minecart::Class::by_key(key) else { refuse(format!("a dispensed {key} (a minecart this port does not model)")) };
    refuse_custom_name(l, &item.borrow());
    // AbstractMinecart.createMinecart: EntityType.create, setInitialPos.
    let mut cart = crate::entity::minecart::new_cart(l, class);
    set_initial_pos(l, &mut cart, d0, d1 + offset, d2);
    crate::entity::add_fresh_entity(l, cart);
    container::shrink(item, 1);
    item.clone()
}

// --- Shears ---

// ShearsDispenseItemBehavior: a beehive full of honey is sheared (tryShearBeehive:
// the honeycombs of the harvest/beehive table, then the honey level back to 0
// and the bees released: a hive here holds none); living entities to shear
// there are none, and no other entity there is shearable or leashed. A shearing
// that succeeds damages the shears by one.
fn shears(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    if is_full_beehive(l, target) {
        // BeehiveBlock.dropHoneycomb(level, stack, state, level.getBlockEntity(pos), null, pos):
        // the table's random sequence is started, and its one pool gives three honeycombs.
        l.block_entity(at);
        l.sequences.get("minecraft:harvest/beehive");
        let comb = ItemStack::new(l.reg.item_by_key["minecraft:honeycomb"], 3);
        crate::entity::item::pop_resource(l, at, comb);
        release_bees_and_reset_honey_level(l, at, target);
        let event = crate::game_event!(l, "shear");
        l.game_event_at(None, event, at);
        hurt_and_break(l, item, 1);
    }
    // setSuccess(false): the stack is not damaged.
    item.clone()
}

// A beehive (the class, in the beehives tag) whose honey level is 5.
fn is_full_beehive(l: &Level, state: State) -> bool {
    l.reg.in_tag(state, "minecraft:beehives") && l.reg.blocks[l.block(state) as usize].class == "BeehiveBlock" && l.reg.get_int(state, l.reg.property_of(state, "honey_level")) >= 5
}

// BeehiveBlock.releaseBeesAndResetHoneyLevel(level, state, pos, null, BEE_RELEASED):
// the honey level is set to 0, and the block entity (made if missing) releases
// the bees it holds, none.
fn release_bees_and_reset_honey_level(l: &mut Level, pos: Pos, state: State) {
    let honey = l.reg.property_of(state, "honey_level");
    l.set(pos, l.reg.with_int(state, honey, 0), UPDATE_ALL);
    l.block_entity(pos);
}

// --- Buckets ---

// BucketItem.emptyContents(null, level, pos, null) for a bucket of a fluid, and
// SolidBucketItem.emptyContents for the powder snow bucket: whether the
// contents were placed.
fn empty_contents(l: &mut Level, pos: Pos, content: Option<Fluid>) -> bool {
    let Some(fluid) = content else {
        // SolidBucketItem: powder snow into an empty block in the world.
        let state = l.get(pos);
        if !crate::world::World::outside(pos) && l.is_air(state) {
            let block = l.reg.block_key("minecraft:powder_snow");
            let placed = l.reg.default_state(block);
            l.set(pos, placed, UPDATE_ALL);
            let event = crate::game_event!(l, "fluid_place");
            l.game_event_at(None, event, pos);
            return true;
        }
        return false;
    };
    let block_state = l.get(pos);
    let can_be_replaced = can_be_replaced_by_fluid(l, block_state);
    let container = supers(l, block_state, "LiquidBlockContainer");
    let acceptable = can_be_replaced || container && crate::fluid::can_place_liquid(l, block_state, fluid);
    let place_here = l.is_air(block_state) || acceptable;
    if !place_here {
        return false;
    }
    if container && fluid == Fluid::Water {
        let source = l.reg.fluid_state(Fluid::Water, false, 0);
        crate::fluid::place_liquid(l, pos, block_state, source);
        let event = crate::game_event!(l, "fluid_place");
        l.game_event_at(None, event, pos);
        return true;
    }
    if can_be_replaced && l.reg.flags(block_state) & crate::registry::LIQUID == 0 {
        l.destroy_block(pos, true, 512);
    }
    let legacy = l.reg.fluids[l.reg.fluid_state(fluid, false, 0) as usize].legacy;
    let old_fluid = l.fluid_of(block_state);
    if !l.set(pos, legacy, 11) && !l.reg.fluids[old_fluid as usize].source {
        return false;
    }
    let event = crate::game_event!(l, "fluid_place");
    l.game_event_at(None, event, pos);
    true
}

// The behaviour of the lava, water and powder snow buckets and the buckets of
// fish: DispenseItemBehavior's first bucket behaviour.
fn full_bucket(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let key = key_of(l, &item.borrow());
    let (content, mob) = match key {
        "minecraft:water_bucket" => (Some(Fluid::Water), false),
        "minecraft:lava_bucket" => (Some(Fluid::Lava), false),
        "minecraft:powder_snow_bucket" => (None, false),
        "minecraft:cod_bucket" | "minecraft:salmon_bucket" | "minecraft:pufferfish_bucket" | "minecraft:tropical_fish_bucket" | "minecraft:axolotl_bucket" | "minecraft:tadpole_bucket" => (Some(Fluid::Water), true),
        other => refuse(format!("dispensing {other} (a bucket)")),
    };
    // BlockDispenseEvent (not cancelled, the item unchanged), then the
    // contents are emptied; a mob bucket's mob then spawns.
    if empty_contents(l, at, content) {
        if mob {
            refuse(format!("a dispenser empties {key} (its mob spawns)"));
        }
        let bucket = ItemStack::new(l.reg.item_by_key["minecraft:bucket"], 1);
        consume_with_remainder(l, state, pos, item, bucket)
    } else {
        dispenser::dispense(l, state, pos, item)
    }
}

// BucketPickup.pickupBlock(null, level, pos, state): the bucket item a block
// gives (the block changed for it), or none. With `apply` false it changes
// nothing (the dummy level the behaviour tries first).
fn pickup_block(l: &mut Level, pos: Pos, state: State, apply: bool) -> Option<&'static str> {
    let class = l.reg.blocks[l.block(state) as usize].class.as_str();
    match class {
        "LiquidBlock" => {
            if l.reg.get_int(state, l.reg.property_of(state, "level")) == 0 {
                let fluid = l.fluid_type(l.fluid_of(state));
                if apply {
                    l.set(pos, l.known.air, 11);
                }
                Some(if fluid.is_lava() { "minecraft:lava_bucket" } else { "minecraft:water_bucket" })
            } else {
                None
            }
        }
        "BubbleColumnBlock" => {
            if apply {
                l.set(pos, l.known.air, 11);
            }
            Some("minecraft:water_bucket")
        }
        "PowderSnowBlock" => {
            if apply {
                l.set(pos, l.known.air, 11);
            }
            Some("minecraft:powder_snow_bucket")
        }
        _ => {
            let overrides = l.reg.blocks[l.block(state) as usize].overrides.iter().any(|name| name == "pickupBlock");
            if overrides || class == "BarrierBlock" {
                refuse(format!("{class}.pickupBlock ({})", l.reg.describe(state)));
            }
            // SimpleWaterloggedBlock.pickupBlock.
            let waterlogged = l.reg.property_of(state, "waterlogged");
            if !l.reg.get_bool(state, waterlogged) {
                return None;
            }
            if apply {
                l.set(pos, l.reg.with_bool(state, waterlogged, false), UPDATE_ALL);
                if !super::can_survive(l, state, pos) {
                    l.destroy_block(pos, true, 512);
                }
            }
            Some("minecraft:water_bucket")
        }
    }
}

// The empty bucket's behaviour: takes the fluid of a block in front.
fn empty_bucket(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    if !supers(l, target, "BucketPickup") {
        return dispenser::dispense(l, state, pos, item);
    }
    let Some(key) = pickup_block(l, at, target, false) else { return dispenser::dispense(l, state, pos, item) };
    let event = crate::game_event!(l, "fluid_pickup");
    l.game_event_at(None, event, at);
    pickup_block(l, at, target, true);
    let filled = ItemStack::new(l.reg.item_by_key[key], 1);
    consume_with_remainder(l, state, pos, item, filled)
}

// --- Flint and steel, fire charges' block cousins ---

// BaseFireBlock.canBePlacedAt(level, pos, direction).
fn can_be_placed_at(l: &mut Level, pos: Pos, direction: Dir) -> bool {
    let target = l.get(pos);
    if !l.is_air(target) {
        return false;
    }
    let fire = super::special::fire::fire_state(l, pos);
    if super::can_survive(l, fire, pos) {
        return true;
    }
    // isPortal: the level is the overworld, where a frame of obsidian may light.
    let mut obsidian = false;
    for dir in crate::dir::ALL {
        let neighbour = l.get(pos.relative(dir));
        if is_block(l, neighbour, "minecraft:obsidian") {
            obsidian = true;
            break;
        }
    }
    if !obsidian {
        return false;
    }
    let axis = if direction.is_horizontal() {
        direction.counter_clockwise().axis()
    } else {
        // Direction.Plane.HORIZONTAL.getRandomAxis: X or Z by the level's random.
        if l.random.next_int_bound(2) == 0 { crate::dir::Axis::X } else { crate::dir::Axis::Z }
    };
    super::special::portal_shape::find_empty_portal_shape(l, pos, axis).is_some()
}

fn flint_and_steel(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let direction = facing(l, state);
    let at = pos.relative(direction);
    let target = l.get(at);
    let mut success = true;
    if can_be_placed_at(l, at, direction) {
        let fire = super::special::fire::fire_state(l, at);
        l.set(at, fire, UPDATE_ALL);
        let event = crate::game_event!(l, "block_place");
        l.game_event_at(None, event, at);
    } else if can_light(l, target) {
        let lit = l.reg.property_of(target, "lit");
        l.set(at, l.reg.with_bool(target, lit, true), UPDATE_ALL);
        let event = crate::game_event!(l, "block_change");
        l.game_event_at(None, event, at);
    } else if l.reg.blocks[l.block(target) as usize].class == "TntBlock" {
        // TntBlock.prime: the TNT entity, then the block goes.
        crate::entity::tnt::prime(l, at);
        l.remove_block(at, false);
    } else {
        success = false;
    }
    if success {
        hurt_and_break(l, item, 1);
    }
    item.clone()
}

// CampfireBlock.canLight, CandleBlock.canLight and CandleCakeBlock.canLight.
fn can_light(l: &Level, state: State) -> bool {
    let reg = l.reg;
    let has = |name: &str| reg.blocks[l.block(state) as usize].properties.iter().any(|(p, _)| reg.properties[*p as usize].name == name);
    let get = |name: &str| reg.get_bool(state, reg.property_of(state, name));
    if reg.in_tag(state, "minecraft:campfires") && has("waterlogged") && has("lit") {
        return !get("waterlogged") && !get("lit");
    }
    if reg.in_tag(state, "minecraft:candles") && has("lit") && has("waterlogged") {
        return !get("lit") && !get("waterlogged");
    }
    reg.in_tag(state, "minecraft:candle_cakes") && has("lit") && !get("lit")
}

// --- Bone meal ---

// BoneMealItem.growCrop and growWaterPlant as the dispenser's behaviour calls
// them. Block changes made while it grows are captured (Paper's
// captureTreeGeneration) and placed afterwards, in the order they were first
// made, each followed by the observer notification of
// checkCapturedTreeStateForObserverNotify. A block that is a
// BonemealableBlock other than kelp and seagrass has growth this port does
// not run (features, many random draws), and the water of a source block
// grows seagrass and corals by the biome.
fn bone_meal(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    let class = l.reg.blocks[l.block(target) as usize].class.clone();
    if supers(l, target, "BonemealableBlock") {
        let mut captured: Vec<(Pos, State, i32)> = Vec::new();
        let valid = match class.as_str() {
            "KelpBlock" | "KelpPlantBlock" | "SeagrassBlock" => bonemeal_kelp_or_seagrass(l, at, target, &class, &mut captured),
            "CropBlock" | "PotatoBlock" | "CarrotBlock" | "BeetrootBlock" | "TorchflowerCropBlock" => bonemeal_crop(l, at, target, &class, &mut captured),
            "CocoaBlock" => bonemeal_age(l, at, target, 2, 1, &mut captured),
            "SweetBerryBushBlock" => bonemeal_age(l, at, target, 3, 1, &mut captured),
            "StemBlock" => bonemeal_stem(l, at, target, &mut captured),
            "TallFlowerBlock" => {
                // popResource of the block's item.
                let item = l.reg.item_of_block[&l.block(target)];
                crate::entity::item::pop_resource(l, at, ItemStack::new(item, 1));
                true
            }
            "TallGrassBlock" => bonemeal_tall_grass(l, at, target, &mut captured),
            _ => refuse(format!("bone meal on {class} ({})", l.reg.describe(target))),
        };
        if valid {
            container::shrink(item, 1);
            for (placed, state, flags) in captured {
                l.set(placed, state, flags);
                // checkCapturedTreeStateForObserverNotify.
                if placed.y() == at.y() && l.get(placed) == state {
                    l.notify_and_update_physics(placed, state, state, state, flags, 512);
                }
            }
            return item.clone();
        }
    }
    // growWaterPlant: a full water block grows plants by its biome.
    if is_block(l, target, "minecraft:water") {
        let fluid = l.fluid_state(at);
        if l.reg.fluids[fluid as usize].amount == 8 {
            refuse("bone meal on water (seagrass and corals by the biome)");
        }
    }
    item.clone()
}

// CropBlock.isValidBonemealTarget (the age is under the maximum) and
// performBonemeal: growCrops, the age up by the bone meal's increase (2 to 5 by
// the level's random; a beetroot's a third of it, a torchflower's 1) to at
// most the maximum, as a block grow event that stands. A torchflower crop of
// its maximum age is the flower.
fn bonemeal_crop(l: &mut Level, pos: Pos, state: State, class: &str, captured: &mut Vec<(Pos, State, i32)>) -> bool {
    let max_age = match class {
        "BeetrootBlock" => 3,
        "TorchflowerCropBlock" => 2,
        _ => 7,
    };
    let age = l.reg.property_of(state, "age");
    let current = l.reg.get_int(state, age);
    if current >= max_age {
        return false;
    }
    let increase = match class {
        "TorchflowerCropBlock" => 1,
        // Mth.nextInt(random, 2, 5), and a beetroot's integer third.
        "BeetrootBlock" => (l.random.next_int_bound(4) + 2) / 3,
        _ => l.random.next_int_bound(4) + 2,
    };
    let grown = max_age.min(current + increase);
    let new_state = if class == "TorchflowerCropBlock" && grown == 2 {
        let flower = l.reg.block_key("minecraft:torchflower");
        l.reg.default_state(flower)
    } else {
        l.reg.with_int(l.reg.default_state(l.block(state)), age, grown)
    };
    capture(l, captured, pos, new_state, UPDATE_CLIENTS);
    true
}

// Cocoa and sweet berry bushes: a valid target is an age under `max`, and the
// age goes up by `by` to at most `max`... (cocoa: 2, sweet berries: 3).
fn bonemeal_age(l: &mut Level, pos: Pos, state: State, max: i32, by: i32, captured: &mut Vec<(Pos, State, i32)>) -> bool {
    let age = l.reg.property_of(state, "age");
    let current = l.reg.get_int(state, age);
    if current >= max {
        return false;
    }
    capture(l, captured, pos, l.reg.with_int(state, age, max.min(current + by)), UPDATE_CLIENTS);
    true
}

// StemBlock: valid under age 7; the age goes up by 2 to 5 (the level's random)
// to at most 7. A stem that reaches 7 random ticks at once and tries to put its
// fruit down, which this port does not run.
fn bonemeal_stem(l: &mut Level, pos: Pos, state: State, captured: &mut Vec<(Pos, State, i32)>) -> bool {
    let age = l.reg.property_of(state, "age");
    let current = l.reg.get_int(state, age);
    if current == 7 {
        return false;
    }
    let grown = 7.min(current + l.random.next_int_bound(4) + 2);
    if grown == 7 {
        refuse("a stem that bone meal ripens (it random ticks to set its fruit)");
    }
    capture(l, captured, pos, l.reg.with_int(state, age, grown), UPDATE_CLIENTS);
    true
}

// TallGrassBlock (short grass and ferns): valid where the double plant it
// grows into can survive and the block above is empty; it places both halves.
fn bonemeal_tall_grass(l: &mut Level, pos: Pos, state: State, captured: &mut Vec<(Pos, State, i32)>) -> bool {
    let grown = if is_block(l, state, "minecraft:fern") { "minecraft:large_fern" } else { "minecraft:tall_grass" };
    let block = l.reg.block_key(grown);
    let lower = l.reg.default_state(block);
    let half = l.reg.property_of(lower, "half");
    if !super::can_survive(l, lower, pos) {
        return false;
    }
    let above = l.get(pos.above());
    if !l.is_air(above) {
        return false;
    }
    let upper = l.reg.with_name(lower, half, "upper");
    capture(l, captured, pos, lower, UPDATE_CLIENTS);
    capture(l, captured, pos.above(), upper, UPDATE_CLIENTS);
    true
}

// Level.setBlock while block changes are captured: the block there at first is
// what is read until they are placed; a block that cannot be destroyed
// stays.
fn capture(l: &mut Level, captured: &mut Vec<(Pos, State, i32)>, pos: Pos, state: State, flags: i32) -> bool {
    let current = captured.iter().find(|(p, _, _)| *p == pos).map(|(_, s, _)| *s).unwrap_or_else(|| l.get(pos));
    if !is_destroyable(l, current) {
        return false;
    }
    match captured.iter_mut().find(|(p, _, _)| *p == pos) {
        Some(entry) => {
            entry.1 = state;
            entry.2 = flags;
        }
        None => captured.push((pos, state, flags)),
    }
    true
}

// What a level that captures block changes shows at a position.
fn captured_state(l: &mut Level, captured: &[(Pos, State, i32)], pos: Pos) -> State {
    captured.iter().find(|(p, _, _)| *p == pos).map(|(_, s, _)| *s).unwrap_or_else(|| l.get(pos))
}

// isValidBonemealTarget, isBonemealSuccess (true) and performBonemeal of kelp,
// its stem and seagrass; whether the target was valid.
fn bonemeal_kelp_or_seagrass(l: &mut Level, pos: Pos, state: State, class: &str, captured: &mut Vec<(Pos, State, i32)>) -> bool {
    match class {
        "SeagrassBlock" => {
            let above = l.get(pos.above());
            if !is_block(l, above, "minecraft:water") {
                return false;
            }
            let tall = l.reg.block_key("minecraft:tall_seagrass");
            let lower = l.reg.default_state(tall);
            let half = l.reg.property_of(lower, "half");
            let upper = l.reg.with_name(lower, half, "upper");
            capture(l, captured, pos, lower, UPDATE_CLIENTS);
            capture(l, captured, pos.above(), upper, UPDATE_CLIENTS);
            true
        }
        "KelpBlock" => {
            // GrowingPlantHeadBlock.isValidBonemealTarget: the block above can be grown into (water).
            let above = l.get(pos.above());
            if !is_block(l, above, "minecraft:water") {
                return false;
            }
            kelp_perform_bonemeal(l, pos, state, captured);
            true
        }
        // GrowingPlantBodyBlock: the head of the stem above decides.
        _ => {
            let Some(head) = kelp_head(l, pos, state) else { return false };
            let above = l.get(head.above());
            if !is_block(l, above, "minecraft:water") {
                return false;
            }
            let head_state = l.get(head);
            kelp_perform_bonemeal(l, head, head_state, captured);
            true
        }
    }
}

// BlockUtil.getTopConnectedBlock(level, pos, body block, up, head block).
fn kelp_head(l: &mut Level, pos: Pos, state: State) -> Option<Pos> {
    let body = l.block(state);
    let head = l.reg.block_key("minecraft:kelp");
    let mut at = pos;
    loop {
        at = at.above();
        let s = l.get(at);
        if l.block(s) == body {
            continue;
        }
        return if l.block(s) == head { Some(at) } else { None };
    }
}

// GrowingPlantHeadBlock.performBonemeal for kelp (one block): the age goes up
// by one (to 25), and the water above becomes a head of that age.
fn kelp_perform_bonemeal(l: &mut Level, pos: Pos, state: State, captured: &mut Vec<(Pos, State, i32)>) {
    let age = l.reg.property_of(state, "age");
    let min = (l.reg.get_int(state, age) + 1).min(25);
    let at = pos.above();
    let target = captured_state(l, captured, at);
    if is_block(l, target, "minecraft:water") {
        capture(l, captured, at, l.reg.with_int(state, age, min), UPDATE_ALL);
    }
}

// --- TNT ---

// The TNT block item: a primed TNT entity at the block in front.
fn tnt(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    // The game rule tntExplodes is on.
    let at = front(l, state, pos);
    crate::entity::tnt::dispensed(l, at);
    container::shrink(item, 1);
    item.clone()
}

// --- Skulls and pumpkins ---

// BlockPattern for the wither's base: three blocks of soul sand-like blocks in a T, with air.
fn wither_skull(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    if l.is_air(target) && at.y() >= -64 + 2 && super::shapes::carved_pumpkin::wither_base_found(l, at) {
        refuse("a wither skull placed on a wither's base (the skull block entity, the wither)");
    }
    // EquipmentDispenseItemBehavior.dispenseEquipment finds no entity.
    item.clone()
}

fn carved_pumpkin(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    if l.is_air(target) && super::shapes::carved_pumpkin::can_spawn_golem(l, at) {
        let block = l.reg.block_key("minecraft:carved_pumpkin");
        let placed = l.reg.default_state(block);
        l.set(at, placed, UPDATE_ALL);
        let event = crate::game_event!(l, "block_place");
        l.game_event_at(None, event, at);
        container::shrink(item, 1);
    }
    item.clone()
}

// --- Glass bottle, glowstone, honeycomb, potion ---

fn glass_bottle(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    if is_full_beehive(l, target) {
        release_bees_and_reset_honey_level(l, at, target);
        // takeLiquid.
        let event = crate::game_event!(l, "fluid_pickup");
        l.game_event_at(None, event, pos);
        let honey = ItemStack::new(l.reg.item_by_key["minecraft:honey_bottle"], 1);
        return consume_with_remainder(l, state, pos, item, honey);
    }
    let fluid = l.fluid_state(at);
    if l.fluid_type(fluid).is_water() {
        // takeLiquid.
        let event = crate::game_event!(l, "fluid_pickup");
        l.game_event_at(None, event, pos);
        let mut water = ItemStack::new(l.reg.item_by_key["minecraft:potion"], 1);
        let mut contents = crate::nbt::Compound::new();
        contents.put("potion", Tag::string("minecraft:water"));
        water.set_component(l.reg, "minecraft:potion_contents", Tag::Compound(contents));
        return consume_with_remainder(l, state, pos, item, water);
    }
    dispenser::dispense(l, state, pos, item)
}

fn glowstone(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    if is_block(l, target, "minecraft:respawn_anchor") {
        let charge = l.reg.property_of(target, "charges");
        if l.reg.get_int(target, charge) != 4 {
            // RespawnAnchorBlock.charge.
            let charged = l.reg.with_int(target, charge, l.reg.get_int(target, charge) + 1);
            l.set(at, charged, UPDATE_ALL);
            let event = crate::game_event!(l, "block_change");
            l.game_event_in(event, at, Context::of_state(charged));
            container::shrink(item, 1);
        }
        item.clone()
    } else {
        dispenser::dispense(l, state, pos, item)
    }
}

fn honeycomb(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let at = front(l, state, pos);
    let target = l.get(at);
    let block = l.block(target);
    match l.reg.waxed.get(&block).copied() {
        Some(waxed) => {
            // Block.withPropertiesOf: the new block's state with the old one's values.
            let mut new = l.reg.default_state(waxed);
            for (name, value) in l.reg.values(target) {
                if l.reg.blocks[waxed as usize].properties.iter().any(|(p, _)| l.reg.properties[*p as usize].name == name) {
                    let property = l.reg.property_of(new, name);
                    new = l.reg.with_name(new, property, value);
                }
            }
            l.set(at, new, UPDATE_ALL);
            container::shrink(item, 1);
            item.clone()
        }
        None => dispenser::dispense(l, state, pos, item),
    }
}

fn potion(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let reg = l.reg;
    let water = matches!(item.borrow().component(reg, "minecraft:potion_contents"), Some(Tag::Compound(c)) if matches!(c.get("potion"), Some(Tag::String(p)) if p == "minecraft:water"));
    if !water {
        return dispenser::dispense(l, state, pos, item);
    }
    let at = front(l, state, pos);
    let target = l.get(at);
    if !l.reg.in_tag(target, "minecraft:convertable_to_mud") {
        return dispenser::dispense(l, state, pos, item);
    }
    // The splash particles take two doubles of the level's random each.
    for _ in 0..5 {
        l.random.next_double();
        l.random.next_double();
    }
    let event = crate::game_event!(l, "fluid_place");
    l.game_event_at(None, event, pos);
    let mud = l.reg.block_key("minecraft:mud");
    let placed = l.reg.default_state(mud);
    l.set(at, placed, UPDATE_ALL);
    consume_with_remainder(l, state, pos, item, ItemStack::new(reg.item_by_key["minecraft:glass_bottle"], 1))
}

// --- Shulker boxes ---

// ShulkerBoxDispenseBehavior: the box is placed in front of the dispenser
// (BlockItem.place with a DirectionalPlaceContext); when it cannot be, the
// stack stays.
fn shulker_box(l: &mut Level, state: State, pos: Pos, item: &Stack) -> Stack {
    let reg = l.reg;
    let direction = facing(l, state);
    let at = pos.relative(direction);
    let below = l.get(at.below());
    let face = if l.is_air(below) { direction } else { Dir::Up };
    // BlockItem.place: context.canPlace() is BlockState.canBeReplaced(context).
    let target = l.get(at);
    if reg.blocks[l.block(target) as usize].overrides.iter().any(|name| name == "canBeReplaced") && reg.block_flags["canBeReplacedContext"][l.block(target) as usize] {
        refuse(format!("{}.canBeReplaced for a place context ({})", reg.blocks[l.block(target) as usize].class, reg.describe(target)));
    }
    let same_item = {
        let stack = item.borrow();
        reg.items[stack.item as usize].block == Some(l.block(target))
    };
    let replaceable = reg.flags(target) & crate::registry::REPLACEABLE != 0 && !same_item && is_destroyable(l, target);
    if !replaceable {
        return item.clone();
    }
    // getPlacementState: FACING is the clicked face; the box survives anywhere
    // and must not collide with an entity that blocks building.
    let block = reg.items[item.borrow().item as usize].block.expect("a shulker box item has its block");
    let facing_property = reg.property_of(reg.default_state(block), "facing");
    let placement = reg.with_dir(reg.default_state(block), facing_property, face);
    if !check_entity_collision(l, at) {
        return item.clone();
    }
    // The item the context holds: a copy of one of the stack.
    let one = container::copy_with_count(item, 1);
    let one = one.borrow().clone();
    if !l.set(at, placement, 11) {
        return item.clone();
    }
    let mut current = l.get(at);
    if l.block(current) == block {
        // updateBlockStateFromTag: the stack's block state properties, set with flags 2.
        if let Some(Tag::Compound(properties)) = one.component(reg, "minecraft:block_state") {
            if !properties.is_empty() {
                let applied = reg.apply_block_state(current, &properties);
                if applied != current {
                    l.set(at, applied, 2);
                }
                current = applied;
            }
        }
        // updateBlockEntityComponents (BlockEntity.applyComponents): the block
        // entity (made if missing) takes the stack's name, lock and container
        // (BaseContainerBlockEntity and RandomizableContainerBlockEntity read
        // these and the container loot), and keeps the other components the
        // stack's patch adds, but for the block entity data and block state.
        let contents = one.component(reg, "minecraft:container");
        let name = one.component(reg, "minecraft:custom_name");
        let lock = one.component(reg, "minecraft:lock");
        let mut kept = crate::nbt::Compound::new();
        for (key, value) in one.patch.entries() {
            const READ: [&str; 6] = ["minecraft:block_entity_data", "minecraft:block_state", "minecraft:custom_name", "minecraft:lock", "minecraft:container", "minecraft:container_loot"];
            if let (Some(value), false) = (value, READ.contains(&key.as_str())) {
                kept.put(key, value.clone());
            }
        }
        if let Some(super::BlockEntity::Container(c)) = l.block_entity(at) {
            c.name = name;
            c.lock = lock;
            if let Some(Tag::List(entries)) = contents {
                for entry in entries {
                    let compound = entry.as_compound().expect("a container entry");
                    let slot = match compound.get("slot") {
                        Some(Tag::Int(slot)) => *slot as usize,
                        _ => unreachable!(),
                    };
                    let stack = crate::components::decode_stack(reg, compound.get("item").expect("a container item")).unwrap_or_else(|fail| refuse(format!("a shulker box's contents: {}", fail.0)));
                    if slot < c.items.len() {
                        c.items[slot] = container::stack_of(stack);
                    }
                }
            }
        }
        if l.block_entity(at).is_some() {
            l.set_block_entity_extras(at, super::Extras { components: kept, bukkit: crate::nbt::Compound::new() });
        }
        container::be_changed(l, at);
    }
    let event = crate::game_event!(l, "block_place");
    l.game_event_in(event, at, Context::of_state(current));
    container::shrink(item, 1);
    item.clone()
}

// Level.checkEntityCollision for a full block at the position: no entity that
// blocks building is within it.
fn check_entity_collision(l: &mut Level, at: Pos) -> bool {
    let aabb = Aabb::new(at.x() as f64, at.y() as f64, at.z() as f64, at.x() as f64 + 1.0, at.y() as f64 + 1.0, at.z() as f64 + 1.0);
    crate::entity::entities_in(l, None, None, crate::entity::Only::Any, &aabb, |e| !e.removed && matches!(e.kind, crate::entity::Kind::Falling(_) | crate::entity::Kind::Tnt(_))).is_empty()
}
