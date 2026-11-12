// Fluids: FlowingFluid, WaterFluid and LavaFluid (net.minecraft.world.level.
// material) as they spread, drain and tick in the overworld (not ultra warm,
// water source conversion on, lava source conversion off, Paper's defaults),
// and the liquid containers (LiquidBlockContainer, SimpleWaterloggedBlock)
// they flow into. The fluid state of a block state is in the registry
// (Registry::fluids, StateData::fluid). Bukkit events (BlockFromToEvent,
// FluidLevelChangeEvent, BlockFormEvent) have no listener, so they do not
// cancel; the fizz level event reaches nobody and draws nothing.

use crate::dir::{Dir, Pos, HORIZONTAL};
use crate::entity::item;
use crate::level::{refuse, Level, UPDATE_ALL};
use crate::registry::{BlockId, Fluid, FluidId, State, BLOCKS_MOTION, BLOCK_ENTITY, EMPTY_FLUID, SOLID};
use std::sync::OnceLock;

// Fluids.WATER.getTickDelay(level), which the waterlogged blocks use.
pub const WATER_TICK_DELAY: i32 = 5;

// Fluid.getTickDelay in the overworld.
pub fn tick_delay(fluid: Fluid) -> i32 {
    match fluid {
        Fluid::Empty => 0,
        Fluid::FlowingWater | Fluid::Water => WATER_TICK_DELAY,
        Fluid::FlowingLava | Fluid::Lava => 30,
    }
}

// FlowingFluid.getDropOff.
fn drop_off(this: Fluid) -> i32 {
    if this.is_lava() {
        2
    } else {
        1
    }
}

// FlowingFluid.getSlopeFindDistance.
fn slope_find_distance(this: Fluid) -> i32 {
    if this.is_lava() {
        2
    } else {
        4
    }
}

// FlowingFluid.canConvertToSource: the water and lava source conversion
// game rules at their defaults.
fn can_convert_to_source(this: Fluid) -> bool {
    this.is_water()
}

fn flowing_kind(this: Fluid) -> Fluid {
    if this.is_lava() {
        Fluid::FlowingLava
    } else {
        Fluid::FlowingWater
    }
}

fn source_kind(this: Fluid) -> Fluid {
    if this.is_lava() {
        Fluid::Lava
    } else {
        Fluid::Water
    }
}

// The per-block facts the fluid code asks of classes.
struct Tables {
    // block instanceof LiquidBlockContainer.
    container: Vec<bool>,
    // The blocks canHoldAnyFluid refuses by what they are, whatever their state.
    cannot_hold: Vec<bool>,
}

fn tables(l: &Level) -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let reg = l.reg;
        let signs = &reg.tags["minecraft:signs"];
        let container = reg.blocks.iter().map(|block| block.supers.iter().any(|s| s == "LiquidBlockContainer")).collect();
        let cannot_hold = reg
            .blocks
            .iter()
            .enumerate()
            .map(|(id, block)| {
                block.class == "DoorBlock"
                    || block.supers.iter().any(|s| s == "DoorBlock")
                    || signs[id]
                    || matches!(
                        block.key.as_str(),
                        "minecraft:ladder" | "minecraft:sugar_cane" | "minecraft:bubble_column" | "minecraft:nether_portal" | "minecraft:end_portal" | "minecraft:end_gateway" | "minecraft:structure_void"
                    )
            })
            .collect();
        Tables { container, cannot_hold }
    })
}

// FluidState.getHeight / FlowingFluid.getOwnHeight.
fn own_height(l: &Level, fluid: FluidId) -> f32 {
    l.reg.fluids[fluid as usize].amount as f32 / 9.0f32
}

// FlowingFluid.getHeight (EmptyFluid: 0): a fluid with the same fluid above
// fills the block.
pub fn height(l: &mut Level, fluid: FluidId, pos: Pos) -> f32 {
    let this = l.fluid_type(fluid);
    if this == Fluid::Empty {
        return 0.0;
    }
    let above = l.fluid_state(pos.above());
    if this.is_same(l.fluid_type(above)) {
        1.0
    } else {
        own_height(l, fluid)
    }
}

