'use strict';

// Checks the Rust engine (core/) against the Java engine (McbenchEngine,
// the pinned server's own classes) byte for byte. Each case is a sequence
// of requests both engines get: a fresh placement, ticks, a player's
// inputs, and edits placed on the live world. Every reply and every file
// they write (blueprints, patches, entity lists) must be identical, and so
// must what each world holds besides its blocks after every request (the
// level random, scheduled ticks, block events, block entities, tickers), except
// where the Rust engine refuses ("unported: ..."): a refused case counts as
// refused, not as a match.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/core-parity.js [--verify] [--stock]
//     [--match regex] [--limit N] [--ticks N]       community builds
//     [--fuzz N] [--seed S] [--families a,b] [--shift]  random worlds instead
//     [--use]                                       the right-click cases (useCases) instead: one of every block
//                                                   that takes a "use" input, among redstone and sculk sensors
//     [--orbs N] [--seed S]                         N cases of furnaces whose experience orbs TNT frees, and of golem
//                                                   patterns (scripts/orb-cases.js), instead
//     [--steps file]                                a kept case's steps.json
//     [--keep folder] [--native] [--drops] [--case steps.json,...] [--batch N]
//     [--base binary]                               an earlier Rust build as the first engine
//     [--rust binary]                               a Rust build made elsewhere (an earlier commit's) as the Rust engine
//
// Community builds (the corpus MCBENCH_CORPUS names, see scripts/corpus.js) are placed as the live import
// places them, then ticked one tick at a time with each lever toggled at
// ticks 2 and ticks/2 and each button pressed at tick 4.
//
// --fuzz makes N random worlds from the --families' blocks in every state
// the registry has (core/minecraft/<version>/blocks.txt), with random inputs
// and random edits between ticks. A fuzz case that mismatches is shrunk to
// a smallest failing case (dropping commands and steps while it still
// fails) and kept in --keep.
//
// --drops turns off the fuzzer's care for supports (blocks that need one are
// placed anywhere, edits remove blocks with others on them), so blocks break
// and drop items often and the items' physics runs. With the hopper family,
// --drops also builds item rigs: hoppers under blocks that break (the drops
// fall into them), chained into chests, barrels, shulker boxes, droppers,
// dispensers, crafters, composters, furnaces, brewing stands, jukeboxes,
// bookshelves, decorated pots, shelves and further hoppers, with comparators
// reading the containers and levers driving the droppers, and more blocks
// that break above the rigs' hoppers as the case runs (so items move).
// --flow makes a case mostly rigs, fed more often with one kind of item
// (stacks grow, comparator levels change).
//
// --blast primes TNT near the pistons now and then and moves the pistons
// often, so that explosions reach moving pistons (a piston's head going out
// or its base going in, and the blocks it moves). Give it --ticks well over
// 80, TNT's fuse.
//
// --batch N makes a fuzz case's runs of ticks (where no input or edit comes
// between) one request of up to N ticks, to run more worlds for longer.
//
// The Java engine runs on GraalVM's JVM unless --native, which builds the
// native program the app runs.
//
// --verify compares the engines' VERIFY requests (verifier experiments)
// instead: see scripts/core-verify-parity.js.
//
// --base compares the Rust engine with an earlier build of itself (a binary
// of core/ made before a change that must not change a byte) instead of the
// Java engine: a quick check for speed work, run before the Java comparison.
//
// --stock compares the Java engine with its own stock path instead of the
// Rust engine: the second engine is the same program run with
// MCBENCH_ENGINE_STOCK=1 (Paper's own code paths only, see McbenchLevel), the
// first with the engine's fast paths (MCBENCH_ENGINE_STOCK=check with
// --check, or the mode --first names: 0, 1 or check; with --first 1 both
// engines are stock, which shows what the engine does differently from one
// run to the next). It never refuses, so every case counts. The harness
// calls the second engine "rust" in its output.

const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync, spawn } = require('child_process');
const ROOT = path.join(__dirname, '..');
const { minecraftLive } = require('../server/minecraft-live');
const { Engine, engineEnv, jvmEngine, coreBuildArgs, rust } = require('../server/engine');
const { importCommands } = require('../server/live-placement');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { World } = require('../server/world');
const { corpusArtifacts } = require('./corpus');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

const VERSION = '1.21.10';

// Blocks the fuzzer builds from, by family.
const FAMILIES = {
  inert: ['minecraft:stone', 'minecraft:glass', 'minecraft:white_wool', 'minecraft:oak_planks', 'minecraft:iron_block', 'minecraft:obsidian'],
  wire: ['minecraft:redstone_wire'],
  torch: ['minecraft:redstone_torch', 'minecraft:redstone_wall_torch'],
  diode: ['minecraft:repeater', 'minecraft:comparator'],
  lever: ['minecraft:lever'],
  button: ['minecraft:stone_button', 'minecraft:oak_button'],
  lamp: ['minecraft:redstone_lamp'],
  observer: ['minecraft:observer'],
  power: ['minecraft:redstone_block'],
  target: ['minecraft:target'],
  note: ['minecraft:note_block'],
  slab: ['minecraft:oak_slab', 'minecraft:stone_slab', 'minecraft:oxidized_cut_copper_slab', 'minecraft:waxed_cut_copper_slab'],
  stairs: ['minecraft:oak_stairs', 'minecraft:cobblestone_stairs', 'minecraft:cut_copper_stairs', 'minecraft:waxed_cut_copper_stairs'],
  trapdoor: ['minecraft:oak_trapdoor', 'minecraft:iron_trapdoor', 'minecraft:copper_trapdoor'],
  door: ['minecraft:oak_door', 'minecraft:iron_door', 'minecraft:copper_door'],
  gate: ['minecraft:oak_fence_gate', 'minecraft:crimson_fence_gate'],
  // Doors and trapdoors of the other kinds: copper ones of every wear and wax (they extend the plain ones or the weathering classes), bamboo.
  hatch: ['minecraft:waxed_copper_door', 'minecraft:exposed_copper_door', 'minecraft:weathered_copper_door', 'minecraft:waxed_oxidized_copper_door', 'minecraft:bamboo_door',
    'minecraft:waxed_copper_trapdoor', 'minecraft:oxidized_copper_trapdoor', 'minecraft:waxed_weathered_copper_trapdoor', 'minecraft:bamboo_trapdoor', 'minecraft:warped_fence_gate', 'minecraft:bamboo_fence_gate'],
  fence: ['minecraft:oak_fence', 'minecraft:nether_brick_fence'],
  wall: ['minecraft:cobblestone_wall', 'minecraft:mud_brick_wall'],
  bars: ['minecraft:iron_bars', 'minecraft:glass_pane', 'minecraft:white_stained_glass_pane', 'minecraft:copper_bars'],
  sign: ['minecraft:oak_sign', 'minecraft:oak_wall_sign', 'minecraft:oak_hanging_sign', 'minecraft:oak_wall_hanging_sign'],
  ladder: ['minecraft:ladder'],
  pumpkin: ['minecraft:carved_pumpkin', 'minecraft:jack_o_lantern', 'minecraft:snow_block'],
  golem: ['minecraft:carved_pumpkin', 'minecraft:copper_block', 'minecraft:waxed_oxidized_copper'],
  banner: ['minecraft:white_banner', 'minecraft:white_wall_banner'],
  coral: ['minecraft:tube_coral_fan', 'minecraft:tube_coral_wall_fan', 'minecraft:dead_tube_coral_fan', 'minecraft:dead_tube_coral_wall_fan'],
  coralplant: ['minecraft:tube_coral', 'minecraft:dead_tube_coral', 'minecraft:tube_coral_block'],
  barrel: ['minecraft:barrel'],
  chest: ['minecraft:chest', 'minecraft:trapped_chest', 'minecraft:ender_chest'],
  shulker: ['minecraft:shulker_box', ...['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan', 'purple', 'blue', 'brown', 'green', 'red', 'black'].map((color) => `minecraft:${color}_shulker_box`)],
  hopper: ['minecraft:hopper'],
  dispenser: ['minecraft:dispenser', 'minecraft:dropper'],
  crafter: ['minecraft:crafter'],
  sand: ['minecraft:sand', 'minecraft:red_sand', 'minecraft:gravel'],
  anvil: ['minecraft:anvil', 'minecraft:chipped_anvil', 'minecraft:damaged_anvil'],
  concrete: ['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan', 'purple', 'blue', 'brown', 'green', 'red', 'black'].map((color) => `minecraft:${color}_concrete_powder`),
  egg: ['minecraft:dragon_egg'],
  tnt: ['minecraft:tnt'],
  scaffolding: ['minecraft:scaffolding'],
  // Blocks entities touch (core/src/entity/inside.rs): powder snow, which freezes, and a cobweb, a cactus and a bed to land on.
  powder: ['minecraft:powder_snow', 'minecraft:powder_snow', 'minecraft:cobweb', 'minecraft:cactus', 'minecraft:white_bed'],
  dripstone: ['minecraft:pointed_dripstone', 'minecraft:dripstone_block'],
  // Blocks a falling block can replace where it lands, or not (canBeReplaced).
  replaceable: ['minecraft:short_grass', 'minecraft:fern', 'minecraft:snow', 'minecraft:vine', 'minecraft:glow_lichen', 'minecraft:candle', 'minecraft:pink_petals', 'minecraft:sea_pickle', 'minecraft:turtle_egg', 'minecraft:dead_bush'],
  composter: ['minecraft:composter'],
  cauldron: ['minecraft:cauldron', 'minecraft:water_cauldron', 'minecraft:lava_cauldron', 'minecraft:powder_snow_cauldron'],
  lectern: ['minecraft:lectern'],
  furnace: ['minecraft:furnace', 'minecraft:blast_furnace', 'minecraft:smoker'],
  brewing: ['minecraft:brewing_stand'],
  jukebox: ['minecraft:jukebox'],
  bookshelf: ['minecraft:chiseled_bookshelf'],
  crafting: ['minecraft:crafting_table'],
  cake: ['minecraft:cake', ...['white', 'orange', 'magenta', 'light_blue', 'yellow', 'lime', 'pink', 'gray', 'light_gray', 'cyan', 'purple', 'blue', 'brown', 'green', 'red', 'black'].map((color) => `minecraft:${color}_candle_cake`), 'minecraft:candle_cake'],
  bell: ['minecraft:bell'],
  bulb: ['', 'exposed_', 'weathered_', 'oxidized_', 'waxed_', 'waxed_exposed_', 'waxed_weathered_', 'waxed_oxidized_'].map((kind) => `minecraft:${kind}copper_bulb`),
  daylight: ['minecraft:daylight_detector'],
  tripwire: ['minecraft:tripwire', 'minecraft:tripwire_hook'],
  piston: ['minecraft:piston'],
  sticky: ['minecraft:sticky_piston'],
  slime: ['minecraft:slime_block'],
  honey: ['minecraft:honey_block'],
  head: [],
  moving: ['minecraft:moving_piston'],
  // The plants (blocks/plants): the ground they stand on, then the classes.
  ground: ['minecraft:dirt', 'minecraft:coarse_dirt', 'minecraft:moss_block', 'minecraft:soul_soil', 'minecraft:clay', 'minecraft:netherrack', 'minecraft:crimson_nylium', 'minecraft:terracotta', 'minecraft:jungle_log'],
  // The terrain blocks (blocks/terrain). Sand, red sand and soul sand belong to other ports.
  terrain: ['minecraft:grass_block', 'minecraft:podzol', 'minecraft:mycelium', 'minecraft:farmland', 'minecraft:dirt_path', 'minecraft:mud', 'minecraft:hay_block'],
  snow: ['minecraft:snow', 'minecraft:snow_block'],
  eggs: ['minecraft:turtle_egg', 'minecraft:sniffer_egg', 'minecraft:dried_ghast'],
  flowers: ['minecraft:poppy', 'minecraft:short_grass', 'minecraft:fern', 'minecraft:oak_sapling', 'minecraft:bush', 'minecraft:pink_petals', 'minecraft:wildflowers', 'minecraft:closed_eyeblossom', 'minecraft:open_eyeblossom', 'minecraft:wither_rose', 'minecraft:sweet_berry_bush', 'minecraft:torchflower'],
  roots: ['minecraft:crimson_roots', 'minecraft:warped_fungus', 'minecraft:crimson_fungus', 'minecraft:azalea', 'minecraft:flowering_azalea', 'minecraft:nether_wart', 'minecraft:dead_bush', 'minecraft:short_dry_grass', 'minecraft:tall_dry_grass', 'minecraft:lily_pad'],
  tallplants: ['minecraft:tall_grass', 'minecraft:large_fern', 'minecraft:sunflower', 'minecraft:lilac', 'minecraft:rose_bush', 'minecraft:peony', 'minecraft:pitcher_plant'],
  // (Crops and mushrooms refuse where canSurvive reads the light: their own families.)
  mushrooms: ['minecraft:red_mushroom', 'minecraft:brown_mushroom'],
  // The block-entity group (blocks/block_entities): skulls and heads, beds,
  // shelves, copper chests, copper golem statues.
  skull: ['skeleton', 'wither_skeleton', 'zombie', 'creeper', 'dragon', 'piglin', 'player'].flatMap((kind) => {
    const head = kind === 'skeleton' || kind === 'wither_skeleton' ? `${kind}_skull` : `${kind}_head`;
    const wall = kind === 'skeleton' || kind === 'wither_skeleton' ? `${kind}_wall_skull` : `${kind}_wall_head`;
    return [`minecraft:${head}`, `minecraft:${wall}`];
  }),
  bed: ['white', 'red', 'blue'].map((color) => `minecraft:${color}_bed`),
  shelf: ['oak', 'spruce', 'bamboo', 'warped'].map((wood) => `minecraft:${wood}_shelf`),
  copperchest: ['', 'exposed_', 'weathered_', 'oxidized_', 'waxed_', 'waxed_exposed_', 'waxed_weathered_', 'waxed_oxidized_'].map((kind) => `minecraft:${kind}copper_chest`),
  statue: ['', 'exposed_', 'oxidized_', 'waxed_', 'waxed_weathered_'].map((kind) => `minecraft:${kind}copper_golem_statue`),
  // The face-attached blocks (blocks/multiface).
  multiface: ['minecraft:vine', 'minecraft:glow_lichen', 'minecraft:sculk_vein', 'minecraft:pale_moss_carpet'],
  decor: ['minecraft:flower_pot', 'minecraft:potted_poppy', 'minecraft:potted_cactus', 'minecraft:potted_open_eyeblossom', 'minecraft:potted_closed_eyeblossom', 'minecraft:candle', 'minecraft:red_candle', 'minecraft:white_carpet', 'minecraft:red_carpet',
    'minecraft:cobweb', 'minecraft:red_mushroom_block', 'minecraft:brown_mushroom_block', 'minecraft:mushroom_stem', 'minecraft:end_rod', 'minecraft:copper_grate', 'minecraft:waxed_oxidized_copper_grate', 'minecraft:exposed_copper_grate',
    'minecraft:iron_chain', 'minecraft:copper_chain', 'minecraft:waxed_weathered_copper_chain', 'minecraft:lightning_rod', 'minecraft:exposed_lightning_rod', 'minecraft:waxed_lightning_rod'],
  hangers: ['minecraft:lantern', 'minecraft:soul_lantern', 'minecraft:copper_lantern', 'minecraft:waxed_exposed_copper_lantern', 'minecraft:torch', 'minecraft:soul_torch', 'minecraft:copper_torch', 'minecraft:wall_torch', 'minecraft:soul_wall_torch',
    'minecraft:copper_wall_torch', 'minecraft:amethyst_cluster', 'minecraft:small_amethyst_bud', 'minecraft:medium_amethyst_bud', 'minecraft:large_amethyst_bud'],
  crops: ['minecraft:pitcher_crop', 'minecraft:wheat', 'minecraft:carrots', 'minecraft:potatoes', 'minecraft:beetroots', 'minecraft:torchflower_crop', 'minecraft:melon_stem', 'minecraft:pumpkin_stem', 'minecraft:attached_melon_stem', 'minecraft:attached_pumpkin_stem', 'minecraft:cocoa', 'minecraft:cactus_flower'],
  // The special blocks.
  plate: ['minecraft:oak_pressure_plate', 'minecraft:stone_pressure_plate', 'minecraft:polished_blackstone_pressure_plate', 'minecraft:light_weighted_pressure_plate', 'minecraft:heavy_weighted_pressure_plate'],
  fire: ['minecraft:fire', 'minecraft:soul_fire', 'minecraft:soul_soil', 'minecraft:oak_planks', 'minecraft:white_wool'],
  ore: ['minecraft:redstone_ore', 'minecraft:deepslate_redstone_ore'],
  frosted: ['minecraft:frosted_ice'],
  sculk: ['minecraft:sculk_sensor', 'minecraft:calibrated_sculk_sensor', 'minecraft:sculk_shrieker', 'minecraft:sculk_catalyst', 'minecraft:amethyst_block', 'minecraft:amethyst_cluster', 'minecraft:white_wool'],
  command: ['minecraft:command_block', 'minecraft:repeating_command_block', 'minecraft:chain_command_block'],
  spawner: ['minecraft:spawner', 'minecraft:vault'],
  trial: ['minecraft:trial_spawner'],
  beacon: ['minecraft:beacon', 'minecraft:iron_block', 'minecraft:white_stained_glass', 'minecraft:red_stained_glass', 'minecraft:white_stained_glass_pane', 'minecraft:bedrock'],
  conduit: ['minecraft:conduit', 'minecraft:prismarine'],
  enchanting: ['minecraft:enchanting_table', 'minecraft:bookshelf'],
  endblocks: ['minecraft:end_portal', 'minecraft:end_gateway', 'minecraft:end_portal_frame'],
  portal: ['minecraft:nether_portal', 'minecraft:obsidian', 'minecraft:fire'],
  anchor: ['minecraft:respawn_anchor'],
  invisible: ['minecraft:barrier', 'minecraft:light', 'minecraft:heavy_core', 'minecraft:structure_void', 'minecraft:grindstone', 'minecraft:stonecutter'],
  structure: ['minecraft:structure_block', 'minecraft:jigsaw', 'minecraft:test_block', 'minecraft:test_instance_block'],
  creaking: ['minecraft:creaking_heart', 'minecraft:pale_oak_log'],
  pot: ['minecraft:decorated_pot'],
  brushable: ['minecraft:suspicious_sand', 'minecraft:suspicious_gravel'],
  rod: ['minecraft:lightning_rod', 'minecraft:exposed_lightning_rod', 'minecraft:waxed_lightning_rod'],
  campfire: ['minecraft:campfire', 'minecraft:soul_campfire', 'minecraft:beehive', 'minecraft:bee_nest', 'minecraft:hay_block'],
  // Blocks that emit light or dim it (light opacity 1 to 14), for the light engine.
  glow: ['minecraft:glowstone', 'minecraft:sea_lantern', 'minecraft:shroomlight', 'minecraft:ice', 'minecraft:tinted_glass'],
  rail: ['minecraft:rail'],
  powered: ['minecraft:powered_rail', 'minecraft:activator_rail'],
  detector: ['minecraft:detector_rail'],
  // Fluids: water and lava of every level (half of them sources), blocks
  // that hold water (waterlogged half the time), bubble columns and what
  // makes them, kelp and seagrass, sponges.
  water: ['minecraft:water'],
  lava: ['minecraft:lava'],
  waterlogged: ['minecraft:water', 'minecraft:oak_slab', 'minecraft:oak_stairs', 'minecraft:oak_trapdoor', 'minecraft:oak_fence', 'minecraft:cobblestone_wall', 'minecraft:iron_bars', 'minecraft:glass_pane',
    'minecraft:oak_sign', 'minecraft:oak_wall_sign', 'minecraft:oak_hanging_sign', 'minecraft:oak_wall_hanging_sign', 'minecraft:ladder', 'minecraft:tube_coral', 'minecraft:tube_coral_fan', 'minecraft:tube_coral_wall_fan',
    'minecraft:dead_tube_coral', 'minecraft:dead_tube_coral_fan', 'minecraft:chest', 'minecraft:trapped_chest', 'minecraft:ender_chest'],
  bubble: ['minecraft:water', 'minecraft:soul_sand', 'minecraft:magma_block', 'minecraft:bubble_column'],
  kelp: ['minecraft:water', 'minecraft:kelp', 'minecraft:kelp_plant', 'minecraft:seagrass', 'minecraft:tall_seagrass'],
  sponge: ['minecraft:water', 'minecraft:sponge', 'minecraft:wet_sponge'],
  // Items that fall into lava and water: things that drop when they break,
  // over a lot of fluid (water and lava are listed more than once, which
  // makes them likelier; use with --drops).
  burn: ['minecraft:lava', 'minecraft:lava', 'minecraft:lava', 'minecraft:water', 'minecraft:ladder', 'minecraft:oak_sign', 'minecraft:oak_wall_sign', 'minecraft:oak_trapdoor', 'minecraft:redstone_wire', 'minecraft:lever',
    'minecraft:stone_button', 'minecraft:redstone_torch', 'minecraft:tube_coral_fan', 'minecraft:oak_hanging_sign'],
  swim: ['minecraft:water', 'minecraft:water', 'minecraft:water', 'minecraft:lava', 'minecraft:soul_sand', 'minecraft:magma_block', 'minecraft:bubble_column', 'minecraft:ladder', 'minecraft:oak_sign', 'minecraft:oak_wall_sign',
    'minecraft:oak_trapdoor', 'minecraft:redstone_wire', 'minecraft:lever', 'minecraft:redstone_torch', 'minecraft:kelp', 'minecraft:seagrass', 'minecraft:oak_hanging_sign'],
  // Water that meets lava, and what they make.
  clash: ['minecraft:water', 'minecraft:lava', 'minecraft:soul_soil', 'minecraft:blue_ice'],
  // The growing plants (blocks/growing): vines hang from or stand on a full
  // block in columns, sugar cane and cactus on sand or dirt, bamboo and
  // chorus on their own grounds. `frost` is frosted ice, which sugar cane
  // stands beside.
  vines: ['minecraft:weeping_vines', 'minecraft:weeping_vines_plant', 'minecraft:twisting_vines', 'minecraft:twisting_vines_plant', 'minecraft:cave_vines', 'minecraft:cave_vines_plant'],
  canes: ['minecraft:sugar_cane', 'minecraft:cactus'],
  bamboo: ['minecraft:bamboo_sapling', 'minecraft:bamboo'],
  chorus: ['minecraft:chorus_plant', 'minecraft:chorus_flower', 'minecraft:end_stone'],
  frost: ['minecraft:frosted_ice'],
  sandy: ['minecraft:sand', 'minecraft:red_sand'],
};

