// Minecraft's blocks and block states as the engine dumps them
// (McbenchEngine.dump): every block with its class, the behaviour methods
// its class overrides and its properties; every state in
// Block.BLOCK_STATE_REGISTRY order with the data Minecraft computes for it;
// every block tag.

use std::collections::HashMap;

pub type State = u16;
// The most properties a block has (Registry.state_values).
const MAX_PROPERTIES: usize = 8;
pub type BlockId = u16;

pub const AIR: u32 = 1;
pub const CONDUCTOR: u32 = 2;
pub const SIGNAL_SOURCE: u32 = 4;
pub const ANALOG: u32 = 8;
pub const LIQUID: u32 = 16;
pub const REPLACEABLE: u32 = 32;
pub const BLOCK_ENTITY: u32 = 64;
pub const BLOCKS_MOTION: u32 = 128;
pub const SOLID: u32 = 256;
pub const COLLISION_FULL: u32 = 512;
pub const SOLID_RENDER: u32 = 1024;
pub const OFFSET: u32 = 2048;
pub const LARGE_COLLISION: u32 = 4096;
// No state cache (Block.hasDynamicShape): face sturdiness reads the level.
pub const DYNAMIC_SHAPE: u32 = 8192;
// instanceof LeavesBlock, useShapeForLightOcclusion and canOcclude.
pub const LEAVES: u32 = 16384;
pub const USE_SHAPE_FOR_LIGHT: u32 = 32768;
pub const CAN_OCCLUDE: u32 = 65536;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Push {
    Normal,
    Destroy,
    Block,
    PushOnly,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Support {
    Full = 0,
    Center = 1,
    Rigid = 2,
}

// A property as Minecraft shares it between blocks: its name and values.
pub struct Property {
    pub name: String,
    pub values: Vec<String>,
    // Each value as a number, a direction, or true, where it is one.
    ints: Vec<Option<i32>>,
    dirs: Vec<Option<crate::dir::Dir>>,
    true_index: Option<u16>,
}

impl Property {
    fn new(name: String, values: Vec<String>) -> Property {
        let ints = values.iter().map(|v| v.parse().ok()).collect();
        let dirs = values
            .iter()
            .map(|v| ["down", "up", "north", "south", "west", "east"].contains(&v.as_str()).then(|| crate::dir::Dir::by_name(v)))
            .collect();
        let true_index = values.iter().position(|v| v == "true").map(|i| i as u16);
        Property { name, values, ints, dirs, true_index }
    }
}

// The fluids (Fluids): the empty fluid and the flowing and source kinds of
// water and lava.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Fluid {
    Empty,
    FlowingWater,
    Water,
    FlowingLava,
    Lava,
}

impl Fluid {
    // Fluid by its registry key, if there is one.
    pub fn from_key(key: &str) -> Option<Fluid> {
        Some(match key {
            "minecraft:empty" => Fluid::Empty,
            "minecraft:flowing_water" => Fluid::FlowingWater,
            "minecraft:water" => Fluid::Water,
            "minecraft:flowing_lava" => Fluid::FlowingLava,
            "minecraft:lava" => Fluid::Lava,
            _ => return None,
        })
    }

    fn by_key(key: &str) -> Fluid {
        match key {
            "minecraft:empty" => Fluid::Empty,
            "minecraft:flowing_water" => Fluid::FlowingWater,
            "minecraft:water" => Fluid::Water,
            "minecraft:flowing_lava" => Fluid::FlowingLava,
            "minecraft:lava" => Fluid::Lava,
            other => panic!("unknown fluid {other}"),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Fluid::Empty => "minecraft:empty",
            Fluid::FlowingWater => "minecraft:flowing_water",
            Fluid::Water => "minecraft:water",
            Fluid::FlowingLava => "minecraft:flowing_lava",
            Fluid::Lava => "minecraft:lava",
        }
    }

    // FluidTags.WATER.
    #[inline]
    pub fn is_water(self) -> bool {
        matches!(self, Fluid::FlowingWater | Fluid::Water)
    }

    // FluidTags.LAVA.
    #[inline]
    pub fn is_lava(self) -> bool {
        matches!(self, Fluid::FlowingLava | Fluid::Lava)
    }

    // Fluid.isSame: the fluid itself, or for water and lava either of its
    // two kinds.
    #[inline]
    pub fn is_same(self, other: Fluid) -> bool {
        match self {
            Fluid::Empty => other == Fluid::Empty,
            Fluid::FlowingWater | Fluid::Water => other.is_water(),
            Fluid::FlowingLava | Fluid::Lava => other.is_lava(),
        }
    }
}

// A fluid state, by its number in Fluid.FLUID_STATE_REGISTRY.
pub type FluidId = u8;
pub const EMPTY_FLUID: FluidId = 0;

pub struct FluidData {
    pub fluid: Fluid,
    pub falling: bool,
    // The flowing fluid's level, 0 for a source or the empty fluid.
    pub level: i32,
    pub amount: i32,
    pub source: bool,
    // FluidState.createLegacyBlock.
    pub legacy: State,
}

pub struct Block {
    pub key: String,
    pub class: String,
    pub overrides: Vec<String>,
    pub first: State,
    pub count: u16,
    pub default: State,
    // Block.getFriction, getSpeedFactor, getJumpFactor.
    pub friction: f32,
    pub speed_factor: f32,
    pub jump_factor: f32,
    // Block.getLootTable: the key of its loot table, if it has one.
    pub loot_table: Option<String>,
    // The simple names of its superclasses below Block and of the liquid
    // interfaces it implements (LiquidBlockContainer, SimpleWaterloggedBlock,
    // BucketPickup).
    pub supers: Vec<String>,
    // Block.getExplosionResistance.
    pub explosion_resistance: f32,
    // (property, stride) in the block's property order (sorted by name);
    // the last property varies fastest.
    pub properties: Vec<(u16, u16)>,
}

