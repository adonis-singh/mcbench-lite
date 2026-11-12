// Block behaviour: each ported block class's code (a module per Java class,
// exporting its Behaviour), and for every other class Minecraft's default
// behaviour (BlockBehaviour) for the methods it does not override. A method
// a class overrides that its module does not port refuses.

use crate::dir::{Dir, Pos};
use crate::entity::Entity;
use crate::level::{refuse, Level};
use crate::registry::{BlockId, Registry, State};

pub mod barrel;
pub mod bell;
pub mod block_entities;
pub mod brewing_stand;
pub mod bubble_column;
pub mod button;
pub mod cake;
pub mod cauldron;
pub mod chest;
pub mod chiseled_bookshelf;
pub mod comparator;
pub mod composter;
pub mod container;
pub mod copper_bulb;
pub mod crafter;
pub mod daylight_detector;
pub mod decor;
pub mod diode;
pub mod dispenser;
pub mod dripstone;
pub mod dropper;
pub mod ender_chest;
pub mod falling;
pub mod entities;
pub mod furnace;
pub mod growing;
pub mod hopper;
pub mod jukebox;
pub mod kelp;
pub mod lamp;
pub mod inventory;
pub mod lectern;
pub mod load;
pub mod honey;
pub mod lever;
pub mod misc;
pub mod moving_piston;
pub mod liquid_block;
pub mod multiface;
pub mod note;
pub mod observer;
pub mod plants;
pub mod piston;
pub mod piston_head;
pub mod piston_resolver;
pub mod powered;
pub mod rail;
pub mod dispense;
pub mod potion_brewing;
pub mod recipes;
pub mod repeater;
pub mod scaffolding;
pub mod seagrass;
pub mod shapes;
pub mod shulker_box;
pub mod slime;
pub mod special;
pub mod sponge;
pub mod target;
pub mod terrain;
pub mod tnt;
pub mod torch;
pub mod trapped_chest;
pub mod tripwire;
pub mod tripwire_hook;
pub mod vegetation;
pub mod wall_torch;
pub mod wire;

// A property's id (Registry.property), looked up once:
// property!(l, "lit", ["true", "false"]).
#[macro_export]
macro_rules! property {
    ($l:expr, $name:literal, [$($value:literal),*]) => {{
        static ID: std::sync::OnceLock<u16> = std::sync::OnceLock::new();
        *ID.get_or_init(|| $l.reg.property($name, &[$($value),*]))
    }};
}

// The behaviour methods the dump names (McbenchEngine.BEHAVIOUR).
pub const BEHAVIOUR: [&str; 37] = [
    "updateShape", "neighborChanged", "onPlace", "affectNeighborsAfterRemoval", "updateIndirectNeighbourShapes", "tick", "randomTick",
    "getSignal", "getDirectSignal", "isSignalSource", "hasAnalogOutputSignal", "getAnalogOutputSignal", "triggerEvent", "canSurvive",
    "entityInside", "stepOn", "fallOn", "updateEntityMovementAfterFallOn", "onProjectileHit", "useWithoutItem", "useItemOn", "attack",
    "onExplosionHit", "playerWillDestroy", "destroy", "setPlacedBy", "newBlockEntity", "getTicker", "isRandomlyTicking", "onRemove",
    "spawnAfterBreak", "getInputSignal", "getOutputSignal", "getCollisionShape", "getShape", "canPlaceLiquid", "placeLiquid",
];

fn method(name: &str) -> u64 {
    1 << BEHAVIOUR.iter().position(|&m| m == name).unwrap_or_else(|| panic!("no behaviour method {name}"))
}

// A behaviour method's bit, from its name, at compile time.
const fn bit(name: &str) -> u64 {
    let mut i = 0;
    while i < BEHAVIOUR.len() {
        if const_eq(BEHAVIOUR[i], name) {
            return 1 << i;
        }
        i += 1;
    }
    panic!("no behaviour method")
}

const fn const_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// A class's ported methods, each as Minecraft's Block method of the same
// name with the level and position. isSignalSource and
// hasAnalogOutputSignal come from the registry (the dumped state data).
#[derive(Clone, Copy)]
pub struct Behaviour {
    pub update_shape: Option<fn(&mut Level, State, Pos, Dir, Pos, State) -> State>,
    pub neighbor_changed: Option<fn(&mut Level, State, Pos, BlockId, bool)>,
    pub on_place: Option<fn(&mut Level, State, Pos, State, bool)>,
    pub affect_neighbors_after_removal: Option<fn(&mut Level, State, Pos, bool)>,
    pub update_indirect_neighbour_shapes: Option<fn(&mut Level, State, Pos, i32, i32)>,
    pub tick: Option<fn(&mut Level, State, Pos)>,
    pub get_signal: Option<fn(&mut Level, State, Pos, Dir) -> i32>,
    pub get_direct_signal: Option<fn(&mut Level, State, Pos, Dir) -> i32>,
    pub get_analog_output_signal: Option<fn(&mut Level, State, Pos, Dir) -> i32>,
    pub trigger_event: Option<fn(&mut Level, State, Pos, i32, i32) -> bool>,
    pub can_survive: Option<fn(&mut Level, State, Pos) -> bool>,
    // EntityBlock.newBlockEntity.
    pub new_block_entity: Option<fn(&mut Level, State, Pos) -> BlockEntity>,
    // useWithoutItem by a player with an empty hand, as McbenchEngine.input
    // makes it: LeverBlock.pull and ButtonBlock.press with no player, and the
    // blocks of a "use" input with the input player. Whether the block
    // consumed the action (InteractionResult.consumesAction).
    pub use_without_item: Option<fn(&mut Level, State, Pos, Option<InputPlayer>) -> bool>,
    // Block.stepOn: an entity standing on the block at the position.
    pub step_on: Option<fn(&mut Level, Pos, State, &Entity)>,
    // The block entity ticker getTicker gives (BlockEntityTicker.tick with
    // the block entity at the position), for a class that overrides
    // getTicker; None where getTicker gives none for the state.
    pub ticker: Option<fn(&Level, State) -> Option<fn(&mut Level, Pos, State)>>,
    // The methods an entity reaches (Block.fallOn(level, state, pos, entity,
    // fallDistance), stepOn(level, pos, state, entity),
    // entityInside(state, level, pos, entity...), and
    // updateEntityMovementAfterFallOn(level, entity)).
    pub fall_on: Option<fn(&mut Level, State, Pos, &mut Entity, f64)>,
    pub entity_inside: Option<fn(&mut Level, State, Pos, &mut Entity, &mut crate::entity::inside::Applier, bool)>,
    pub update_entity_movement_after_fall_on: Option<fn(&mut Level, &mut Entity)>,
    // Block.stepOn for a class that changes the entity (step_on only reads it).
    pub step_on_mut: Option<fn(&mut Level, Pos, State, &mut Entity)>,
    // The class's entity methods above are written for every kind of entity
    // the port has (the others' model item entities only).
    pub all_entities: bool,
}