// The misc plants (blocks/misc): leaves and the logs their distance counts
// from, then the others.
Object.assign(FAMILIES, {
  logs: ['minecraft:oak_log', 'minecraft:stripped_dark_oak_wood', 'minecraft:pale_oak_log'],
  leaves: ['oak', 'spruce', 'birch', 'jungle', 'acacia', 'cherry', 'dark_oak', 'pale_oak', 'mangrove', 'azalea', 'flowering_azalea'].map((wood) => `minecraft:${wood}_leaves`),
  mangrove: ['minecraft:mangrove_propagule', 'minecraft:mangrove_roots', 'minecraft:mangrove_leaves'],
  dripleaf: ['minecraft:big_dripleaf', 'minecraft:big_dripleaf_stem', 'minecraft:small_dripleaf'],
  hanging: ['minecraft:hanging_roots', 'minecraft:spore_blossom', 'minecraft:pale_hanging_moss'],
  litter: ['minecraft:leaf_litter', 'minecraft:firefly_bush', 'minecraft:sea_pickle', 'minecraft:frogspawn'],
});

// Blocks that survive only on certain blocks: the blocks below them, where
// the fuzzer puts them (others it puts on them seldom).
const DIRT = ['minecraft:dirt', 'minecraft:grass_block', 'minecraft:podzol', 'minecraft:mycelium', 'minecraft:moss_block', 'minecraft:mud'];
const VEGETATION = [...DIRT, 'minecraft:farmland'];
const STANDS_ON = {};
for (const key of ['minecraft:poppy', 'minecraft:short_grass', 'minecraft:fern', 'minecraft:oak_sapling', 'minecraft:bush', 'minecraft:pink_petals', 'minecraft:wildflowers', 'minecraft:closed_eyeblossom',
  'minecraft:open_eyeblossom', 'minecraft:sweet_berry_bush', 'minecraft:torchflower', 'minecraft:tall_grass', 'minecraft:large_fern', 'minecraft:sunflower', 'minecraft:lilac', 'minecraft:rose_bush', 'minecraft:peony',
  'minecraft:pitcher_plant']) STANDS_ON[key] = VEGETATION;
Object.assign(STANDS_ON, {
  'minecraft:wither_rose': [...VEGETATION, 'minecraft:netherrack', 'minecraft:soul_sand', 'minecraft:soul_soil'],
  'minecraft:crimson_roots': [...VEGETATION, 'minecraft:crimson_nylium', 'minecraft:soul_soil'],
  'minecraft:warped_fungus': [...VEGETATION, 'minecraft:crimson_nylium', 'minecraft:mycelium', 'minecraft:soul_soil'],
  'minecraft:crimson_fungus': [...VEGETATION, 'minecraft:crimson_nylium', 'minecraft:mycelium', 'minecraft:soul_soil'],
  'minecraft:azalea': [...VEGETATION, 'minecraft:clay'],
  'minecraft:flowering_azalea': [...VEGETATION, 'minecraft:clay'],
  'minecraft:red_mushroom': ['minecraft:mycelium', 'minecraft:podzol', 'minecraft:crimson_nylium'],
  'minecraft:brown_mushroom': ['minecraft:mycelium', 'minecraft:podzol', 'minecraft:crimson_nylium'],
  'minecraft:nether_wart': ['minecraft:soul_sand'],
  'minecraft:dead_bush': ['minecraft:sand', 'minecraft:red_sand', 'minecraft:terracotta'],
  'minecraft:short_dry_grass': ['minecraft:sand', 'minecraft:red_sand', 'minecraft:terracotta'],
  'minecraft:tall_dry_grass': ['minecraft:sand', 'minecraft:red_sand', 'minecraft:terracotta'],
  'minecraft:melon_stem': ['minecraft:farmland'],
  'minecraft:pumpkin_stem': ['minecraft:farmland'],
  'minecraft:attached_melon_stem': ['minecraft:farmland'],
  'minecraft:attached_pumpkin_stem': ['minecraft:farmland'],
  'minecraft:wheat': ['minecraft:farmland'],
  'minecraft:carrots': ['minecraft:farmland'],
  'minecraft:potatoes': ['minecraft:farmland'],
  'minecraft:beetroots': ['minecraft:farmland'],
  'minecraft:torchflower_crop': ['minecraft:farmland'],
  'minecraft:pitcher_crop': ['minecraft:farmland'],
  'minecraft:cactus_flower': ['minecraft:sand', 'minecraft:red_sand', 'minecraft:farmland', 'minecraft:cactus'],
});
// The misc plants: a propagule (when it stands) on dirt or clay, the big
// dripleaf on its placeable blocks (or on a dripleaf), the small one on clay
// or moss, a sea pickle on a block with a top face (FULL, leaves).
const DRIPLEAF_BASE = ['minecraft:dirt', 'minecraft:coarse_dirt', 'minecraft:moss_block', 'minecraft:clay', 'minecraft:grass_block', 'minecraft:podzol', 'minecraft:mycelium', 'minecraft:farmland'];
Object.assign(STANDS_ON, {
  'minecraft:mangrove_propagule': [...VEGETATION, 'minecraft:clay'],
  'minecraft:big_dripleaf': [...DRIPLEAF_BASE, 'minecraft:big_dripleaf', 'minecraft:big_dripleaf_stem'],
  'minecraft:big_dripleaf_stem': [...DRIPLEAF_BASE, 'minecraft:big_dripleaf', 'minecraft:big_dripleaf_stem'],
  'minecraft:small_dripleaf': ['minecraft:clay', 'minecraft:moss_block'],
  'minecraft:firefly_bush': VEGETATION,
});
const BAMBOO_GROUND = ['minecraft:dirt', 'minecraft:coarse_dirt', 'minecraft:moss_block', 'minecraft:grass_block', 'minecraft:podzol', 'minecraft:mycelium', 'minecraft:sand', 'minecraft:red_sand', 'minecraft:bamboo', 'minecraft:bamboo_sapling'];
Object.assign(STANDS_ON, {
  'minecraft:sugar_cane': ['minecraft:sugar_cane', 'minecraft:dirt', 'minecraft:coarse_dirt', 'minecraft:moss_block', 'minecraft:grass_block', 'minecraft:podzol', 'minecraft:mycelium', 'minecraft:sand', 'minecraft:red_sand'],
  'minecraft:cactus': ['minecraft:cactus', 'minecraft:sand', 'minecraft:red_sand'],
  'minecraft:bamboo_sapling': BAMBOO_GROUND,
  'minecraft:bamboo': BAMBOO_GROUND,
  'minecraft:chorus_plant': ['minecraft:end_stone', 'minecraft:chorus_plant'],
  'minecraft:chorus_flower': ['minecraft:end_stone', 'minecraft:chorus_plant'],
});
// The ground blocks the fuzzer lays under plants.
const GROUND = new Set([...FAMILIES.ground, ...FAMILIES.terrain, ...FAMILIES.sandy, 'minecraft:end_stone'].filter((key) => key !== 'minecraft:hay_block' && key !== 'minecraft:dirt_path'));