pub struct StateData {
    pub block: BlockId,
    pub flags: u32,
    pub sturdy: u32,
    pub push: Push,
    pub light_emission: u8,
    pub light_block: u8,
    // BlockBehaviour.instrument(): the note block instrument it gives.
    pub instrument: String,
    // BlockStateBase.getDestroySpeed (-1 for blocks that cannot be broken).
    pub destroy_speed: f32,
    // getCollisionShape at the origin with an empty collision context.
    pub collision_shape: u16,
    // BlockState.getFluidState.
    pub fluid: FluidId,
}

// A collision shape as Minecraft's VoxelShape holds it: its coordinates per
// axis, which voxels between them are full, and moonrise's single-box
// representation where it has one.
#[derive(Clone)]
pub struct Shape {
    pub coords: [Vec<f64>; 3],
    pub size: [usize; 3],
    pub full: Vec<bool>,
    pub single: Option<[f64; 6]>,
    // VoxelShape.moonrise$occludesFullBlock.
    pub occludes_full_block: bool,
    empty: bool,
    // VoxelShape.toAabbs, made when first asked for (crate::clip).
    pub boxes: std::sync::OnceLock<Vec<[f64; 6]>>,
}

impl Shape {
    // A shape made by code (crate::shape_ops, a block's own shape).
    pub fn new(coords: [Vec<f64>; 3], size: [usize; 3], full: Vec<bool>, single: Option<[f64; 6]>, occludes_full_block: bool) -> Shape {
        let empty = !full.iter().any(|&f| f);
        Shape { coords, size, full, single, occludes_full_block, empty, boxes: std::sync::OnceLock::new() }
    }

    // DiscreteVoxelShape.isFull at x, y, z.
    pub fn is_full(&self, x: usize, y: usize, z: usize) -> bool {
        self.full[(x * self.size[1] + y) * self.size[2] + z]
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }
}

pub struct Item {
    pub key: String,
    pub max_stack: i32,
    pub block: Option<BlockId>,
    // DispenserBlock.getDispenseMethod's inputs: whether the item's feature
    // is enabled, the class of the behaviour registered for it, whether it
    // has the equippable component.
    pub dispense_enabled: bool,
    pub dispense_behaviour: Option<String>,
    pub equippable: bool,
    // ComposterBlock.COMPOSTABLES: the chance of the item raising the level.
    pub compost: Option<f32>,
    // The damage type tag the item resists (DataComponents.DAMAGE_RESISTANT).
    pub damage_resistant: Option<String>,
    // The item's default components that are saved (component key, value as
    // SNBT), read when a stack's component patch is compared with them.
    pub components: Vec<(String, String)>,
}

// EntityType's dimensions (EntityType.getDimensions) and whether fire cannot
// hurt it.
#[derive(Clone, Debug)]
pub struct EntityType {
    pub key: String,
    pub width: f32,
    pub height: f32,
    pub eye_height: f32,
    pub fire_immune: bool,
}

// A painting variant of the data pack: its size in blocks.
#[derive(Clone, Debug)]
pub struct PaintingVariant {
    pub key: String,
    pub width: i32,
    pub height: i32,
}

// A crafting recipe as the server holds it: each ingredient as the items it
// accepts, and the result (none if it holds a component this port does not
// model).
pub enum Recipe {
    // The ingredients row by row (none: an empty cell).
    Shaped { id: String, width: usize, height: usize, cells: Vec<Option<Vec<u16>>>, result: Option<crate::loot::ItemStack> },
    Shapeless { id: String, ingredients: Vec<Vec<u16>>, result: Option<crate::loot::ItemStack> },
    // TransmuteRecipe: the input and the material, and the item, count and
    // component changes it gives the copy of its input.
    Transmute { id: String, ingredients: Vec<Vec<u16>>, result: Option<crate::loot::ItemStack> },
    // A recipe class with its own matching (CustomRecipe, DecoratedPotRecipe),
    // and the items it can match: its matches() fails if any item of the
    // input is outside them.
    Other { id: String, class: String, universe: Vec<u16> },
}

impl Recipe {
    pub fn id(&self) -> &str {
        match self {
            Recipe::Shaped { id, .. } | Recipe::Shapeless { id, .. } | Recipe::Transmute { id, .. } | Recipe::Other { id, .. } => id,
        }
    }
}