pub const NONE: Behaviour = Behaviour {
    update_shape: None,
    neighbor_changed: None,
    on_place: None,
    affect_neighbors_after_removal: None,
    update_indirect_neighbour_shapes: None,
    tick: None,
    get_signal: None,
    get_direct_signal: None,
    get_analog_output_signal: None,
    trigger_event: None,
    can_survive: None,
    new_block_entity: None,
    use_without_item: None,
    step_on: None,
    ticker: None,
    fall_on: None,
    entity_inside: None,
    update_entity_movement_after_fall_on: None,
    step_on_mut: None,
    all_entities: false,
};

// The ported classes, by simple name.
const CLASSES: &[(&str, Behaviour)] = &[
    ("RedStoneWireBlock", wire::BEHAVIOUR),
    ("RepeaterBlock", repeater::BEHAVIOUR),
    ("ComparatorBlock", comparator::BEHAVIOUR),
    ("RedstoneTorchBlock", torch::BEHAVIOUR),
    ("RedstoneWallTorchBlock", wall_torch::BEHAVIOUR),
    ("LeverBlock", lever::BEHAVIOUR),
    ("ButtonBlock", button::BEHAVIOUR),
    ("RedstoneLampBlock", lamp::BEHAVIOUR),
    ("ObserverBlock", observer::BEHAVIOUR),
    ("TargetBlock", target::BEHAVIOUR),
    ("PoweredBlock", powered::BEHAVIOUR),
    ("NoteBlock", note::BEHAVIOUR),
    // The shape and attachment blocks (blocks/shapes).
    ("SlabBlock", shapes::slab::BEHAVIOUR),
    ("WeatheringCopperSlabBlock", shapes::slab::BEHAVIOUR),
    ("StairBlock", shapes::stairs::BEHAVIOUR),
    ("WeatheringCopperStairBlock", shapes::stairs::BEHAVIOUR),
    ("TrapDoorBlock", shapes::trapdoor::BEHAVIOUR),
    ("WeatheringCopperTrapDoorBlock", shapes::trapdoor::BEHAVIOUR),
    ("DoorBlock", shapes::door::BEHAVIOUR),
    ("WeatheringCopperDoorBlock", shapes::door::BEHAVIOUR),
    ("FenceGateBlock", shapes::fence_gate::BEHAVIOUR),
    ("FenceBlock", shapes::fence::BEHAVIOUR),
    ("WallBlock", shapes::wall::BEHAVIOUR),
    ("IronBarsBlock", shapes::iron_bars::BEHAVIOUR),
    ("StainedGlassPaneBlock", shapes::stained_glass_pane::BEHAVIOUR),
    ("WeatheringCopperBarsBlock", shapes::iron_bars::BEHAVIOUR),
    ("WallSignBlock", shapes::wall_sign::BEHAVIOUR),
    ("StandingSignBlock", shapes::standing_sign::BEHAVIOUR),
    ("WallHangingSignBlock", shapes::wall_hanging_sign::BEHAVIOUR),
    ("CeilingHangingSignBlock", shapes::ceiling_hanging_sign::BEHAVIOUR),
    ("LadderBlock", shapes::ladder::BEHAVIOUR),
    ("CarvedPumpkinBlock", shapes::carved_pumpkin::BEHAVIOUR),
    ("BannerBlock", shapes::banner::BEHAVIOUR),
    ("WallBannerBlock", shapes::wall_banner::BEHAVIOUR),
    ("CoralFanBlock", shapes::coral_fan::BEHAVIOUR),
    ("CoralPlantBlock", shapes::coral_fan::BEHAVIOUR),
    ("CoralWallFanBlock", shapes::coral_wall_fan::BEHAVIOUR),
    ("BaseCoralFanBlock", shapes::base_coral_fan::BEHAVIOUR),
    ("BaseCoralPlantBlock", shapes::base_coral_fan::BEHAVIOUR),
    ("BaseCoralWallFanBlock", shapes::base_coral_wall_fan::BEHAVIOUR),
    ("CoralBlock", shapes::coral::BLOCK_BEHAVIOUR),
    ("BarrelBlock", barrel::BEHAVIOUR),
    ("ChestBlock", chest::BEHAVIOUR),
    ("TrappedChestBlock", trapped_chest::BEHAVIOUR),
    ("EnderChestBlock", ender_chest::BEHAVIOUR),
    ("ShulkerBoxBlock", shulker_box::BEHAVIOUR),
    ("HopperBlock", hopper::BEHAVIOUR),
    ("DispenserBlock", dispenser::BEHAVIOUR),
    ("DropperBlock", dropper::BEHAVIOUR),
    ("CrafterBlock", crafter::BEHAVIOUR),
    // Falling blocks and TNT.
    ("SandBlock", falling::BEHAVIOUR),
    ("ColoredFallingBlock", falling::BEHAVIOUR),
    ("AnvilBlock", falling::BEHAVIOUR),
    ("DragonEggBlock", falling::BEHAVIOUR),
    ("ConcretePowderBlock", falling::CONCRETE_POWDER),
    ("TntBlock", tnt::BEHAVIOUR),
    ("ScaffoldingBlock", scaffolding::BEHAVIOUR),
    ("PointedDripstoneBlock", dripstone::BEHAVIOUR),
    ("ComposterBlock", composter::BEHAVIOUR),
    ("LayeredCauldronBlock", cauldron::LAYERED),
    ("LavaCauldronBlock", cauldron::LAVA),
    ("LecternBlock", lectern::BEHAVIOUR),
    ("FurnaceBlock", furnace::FURNACE),
    ("BlastFurnaceBlock", furnace::BLAST_FURNACE),
    ("SmokerBlock", furnace::SMOKER),
    ("BrewingStandBlock", brewing_stand::BEHAVIOUR),
    ("JukeboxBlock", jukebox::BEHAVIOUR),
    ("ChiseledBookShelfBlock", chiseled_bookshelf::BEHAVIOUR),
    ("CakeBlock", cake::CAKE),
    ("CandleCakeBlock", cake::CANDLE_CAKE),
    ("BellBlock", bell::BEHAVIOUR),
    ("CopperBulbBlock", copper_bulb::BEHAVIOUR),
    ("WeatheringCopperBulbBlock", copper_bulb::BEHAVIOUR),
    ("DaylightDetectorBlock", daylight_detector::BEHAVIOUR),
    ("TripWireBlock", tripwire::BEHAVIOUR),
    ("TripWireHookBlock", tripwire_hook::BEHAVIOUR),
    ("PistonBaseBlock", piston::BEHAVIOUR),
    ("PistonHeadBlock", piston_head::BEHAVIOUR),
    ("MovingPistonBlock", moving_piston::BEHAVIOUR),
    ("SlimeBlock", slime::BEHAVIOUR),
    ("HoneyBlock", honey::BEHAVIOUR),
    ("RailBlock", rail::RAIL),
    ("PoweredRailBlock", rail::POWERED),
    ("DetectorRailBlock", rail::DETECTOR),
    // Fluids and what holds them.
    ("LiquidBlock", liquid_block::BEHAVIOUR),
    ("BubbleColumnBlock", bubble_column::BEHAVIOUR),
    ("SoulSandBlock", bubble_column::COLUMN_BASE_BEHAVIOUR),
    ("MagmaBlock", bubble_column::COLUMN_BASE_BEHAVIOUR),
    ("KelpBlock", kelp::HEAD_BEHAVIOUR),
    ("KelpPlantBlock", kelp::BODY_BEHAVIOUR),
    ("SeagrassBlock", seagrass::BEHAVIOUR),
    ("TallSeagrassBlock", seagrass::TALL_BEHAVIOUR),
    ("SpongeBlock", sponge::BEHAVIOUR),
    ("WetSpongeBlock", sponge::WET_BEHAVIOUR),
    // The special blocks (blocks/special).
    ("PressurePlateBlock", special::pressure_plate::BEHAVIOUR),
    ("PowderSnowBlock", special::powder_snow::BEHAVIOUR),
    ("WeightedPressurePlateBlock", special::pressure_plate::BEHAVIOUR),
    ("FireBlock", special::fire::FIRE),
    ("SoulFireBlock", special::fire::SOUL_FIRE),
    ("RedStoneOreBlock", special::redstone_ore::BEHAVIOUR),
    ("FrostedIceBlock", special::frosted_ice::BEHAVIOUR),
    ("SculkSensorBlock", special::sculk_sensor::SENSOR),
    ("CalibratedSculkSensorBlock", special::sculk_sensor::CALIBRATED),
    ("SculkCatalystBlock", special::sculk_catalyst::BEHAVIOUR),
    ("SculkShriekerBlock", special::sculk_shrieker::BEHAVIOUR),
    ("CommandBlock", special::command_block::BEHAVIOUR),
    ("SpawnerBlock", special::spawner::BEHAVIOUR),
    ("TrialSpawnerBlock", special::trial_spawner::BEHAVIOUR),
    ("VaultBlock", special::vault::BEHAVIOUR),
    ("BeaconBlock", special::beacon::BEHAVIOUR),
    ("ConduitBlock", special::conduit::BEHAVIOUR),
    ("EnchantingTableBlock", special::enchanting_table::BEHAVIOUR),
    ("EndPortalBlock", special::end_portal::BEHAVIOUR),
    ("EndGatewayBlock", special::end_gateway::BEHAVIOUR),
    ("EndPortalFrameBlock", special::end_portal_frame::BEHAVIOUR),
    ("NetherPortalBlock", special::nether_portal::BEHAVIOUR),
    ("RespawnAnchorBlock", special::respawn_anchor::BEHAVIOUR),
    ("BarrierBlock", special::waterlogged::BEHAVIOUR),
    ("LightBlock", special::waterlogged::BEHAVIOUR),
    ("HeavyCoreBlock", special::waterlogged::BEHAVIOUR),
    ("StructureBlock", special::structure_block::BEHAVIOUR),
    ("JigsawBlock", special::jigsaw::BEHAVIOUR),
    ("TestBlock", special::test_block::BEHAVIOUR),
    ("TestInstanceBlock", special::test_instance_block::BEHAVIOUR),
    ("GrindstoneBlock", special::grindstone::BEHAVIOUR),
    ("CreakingHeartBlock", special::creaking_heart::BEHAVIOUR),
    ("DecoratedPotBlock", special::decorated_pot::BEHAVIOUR),
    ("BrushableBlock", special::suspicious::BEHAVIOUR),
    ("LightningRodBlock", special::lightning_rod::BEHAVIOUR),
    ("BeehiveBlock", special::beehive::BEHAVIOUR),
    ("CampfireBlock", special::campfire::BEHAVIOUR),
    ("AmethystClusterBlock", special::amethyst_cluster::BEHAVIOUR),
    ("WeatheringLightningRodBlock", special::lightning_rod::BEHAVIOUR),
];

