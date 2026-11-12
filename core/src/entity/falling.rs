// FallingBlockEntity: falling (FallingBlockEntity.fall), its tick, its
// landing (placing the block, or dropping it as an item), and the fall
// damage an anvil does. Paper's configuration is at its defaults:
// fixes.falling-block-height-nerf disabled, and the game rules at theirs
// (doEntityDrops true).
//
// Events the Bukkit layer calls (EntityChangeBlockEvent, BlockFormEvent,
// EntityDropItemEvent) have no listener: their results stand.

use super::motion::{self, set_pos};
use super::{add_fresh_entity, remove, Entity, EntityId, Kind};
use crate::blocks;
use crate::dir::{Dir, Pos, ALL};
use crate::level::{refuse, Level, UPDATE_ALL};
use crate::loot::ItemStack;
use crate::phys::{self, Vec3};
use crate::registry::{BlockId, Fluid, State, Support, AIR, LIQUID, REPLACEABLE};
use crate::world::{HEIGHT, MIN_Y};

// FallingBlockEntity's fields (its block entity data is none).
pub struct FallingData {
    pub state: State,
    pub time: i32,
    pub drop_item: bool,
    pub cancel_drop: bool,
    pub hurt_entities: bool,
    pub fall_damage_max: i32,
    pub fall_damage_per_distance: f32,
    pub start_pos: Pos,
}

fn data(e: &Entity) -> &FallingData {
    match &e.kind {
        Kind::Falling(d) => d,
        _ => panic!("not a falling block"),
    }
}

fn data_mut(e: &mut Entity) -> &mut FallingData {
    match &mut e.kind {
        Kind::Falling(d) => d,
        _ => panic!("not a falling block"),
    }
}

// EntityType.FALLING_BLOCK: 0.98 by 0.98, the default eye height.
const SIZE: f32 = 0.98f32;

// FallingBlock.isFree.
pub fn is_free(l: &Level, state: State) -> bool {
    l.reg.flags(state) & (AIR | LIQUID | REPLACEABLE) != 0 || l.reg.in_tag(state, "minecraft:fire")
}

fn class(l: &Level, state: State) -> &str {
    l.reg.blocks[l.block(state) as usize].class.as_str()
}

// FallingBlockEntity.fall: the block at `pos` becomes a falling block
// entity (the fluid its state held stays there). The
// entity is in the level; its id is returned for the block to change it
// (FallingBlock.falling).
pub fn fall(l: &mut Level, pos: Pos, state: State) -> EntityId {
    let waterlogged = l.reg.blocks[l.block(state) as usize].properties.iter().find(|&&(id, _)| l.reg.properties[id as usize].name == "waterlogged").map(|&(id, _)| id);
    let stored = match waterlogged {
        Some(property) => l.reg.with_bool(state, property, false),
        None => state,
    };
    let data = FallingData { state: stored, time: 0, drop_item: true, cancel_drop: false, hurt_entities: false, fall_damage_max: 40, fall_damage_per_distance: 0.0f32, start_pos: Pos::new(0, 0, 0) };
    let mut e = Box::new(Entity::new(l, Kind::Falling(data), SIZE, SIZE, SIZE * 0.85f32));
    let (x, y, z) = (pos.x() as f64 + 0.5, pos.y() as f64, pos.z() as f64 + 0.5);
    set_pos(l, &mut e, x, y, z);
    e.delta = Vec3::ZERO;
    e.xo = x;
    e.yo = y;
    e.zo = z;
    data_mut(&mut e).start_pos = e.block_pos;
    // state.getFluidState().createLegacyBlock(): air where the state holds no fluid.
    let legacy = l.reg.fluids[l.fluid_of(state) as usize].legacy;
    l.set(pos, legacy, UPDATE_ALL);
    let id = e.id;
    add_fresh_entity(l, e);
    id
}