// (The head family has no blocks of its own: it gives extended pistons their
// heads.)
// Pistons get blocks in front of them to push and redstone blocks beside them
// to power them (see fuzzCase).
const PISTONS = new Set(['minecraft:piston', 'minecraft:sticky_piston']);
const BLAST = process.argv.includes('--blast');
// Rails stand on full blocks, in runs that turn, climb and fall (see fuzzCase).
const RAILS = new Set([...FAMILIES.rail, ...FAMILIES.powered, ...FAMILIES.detector]);
const FLAT_SHAPES = ['north_south', 'east_west'];
const CURVED_SHAPES = ['south_east', 'south_west', 'north_west', 'north_east'];
const SLOPED_SHAPES = ['ascending_east', 'ascending_west', 'ascending_north', 'ascending_south'];
const DIRECTIONS = { north: [0, 0, -1], south: [0, 0, 1], west: [-1, 0, 0], east: [1, 0, 0], up: [0, 1, 0], down: [0, -1, 0] };

// Blocks the Rust core refuses when waterlogged (block-entity blocks it does not model with water).
const DRY = new Set([...FAMILIES.copperchest, ...FAMILIES.shelf, ...FAMILIES.statue]);

// The chance a waterlogged block is waterlogged: rare unless fluids are in
// the world.
let WET = 0.01;
const FLUIDS = new Set(['water', 'lava', 'waterlogged', 'bubble', 'kelp', 'sponge', 'clash', 'burn', 'swim']);

// The blocks that fall (FallingBlock): a case drops them from high up too.
const FALLING = new Set([...FAMILIES.sand, ...FAMILIES.anvil, ...FAMILIES.concrete, ...FAMILIES.egg, ...FAMILIES.scaffolding]);

// The registry: block key -> [[property, values]].
function registry() {
  const blocks = new Map();
  let current = null;
  require('../server/minecraft-tables').checkTables(VERSION);
  for (const line of fs.readFileSync(path.join(ROOT, 'core', 'minecraft', VERSION, 'blocks.txt'), 'utf8').split('\n')) {
    const p = line.split('\t');
    if (p[0] === 'block') blocks.set(p[1], current = []);
    else if (p[0] === 'property') current.push([p[1], p[2].split(',')]);
  }
  return blocks;
}

// A seeded random (mulberry32).
function random(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6D2B79F5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return { next, int: (n) => Math.floor(next() * n), pick: (list) => list[Math.floor(next() * list.length)] };
}

// A random state of the block, its properties random but for `given`.
// Water and lava are sources half the time.
function stateText(blocks, key, r, given = {}) {
  const properties = blocks.get(key);
  if (!properties.length) return key;
  const value = (name, values) => (name in given ? given[name] : name === 'waterlogged' ? (!DRY.has(key) && r.next() < WET ? 'true' : 'false')
    : name === 'level' && (key === 'minecraft:water' || key === 'minecraft:lava') && r.next() < 0.5 ? '0' : r.pick(values));
  return `${key}[${properties.map(([name, values]) => `${name}=${value(name, values)}`).join(',')}]`;
}

// A chest placed beside one of its kind often faces as it does and links to
// it (chests joining into a double chest are rare among random states).
const COPPER_CHESTS = new Set(FAMILIES.copperchest);
const CHESTS = new Set(['minecraft:chest', 'minecraft:trapped_chest', ...COPPER_CHESTS]);
const CLOCKWISE = { north: 'east', east: 'south', south: 'west', west: 'north' };
function chestPair(r, placed, key, x, y, z) {
  if (!CHESTS.has(key) || r.next() < 0.4) return {};
  for (const [dx, dz, toward] of [[-1, 0, 'west'], [0, -1, 'north']]) {
    const other = placed.get(`${x + dx},${y},${z + dz}`);
    // (Copper chests of any oxidation and wax join.)
    if (!other || (other.key !== key && !(COPPER_CHESTS.has(key) && COPPER_CHESTS.has(other.key))) || (other.facing === 'north' || other.facing === 'south') !== (dx !== 0)) continue;
    return { facing: other.facing, type: CLOCKWISE[other.facing] === toward ? 'left' : 'right' };
  }
  return {};
}

// Shelves placed beside a shelf mostly face as it does (and are mostly
// powered), so their chains form: shelves of one facing stand side by side
// along the axis across it.
const SHELVES = new Set(FAMILIES.shelf);
function shelfPair(r, placed, key, x, y, z) {
  if (!SHELVES.has(key) || r.next() < 0.3) return {};
  for (const [dx, dz] of [[-1, 0], [0, -1]]) {
    const other = placed.get(`${x + dx},${y},${z + dz}`);
    if (!other || !SHELVES.has(other.key) || (other.facing === 'north' || other.facing === 'south') !== (dx !== 0)) continue;
    return { facing: other.facing, ...(r.next() < 0.8 ? { powered: 'true' } : {}) };
  }
  return {};
}

// Blocks that need no support (skulls, heads, beds, shelves, statues).
const NO_SUPPORT = new Set([...FAMILIES.skull, ...FAMILIES.bed, ...FAMILIES.shelf, ...FAMILIES.copperchest, ...FAMILIES.statue]);
// Beds: two halves, the head in front of the foot.
const BEDS = new Set(FAMILIES.bed);

// Blocks with a full, sturdy top: what wire, torches, diodes, levers and
// buttons stand on.
const FULL = new Set([...FAMILIES.inert, ...FAMILIES.glow, 'minecraft:redstone_block', 'minecraft:target', 'minecraft:redstone_lamp', 'minecraft:observer', 'minecraft:note_block',
  'minecraft:magma_block', 'minecraft:sponge', 'minecraft:wet_sponge', 'minecraft:soul_soil', 'minecraft:blue_ice',
  'minecraft:carved_pumpkin', 'minecraft:jack_o_lantern', 'minecraft:snow_block', 'minecraft:copper_block', 'minecraft:tube_coral_block',
  ...FAMILIES.barrel, ...FAMILIES.shulker, ...FAMILIES.dispenser, ...FAMILIES.crafter,
  ...FAMILIES.furnace, ...FAMILIES.jukebox, ...FAMILIES.bookshelf, ...FAMILIES.crafting, ...FAMILIES.bulb, ...FAMILIES.slime,
  ...FAMILIES.command, 'minecraft:spawner', 'minecraft:beacon', 'minecraft:bookshelf', 'minecraft:prismarine',
  ...FAMILIES.ore, 'minecraft:pale_oak_log', 'minecraft:white_stained_glass', 'minecraft:red_stained_glass', 'minecraft:bedrock', 'minecraft:amethyst_block', 'minecraft:sculk_catalyst', 'minecraft:test_block',
  'minecraft:structure_block', 'minecraft:suspicious_sand', 'minecraft:suspicious_gravel',
  ...FAMILIES.sand, ...FAMILIES.concrete, ...FAMILIES.tnt, 'minecraft:dripstone_block',
  ...[...FAMILIES.ground, ...FAMILIES.terrain].filter((key) => key !== 'minecraft:farmland' && key !== 'minecraft:dirt_path'), 'minecraft:end_stone']);

FAMILIES.logs.forEach((key) => FULL.add(key));
STANDS_ON['minecraft:sea_pickle'] = [...FULL, ...FAMILIES.leaves];
// Blocks that stand anywhere (leaves, roots, spawn), and the ones that hang
// from a block above (placed with their support).
const ANYWHERE = new Set([...FAMILIES.leaves, 'minecraft:mangrove_roots', 'minecraft:frogspawn']);
const HANGING = new Set([...FAMILIES.hanging, 'minecraft:mangrove_propagule']);
// The big dripleaf and its stem are placed as a column of stems under a leaf.
const DRIPLEAF = new Set(['minecraft:big_dripleaf', 'minecraft:big_dripleaf_stem']);

// Blocks that hold faces on the full blocks around them (vines hang from the
// vine above too), and the pale moss carpet, which stands on any block (and
// a carpet with a base).
const MULTIFACE = new Set(['minecraft:vine', 'minecraft:glow_lichen', 'minecraft:sculk_vein']);
const PARTIAL = ['minecraft:stone_slab', 'minecraft:oak_stairs', 'minecraft:oak_trapdoor'];
STANDS_ON['minecraft:pale_moss_carpet'] = [...FULL, 'minecraft:pale_moss_carpet'];

// Blocks attached to the side of a block (the block behind is full), and the
// two-block ones.
const SIDE_ATTACHED = new Set(['minecraft:wall_torch', 'minecraft:soul_wall_torch', 'minecraft:copper_wall_torch', 'minecraft:oak_wall_sign', 'minecraft:white_wall_banner', 'minecraft:ladder', 'minecraft:tube_coral_wall_fan', 'minecraft:dead_tube_coral_wall_fan']);
const CEILING_ATTACHED = new Set(['minecraft:oak_hanging_sign']);
const DOORS = new Set(['minecraft:oak_door', 'minecraft:iron_door', 'minecraft:copper_door', 'minecraft:tall_seagrass', ...FAMILIES.hatch.filter((key) => key.endsWith('_door'))]);
// Blocks that need nothing under them (fluids and what they hold up).
const FREE = new Set(['minecraft:water', 'minecraft:lava', 'minecraft:bubble_column']);
// Blocks of two halves, placed together (DOORS and the tall plants).
const TWO_HIGH = new Set([...DOORS, ...FAMILIES.tallplants, 'minecraft:small_dripleaf']);
// A block attached to a log it faces (cocoa).
const LOG_ATTACHED = new Set(['minecraft:cocoa']);
// Lanterns stand on or hang from a full block; amethyst clusters point away
// from a full block in any of the six directions.
const LANTERNS = new Set(['minecraft:lantern', 'minecraft:soul_lantern', 'minecraft:copper_lantern', 'minecraft:waxed_exposed_copper_lantern']);
const POINTING = new Set(['minecraft:amethyst_cluster', 'minecraft:small_amethyst_bud', 'minecraft:medium_amethyst_bud', 'minecraft:large_amethyst_bud']);
// The vines, in columns along their growth: [step of the growth (up or down),
// head, body]. A column has its head (usually) at the end, a full block at its
// root, and the cactus keeps solid blocks away from its sides.
const WEEPING = [-1, 'minecraft:weeping_vines', 'minecraft:weeping_vines_plant'];
const TWISTING = [1, 'minecraft:twisting_vines', 'minecraft:twisting_vines_plant'];
const CAVE = [-1, 'minecraft:cave_vines', 'minecraft:cave_vines_plant'];
const VINES = {
  'minecraft:weeping_vines': WEEPING, 'minecraft:weeping_vines_plant': WEEPING,
  'minecraft:twisting_vines': TWISTING, 'minecraft:twisting_vines_plant': TWISTING,
  'minecraft:cave_vines': CAVE, 'minecraft:cave_vines_plant': CAVE,
};
const CHORUS = new Set(['minecraft:chorus_plant', 'minecraft:chorus_flower']);
const BAMBOO = new Set(['minecraft:bamboo_sapling', 'minecraft:bamboo']);
const STEP = { north: [0, -1], south: [0, 1], west: [-1, 0], east: [1, 0] };
const OPPOSITE = { north: 'south', south: 'north', west: 'east', east: 'west' };

// Blocks that break when nothing holds them and drop an item, as the fuzzer
// places them without supports.
const FRAGILE = new Set(['minecraft:redstone_wire', 'minecraft:redstone_torch', 'minecraft:redstone_wall_torch', 'minecraft:lever', 'minecraft:stone_button', 'minecraft:oak_button',
  'minecraft:repeater', 'minecraft:comparator', 'minecraft:ladder', 'minecraft:oak_sign', 'minecraft:oak_wall_sign', 'minecraft:white_banner', 'minecraft:white_wall_banner',
  'minecraft:tube_coral_fan', 'minecraft:dead_tube_coral_fan', 'minecraft:oak_hanging_sign']);
const CONTAINER_FAMILIES = ['chest', 'barrel', 'shulker', 'dispenser', 'crafter', 'composter', 'furnace', 'brewing', 'jukebox', 'bookshelf', 'pot', 'shelf'];
// Containers with a horizontal facing only (or none).
const HORIZONTAL_CONTAINERS = new Set(['minecraft:furnace', 'minecraft:blast_furnace', 'minecraft:smoker', 'minecraft:chiseled_bookshelf', 'minecraft:decorated_pot', ...FAMILIES.shelf]);
const FACINGS = ['north', 'south', 'east', 'west'];