// A cooking recipe (AbstractCookingRecipe): which furnace type uses it, the
// items its ingredient accepts, its result, experience and cooking time.
pub struct Cooking {
    pub kind: CookingKind,
    pub id: String,
    pub ingredient: Vec<u16>,
    pub result: Option<crate::loot::ItemStack>,
    pub experience: f32,
    pub cooking_time: i32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CookingKind {
    Smelting,
    Blasting,
    Smoking,
    Campfire,
}

// PotionBrewing.Mix: from a potion (key) or an item, with an ingredient, to.
pub struct PotionMix {
    pub from: String,
    pub ingredient: Vec<u16>,
    pub to: String,
}

pub struct ContainerMix {
    pub from: u16,
    pub ingredient: Vec<u16>,
    pub to: u16,
}

// A jukebox song: its length in seconds and comparator output.
pub struct Song {
    pub length_in_seconds: f32,
    pub comparator_output: i32,
}

// A game event: its notification radius, its vibration frequency and the
// tags it is in (GameEvent, VibrationSystem.getGameEventFrequency).
pub struct GameEventData {
    pub key: String,
    pub radius: i32,
    pub frequency: i32,
    pub tags: Vec<String>,
}

// Starlight's occlusion data: the face shape of each conditionally full opaque
// state (a state that occludes and uses its shape for light) per direction,
// as a number among the distinct face shapes (0 is Shapes.empty()), and
// whether two face shapes together occlude (Shapes.faceShapeOccludes).
pub struct Occlusion {
    // By state; zeros for a state that is not conditionally full opaque.
    pub faces: Vec<[u8; 6]>,
    pub shapes: usize,
    occludes: Vec<bool>,
}

impl Occlusion {
    // Shapes.faceShapeOccludes of two face shapes.
    #[inline]
    pub fn occludes(&self, a: u8, b: u8) -> bool {
        self.occludes[a as usize * self.shapes + b as usize]
    }
}

pub struct Registry {
    pub blocks: Vec<Block>,
    pub states: Vec<StateData>,
    // The hot fields of the states, dense: each state's block and flags, and
    // its property values (one byte per property of its block, in the block's
    // property order) with the slot of each (block, property): the stride, the
    // property's place among the block's and its value count, packed
    // (stride | place << 16 | count << 24), 0 where the block lacks it.
    state_block: Vec<BlockId>,
    state_flags: Vec<u32>,
    // light_block | light_emission << 8, and the fluid state, by state.
    state_light: Vec<u16>,
    // What the light engine reads of a state, in one word: getLightBlock
    // (bits 0-3), getLightEmission (4-7), and whether it is conditionally
    // full opaque (bit 8: it uses its shape for light and can occlude).
    state_light_info: Vec<u16>,
    state_fluid: Vec<FluidId>,
    // The index of "true" among each property's values, 0xFFFF where it has none.
    property_true: Vec<u16>,
    state_values: Vec<[u8; MAX_PROPERTIES]>,
    slots: Vec<u32>,
    pub properties: Vec<Property>,
    pub by_key: HashMap<String, BlockId>,
    property_ids: HashMap<(String, Vec<String>), u16>,
    // tag name -> member flags by block id
    pub tags: HashMap<String, Vec<bool>>,
    pub shapes: Vec<Shape>,
    pub items: Vec<Item>,
    pub item_by_key: HashMap<String, u16>,
    pub game_events: Vec<GameEventData>,
    pub game_event_by_key: HashMap<String, u16>,
    // FireBlock.igniteOdds and burnOdds by block.
    pub fire_odds: HashMap<BlockId, (i32, i32)>,
    // BeaconBeamBlock.getColor().getTextureDiffuseColor() by block.
    pub beam_colors: HashMap<BlockId, i32>,
    // BasePressurePlateBlock: whether the plate senses only mobs
    // (PressurePlateSensitivity.MOBS), and a weighted plate's max weight.
    pub plates: HashMap<BlockId, (bool, Option<i32>)>,
    // CommandBlock.automatic by block.
    pub automatic: HashMap<BlockId, bool>,
    // Item tag name -> member flags by item id.
    pub item_tags: HashMap<String, Vec<bool>>,
    pub occlusion: Occlusion,
    pub recipes: Vec<Recipe>,
    // The cooking recipes in the recipe manager's order (by id).
    pub cooking: Vec<Cooking>,
    // FuelValues: the burn time in ticks by item.
    pub fuel: HashMap<u16, i32>,
    // PotionBrewing: the containers, the potion mixes and the container mixes.
    pub brew_containers: Vec<Vec<u16>>,
    pub potion_mixes: Vec<PotionMix>,
    pub container_mixes: Vec<ContainerMix>,
    // The jukebox songs by key.
    pub songs: HashMap<String, Song>,
    // Item.getCraftingRemainder: the item and count an item leaves.
    pub remainders: HashMap<u16, (u16, i32)>,
    // The keys of the potions and mob effects the registries hold.
    pub potions: std::collections::HashSet<String>,
    // HoneycombItem.WAXABLES: the block each block waxes to.
    pub waxed: HashMap<BlockId, BlockId>,
    // The enchantments of the tag #minecraft:curse.
    pub enchantment_curses: Vec<String>,
    pub mob_effects: std::collections::HashSet<String>,
    pub banner_patterns: std::collections::HashSet<String>,
    // The keys of the data component types (DataComponentType.CODEC's registry).
    pub component_types: std::collections::HashSet<String>,
    pub fluids: Vec<FluidData>,
    // The fluid state of each (fluid, falling, level), 255 where there is none.
    fluid_table: Vec<u8>,
    // Shapes.mergedFaceOccludes(first, second, direction), at
    // (direction * shapes + first) * shapes + second.
    occludes: Vec<bool>,
    // Block.asItem (Item.BY_BLOCK): the item registered last for a block.
    pub item_of_block: HashMap<BlockId, u16>,
    // Dumped sets of blocks (McbenchEngine.dump's flag lines), by name: the
    // blocks whose class overrides a method with a signature the behaviour
    // list does not tell apart.
    pub block_flags: HashMap<String, Vec<bool>>,
    // The keys of the enchantments the data pack holds.
    pub enchantments: std::collections::HashSet<String>,
    // The entity types, by key, and the painting variants in registry order
    // (the first is the one a painting has before its variant is read).
    pub entity_types: HashMap<String, EntityType>,
    pub paintings: Vec<PaintingVariant>,
    // Mth.ASIN_TAB and Mth.COS_TAB.
    pub asin_tab: Vec<f64>,
    pub cos_tab: Vec<f64>,
}

impl Registry {
    // An item's default component, as the tag its codec writes, if it has one.
    pub fn item_component(&self, item: u16, key: &str) -> Option<crate::nbt::Tag> {
        let (_, text) = self.items[item as usize].components.iter().find(|(k, _)| k == key)?;
        match crate::nbt::parse(text) {
            Ok(tag) => Some(tag),
            Err(error) => panic!("the default component {key} of {} does not read: {error:?}", self.items[item as usize].key),
        }
    }