// FallingBlockEntity.setHurtsEntities, for the entity `fall` made.
pub fn set_hurts_entities(l: &mut Level, id: EntityId, per_distance: f32, max: i32) {
    let e = l.entities.all.get_mut(&id).expect("a falling block that was just made");
    let d = data_mut(e);
    d.hurt_entities = true;
    d.fall_damage_per_distance = per_distance;
    d.fall_damage_max = max;
}

// BlockBehaviour.canBeReplaced(state, DirectionalPlaceContext(.., ItemStack.EMPTY, ..)):
// the block's own rule where its class overrides it, with an empty hand and
// no player, else replaceable and destroyable.
fn can_be_replaced(l: &mut Level, state: State) -> bool {
    if l.reg.flagged(state, "canBeReplacedContext") {
        return match class(l, state) {
            "PitcherCropBlock" | "ScaffoldingBlock" | "SlabBlock" | "WeatheringCopperSlabBlock" => false,
            "SnowLayerBlock" => l.reg.get_int(state, l.reg.property_of(state, "layers")) == 1,
            // !stack.is(this.asItem()) || hasAnyVacantFace.
            "GlowLichenBlock" | "MultifaceBlock" | "SculkVeinBlock" | "ResinClumpBlock" => true,
            "VineBlock" => {
                let faces = ["up", "north", "east", "south", "west"].iter().filter(|name| l.reg.get_bool(state, l.reg.property_of(state, name))).count();
                faces < 5
            }
            // These defer to BlockBehaviour's rule with an empty hand.
            "CandleBlock" | "FlowerBedBlock" | "LeafLitterBlock" | "SeaPickleBlock" | "TurtleEggBlock" | "KelpPlantBlock" | "WeepingVinesPlantBlock" | "TwistingVinesPlantBlock" | "CaveVinesPlantBlock" => default_can_be_replaced(l, state),
            other => refuse(format!("{other}.canBeReplaced ({})", l.reg.describe(state))),
        };
    }
    default_can_be_replaced(l, state)
}

fn default_can_be_replaced(l: &Level, state: State) -> bool {
    l.reg.flags(state) & REPLACEABLE != 0 && is_destroyable(l, state)
}