// FlowingFluid.isSourceBlockOfThisType.
fn is_source_block_of_this_type(l: &Level, this: Fluid, fluid: FluidId) -> bool {
    l.fluid_type(fluid).is_same(this) && l.reg.fluids[fluid as usize].source
}

// FlowingFluid.canHoldAnyFluid.
fn can_hold_any_fluid(l: &Level, state: State) -> bool {
    let block = l.block(state) as usize;
    let tables = tables(l);
    tables.container[block] || (l.reg.flags(state) & BLOCKS_MOTION == 0 && !tables.cannot_hold[block])
}

// LiquidBlockContainer.canPlaceLiquid(null, level, pos, state, fluid).
pub fn can_place_liquid(l: &Level, state: State, fluid: Fluid) -> bool {
    let block = &l.reg.blocks[l.block(state) as usize];
    match block.class.as_str() {
        // The growing plants and seagrasses refuse every fluid.
        "KelpBlock" | "KelpPlantBlock" | "SeagrassBlock" | "TallSeagrassBlock" => false,
        // SlabBlock: not a double slab, then SimpleWaterloggedBlock's.
        "SlabBlock" | "WeatheringCopperSlabBlock" => !is_double_slab(l, state) && fluid == Fluid::Water,
        _ => {
            if block.overrides.iter().any(|name| name == "canPlaceLiquid") {
                refuse(format!("{}.canPlaceLiquid ({})", block.class, l.reg.describe(state)));
            }
            // SimpleWaterloggedBlock.canPlaceLiquid.
            fluid == Fluid::Water
        }
    }
}

fn is_double_slab(l: &Level, state: State) -> bool {
    l.reg.value_name(state, l.reg.property_of(state, "type")) == "double"
}

// FlowingFluid.canHoldSpecificFluid.
fn can_hold_specific_fluid(l: &Level, state: State, fluid: Fluid) -> bool {
    !tables(l).container[l.block(state) as usize] || can_place_liquid(l, state, fluid)
}

// LiquidBlockContainer.placeLiquid(level, pos, state, fluidState).
pub fn place_liquid(l: &mut Level, pos: Pos, state: State, fluid: FluidId) -> bool {
    let block = &l.reg.blocks[l.block(state) as usize];
    match block.class.as_str() {
        "KelpBlock" | "KelpPlantBlock" | "SeagrassBlock" | "TallSeagrassBlock" => return false,
        "SlabBlock" | "WeatheringCopperSlabBlock" => {
            if is_double_slab(l, state) {
                return false;
            }
        }
        // CampfireBlock.placeLiquid: water puts a lit campfire out.
        "CampfireBlock" => {
            let waterlogged = l.reg.property_of(state, "waterlogged");
            let fluid_type = l.fluid_type(fluid);
            if l.reg.get_bool(state, waterlogged) || fluid_type != Fluid::Water {
                return false;
            }
            let lit = l.reg.property_of(state, "lit");
            if l.reg.get_bool(state, lit) {
                // dowse: the block changes for the game.
                let event = crate::game_event!(l, "block_change");
                l.game_event_at(None, event, pos);
            }
            l.set(pos, l.reg.with_bool(l.reg.with_bool(state, waterlogged, true), lit, false), UPDATE_ALL);
            l.schedule_fluid_tick(pos, fluid_type, tick_delay(fluid_type));
            return true;
        }
        // DriedGhastBlock.placeLiquid is SimpleWaterloggedBlock's with a sound.
        "DriedGhastBlock" => {}
        "CandleBlock" => return crate::blocks::decor::candle::place_liquid(l, pos, state, l.fluid_type(fluid)),
        _ => {
            if block.overrides.iter().any(|name| name == "placeLiquid") {
                refuse(format!("{}.placeLiquid ({})", block.class, l.reg.describe(state)));
            }
        }
    }
    // SimpleWaterloggedBlock.placeLiquid.
    let waterlogged = l.reg.property_of(state, "waterlogged");
    let fluid_type = l.fluid_type(fluid);
    if !l.reg.get_bool(state, waterlogged) && fluid_type == Fluid::Water {
        l.set(pos, l.reg.with_bool(state, waterlogged, true), UPDATE_ALL);
        l.schedule_fluid_tick(pos, fluid_type, tick_delay(fluid_type));
        true
    } else {
        false
    }
}