// Item rigs (see --drops): each is a hopper with fragile blocks over it and a
// line of containers it feeds, with comparators and levers around them.
function buildRigs({ blocks, families, keys, r, size, height, grid, commands, inputs, feeds, flow }) {
  const inside = (x, y, z) => x >= 0 && z >= 0 && x < size && z < size && y >= 1 && y <= height + 2;
  const set = (key, x, y, z, given = {}) => {
    const text = stateText(blocks, key, r, given);
    grid.set(`${x},${y},${z}`, key);
    commands.push(`setblock ${x} ${y} ${z} ${text}`);
    return text;
  };
  // A container: a hopper (twice as likely) or a family's, then a block of it.
  const poolFamilies = [['minecraft:hopper'], ['minecraft:hopper'], ...CONTAINER_FAMILIES.filter((family) => families.includes(family)).map((family) => FAMILIES[family].filter((key) => key !== 'minecraft:ender_chest'))];
  const pool = { pick: () => r.pick(r.pick(poolFamilies)) };
  const fragile = keys.filter((key) => FRAGILE.has(key));
  const step = (dir) => (dir === 'down' ? [0, -1, 0] : dir === 'up' ? [0, 1, 0] : [STEP[dir][0], 0, STEP[dir][1]]);
  const rigsCount = flow ? 2 + r.int(4) : 1 + r.int(3);
  for (let n = 0; n < rigsCount; n++) {
    let [x, y, z] = [r.int(size), 1 + r.int(height), r.int(size)];
    // The feeding hopper, with blocks over it that break.
    let dir = r.pick(['down', 'down', ...FACINGS]);
    set('minecraft:hopper', x, y, z, { facing: dir, enabled: r.next() < 0.9 ? 'true' : 'false' });
    const rigFragile = flow && fragile.length ? [r.pick(fragile)] : fragile;
    feeds.push([x, y + 1, z, flow && rigFragile.length ? rigFragile[0] : null]);
    if (rigFragile.length) {
      for (let k = 1 + r.int(2); k > 0; k--) {
        const key = r.pick(rigFragile);
        if (DOORS.has(key) || SIDE_ATTACHED.has(key) || CEILING_ATTACHED.has(key)) continue;
        if (inside(x, y + k, z)) set(key, x, y + k, z);
      }
    }
    const placed = [];
    // The line: each element is where the one before faces.
    for (let depth = 0; depth < 4; depth++) {
      const [dx, dy, dz] = step(dir);
      [x, y, z] = [x + dx, y + dy, z + dz];
      if (!inside(x, y, z)) break;
      const key = pool.pick();
      let facing = r.pick(['down', ...FACINGS]);
      if (key === 'minecraft:hopper') {
        set(key, x, y, z, { facing, enabled: r.next() < 0.9 ? 'true' : 'false' });
        placed.push([key, x, y, z]);
        // The hopper may be fed by a block over it too.
        if (fragile.length && r.next() < 0.3 && inside(x, y + 1, z)) set(r.pick(fragile.filter((k) => !DOORS.has(k) && !SIDE_ATTACHED.has(k) && !CEILING_ATTACHED.has(k))), x, y + 1, z);
        dir = facing;
        continue;
      }
      if (CHESTS.has(key)) {
        facing = r.pick(FACINGS);
        if (r.next() < 0.4) {
          // A double chest: this one on the left, its partner clockwise.
          const [px, , pz] = step(CLOCKWISE[facing]);
          if (inside(x + px, y, z + pz)) {
            set(key, x, y, z, { facing, type: 'left' });
            set(key, x + px, y, z + pz, { facing, type: 'right' });
            placed.push([key, x, y, z]);
            break;
          }
        }
        set(key, x, y, z, { facing, type: 'single' });
      } else if (key === 'minecraft:crafter') {
        set(key, x, y, z, { triggered: 'false', crafting: 'false' });
      } else if (HORIZONTAL_CONTAINERS.has(key)) {
        set(key, x, y, z, { facing: r.pick(FACINGS) });
      } else if (key === 'minecraft:composter' || key === 'minecraft:brewing_stand' || key === 'minecraft:jukebox') {
        set(key, x, y, z);
      } else {
        set(key, x, y, z, { facing });
      }
      placed.push([key, x, y, z]);
      if (key === 'minecraft:dropper' || key === 'minecraft:dispenser') {
        // Levers that power it: toggled by the case's inputs.
        for (let k = 1 + r.int(2); k > 0; k--) {
          const side = r.pick(FACINGS);
          const [sx, , sz] = step(side);
          if (inside(x + sx, y, z + sz) && families.includes('lever')) {
            set('minecraft:lever', x + sx, y, z + sz, { face: 'wall', facing: side, powered: 'false' });
            inputs.push(['toggle', x + sx, y, z + sz]);
          }
        }
        // It dispenses toward the next element.
        dir = facing;
        continue;
      }
      break;
    }
    // A hopper under a container pulls from it, and passes on to a container of its own.
    for (const [key, cx, cy, cz] of placed) {
      if (key === 'minecraft:hopper' || r.next() >= 0.4 || !inside(cx, cy - 1, cz)) continue;
      const out = r.pick(['down', ...FACINGS]);
      set('minecraft:hopper', cx, cy - 1, cz, { facing: out, enabled: 'true' });
      const [ox, oy, oz] = step(out);
      const containers = poolFamilies.filter((family) => family[0] !== 'minecraft:hopper');
      if (containers.length && inside(cx + ox, cy - 1 + oy, cz + oz)) {
        const container = r.pick(r.pick(containers));
        const given = CHESTS.has(container) || HORIZONTAL_CONTAINERS.has(container) ? { facing: r.pick(FACINGS), ...(CHESTS.has(container) ? { type: 'single' } : {}) } : ['minecraft:composter', 'minecraft:crafter', 'minecraft:brewing_stand', 'minecraft:jukebox'].includes(container) ? {} : { facing: r.pick(['down', ...FACINGS]) };
        set(container, cx + ox, cy - 1 + oy, cz + oz, given);
      }
    }
    // A composter over the first hopper, full of bone meal for it to pull.
    if (families.includes('composter') && r.next() < 0.25) {
      const [hx, hy, hz] = feeds[feeds.length - 1];
      if (inside(hx, hy, hz)) set('minecraft:composter', hx, hy, hz, { level: r.next() < 0.7 ? '8' : String(r.int(9)) });
    }
    // Comparators reading the line's containers, with a lamp or wire after them.
    if (families.includes('diode')) {
      for (const [key, cx, cy, cz] of placed) {
        if (r.next() < (flow ? 0.9 : 0.6)) {
          const side = r.pick(FACINGS);
          const [sx, , sz] = step(side);
          if (!inside(cx + sx, cy, cz + sz)) continue;
          set('minecraft:comparator', cx + sx, cy, cz + sz, { facing: side });
          if (cy - 1 >= 0) {
            commands.push(`setblock ${cx + sx} ${cy - 1} ${cz + sz} minecraft:stone`);
            grid.set(`${cx + sx},${cy - 1},${cz + sz}`, 'minecraft:stone');
          }
          if (families.includes('lamp') && inside(cx + 2 * sx, cy, cz + 2 * sz) && r.next() < 0.7) set('minecraft:redstone_lamp', cx + 2 * sx, cy, cz + 2 * sz, { lit: 'false' });
          else if (families.includes('wire') && inside(cx + 2 * sx, cy, cz + 2 * sz) && r.next() < 0.7) set('minecraft:redstone_wire', cx + 2 * sx, cy, cz + 2 * sz);
        }
      }
    }
  }
}

// The blocks a right click ("use" input) is made on: doors, trapdoors, fence
// gates, repeaters, comparators, note blocks and daylight detectors, the iron
// ones too (they take it and do nothing, as an error).
const USED = new Set([...FAMILIES.door, ...FAMILIES.trapdoor, ...FAMILIES.gate, ...FAMILIES.hatch, ...FAMILIES.diode, ...FAMILIES.note, ...FAMILIES.daylight, ...FAMILIES.wire, ...FAMILIES.pot]);

// The yaw of a right click: the four ways a player faces, the edges between
// them (a gate faces the way the yaw rounds to) and any angle, as floats.
const YAWS = [0, 90, -90, 180, -180, 270, 360, 45, -45, 135, -135, 225, 315, 44.99, 45.01, -44.99, 134.99, 540, -540];
const yawOf = (r) => Math.fround(r.next() < 0.5 ? r.pick(YAWS) : (r.next() - 0.5) * 1440);

// The right clicks' cases: a row of every block that takes one, with redstone
// to show what the click changed (a lamp the repeater and the comparator feed,
// a lamp a door-side lever lights, sculk sensors that hear the doors and a
// decorated pot, redstone dust as a dot, a cross, a line and a junction, and a
// dot on a redstone block that turns into a stone with a lamp behind), and
// clicks over 80 ticks at every yaw that tells a fence gate's facing apart. The
// iron door and trapdoor take the click and do nothing, as do the line and the
// junction of dust: an error on both.
function useCases() {
  const floor = 'fill 0 63 0 24 63 12 minecraft:stone';
  const door = (x, z, wood, facing, hinge) => [
    `setblock ${x} 64 ${z} minecraft:${wood}_door[facing=${facing},half=lower,hinge=${hinge},open=false,powered=false]`,
    `setblock ${x} 65 ${z} minecraft:${wood}_door[facing=${facing},half=upper,hinge=${hinge},open=false,powered=false]`];
  const row = [
    floor,
    ...door(1, 1, 'oak', 'south', 'left'), ...door(3, 1, 'copper', 'east', 'right'), ...door(5, 1, 'iron', 'north', 'left'), ...door(7, 1, 'waxed_oxidized_copper', 'west', 'left'),
    'setblock 9 64 1 minecraft:spruce_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]',
    'setblock 11 64 1 minecraft:oak_trapdoor[facing=south,half=top,open=false,powered=false,waterlogged=true]',
    'setblock 13 64 1 minecraft:iron_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]',
    'setblock 15 64 1 minecraft:oak_fence_gate[facing=north,in_wall=false,open=false,powered=false]',
    'setblock 17 64 1 minecraft:crimson_fence_gate[facing=east,in_wall=false,open=false,powered=false]',
    'setblock 19 64 1 minecraft:daylight_detector[inverted=false,power=0]',
    // A repeater into wire into a lamp; a comparator (back: a redstone block, side: one too) into a lamp.
    'setblock 1 64 5 minecraft:redstone_block', 'setblock 2 64 5 minecraft:repeater[delay=1,facing=west,locked=false,powered=false]', 'setblock 3 64 5 minecraft:redstone_wire', 'setblock 4 64 5 minecraft:redstone_lamp',
    'setblock 7 64 5 minecraft:redstone_block', 'setblock 6 64 5 minecraft:comparator[facing=east,mode=compare,powered=false]', 'setblock 6 64 4 minecraft:redstone_block', 'setblock 5 64 5 minecraft:redstone_lamp',
    // A note block (a harp over stone, a bell over gold) and a lever beside a lamp.
    'setblock 9 64 5 minecraft:note_block[instrument=harp,note=0,powered=false]', 'setblock 11 64 5 minecraft:note_block[instrument=harp,note=24,powered=false]', 'setblock 11 63 5 minecraft:gold_block',
    // Sculk sensors by the doors, a calibrated one by a trapdoor, and a block of wool that dampens the vibration beside one.
    'setblock 2 64 3 minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]', 'setblock 10 64 3 minecraft:calibrated_sculk_sensor[facing=north,power=0,sculk_sensor_phase=inactive,waterlogged=false]',
    'setblock 16 64 3 minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]', 'setblock 17 64 3 minecraft:white_wool',
    // Redstone dust: a dot, a cross, a line, a junction, and a dot on a redstone block beside a stone with a lamp behind it.
    'setblock 1 64 9 minecraft:redstone_wire[east=none,north=none,power=0,south=none,west=none]',
    'setblock 3 64 9 minecraft:redstone_wire[east=side,north=side,power=0,south=side,west=side]',
    'setblock 5 64 9 minecraft:redstone_wire[east=side,north=none,power=0,south=none,west=side]', 'setblock 6 64 9 minecraft:redstone_wire[east=side,north=none,power=0,south=none,west=side]',
    ...[[9, 10], [8, 10], [10, 10], [9, 9], [9, 11]].map(([x, z]) => `setblock ${x} 64 ${z} minecraft:redstone_wire[east=side,north=side,power=0,south=side,west=side]`),
    'setblock 13 63 9 minecraft:redstone_block', 'setblock 13 64 9 minecraft:redstone_wire[east=none,north=none,power=15,south=none,west=none]',
    'setblock 14 64 9 minecraft:stone', 'setblock 15 64 9 minecraft:redstone_lamp[lit=false]',
    // A decorated pot a sculk sensor hears.
    'setblock 21 64 1 minecraft:decorated_pot[cracked=false,facing=north,waterlogged=false]', 'setblock 21 64 4 minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]',
  ];
  const at = {
    oak: [1, 64, 1], copper: [3, 64, 1], iron: [5, 64, 1], waxed: [7, 64, 1], spruce: [9, 64, 1], wet: [11, 64, 1], irontrap: [13, 64, 1], gate: [15, 64, 1], crimson: [17, 64, 1],
    daylight: [19, 64, 1], repeater: [2, 64, 5], comparator: [6, 64, 5], note: [9, 64, 5], high: [11, 64, 5],
    dot: [1, 64, 9], cross: [3, 64, 9], line: [5, 64, 9], junction: [9, 64, 10], powered: [13, 64, 9], pot: [21, 64, 1],
  };
  const yaws = [0, 90, 180, -90, 45, -45, 134.99, 270, -180, 315.5];
  // With `errors`, the clicks the blocks do not take too (iron, stone).
  const rowSteps = (errors) => {
    const steps = [{ place: row, fresh: true }, { tick: 2 }];
    let n = 0;
    for (let t = 0; t < 80; t += 1) {
      for (const name of Object.keys(at)) {
        if (!errors && (name.startsWith('iron') || name === 'line' || name === 'junction')) continue;
        if ((t + name.length) % 4 === 0) steps.push({ input: ['use', ...at[name], Math.fround(yaws[n++ % yaws.length])] });
      }
      // The upper half of a door takes the click as the lower half does.
      if (t % 9 === 0) steps.push({ input: ['use', 1, 65, 1, Math.fround(yaws[n++ % yaws.length])] });
      // A block that takes no click is an error on both.
      if (errors && t % 25 === 3) steps.push({ input: ['use', 0, 63, 0, 0] });
      steps.push({ tick: 1 });
    }
    return steps;
  };
  const cases = [{ name: 'use-row', steps: rowSteps(true) }, { name: 'use-row-taken', steps: rowSteps(false) }];
  // Fence gates facing each way, opened by players looking each way.
  const gates = [floor];
  const gateAt = [];
  ['north', 'south', 'east', 'west'].forEach((facing, i) => {
    for (let k = 0; k < 4; k += 1) {
      gates.push(`setblock ${1 + k * 3} 64 ${1 + i * 3} minecraft:oak_fence_gate[facing=${facing},in_wall=false,open=false,powered=false]`);
      gateAt.push([1 + k * 3, 64, 1 + i * 3]);
    }
  });
  const gateSteps = [{ place: gates, fresh: true }, { tick: 1 }];
  [0, 90, 180, 270, -90, 45, -45, 44.99, 135, 224.99].forEach((yaw) => {
    for (const cell of gateAt) gateSteps.push({ input: ['use', ...cell, Math.fround(yaw)] });
    gateSteps.push({ tick: 3 });
  });
  for (let t = 0; t < 40; t += 1) gateSteps.push({ tick: 1 });
  cases.push({ name: 'use-gates', steps: gateSteps });
  // Sculk shriekers that hear the input player through the sensors its
  // clicks set off (the tendrils' clicking names the player): one an observer
  // watches into a lamp, one that could summon, a waterlogged one, one behind
  // wool, one already shrieking, one out of reach, one by a calibrated sensor.
  const sculk = [
    floor,
    ...door(1, 1, 'oak', 'south', 'left'),
    'setblock 2 64 3 minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]',
    'setblock 4 64 3 minecraft:sculk_shrieker[can_summon=false,shrieking=false,waterlogged=false]',
    'setblock 5 64 3 minecraft:observer[facing=west,powered=false]', 'setblock 6 64 3 minecraft:redstone_lamp[lit=false]',
    'setblock 2 64 6 minecraft:sculk_shrieker[can_summon=true,shrieking=false,waterlogged=false]',
    'setblock 9 64 1 minecraft:spruce_trapdoor[facing=north,half=bottom,open=false,powered=false,waterlogged=false]',
    'setblock 9 64 3 minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]',
    'setblock 11 64 3 minecraft:sculk_shrieker[can_summon=false,shrieking=false,waterlogged=true]',
    'setblock 15 64 1 minecraft:oak_fence_gate[facing=north,in_wall=false,open=false,powered=false]',
    'setblock 15 64 3 minecraft:sculk_sensor[power=0,sculk_sensor_phase=inactive,waterlogged=false]',
    'setblock 16 64 3 minecraft:white_wool', 'setblock 16 65 3 minecraft:white_wool', 'setblock 16 64 4 minecraft:white_wool',
    'setblock 17 64 3 minecraft:sculk_shrieker[can_summon=false,shrieking=false,waterlogged=false]',
    'setblock 15 64 7 minecraft:sculk_shrieker[can_summon=true,shrieking=true,waterlogged=false]',
    'setblock 24 64 11 minecraft:sculk_shrieker[can_summon=false,shrieking=false,waterlogged=false]',
    'setblock 21 64 1 minecraft:birch_door[facing=south,half=lower,hinge=left,open=false,powered=false]', 'setblock 21 65 1 minecraft:birch_door[facing=south,half=upper,hinge=left,open=false,powered=false]',
    'setblock 21 64 4 minecraft:calibrated_sculk_sensor[facing=south,power=0,sculk_sensor_phase=inactive,waterlogged=false]',
    'setblock 21 64 7 minecraft:sculk_shrieker[can_summon=false,shrieking=false,waterlogged=false]',
  ];
  const sculkAt = [[1, 64, 1], [9, 64, 1], [15, 64, 1], [21, 64, 1]];
  const sculkSteps = [{ place: sculk, fresh: true }, { tick: 2 }];
  for (let t = 0; t < 320; t += 1) {
    sculkAt.forEach((cell, i) => {
      if ((t + i * 7) % (11 + i * 6) === 0) sculkSteps.push({ input: ['use', ...cell, Math.fround(yaws[(t + i) % yaws.length])] });
    });
    sculkSteps.push({ tick: 1 });
  }
  cases.push({ name: 'use-shriekers', steps: sculkSteps });
  return cases;
}