// Every ported class: this module's, then the plant groups'.
fn classes() -> impl Iterator<Item = &'static (&'static str, Behaviour)> {
    CLASSES.iter().chain(plants::CLASSES).chain(growing::CLASSES).chain(decor::CLASSES).chain(block_entities::CLASSES).chain(terrain::CLASSES).chain(misc::CLASSES).chain(multiface::CLASSES)
}

// A block's class: 0 for a class not ported, else 1 + its index in CLASSES.
pub type Kind = u16;

// Block entities this port models.
#[derive(Clone, Debug)]
pub enum BlockEntity {
    // ComparatorBlockEntity: the signal the comparator last output.
    Comparator { output: i32 },
    // SignBlockEntity, HangingSignBlockEntity and BannerBlockEntity: nothing
    // of them reaches a block's state.
    Sign(entities::Sign),
    HangingSign(entities::Sign),
    // BannerBlockEntity: its custom name and its pattern layers (as the tags
    // BannerPatternLayers.CODEC writes them).
    Banner { name: Option<crate::nbt::Tag>, patterns: Vec<crate::nbt::Tag> },
    // The container block entities (container.rs).
    Container(container::Container),
    // The block entities of the redstone-adjacent blocks (entities.rs).
    Entity(entities::Entity),
    // The special blocks' block entities (special/mod.rs).
    Special(special::Special),
    // PistonMovingBlockEntity (moving_piston.rs).
    Piston(moving_piston::Moving),
    // The skulls, beds, shelves and copper golem statues (block_entities).
    Plain(block_entities::Plain),
}