// What FlowingFluid.canPassThroughWall reads of a block: the number of its
// state cache's collision shape, or, for a block with a dynamic shape (no
// state cache), the shape it gives at its position.
enum WallSide {
    Cached(u16),
    Dynamic(std::borrow::Cow<'static, crate::registry::Shape>),
}

// The collision shape the block's getCollisionShape(level, pos) gives. A
// shulker box's is the full block while it is closed, as the dump has it (the
// port never opens one); scaffolding's and powder snow's (for no entity) are
// the dump's; bamboo and pointed dripstone offset the dump's shape by
// position; a moving piston has its block entity's.
fn wall_side(l: &Level, state: State, pos: Pos) -> WallSide {
    let data = &l.reg.states[state as usize];
    if data.flags & crate::registry::DYNAMIC_SHAPE == 0 {
        return WallSide::Cached(data.collision_shape);
    }
    match l.reg.blocks[data.block as usize].class.as_str() {
        "ShulkerBoxBlock" | "ScaffoldingBlock" | "PowderSnowBlock" => WallSide::Dynamic(std::borrow::Cow::Borrowed(&l.reg.shapes[data.collision_shape as usize])),
        "BambooStalkBlock" | "PointedDripstoneBlock" => WallSide::Dynamic(crate::shape_ops::collision_shape_at(l, state, pos)),
        "MovingPistonBlock" => WallSide::Dynamic(std::borrow::Cow::Owned(crate::blocks::moving_piston::shape_at(l, pos))),
        _ => refuse(format!("collision shape of {}", l.reg.describe(state))),
    }
}

// FlowingFluid.canPassThroughWall: whether the two blocks' collision shapes
// leave the face between them open (moonrise's cached form).
pub fn can_pass_through_wall(l: &Level, direction: Dir, from: State, from_pos: Pos, to: State, to_pos: Pos) -> bool {
    let (from_side, to_side) = (wall_side(l, from, from_pos), wall_side(l, to, to_pos));
    let reg = l.reg;
    // A block with a dynamic shape has no cache: neither flag is set.
    let cached = |side: &WallSide| match side {
        WallSide::Cached(id) => Some(&reg.shapes[*id as usize]),
        WallSide::Dynamic(_) => None,
    };
    let from_empty = cached(&from_side).is_some_and(|shape| shape.is_empty());
    let to_empty = cached(&to_side).is_some_and(|shape| shape.is_empty());
    if from_empty & to_empty {
        return true;
    }
    let from_occludes = cached(&from_side).is_some_and(|shape| shape.occludes_full_block);
    let to_occludes = cached(&to_side).is_some_and(|shape| shape.occludes_full_block);
    if from_occludes | to_occludes {
        return false;
    }
    match (&from_side, &to_side) {
        (WallSide::Cached(first), WallSide::Cached(second)) => !reg.merged_face_occludes(*first, *second, direction),
        _ => {
            fn shape_of<'a>(reg: &'a crate::registry::Registry, side: &'a WallSide) -> &'a crate::registry::Shape {
                match side {
                    WallSide::Cached(id) => &reg.shapes[*id as usize],
                    WallSide::Dynamic(shape) => shape,
                }
            }
            !crate::shape_ops::merged_face_occludes(shape_of(reg, &from_side), shape_of(reg, &to_side), direction)
        }
    }
}

// FlowingFluid.getNewLiquid: the fluid the block at `pos` should hold, from
// its neighbours.
pub fn get_new_liquid(l: &mut Level, this: Fluid, pos: Pos, state: State) -> FluidId {
    let reg = l.reg;
    let mut amount = 0;
    let mut sources = 0;
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        if let Some(neighbor) = l.get_if_loaded(at) {
            let fluid = l.fluid_of(neighbor);
            if l.fluid_type(fluid).is_same(this) && can_pass_through_wall(l, dir, state, pos, neighbor, at) {
                let data = &reg.fluids[fluid as usize];
                if data.source {
                    sources += 1;
                }
                amount = amount.max(data.amount);
            }
        }
    }
    if sources >= 2 && can_convert_to_source(this) {
        let below = l.get(pos.below());
        let below_fluid = l.fluid_of(below);
        if reg.flags(below) & SOLID != 0 || is_source_block_of_this_type(l, this, below_fluid) {
            return reg.fluid_state(source_kind(this), false, 0);
        }
    }
    let above_pos = pos.above();
    let above = l.get(above_pos);
    let above_fluid = l.fluid_of(above);
    if above_fluid != EMPTY_FLUID && l.fluid_type(above_fluid).is_same(this) && can_pass_through_wall(l, Dir::Up, state, pos, above, above_pos) {
        return reg.fluid_state(flowing_kind(this), true, 8);
    }
    let level = amount - drop_off(this);
    if level <= 0 {
        EMPTY_FLUID
    } else {
        reg.fluid_state(flowing_kind(this), false, level)
    }
}