// A random world: a stone floor with holes, then blocks above it, a block
// that needs support only on a full block (mostly), and a plan of ticks,
// inputs and edits. Edits remove only blocks nothing stands on (mostly), so
// most cases run without breaking blocks, which would drop items.
function fuzzCase(blocks, families, seed, ticks, drops, flow, shifted = false) {
  const r = random(seed);
  // (The `sandy` blocks only lie on the floor: elsewhere they would fall.)
  const all = families.flatMap((family) => FAMILIES[family]);
  const keys = all.filter((key) => !FAMILIES.sandy.includes(key));
  const full = keys.filter((key) => FULL.has(key));
  const falling = keys.filter((key) => FALLING.has(key));
  const free = keys.filter((key) => FREE.has(key));
  // Fluids spread over larger floors.
  const wet = families.some((family) => FLUIDS.has(family));
  WET = wet ? 0.4 : 0.01;
  const size = wet ? 5 + r.int(9) : 3 + r.int(6);
  const height = 1 + r.int(3);
  const grid = new Map();
  const at = (x, y, z) => grid.get(`${x},${y},${z}`) || null;
  const commands = [];
  const chests = new Map();
  const inputs = [];
  // A tripwire hook faces away from a full block beside it (west or north of
  // it, already placed); a bell mostly stands on the full block below.
  const attachment = (key, x, y, z) => {
    if (key === 'minecraft:tripwire_hook') {
      if (FULL.has(at(x - 1, y, z))) return { facing: 'east' };
      if (FULL.has(at(x, y, z - 1))) return { facing: 'south' };
    }
    if (key === 'minecraft:bell' && r.next() < 0.8) return { attachment: 'floor' };
    return {};
  };
  const pistons = [];
  const paleBases = new Map();
  // Whether a block can stand on the block below it.
  const stands = (key, below) => FULL.has(key) || NO_SUPPORT.has(key) || ANYWHERE.has(key) || MULTIFACE.has(key) || VINES[key] || HANGING.has(key) || (STANDS_ON[key] ? STANDS_ON[key].includes(below) : below && FULL.has(below));
  // A rail's shape: mostly flat, now and then curved (the plain rail) or sloped.
  const railShape = (key) => {
    const roll = r.next();
    if (roll < 0.1) return r.pick(SLOPED_SHAPES);
    if (key === 'minecraft:rail' && roll < 0.4) return r.pick(CURVED_SHAPES);
    return r.pick(FLAT_SHAPES);
  };
  const choose = (x, y, z) => {
    if (drops) return r.pick(keys);
    const below = y === 0 ? 'minecraft:stone' : at(x, y - 1, z);
    // (A chorus plant or flower also hangs beside a chorus plant.)
    const chorusBeside = Object.values(STEP).some(([dx, dz]) => at(x + dx, y, z + dz) === 'minecraft:chorus_plant');
    const candidates = keys.filter((key) => stands(key, below) || (CHORUS.has(key) && chorusBeside)).concat(free);
    // Above a wall any block goes (a wall's shape depends on the block above it).
    const key = r.next() < (below && below.endsWith('_wall') ? 0.7 : 0.05) || (!candidates.length && wantsGround) ? r.pick(keys) : candidates.length ? r.pick(candidates) : null;
    // A hook with no wall to hang on would be destroyed (and drop) at once.
    return key === 'minecraft:tripwire_hook' && !attachment(key, x, y, z).facing ? 'minecraft:tripwire' : key;
  };
  // The commands that place a block: a side-attached block faces away from a
  // full block, a door comes with its upper half, a hanging sign under a full
  // block; null where the block cannot stand.
  const placement = (key, x, y, z, edit) => {
    const put = (block, px, py, pz, text) => {
      grid.set(`${px},${py},${pz}`, block);
      return `setblock ${px} ${py} ${pz} ${text}`;
    };
    if (MULTIFACE.has(key)) {
      // Faces on the full blocks around (mostly), a full block made now and
      // then where a face has none, and now and then a face with nothing
      // behind it. A vine also hangs a vine below it.
      const names = key === 'minecraft:vine' ? ['up', 'north', 'east', 'south', 'west'] : ['down', 'up', 'north', 'south', 'west', 'east'];
      const lines = [];
      const faces = {};
      let any = false;
      for (const name of names) {
        const [dx, dy, dz] = DIRECTIONS[name];
        const [nx, ny, nz] = [x + dx, y + dy, z + dz];
        let held = FULL.has(at(nx, ny, nz));
        let attempt = false;
        if (!held && !drops && !at(nx, ny, nz) && ny >= 1 && full.length && r.next() < 0.15) {
          // (Now and then a block with a partial face, a slab or stairs.)
          const partial = r.next() < 0.25;
          const support = partial ? r.pick(PARTIAL) : r.pick(full);
          lines.push(put(support, nx, ny, nz, stateText(blocks, support, r, { waterlogged: 'false' })));
          held = !partial;
          attempt = partial;
        }
        faces[name] = drops ? r.next() < 0.4 : held ? r.next() < 0.75 : attempt ? r.next() < 0.7 : r.next() < 0.15;
        any ||= faces[name];
      }
      if (!any && !drops && (edit || r.next() >= 0.03)) return null;
      const given = { waterlogged: r.next() < 0.0008 ? 'true' : 'false', ...Object.fromEntries(Object.entries(faces).map(([name, value]) => [name, String(value)])) };
      lines.push(put(key, x, y, z, stateText(blocks, key, r, given)));
      const hanging = names.filter((name) => faces[name] && name !== 'up' && name !== 'down');
      if (key === 'minecraft:vine' && hanging.length && y > 1 && !at(x, y - 1, z) && r.next() < 0.4) {
        const below = Object.fromEntries(hanging.filter(() => r.next() < 0.8).map((name) => [name, 'true']));
        lines.push(put(key, x, y - 1, z, stateText(blocks, key, r, { up: 'false', north: 'false', east: 'false', south: 'false', west: 'false', ...below })));
      }
      return lines;
    }
    if (key === 'minecraft:pale_moss_carpet') {
      const below = at(x, y - 1, z);
      let bottom = 'true';
      if (!drops) {
        if (!below) {
          if (edit || r.next() >= 0.03) return null;
        } else if (below === key && paleBases.get(`${x},${y - 1},${z}`)) {
          bottom = r.next() < 0.75 ? 'false' : 'true';
        }
      } else bottom = r.next() < 0.5 ? 'true' : 'false';
      const given = { bottom };
      for (const name of ['north', 'east', 'south', 'west']) {
        const [dx, , dz] = DIRECTIONS[name];
        const held = FULL.has(at(x + dx, y, z + dz));
        given[name] = drops || r.next() < 0.15 ? r.pick(['none', 'low', 'tall']) : held ? r.pick(['low', 'low', 'tall', 'none']) : r.next() < 0.95 ? 'none' : r.pick(['low', 'tall']);
      }
      paleBases.set(`${x},${y},${z}`, bottom === 'true');
      return [put(key, x, y, z, stateText(blocks, key, r, given))];
    }
    if (VINES[key]) {
      // A column of 0 to 3 bodies behind the end (its head, mostly) and a full
      // block at its root, in free cells.
      const [step, head, body] = VINES[key];
      const length = r.int(4);
      const cells = [];
      for (let i = 0; i <= length; i++) cells.push([y - step * i, i === 0 ? (r.next() < 0.85 ? head : body) : (r.next() < 0.12 ? head : body)]);
      const rootY = y - step * (length + 1);
      if (cells.some(([cy]) => cy < 1 || at(x, cy, z)) || rootY < 0 || (at(x, rootY, z) ? !FULL.has(at(x, rootY, z)) : !full.length)) return null;
      const lines = [];
      if (!at(x, rootY, z) && r.next() < 0.88) {
        const support = r.pick(full);
        lines.push(put(support, x, rootY, z, stateText(blocks, support, r)));
      }
      for (const [cy, cell] of cells.reverse()) lines.push(put(cell, x, cy, z, stateText(blocks, cell, r)));
      return lines;
    }
    if (BAMBOO.has(key)) {
      // A stalk of up to four, the picked block at its foot, in free cells.
      const lines = [put(key, x, y, z, stateText(blocks, key, r))];
      for (let cy = y + 1, n = r.int(4); n > 0 && !at(x, cy, z); n--, cy++) lines.push(put('minecraft:bamboo', x, cy, z, stateText(blocks, 'minecraft:bamboo', r)));
      return lines;
    }
    if (TWO_HIGH.has(key)) {
      if (edit || at(x, y + 1, z)) return null;
      const lower = stateText(blocks, key, r, { half: 'lower' });
      return [put(key, x, y, z, lower), put(key, x, y + 1, z, lower.replace('half=lower', 'half=upper'))];
    }
    if (HANGING.has(key)) {
      // Hangs from the block above: a support is placed above if none is there.
      // A mangrove propagule that does not hang stands on the ground instead.
      const stone = full.length ? full : ['minecraft:stone'];
      const supports = key === 'minecraft:mangrove_propagule' ? ['minecraft:mangrove_leaves'] : key === 'minecraft:pale_hanging_moss' ? [...stone, 'minecraft:pale_oak_leaves'] : stone;
      const hangsFrom = (block) => (key === 'minecraft:mangrove_propagule' ? block === 'minecraft:mangrove_leaves' : FULL.has(block) || key === 'minecraft:pale_hanging_moss' && (block === 'minecraft:pale_oak_leaves' || block === key));
      const above = at(x, y + 1, z);
      const hanging = key !== 'minecraft:mangrove_propagule' || r.next() < 0.6;
      if (!hanging) {
        const below = at(x, y - 1, z);
        return below && STANDS_ON[key].includes(below) ? [put(key, x, y, z, stateText(blocks, key, r, { hanging: 'false' }))] : null;
      }
      if (above ? !hangsFrom(above) : edit) return null;
      const lines = [put(key, x, y, z, stateText(blocks, key, r, key === 'minecraft:mangrove_propagule' ? { hanging: 'true' } : {}))];
      if (!above) {
        const support = r.pick(supports);
        lines.push(put(support, x, y + 1, z, stateText(blocks, support, r)));
      }
      // Moss hangs in strings.
      if (key === 'minecraft:pale_hanging_moss') {
        for (let d = 1; r.next() < 0.5 && y - d >= 1 && !at(x, y - d, z); d++) lines.push(put(key, x, y - d, z, stateText(blocks, key, r)));
      }
      return lines;
    }
    if (DRIPLEAF.has(key)) {
      // A column of stems (one or more for a stem) under a leaf (now and then
      // left off), all facing alike.
      const stems = key === 'minecraft:big_dripleaf_stem' ? 1 + r.int(3) : r.int(3);
      const leaf = r.next() < 0.9;
      const given = { facing: r.pick(Object.keys(STEP)), waterlogged: 'false' };
      const column = [];
      for (let i = 0; i <= stems; i++) {
        if (at(x, y + i, z)) return null;
        if (i < stems) column.push(put('minecraft:big_dripleaf_stem', x, y + i, z, stateText(blocks, 'minecraft:big_dripleaf_stem', r, given)));
        else if (leaf) column.push(put('minecraft:big_dripleaf', x, y + i, z, stateText(blocks, 'minecraft:big_dripleaf', r, given)));
      }
      return column.length ? column : null;
    }
    if (LOG_ATTACHED.has(key)) {
      const facings = Object.keys(STEP).filter((f) => at(x + STEP[f][0], y, z + STEP[f][1]) === 'minecraft:jungle_log');
      if (!facings.length) return null;
      return [put(key, x, y, z, stateText(blocks, key, r, { facing: r.pick(facings) }))];
    }
    if (LANTERNS.has(key)) {
      const [below, above] = [at(x, y - 1, z), at(x, y + 1, z)];
      const options = [...(FULL.has(below) ? ['false'] : []), ...(FULL.has(above) ? ['true'] : [])];
      if (!options.length && (edit || r.next() >= 0.05)) return null;
      return [put(key, x, y, z, stateText(blocks, key, r, options.length ? { hanging: r.pick(options) } : {}))];
    }
    if (POINTING.has(key)) {
      const facings = Object.entries(DIRECTIONS).filter(([, [dx, dy, dz]]) => FULL.has(at(x - dx, y - dy, z - dz))).map(([facing]) => facing);
      if (!facings.length && (edit || r.next() >= 0.05)) return null;
      return [put(key, x, y, z, stateText(blocks, key, r, facings.length ? { facing: r.pick(facings) } : {}))];
    }
    if (SIDE_ATTACHED.has(key)) {
      const facings = Object.keys(STEP).filter((f) => FULL.has(at(x + STEP[OPPOSITE[f]][0], y, z + STEP[OPPOSITE[f]][1])));
      if (!facings.length) return null;
      return [put(key, x, y, z, stateText(blocks, key, r, { facing: r.pick(facings) }))];
    }
    if (CEILING_ATTACHED.has(key)) {
      const above = at(x, y + 1, z);
      if (above ? !FULL.has(above) : edit || !full.length) return null;
      const lines = [put(key, x, y, z, stateText(blocks, key, r))];
      if (!above) {
        const support = r.pick(full);
        lines.push(put(support, x, y + 1, z, stateText(blocks, support, r)));
      }
      return lines;
    }
    if (BEDS.has(key)) {
      // The foot, and the head in front of it (a pair of unrelated halves now and then).
      const facing = r.pick(Object.keys(STEP));
      const [hx, hz] = [x + STEP[facing][0], z + STEP[facing][1]];
      if (hx < 0 || hz < 0 || hx >= size || hz >= size || at(hx, y, hz)) return null;
      const foot = stateText(blocks, key, r, { part: 'foot', facing });
      const mismatched = r.next() < 0.1;
      const head = mismatched ? stateText(blocks, r.pick(FAMILIES.bed), r, { part: 'head' }) : stateText(blocks, key, r, { part: 'head', facing });
      return [put(key, x, y, z, foot), put(head.slice(0, head.indexOf('[')), hx, y, hz, head)];
    }
    const text = stateText(blocks, key, r, { ...chestPair(r, chests, key, x, y, z), ...shelfPair(r, chests, key, x, y, z), ...attachment(key, x, y, z), ...(PISTONS.has(key) && r.next() < 0.6 ? { extended: 'false' } : {}), ...(RAILS.has(key) && !drops ? { shape: railShape(key) } : {}) });
    if (PISTONS.has(key) && !edit) pistons.push([x, y, z, /facing=(\w+)/.exec(text)[1], key, /extended=true/.test(text)]);
    if (CHESTS.has(key) || SHELVES.has(key)) chests.set(`${x},${y},${z}`, { key, facing: /facing=(\w+)/.exec(text)[1] });
    const lines = [put(key, x, y, z, text)];
    // A cactus keeps (mostly) its sides free of solid blocks, which would
    // break it.
    if (key === 'minecraft:cactus' && r.next() < 0.85) {
      for (const [dx, dz] of Object.values(STEP)) if (!at(x + dx, y, z + dz)) grid.set(`${x + dx},${y},${z + dz}`, 'minecraft:air');
    }
    return lines;
  };
  // Whether an edit at a position would break a block that stands on it.
  const holds = (x, y, z) => [at(x, y, z), at(x, y + 1, z), at(x, y - 1, z), ...Object.values(STEP).map(([dx, dz]) => at(x + dx, y, z + dz))]
    .some((block) => block && (TWO_HIGH.has(block) || BEDS.has(block) || SIDE_ATTACHED.has(block) || CEILING_ATTACHED.has(block) || LOG_ATTACHED.has(block) || LANTERNS.has(block) || POINTING.has(block) || HANGING.has(block) || DRIPLEAF.has(block)));
  // The floor is stone, or ground for the plants that stand on it.
  const grounds = all.filter((key) => GROUND.has(key));
  const wantsGround = keys.some((key) => STANDS_ON[key]);
  for (let x = 0; x < size; x++) for (let z = 0; z < size; z++) {
    if (r.next() < 0.85) {
      const floor = wantsGround && grounds.length && r.next() < 0.8 ? r.pick(grounds) : 'minecraft:stone';
      // (Sand falls from the air: it lies on stone.)
      if (FAMILIES.sandy.includes(floor)) commands.push(`setblock ${x} -1 ${z} minecraft:stone`);
      commands.push(`setblock ${x} 0 ${z} ${stateText(blocks, floor, r)}`);
      grid.set(`${x},0,${z}`, floor);
    }
  }
  for (let y = 1; y <= height; y++) for (let x = 0; x < size; x++) for (let z = 0; z < size; z++) {
    if (r.next() < (flow ? 0.9 : 0.45) || at(x, y, z)) continue;
    const key = choose(x, y, z);
    if (!key) continue;
    const placed = placement(key, x, y, z, false);
    if (!placed) continue;
    commands.push(...placed);
    if (key === 'minecraft:lever') inputs.push(['toggle', x, y, z]);
    if (key.endsWith('_button')) inputs.push(['press', x, y, z]);
    // A right click on a block that takes one (both halves of a door).
    if (USED.has(key)) inputs.push(['use', x, y, z], ...(TWO_HIGH.has(key) ? [['use', x, y + 1, z]] : []));
  }
  // Pistons push what stands in front of them (a line of full blocks, with
  // gaps) and are powered by a redstone block at one of the sides a piston
  // reads power from, or above one of those (quasi-connectivity).
  const powers = families.includes('power') ? FAMILIES.power : [];
  const pistonKeys = keys.filter((key) => PISTONS.has(key));
  const powerCells = new Set();
  const neighbours = (x, y, z) => [...Object.values(DIRECTIONS), ...['north', 'south', 'west', 'east'].map((d) => [DIRECTIONS[d][0], 1, DIRECTIONS[d][2]])]
    .map(([dx, dy, dz]) => [x + dx, y + dy, z + dz]);
  const put = (x, y, z, key, given = {}) => {
    grid.set(`${x},${y},${z}`, key);
    commands.push(`setblock ${x} ${y} ${z} ${stateText(blocks, key, r, given)}`);
  };
  for (const [x, y, z, facing, piston, extended] of pistons) {
    const [dx, dy, dz] = DIRECTIONS[facing];
    // An extended piston usually has its head.
    if (extended && families.includes('head') && r.next() < 0.8 && y + dy >= 1) {
      // (A short head is one mid-motion; a still piston's head is not.)
      put(x + dx, y + dy, z + dz, 'minecraft:piston_head', { facing, type: piston === 'minecraft:piston' ? 'normal' : 'sticky', ...(r.next() < 0.9 ? { short: 'false' } : {}) });
    }
    if (full.length && r.next() < 0.7) {
      for (let n = 1 + r.int(5), d = 1; n > 0; n--, d++) {
        const [px, py, pz] = [x + dx * d, y + dy * d, z + dz * d];
        if (py < 1 || at(px, py, pz) || r.next() >= 0.85) continue;
        // Now and then the pushed block is a piston, which acts once it lands.
        if (pistonKeys.length && r.next() < 0.25) put(px, py, pz, r.pick(pistonKeys), { extended: 'false' });
        else put(px, py, pz, r.pick(full));
      }
    }
    if (powers.length && r.next() < 0.7) {
      const [px, py, pz] = r.pick(neighbours(x, y, z));
      if (py >= 1 && !at(px, py, pz)) {
        put(px, py, pz, r.pick(powers));
        powerCells.add(`${px},${py},${pz}`);
      }
    }
  }
  // Tripwire hooks at the ends of a run of tripwire, each on a wall block.
  if (families.includes('tripwire') && r.next() < 0.7) {
    const alongX = r.next() < 0.5;
    const fixed = r.int(size);
    const start = r.int(size - 1);
    const wires = 1 + r.int(Math.max(1, size - 1 - start));
    const [toward, away] = alongX ? ['east', 'west'] : ['south', 'north'];
    const put = (along, key, given) => {
      const [x, z] = alongX ? [along, fixed] : [fixed, along];
      grid.set(`${x},1,${z}`, key);
      commands.push(`setblock ${x} 1 ${z} ${stateText(blocks, key, r, given)}`);
    };
    const joined = r.next() < 0.7 ? (alongX ? { east: 'true', west: 'true', north: 'false', south: 'false' } : { north: 'true', south: 'true', east: 'false', west: 'false' }) : {};
    put(start - 1, 'minecraft:stone', {});
    put(start, 'minecraft:tripwire_hook', { facing: toward });
    for (let i = 1; i <= wires; i++) put(start + i, 'minecraft:tripwire', joined);
    put(start + wires + 1, 'minecraft:tripwire_hook', { facing: away });
    put(start + wires + 2, 'minecraft:stone', {});
  }
  // Scenarios for the special blocks: an item that falls on a plate, an ore, a
  // sensor or a bell (a torch on a stone loses its stone), a beacon over a
  // pyramid, a nether portal frame with fire or a portal in it.
  const scheduled = [];
  const vacant = (x, y, z) => !at(x, y, z);
  const specials = (names) => names.some((name) => families.includes(name));
  if (specials(['plate', 'ore', 'sculk', 'frosted', 'fire']) && drops) {
    for (let n = r.int(3); n >= 0; n--) {
      const [x, z] = [r.int(size), r.int(size)];
      if (![1, 2, 3].every((y) => vacant(x, y, z)) || !at(x, 0, z)) continue;
      const target = r.pick(keys.filter((key) => /pressure_plate|redstone_ore|sculk_sensor|frosted_ice|soul_sand|fire$/.test(key)).concat(keys));
      put(x, 1, z, target);
      put(x, 2, z, 'minecraft:stone');
      put(x, 3, z, r.pick(['minecraft:redstone_torch', 'minecraft:lever']), { face: 'floor' });
      scheduled.push({ tick: r.int(Math.max(1, ticks - 10)), commands: [`setblock ${x} 2 ${z} minecraft:air`] });
    }
  }
  if (specials(['beacon']) && r.next() < 0.9) {
    const [x, z] = [1 + r.int(Math.max(1, size - 2)), 1 + r.int(Math.max(1, size - 2))];
    const y = 1 + r.int(3);
    if (vacant(x, y, z)) {
      put(x, y, z, 'minecraft:beacon');
      const base = r.pick(['minecraft:iron_block', 'minecraft:gold_block', 'minecraft:emerald_block', 'minecraft:diamond_block', 'minecraft:netherite_block']);
      for (let layer = 1; layer <= r.int(5); layer++) {
        for (let dx = -layer; dx <= layer; dx++) for (let dz = -layer; dz <= layer; dz++) {
          if (r.next() < 0.97) { put(x + dx, y - layer, z + dz, r.next() < 0.95 ? base : 'minecraft:stone'); }
        }
      }
      let top = y + 1;
      for (let n = r.int(6); n > 0; n--, top++) {
        const above = r.pick(['minecraft:white_stained_glass', 'minecraft:red_stained_glass', 'minecraft:white_stained_glass_pane', 'minecraft:blue_stained_glass', 'minecraft:air', 'minecraft:bedrock', 'minecraft:stone', 'minecraft:glass']);
        put(x, top, z, above);
      }
      // The beam gets built, then something changes.
      if (r.next() < 0.7) scheduled.push({ tick: r.int(Math.max(1, ticks - 5)), commands: [`setblock ${x} ${top} ${z} ${r.pick(['minecraft:stone', 'minecraft:white_stained_glass', 'minecraft:air'])}`] });
      if (r.next() < 0.5) scheduled.push({ tick: r.int(Math.max(1, ticks - 5)), commands: [`setblock ${x + r.int(3) - 1} ${y - 1} ${z} minecraft:air`] });
    }
  }
  if (specials(['portal']) && r.next() < 0.9) {
    const alongX = r.next() < 0.5;
    const [width, height] = [2 + r.int(3), 3 + r.int(3)];
    const [x0, z0] = [r.int(Math.max(1, size - width - 1)) + 1, r.int(Math.max(1, size - width - 1)) + 1];
    const y0 = 1;
    const cell = (i, j) => (alongX ? [x0 + i, y0 + j, z0] : [x0, y0 + j, z0 + i]);
    const frame = [];
    for (let i = -1; i <= width; i++) for (let j = -1; j <= height; j++) {
      const border = i === -1 || i === width || j === -1 || j === height;
      const corner = (i === -1 || i === width) && (j === -1 || j === height);
      if (border && !(corner && r.next() < 0.5) && r.next() < 0.97) frame.push(cell(i, j));
    }
    for (const [x, y, z] of frame) { if (vacant(x, y, z) && y >= 1) put(x, y, z, 'minecraft:obsidian'); }
    const lit = cell(r.int(width), 0);
    if (r.next() < 0.6) {
      scheduled.push({ tick: r.int(Math.max(1, ticks - 5)), commands: [`setblock ${lit[0]} ${lit[1]} ${lit[2]} minecraft:fire`] });
    } else {
      for (let i = 0; i < width; i++) for (let j = 0; j < height; j++) {
        const [x, y, z] = cell(i, j);
        if (vacant(x, y, z)) put(x, y, z, 'minecraft:nether_portal', { axis: alongX ? 'x' : 'z' });
      }
    }
    // The frame loses a block.
    if (r.next() < 0.5) {
      const [x, y, z] = r.pick(frame);
      scheduled.push({ tick: r.int(Math.max(1, ticks - 5)), commands: [`setblock ${x} ${y} ${z} minecraft:air`] });
    }
  }
  // Runs of rail that turn, climb and fall, each cell on a full block, with
  // redstone blocks beside some of them (they power the powered rails and the
  // rails beside them, and the detector rails are read by what is around them).
  const railKeys = keys.filter((key) => RAILS.has(key));
  const railPowers = new Set();
  if (railKeys.length) {
    const toward = { north: [0, -1], south: [0, 1], west: [-1, 0], east: [1, 0] };
    for (let run = 1 + r.int(3); run > 0; run--) {
      let [x, y, z] = [r.int(size), 1 + (r.next() < 0.3 ? r.int(height) : 0), r.int(size)];
      let dir = r.pick(Object.keys(toward));
      const kind = r.pick(railKeys);
      for (let n = 3 + r.int(10); n > 0; n--) {
        if (x < 0 || z < 0 || x >= size || z >= size) break;
        if (!at(x, y, z) && (at(x, y - 1, z) || y - 1 >= 1)) {
          if (!at(x, y - 1, z)) put(x, y - 1, z, 'minecraft:stone');
          if (FULL.has(at(x, y - 1, z)) || drops) {
            const key = r.next() < 0.85 ? kind : r.pick(railKeys);
            put(x, y, z, key, { shape: railShape(key) });
            if (powers.length && r.next() < 0.12) {
              const [px, py, pz] = r.pick(neighbours(x, y, z));
              if (py >= 1 && !at(px, py, pz)) {
                put(px, py, pz, r.pick(powers));
                railPowers.add(`${px},${py},${pz}`);
              }
            }
          }
        }
        const roll = r.next();
        if (roll < 0.15) dir = r.pick(Object.keys(toward));
        x += toward[dir][0];
        z += toward[dir][1];
        // The next cell is a step up (on a block beside this one) or down.
        if (roll > 0.85 && y < 1 + height) {
          if (!at(x, y, z)) put(x, y, z, 'minecraft:stone');
          y++;
        } else if (roll > 0.75 && y > 1) y--;
      }
    }
  }
  // A knot of rails (junctions, crossings and loops: the connection logic's
  // hard cases) on a floor of stone.
  if (railKeys.length && r.next() < 0.5) {
    const [cx, cz] = [r.int(size), r.int(size)];
    for (let x = cx - 1; x <= cx + 1; x++) for (let z = cz - 1; z <= cz + 1; z++) {
      if (x < 0 || z < 0 || x >= size || z >= size || at(x, 1, z) || !at(x, 0, z) || r.next() < 0.4) continue;
      const key = r.pick(railKeys);
      put(x, 1, z, key, { shape: railShape(key) });
    }
  }
  // A row of shelves facing one way, side by side, so chains form.
  if (families.includes('shelf') && r.next() < 0.8) {
    const facing = r.pick(Object.keys(STEP));
    const along = facing === 'north' || facing === 'south' ? [1, 0] : [0, 1];
    const [sx, sz] = [r.int(size), r.int(size)];
    const key = r.pick(FAMILIES.shelf);
    for (let i = 0, n = 2 + r.int(4); i < n; i++) {
      const [x, z] = [sx + along[0] * i, sz + along[1] * i];
      if (x >= size || z >= size) break;
      put(x, 1, z, r.next() < 0.9 ? key : r.pick(FAMILIES.shelf), { facing, ...(r.next() < 0.85 ? { powered: 'true' } : {}) });
    }
  }
  // Redstone blocks come and go beside shelves and skulls.
  const powered = [...grid.entries()].filter(([, key]) => SHELVES.has(key) || FAMILIES.skull.includes(key)).map(([cell]) => cell.split(',').map(Number));
  const toggled = new Set();
  const feeds = [];
  if (drops && families.includes('hopper')) buildRigs({ blocks, families, keys, r, size, height, grid, commands, inputs, feeds, flow });
  const steps = [{ place: commands, fresh: true }];
  for (let t = 0; t < ticks; t++) {
    if (powered.length && powers.length && r.next() < 0.1) {
      const [x, y, z] = r.pick(powered);
      const [px, py, pz] = r.pick(neighbours(x, y, z));
      const cell = `${px},${py},${pz}`;
      if (py >= 1 && !at(px, py, pz)) {
        grid.set(cell, r.pick(powers));
        toggled.add(cell);
        steps.push({ place: [`setblock ${px} ${py} ${pz} ${grid.get(cell)}`], fresh: false });
      } else if (toggled.has(cell) && at(px, py, pz) && !at(px, py + 1, pz)) {
        grid.delete(cell);
        steps.push({ place: [`setblock ${px} ${py} ${pz} minecraft:air`], fresh: false });
      }
    }
    for (const entry of scheduled) if (entry.tick === t) steps.push({ place: entry.commands, fresh: false });
    // A redstone block beside a rail comes or goes.
    if (railPowers.size && powers.length && r.next() < 0.1) {
      const cell = r.pick([...railPowers]);
      const [px, py, pz] = cell.split(',').map(Number);
      if (at(px, py, pz) && !at(px, py + 1, pz)) {
        grid.delete(cell);
        steps.push({ place: [`setblock ${px} ${py} ${pz} minecraft:air`], fresh: false });
      } else if (!at(px, py, pz)) {
        grid.set(cell, r.pick(powers));
        steps.push({ place: [`setblock ${px} ${py} ${pz} ${grid.get(cell)}`], fresh: false });
      }
    }
    if (inputs.length && r.next() < 0.15) {
      const [kind, ...cell] = r.pick(inputs);
      steps.push({ input: [kind, ...cell, kind === 'use' ? yawOf(r) : 0] });
    }
    // Blocks breaking above a rig's hopper: items for it.
    if (feeds.length && r.next() < (flow ? 0.25 : 0.08)) {
      const [x, y, z, feed] = r.pick(feeds);
      const fragile = keys.filter((key) => FRAGILE.has(key));
      if (fragile.length) {
        const key = feed || r.pick(fragile);
        const placed = placement(key, x, y, z, true);
        if (placed) steps.push({ place: placed, fresh: false });
      }
    }
    // --blast: TNT primed near a piston (a redstone block placed on it), which
    // explodes 80 ticks later, about when a redstone block beside the piston
    // comes or goes (so it moves as the TNT explodes).
    if (BLAST && pistons.length && r.next() < 0.06) {
      const [x, y, z] = r.pick(pistons);
      const [tx, ty, tz] = [x + r.int(5) - 2, y + r.int(3) - 1, z + r.int(5) - 2];
      if (ty >= 1 && !at(tx, ty, tz) && !at(tx, ty + 1, tz)) {
        grid.set(`${tx},${ty + 1},${tz}`, 'minecraft:redstone_block');
        steps.push({ place: [`setblock ${tx} ${ty} ${tz} minecraft:tnt[unstable=false]`, `setblock ${tx} ${ty + 1} ${tz} minecraft:redstone_block`], fresh: false });
        const [px, py, pz] = r.pick(neighbours(x, y, z));
        if (py >= 1) scheduled.push({ tick: t + 77 + r.int(5), commands: [`setblock ${px} ${py} ${pz} ${r.next() < 0.5 ? 'minecraft:redstone_block' : 'minecraft:air'}`] });
      }
    }
    // A redstone block beside a piston comes or goes (often with --blast, so
    // that the pistons are moving when the TNT explodes).
    if (pistons.length && powers.length && r.next() < (BLAST ? 0.5 : 0.08)) {
      const [x, y, z] = r.pick(pistons);
      const [px, py, pz] = r.pick(neighbours(x, y, z));
      const cell = `${px},${py},${pz}`;
      if (py >= 1 && !at(px, py, pz)) {
        grid.set(cell, r.pick(powers));
        powerCells.add(cell);
        steps.push({ place: [`setblock ${px} ${py} ${pz} ${grid.get(cell)}`], fresh: false });
      } else if (powerCells.has(cell) && at(px, py, pz) && !at(px, py + 1, pz)) {
        grid.delete(cell);
        steps.push({ place: [`setblock ${px} ${py} ${pz} minecraft:air`], fresh: false });
      }
    }
    if (r.next() < 0.05) {
      const edit = [];
      for (let n = 1 + r.int(3); n > 0; n--) {
        const [x, y, z] = [r.int(size), 1 + r.int(height), r.int(size)];
        if (holds(x, y, z)) continue;
        const above = at(x, y + 1, z);
        let key;
        if (drops) key = r.next() < 0.5 ? null : r.pick(keys);
        else if (r.next() < 0.3 && (!above || r.next() < 0.05)) key = null;
        else key = above && !FULL.has(above) && r.next() >= 0.05 ? (full.length ? r.pick(full) : null) : choose(x, y, z);
        if (!drops && key === null && above && !FULL.has(above)) continue;
        const placed = key ? placement(key, x, y, z, true) : [];
        if (!placed) continue;
        if (!key) grid.delete(`${x},${y},${z}`);
        edit.push(...(key ? placed : [`setblock ${x} ${y} ${z} minecraft:air`]));
      }
      if (edit.length) steps.push({ place: edit, fresh: false });
    }
    // A falling block placed high above the world, which lands at speed.
    if (falling.length && r.next() < 0.02) {
      const key = r.pick(falling);
      steps.push({ place: [`setblock ${r.int(size)} ${height + 2 + r.int(40)} ${r.int(size)} ${stateText(blocks, key, r)}`], fresh: false });
    }
    steps.push({ tick: 1 });
  }
  // --shift moves the world by a random offset (negative coordinates, chunk
  // borders, other sections) for the light engine, whose chunks and sections
  // the unshifted worlds do not cross.
  if (shifted) {
    const [dx, dy, dz] = [r.int(41) - 20, r.int(3) === 0 ? r.int(300) - 100 : r.int(40) - 10, r.int(41) - 20];
    const move = (command) => command.replace(/^setblock (-?\d+) (-?\d+) (-?\d+)/, (_, x, y, z) => `setblock ${Number(x) + dx} ${Number(y) + dy} ${Number(z) + dz}`);
    for (const step of steps) {
      if (step.place) step.place = step.place.map(move);
      if (step.input) step.input = [step.input[0], step.input[1] + dx, step.input[2] + dy, step.input[3] + dz, step.input[4]];
    }
  }
  // --batch N runs up to N ticks in one request where nothing happens in
  // between (fewer requests, so fewer comparisons).
  const batch = Number(option('batch', 1));
  if (batch > 1) {
    const merged = [];
    for (const step of steps) {
      const last = merged[merged.length - 1];
      if (step.tick && last?.tick && last.tick < batch) last.tick += step.tick;
      else merged.push({ ...step });
    }
    return { name: `fuzz-${seed}`, steps: merged };
  }
  return { name: `fuzz-${seed}`, steps };
}