    pub fn parse(text: &str) -> Registry {
        let mut registry = Registry {
            blocks: Vec::new(),
            states: Vec::new(),
            state_block: Vec::new(),
            state_flags: Vec::new(),
            state_light: Vec::new(),
            state_light_info: Vec::new(),
            state_fluid: Vec::new(),
            property_true: Vec::new(),
            state_values: Vec::new(),
            slots: Vec::new(),
            properties: Vec::new(),
            by_key: HashMap::new(),
            property_ids: HashMap::new(),
            tags: HashMap::new(),
            shapes: Vec::new(),
            items: Vec::new(),
            item_by_key: HashMap::new(),
            game_events: Vec::new(),
            game_event_by_key: HashMap::new(),
            fire_odds: HashMap::new(),
            beam_colors: HashMap::new(),
            plates: HashMap::new(),
            automatic: HashMap::new(),
            item_tags: HashMap::new(),
            recipes: Vec::new(),
            cooking: Vec::new(),
            fuel: HashMap::new(),
            brew_containers: Vec::new(),
            potion_mixes: Vec::new(),
            container_mixes: Vec::new(),
            songs: HashMap::new(),
            remainders: HashMap::new(),
            potions: std::collections::HashSet::new(),
            waxed: HashMap::new(),
            enchantment_curses: Vec::new(),
            mob_effects: std::collections::HashSet::new(),
            banner_patterns: std::collections::HashSet::new(),
            component_types: std::collections::HashSet::new(),
            fluids: Vec::new(),
            fluid_table: vec![255; 5 * 2 * 9],
            occludes: Vec::new(),
            item_of_block: HashMap::new(),
            block_flags: HashMap::new(),
            enchantments: std::collections::HashSet::new(),
            entity_types: HashMap::new(),
            paintings: Vec::new(),
            asin_tab: Vec::new(),
            cos_tab: Vec::new(),
            occlusion: Occlusion { faces: Vec::new(), shapes: 0, occludes: Vec::new() },
        };
        let mut result_texts: Vec<&str> = Vec::new();
        let mut cooking_texts: Vec<&str> = Vec::new();
        let mut occludes_rows: Vec<(usize, usize, &str)> = Vec::new();
        let mut conditional: Vec<(usize, [u8; 6])> = Vec::new();
        let mut values_by_block: Vec<Vec<u16>> = Vec::new();
        for line in text.lines() {
            let p: Vec<&str> = line.split('\t').collect();
            match p[0] {
                "block" => {
                    let id = registry.blocks.len() as BlockId;
                    registry.by_key.insert(p[1].to_string(), id);
                    registry.blocks.push(Block {
                        key: p[1].to_string(),
                        class: p[2].rsplit('.').next().unwrap().to_string(),
                        overrides: if p[3] == "-" { Vec::new() } else { p[3].split(',').map(str::to_string).collect() },
                        first: p[4].parse().unwrap(),
                        count: p[5].parse().unwrap(),
                        default: p[6].parse().unwrap(),
                        friction: p[7].parse().unwrap(),
                        speed_factor: p[8].parse().unwrap(),
                        jump_factor: p[9].parse().unwrap(),
                        loot_table: (p[10] != "-").then(|| p[10].to_string()),
                        supers: if p[11] == "-" { Vec::new() } else { p[11].split(',').map(str::to_string).collect() },
                        explosion_resistance: p[12].parse().unwrap(),
                        properties: Vec::new(),
                    });
                }
                "property" => {
                    let key = (p[1].to_string(), p[2].split(',').map(str::to_string).collect::<Vec<_>>());
                    let next = registry.properties.len() as u16;
                    let id = *registry.property_ids.entry(key.clone()).or_insert(next);
                    if id == next {
                        registry.properties.push(Property::new(key.0, key.1));
                    }
                    registry.blocks.last_mut().unwrap().properties.push((id, 0));
                }
                "state" => {
                    let id: usize = p[1].parse().unwrap();
                    assert_eq!(id, registry.states.len(), "states out of order");
                    let block = (registry.blocks.len() - 1) as BlockId;
                    values_by_block.resize(id + 1, Vec::new());
                    values_by_block[id] = if p[2] == "-" { Vec::new() } else { p[2].split(',').map(|v| v.parse().unwrap()).collect() };
                    registry.states.push(StateData {
                        block,
                        flags: p[3].parse().unwrap(),
                        sturdy: p[4].parse().unwrap(),
                        push: match p[5] {
                            "NORMAL" => Push::Normal,
                            "DESTROY" => Push::Destroy,
                            "BLOCK" => Push::Block,
                            "PUSH_ONLY" => Push::PushOnly,
                            other => panic!("unknown push reaction {other}"),
                        },
                        light_emission: p[6].parse().unwrap(),
                        light_block: p[7].parse().unwrap(),
                        instrument: p[8].to_string(),
                        destroy_speed: p[9].parse().unwrap(),
                        collision_shape: p[10].parse().unwrap(),
                        fluid: p[11].parse().unwrap(),
                    });
                }
                "tag" => {
                    let mut members = vec![false; registry.blocks.len()];
                    if p[2] != "-" {
                        for key in p[2].split(',') {
                            members[registry.by_key[key] as usize] = true;
                        }
                    }
                    registry.tags.insert(p[1].to_string(), members);
                }
                "shape" => {
                    let coords = [1, 2, 3].map(|i| p[i].split(',').map(|v| v.parse().unwrap()).collect::<Vec<f64>>());
                    let size: Vec<usize> = p[4].split(',').map(|v| v.parse().unwrap()).collect();
                    let single = (p[6] != "-").then(|| {
                        let v: Vec<f64> = p[6].split(',').map(|v| v.parse().unwrap()).collect();
                        [v[0], v[1], v[2], v[3], v[4], v[5]]
                    });
                    let full: Vec<bool> = p[5].bytes().map(|b| b == b'1').collect();
                    let empty = !full.iter().any(|&f| f);
                    registry.shapes.push(Shape {
                        coords,
                        size: [size[0], size[1], size[2]],
                        full,
                        single,
                        occludes_full_block: p[7] == "1",
                        empty,
                        boxes: std::sync::OnceLock::new(),
                    });
                }
                "occlusion" => {
                    let mut faces = [0u8; 6];
                    for (face, value) in faces.iter_mut().zip(&p[2..8]) {
                        *face = value.parse().unwrap();
                    }
                    conditional.push((p[1].parse().unwrap(), faces));
                }
                "faceoccludes" => {
                    registry.occlusion.shapes = p[1].parse().unwrap();
                    registry.occlusion.occludes = p[2].bytes().map(|b| b == b'1').collect();
                    assert_eq!(registry.occlusion.occludes.len(), registry.occlusion.shapes * registry.occlusion.shapes);
                }
                "item" => {
                    let id = registry.items.len() as u16;
                    registry.item_by_key.insert(p[1].to_string(), id);
                    let block = (p[3] != "-").then(|| registry.by_key[p[3]]);
                    if let Some(block) = block {
                        registry.item_of_block.insert(block, id);
                    }
                    registry.items.push(Item {
                        key: p[1].to_string(),
                        max_stack: p[2].parse().unwrap(),
                        block,
                        dispense_enabled: true,
                        dispense_behaviour: None,
                        equippable: false,
                        compost: None,
                        damage_resistant: (p[4] != "-").then(|| p[4].to_string()),
                        components: Vec::new(),
                    });
                }
                "gameevent" => {
                    registry.game_event_by_key.insert(p[1].to_string(), registry.game_events.len() as u16);
                    registry.game_events.push(GameEventData {
                        key: p[1].to_string(),
                        radius: p[2].parse().unwrap(),
                        frequency: p[3].parse().unwrap(),
                        tags: if p[4] == "-" { Vec::new() } else { p[4].split(',').map(str::to_string).collect() },
                    });
                }
                "fire" => {
                    registry.fire_odds.insert(registry.by_key[p[1]], (p[2].parse().unwrap(), p[3].parse().unwrap()));
                }
                "beam" => {
                    registry.beam_colors.insert(registry.by_key[p[1]], p[2].parse().unwrap());
                }
                "plate" => {
                    registry.plates.insert(registry.by_key[p[1]], (p[2] == "MOBS", (p[3] != "-").then(|| p[3].parse().unwrap())));
                }
                "automatic" => {
                    registry.automatic.insert(registry.by_key[p[1]], p[2] == "true");
                }
                "itemtag" => {
                    let mut members = vec![false; registry.items.len()];
                    if p[2] != "-" {
                        for key in p[2].split(',') {
                            members[registry.item_by_key[key] as usize] = true;
                        }
                    }
                    registry.item_tags.insert(p[1].to_string(), members);
                }
                "itemcomponent" => {
                    let item = registry.item_by_key[p[1]];
                    registry.items[item as usize].components.push((p[2].to_string(), p[3].to_string()));
                }
                "entries" => {
                    let keys = || p[2].split(',').map(str::to_string).collect();
                    match p[1] {
                        "minecraft:enchantment" => registry.enchantments = keys(),
                        "minecraft:potion" => registry.potions = keys(),
                        "minecraft:mob_effect" => registry.mob_effects = keys(),
                        "minecraft:banner_pattern" => registry.banner_patterns = keys(),
                        "minecraft:data_component_type" => registry.component_types = keys(),
                        "#minecraft:enchantment/curse" => registry.enchantment_curses = p[2].split(',').map(str::to_string).collect(),
                        other => panic!("unknown registry entries {other}"),
                    }
                }
                "entitytype" => {
                    registry.entity_types.insert(
                        p[1].to_string(),
                        EntityType { key: p[1].to_string(), width: p[2].parse().unwrap(), height: p[3].parse().unwrap(), eye_height: p[4].parse().unwrap(), fire_immune: p[5] == "1" },
                    );
                }
                "mthtab" => {
                    let values: Vec<f64> = p[2].split(',').map(|bits| f64::from_bits(u64::from_str_radix(bits, 16).unwrap())).collect();
                    assert_eq!(values.len(), 257, "a Mth table of 257 entries");
                    match p[1] {
                        "ASIN_TAB" => registry.asin_tab = values,
                        "COS_TAB" => registry.cos_tab = values,
                        other => panic!("unknown Mth table {other}"),
                    }
                }
                "painting" => {
                    registry.paintings.push(PaintingVariant { key: p[1].to_string(), width: p[2].parse().unwrap(), height: p[3].parse().unwrap() });
                }
                "flag" => {
                    let mut members = vec![false; registry.blocks.len()];
                    if p[2] != "-" {
                        for key in p[2].split(',') {
                            members[registry.by_key[key] as usize] = true;
                        }
                    }
                    registry.block_flags.insert(p[1].to_string(), members);
                }
                "dispense" => {
                    let item = &mut registry.items[registry.item_by_key[p[1]] as usize];
                    item.dispense_enabled = p[2] == "1";
                    item.dispense_behaviour = (p[3] != "-").then(|| p[3].to_string());
                    item.equippable = p[4] == "1";
                }
                "compost" => {
                    let item = &mut registry.items[registry.item_by_key[p[1]] as usize];
                    item.compost = Some(p[2].parse().unwrap());
                }
                "recipe" => {
                    let items = |text: &str| -> Vec<u16> { text.split(',').map(|key| registry.item_by_key[key]).collect() };
                    let recipe = match p[1] {
                        "shaped" => Recipe::Shaped {
                            id: p[2].to_string(),
                            width: p[3].parse().unwrap(),
                            height: p[4].parse().unwrap(),
                            cells: p[5].split(';').map(|cell| (cell != "-").then(|| items(cell))).collect(),
                            result: None,
                        },
                        "shapeless" => Recipe::Shapeless { id: p[2].to_string(), ingredients: p[3].split(';').map(items).collect(), result: None },
                        "transmute" => Recipe::Transmute { id: p[2].to_string(), ingredients: p[3].split(';').map(items).collect(), result: None },
                        "other" => Recipe::Other { id: p[2].to_string(), class: p[3].rsplit('.').next().unwrap().to_string(), universe: items(p[4]) },
                        other => panic!("unknown recipe kind {other}"),
                    };
                    result_texts.push(p.get(match p[1] { "shaped" => 6, "shapeless" | "transmute" => 4, _ => usize::MAX }).copied().unwrap_or(""));
                    registry.recipes.push(recipe);
                }
                "cooking" => {
                    let kind = match p[1] {
                        "minecraft:smelting" => CookingKind::Smelting,
                        "minecraft:blasting" => CookingKind::Blasting,
                        "minecraft:smoking" => CookingKind::Smoking,
                        "minecraft:campfire_cooking" => CookingKind::Campfire,
                        other => panic!("unknown cooking recipe type {other}"),
                    };
                    let ingredient = p[3].split(',').map(|key| registry.item_by_key[key]).collect();
                    cooking_texts.push(p[4]);
                    registry.cooking.push(Cooking { kind, id: p[2].to_string(), ingredient, result: None, experience: p[5].parse().unwrap(), cooking_time: p[6].parse().unwrap() });
                }
                "fuel" => {
                    registry.fuel.insert(registry.item_by_key[p[1]], p[2].parse().unwrap());
                }
                "brewcontainer" => {
                    let items = p[1].split(',').map(|key| registry.item_by_key[key]).collect();
                    registry.brew_containers.push(items);
                }
                "potionmix" => {
                    let ingredient = p[2].split(',').map(|key| registry.item_by_key[key]).collect();
                    registry.potion_mixes.push(PotionMix { from: p[1].to_string(), ingredient, to: p[3].to_string() });
                }
                "containermix" => {
                    let ingredient = p[2].split(',').map(|key| registry.item_by_key[key]).collect();
                    registry.container_mixes.push(ContainerMix { from: registry.item_by_key[p[1]], ingredient, to: registry.item_by_key[p[3]] });
                }
                "waxed" => {
                    registry.waxed.insert(registry.by_key[p[1]], registry.by_key[p[2]]);
                }
                "jukebox" => {
                    registry.songs.insert(p[1].to_string(), Song { length_in_seconds: p[2].parse().unwrap(), comparator_output: p[3].parse().unwrap() });
                }
                "remainder" => {
                    registry.remainders.insert(registry.item_by_key[p[1]], (registry.item_by_key[p[2]], p[3].parse().unwrap()));
                }
                "fluid" => {
                    let id: usize = p[1].parse().unwrap();
                    assert_eq!(id, registry.fluids.len(), "fluid states out of order");
                    registry.fluids.push(FluidData {
                        fluid: Fluid::by_key(p[2]),
                        falling: p[3] == "1",
                        level: if p[4] == "-" { 0 } else { p[4].parse().unwrap() },
                        amount: p[5].parse().unwrap(),
                        source: p[6] == "1",
                        legacy: p[7].parse().unwrap(),
                    });
                }
                "occludes" => occludes_rows.push((p[1].parse().unwrap(), p[2].parse().unwrap(), p[3])),
                other => panic!("unknown registry line {other}"),
            }
        }
        // The recipes' results, read now that the items and their default
        // components are known; a result with a component this port does not
        // model is none.
        let decode = |text: &str| -> Option<crate::loot::ItemStack> {
            if text.is_empty() {
                return None;
            }
            let tag = crate::nbt::parse(text).unwrap_or_else(|e| panic!("a recipe result {text} does not read: {e:?}"));
            crate::components::decode_stack(&registry, &tag).ok()
        };
        let decoded: Vec<Option<crate::loot::ItemStack>> = result_texts.iter().map(|text| decode(text)).collect();
        let decoded_cooking: Vec<Option<crate::loot::ItemStack>> = cooking_texts.iter().map(|text| decode(text)).collect();
        for (recipe, stack) in registry.recipes.iter_mut().zip(decoded) {
            match recipe {
                Recipe::Shaped { result, .. } | Recipe::Shapeless { result, .. } | Recipe::Transmute { result, .. } => *result = stack,
                Recipe::Other { .. } => {}
            }
        }
        for (recipe, stack) in registry.cooking.iter_mut().zip(decoded_cooking) {
            recipe.result = stack;
        }
        registry.occlusion.faces = vec![[0; 6]; registry.states.len()];
        for (state, faces) in conditional {
            registry.occlusion.faces[state] = faces;
        }
        for (id, f) in registry.fluids.iter().enumerate() {
            registry.fluid_table[(f.fluid as usize * 2 + f.falling as usize) * 9 + f.level as usize] = id as u8;
        }
        let n = registry.shapes.len();
        registry.occludes = vec![false; 6 * n * n];
        for (direction, first, bits) in occludes_rows {
            for (at, digit) in bits.bytes().enumerate() {
                let digit = (digit as char).to_digit(16).unwrap();
                for bit in 0..4 {
                    let second = at * 4 + bit;
                    if second < n {
                        registry.occludes[(direction * n + first) * n + second] = digit >> (3 - bit) & 1 == 1;
                    }
                }
            }
        }
        // Strides: the last property varies fastest. Checked against every
        // state's dumped value indices.
        for block in &mut registry.blocks {
            let mut stride = 1u16;
            for entry in block.properties.iter_mut().rev() {
                entry.1 = stride;
                stride *= registry.properties[entry.0 as usize].values.len() as u16;
            }
            assert_eq!(stride, block.count, "{} state count", block.key);
        }
        for (id, values) in values_by_block.iter().enumerate() {
            let data = &registry.states[id];
            let block = &registry.blocks[data.block as usize];
            let mut offset = 0u16;
            for (i, &(_, stride)) in block.properties.iter().enumerate() {
                offset += values[i] * stride;
            }
            assert_eq!(block.first as usize + offset as usize, id, "{} state order", block.key);
        }
        registry.state_block = registry.states.iter().map(|data| data.block).collect();
        registry.state_flags = registry.states.iter().map(|data| data.flags).collect();
        registry.state_light = registry.states.iter().map(|data| data.light_block as u16 | (data.light_emission as u16) << 8).collect();
        registry.state_light_info = registry.states.iter().map(|data| {
            assert!(data.light_block < 16 && data.light_emission < 16, "a light level past 15");
            let conditional = data.flags & (USE_SHAPE_FOR_LIGHT | CAN_OCCLUDE) == (USE_SHAPE_FOR_LIGHT | CAN_OCCLUDE);
            data.light_block as u16 | (data.light_emission as u16) << 4 | (conditional as u16) << 8
        }).collect();
        registry.state_fluid = registry.states.iter().map(|data| data.fluid).collect();
        registry.property_true = registry.properties.iter().map(|property| property.true_index.unwrap_or(0xFFFF)).collect();
        let n_properties = registry.properties.len();
        registry.slots = vec![0; registry.blocks.len() * n_properties];
        for (b, block) in registry.blocks.iter().enumerate() {
            assert!(block.properties.len() <= MAX_PROPERTIES, "{} has more than {MAX_PROPERTIES} properties", block.key);
            for (place, &(id, stride)) in block.properties.iter().enumerate() {
                let count = registry.properties[id as usize].values.len();
                assert!(count < 256, "{} has a property of {count} values", block.key);
                registry.slots[b * n_properties + id as usize] = stride as u32 | (place as u32) << 16 | (count as u32) << 24;
            }
        }
        registry.state_values = (0..registry.states.len())
            .map(|id| {
                let block = &registry.blocks[registry.state_block[id] as usize];
                let offset = id as u16 - block.first;
                let mut values = [0u8; MAX_PROPERTIES];
                for (place, &(property, stride)) in block.properties.iter().enumerate() {
                    values[place] = ((offset / stride) % registry.properties[property as usize].values.len() as u16) as u8;
                }
                values
            })
            .collect();
        registry
    }