// FlowingFluid.canMaybePassThrough.
fn can_maybe_pass_through(l: &Level, this: Fluid, from: State, from_pos: Pos, direction: Dir, to: State, to_pos: Pos, to_fluid: FluidId) -> bool {
    !is_source_block_of_this_type(l, this, to_fluid) && can_hold_any_fluid(l, to) && can_pass_through_wall(l, direction, from, from_pos, to, to_pos)
}

// FlowingFluid.canPassThrough, for the flowing fluid of the family.
fn can_pass_through(l: &Level, this: Fluid, from: State, from_pos: Pos, direction: Dir, to: State, to_pos: Pos, to_fluid: FluidId) -> bool {
    can_maybe_pass_through(l, this, from, from_pos, direction, to, to_pos, to_fluid) && can_hold_specific_fluid(l, to, flowing_kind(this))
}

// FlowingFluid.isWaterHole.
fn is_water_hole(l: &Level, this: Fluid, pos: Pos, pos_state: State, below_state: State) -> bool {
    can_pass_through_wall(l, Dir::Down, pos_state, pos, below_state, pos.below())
        && (l.fluid_type(l.fluid_of(below_state)).is_same(this) || (can_hold_any_fluid(l, below_state) && can_hold_specific_fluid(l, below_state, flowing_kind(this))))
}

// FlowingFluid.SpreadContext: the origin of a search and the holes it found.
struct SpreadContext {
    origin: Pos,
    holes: Vec<(i32, i32, bool)>,
}

impl SpreadContext {
    // The block at `pos` (getBlockStateIfLoaded: a chunk that is not loaded
    // gives none).
    fn state_if_loaded(&self, l: &mut Level, pos: Pos) -> Option<State> {
        l.get_if_loaded(pos)
    }

    fn is_hole(&mut self, l: &mut Level, this: Fluid, pos: Pos) -> bool {
        let key = (pos.x() - self.origin.x(), pos.z() - self.origin.z());
        if let Some(&(_, _, hole)) = self.holes.iter().find(|&&(x, z, _)| (x, z) == key) {
            return hole;
        }
        let state = l.get(pos);
        let below = l.get(pos.below());
        let hole = is_water_hole(l, this, pos, state, below);
        self.holes.push((key.0, key.1, hole));
        hole
    }
}