function communityCases(ticks) {
  const builds = corpusArtifacts();
  return builds.sort(([a], [b]) => a.localeCompare(b)).map(([name, file]) => ({
    name,
    load() {
      const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
      const world = new World();
      for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
      world.finalize();
      const commands = importCommands(world, null, Y_OFFSET).commands;
      const at = (block) => [block.pos[0], block.pos[1] + Y_OFFSET, block.pos[2]];
      const steps = [{ place: commands, fresh: true }];
      for (let t = 1; t <= ticks; t++) {
        for (const block of artifact.blocks) {
          if (block.id === 'minecraft:lever' && (t === 2 || t === Math.floor(ticks / 2))) steps.push({ input: ['toggle', ...at(block), 0] });
          if (block.id.endsWith('_button') && t === 4) steps.push({ input: ['press', ...at(block), 0] });
        }
        steps.push({ tick: 1 });
      }
      return { name, steps };
    },
  }));
}

// Runs a case's steps on one engine: each step's outcome and the bytes of
// what it wrote.
async function run(engine, folder, steps) {
  const outcomes = [];
  for (const [index, step] of steps.entries()) {
    const file = (name) => path.join(folder, `${index}-${name}`);
    for (const name of fs.readdirSync(folder)) fs.rmSync(path.join(folder, name));
    let outcome;
    try {
      if (step.place) {
        fs.writeFileSync(file('commands.txt'), `${step.place.join('\n')}\n`);
        await engine.place(file('commands.txt'), file('blueprint.bin'), { fresh: step.fresh });
        outcome = { ok: 'placed', files: { blueprint: fs.readFileSync(file('blueprint.bin')) } };
      } else if (step.tick) {
        await engine.tick(step.tick, file('patch.bin'), file('entities.json'));
        outcome = { ok: 'ticked', files: { patch: fs.readFileSync(file('patch.bin')), entities: fs.readFileSync(file('entities.json')) } };
      } else if (step.spawn) {
        // The Rust engine's creative player (core/src/player).
        await engine.spawnPlayer(...step.spawn);
        outcome = { ok: 'spawned', files: {} };
      } else if (step.player) {
        const [keys, yaw, pitch] = step.player;
        await engine.playerTick(keys, yaw, pitch, file('patch.bin'), file('entities.json'), file('view.bin'));
        outcome = { ok: 'ticked', files: { patch: fs.readFileSync(file('patch.bin')), entities: fs.readFileSync(file('entities.json')), view: fs.readFileSync(file('view.bin')) } };
      } else {
        await engine.input(...step.input);
        outcome = { ok: 'input', files: {} };
      }
    } catch (error) {
      const reason = error.message.replace(/^The engine failed: /, '');
      outcome = { failed: reason };
      outcomes.push(outcome);
      // A refusal ends the case for the Rust engine.
      if (reason.startsWith('unported:')) break;
      continue;
    }
    // What the world holds besides its blocks (McbenchEngine.state): the
    // random, scheduled ticks, block events, block entities, tickers.
    await engine.state(file('state.txt'));
    outcome.files.state = fs.readFileSync(file('state.txt'));
    outcomes.push(outcome);
  }
  return outcomes;
}