// What every block entity holds besides its own fields (BlockEntity.components
// and Bukkit's persistent data container), as saved data gave it: the
// components as the tag the codec writes, and the container's compound.
#[derive(Clone, Debug, Default)]
pub struct Extras {
    pub components: crate::nbt::Compound,
    pub bukkit: crate::nbt::Compound,
}

impl Extras {
    pub fn is_empty(&self) -> bool {
        self.components.is_empty() && self.bukkit.is_empty()
    }
}

pub const NEEDS_UPDATE_SHAPE: u8 = 1;
pub const NEEDS_NEIGHBOR_CHANGED: u8 = 2;
pub const NEEDS_GET_SIGNAL: u8 = 4;
pub const NEEDS_GET_DIRECT_SIGNAL: u8 = 8;

pub struct Blocks {
    pub kinds: Vec<Kind>,
    // The kind of each state's block.
    pub state_kinds: Vec<Kind>,
    // For each state, the bits of the methods below that its block has to do
    // something for: a ported method, or a method the class overrides (which
    // refuses where not ported). A method whose bit is clear is Minecraft's
    // default for the block, which does nothing.
    pub state_needs: Vec<u8>,
    pub overrides: Vec<u64>,
    pub behaviours: Vec<Behaviour>,
}

impl Blocks {
    pub fn new(reg: &Registry) -> Blocks {
        let kinds: Vec<Kind> = reg
            .blocks
            .iter()
            .map(|block| classes().position(|(class, _)| *class == block.class).map_or(0, |i| i as Kind + 1))
            .collect();
        let overrides: Vec<u64> = reg.blocks.iter().map(|block| block.overrides.iter().fold(0, |mask, name| mask | method(name))).collect();
        let behaviours: Vec<Behaviour> = std::iter::once(NONE).chain(classes().map(|(_, b)| *b)).collect();
        let state_kinds: Vec<Kind> = (0..reg.states.len()).map(|state| kinds[reg.block(state as State) as usize]).collect();
        let state_needs = (0..reg.states.len())
            .map(|state| {
                let block = reg.block(state as State) as usize;
                let behaviour = &behaviours[state_kinds[state] as usize];
                let has = |ported: bool, name: u64| ported || overrides[block] & name != 0;
                (has(behaviour.update_shape.is_some(), bit("updateShape")) as u8) * NEEDS_UPDATE_SHAPE
                    | (has(behaviour.neighbor_changed.is_some(), bit("neighborChanged")) as u8) * NEEDS_NEIGHBOR_CHANGED
                    | (has(behaviour.get_signal.is_some(), bit("getSignal")) as u8) * NEEDS_GET_SIGNAL
                    | (has(behaviour.get_direct_signal.is_some(), bit("getDirectSignal")) as u8) * NEEDS_GET_DIRECT_SIGNAL
            })
            .collect();
        Blocks { kinds, state_kinds, state_needs, overrides, behaviours }
    }
}