// FlowingFluid.getSlopeDistance.
fn get_slope_distance(l: &mut Level, this: Fluid, pos: Pos, depth: i32, from: Dir, state: State, context: &mut SpreadContext) -> i32 {
    let mut best = 1000;
    for dir in HORIZONTAL {
        if dir != from {
            let at = pos.relative(dir);
            if let Some(neighbor) = context.state_if_loaded(l, at) {
                let fluid = l.fluid_of(neighbor);
                if can_pass_through(l, this, state, pos, dir, neighbor, at, fluid) {
                    if context.is_hole(l, this, at) {
                        return depth;
                    }
                    if depth < slope_find_distance(this) {
                        let distance = get_slope_distance(l, this, at, depth + 1, dir.opposite(), neighbor, context);
                        if distance < best {
                            best = distance;
                        }
                    }
                }
            }
        }
    }
    best
}

// FluidState.canBeReplacedWith: the replaced fluid's own rule.
fn can_be_replaced_with(l: &mut Level, replaced: FluidId, pos: Pos, fluid: Fluid, direction: Dir) -> bool {
    match l.fluid_type(replaced) {
        Fluid::Empty => true,
        Fluid::FlowingWater | Fluid::Water => direction == Dir::Down && !fluid.is_water(),
        Fluid::FlowingLava | Fluid::Lava => height(l, replaced, pos) >= 0.44444445f32 && fluid.is_water(),
    }
}

// FlowingFluid.getSpread: the sides to spread to, in Direction's order (the
// EnumMap's).
fn get_spread(l: &mut Level, this: Fluid, pos: Pos, state: State) -> Vec<(Dir, FluidId)> {
    let mut best = 1000;
    let mut map: Vec<(Dir, FluidId)> = Vec::new();
    let mut context: Option<SpreadContext> = None;
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        let Some(neighbor) = l.get_if_loaded(at) else { continue };
        let fluid = l.fluid_of(neighbor);
        if !can_maybe_pass_through(l, this, state, pos, dir, neighbor, at, fluid) {
            continue;
        }
        let new_liquid = get_new_liquid(l, this, at, neighbor);
        if !can_hold_specific_fluid(l, neighbor, l.fluid_type(new_liquid)) {
            continue;
        }
        let context = context.get_or_insert_with(|| SpreadContext { origin: pos, holes: Vec::new() });
        let distance = if context.is_hole(l, this, at) { 0 } else { get_slope_distance(l, this, at, 1, dir.opposite(), neighbor, context) };
        if distance < best {
            map.clear();
        }
        if distance <= best {
            if can_be_replaced_with(l, fluid, at, l.fluid_type(new_liquid), dir) {
                map.retain(|&(d, _)| d != dir);
                map.push((dir, new_liquid));
            }
            best = distance;
        }
    }
    map.sort_by_key(|&(dir, _)| dir as u8);
    map
}

// FlowingFluid.sourceNeighborCount.
fn source_neighbor_count(l: &mut Level, this: Fluid, pos: Pos) -> i32 {
    let mut count = 0;
    for dir in HORIZONTAL {
        let fluid = l.fluid_state(pos.relative(dir));
        if is_source_block_of_this_type(l, this, fluid) {
            count += 1;
        }
    }
    count
}

// FlowingFluid.spreadTo, with LavaFluid's override.
fn spread_to(l: &mut Level, this: Fluid, pos: Pos, state: State, direction: Dir, fluid: FluidId) {
    if this.is_lava() && direction == Dir::Down {
        let there = l.fluid_state(pos);
        if l.fluid_type(there).is_water() {
            // BlockFormEvent: stone forms where lava meets a liquid block;
            // another block (a waterlogged one) is left as it is.
            if l.reg.blocks[l.block(state) as usize].class == "LiquidBlock" {
                let stone = l.reg.default_state(l.reg.block_key("minecraft:stone"));
                if !l.set(pos, stone, UPDATE_ALL) {
                    return;
                }
            }
            // fizz: a level event, which reaches nobody.
            return;
        }
    }
    if tables(l).container[l.block(state) as usize] {
        place_liquid(l, pos, state, fluid);
    } else {
        if !l.is_air(state) {
            before_destroying_block(l, this, pos, state);
        }
        l.set(pos, l.reg.fluids[fluid as usize].legacy, UPDATE_ALL);
    }
}