// Where the Rust engine's outcomes first differ from the Java engine's, or
// that it refused, or null if they match.
function compare(java, rust, steps) {
  for (let i = 0; i < rust.length; i++) {
    const [j, r] = [java[i], rust[i]];
    const what = steps[i].place ? 'place' : steps[i].tick ? 'tick' : 'input';
    if (r.failed?.startsWith('unported:')) {
      // Against an earlier build of itself, a refusal is a difference unless that build refused the same way.
      if (option('base', null) !== null && j.failed !== r.failed) return { step: i, what, difference: `base ${j.failed ?? j.ok}, new ${r.failed}` };
      return { refused: r.failed.slice('unported: '.length), step: i };
    }
    if (r.failed?.startsWith('mcbench-core failed')) return { step: i, what, difference: r.failed };
    if (!!j.failed !== !!r.failed) return { step: i, what, difference: `java ${j.failed ? `failed: ${j.failed.slice(0, 1500)}` : j.ok}, rust ${r.failed ? `failed: ${r.failed.slice(0, 1500)}` : r.ok}` };
    if (j.failed) continue;
    if (j.ok !== r.ok) return { step: i, what, difference: `java ${j.ok}, rust ${r.ok}` };
    for (const name of Object.keys(j.files)) {
      if (!j.files[name].equals(r.files[name])) return { step: i, what, difference: `${name} differs (java ${j.files[name].length} bytes, rust ${r.files[name].length})`, file: name };
    }
  }
  return null;
}