// Block.isDestroyable with Paper's permanent block break exploits off.
pub fn is_destroyable(l: &Level, state: State) -> bool {
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

// Entity.spawnAtLocation(level, block): the block's item, one of it, at the
// entity's position, with the default pickup delay.
fn spawn_at_location(l: &mut Level, e: &Entity, block: BlockId) {
    let Some(&item) = l.reg.item_of_block.get(&block) else { refuse(format!("{} has no item", l.reg.blocks[block as usize].key)) };
    let mut item = super::item::new_item(l, e.pos.x + 0.0, e.pos.y + 0.0, e.pos.z + 0.0, ItemStack::new(item, 1), None);
    super::item::set_default_pickup_delay(&mut item);
    add_fresh_entity(l, item);
}

// FallingBlockEntity.disableDrop, for the entity `fall` made.
pub fn disable_drop(l: &mut Level, id: EntityId) {
    let e = l.entities.all.get_mut(&id).expect("a falling block that was just made");
    data_mut(e).cancel_drop = true;
}

// FallingBlockEntity.callOnBrokenAfterFall, Fallable.onBrokenAfterFall:
// a level event for anvils and pointed dripstone, which reaches no one; a
// brushable block also posts block_destroy at the centre of the falling
// block's box, the falling block as its source.
fn on_broken_after_fall(l: &mut Level, e: &Entity, block: BlockId) {
    if l.reg.blocks[block as usize].class == "BrushableBlock" {
        let at = e.bb.center();
        let source = crate::game_event::Source::of(l, e);
        let event = crate::game_event!(l, "block_destroy");
        l.game_event(event, at, crate::game_event::Context::of_entity(Some(source)));
    }
}

// ConcretePowderBlock.canSolidify: whether the state holds water.
fn can_solidify(l: &Level, state: State) -> bool {
    l.fluid_type(l.fluid_of(state)).is_water()
}

// ConcretePowderBlock.touchesLiquid: the blocks it reads, in its order.
pub fn touches_liquid(l: &mut Level, pos: Pos) -> bool {
    let mut mutable = pos;
    for dir in ALL {
        let state = l.get(mutable);
        if dir != Dir::Down || can_solidify(l, state) {
            mutable = pos.relative(dir);
            let neighbour = l.get(mutable);
            if can_solidify(l, neighbour) && !crate::shape_ops::is_face_sturdy(l, neighbour, mutable, dir.opposite(), Support::Full) {
                return true;
            }
        }
    }
    false
}

// ConcretePowderBlock.concrete: the powder's concrete.
pub fn concrete_of(l: &Level, powder: BlockId) -> State {
    let key = l.reg.blocks[powder as usize].key.replace("_concrete_powder", "_concrete");
    l.reg.default_state(l.reg.block_key(&key))
}

// FallingBlockEntity.tick.
pub fn tick(l: &mut Level, e: &mut Entity) {
    if l.is_air(data(e).state) {
        remove(l, e);
        return;
    }
    let block = l.block(data(e).state);
    data_mut(e).time += 1;
    // applyGravity: 0.04.
    e.delta = e.delta.add(0.0, -0.04, 0.0);
    motion::move_entity(l, e, e.delta);
    motion::apply_effects_from_blocks(l, e);
    // Paper's falling-block-height-nerf is disabled; handlePortal.
    motion::handle_portal(e);
    if !e.removed {
        let mut block_pos = e.block_pos;
        let concrete = class(l, data(e).state) == "ConcretePowderBlock";
        let mut in_water = false;
        if concrete {
            let state = l.get(block_pos);
            in_water = can_solidify(l, state);
        }
        let speed_sqr = e.delta.length_sqr();
        if concrete && speed_sqr > 1.0 {
            // level.clip(ClipContext(xo, yo, zo -> position, COLLIDER, SOURCE_ONLY, this)):
            // a hit on water moves the landing to it.
            let from = Vec3::new(e.xo, e.yo, e.zo);
            let ctx = super::collide::context(l, e);
            if let Some(hit) = clip_collider_source(l, ctx, from, e.pos) {
                let state = l.get(hit);
                if can_solidify(l, state) {
                    block_pos = hit;
                    in_water = true;
                }
            }
        }
        if !e.on_ground && !in_water {
            let time = data(e).time;
            let out_of_world = block_pos.y() <= MIN_Y || block_pos.y() > MIN_Y + HEIGHT - 1;
            // autoExpire is on.
            if (time > 100 && out_of_world) || time > 600 {
                if data(e).drop_item {
                    spawn_at_location(l, e, block);
                }
                remove(l, e);
            }
        } else {
            land(l, e, block, block_pos, concrete, in_water);
        }
    }
    e.delta = e.delta.scale(0.98);
}

// The landing branch of FallingBlockEntity.tick: the entity is on the
// ground (or in water).
fn land(l: &mut Level, e: &mut Entity, block: BlockId, block_pos: Pos, concrete: bool, in_water: bool) {
    let at = l.get(block_pos);
    e.delta = e.delta.multiply(0.7, -0.5, 0.7);
    if l.reg.blocks[l.block(at) as usize].key == "minecraft:moving_piston" {
        return;
    }
    if data(e).cancel_drop {
        remove(l, e);
        on_broken_after_fall(l, e, block);
        return;
    }
    let can_be_replaced = can_be_replaced(l, at);
    let below = l.get(block_pos.below());
    let free_below = is_free(l, below) && (!concrete || !in_water);
    let state = data(e).state;
    let survives = blocks::can_survive(l, state, block_pos) && !free_below;
    if can_be_replaced && survives {
        // A waterlogging state landing in (still) water holds it.
        let waterlogged = l.reg.blocks[l.block(state) as usize].properties.iter().find(|&&(id, _)| l.reg.properties[id as usize].name == "waterlogged").map(|&(id, _)| id);
        if let Some(property) = waterlogged {
            let here = l.fluid_state(block_pos);
            if l.fluid_type(here) == Fluid::Water {
                data_mut(e).state = l.reg.with_bool(state, property, true);
            }
        }
        let state = data(e).state;
        if l.set(block_pos, state, UPDATE_ALL) {
            remove(l, e);
            on_land(l, block, block_pos, at);
        } else if data(e).drop_item {
            remove(l, e);
            on_broken_after_fall(l, e, block);
            spawn_at_location(l, e, block);
        }
    } else {
        remove(l, e);
        if data(e).drop_item {
            on_broken_after_fall(l, e, block);
            spawn_at_location(l, e, block);
        }
    }
}

// Fallable.onLand: an anvil's sound reaches no one; concrete powder
// hardens where it touches water (BlockFormEvent has no listener).
fn on_land(l: &mut Level, block: BlockId, pos: Pos, replaceable: State) {
    if l.reg.blocks[block as usize].class == "ConcretePowderBlock" && (can_solidify(l, replaceable) || touches_liquid(l, pos)) {
        let concrete = concrete_of(l, block);
        l.set(pos, concrete, UPDATE_ALL);
    }
}

// level.clip(ClipContext(from, to, COLLIDER, SOURCE_ONLY, entity)) for the
// block it hits, if any: the first block along the segment whose collision
// shape, or source fluid, the segment meets.
fn clip_collider_source(l: &mut Level, ctx: crate::collision::Ctx, from: Vec3, to: Vec3) -> Option<Pos> {
    let result = crate::clip::traverse_blocks(from, to, |pos| {
        let Some(state) = l.get_if_loaded(pos) else { return Some(None) };
        if l.is_air(state) {
            return None;
        }
        let shape = crate::collision::shape_of(l, state, pos, ctx);
        if crate::clip::shape_clip_hits(&shape, from, to, pos) {
            return Some(Some(pos));
        }
        let fluid_state = l.fluid_of(state);
        if l.reg.fluids[fluid_state as usize].source {
            let height = crate::fluid::height(l, fluid_state, pos);
            if crate::clip::box_clip_hits(height as f64, from, to, pos) {
                return Some(Some(pos));
            }
        }
        None
    });
    result.flatten()
}

// FallingBlockEntity.causeFallDamage. Nothing alive stands in the level
// (getEntities with the living-entity predicates finds none), so what remains
// is an anvil that may chip.
pub fn cause_fall_damage(l: &mut Level, e: &mut Entity, fall_distance: f64) {
    if !data(e).hurt_entities {
        return;
    }
    let ceil = phys::ceil(fall_distance - 1.0);
    if ceil < 0 {
        return;
    }
    // Math.min(Mth.floor((float) ceil * perDistance), max).
    let scaled = ceil as f32 * data(e).fall_damage_per_distance;
    let f = (mth_floor_float(scaled).min(data(e).fall_damage_max)) as f32;
    let is_anvil = l.reg.in_tag(data(e).state, "minecraft:anvil");
    if is_anvil && f > 0.0 && l.shared_random.next_float() < 0.05f32 + ceil as f32 * 0.05f32 {
        match anvil_damage(l, data(e).state) {
            Some(damaged) => data_mut(e).state = damaged,
            None => data_mut(e).cancel_drop = true,
        }
    }
}

// Mth.floor(float).
fn mth_floor_float(f: f32) -> i32 {
    let i = f as i32;
    if f < i as f32 {
        i - 1
    } else {
        i
    }
}

// AnvilBlock.damage: the anvil one step more damaged, facing as it did, or
// none for a damaged anvil.
fn anvil_damage(l: &Level, state: State) -> Option<State> {
    let key = l.reg.blocks[l.block(state) as usize].key.as_str();
    let next = match key {
        "minecraft:anvil" => "minecraft:chipped_anvil",
        "minecraft:chipped_anvil" => "minecraft:damaged_anvil",
        _ => return None,
    };
    let facing = l.reg.property_of(state, "facing");
    let to = l.reg.default_state(l.reg.block_key(next));
    Some(l.reg.with_dir(to, facing, l.reg.get_dir(state, facing)))
}