    // Shapes.mergedFaceOccludes of two of the dump's collision shapes.
    #[inline]
    pub fn merged_face_occludes(&self, first: u16, second: u16, direction: crate::dir::Dir) -> bool {
        let n = self.shapes.len();
        self.occludes[(direction as usize * n + first as usize) * n + second as usize]
    }

    // The fluid state of the fluid with these properties: a flowing fluid by
    // level 1 to 8 (FlowingFluid.getFlowing), a source or the empty fluid at
    // level 0 (FlowingFluid.getSource).
    pub fn fluid_state(&self, fluid: Fluid, falling: bool, level: i32) -> FluidId {
        let id = self.fluid_table[(fluid as usize * 2 + falling as usize) * 9 + level as usize];
        assert!(id != 255, "no fluid state {fluid:?} falling {falling} level {level}");
        id
    }

    pub fn block_key(&self, key: &str) -> BlockId {
        *self.by_key.get(key).unwrap_or_else(|| panic!("no block {key}"))
    }

    pub fn property(&self, name: &str, values: &[&str]) -> u16 {
        let key = (name.to_string(), values.iter().map(|v| v.to_string()).collect::<Vec<_>>());
        *self.property_ids.get(&key).unwrap_or_else(|| panic!("no property {name} {values:?}"))
    }