async function runCase(engines, steps) {
  const [java, rust] = await Promise.all([run(engines.java, engines.javaFolder, steps), run(engines.rust, engines.rustFolder, steps)]);
  return { java, rust, result: compare(java, rust, steps) };
}

// Drops commands and steps while the case still mismatches (not refuses).
async function shrink(engines, steps) {
  const fails = async (candidate) => {
    const { result } = await runCase(engines, candidate);
    return result && !result.refused;
  };
  let current = steps;
  for (let progress = true; progress;) {
    progress = false;
    for (let i = current.length - 1; i >= 1; i--) {
      const candidate = current.filter((_, at) => at !== i);
      if (await fails(candidate)) { current = candidate; progress = true; }
    }
    for (let i = 0; i < current.length; i++) {
      if (!current[i].place) continue;
      for (let c = current[i].place.length - 1; c >= 0; c--) {
        if (current[i].place.length === 1) break;
        const candidate = current.map((step, at) => (at === i ? { ...step, place: step.place.filter((_, k) => k !== c) } : step));
        if (await fails(candidate)) { current = candidate; progress = true; }
      }
    }
  }
  return current;
}

// The second engine: the Rust engine, or with --stock the Java engine on
// Paper's own code paths.
function secondEngine(classpath, scratch, native) {
  if (!process.argv.includes('--stock')) return rustEngine();
  const env = engineEnv('1');
  if (native) return new Engine({ spawnEngine: (binary, args, options) => spawn(binary, args, { ...options, env }) });
  fs.mkdirSync(path.join(scratch, 'classes-stock'));
  return jvmEngine({ classpath, classes: path.join(scratch, 'classes-stock'), env });
}

// The first engine: the Java engine (on the JVM, or the native program with --native).
function firstEngine(classpath, scratch, native) {
  if (option('base', null) !== null) return new Engine({ build: async () => path.resolve(option('base')) });
  const env = engineEnv(option('first', process.argv.includes('--check') ? 'check' : '0'));
  if (native) return new Engine({ spawnEngine: (binary, args, options) => spawn(binary, args, { ...options, env }) });
  fs.mkdirSync(path.join(scratch, 'classes'));
  return jvmEngine({ classpath, classes: path.join(scratch, 'classes'), env });
}

// The Rust engine: core/ built, or the binary --rust names (an earlier
// commit's build, for what it refused).
function rustEngine() {
  if (option('rust', null) !== null) {
    const binary = path.resolve(option('rust'));
    return new Engine({ build: async () => binary });
  }
  execFileSync(rust().cargo, coreBuildArgs(), { cwd: path.join(ROOT, 'core'), env: rust().env, stdio: 'inherit' });
  const binary = path.join(ROOT, 'core', 'target', 'release', 'mcbench-core');
  return new Engine({ build: async () => binary });
}

async function main() {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-core-parity-'));
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const native = process.argv.includes('--native');
  const java = firstEngine(classpath, scratch, native);
  const rust = secondEngine(classpath, scratch, native);
  const engines = { java, rust, javaFolder: path.join(scratch, 'java-out'), rustFolder: path.join(scratch, 'rust-out') };
  fs.mkdirSync(engines.javaFolder);
  fs.mkdirSync(engines.rustFolder);
  const binaries = path.join(scratch, 'binaries');
  await Promise.all([
    java.warm({ classpath, binaries, folder: path.join(scratch, 'java'), minecraftVersion: minecraftLive.minecraftVersion }),
    rust.warm({ classpath, binaries, folder: path.join(scratch, 'rust'), minecraftVersion: minecraftLive.minecraftVersion }),
  ]);
  const keep = option('keep', null);
  const ticks = Number(option('ticks', 100));
  let cases;
  if (option('steps', null) !== null) {
    cases = [{ name: 'steps', load: () => ({ name: 'steps', steps: JSON.parse(fs.readFileSync(option('steps'), 'utf8')) }) }];
  } else if (option('case', null) !== null) {
    // A case's steps from a file (a kept steps.json, or one written by hand).
    cases = option('case').split(',').map((file) => ({ name: path.basename(file), load: () => ({ name: path.basename(file), steps: JSON.parse(fs.readFileSync(file, 'utf8')) }) }));
  } else if (process.argv.includes('--use')) {
    cases = useCases().map(({ name, steps }) => ({ name, load: () => ({ name, steps }) }));
  } else if (option('orbs', null) !== null) {
    cases = require('./orb-cases').orbCases(Number(option('orbs')), Number(option('seed', 1)), random).map(({ name, steps }) => ({ name, load: () => ({ name, steps }) }));
  } else if (option('fuzz', null) !== null) {
    const blocks = registry();
    const families = option('families', Object.keys(FAMILIES).join(',')).split(',');
    const seed = Number(option('seed', 1));
    cases = Array.from({ length: Number(option('fuzz')) }, (_, i) => ({ name: `fuzz-${seed + i}`, load: () => fuzzCase(blocks, families, seed + i, Number(option('ticks', 40)), process.argv.includes('--drops'), process.argv.includes('--flow'), process.argv.includes('--shift')) }));
  } else {
    cases = communityCases(ticks).filter(({ name }) => new RegExp(option('match', '')).test(name)).slice(0, Number(option('limit', Infinity)));
  }
  const totals = { matched: 0, refused: 0, mismatched: 0 };
  const refusals = new Map();
  // Cases in which a piston moved blocks (a moving block entity existed).
  let pushed = 0;
  for (const entry of cases) {
    const { name, steps } = entry.load();
    if (process.argv.includes('--show')) console.log(steps.map((step) => JSON.stringify(step)).join('\n'));
    const { result, java, rust: rustOutcomes } = await runCase(engines, steps);
    const javaOutcomes = java;
    if (process.argv.includes('--trace')) java.forEach((o, i) => console.log(`  step ${i} ${JSON.stringify(steps[i]).slice(0, 80)}: ${o.ok || o.failed.slice(0, 160)} ${o.files?.entities ? o.files.entities.toString().slice(0, 200) : ''}`));
    if (javaOutcomes.some((outcome) => outcome.files?.state?.includes(' minecraft:piston {'))) pushed++;
    // A case from a file keeps every step's files of both engines.
    if (option('case', null) !== null && keep) {
      for (const [side, outcomes] of [['java', javaOutcomes], ['rust', rustOutcomes]]) {
        fs.mkdirSync(path.join(keep, name, side), { recursive: true });
        for (const [index, outcome] of outcomes.entries()) {
          for (const [file, bytes] of Object.entries(outcome.files || {})) fs.writeFileSync(path.join(keep, name, side, `${index}-${file}`), bytes);
        }
      }
    }
    if (!result) {
      totals.matched++;
      // The right clicks the engines took, and those a block did not take (the same on both).
      steps.forEach((step, i) => {
        if (step.input?.[0] !== 'use') return;
        totals[java[i].ok ? 'used' : 'unused'] = (totals[java[i].ok ? 'used' : 'unused'] || 0) + 1;
      });
      const items = Math.max(0, ...java.map((o) => (o.files?.entities ? (o.files.entities.toString().match(/"id"/g) || []).length : 0)));
      totals.items = (totals.items || 0) + items;
      // The most of each entity type any frame held, and across the case's frames.
      const types = {};
      for (const o of java) {
        const here = {};
        for (const [, type] of (o.files?.entities?.toString() || '').matchAll(/"type":"minecraft:(\w+)"/g)) here[type] = (here[type] || 0) + 1;
        for (const [type, count] of Object.entries(here)) types[type] = Math.max(types[type] || 0, count);
      }
      for (const [type, count] of Object.entries(types)) totals[type] = (totals[type] || 0) + count;
      // Primed TNT that left the world between frames: it exploded.
      let tntBefore = new Set();
      let exploded = 0;
      for (const o of java) {
        if (!o.files?.entities) continue;
        const now = new Set(JSON.parse(o.files.entities.toString()).filter((entity) => entity.type === 'minecraft:tnt').map((entity) => entity.id));
        for (const id of tntBefore) if (!now.has(id)) exploded++;
        tntBefore = now;
      }
      totals.explosions = (totals.explosions || 0) + exploded;
      const kinds = [...Object.entries(types).filter(([type]) => type !== 'item').map(([type, count]) => `${type} ${count}`), ...(exploded ? [`${exploded} explosions`] : [])].join(', ');
      // How often the containers' contents changed (the item stacks in the block entities' saved data).
      let changes = 0;
      let before = '';
      for (const outcome of java) {
        const contents = (outcome.files?.state?.toString() || '').split('\n').filter((line) => line.startsWith('blockentity') && line.includes('Slot:')).join('\n');
        if (contents !== before) changes++;
        before = contents;
      }
      if (changes) totals.stocked = (totals.stocked || 0) + 1;
      totals.changes = (totals.changes || 0) + changes;
      console.log(`match     ${name} (${steps.length} steps, up to ${items} entities${kinds ? `: ${kinds}` : ''}, ${changes} content changes)`);
    } else if (result.refused) {
      totals.refused++;
      const reason = result.refused.replace(/ at -?\d+, -?\d+, -?\d+/, '').replace(/\(.*\)$/, '').trim();
      refusals.set(reason, (refusals.get(reason) || 0) + 1);
      console.log(`refused   ${name} at step ${result.step}: ${result.refused}`);
    } else {
      totals.mismatched++;
      console.log(`MISMATCH  ${name} at step ${result.step} (${result.what}): ${result.difference}`);
      if (keep) {
        const folder = path.join(keep, name);
        fs.mkdirSync(folder, { recursive: true });
        if (process.argv.includes('--verbose')) for (let i = 0; i <= result.step; i++) console.log(`  step ${i}: java ${java[i]?.ok || java[i]?.failed?.slice(0, 200)} | rust ${rustOutcomes[i]?.ok || rustOutcomes[i]?.failed?.slice(0, 200)}`);
        // What the engines wrote the first time the case ran.
        for (const [side, outcomes] of [['java', java], ['rust', rustOutcomes]]) {
          for (const [file, bytes] of Object.entries(outcomes[result.step]?.files || {})) fs.writeFileSync(path.join(folder, `first-${side}-${result.step}-${file}`), bytes);
        }
        const smallest = name.startsWith('fuzz-') ? await shrink(engines, steps) : steps;
        fs.writeFileSync(path.join(folder, 'steps.json'), JSON.stringify(smallest, null, 1));
        const { java: j, rust: r, result: shrunk } = await runCase(engines, smallest);
        fs.writeFileSync(path.join(folder, 'result.json'), JSON.stringify(shrunk, null, 1));
        const at = shrunk?.step ?? result.step;
        for (const [side, outcomes] of [['java', j], ['rust', r]]) {
          for (const [file, bytes] of Object.entries(outcomes[at]?.files || {})) fs.writeFileSync(path.join(folder, `${side}-${at}-${file}`), bytes);
        }
        console.log(`          kept ${folder} (${smallest.length} steps, ${smallest.reduce((n, s) => n + (s.place?.length || 0), 0)} commands)`);
      }
    }
  }
  console.log(JSON.stringify(totals));
  console.log(`  ${pushed}\tcases moved blocks with pistons`);
  for (const [reason, count] of [...refusals].sort((a, b) => b[1] - a[1])) console.log(`  ${count}\t${reason}`);
  await Promise.all([java.stop(), rust.stop()]);
  fs.rmSync(scratch, { recursive: true, force: true });
  // Against an earlier Rust build a refusal counts only when both refused the
  // same way (compare); against the Java engine only the Rust engine refuses.
  // A case that throws ends the run, so none is counted as errored.
  console.log(`${totals.matched} matched, ${totals.mismatched} differed, ${option('base', null) !== null ? `${totals.refused} refused by both, 0 refused by the Rust engine alone` : `0 refused by both, ${totals.refused} refused by the Rust engine alone`}, 0 errored`);
  // A run that compared nothing proves nothing.
  if (totals.mismatched || !totals.matched) process.exitCode = 1;
}

// scripts/core-verify-parity.js builds its random experiments from the
// fuzzer's worlds, and scripts/wasm-parity.js runs the same cases.
module.exports = { FAMILIES, USED, yawOf, fuzzCase, registry, random, communityCases, useCases, run, option };

// --verify compares the engines' VERIFY instead (scripts/core-verify-parity.js).
if (require.main === module) {
  (process.argv.includes('--verify') ? require('./core-verify-parity').main() : main()).catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