#[inline]
fn behaviour(l: &Level, state: State) -> &'static Behaviour {
    &l.blocks.behaviours[l.kind(state) as usize]
}

// Minecraft's default for a method the class does not override; a refusal
// for one it overrides that is not ported.
macro_rules! default {
    ($l:expr, $state:expr, $name:literal) => {{
        const BIT: u64 = bit($name);
        if $l.blocks.overrides[$l.block($state) as usize] & BIT != 0 {
            unported($l, $state, $name);
        }
    }};
}

#[cold]
fn unported(l: &Level, state: State, name: &str) -> ! {
    refuse(format!("{}.{} ({})", l.reg.blocks[l.block(state) as usize].class, name, l.reg.describe(state)))
}

#[inline(always)]
pub fn update_shape(l: &mut Level, state: State, pos: Pos, dir: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    if l.blocks.state_needs[state as usize] & NEEDS_UPDATE_SHAPE == 0 {
        return state;
    }
    update_shape_of(l, state, pos, dir, neighbor_pos, neighbor)
}

#[inline(never)]
fn update_shape_of(l: &mut Level, state: State, pos: Pos, dir: Dir, neighbor_pos: Pos, neighbor: State) -> State {
    match behaviour(l, state).update_shape {
        Some(f) => f(l, state, pos, dir, neighbor_pos, neighbor),
        None => {
            default!(l, state, "updateShape");
            state
        }
    }
}

#[inline(always)]
pub fn neighbor_changed(l: &mut Level, state: State, pos: Pos, neighbor_block: BlockId, moved: bool) {
    if l.blocks.state_needs[state as usize] & NEEDS_NEIGHBOR_CHANGED == 0 {
        return;
    }
    neighbor_changed_of(l, state, pos, neighbor_block, moved)
}

#[inline(never)]
fn neighbor_changed_of(l: &mut Level, state: State, pos: Pos, neighbor_block: BlockId, moved: bool) {
    match behaviour(l, state).neighbor_changed {
        Some(f) => f(l, state, pos, neighbor_block, moved),
        None => default!(l, state, "neighborChanged"),
    }
}

pub fn on_place(l: &mut Level, state: State, pos: Pos, old: State, moved: bool) {
    match behaviour(l, state).on_place {
        Some(f) => f(l, state, pos, old, moved),
        None => default!(l, state, "onPlace"),
    }
}

pub fn affect_neighbors_after_removal(l: &mut Level, state: State, pos: Pos, moved: bool) {
    match behaviour(l, state).affect_neighbors_after_removal {
        Some(f) => f(l, state, pos, moved),
        None => default!(l, state, "affectNeighborsAfterRemoval"),
    }
}

pub fn update_indirect_neighbour_shapes(l: &mut Level, state: State, pos: Pos, flags: i32, recursion: i32) {
    match behaviour(l, state).update_indirect_neighbour_shapes {
        Some(f) => f(l, state, pos, flags, recursion),
        None => default!(l, state, "updateIndirectNeighbourShapes"),
    }
}

pub fn tick(l: &mut Level, state: State, pos: Pos) {
    match behaviour(l, state).tick {
        Some(f) => f(l, state, pos),
        None => default!(l, state, "tick"),
    }
}

#[inline(always)]
pub fn get_signal(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    if l.blocks.state_needs[state as usize] & NEEDS_GET_SIGNAL == 0 {
        return 0;
    }
    get_signal_of(l, state, pos, dir)
}

#[inline(never)]
fn get_signal_of(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    match behaviour(l, state).get_signal {
        Some(f) => f(l, state, pos, dir),
        None => {
            default!(l, state, "getSignal");
            0
        }
    }
}

#[inline(always)]
pub fn get_direct_signal(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    if l.blocks.state_needs[state as usize] & NEEDS_GET_DIRECT_SIGNAL == 0 {
        return 0;
    }
    get_direct_signal_of(l, state, pos, dir)
}

#[inline(never)]
fn get_direct_signal_of(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    match behaviour(l, state).get_direct_signal {
        Some(f) => f(l, state, pos, dir),
        None => {
            default!(l, state, "getDirectSignal");
            0
        }
    }
}

pub fn get_analog_output_signal(l: &mut Level, state: State, pos: Pos, dir: Dir) -> i32 {
    match behaviour(l, state).get_analog_output_signal {
        Some(f) => f(l, state, pos, dir),
        None => {
            default!(l, state, "getAnalogOutputSignal");
            0
        }
    }
}

pub fn trigger_event(l: &mut Level, state: State, pos: Pos, a: i32, b: i32) -> bool {
    match behaviour(l, state).trigger_event {
        Some(f) => f(l, state, pos, a, b),
        None => {
            default!(l, state, "triggerEvent");
            false
        }
    }
}

pub fn can_survive(l: &mut Level, state: State, pos: Pos) -> bool {
    match behaviour(l, state).can_survive {
        Some(f) => f(l, state, pos),
        None => {
            default!(l, state, "canSurvive");
            true
        }
    }
}

// Block.fallOn: the default hurts a living entity, and an entity that is not
// one (an item) is unaffected.
pub fn fall_on(l: &mut Level, state: State, pos: Pos, entity: &mut Entity, fall_distance: f64) {
    let b = behaviour(l, state);
    match b.fall_on {
        // The hooks model item entities only, unless the class's are for all.
        Some(f) if b.all_entities || matches!(entity.kind, crate::entity::Kind::Item(_)) => f(l, state, pos, entity, fall_distance),
        Some(_) => unported(l, state, "fallOn"),
        None => default!(l, state, "fallOn"),
    }
}