    #[inline]
    pub fn block(&self, state: State) -> BlockId {
        self.state_block[state as usize]
    }

    #[inline]
    pub fn flags(&self, state: State) -> u32 {
        self.state_flags[state as usize]
    }

    // BlockStateBase.getLightBlock.
    #[inline]
    pub fn light_block(&self, state: State) -> u8 {
        self.state_light[state as usize] as u8
    }

    // BlockStateBase.getLightEmission.
    #[inline]
    pub fn light_emission(&self, state: State) -> u8 {
        (self.state_light[state as usize] >> 8) as u8
    }

    // The light engine's word of a state (state_light_info).
    #[inline]
    pub fn light_info(&self, state: State) -> u16 {
        self.state_light_info[state as usize]
    }

    // Both, as one word that differs where either does.
    #[inline]
    pub fn light_properties(&self, state: State) -> u16 {
        self.state_light[state as usize]
    }

    // BlockState.getFluidState.
    #[inline]
    pub fn fluid(&self, state: State) -> FluidId {
        self.state_fluid[state as usize]
    }

    #[inline]
    pub fn is_air(&self, state: State) -> bool {
        self.flags(state) & AIR != 0
    }

    #[inline]
    // BlockStateBase.isFaceSturdy. A block with a dynamic shape answers from
    // the level (its block entity, its position), which the dump cannot
    // hold: those refuse (crate::shape_ops::is_face_sturdy reads them), but
    // for shulker boxes, whose shape is the dumped full block while the lid is
    // closed (shulker_box.rs refuses where a lid would open), and scaffolding
    // and powder snow, whose support shape (getCollisionShape for no entity)
    // is the dump's.
    pub fn sturdy(&self, state: State, direction: crate::dir::Dir, support: Support) -> bool {
        let data = &self.states[state as usize];
        if data.flags & DYNAMIC_SHAPE != 0 && !matches!(self.blocks[data.block as usize].class.as_str(), "ShulkerBoxBlock" | "ScaffoldingBlock" | "PowderSnowBlock") {
            crate::level::refuse(format!("face sturdiness of {}, a dynamic shape", self.describe(state)));
        }
        data.sturdy & (1 << (direction as u32 * 3 + support as u32)) != 0
    }