// FlowingFluid.beforeDestroyingBlock: water drops what it destroys, lava
// fizzes (a level event that reaches nobody).
fn before_destroying_block(l: &mut Level, this: Fluid, pos: Pos, state: State) {
    if this.is_water() {
        if l.reg.flags(state) & BLOCK_ENTITY != 0 {
            l.block_entity(pos);
        }
        item::drop_resources_by_block(l, state, pos);
    }
}

// FlowingFluid.spreadToSides.
fn spread_to_sides(l: &mut Level, this: Fluid, pos: Pos, fluid: FluidId, state: State) {
    let mut amount = l.reg.fluids[fluid as usize].amount - drop_off(this);
    if l.reg.fluids[fluid as usize].falling {
        amount = 7;
    }
    if amount > 0 {
        for (dir, new_liquid) in get_spread(l, this, pos, state) {
            let at = pos.relative(dir);
            if let Some(there) = l.get_if_loaded(at) {
                spread_to(l, this, at, there, dir, new_liquid);
            }
        }
    }
}

// FlowingFluid.spread.
fn spread(l: &mut Level, this: Fluid, pos: Pos, state: State, fluid: FluidId) {
    if fluid == EMPTY_FLUID {
        return;
    }
    let below_pos = pos.below();
    let below = l.get(below_pos);
    let below_fluid = l.fluid_of(below);
    if can_maybe_pass_through(l, this, state, pos, Dir::Down, below, pos.below(), below_fluid) {
        let new_liquid = get_new_liquid(l, this, below_pos, below);
        let new_type = l.fluid_type(new_liquid);
        if can_be_replaced_with(l, below_fluid, below_pos, new_type, Dir::Down) && can_hold_specific_fluid(l, below, new_type) {
            spread_to(l, this, below_pos, below, Dir::Down, new_liquid);
            if source_neighbor_count(l, this, pos) >= 3 {
                spread_to_sides(l, this, pos, fluid, state);
            }
            return;
        }
    }
    if l.reg.fluids[fluid as usize].source || !is_water_hole(l, this, pos, state, below) {
        spread_to_sides(l, this, pos, fluid, state);
    }
}

// FlowingFluid.getSpreadDelay: water's and lava's tick delay, lava's four
// times longer (three times in four) when it rises to a fuller state.
fn spread_delay(l: &mut Level, this: Fluid, pos: Pos, current: FluidId, new: FluidId) -> i32 {
    let delay = tick_delay(this);
    if this.is_lava()
        && current != EMPTY_FLUID
        && new != EMPTY_FLUID
        && !l.reg.fluids[current as usize].falling
        && !l.reg.fluids[new as usize].falling
        && height(l, new, pos) > height(l, current, pos)
        && l.random.next_int_bound(4) != 0
    {
        return delay * 4;
    }
    delay
}

// FlowingFluid.tick (FluidState.tick): `state` is the block at `pos`,
// `fluid` its fluid state.
pub fn tick(l: &mut Level, pos: Pos, state: State, fluid: FluidId) {
    let reg = l.reg;
    let this = reg.fluids[fluid as usize].fluid;
    let (mut state, mut fluid) = (state, fluid);
    if !reg.fluids[fluid as usize].source {
        let here = l.get(pos);
        let new_liquid = get_new_liquid(l, this, pos, here);
        let delay = spread_delay(l, this, pos, fluid, new_liquid);
        if new_liquid == EMPTY_FLUID {
            fluid = new_liquid;
            state = l.known.air;
            l.set(pos, state, UPDATE_ALL);
        } else if new_liquid != fluid {
            fluid = new_liquid;
            state = reg.fluids[new_liquid as usize].legacy;
            l.set(pos, state, UPDATE_ALL);
            l.schedule_fluid_tick(pos, reg.fluids[new_liquid as usize].fluid, delay);
        }
    }
    spread(l, this, pos, state, fluid);
}