// Block.updateEntityMovementAfterFallOn: the default zeroes the vertical
// motion.
pub fn update_entity_movement_after_fall_on(l: &mut Level, state: State, entity: &mut Entity) {
    let b = behaviour(l, state);
    match b.update_entity_movement_after_fall_on {
        Some(f) if b.all_entities || matches!(entity.kind, crate::entity::Kind::Item(_)) => f(l, entity),
        Some(_) => unported(l, state, "updateEntityMovementAfterFallOn"),
        None => {
            default!(l, state, "updateEntityMovementAfterFallOn");
            entity.delta = entity.delta.multiply(1.0, 0.0, 1.0);
        }
    }
}

// The entityInside of the state's block for an entity, where the port
// models one (for item entities only: the hooks check the kind).
pub fn entity_inside_of(l: &Level, state: State) -> Option<fn(&mut Level, State, Pos, &mut Entity, &mut crate::entity::inside::Applier, bool)> {
    behaviour(l, state).entity_inside
}

// Whether the state's block class overrides entityInside.
pub fn overrides_entity_inside(l: &Level, state: State) -> bool {
    const BIT: u64 = bit("entityInside");
    l.blocks.overrides[l.block(state) as usize] & BIT != 0
}

// Whether the class's entity methods are written for every kind of entity.
pub fn all_entities(l: &Level, state: State) -> bool {
    behaviour(l, state).all_entities
}

// Block.stepOn, where the class's is ported.
pub fn has_step_on(l: &Level, state: State) -> bool {
    let b = behaviour(l, state);
    b.step_on.is_some() || b.step_on_mut.is_some()
}

pub fn step_on(l: &mut Level, pos: Pos, state: State, entity: &mut Entity) {
    let b = behaviour(l, state);
    match (b.step_on_mut, b.step_on) {
        (Some(f), _) => f(l, pos, state, entity),
        (None, Some(f)) => f(l, pos, state, entity),
        (None, None) => panic!("a ported stepOn"),
    }
}

pub fn new_block_entity(l: &mut Level, state: State, pos: Pos) -> Option<BlockEntity> {
    match behaviour(l, state).new_block_entity {
        Some(f) => Some(f(l, state, pos)),
        // MovingPistonBlock.newBlockEntity returns null: the piston that
        // makes the block sets its entity itself.
        None if moving_piston::is_moving_piston(l, state) => None,
        None => refuse(format!("block entity of {}", l.reg.describe(state))),
    }
}

// Whether the state's block entity ticks (getTicker gives a ticker).
pub fn has_ticker(l: &Level, state: State) -> bool {
    match behaviour(l, state).ticker {
        Some(f) => f(l, state).is_some(),
        None => {
            default!(l, state, "getTicker");
            false
        }
    }
}

// The ticker of the block entity at `pos` (BlockEntityTicker.tick).
pub fn tick_block_entity(l: &mut Level, state: State, pos: Pos) {
    let ticker = behaviour(l, state).ticker.and_then(|f| f(l, state)).expect("a ticking block entity without a ticker");
    ticker(l, pos, state)
}

// BlockEntityType.isValid for the block entity at a state.
pub fn block_entity_valid(l: &Level, entity: &BlockEntity, state: State) -> bool {
    match entity {
        BlockEntity::Comparator { .. } => l.block(state) == l.known.comparator,
        BlockEntity::Sign(_) => crate::shape_classes!(l, ["StandingSignBlock", "WallSignBlock"])[l.block(state) as usize],
        BlockEntity::HangingSign(_) => crate::shape_classes!(l, ["CeilingHangingSignBlock", "WallHangingSignBlock"])[l.block(state) as usize],
        BlockEntity::Banner { .. } => crate::shape_classes!(l, ["BannerBlock", "WallBannerBlock"])[l.block(state) as usize],
        BlockEntity::Container(container) => container::valid(l, container, state),
        BlockEntity::Entity(entity) => entities::valid(l, entity, state),
        BlockEntity::Special(entity) => special::valid(l, entity, state),
        BlockEntity::Piston(_) => moving_piston::is_moving_piston(l, state),
        BlockEntity::Plain(entity) => block_entities::valid(l, entity, state),
    }
}

impl BlockEntity {
    // Its type's key and its saved data (BlockEntity.saveWithoutMetadata).
    pub fn saved_tag(&self, reg: &Registry) -> (&'static str, crate::nbt::Compound) {
        use crate::nbt::{Compound, Tag};
        match self {
            BlockEntity::Comparator { output } => {
                let mut out = Compound::new();
                out.put_int("OutputSignal", *output);
                out.put("components", Tag::Compound(Compound::new()));
                ("minecraft:comparator", out)
            }
            BlockEntity::Container(container) => container::saved(reg, container),
            BlockEntity::Sign(sign) => ("minecraft:sign", sign.saved()),
            BlockEntity::HangingSign(sign) => ("minecraft:hanging_sign", sign.saved()),
            BlockEntity::Banner { name, patterns } => {
                let mut out = Compound::new();
                if let Some(name) = name {
                    out.put("CustomName", name.clone());
                }
                if !patterns.is_empty() {
                    out.put("patterns", Tag::List(patterns.clone()));
                }
                out.put("components", Tag::Compound(Compound::new()));
                ("minecraft:banner", out)
            }
            BlockEntity::Entity(entity) => entities::saved(reg, entity),
            BlockEntity::Special(entity) => {
                let (key, data) = special::saved(reg, entity);
                (key, crate::nbt::parse_compound(&data).expect("the block entity's saved data reads"))
            }
            BlockEntity::Plain(entity) => {
                let (key, data) = block_entities::saved(entity);
                (key, crate::nbt::parse_compound(&data).expect("the block entity's saved data reads"))
            }
            BlockEntity::Piston(moving) => {
                let (key, data) = moving_piston::saved(moving, reg);
                (key, crate::nbt::parse_compound(&data).expect("the moving piston's saved data reads"))
            }
        }
    }