    pub fn default_state(&self, block: BlockId) -> State {
        self.blocks[block as usize].default
    }

    // The property's slot in the block's state (stride | place << 16 | count
    // << 24), 0 if the block lacks it.
    #[inline(always)]
    fn slot(&self, state: State, property: u16) -> u32 {
        self.slots[self.block(state) as usize * self.properties.len() + property as usize]
    }

    #[cold]
    #[inline(never)]
    fn missing(&self, state: State, property: u16) -> ! {
        panic!("{} has no {}", self.describe(state), self.properties[property as usize].name)
    }

    pub fn has(&self, state: State, property: u16) -> bool {
        self.slot(state, property) != 0
    }

    // The index of the state's value of `property`.
    #[inline]
    pub fn get(&self, state: State, property: u16) -> u16 {
        let slot = self.slot(state, property);
        if slot == 0 {
            self.missing(state, property);
        }
        self.state_values[state as usize][(slot >> 16 & 0xff) as usize] as u16
    }

    #[inline]
    pub fn with(&self, state: State, property: u16, value: u16) -> State {
        let slot = self.slot(state, property);
        if slot == 0 {
            self.missing(state, property);
        }
        assert!(value < (slot >> 24) as u16);
        let current = self.state_values[state as usize][(slot >> 16 & 0xff) as usize] as u16;
        let stride = (slot & 0xffff) as u16;
        state - current * stride + value * stride
    }

    // Whether the state's block is in a dumped set (flag line).
    pub fn flagged(&self, state: State, flag: &str) -> bool {
        self.block_flags.get(flag).unwrap_or_else(|| panic!("no flag {flag}"))[self.block(state) as usize]
    }

    pub fn in_tag(&self, state: State, tag: &str) -> bool {
        self.tags.get(tag).unwrap_or_else(|| panic!("no tag {tag}"))[self.block(state) as usize]
    }

    // The property with this name on the state's block.
    pub fn property_of(&self, state: State, name: &str) -> u16 {
        let block = &self.blocks[self.block(state) as usize];
        block
            .properties
            .iter()
            .find(|&&(id, _)| self.properties[id as usize].name == name)
            .unwrap_or_else(|| panic!("{} has no property {name}", block.key))
            .0
    }

    pub fn value_name(&self, state: State, property: u16) -> &str {
        &self.properties[property as usize].values[self.get(state, property) as usize]
    }