// FlowingFluid.affectsFlow.
fn affects_flow(l: &Level, this: Fluid, fluid: FluidId) -> bool {
    fluid == EMPTY_FLUID || l.fluid_type(fluid).is_same(this)
}

// FlowingFluid.isSolidFace.
fn is_solid_face(l: &mut Level, this: Fluid, pos: Pos, side: Dir) -> bool {
    let state = l.get(pos);
    let fluid = l.fluid_state(pos);
    if l.fluid_type(fluid).is_same(this) {
        return false;
    }
    let block = &l.reg.blocks[l.block(state) as usize];
    let ice = block.class == "IceBlock" || block.supers.iter().any(|s| s == "IceBlock");
    side == Dir::Up || (!ice && crate::shape_ops::is_face_sturdy(l, state, pos, side, crate::registry::Support::Full))
}

// FlowingFluid.getFlow: the direction the fluid at `pos` pushes.
pub fn get_flow(l: &mut Level, pos: Pos, fluid: FluidId) -> crate::phys::Vec3 {
    use crate::phys::Vec3;
    let this = l.fluid_type(fluid);
    let (mut d, mut d1) = (0.0f64, 0.0f64);
    for dir in HORIZONTAL {
        let at = pos.relative(dir);
        let neighbor = l.fluid_state(at);
        if affects_flow(l, this, neighbor) {
            let mut neighbor_height = own_height(l, neighbor);
            let mut f = 0.0f32;
            if neighbor_height == 0.0 {
                let state = l.get(at);
                if l.reg.flags(state) & BLOCKS_MOTION == 0 {
                    let below = l.fluid_state(at.below());
                    if affects_flow(l, this, below) {
                        neighbor_height = own_height(l, below);
                        if neighbor_height > 0.0 {
                            f = own_height(l, fluid) - (neighbor_height - 0.8888889f32);
                        }
                    }
                }
            } else if neighbor_height > 0.0 {
                f = own_height(l, fluid) - neighbor_height;
            }
            if f != 0.0 {
                let (dx, _, dz) = dir.offset();
                d += (dx as f32 * f) as f64;
                d1 += (dz as f32 * f) as f64;
            }
        }
    }
    let mut vec = Vec3::new(d, 0.0, d1);
    if l.reg.fluids[fluid as usize].falling {
        for dir in HORIZONTAL {
            let at = pos.relative(dir);
            if is_solid_face(l, this, at, dir) || is_solid_face(l, this, at.above(), dir) {
                vec = vec.normalize().add(0.0, -6.0, 0.0);
                break;
            }
        }
    }
    vec.normalize()
}

// The blocks the fluid code names, by id and default state.
pub struct Named {
    pub water: BlockId,
    pub bubble_column: BlockId,
    pub magma_block: BlockId,
    pub soul_sand: BlockId,
    pub kelp: BlockId,
    pub kelp_plant: BlockId,
    pub seagrass: BlockId,
    pub tall_seagrass: BlockId,
    pub wet_sponge: BlockId,
}

pub fn named(l: &Level) -> &'static Named {
    static NAMED: OnceLock<Named> = OnceLock::new();
    NAMED.get_or_init(|| {
        let reg = l.reg;
        Named {
            water: reg.block_key("minecraft:water"),
            bubble_column: reg.block_key("minecraft:bubble_column"),
            magma_block: reg.block_key("minecraft:magma_block"),
            soul_sand: reg.block_key("minecraft:soul_sand"),
            kelp: reg.block_key("minecraft:kelp"),
            kelp_plant: reg.block_key("minecraft:kelp_plant"),
            seagrass: reg.block_key("minecraft:seagrass"),
            tall_seagrass: reg.block_key("minecraft:tall_seagrass"),
            wet_sponge: reg.block_key("minecraft:wet_sponge"),
        }
    })
}