    // Its type's key and its saved data as SNBT with keys in order, as
    // McbenchEngine.state writes them.
    pub fn saved(&self, reg: &Registry) -> (&'static str, String) {
        let (key, data) = self.saved_tag(reg);
        (key, data.to_string())
    }

    // BlockEntity.saveWithFullMetadata: the saved data with the type's id and
    // the position.
    pub fn saved_with_metadata(&self, reg: &Registry, pos: Pos) -> String {
        let (key, mut data) = self.saved_tag(reg);
        data.put_string("id", key);
        data.put_int("x", pos.x());
        data.put_int("y", pos.y());
        data.put_int("z", pos.z());
        data.to_string()
    }

    // Its type's key (BlockEntityType.getKey).
    pub fn type_key(&self, reg: &Registry) -> &'static str {
        self.saved_tag(reg).0
    }
}

// BlockEntity.collectComponents for the block entities of this port: the
// components the entity holds, which its implicit ones (collectImplicitComponents)
// set or, for a name it lacks, remove: the custom name and the items of a
// container, a banner's name and pattern layers, the items of a bookshelf.
pub fn collect_components(reg: &Registry, entity: &BlockEntity, own: Option<&crate::nbt::Compound>) -> crate::nbt::Compound {
    use crate::nbt::Tag;
    let mut out = own.cloned().unwrap_or_default();
    let contents = |items: &[container::Stack]| {
        crate::components::container_contents(reg, items).unwrap_or_else(|fail| crate::level::refuse(format!("the items of a container: {}", fail.0)))
    };
    // DataComponentMap.Builder.set: a value, or the removal of the component.
    let set = |out: &mut crate::nbt::Compound, key: &str, value: Option<Tag>| match value {
        Some(value) => out.put(key, value),
        None => {
            out.remove(key);
        }
    };
    match entity {
        BlockEntity::Banner { name, patterns } => {
            out.put("minecraft:banner_patterns", Tag::List(patterns.clone()));
            set(&mut out, "minecraft:custom_name", name.clone());
        }
        BlockEntity::Container(c) if c.is_container() => {
            set(&mut out, "minecraft:custom_name", c.name.clone());
            if let Some(lock) = locked(&c.lock) {
                out.put("minecraft:lock", lock);
            }
            out.put("minecraft:container", contents(&c.items));
        }
        BlockEntity::Entity(entities::Entity::Furnace(_, f)) => {
            set(&mut out, "minecraft:custom_name", f.name.clone());
            if let Some(lock) = locked(&f.lock) {
                out.put("minecraft:lock", lock);
            }
            out.put("minecraft:container", contents(&f.items));
        }
        BlockEntity::Entity(entities::Entity::BrewingStand(b)) => {
            set(&mut out, "minecraft:custom_name", b.name.clone());
            if let Some(lock) = locked(&b.lock) {
                out.put("minecraft:lock", lock);
            }
            out.put("minecraft:container", contents(&b.items));
        }
        BlockEntity::Entity(entities::Entity::ChiseledBookShelf(s)) => out.put("minecraft:container", contents(&s.items)),
        // SkullBlockEntity: the profile (none is modeled), the note block sound and the name.
        BlockEntity::Plain(block_entities::Plain::Skull { note_block_sound, custom_name }) => {
            set(&mut out, "minecraft:profile", None);
            set(&mut out, "minecraft:note_block_sound", note_block_sound.as_ref().map(|sound| Tag::string(sound)));
            set(&mut out, "minecraft:custom_name", custom_name.clone());
        }
        // ShelfBlockEntity: its items.
        BlockEntity::Entity(entities::Entity::Shelf(shelf)) => out.put("minecraft:container", contents(&shelf.items)),
        // DecoratedPotBlockEntity: its decorations and its item.
        BlockEntity::Entity(entities::Entity::DecoratedPot(pot)) => {
            out.put("minecraft:pot_decorations", Tag::List(pot.sherds.iter().map(|sherd| Tag::string(sherd)).collect()));
            out.put("minecraft:container", contents(&pot.items));
        }
        BlockEntity::Special(special::Special::Beacon(beacon)) => {
            set(&mut out, "minecraft:custom_name", beacon.name.clone());
            if let Some(lock) = locked(&beacon.lock) {
                out.put("minecraft:lock", lock);
            }
        }
        BlockEntity::Special(special::Special::EnchantingTable { name }) => set(&mut out, "minecraft:custom_name", name.clone()),
        _ => {}
    }
    out
}

// What a block entity does once it is loaded and stored (a command block made
// automatic schedules its tick).
pub fn loaded(l: &mut Level, pos: Pos) {
    special::command_block::after_load(l, pos);
    special::structure_block::after_load(l, pos);
}

// BaseContainerBlockEntity.isLocked: a lock key that is not LockCode.NO_LOCK
// (the predicate that requires nothing, which a lock read as `{}` equals).
fn locked(lock: &Option<crate::nbt::Tag>) -> Option<crate::nbt::Tag> {
    match lock {
        Some(crate::nbt::Tag::Compound(c)) if c.is_empty() => None,
        other => other.clone(),
    }
}