    pub fn with_name(&self, state: State, property: u16, value: &str) -> State {
        let index = self.properties[property as usize].values.iter().position(|v| v == value).unwrap_or_else(|| panic!("{} has no value {value}", self.properties[property as usize].name));
        self.with(state, property, index as u16)
    }

    // Property.getValue(name): the value an enum or boolean property names
    // exactly, or the one an integer property's Integer.parseInt of the name
    // is (a sign, then decimal digits; Java's digits beyond ASCII are refused).
    pub fn parse_value(&self, property: u16, name: &str) -> Option<u16> {
        let property = &self.properties[property as usize];
        if property.ints.iter().all(Option::is_some) {
            let digits = name.strip_prefix(['+', '-']).unwrap_or(name);
            if digits.is_empty() {
                return None;
            }
            if !digits.is_ascii() {
                crate::level::refuse(format!("an integer property value {name:?} of digits beyond ASCII (Integer.parseInt)"));
            }
            let value: i32 = name.parse().ok()?;
            return property.ints.iter().position(|&v| v == Some(value)).map(|i| i as u16);
        }
        property.values.iter().position(|v| v == name).map(|i| i as u16)
    }

    // BlockItemStateProperties.apply: each property the block has, set to the
    // value the component names where it is one.
    pub fn apply_block_state(&self, mut state: State, properties: &crate::nbt::Compound) -> State {
        let block = &self.blocks[self.block(state) as usize];
        for (name, value) in &properties.0 {
            let Some(&(property, _)) = block.properties.iter().find(|&&(id, _)| self.properties[id as usize].name == *name) else { continue };
            let value = value.as_str().expect("a block state component's value is a string");
            if let Some(index) = self.parse_value(property, value) {
                state = self.with(state, property, index);
            }
        }
        state
    }

    // BlockState.cycle: the property's next value, from the last around to the first.
    pub fn cycle(&self, state: State, property: u16) -> State {
        let next = (self.get(state, property) as usize + 1) % self.properties[property as usize].values.len();
        self.with(state, property, next as u16)
    }

    #[inline]
    pub fn get_bool(&self, state: State, property: u16) -> bool {
        self.get(state, property) == self.property_true[property as usize]
    }

    #[inline]
    pub fn with_bool(&self, state: State, property: u16, value: bool) -> State {
        let true_index = self.properties[property as usize].true_index.expect("not a boolean property");
        self.with(state, property, if value { true_index } else { 1 - true_index })
    }

    #[inline]
    pub fn get_int(&self, state: State, property: u16) -> i32 {
        self.properties[property as usize].ints[self.get(state, property) as usize].expect("not a number")
    }

    #[inline]
    pub fn with_int(&self, state: State, property: u16, value: i32) -> State {
        let index = self.properties[property as usize].ints.iter().position(|&v| v == Some(value)).unwrap_or_else(|| panic!("{} has no value {value}", self.properties[property as usize].name));
        self.with(state, property, index as u16)
    }

    #[inline]
    pub fn get_dir(&self, state: State, property: u16) -> crate::dir::Dir {
        self.properties[property as usize].dirs[self.get(state, property) as usize].expect("not a direction")
    }

    #[inline]
    pub fn with_dir(&self, state: State, property: u16, value: crate::dir::Dir) -> State {
        let index = self.properties[property as usize].dirs.iter().position(|&v| v == Some(value)).unwrap_or_else(|| panic!("{} has no value {value:?}", self.properties[property as usize].name));
        self.with(state, property, index as u16)
    }

    pub fn instrument(&self, state: State) -> &str {
        &self.states[state as usize].instrument
    }

    // id[name=value,...] as Minecraft writes block states.
    pub fn describe(&self, state: State) -> String {
        let block = &self.blocks[self.block(state) as usize];
        if block.properties.is_empty() {
            return block.key.clone();
        }
        let offset = state - block.first;
        let values: Vec<String> = block
            .properties
            .iter()
            .map(|&(id, stride)| {
                let property = &self.properties[id as usize];
                format!("{}={}", property.name, property.values[((offset / stride) % property.values.len() as u16) as usize])
            })
            .collect();
        format!("{}[{}]", block.key, values.join(","))
    }

    // (name, value) pairs in property order, as blueprints write them.
    pub fn values(&self, state: State) -> Vec<(&str, &str)> {
        let block = &self.blocks[self.block(state) as usize];
        let offset = state - block.first;
        block
            .properties
            .iter()
            .map(|&(id, stride)| {
                let property = &self.properties[id as usize];
                (property.name.as_str(), property.values[((offset / stride) % property.values.len() as u16) as usize].as_str())
            })
            .collect()
    }

    // A command's block argument, as BlockStateParser.parseForBlock reads it
    // without NBT: the block's default state with the named properties set,
    // and which properties were named.
    pub fn parse_state(&self, text: &str) -> (State, Vec<u16>) {
        let (name, rest) = match text.find('[') {
            Some(at) => (&text[..at], Some(&text[at + 1..text.len() - 1])),
            None => (text, None),
        };
        assert!(rest.is_none() || text.ends_with(']'), "bad block state {text}");
        let key = if name.contains(':') { name.to_string() } else { format!("minecraft:{name}") };
        let block = self.block_key(&key);
        let mut state = self.blocks[block as usize].default;
        let mut named = Vec::new();
        if let Some(rest) = rest {
            for pair in rest.split(',').filter(|pair| !pair.is_empty()) {
                let (name, value) = pair.split_once('=').unwrap_or_else(|| panic!("bad property {pair} in {text}"));
                let &(id, _) = self.blocks[block as usize]
                    .properties
                    .iter()
                    .find(|&&(id, _)| self.properties[id as usize].name == name)
                    .unwrap_or_else(|| panic!("{key} has no property {name}"));
                let index = self.properties[id as usize].values.iter().position(|v| v == value).unwrap_or_else(|| panic!("{key} {name} has no value {value}"));
                state = self.with(state, id, index as u16);
                assert!(!named.contains(&id), "{key} names {name} twice");
                named.push(id);
            }
        }
        (state, named)
    }
}