// BlockEntity.preRemoveSideEffects.
pub fn pre_remove_side_effects(l: &mut Level, entity: BlockEntity, pos: Pos, old: State) {
    match entity {
        BlockEntity::Comparator { .. } | BlockEntity::Sign(_) | BlockEntity::HangingSign(_) | BlockEntity::Banner { .. } => {}
        BlockEntity::Container(container) => container::pre_remove_side_effects(l, &container, pos),
        BlockEntity::Entity(entity) => entities::pre_remove_side_effects(l, &entity, pos, old),
        BlockEntity::Special(entity) => special::pre_remove_side_effects(l, &entity),
        BlockEntity::Piston(moving) => moving_piston::pre_remove_side_effects(l, &moving, pos),
        BlockEntity::Plain(entity) => block_entities::pre_remove_side_effects(l, &entity),
    }
}

// BlockBehaviour.shouldChangedStateKeepBlockEntity of the new state's block,
// with the old state: the copper chests and copper golem statues keep the
// block entity of a state of their own kind (another oxidation or wax).
pub fn keeps_block_entity(l: &Level, state: State, old: State) -> bool {
    match l.reg.blocks[l.block(state) as usize].class.as_str() {
        "CopperChestBlock" | "WeatheringCopperChestBlock" => l.reg.in_tag(old, "minecraft:copper_chests"),
        "CopperGolemStatueBlock" | "WeatheringCopperGolemStatueBlock" => l.reg.in_tag(old, "minecraft:copper_golem_statues"),
        _ => false,
    }
}

// DiodeBlock.isDiode: repeaters and comparators.
pub fn is_diode(l: &Level, state: State) -> bool {
    let block = l.block(state);
    block == l.known.repeater || block == l.known.comparator
}

// RedStoneWireBlock.POWER of a wire state.
pub fn wire_power(l: &Level, state: State) -> i32 {
    l.reg.get_int(state, l.reg.property_of(state, "power"))
}

// BaseRailBlock instances.
pub fn is_rail(l: &Level, state: State) -> bool {
    matches!(l.reg.blocks[l.block(state) as usize].class.as_str(), "RailBlock" | "PoweredRailBlock" | "DetectorRailBlock")
}

// The player of a "use" input: the detached survival player
// McbenchExperiment.newInputPlayer makes, looking with the request's yaw.
// What a block asks of it: where it looks (Entity.getDirection), that it may
// build (abilities.mayBuild), and the entity its game events name.
#[derive(Clone, Copy)]
pub struct InputPlayer {
    pub source: crate::game_event::Source,
    pub yaw: f32,
}

impl InputPlayer {
    // The live world's (McbenchEngine.input): its making is put back, so it
    // has no entity id of the level's.
    pub fn new(yaw: f32) -> InputPlayer {
        InputPlayer { source: crate::game_event::Source::input_player(-1), yaw }
    }

    // A verifier experiment's (McbenchExperiment.inputPlayer), made once: the
    // Entity constructor draws the next entity id and the two longs of an
    // insecure UUID from the shared random (the game profile's uuid replaces
    // it). Its yaw is a Math.random value nothing reads before an input sets
    // it (a fence gate's use requires one).
    pub fn make(l: &mut Level) -> InputPlayer {
        l.entities.counter += 1;
        let id = l.entities.counter;
        l.shared_random.next_long();
        l.shared_random.next_long();
        InputPlayer { source: crate::game_event::Source::input_player(id), yaw: 0.0 }
    }

    // Entity.getDirection: Direction.fromYRot(getYRot()), the horizontal
    // direction the yaw is nearest, from south (0) by west, north and east.
    pub fn direction(&self) -> Dir {
        match crate::phys::floor(self.yaw as f64 / 90.0 + 0.5) & 3 {
            0 => Dir::South,
            1 => Dir::West,
            2 => Dir::North,
            _ => Dir::East,
        }
    }
}

// The blocks a "use" input takes: the classes McbenchExperiment.usable lists
// (WeatheringCopperDoorBlock and WeatheringCopperTrapDoorBlock extend the
// door's and the trapdoor's).
pub fn takes_use(class: &str) -> bool {
    matches!(
        class,
        "DoorBlock"
            | "WeatheringCopperDoorBlock"
            | "TrapDoorBlock"
            | "WeatheringCopperTrapDoorBlock"
            | "FenceGateBlock"
            | "RepeaterBlock"
            | "ComparatorBlock"
            | "NoteBlock"
            | "DaylightDetectorBlock"
            | "RedStoneWireBlock"
            | "DecoratedPotBlock"
    )
}

// BlockState.useWithoutItem by the input player on a block takes_use takes:
// whether the block consumed the action.
pub fn use_without_item(l: &mut Level, state: State, pos: Pos, player: InputPlayer) -> bool {
    let class = l.reg.blocks[l.block(state) as usize].class.as_str();
    assert!(takes_use(class), "{class} takes no use input");
    (behaviour(l, state).use_without_item.expect("a block that takes a use input uses"))(l, state, pos, Some(player))
}

// A player's input (McbenchEngine.input), "toggle", "press" or "use" with the
// player's yaw: whether the block takes it.
pub fn input(l: &mut Level, kind: &str, state: State, pos: Pos, yaw: f32) -> bool {
    let class = l.reg.blocks[l.block(state) as usize].class.as_str();
    let player = match (kind, class) {
        ("toggle", "LeverBlock") => None,
        ("press", "ButtonBlock") => {
            if l.reg.get_bool(state, l.reg.property_of(state, "powered")) {
                return true;
            }
            None
        }
        ("use", class) if takes_use(class) => Some(InputPlayer::new(yaw)),
        _ => return false,
    };
    match behaviour(l, state).use_without_item {
        Some(f) => f(l, state, pos, player),
        None => refuse(format!("{class} input")),
    }
}
