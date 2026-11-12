'use strict';

// Worlds and requests that exercise the verifier's imports, inputs and
// censuses, which the community builds never do: block_entities,
// scheduled_ticks and block_data_version imports, item `stack` data, lectern
// book and page inputs, use inputs on every block that takes one, item frame,
// boat, cart and passenger entity imports, living and item census, inventory
// observation. Each fixture is one (world, request, placement mode) case
// taken from the Paper integration tests (*.integration.test.js) or
// written here (use-input/); scripts/ticker-parity.js
// --fixtures runs them through the engine and through Paper. Fixtures are
// named <test file>/<case>. Cases the tests run under both sprint and stock
// pacing appear once, and cases that expect the verifier to reject the
// request are left out. placementMode, when set, replaces --placement.

const { World } = require('../server/world');
const { corpusArtifact } = require('./corpus');

const sequence = (count) => Array.from({ length: count }, (_, index) => index);
const placeBlocks = (world, blocks) => {
  for (const block of blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states);
  world.finalize();
  return world;
};

// ---- block-version

const blockVersionWorld = (id) => () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.set(0, 1, 0, id, { axis: 'x', waterlogged: 'false' });
  world.finalize();
  return world;
};
const blockVersionRequest = { probes: [[0, 0, 0], [0, 1, 0]], samples: sequence(21), settle_ticks: 1 };

// ---- custom-item-supply

const customItemWorld = () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:barrel');
  world.finalize();
  return world;
};
const putStack = (tick, slot, item, snbt, data_version = 4556) => ({ tick, type: 'insert', at: [0, 0, 0], slot, item: `minecraft:${item}`, count: 1, stack: { data_version, snbt } });
const customItemRequest = {
  probes: [[0, 0, 0]], inventory_probes: [[0, 0, 0]], settle_ticks: 1, item_entities: true, samples: sequence(11),
  actions: [
    putStack(1, 0, 'potion', '{id:"minecraft:potion",Count:1b,tag:{Potion:"minecraft:strong_healing"}}', 3465),
    putStack(2, 1, 'enchanted_book', '{id:"minecraft:enchanted_book",Count:1b,tag:{StoredEnchantments:[{id:"minecraft:sharpness",lvl:5s}]}}', 3465),
    putStack(3, 2, 'diamond', '{id:"minecraft:diamond",count:1,components:{"minecraft:custom_name":"Token A"}}'),
    putStack(4, 2, 'diamond', '{id:"minecraft:diamond",count:1,components:{"minecraft:custom_name":"Token B"}}'),
    putStack(5, 3, 'diamond', '{id:"minecraft:emerald",count:1}'),
    putStack(6, 3, 'diamond', '{id:"minecraft:diamond",count:2}'),
    putStack(7, 3, 'potion', '{id:"minecraft:potion",count:1,components:{"minecraft:potion_contents":{potion:"minecraft:not_a_potion"}}}'),
    putStack(8, 3, 'diamond', '{id:"minecraft:diamond",count:1}', 999999),
    { tick: 9, type: 'drop', at: [0.5, 3, 0.5], velocity: [0, 0, 0], item: 'minecraft:potion', count: 1, stack: { data_version: 4556, snbt: '{id:"minecraft:potion",count:1,components:{"minecraft:potion_contents":{potion:"minecraft:water"}}}' } },
  ],
};

// ---- dynamic-inventory

const dynamicInventoryWorld = () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:piston', { facing: 'east', extended: 'false' });
  world.set(-1, 1, 0, 'minecraft:lever', { face: 'wall', facing: 'west', powered: 'false' });
  world.set(1, 0, 0, 'minecraft:stone');
  world.set(1, 1, 0, 'minecraft:shulker_box', { facing: 'up' });
  world.set(4, 0, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.finalize();
  return world;
};
const dynamicInventoryBase = {
  probes: [[0, 1, 0], [1, 1, 0], [4, 0, 0]], samples: sequence(61), settle_ticks: 1,
  actions: [{ tick: 10, type: 'toggle', at: [-1, 1, 0] }],
  block_entities: [{ at: [1, 1, 0], data_version: 4556, snbt: '{id:"minecraft:shulker_box",Items:[{Slot:0b,id:"minecraft:stone",count:7}]}' }],
};

// ---- entity-import, entity-inventory and frame-import
// Two storage catalog builds of the corpus (scripts/corpus.js) with their
// entities and inventories. These five fixtures are listed only while
// MCBENCH_CORPUS names a corpus.

function corpusFixtures() {
  const NT003 = corpusArtifact('storage-catalog-NT003-dual-collector');
  const entityImportWorld = () => placeBlocks(new World(), NT003.blocks);
  const entityImportBase = { probes: NT003.blocks.map((block) => block.pos), samples: [0, 1, 5, 20], settle_ticks: 1, entities: NT003.entities };
  const importedItem = { at: [6.5, 5, 2.5], data_version: 4556, snbt: '{id:"minecraft:item",UUID:[I;1,2,3,4],Pos:[6.5d,5.0d,2.5d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],Item:{id:"minecraft:diamond",count:7},Age:0s,PickupDelay:32767s,Health:5s}' };

  const US009 = corpusArtifact('storage-catalog-US009');
  const entityInventoryWorld = () => {
    const world = new World();
    for (const block of US009.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states);
    world.set(20, 0, 0, 'minecraft:stone');
    world.set(20, 1, 0, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' });
    world.set(24, 0, 0, 'minecraft:stone');
    world.finalize();
    return world;
  };
  const cart = { at: [20.5, 1.0625, 0.5], data_version: 4556, snbt: '{id:"minecraft:chest_minecart",UUID:[I;13,14,15,16],Pos:[20.5d,1.0625d,0.5d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],Items:[{Slot:3b,id:"minecraft:diamond",count:7},{Slot:4b,id:"minecraft:bundle",count:1,components:{"minecraft:bundle_contents":[{id:"minecraft:diamond",count:2}]}}]}' };
  const boat = { at: [24.5, 1, 0.5], data_version: 4556, snbt: '{id:"minecraft:oak_boat",UUID:[I;23,24,25,26],Pos:[24.5d,1.0d,0.5d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f]}' };
  const CONTAINERS = new Set(['minecraft:hopper', 'minecraft:dropper', 'minecraft:dispenser', 'minecraft:chest', 'minecraft:jukebox']);
  const containerProbes = US009.blocks.filter((block) => CONTAINERS.has(block.id)).map((block) => block.pos);
  const entityInventoryRequest = {
    probes: containerProbes, inventory_probes: containerProbes, entities: [...US009.entities, cart, boat], block_entities: US009.block_entities,
    item_entities: true, settle_ticks: 200, samples: sequence(151),
    actions: [{ tick: 3, type: 'insert', at: [2, 7, 2], slot: 0, item: 'minecraft:music_disc_13', count: 1 }],
  };
  return [
    { name: 'entity-import/source-boat', world: entityImportWorld, args: entityImportBase },
    { name: 'entity-import/native-item', world: entityImportWorld, args: { ...entityImportBase, entities: [...NT003.entities, importedItem], item_entities: true } },
    { name: 'entity-inventory/observed', world: entityInventoryWorld, args: { ...entityInventoryRequest, world_inventories: true }, placementMode: 'strict' },
    { name: 'entity-inventory/baseline', world: entityInventoryWorld, args: { ...entityInventoryRequest, world_inventories: false }, placementMode: 'strict' },
    {
      name: 'frame-import/source', world: () => placeBlocks(new World(), US009.blocks), placementMode: 'strict',
      args: { probes: US009.blocks.map((block) => block.pos), block_entities: US009.block_entities, entities: US009.entities, item_entities: true, settle_ticks: 200, samples: [0, 100, 200, 400] },
    },
  ];
}

// ---- use-input
// A player's right click with an empty hand on every block that takes one:
// doors (wood, copper, iron, which cannot be opened by hand), trapdoors, fence
// gates facing each way used with yaws on both sides of every edge (and one
// without a yaw, an error), a repeater and a comparator feeding lamps, a note
// block an observer watches, a daylight detector, redstone dust (a dot, a
// cross, a line and a junction, which a use does not change, and a dot on a
// redstone block that becomes a cross into a stone with a lamp behind), a
// decorated pot, and stone. Sculk sensors hear the player. Items dropped
// before and after the first use (which makes the input player: an entity id
// and shared random values) show the draws.

const useWorld = (shrieker) => () => {
  const world = new World();
  world.fill([-1, 0, -1], [26, 0, 12], 'minecraft:stone');
  const door = (x, z, id, facing, hinge) => {
    world.set(x, 1, z, `minecraft:${id}`, { facing, half: 'lower', hinge, open: 'false', powered: 'false' });
    world.set(x, 2, z, `minecraft:${id}`, { facing, half: 'upper', hinge, open: 'false', powered: 'false' });
  };
  door(0, 0, 'oak_door', 'south', 'left');
  door(2, 0, 'copper_door', 'east', 'right');
  door(4, 0, 'iron_door', 'north', 'left');
  world.set(6, 1, 0, 'minecraft:bamboo_trapdoor', { facing: 'north', half: 'bottom', open: 'false', powered: 'false', waterlogged: 'false' });
  world.set(8, 1, 0, 'minecraft:iron_trapdoor', { facing: 'south', half: 'top', open: 'false', powered: 'false', waterlogged: 'false' });
  world.set(10, 1, 0, 'minecraft:waxed_oxidized_copper_trapdoor', { facing: 'east', half: 'top', open: 'true', powered: 'false', waterlogged: 'false' });
  ['north', 'south', 'east', 'west'].forEach((facing, i) => world.set(i * 3, 1, 3, 'minecraft:oak_fence_gate', { facing, in_wall: 'false', open: 'false', powered: 'false' }));
  world.set(0, 1, 6, 'minecraft:redstone_block');
  world.set(1, 1, 6, 'minecraft:repeater', { delay: '1', facing: 'west', locked: 'false', powered: 'false' });
  world.set(2, 1, 6, 'minecraft:redstone_wire', { east: 'side', west: 'side', north: 'none', south: 'none', power: '0' });
  world.set(3, 1, 6, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(7, 1, 6, 'minecraft:redstone_block');
  world.set(6, 1, 6, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  world.set(6, 1, 5, 'minecraft:redstone_block');
  world.set(5, 1, 6, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(9, 1, 6, 'minecraft:note_block', { instrument: 'harp', note: '23', powered: 'false' });
  world.set(9, 2, 6, 'minecraft:observer', { facing: 'down', powered: 'false' });
  world.set(9, 3, 6, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(12, 1, 6, 'minecraft:daylight_detector', { inverted: 'false', power: '0' });
  const dust = (x, z, sides) => world.set(x, 1, z, 'minecraft:redstone_wire', { north: sides[0], east: sides[1], south: sides[2], west: sides[3], power: '0' });
  dust(0, 9, ['none', 'none', 'none', 'none']);
  dust(3, 9, ['side', 'side', 'side', 'side']);
  dust(6, 9, ['none', 'side', 'none', 'side']);
  dust(7, 9, ['none', 'side', 'none', 'side']);
  for (const [x, z] of [[11, 10], [10, 10], [12, 10], [11, 9], [11, 11]]) dust(x, z, ['side', 'side', 'side', 'side']);
  world.set(15, 0, 9, 'minecraft:redstone_block');
  dust(15, 9, ['none', 'none', 'none', 'none']);
  world.set(17, 1, 9, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(16, 1, 9, 'minecraft:stone');
  world.set(20, 1, 0, 'minecraft:decorated_pot', { cracked: 'false', facing: 'north', waterlogged: 'false' });
  world.set(2, 1, 2, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(20, 1, 3, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(23, 1, 2, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(10, 1, 3, 'minecraft:white_wool');
  if (shrieker) world.set(1, 1, 1, 'minecraft:sculk_shrieker', { can_summon: 'false', shrieking: 'false', waterlogged: 'false' });
  world.finalize();
  return world;
};
const useProbes = [[0, 1, 0], [0, 2, 0], [2, 1, 0], [4, 1, 0], [6, 1, 0], [8, 1, 0], [10, 1, 0], [0, 1, 3], [3, 1, 3], [6, 1, 3], [9, 1, 3], [1, 1, 6], [2, 1, 6], [3, 1, 6], [6, 1, 6], [5, 1, 6], [9, 1, 6],
  [9, 2, 6], [9, 3, 6], [12, 1, 6], [0, 1, 9], [3, 1, 9], [6, 1, 9], [11, 1, 10], [15, 1, 9], [16, 1, 9], [17, 1, 9], [20, 1, 0], [2, 1, 2], [20, 1, 3], [23, 1, 2], [1, 1, 1]];
const useAt = (tick, at, yaw) => ({ tick, type: 'use', at, ...(yaw === undefined ? {} : { yaw }) });
const useRequest = {
  probes: useProbes, samples: sequence(61), settle_ticks: 2, item_entities: true, living_entities: true,
  actions: [
    { tick: 0, type: 'drop', at: [25.5, 2, 0.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 1 },
    useAt(1, [0, 1, 0], 0), useAt(3, [0, 2, 0]), useAt(4, [2, 1, 0], 180), useAt(5, [4, 1, 0], 0), useAt(6, [6, 1, 0]), useAt(7, [8, 1, 0]), useAt(7, [10, 1, 0], 90),
    { tick: 2, type: 'drop', at: [25.5, 2, 2.5], velocity: [0.1, 0.2, 0], item: 'minecraft:redstone', count: 3 },
    ...[0, 90, 180, 270].map((yaw, i) => useAt(8 + i, [i * 3, 1, 3], yaw)), useAt(12, [0, 1, 3]),
    ...[44.99, -45, 134.99, 540].map((yaw, i) => useAt(14, [i * 3, 1, 3], yaw)), ...[-1000.5, 225.01, 315, -0.01].map((yaw, i) => useAt(16, [i * 3, 1, 3], yaw)),
    useAt(8, [1, 1, 6]), useAt(9, [1, 1, 6]), useAt(10, [1, 1, 6]), useAt(11, [1, 1, 6], 33),
    useAt(9, [6, 1, 6]), useAt(15, [6, 1, 6]),
    useAt(10, [9, 1, 6]), useAt(12, [9, 1, 6]), useAt(13, [9, 1, 6], 7.5),
    useAt(11, [12, 1, 6]), useAt(30, [12, 1, 6]),
    useAt(5, [0, 1, 9]), useAt(20, [0, 1, 9]), useAt(6, [3, 1, 9]), useAt(7, [6, 1, 9]), useAt(8, [11, 1, 10]), useAt(9, [15, 1, 9]), useAt(25, [15, 1, 9]),
    useAt(3, [20, 1, 0]), useAt(17, [20, 1, 0], 12),
    useAt(4, [16, 1, 9]), useAt(4, [25, 1, 5]),
    { tick: 20, type: 'drop', at: [25.5, 2, 4.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 1 },
  ],
};

// ---- inventory-verify

const inventoryVerifyWorld = () => {
  const world = new World();
  world.set(0, 2, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(0, 1, 0, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  world.set(0, 0, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(4, 0, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(8, 2, 0, 'minecraft:note_block', { instrument: 'harp', note: '0', powered: 'false' });
  world.set(8, 1, 0, 'minecraft:observer', { facing: 'up', powered: 'false' });
  world.set(8, 0, 0, 'minecraft:redstone_lamp', { lit: 'false' });
  world.finalize();
  return world;
};
const inventoryProbes = [[0, 2, 0], [0, 1, 0], [0, 0, 0], [4, 0, 0]];
const inventoryVerifyRequest = {
  probes: [...inventoryProbes, [8, 2, 0], [8, 1, 0], [8, 0, 0]], inventory_probes: inventoryProbes, settle_ticks: 1, samples: sequence(81),
  block_entities: [
    { at: [0, 2, 0], data_version: 2975, snbt: '{Items:[{Slot:0b,id:"minecraft:iron_ingot",Count:4b}]}' },
    { at: [4, 0, 0], data_version: 2975, snbt: '{Items:[{Slot:5b,id:"minecraft:stone",Count:7b,tag:{display:{Name:\'{"text":"Filter"}\'}}}]}' },
  ],
  actions: [
    { tick: 10, type: 'use', at: [8, 2, 0] }, { tick: 40, type: 'use', at: [8, 2, 0] },
    { tick: 0, type: 'insert', at: [0, 2, 0], slot: 1, item: 'minecraft:gold_ingot', count: 3 },
    { tick: 0, type: 'insert', at: [4, 0, 0], slot: 5, item: 'minecraft:dirt', count: 1 },
    { tick: 0, type: 'extract', at: [4, 0, 0], slot: 5, count: 8 },
    { tick: 50, type: 'extract', at: [4, 0, 0], slot: 5, count: 2 },
  ],
};

// ---- inventory-input-errors
// An insert and an extract at slots in and out of range on every kind of block
// entity, containers or not: what each refuses with ("Input is not a container",
// "Invalid inventory slot", ...) is the same on Paper and on both engines.

const inputErrorBlocks = [
  'chest', 'trapped_chest', 'ender_chest', 'barrel', 'shulker_box', 'white_shulker_box', 'hopper', 'dropper', 'dispenser', 'furnace', 'blast_furnace', 'smoker',
  'brewing_stand', 'lectern', 'jukebox', 'chiseled_bookshelf', 'crafter', 'campfire', 'soul_campfire', 'oak_sign', 'oak_hanging_sign', 'white_banner', 'beacon',
  'white_bed', 'bell', 'comparator', 'daylight_detector', 'enchanting_table', 'end_portal', 'end_gateway', 'player_head', 'spawner', 'trial_spawner', 'vault',
  'conduit', 'command_block', 'structure_block', 'test_block', 'sculk_sensor', 'beehive', 'piston', 'redstone_wire', 'stone',
  'decorated_pot', 'oak_shelf',
];
const inputErrorWorld = () => {
  const world = new World();
  inputErrorBlocks.forEach((id, index) => world.set(index * 2, 1, 0, `minecraft:${id}`));
  world.finalize();
  return world;
};
const inputErrorActions = inputErrorBlocks.flatMap((_, index) => [0, 5, 40, 200].flatMap((slot) => [
  { tick: 1 + (slot % 7), type: 'insert', at: [index * 2, 1, 0], slot, item: 'minecraft:stick', count: 1 },
  { tick: 12, type: 'extract', at: [index * 2, 1, 0], slot, count: 1 },
]));

// ---- item-supply

const stoneWorld = () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.finalize();
  return world;
};
const itemSupplyRequest = {
  probes: [[0, 0, 0]], settle_ticks: 1, item_entities: true, samples: sequence(21),
  actions: [{ tick: 3, type: 'drop', at: [0.5, 5, 0.5], velocity: [0.1, 0, 0], item: 'minecraft:diamond', count: 7 }],
};

// ---- lectern-input

const lecternWorld = (hasBook) => () => {
  const world = new World();
  for (let x = 0; x < 3; x++) world.set(x, 0, 0, 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:lectern', { facing: 'north', has_book: hasBook, powered: 'false' });
  world.set(1, 1, 0, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  world.set(2, 1, 0, 'minecraft:redstone_wire');
  world.finalize();
  return world;
};
const lecternPages = sequence(15).map((index) => JSON.stringify(JSON.stringify(String(index)))).join(',');
const lecternInputRequest = {
  probes: [[0, 1, 0], [1, 1, 0], [2, 1, 0]], samples: sequence(96), settle_ticks: 20,
  block_entities: [{ at: [0, 1, 0], data_version: 4189, snbt: `{id:"minecraft:lectern",Page:0,Book:{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"levels",author:"Verifier",pages:[${lecternPages}],resolved:true}}}}` }],
  actions: [
    { tick: 5, type: 'page', at: [0, 1, 0], page: 14 }, { tick: 15, type: 'page', at: [0, 1, 0], page: 7 },
    { tick: 25, type: 'page', at: [0, 1, 0], page: 0 }, { tick: 35, type: 'page', at: [0, 1, 0], page: 15 },
    { tick: 45, type: 'page', at: [0, 1, 0], page: 0 }, { tick: 55, type: 'book', at: [0, 1, 0], present: false },
    { tick: 65, type: 'page', at: [0, 1, 0], page: 7 }, { tick: 75, type: 'book', at: [0, 1, 0], present: true },
    { tick: 75, type: 'page', at: [0, 1, 0], page: 7 },
  ],
};

// ---- lectern-supply

const suppliedBook = {
  item: 'minecraft:written_book', count: 1,
  stack: { data_version: 3465, snbt: JSON.stringify({ id: 'minecraft:written_book', Count: 1, tag: { title: 'Levels', author: 'Fixture', pages: sequence(15).map((index) => JSON.stringify({ text: String(index) })), resolved: true } }) },
};
const supply = (tick) => ({ tick, type: 'book', at: [0, 1, 0], present: true, ...suppliedBook });
const lecternSupplyRequest = {
  probes: [[0, 1, 0], [1, 1, 0], [2, 1, 0]], world_inventories: true, samples: sequence(101), settle_ticks: 20,
  actions: [
    { ...supply(1), stack: { ...suppliedBook.stack, data_version: 999999 } }, supply(10), supply(20),
    { tick: 30, type: 'page', at: [0, 1, 0], page: 14 }, { tick: 40, type: 'book', at: [0, 1, 0], present: false },
    supply(50), { tick: 60, type: 'book', at: [0, 1, 0], present: true }, { tick: 70, type: 'page', at: [0, 1, 0], page: 7 },
  ],
};

// ---- living-census

const livingCensusWorld = () => {
  const world = new World();
  for (let x = -3; x <= 3; x++) for (let z = -4; z <= 2; z++) world.set(x, 0, z, 'minecraft:stone');
  world.set(0, 2, 0, 'minecraft:dispenser', { facing: 'north', triggered: 'false' });
  world.set(1, 2, 0, 'minecraft:lever', { face: 'wall', facing: 'east', powered: 'false' });
  world.finalize();
  return world;
};
const livingCensusRequest = {
  probes: [[0, 2, 0], [0, 2, -1], [1, 2, 0]], inventory_probes: [[0, 2, 0]], world_inventories: true, item_entities: true, settle_ticks: 1, samples: sequence(81),
  actions: [
    { tick: 1, type: 'insert', at: [0, 2, 0], slot: 0, item: 'minecraft:axolotl_bucket', count: 1, stack: { data_version: 4556, snbt: '{id:"minecraft:axolotl_bucket",count:1,components:{"minecraft:bucket_entity_data":{NoAI:1b,Health:14.0f}}}' } },
    { tick: 2, type: 'toggle', at: [1, 2, 0] },
  ],
};

// ---- passenger-import (the synthetic minecart with a dolphin rider)

const passengerWorld = () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' });
  world.finalize();
  return world;
};
const passengerRequest = {
  probes: [[0, 1, 0]], samples: [0, 1, 5, 20], settle_ticks: 1, world_inventories: true, item_entities: true,
  entities: [{ at: [0.5, 1, 0.5], data_version: 3465, snbt: '{id:"minecraft:minecart",UUID:[I;101,102,103,104],Pos:[0.5d,1.0d,0.5d],Passengers:[{id:"minecraft:dolphin",UUID:[I;201,202,203,204],Pos:[0.5d,1.0d,0.5d],NoAI:1b,Air:4800s,Moistness:2400,HandItems:[{id:"minecraft:diamond",Count:7b},{}]}]}' }],
};

// ---- scheduled-ticks

const scheduledTicksWorld = () => {
  const world = new World();
  world.set(0, 1, 0, 'minecraft:redstone_lamp', { lit: 'true' });
  world.set(2, 1, 0, 'minecraft:redstone_lamp', { lit: 'true' });
  for (let x = 4; x < 8; x++) for (let z = 0; z < 3; z++) world.set(x, 0, z, 'minecraft:stone');
  for (let x = 4; x < 8; x++) {
    world.set(x, 1, 0, 'minecraft:stone');
    world.set(x, 1, 2, 'minecraft:stone');
  }
  world.set(4, 1, 1, 'minecraft:stone');
  world.set(5, 1, 1, 'minecraft:water', { level: '0' });
  world.finalize();
  return world;
};
const savedTicks = {
  bounds: [[0, 0, 0], [7, 2, 2]], data_version: 4189,
  entries: [
    { kind: 'block', id: 'minecraft:redstone_lamp', at: [0, 1, 0], delay: 10, priority: -1, sub_tick: '9007199254740993' },
    { kind: 'block', id: 'minecraft:redstone_lamp', at: [2, 1, 0], delay: 15, priority: 2, sub_tick: '9007199254740994' },
    { kind: 'fluid', id: 'minecraft:water', at: [5, 1, 1], delay: 12, priority: 0, sub_tick: '-2' },
  ],
};
const scheduledTicksRequest = { probes: [[0, 1, 0], [2, 1, 0], [6, 1, 1]], samples: sequence(22), settle_ticks: 1, scheduled_ticks: savedTicks };

// ---- versioned-verifier (the test's default 1.21.11 target, strict placement)

const versionedWorld = () => {
  const world = new World();
  for (const z of [0, 4, 7, 10]) for (let x = 0; x < 6; x++) world.set(x, 0, z, 'minecraft:stone');
  world.set(2, 0, 1, 'minecraft:stone');
  world.set(2, 0, 2, 'minecraft:stone');
  world.set(2, 0, 3, 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:stone_button', { face: 'floor', facing: 'north', powered: 'false' });
  for (const x of [1, 2]) world.set(x, 1, 0, 'minecraft:redstone_wire');
  world.set(3, 1, 0, 'minecraft:redstone_lamp');
  world.set(4, 1, 0, 'minecraft:observer', { facing: 'west', powered: 'false' });
  world.set(5, 1, 0, 'minecraft:redstone_lamp');
  world.set(0, 0, 1, 'minecraft:sticky_piston', { facing: 'south', extended: 'false' });
  world.set(0, 0, 2, 'minecraft:stone');
  world.set(0, 1, 4, 'minecraft:chest');
  world.set(0, 1, 7, 'minecraft:lectern', { facing: 'north', has_book: 'false', powered: 'false' });
  for (const z of [4, 7]) {
    world.set(1, 1, z, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
    world.set(2, 1, z, 'minecraft:redstone_wire');
    world.set(3, 1, z, 'minecraft:redstone_lamp');
  }
  world.set(0, 1, 10, 'minecraft:hopper', { facing: 'east', enabled: 'true' });
  world.set(1, 1, 10, 'minecraft:chest');
  world.set(4, 1, 10, 'minecraft:barrel');
  world.finalize();
  return world;
};
const versionedBook = { id: 'minecraft:written_book', Count: 1, tag: { title: 'Levels', author: 'Fixture', pages: sequence(15).map((index) => JSON.stringify({ text: String(index) })), resolved: true } };
const versionedRequest = {
  // 4556 is this app's runtime data version; the test's 1.21.11 value (4671) is a future version for it.
  block_data_version: 4556,
  block_entities: [{ at: [4, 1, 10], data_version: 3465, snbt: '{id:"minecraft:barrel",Items:[{Slot:2b,id:"minecraft:gold_ingot",Count:7b}]}' }],
  probes: [[0, 1, 0], [1, 1, 0], [3, 1, 0], [4, 1, 0], [5, 1, 0], [0, 0, 1], [0, 0, 2], [0, 0, 3], [2, 1, 4], [0, 1, 7], [2, 1, 7]],
  world_inventories: true, item_entities: true, living_entities: true, settle_ticks: 40, samples: sequence(121),
  actions: [
    { tick: 5, type: 'press', at: [0, 1, 0] },
    { tick: 10, type: 'insert', at: [0, 1, 4], slot: 0, item: 'minecraft:diamond', count: 64 },
    { tick: 15, type: 'drop', at: [0.5, 2, 10.5], velocity: [0, 0, 0], item: 'minecraft:emerald', count: 7 },
    { tick: 20, type: 'book', at: [0, 1, 7], present: true, item: 'minecraft:written_book', count: 1, stack: { data_version: 3465, snbt: JSON.stringify(versionedBook) } },
    { tick: 35, type: 'page', at: [0, 1, 7], page: 14 },
    { tick: 45, type: 'extract', at: [0, 1, 4], slot: 0, count: 64 },
    { tick: 65, type: 'book', at: [0, 1, 7], present: false },
    { tick: 80, type: 'book', at: [0, 1, 7], present: true },
  ],
};

// ---- minecraft-verify.integration (its experiment cases)

const buttonsWorld = () => {
  const world = new World();
  world.fill([-1, 0, -4], [25, 0, 3], 'minecraft:stone');
  for (const x of [0, 8]) {
    world.set(x, 1, 1, 'minecraft:waxed_copper_bulb', { lit: 'false', powered: 'false' });
    world.set(x, 1, 2, 'minecraft:stone_button', { face: 'wall', facing: 'south', powered: 'false' });
    world.set(x, 1, 0, 'minecraft:comparator', { facing: x === 0 ? 'south' : 'north', mode: 'compare', powered: 'false' });
    world.set(x, 1, -1, 'minecraft:redstone_wire');
    world.set(x, 1, -2, 'minecraft:repeater', { facing: 'south', delay: '1', powered: 'false' });
    world.set(x, 1, -3, 'minecraft:redstone_lamp', { lit: 'false' });
  }
  world.set(16, 1, -1, 'minecraft:stone');
  world.set(16, 1, 0, 'minecraft:lever', { face: 'wall', facing: 'south', powered: 'false' });
  world.set(16, 1, -2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(24, 4, 0, 'minecraft:sand');
  world.finalize();
  return world;
};
const buttonProbes = [[0, 1, 2], [0, 1, 1], [0, 1, -3], [8, 1, -3], [16, 1, -2], [24, 4, 0], [24, 1, 0]];
const buttonsRequest = {
  probes: buttonProbes, samples: [0, 10, 25, 40, 50, 70],
  actions: [0, 40].flatMap((tick) => [
    { tick, type: 'press', at: [0, 1, 2] }, { tick, type: 'press', at: [8, 1, 2] }, { tick, type: 'toggle', at: [16, 1, 0] },
  ]),
};
const drainWorld = () => {
  const world = new World();
  world.fill([0, 0, 0], [2, 0, 2], 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(0, 1, 1, 'minecraft:comparator', { facing: 'north', mode: 'compare', powered: 'false' });
  world.set(0, 1, 2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.finalize();
  return world;
};
const drainRequest = {
  probes: [[0, 1, 0], [0, 1, 2]], inventory_probes: [[0, 1, 0]], samples: sequence(41),
  actions: [
    { tick: 5, type: 'insert', at: [0, 1, 0], slot: 0, item: 'minecraft:stone', count: 64 },
    { tick: 5, type: 'insert', at: [0, 1, 0], slot: 3, item: 'minecraft:ender_pearl', count: 16 },
    { tick: 20, type: 'drain', at: [0, 1, 0] }, { tick: 30, type: 'drain', at: [0, 1, 0] }, { tick: 30, type: 'drain', at: [2, 0, 0] },
  ],
};
// A lever powers and unpowers a shelf (block_activate, block_deactivate), which a
// sculk sensor and a calibrated one in range hear.
const shelfSensorWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [9, 0, 3], 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:stone');
  world.set(0, 1, 1, 'minecraft:lever', { face: 'wall', facing: 'south', powered: 'false' });
  world.set(1, 1, 0, 'minecraft:oak_shelf', { facing: 'south', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  world.set(5, 1, 0, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(8, 1, 2, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.finalize();
  return world;
};
const shelfSensorRequest = {
  probes: [[1, 1, 0], [5, 1, 0], [8, 1, 2]], samples: sequence(81), settle_ticks: 1,
  actions: [{ tick: 3, type: 'toggle', at: [0, 1, 1] }, { tick: 45, type: 'toggle', at: [0, 1, 1] }],
};
// A hopper empties a full composter, and another fills one (block_change from the composter, a state the composter holds when it
// posts it), which a sculk sensor and a calibrated one in range hear.
const composterSensorWorld = (output) => () => {
  const world = new World();
  world.fill([-1, 0, -1], [9, 0, 3], 'minecraft:stone');
  if (output) {
    world.set(0, 2, 0, 'minecraft:composter', { level: '8' });
    world.set(0, 1, 0, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  } else {
    world.set(0, 1, 0, 'minecraft:composter', { level: '2' });
    world.set(0, 2, 0, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  }
  world.set(5, 1, 0, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(8, 1, 2, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.finalize();
  return world;
};
const composterSensorRequest = (output) => ({
  probes: [[0, output ? 2 : 1, 0], [5, 1, 0], [8, 1, 2]], samples: sequence(41), settle_ticks: 1,
  ...(output ? {} : { actions: [1, 2, 3, 4, 5, 6].map((tick) => ({ tick, type: 'insert', at: [0, 2, 0], slot: 0, item: 'minecraft:wheat_seeds', count: 1 })) }),
});
// A sniffer egg placed within a sculk sensor's and a calibrated one's range (block_place), over sand and over dirt.
const snifferEggSensorWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [9, 0, 3], 'minecraft:stone');
  world.set(5, 1, 0, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(8, 1, 2, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(1, 0, 0, 'minecraft:sand');
  world.set(1, 1, 0, 'minecraft:sniffer_egg', { hatch: '0' });
  world.set(1, 1, 2, 'minecraft:sniffer_egg', { hatch: '1' });
  world.finalize();
  return world;
};
// Blocks that change by themselves in range of a sculk sensor and a calibrated one (block_change): farmland under a solid block turns
// to dirt, dried ghasts (dry, and waterlogged) change hydration on a saved tick, and flowing water puts a lit candle out.
const changeSensorWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [9, 0, 5], 'minecraft:stone');
  world.set(5, 1, 0, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(8, 1, 3, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(1, 1, 0, 'minecraft:farmland', { moisture: '0' });
  world.set(1, 2, 0, 'minecraft:dripstone_block');
  world.set(1, 1, 2, 'minecraft:dried_ghast', { facing: 'north', hydration: '2', waterlogged: 'false' });
  world.set(3, 1, 2, 'minecraft:dried_ghast', { facing: 'north', hydration: '1', waterlogged: 'true' });
  world.set(1, 1, 4, 'minecraft:water', { level: '0' });
  world.set(2, 1, 4, 'minecraft:candle', { candles: '1', lit: 'true', waterlogged: 'false' });
  world.finalize();
  return world;
};
const changeSensorRequest = {
  probes: [[1, 1, 0], [1, 1, 2], [3, 1, 2], [2, 1, 4], [5, 1, 0], [8, 1, 3]], samples: sequence(41), settle_ticks: 1,
  scheduled_ticks: {
    bounds: [[0, 0, 0], [4, 3, 3]], data_version: 4556,
    entries: [
      { kind: 'block', id: 'minecraft:dried_ghast', at: [1, 1, 2], delay: 5, priority: 0, sub_tick: '9007199254740993' },
      { kind: 'block', id: 'minecraft:dried_ghast', at: [3, 1, 2], delay: 9, priority: 0, sub_tick: '9007199254740994' },
    ],
  },
};
// A big dripleaf that tilts on a saved tick, each with a sensor beside it that no other leaf is near: unstable (the tilt that
// makes no vibration) to partial, partial to full, and full back to none make one each; the sensor holds what it heard.
const tiltSensorWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [31, 0, 5], 'minecraft:stone');
  for (const [i, tilt] of ['unstable', 'partial', 'full'].entries()) {
    world.set(1 + 12 * i, 0, 2, 'minecraft:clay');
    world.set(1 + 12 * i, 1, 2, 'minecraft:big_dripleaf', { facing: 'north', tilt, waterlogged: 'false' });
    world.set(2 + 12 * i, 1, 2, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  }
  world.finalize();
  return world;
};
const tiltSensorRequest = {
  probes: [[1, 1, 2], [2, 1, 2], [13, 1, 2], [14, 1, 2], [25, 1, 2], [26, 1, 2]], samples: sequence(41), settle_ticks: 1,
  scheduled_ticks: {
    bounds: [[0, 0, 0], [28, 3, 3]], data_version: 4556,
    entries: [0, 1, 2].map((i) => ({ kind: 'block', id: 'minecraft:big_dripleaf', at: [1 + 12 * i, 1, 2], delay: 5 + i, priority: 0, sub_tick: String(9007199254740993n + BigInt(i)) })),
  },
};
const fallingWorld = () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  world.set(0, 4, 0, 'minecraft:sand');
  world.finalize();
  return world;
};

// ---- imports at the server's own data version (4556): block entities, entities and supplied stacks
// the Rust engine reads (older versions go through the data fixer: the fixtures above and the fuzzer's old imports)

const NOW = 4556;
const slotted = (slot, item, count, components) => `{Slot:${slot}b,id:"minecraft:${item}",count:${count}${components ? `,components:${components}` : ''}}`;
const blockEntity = (at, id, fields) => ({ at, data_version: NOW, snbt: `{id:"minecraft:${id}"${fields ? `,${fields}` : ''}}` });
const ground = (world, x0, z0, x1, z1) => world.fill([x0, 0, z0], [x1, 0, z1], 'minecraft:stone');
// ---- brushable-fall (suspicious sand and gravel that fall: a falling block that breaks where it lands,
// and posts block_destroy at the centre of its box, which the sensors beside the landings hear)

const brushableFallWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [13, 0, 5], 'minecraft:stone');
  world.set(2, 5, 1, 'minecraft:suspicious_sand', { dusted: '0' });
  world.set(4, 1, 1, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(6, 6, 1, 'minecraft:suspicious_gravel', { dusted: '3' });
  world.set(8, 1, 1, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(10, 4, 1, 'minecraft:suspicious_sand', { dusted: '2' });
  world.set(10, 1, 1, 'minecraft:snow', { layers: '1' });
  world.set(12, 3, 1, 'minecraft:suspicious_gravel', { dusted: '1' });
  world.set(12, 1, 1, 'minecraft:water', { level: '0' });
  world.set(12, 2, 1, 'minecraft:water', { level: '0' });
  world.set(0, 3, 3, 'minecraft:suspicious_sand', { dusted: '1' });
  world.set(0, 2, 3, 'minecraft:suspicious_gravel', { dusted: '0' });
  world.set(1, 1, 3, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.finalize();
  return world;
};
const brushableFallRequest = {
  probes: [[2, 1, 1], [4, 1, 1], [6, 1, 1], [8, 1, 1], [10, 1, 1], [12, 1, 1], [0, 1, 3], [1, 1, 3]], item_entities: true, samples: sequence(41), settle_ticks: 1,
};

// ---- trial-spawner (every state a trial spawner is placed in, ominous or not: those waiting for players or
// active ask the game rules whether they may spawn)

const TRIAL_STATES = ['inactive', 'waiting_for_players', 'active', 'waiting_for_reward_ejection', 'ejecting_reward', 'cooldown'];
const trialSpawnerWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [13, 0, 3], 'minecraft:stone');
  TRIAL_STATES.forEach((state, index) => {
    world.set(index * 2, 1, 0, 'minecraft:trial_spawner', { trial_spawner_state: state, ominous: 'false' });
    world.set(index * 2, 1, 2, 'minecraft:trial_spawner', { trial_spawner_state: state, ominous: 'true' });
  });
  world.finalize();
  return world;
};
const trialSpawnerRequest = {
  probes: TRIAL_STATES.flatMap((_, index) => [[index * 2, 1, 0], [index * 2, 1, 2]]), samples: sequence(81), settle_ticks: 1,
};

// ---- command-blocks (saved command blocks: a command is never run, the success counts and ticks are what is seen)

const commandBlocksWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [20, 0, 12], 'minecraft:stone');
  // Rows along x: each block with a comparator, wire and lamp beside it reading its success count.
  const row = (z, blocks) => blocks.forEach(([id, facing, conditional], index) => {
    const x = index * 3;
    world.set(x, 1, z, id, { conditional: conditional || 'false', facing });
  });
  row(0, [['minecraft:command_block', 'east'], ['minecraft:command_block', 'east'], ['minecraft:command_block', 'east', 'true']]);
  row(3, [['minecraft:repeating_command_block', 'east'], ['minecraft:chain_command_block', 'east'], ['minecraft:chain_command_block', 'east', 'true']]);
  row(6, [['minecraft:chain_command_block', 'east'], ['minecraft:repeating_command_block', 'south'], ['minecraft:command_block', 'west']]);
  world.set(0, 1, 9, 'minecraft:redstone_block');
  world.set(1, 1, 9, 'minecraft:command_block', { conditional: 'false', facing: 'east' });
  world.set(2, 1, 9, 'minecraft:chain_command_block', { conditional: 'false', facing: 'east' });
  world.set(3, 1, 9, 'minecraft:chain_command_block', { conditional: 'true', facing: 'east' });
  world.set(4, 1, 9, 'minecraft:chain_command_block', { conditional: 'false', facing: 'north' });
  world.finalize();
  return world;
};
const commandBlocksProbes = [[0, 1, 0], [3, 1, 0], [6, 1, 0], [0, 1, 3], [3, 1, 3], [6, 1, 3], [0, 1, 6], [3, 1, 6], [6, 1, 6], [1, 1, 9], [2, 1, 9], [3, 1, 9], [4, 1, 9]];
const commandBlocksRequest = {
  probes: commandBlocksProbes, samples: sequence(41), settle_ticks: 1,
  block_entities: [
    blockEntity([0, 1, 0], 'command_block', 'Command:"Searge",auto:1b'),
    blockEntity([3, 1, 0], 'command_block', 'Command:"Searge",SuccessCount:3,TrackOutput:0b,LastOutput:{text:"old",color:"red"}'),
    blockEntity([6, 1, 0], 'command_block', 'Command:"",powered:1b,conditionMet:1b,CustomName:\'{"text":"cb"}\',UpdateLastExecution:0b'),
    blockEntity([0, 1, 3], 'command_block', 'Command:"say a",auto:0b,SuccessCount:2'),
    blockEntity([3, 1, 3], 'command_block', 'Command:"SEARGE",auto:1b,LastExecution:5L'),
    blockEntity([6, 1, 3], 'command_block', 'Command:"",auto:1b,conditionMet:1b,SuccessCount:2'),
    blockEntity([0, 1, 6], 'command_block', 'auto:1b,SuccessCount:7'),
    blockEntity([3, 1, 6], 'command_block', 'Command:"ſearge",auto:1b,LastExecution:0L'),
    blockEntity([6, 1, 6], 'command_block', 'Command:"",auto:1b,powered:1b'),
    blockEntity([1, 1, 9], 'command_block', 'Command:"Searge",SuccessCount:1'),
    blockEntity([2, 1, 9], 'command_block', 'Command:"Searge",auto:1b'),
    blockEntity([3, 1, 9], 'command_block', 'Command:"Searge",auto:1b'),
    blockEntity([4, 1, 9], 'command_block', 'Command:"",auto:1b,SuccessCount:4'),
  ],
};

// ---- beacons (saved beacons: the levels, effects, name and lock they hold; the base is checked every 80 ticks)

const beaconsWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [14, 0, 6], 'minecraft:stone');
  // A one-layer pyramid and a bare beacon; a tinted beam.
  for (const [x, base] of [[1, true], [5, false], [9, true]]) {
    if (base) for (let dx = -1; dx <= 1; dx++) for (let dz = -1; dz <= 1; dz++) world.set(x + dx, 1, 2 + dz, 'minecraft:iron_block');
    world.set(x, 2, 2, 'minecraft:beacon');
    world.set(x, 3, 2, x === 9 ? 'minecraft:red_stained_glass' : 'minecraft:air');
  }
  world.finalize();
  return world;
};
const beaconsRequest = {
  probes: [[1, 2, 2], [5, 2, 2], [9, 2, 2]], samples: [0, 1, 40, 79, 80, 81, 160, 161, 240], settle_ticks: 1,
  block_entities: [
    blockEntity([1, 2, 2], 'beacon', 'Levels:3,primary_effect:"minecraft:speed",secondary_effect:"minecraft:poison",CustomName:{text:"Beam",color:"gold"},lock:{items:"minecraft:stone"},Paper.Range:12.5d'),
    blockEntity([5, 2, 2], 'beacon', 'Levels:4,primary_effect:"minecraft:haste"'),
    blockEntity([9, 2, 2], 'beacon', 'Levels:-5,secondary_effect:"minecraft:regeneration",components:{"minecraft:rarity":"rare"}'),
  ],
};

// ---- saved-specials (saved structure, test and jigsaw blocks, end gateways and portals, decorated pots, suspicious sand)

const savedSpecialsWorld = () => {
  const world = new World();
  world.fill([-1, 0, -1], [30, 0, 8], 'minecraft:stone');
  world.set(0, 1, 0, 'minecraft:structure_block', { mode: 'save' });
  world.set(3, 1, 0, 'minecraft:structure_block', { mode: 'data' });
  world.set(6, 1, 0, 'minecraft:structure_block', { mode: 'load' });
  world.set(9, 1, 0, 'minecraft:structure_block', { mode: 'corner' });
  world.set(12, 1, 0, 'minecraft:structure_block', { mode: 'data' });
  world.set(12, 1, 1, 'minecraft:redstone_block');
  world.set(15, 1, 0, 'minecraft:structure_block', { mode: 'save' });
  world.set(15, 1, 1, 'minecraft:redstone_block');
  world.set(0, 1, 3, 'minecraft:test_block', { mode: 'start' });
  world.set(3, 1, 3, 'minecraft:test_block', { mode: 'accept' });
  world.set(3, 1, 4, 'minecraft:redstone_block');
  world.set(6, 1, 3, 'minecraft:test_block', { mode: 'log' });
  world.set(9, 1, 3, 'minecraft:test_block', { mode: 'start' });
  world.set(0, 1, 6, 'minecraft:jigsaw', { orientation: 'north_up' });
  world.set(3, 1, 6, 'minecraft:jigsaw', { orientation: 'up_north' });
  world.set(6, 1, 6, 'minecraft:end_gateway');
  world.set(9, 1, 6, 'minecraft:end_gateway');
  world.set(12, 1, 6, 'minecraft:end_portal');
  world.set(15, 1, 6, 'minecraft:decorated_pot', { cracked: 'false', facing: 'north', waterlogged: 'false' });
  world.set(18, 5, 6, 'minecraft:suspicious_sand', { dusted: '0' });
  world.set(21, 5, 6, 'minecraft:suspicious_gravel', { dusted: '1' });
  world.set(18, 1, 6, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(24, 1, 6, 'minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(27, 1, 6, 'minecraft:sculk_shrieker', { can_summon: 'false', shrieking: 'false', waterlogged: 'false' });
  world.set(30, 1, 6, 'minecraft:sculk_catalyst', { bloom: 'false' });
  world.finalize();
  return world;
};
const savedSpecialsRequest = {
  probes: [[0, 1, 0], [3, 1, 0], [6, 1, 0], [9, 1, 0], [12, 1, 0], [15, 1, 0], [0, 1, 3], [3, 1, 3], [6, 1, 3], [9, 1, 3], [0, 1, 6], [3, 1, 6], [6, 1, 6], [9, 1, 6], [12, 1, 6], [15, 1, 6], [18, 1, 6], [18, 5, 6], [21, 5, 6], [24, 1, 6], [27, 1, 6], [30, 1, 6]],
  samples: [0, 1, 2, 5, 10, 20, 40, 60], settle_ticks: 1,
  block_entities: [
    blockEntity([0, 1, 0], 'structure_block', 'name:"a",author:"me",metadata:"m",posX:100,posY:-100,sizeX:60,rotation:"CLOCKWISE_90",mirror:"LEFT_RIGHT",mode:"DATA",strict:1b,showair:1b,integrity:0.5f,seed:5L'),
    blockEntity([3, 1, 0], 'structure_block', 'name:"Bad:Name",mode:"LOAD",powered:1b'),
    blockEntity([6, 1, 0], 'structure_block', 'sizeY:3,ignoreEntities:0b,showboundingbox:0b'),
    blockEntity([9, 1, 0], 'structure_block', 'mode:"SAVE",name:"minecraft:x",powered:1b'),
    blockEntity([12, 1, 0], 'structure_block', 'mode:"DATA",name:"d"'),
    blockEntity([15, 1, 0], 'structure_block', 'mode:"SAVE",name:"",powered:0b'),
    blockEntity([0, 1, 3], 'test_block', 'mode:"start",powered:1b,message:"hi"'),
    blockEntity([3, 1, 3], 'test_block', 'message:"there"'),
    blockEntity([6, 1, 3], 'test_block', 'mode:"log",message:"m",powered:1b'),
    blockEntity([9, 1, 3], 'test_block', 'mode:"accept",powered:1b'),
    blockEntity([0, 1, 6], 'jigsaw', 'name:"a:b",target:"c",pool:"d/e",final_state:"minecraft:stone",joint:"aligned",placement_priority:3,selection_priority:-2'),
    blockEntity([3, 1, 6], 'jigsaw', 'pool:"x"'),
    blockEntity([6, 1, 6], 'end_gateway', 'Age:2390L,ExactTeleport:1b,exit_portal:[I;1,70,3]'),
    blockEntity([9, 1, 6], 'end_gateway', 'Age:5L,exit_portal:[I;1,30000000,3]'),
    blockEntity([12, 1, 6], 'end_portal', 'foo:1'),
    blockEntity([15, 1, 6], 'decorated_pot', 'sherds:["minecraft:brick","minecraft:angler_pottery_sherd","minecraft:stone"]'),
    blockEntity([18, 5, 6], 'brushable_block', 'item:{id:"minecraft:diamond",count:3},hit_direction:2'),
    blockEntity([18, 1, 6], 'sculk_sensor', 'last_vibration_frequency:5,listener:{event_delay:3,selector:{tick:7L}}'),
    blockEntity([24, 1, 6], 'calibrated_sculk_sensor', 'last_vibration_frequency:9,listener:{event_delay:-2}'),
    blockEntity([27, 1, 6], 'sculk_shrieker', 'warning_level:3,listener:{event_delay:12,selector:{tick:-1L}}'),
    blockEntity([30, 1, 6], 'sculk_catalyst', 'cursors:[]'),
    blockEntity([21, 5, 6], 'brushable_block', 'item:{id:"minecraft:emerald",count:1,components:{"minecraft:custom_name":{text:"x",color:"red"}}}'),
  ],
  item_entities: true,
};

const wire = (world, x, y, z) => world.set(x, y, z, 'minecraft:redstone_wire');
const lampAfter = (world, x, y, z) => world.set(x, y, z, 'minecraft:redstone_lamp', { lit: 'false' });

// Containers holding items with components, read by comparators, with a hopper
// chain, a dropper a lever fires, and a crafter with a disabled slot.
const importedContainersWorld = () => {
  const world = new World();
  ground(world, 0, 0, 12, 6);
  world.set(0, 1, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(1, 1, 0, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  wire(world, 2, 1, 0);
  lampAfter(world, 3, 1, 0);
  world.set(0, 2, 2, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(0, 1, 2, 'minecraft:hopper', { facing: 'east', enabled: 'true' });
  world.set(1, 1, 2, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(1, 1, 3, 'minecraft:comparator', { facing: 'south', mode: 'compare', powered: 'false' });
  world.set(1, 1, 4, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(5, 1, 0, 'minecraft:dropper', { facing: 'east', triggered: 'false' });
  world.set(6, 1, 0, 'minecraft:chest', { facing: 'west', type: 'single', waterlogged: 'false' });
  world.set(5, 1, 1, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' });
  world.set(8, 1, 0, 'minecraft:crafter', { orientation: 'north_up', triggered: 'false', crafting: 'false' });
  world.set(9, 1, 0, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  wire(world, 10, 1, 0);
  lampAfter(world, 11, 1, 0);
  world.set(8, 1, 3, 'minecraft:white_shulker_box', { facing: 'up' });
  world.set(10, 1, 3, 'minecraft:dispenser', { facing: 'up', triggered: 'false' });
  world.finalize();
  return world;
};
const importedContainersBase = {
  probes: [[0, 1, 0], [3, 1, 0], [0, 2, 2], [1, 1, 2], [4, 1, 4], [5, 1, 0], [6, 1, 0], [8, 1, 0], [11, 1, 0], [8, 1, 3], [10, 1, 3]],
  inventory_probes: [[0, 1, 0], [0, 2, 2], [1, 1, 2], [5, 1, 0], [6, 1, 0], [8, 1, 0], [8, 1, 3], [10, 1, 3]],
  world_inventories: true, item_entities: true, settle_ticks: 2, samples: sequence(60),
  actions: [{ tick: 4, type: 'toggle', at: [5, 1, 1] }, { tick: 20, type: 'toggle', at: [5, 1, 1] }, { tick: 22, type: 'toggle', at: [5, 1, 1] }],
  block_entities: [
    blockEntity([0, 1, 0], 'chest', `CustomName:"Stock",Items:[${slotted(0, 'cobblestone', 64)},${slotted(3, 'diamond_sword', 1, '{"minecraft:custom_name":"Blade","minecraft:damage":7,"minecraft:enchantments":{"minecraft:sharpness":3}}')},${slotted(26, 'shulker_box', 1, '{"minecraft:container":[{slot:2,item:{id:"minecraft:stone",count:5}}]}')}]`),
    blockEntity([0, 2, 2], 'barrel', `Items:[${slotted(0, 'iron_ingot', 9, '{"minecraft:custom_name":"Pure"}')},${slotted(1, 'iron_ingot', 3)},${slotted(5, 'bundle', 1, '{"minecraft:bundle_contents":[{id:"minecraft:redstone",count:4}]}')}]`),
    blockEntity([5, 1, 0], 'dropper', `Items:[${slotted(0, 'snowball', 16)},${slotted(1, 'stick', 3, '{"minecraft:rarity":"rare"}')},${slotted(2, 'stick', 3)}]`),
    blockEntity([8, 1, 0], 'crafter', `crafting_ticks_remaining:0,disabled_slots:[I;0,4],triggered:0,Items:[${slotted(1, 'cobblestone', 2)},${slotted(8, 'stick', 1)}]`),
    blockEntity([8, 1, 3], 'shulker_box', `Items:[${slotted(0, 'emerald', 33)},${slotted(13, 'gold_ingot', 64, '{"minecraft:max_stack_size":64}')}]`),
    blockEntity([10, 1, 3], 'dispenser', `Items:[${slotted(4, 'arrow', 12)}]`),
    blockEntity([1, 1, 2], 'barrel', `Items:[${slotted(2, 'iron_ingot', 50, '{"minecraft:custom_name":"Pure"}')}]`),
  ],
};


// Item components the stacks carry and a placed shulker box keeps: tooltip
// displays (hidden, a hidden component, and the same one named twice, which
// the set keeps once, so a hopper merges the two stacks), block state properties
// on items, and dispensers that place shulker boxes whose stacks carry lore, a
// lock, block state properties (a facing, a value no property has, a property
// the box lacks), a tooltip display, a name and contents.
const COMPONENT_STACKS = {
  hidden: '{"minecraft:tooltip_display":{hide_tooltip:1b}}',
  loreFirst: '{"minecraft:tooltip_display":{hidden_components:["minecraft:lore"]}}',
  rarityFirst: '{"minecraft:tooltip_display":{hidden_components:["minecraft:lore","minecraft:lore"]}}',
  slab: '{"minecraft:block_state":{type:"top",waterlogged:"true"}}',
  box: (facing) => `{"minecraft:lore":["Line one",{text:"Line two",color:"gold"}],"minecraft:lock":{items:"minecraft:diamond"},"minecraft:block_state":${facing},"minecraft:tooltip_display":{hidden_components:["minecraft:lore"]},"minecraft:custom_name":"Box","minecraft:container":[{slot:0,item:{id:"minecraft:stone",count:3}}]}`,
};
const importedComponentsWorld = () => {
  const world = new World();
  ground(world, 0, 0, 10, 4);
  world.set(0, 1, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(3, 2, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(3, 1, 0, 'minecraft:hopper', { enabled: 'true', facing: 'east' });
  world.set(4, 1, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  for (const [x, z] of [[6, 0], [6, 2], [6, 4]]) {
    world.set(x, 1, z, 'minecraft:dispenser', { facing: 'east', triggered: 'false' });
    world.set(x - 1, 1, z, 'minecraft:lever', { face: 'wall', facing: 'west', powered: 'false' });
  }
  world.finalize();
  return world;
};
const importedComponentsRequest = {
  probes: [[0, 1, 0], [3, 2, 0], [3, 1, 0], [4, 1, 0], [6, 1, 0], [7, 1, 0], [6, 1, 2], [7, 1, 2], [6, 1, 4], [7, 1, 4]],
  inventory_probes: [[0, 1, 0], [3, 2, 0], [3, 1, 0], [4, 1, 0], [6, 1, 0], [6, 1, 2], [6, 1, 4]],
  world_inventories: true, item_entities: true, settle_ticks: 2, samples: sequence(40),
  block_entities: [
    blockEntity([0, 1, 0], 'chest', `Items:[${slotted(0, 'diamond', 2, COMPONENT_STACKS.hidden)},${slotted(1, 'stick', 5, COMPONENT_STACKS.loreFirst)},${slotted(2, 'oak_slab', 4, COMPONENT_STACKS.slab)},${slotted(3, 'stick', 1, '{"minecraft:tooltip_display":{}}')}]`),
    blockEntity([3, 2, 0], 'chest', `Items:[${slotted(0, 'stick', 3, COMPONENT_STACKS.loreFirst)},${slotted(1, 'stick', 3, COMPONENT_STACKS.rarityFirst)}]`),
    blockEntity([6, 1, 0], 'dispenser', `Items:[${slotted(0, 'shulker_box', 1, COMPONENT_STACKS.box('{facing:"down"}'))}]`),
    blockEntity([6, 1, 2], 'dispenser', `Items:[${slotted(0, 'red_shulker_box', 1, COMPONENT_STACKS.box('{facing:"sideways",powered:"true"}'))}]`),
    blockEntity([6, 1, 4], 'dispenser', `Items:[${slotted(0, 'shulker_box', 1, COMPONENT_STACKS.box('{}'))}]`),
  ],
  actions: [{ tick: 3, type: 'toggle', at: [5, 1, 0] }, { tick: 3, type: 'toggle', at: [5, 1, 2] }, { tick: 3, type: 'toggle', at: [5, 1, 4] }],
};


// A lectern holding a written book (a comparator reads its page), and a page input the book has no room for.
const importedLecternWorld = () => {
  const world = new World();
  ground(world, 0, 0, 4, 2);
  world.set(0, 1, 0, 'minecraft:lectern', { facing: 'north', has_book: 'true', powered: 'false' });
  world.set(1, 1, 0, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  wire(world, 2, 1, 0);
  lampAfter(world, 3, 1, 0);
  world.set(0, 1, 2, 'minecraft:lectern', { facing: 'north', has_book: 'true', powered: 'false' });
  world.set(1, 1, 2, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  world.set(0, 1, 1, 'minecraft:lectern', { facing: 'north', has_book: 'false', powered: 'false' });
  world.finalize();
  return world;
};
const pages = (n) => sequence(n).map((index) => JSON.stringify(String(index))).join(',');
const importedLecternRequest = {
  probes: [[0, 1, 0], [1, 1, 0], [3, 1, 0], [0, 1, 2], [1, 1, 2], [0, 1, 1]], world_inventories: true, samples: sequence(30), settle_ticks: 2,
  block_entities: [
    blockEntity([0, 1, 0], 'lectern', `Page:7,Book:{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"Levels",author:"Fixture",pages:[${pages(15)}],resolved:true}}}`),
    blockEntity([0, 1, 2], 'lectern', `Page:5,Book:{id:"minecraft:writable_book",count:1,components:{"minecraft:writable_book_content":{pages:["a","b","c"]}}}`),
    blockEntity([0, 1, 1], 'lectern', 'Page:3'),
  ],
  actions: [{ tick: 5, type: 'page', at: [0, 1, 0], page: 40 }, { tick: 6, type: 'page', at: [0, 1, 1], page: 0 }, { tick: 7, type: 'page', at: [3, 0, 0], page: 0 }],
};
// Pages turned by the input player (the first turn makes it: an entity id and
// shared random values, which the items dropped before and after show), on a
// written and a writable book: pages in range, the page already open, one
// past the end, a lectern without a book, and turns a tick apart, which pulse
// the lectern again while it is powered.
const lecternPagesRequest = {
  ...importedLecternRequest, samples: sequence(60), item_entities: true,
  actions: [
    { tick: 1, type: 'drop', at: [2.5, 3, 1.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 1 },
    { tick: 2, type: 'page', at: [0, 1, 0], page: 14 }, { tick: 3, type: 'drop', at: [2.5, 3, 2.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 2 },
    { tick: 10, type: 'page', at: [0, 1, 0], page: 3 }, { tick: 11, type: 'page', at: [0, 1, 0], page: 4 }, { tick: 20, type: 'page', at: [0, 1, 0], page: 4 },
    { tick: 21, type: 'page', at: [0, 1, 0], page: 15 }, { tick: 22, type: 'page', at: [0, 1, 1], page: 0 },
    { tick: 25, type: 'page', at: [0, 1, 2], page: 0 }, { tick: 26, type: 'page', at: [0, 1, 2], page: 2 }, { tick: 40, type: 'page', at: [0, 1, 2], page: 3 },
  ],
};

// ---- lectern-book: book inputs on saved data of the server's own version. Lecterns with a written
// book, a writable one, none, three writable books in one stack and a damaged written book, each read by
// a comparator; a sculk sensor beside the first hears its block changes (by the input player when a book
// is put back, by no entity when the menu takes one). Books are taken through the lectern's menu into the
// input player's inventory and put back; the stack of three spreads over three slots, which fails and
// leaves the inventory full for the inputs after it.
const lecternBookWorld = () => {
  const world = new World();
  ground(world, 0, 0, 4, 10);
  for (const z of [0, 2, 4, 6, 8, 10]) {
    world.set(0, 1, z, 'minecraft:lectern', { facing: 'north', has_book: z === 4 ? 'false' : 'true', powered: 'false' });
    world.set(1, 1, z, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
    wire(world, 2, 1, z);
    lampAfter(world, 3, 1, z);
  }
  world.set(0, 1, 1, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.finalize();
  return world;
};
const writtenBook = (count, extra = '') => `{id:"minecraft:written_book",count:${count},components:{${extra}"minecraft:written_book_content":{title:"Levels",author:"Fixture",pages:[${pages(15)}]}}}`;
const writableBook = (count) => `{id:"minecraft:writable_book",count:${count},components:{"minecraft:writable_book_content":{pages:["a","b","c"]}}}`;
const lecternBookProbes = [0, 2, 4, 6, 8, 10].flatMap((z) => [[0, 1, z], [1, 1, z], [3, 1, z]]).concat([[0, 1, 1]]);
const lecternBookRequest = {
  probes: lecternBookProbes, world_inventories: true, item_entities: true, samples: sequence(70), settle_ticks: 2,
  block_entities: [
    blockEntity([0, 1, 0], 'lectern', `Page:7,Book:${writtenBook(1)}`),
    blockEntity([0, 1, 2], 'lectern', `Page:2,Book:${writableBook(1)}`),
    blockEntity([0, 1, 6], 'lectern', `Page:1,Book:${writableBook(3)}`),
    blockEntity([0, 1, 8], 'lectern', `Page:4,Book:${writtenBook(1, '"minecraft:max_stack_size":1,"minecraft:max_damage":10,"minecraft:damage":3,')}`),
    blockEntity([0, 1, 10], 'lectern', `Page:0,Book:${writtenBook(5)}`),
  ],
  actions: [
    { tick: 1, type: 'drop', at: [2.5, 3, 1.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 1 },
    { tick: 2, type: 'book', at: [0, 1, 0], present: false },
    { tick: 3, type: 'drop', at: [2.5, 3, 2.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 2 },
    { tick: 4, type: 'page', at: [0, 1, 0], page: 3 },
    { tick: 5, type: 'book', at: [0, 1, 0], present: false },
    { tick: 6, type: 'book', at: [0, 1, 2], present: false },
    { tick: 10, type: 'book', at: [0, 1, 0], present: true },
    { tick: 12, type: 'page', at: [0, 1, 0], page: 14 },
    { tick: 14, type: 'book', at: [0, 1, 0], present: true },
    { tick: 16, type: 'book', at: [0, 1, 0], present: false },
    { tick: 18, type: 'book', at: [0, 1, 0], present: false },
    { tick: 20, type: 'book', at: [0, 1, 4], present: true },
    { tick: 21, type: 'book', at: [0, 1, 4], present: false },
    { tick: 22, type: 'book', at: [3, 0, 0], present: true },
    { tick: 24, type: 'book', at: [0, 1, 2], present: true },
    { tick: 25, type: 'book', at: [0, 1, 8], present: false },
    { tick: 26, type: 'book', at: [0, 1, 10], present: false },
    { tick: 28, type: 'book', at: [0, 1, 10], present: true },
    { tick: 30, type: 'book', at: [0, 1, 8], present: true },
    { tick: 32, type: 'book', at: [0, 1, 6], present: false },
    { tick: 34, type: 'book', at: [0, 1, 0], present: true },
    { tick: 36, type: 'book', at: [0, 1, 0], present: false },
    { tick: 38, type: 'book', at: [0, 1, 6], present: true },
    { tick: 40, type: 'page', at: [0, 1, 2], page: 2 },
  ],
};
// Books supplied (saved in the server's own data version) to empty lecterns: a written book whose pages
// are not resolved (Paper resolves no selectors in books), a writable one, and the supplies that fail: a
// future data version, a lectern with a book, a stack of two, an ID that does not match the stack, a
// lectern whose state has a book its block entity does not hold, and a written book without content,
// which the lectern takes but holds as no book.
const lecternSupplyWorld = () => {
  const world = new World();
  ground(world, 0, 0, 4, 6);
  for (const z of [0, 2, 4, 6]) {
    world.set(0, 1, z, 'minecraft:lectern', { facing: 'north', has_book: z === 4 ? 'true' : 'false', powered: 'false' });
    world.set(1, 1, z, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
    wire(world, 2, 1, z);
    lampAfter(world, 3, 1, z);
  }
  world.set(0, 1, 1, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.finalize();
  return world;
};
const supplyNow = (tick, z, item, count, snbt, version = NOW) => ({ tick, type: 'book', at: [0, 1, z], present: true, item, count, stack: { data_version: version, snbt } });
const unresolvedBook = `{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"Levels",author:"Fixture",pages:[${sequence(15).map((index) => `{text:"${index}"}`).join(',')}],resolved:false}}}`;
const lecternSupplyNowRequest = {
  probes: [0, 2, 4, 6].flatMap((z) => [[0, 1, z], [1, 1, z], [3, 1, z]]).concat([[0, 1, 1]]), world_inventories: true, samples: sequence(60), settle_ticks: 2,
  actions: [
    supplyNow(1, 0, 'minecraft:written_book', 1, unresolvedBook, 99999),
    supplyNow(2, 0, 'minecraft:written_book', 1, unresolvedBook),
    supplyNow(3, 0, 'minecraft:written_book', 1, unresolvedBook),
    supplyNow(4, 2, 'minecraft:written_book', 1, writtenBook(2)),
    supplyNow(6, 2, 'minecraft:writable_book', 1, writtenBook(1)),
    supplyNow(7, 2, 'minecraft:writable_book', 1, writableBook(1)),
    supplyNow(8, 4, 'minecraft:written_book', 1, writtenBook(1)),
    { tick: 10, type: 'page', at: [0, 1, 0], page: 14 },
    { tick: 15, type: 'book', at: [0, 1, 0], present: false },
    supplyNow(20, 0, 'minecraft:written_book', 1, writtenBook(1)),
    { tick: 25, type: 'book', at: [0, 1, 0], present: true },
    supplyNow(30, 6, 'minecraft:written_book', 1, '{id:"minecraft:written_book",count:1}'),
    { tick: 32, type: 'book', at: [0, 1, 6], present: false },
    supplyNow(34, 6, 'minecraft:written_book', 1, writtenBook(1)),
    { tick: 36, type: 'book', at: [0, 1, 2], present: false },
    { tick: 40, type: 'book', at: [0, 1, 2], present: true },
  ],
};

// Signs, a banner, a furnace burning down, a brewing stand, a bookshelf and a jukebox, as saved.
const importedMiscWorld = () => {
  const world = new World();
  ground(world, 0, 0, 12, 4);
  world.set(0, 1, 0, 'minecraft:oak_sign', { rotation: '0', waterlogged: 'false' });
  world.set(2, 1, 0, 'minecraft:oak_hanging_sign', { rotation: '0', attached: 'false', waterlogged: 'false' });
  world.set(4, 1, 0, 'minecraft:white_banner', { rotation: '0' });
  world.set(0, 1, 2, 'minecraft:furnace', { facing: 'north', lit: 'false' });
  world.set(1, 1, 2, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  wire(world, 2, 1, 2);
  lampAfter(world, 3, 1, 2);
  world.set(4, 1, 2, 'minecraft:blast_furnace', { facing: 'north', lit: 'false' });
  world.set(6, 1, 2, 'minecraft:brewing_stand', { has_bottle_0: 'false', has_bottle_1: 'false', has_bottle_2: 'false' });
  world.set(7, 1, 2, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  wire(world, 8, 1, 2);
  lampAfter(world, 9, 1, 2);
  world.set(6, 1, 0, 'minecraft:chiseled_bookshelf', { facing: 'north', slot_0_occupied: 'false', slot_1_occupied: 'false', slot_2_occupied: 'false', slot_3_occupied: 'false', slot_4_occupied: 'false', slot_5_occupied: 'false' });
  world.set(7, 1, 0, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  wire(world, 8, 1, 0);
  lampAfter(world, 9, 1, 0);
  world.set(10, 1, 0, 'minecraft:jukebox', { has_record: 'false' });
  world.set(11, 1, 0, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  world.set(10, 1, 2, 'minecraft:smoker', { facing: 'north', lit: 'false' });
  world.finalize();
  return world;
};
const sign = (extra) => `front_text:{messages:["Hello","","world",""],color:"red",has_glowing_text:1b},back_text:{messages:["","","",""]},is_waxed:1b${extra || ''}`;
const importedMiscRequest = {
  probes: [[0, 1, 0], [2, 1, 0], [4, 1, 0], [0, 1, 2], [3, 1, 2], [4, 1, 2], [6, 1, 2], [9, 1, 2], [9, 1, 0], [6, 1, 0], [10, 1, 0], [10, 1, 2]],
  inventory_probes: [[0, 1, 2], [4, 1, 2], [6, 1, 2], [6, 1, 0], [10, 1, 0]], inventory_missing: 'report', world_inventories: true, samples: sequence(40), settle_ticks: 2,
  block_entities: [
    blockEntity([0, 1, 0], 'sign', sign()),
    blockEntity([2, 1, 0], 'hanging_sign', 'front_text:{messages:["a","b","c","d"],filtered_messages:["a","","c","d"],color:"blue"},is_waxed:0b'),
    blockEntity([4, 1, 0], 'banner', 'CustomName:"Flag"'),
    blockEntity([0, 1, 2], 'furnace', `lit_time_remaining:5s,lit_total_time:200s,cooking_time_spent:30s,cooking_total_time:200s,Items:[${slotted(1, 'coal', 8)},${slotted(2, 'iron_ingot', 20)}]`),
    blockEntity([4, 1, 2], 'blast_furnace', `cooking_time_spent:100s,cooking_total_time:100s,lit_time_remaining:0s,Items:[${slotted(2, 'gold_ingot', 3)}]`),
    blockEntity([6, 1, 2], 'brewing_stand', `BrewTime:50s,Fuel:12b,Items:[${slotted(0, 'glass_bottle', 1)},${slotted(2, 'glass_bottle', 1)},${slotted(4, 'blaze_powder', 3)}]`),
    blockEntity([6, 1, 0], 'chiseled_bookshelf', `last_interacted_slot:3,Items:[${slotted(0, 'book', 1)},${slotted(3, 'written_book', 1, '{"minecraft:written_book_content":{title:"T",author:"A"}}')}]`),
    blockEntity([10, 1, 0], 'jukebox', 'ticks_since_song_started:5L'),
    blockEntity([10, 1, 2], 'smoker', `Items:[${slotted(0, 'beef', 5)}]`),
  ],
  actions: [{ tick: 3, type: 'extract', at: [0, 1, 0], slot: 2, count: 1 }],
};

// Item entities, loaded as saved: falling into a hopper and a chest, over an explosion that destroys a shulker box.
const importedEntitiesWorld = () => {
  const world = new World();
  ground(world, 0, 0, 12, 6);
  world.set(0, 1, 0, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  world.set(0, 0, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(1, 1, 0, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  world.set(6, 1, 3, 'minecraft:tnt');
  world.set(6, 1, 4, 'minecraft:redstone_block');
  world.finalize();
  return world;
};
const itemTag = (n, pos, item, extra) => ({ at: [pos[0] + 0.5, pos[1] + 0.25, pos[2] + 0.5], data_version: NOW, snbt: `{id:"minecraft:item",UUID:[I;${n},${n + 1},${n + 2},${n + 3}],Pos:[${pos[0] + 0.5}d,${pos[1] + 0.25}d,${pos[2] + 0.5}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],Item:${item},Age:0s,PickupDelay:0s${extra ? `,${extra}` : ''}}` });
const importedEntitiesRequest = {
  probes: [[0, 1, 0], [0, 0, 0], [1, 1, 0], [6, 1, 3]], inventory_probes: [[0, 1, 0], [0, 0, 0]], inventory_missing: 'report', world_inventories: true, item_entities: true, samples: sequence(80), settle_ticks: 1,
  entities: [
    itemTag(100, [0, 4, 0], '{id:"minecraft:diamond",count:7,components:{"minecraft:custom_name":"Gem"}}'),
    itemTag(200, [0, 3, 0], '{id:"minecraft:diamond",count:7}'),
    itemTag(300, [1, 2, 0], '{id:"minecraft:cobblestone",count:64}', 'NoGravity:1b,Motion:[0.0d,0.0d,0.0d]'),
    itemTag(400, [6, 2, 3], '{id:"minecraft:shulker_box",count:1,components:{"minecraft:container":[{slot:0,item:{id:"minecraft:stone",count:4}},{slot:5,item:{id:"minecraft:apple",count:2}}]}}'),
    itemTag(500, [7, 2, 3], '{id:"minecraft:bundle",count:1,components:{"minecraft:bundle_contents":[{id:"minecraft:redstone",count:5}]}}'),
    itemTag(600, [5, 2, 3], '{id:"minecraft:emerald",count:3}', 'Invulnerable:1b'),
    itemTag(700, [2, 2, 3], '{id:"minecraft:emerald",count:3}', 'Owner:[I;9,9,9,9]'),
    itemTag(800, [2, 2, 3], '{id:"minecraft:emerald",count:3}', 'Owner:[I;9,9,9,9]'),
    itemTag(900, [3, 2, 3], '{id:"minecraft:emerald",count:3}'),
  ],
};

// Item frames: comparators reading them, TNT blowing up some, one with no wall
// (it breaks when the frame next checks), a fixed one, an invisible one.
const FRAME_FACING = { north: [0, -1, 2, 180], south: [0, 1, 3, 0], west: [-1, 0, 4, 90], east: [1, 0, 5, 270] };
const frameTag = (n, block, facing, item, extra = '', id = 'item_frame') => {
  const [dx, dz, facingId, yaw] = FRAME_FACING[facing];
  const pos = [block[0] + 0.5 - dx * 0.46875, block[1] + 0.5, block[2] + 0.5 - dz * 0.46875];
  const itemField = item ? `Item:${item},` : '';
  return { at: pos, data_version: NOW, snbt: `{id:"minecraft:${id}",UUID:[I;${n},${n + 1},${n + 2},${n + 3}],Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[${yaw}.0f,0.0f],block_pos:[I;${block[0]},${block[1]},${block[2]}],Facing:${facingId}b,${itemField}ItemDropChance:1.0f${extra ? `,${extra}` : ''}}` };
};
const importedFramesWorld = () => {
  const world = new World();
  ground(world, 0, 0, 14, 10);
  // Each comparator reads the frame behind the stone in front of it, shows it
  // on a wire behind, and a lever beside it wakes it.
  for (const z of [2, 4, 6, 8]) {
    world.set(1, 1, z, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
    world.set(2, 1, z, 'minecraft:stone');
    world.set(0, 1, z, 'minecraft:redstone_wire');
    world.set(1, 1, z + 1, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' });
  }
  // The TNT with its frames: walls for each.
  world.set(8, 1, 5, 'minecraft:tnt');
  world.set(8, 1, 6, 'minecraft:redstone_block');
  world.set(9, 1, 2, 'minecraft:stone');
  world.set(5, 1, 5, 'minecraft:stone');
  world.set(11, 1, 5, 'minecraft:stone');
  world.set(8, 1, 9, 'minecraft:stone');
  world.set(12, 0, 1, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  world.finalize();
  return world;
};
const importedFramesRequest = {
  probes: [[1, 1, 2], [1, 1, 4], [1, 1, 6], [1, 1, 8], [0, 1, 2], [0, 1, 4], [0, 1, 6], [0, 1, 8], [8, 1, 5]], world_inventories: true, item_entities: true, samples: sequence(130), settle_ticks: 1,
  actions: [2, 4, 6, 8].map((z, index) => ({ tick: 2 + index, type: 'toggle', at: [1, 1, z + 1] })),
  entities: [
    frameTag(100, [3, 1, 2], 'east', '{id:"minecraft:diamond",count:3}', 'ItemRotation:3b'),
    frameTag(200, [3, 1, 4], 'east', null),
    frameTag(300, [3, 1, 6], 'east', '{id:"minecraft:cobblestone",count:1,components:{"minecraft:custom_name":"Rock"}}', 'ItemRotation:7b,Fixed:1b', 'glow_item_frame'),
    frameTag(400, [3, 1, 8], 'east', '{id:"minecraft:shulker_box",count:1,components:{"minecraft:container":[{slot:0,item:{id:"minecraft:stone",count:4}}]}}', 'ItemRotation:12b,Invisible:1b'),
    frameTag(500, [8, 1, 2], 'north', '{id:"minecraft:emerald",count:1}'),
    frameTag(600, [6, 1, 5], 'west', '{id:"minecraft:apple",count:1}', 'Fixed:1b'),
    frameTag(700, [10, 1, 5], 'east', '{id:"minecraft:bundle",count:1,components:{"minecraft:bundle_contents":[{id:"minecraft:redstone",count:5}]}}'),
    frameTag(800, [8, 1, 8], 'south', '{id:"minecraft:book",count:1}', 'ItemDropChance:0.0f'),
    frameTag(900, [12, 1, 1], 'east', '{id:"minecraft:gold_ingot",count:2}'),
  ],
};

// TNT under water or in the open, each next to a frame: an explosion whose
// source is in water leaves the frame be.
const importedFramesWaterWorld = (wet) => () => {
  const world = new World();
  ground(world, 0, 0, 12, 6);
  world.set(4, 1, 3, 'minecraft:tnt');
  if (wet) world.set(4, 2, 3, 'minecraft:water', { level: '0' });
  world.set(4, 1, 4, 'minecraft:redstone_block');
  world.set(3, 1, 2, 'minecraft:dirt');
  world.finalize();
  return world;
};
const importedFramesWaterRequest = {
  probes: [[3, 1, 2], [4, 1, 3]], world_inventories: true, item_entities: true, samples: sequence(100), settle_ticks: 1,
  entities: [frameTag(100, [4, 1, 2], 'east', '{id:"minecraft:clock",count:1}')],
};

// Stacks the inputs supply: with components, and every way one is turned down.
const suppliedWorld = () => {
  const world = new World();
  ground(world, 0, 0, 6, 2);
  world.set(0, 1, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(1, 1, 0, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  world.set(2, 1, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.finalize();
  return world;
};
const supplyAt = (tick, slot, id, count, snbt, version = NOW, target = [0, 1, 0]) => ({ tick, type: 'insert', at: target, slot, item: `minecraft:${id}`, count, stack: { data_version: version, snbt } });
const suppliedRequest = {
  probes: [[0, 1, 0], [1, 1, 0], [2, 1, 0]], inventory_probes: [[0, 1, 0], [2, 1, 0]], item_entities: true, world_inventories: true, samples: sequence(24), settle_ticks: 1,
  actions: [
    supplyAt(1, 0, 'diamond', 2, '{id:"minecraft:diamond",count:2,components:{"minecraft:custom_name":"Token A","minecraft:rarity":"epic"}}'),
    supplyAt(2, 0, 'diamond', 2, '{id:"minecraft:diamond",count:2,components:{"minecraft:custom_name":"Token A","minecraft:rarity":"epic"}}'),
    supplyAt(3, 0, 'diamond', 1, '{id:"minecraft:diamond",count:1}'),
    supplyAt(4, 1, 'diamond_sword', 1, '{id:"minecraft:diamond_sword",count:1,components:{"minecraft:damage":9,"minecraft:enchantments":{"minecraft:sharpness":5,"minecraft:unbreaking":3}}}'),
    supplyAt(5, 2, 'shulker_box', 1, '{id:"minecraft:shulker_box",count:1,components:{"minecraft:container":[{slot:1,item:{id:"minecraft:stone",count:9}},{slot:0,item:{id:"minecraft:bundle",count:1,components:{"minecraft:bundle_contents":[{id:"minecraft:apple",count:2}]}}}]}}'),
    supplyAt(6, 3, 'stone', 1, '{id:"minecraft:dirt",count:1}'),
    supplyAt(7, 3, 'stone', 2, '{id:"minecraft:stone",count:1}'),
    supplyAt(8, 3, 'stone', 1, '{id:"minecraft:stone",count:1,components:{"minecraft:max_stack_size":7}}'),
    supplyAt(9, 4, 'stone', 1, '{id:"minecraft:stone",count:1}', 999999),
    supplyAt(10, 4, 'stone', 8, '{id:"minecraft:stone",count:8,components:{"minecraft:max_stack_size":7}}'),
    { tick: 12, type: 'insert', at: [0, 1, 0], slot: 5, item: 'minecraft:nonsense', count: 1 },
    supplyAt(13, 5, 'written_book', 1, '{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"X",author:"Y",pages:["p1","p2"]}}}'),
    supplyAt(14, 0, 'stick', 1, '{id:"minecraft:stick",count:1,components:{"minecraft:rarity":"rare"}}', NOW, [2, 1, 0]),
    { tick: 15, type: 'drop', at: [3.5, 3, 1.5], velocity: [0, 0, 0], item: 'minecraft:potion', count: 1 },
    { tick: 15, type: 'drop', at: [3.5, 3, 1.5], velocity: [0.1, 0, 0], item: 'minecraft:emerald', count: 5, stack: { data_version: NOW, snbt: '{id:"minecraft:emerald",count:5,components:{"minecraft:custom_name":"E","minecraft:lore":["l1","l2"]}}' } },
    { tick: 16, type: 'drop', at: [4.5, 3, 1.5], velocity: [0, 0, 0], item: 'minecraft:emerald', count: 6, stack: { data_version: NOW, snbt: '{id:"minecraft:emerald",count:5}' } },
    { tick: 16, type: 'drop', at: [4.5, 3, 1.5], velocity: [0, 0, 0], item: 'minecraft:stone', count: 1, stack: { data_version: NOW, snbt: '{id:"minecraft:emerald",count:1}' } },
    { tick: 17, type: 'extract', at: [0, 1, 0], slot: 0, count: 1 },
    { tick: 18, type: 'drain', at: [0, 1, 0] },
  ],
};

// ---- vehicles: minecarts on rails (powered, detector, activator), as saved

const cartTag = (n, pos, id = 'minecart', extra = '') => ({
  at: pos, data_version: NOW,
  snbt: `{id:"minecraft:${id}",UUID:[I;${n},${n + 1},${n + 2},${n + 3}],Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f]${extra ? `,${extra}` : ''}}`,
});
// A rectangular loop of rail at y = 1 from (x0, z0) to (x1, z1), the corners shaped, plain rails along the edges.
const railLoop = (world, x0, z0, x1, z1) => {
  for (let x = x0 + 1; x < x1; x++) { world.set(x, 1, z0, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' }); world.set(x, 1, z1, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' }); }
  for (let z = z0 + 1; z < z1; z++) { world.set(x0, 1, z, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' }); world.set(x1, 1, z, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' }); }
  world.set(x0, 1, z0, 'minecraft:rail', { shape: 'south_east', waterlogged: 'false' });
  world.set(x1, 1, z0, 'minecraft:rail', { shape: 'south_west', waterlogged: 'false' });
  world.set(x0, 1, z1, 'minecraft:rail', { shape: 'north_east', waterlogged: 'false' });
  world.set(x1, 1, z1, 'minecraft:rail', { shape: 'north_west', waterlogged: 'false' });
};
const poweredRail = (world, x, y, z, shape, powered = 'false') => world.set(x, y, z, 'minecraft:powered_rail', { shape, powered, waterlogged: 'false' });
const detectorRail = (world, x, y, z, shape) => world.set(x, y, z, 'minecraft:detector_rail', { shape, powered: 'false', waterlogged: 'false' });
const activatorRail = (world, x, y, z, shape, powered = 'false') => world.set(x, y, z, 'minecraft:activator_rail', { shape, powered, waterlogged: 'false' });

// A loop with a booster, a detector rail whose comparator reads a chest minecart, an activator rail and carts of several kinds.
const cartLoopWorld = () => {
  const world = new World();
  ground(world, -2, -2, 16, 12);
  railLoop(world, 0, 0, 12, 8);
  // The booster on the top edge, powered by a block under a torch... a redstone block beside it.
  poweredRail(world, 6, 1, 0, 'east_west');
  world.set(6, 1, -1, 'minecraft:redstone_block');
  // The detector on the right edge with a comparator and a lamp beside it.
  detectorRail(world, 12, 1, 4, 'north_south');
  world.set(13, 1, 4, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
  wire(world, 14, 1, 4);
  lampAfter(world, 15, 1, 4);
  // The activator on the bottom edge.
  activatorRail(world, 6, 1, 8, 'east_west');
  world.set(6, 1, 9, 'minecraft:redstone_block');
  // A hopper under the left edge reading nothing, a chest.
  world.finalize();
  return world;
};
const cartLoopRequest = {
  probes: [[12, 1, 4], [13, 1, 4], [14, 1, 4], [15, 1, 4], [6, 1, 0], [6, 1, 8]], world_inventories: true, item_entities: true, samples: sequence(120), settle_ticks: 1,
  entities: [
    cartTag(100, [3.5, 1.0625, 0], 'minecart', 'Motion:[0.3d,0.0d,0.0d]'),
    cartTag(200, [9.5, 1.0625, 8], 'chest_minecart', `Motion:[-0.2d,0.0d,0.0d],Items:[${slotted(0, 'cobblestone', 64)},${slotted(5, 'diamond', 3)}]`),
    cartTag(300, [0, 1.0625, 4.5], 'furnace_minecart', 'Fuel:200s,PushX:0.0d,PushZ:1.0d'),
  ],
};

// Straight tracks along x, one per scenario (z = 0, 4, 8, ...): a detector rail with a comparator that reads a chest cart, carts
// meeting head on, a furnace cart pushing, a track that ends, one that climbs and one that dips into water and lava.
const cartTracksWorld = () => {
  const world = new World();
  ground(world, -3, -3, 24, 40);
  const line = (z, x0, x1, shape = 'east_west') => { for (let x = x0; x <= x1; x++) world.set(x, 1, z, 'minecraft:rail', { shape, waterlogged: 'false' }); };
  // 0: a booster, then a detector with a comparator behind it, a lamp.
  line(0, 0, 20);
  poweredRail(world, 2, 1, 0, 'east_west');
  world.set(2, 1, 1, 'minecraft:redstone_block');
  detectorRail(world, 10, 1, 0, 'east_west');
  world.set(10, 1, 1, 'minecraft:comparator', { facing: 'south', mode: 'compare', powered: 'false' });
  world.set(10, 1, 2, 'minecraft:redstone_lamp', { lit: 'false' });
  detectorRail(world, 14, 1, 0, 'east_west');
  world.set(14, 1, -1, 'minecraft:comparator', { facing: 'north', mode: 'compare', powered: 'false' });
  wire(world, 14, 1, -2);
  // 4: carts meeting.
  line(4, 0, 20);
  // 8: a furnace cart pushing from behind a stopped cart, a powered rail that is off.
  line(8, 0, 20);
  poweredRail(world, 12, 1, 8, 'east_west');
  // 12: a track that ends at x = 6.
  line(12, 0, 6);
  // 16: a climb: rails up and down.
  for (let x = 0; x <= 3; x++) world.set(x, 1, 16, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' });
  world.set(4, 1, 16, 'minecraft:rail', { shape: 'ascending_east', waterlogged: 'false' });
  world.set(5, 2, 16, 'minecraft:rail', { shape: 'ascending_east', waterlogged: 'false' });
  world.set(6, 3, 16, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' });
  world.set(7, 3, 16, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' });
  world.set(8, 3, 16, 'minecraft:rail', { shape: 'ascending_west', waterlogged: 'false' });
  world.set(9, 2, 16, 'minecraft:rail', { shape: 'ascending_west', waterlogged: 'false' });
  line(16, 10, 20);
  for (const x of [4, 5]) world.set(x, 1, 16, x === 4 ? 'minecraft:rail' : 'minecraft:stone', x === 4 ? { shape: 'ascending_east', waterlogged: 'false' } : {});
  world.set(6, 2, 16, 'minecraft:stone');
  world.set(7, 2, 16, 'minecraft:stone');
  world.set(8, 2, 16, 'minecraft:stone');
  world.set(5, 1, 16, 'minecraft:stone');
  // 20: rails across water and lava.
  line(20, 0, 20);
  world.set(8, 1, 20, 'minecraft:water', { level: '0' });
  world.set(9, 1, 20, 'minecraft:water', { level: '0' });
  world.set(15, 1, 20, 'minecraft:lava', { level: '0' });
  // 24: the activator rail and a powered rail in a row for a hopper cart, a chest above.
  line(24, 0, 20);
  activatorRail(world, 8, 1, 24, 'east_west');
  world.set(8, 1, 25, 'minecraft:redstone_block');
  world.finalize();
  return world;
};
const cartTracksRequest = {
  probes: [[10, 1, 0], [10, 1, 1], [10, 1, 2], [14, 1, 0], [14, 1, -1], [14, 1, -2], [8, 1, 24], [2, 1, 0]], world_inventories: true, item_entities: true, samples: sequence(140), settle_ticks: 1,
  entities: [
    cartTag(100, [4.5, 1.0625, 0], 'chest_minecart', `Motion:[0.35d,0.0d,0.0d],Items:[${slotted(0, 'cobblestone', 64)},${slotted(1, 'diamond', 3)},${slotted(26, 'stick', 7)}]`),
    cartTag(200, [0.5, 1.0625, 4], 'minecart', 'Motion:[0.4d,0.0d,0.0d]'),
    cartTag(300, [19.5, 1.0625, 4], 'furnace_minecart', 'Motion:[-0.3d,0.0d,0.0d],Fuel:600s,PushX:-1.0d'),
    cartTag(400, [6.5, 1.0625, 8], 'minecart'),
    cartTag(500, [1.5, 1.0625, 8], 'furnace_minecart', 'Fuel:2000s,PushX:1.0d,PushZ:0.0d'),
    cartTag(600, [1.5, 1.0625, 12], 'minecart', 'Motion:[0.4d,0.0d,0.0d]'),
    cartTag(700, [1.5, 1.0625, 16], 'minecart', 'Motion:[0.4d,0.0d,0.0d]'),
    cartTag(800, [3.5, 1.0625, 20], 'minecart', 'Motion:[0.35d,0.0d,0.0d]'),
    cartTag(900, [4.5, 1.0625, 20], 'chest_minecart', 'Motion:[0.35d,0.0d,0.0d]'),
    cartTag(1000, [1.5, 1.0625, 24], 'hopper_minecart', `Motion:[0.3d,0.0d,0.0d],Items:[${slotted(1, 'iron_ingot', 5)}]`),
    cartTag(1100, [12.5, 1.0625, 24], 'minecart', 'Motion:[-0.3d,0.0d,0.0d]'),
  ],
};

// Hopper minecarts: under a chest, over item drops, fed by a hopper block, drained into a hopper block under the rail, disabled by an
// activator rail; chest carts as sources and destinations of hoppers and a dropper.
const cartHoppersWorld = () => {
  const world = new World();
  ground(world, -3, -3, 24, 30);
  const line = (z, x0, x1) => { for (let x = x0; x <= x1; x++) world.set(x, 1, z, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' }); };
  // 0: a chest above the rail at x = 6 holding stuff; a hopper block beside the rail at x = 12 pushing into the rail's cart, fed by a chest.
  line(0, 0, 20);
  world.set(6, 2, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(12, 1, 1, 'minecraft:hopper', { facing: 'north', enabled: 'true' });
  world.set(12, 2, 1, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  // 4: a hopper block under the rail at x = 8 taking from the cart above, a comparator on it.
  line(4, 0, 20);
  world.set(8, 0, 4, 'minecraft:hopper', { facing: 'south', enabled: 'true' });
  world.set(8, 0, 5, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  // 8: an activator rail over the middle, a redstone block under... beside it.
  line(8, 0, 20);
  activatorRail(world, 8, 1, 8, 'east_west');
  world.set(8, 1, 9, 'minecraft:redstone_block');
  // 12: a dropper pushing into a chest cart in front of it, a hopper cart that a dropper fills.
  line(12, 0, 20);
  world.set(5, 1, 11, 'minecraft:dropper', { facing: 'south', triggered: 'false' });
  world.set(5, 1, 10, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' });
  // 16: a stopped hopper cart under a hopper block that drains into it, and an item that falls onto the cart.
  line(16, 0, 20);
  world.set(10, 2, 16, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  world.set(10, 3, 16, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.finalize();
  return world;
};
const cartHoppersRequest = {
  probes: [[6, 2, 0], [12, 1, 1], [12, 2, 1], [8, 0, 4], [8, 0, 5], [5, 1, 11], [10, 2, 16], [10, 3, 16]], inventory_probes: [[6, 2, 0], [12, 2, 1], [8, 0, 5], [12, 1, 1], [8, 0, 4], [5, 1, 11], [10, 2, 16], [10, 3, 16]],
  inventory_missing: 'report', world_inventories: true, item_entities: true, samples: sequence(120), settle_ticks: 1,
  actions: [{ tick: 3, type: 'toggle', at: [5, 1, 10] }, { tick: 20, type: 'toggle', at: [5, 1, 10] }, { tick: 22, type: 'toggle', at: [5, 1, 10] }],
  block_entities: [
    blockEntity([6, 2, 0], 'chest', `Items:[${slotted(0, 'cobblestone', 64)},${slotted(1, 'diamond', 3)},${slotted(2, 'stick', 9)},${slotted(26, 'redstone', 2)}]`),
    blockEntity([12, 2, 1], 'chest', `Items:[${slotted(0, 'emerald', 5)},${slotted(1, 'stick', 40)}]`),
    blockEntity([8, 0, 5], 'chest', ''),
    blockEntity([5, 1, 11], 'dropper', `Items:[${slotted(0, 'cobblestone', 20)},${slotted(1, 'apple', 7)}]`),
    blockEntity([10, 3, 16], 'chest', `Items:[${slotted(0, 'gold_ingot', 12)},${slotted(1, 'gold_ingot', 12, '{"minecraft:custom_name":"Pure"}')}]`),
    blockEntity([10, 2, 16], 'hopper', 'TransferCooldown:0'),
    blockEntity([12, 1, 1], 'hopper', 'TransferCooldown:0'),
    blockEntity([8, 0, 4], 'hopper', 'TransferCooldown:0'),
  ],
  entities: [
    cartTag(100, [3.5, 1.0625, 0], 'hopper_minecart', 'Motion:[0.25d,0.0d,0.0d]'),
    itemTag(150, [14, 1, 0], '{id:"minecraft:iron_ingot",count:4}'),
    cartTag(200, [4.5, 1.0625, 4], 'chest_minecart', `Motion:[0.25d,0.0d,0.0d],Items:[${slotted(0, 'cobblestone', 64)},${slotted(3, 'diamond', 3)}]`),
    cartTag(300, [2.5, 1.0625, 8], 'hopper_minecart', `Motion:[0.3d,0.0d,0.0d],Items:[${slotted(1, 'iron_ingot', 5)}]`),
    itemTag(350, [12, 1, 8], '{id:"minecraft:coal",count:2}'),
    cartTag(400, [5.5, 1.0625, 12], 'chest_minecart', ''),
    cartTag(500, [10.5, 1.0625, 16], 'hopper_minecart', `Items:[${slotted(0, 'gold_ingot', 63)}]`),
    itemTag(550, [14, 3, 16], '{id:"minecraft:coal",count:2}'),
  ],
};

// TNT minecarts: lit by an activator rail, going off by a bump or a fall, near other carts, a frame and blocks.
const cartTntWorld = () => {
  const world = new World();
  ground(world, -3, -3, 30, 24);
  const line = (z, x0, x1) => { for (let x = x0; x <= x1; x++) world.set(x, 1, z, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' }); };
  line(0, 0, 20);
  activatorRail(world, 6, 1, 0, 'east_west');
  world.set(6, 1, 1, 'minecraft:redstone_block');
  world.set(14, 1, 2, 'minecraft:stone_bricks');
  world.set(14, 2, 2, 'minecraft:stone_bricks');
  world.set(15, 1, 1, 'minecraft:oak_planks');
  line(6, 0, 12);
  world.set(13, 1, 6, 'minecraft:stone');
  world.set(13, 2, 6, 'minecraft:stone');
  line(12, 0, 4);
  world.set(5, 1, 12, 'minecraft:stone');
  world.set(2, 1, 14, 'minecraft:oak_planks');
  world.set(3, 1, 14, 'minecraft:oak_planks');
  world.set(2, 2, 14, 'minecraft:tnt');
  world.finalize();
  return world;
};
const cartTntRequest = {
  probes: [[14, 1, 2], [14, 2, 2], [15, 1, 1], [13, 1, 6], [5, 1, 12], [2, 2, 14]], world_inventories: true, item_entities: true, samples: sequence(160), settle_ticks: 1,
  entities: [
    cartTag(100, [2.5, 1.0625, 0], 'tnt_minecart', 'Motion:[0.3d,0.0d,0.0d]'),
    cartTag(200, [10.5, 1.0625, 0], 'chest_minecart', `Items:[${slotted(0, 'cobblestone', 64)}]`),
    cartTag(300, [16.5, 1.0625, 0], 'furnace_minecart', 'Fuel:100s'),
    cartTag(400, [2.5, 1.0625, 6], 'tnt_minecart', 'Motion:[0.4d,0.0d,0.0d]'),
    cartTag(500, [1.5, 1.0625, 12], 'tnt_minecart', 'Motion:[0.4d,0.0d,0.0d],fuse:20,explosion_power:2.0f,explosion_speed_factor:3.0f'),
    cartTag(600, [3.5, 1.0625, 12], 'minecart', ''),
    frameTag(700, [15, 1, 1], 'north', '{id:"minecraft:clock",count:1}'),
  ],
};

// Decorated pots and shelves as containers: inserts, extracts and drains (in
// and out of range, a different item, more than fits), hoppers that pull from
// them and feed them, comparators reading them into lamps (a shelf only from
// behind), a sculk sensor that hears a shelf change (block_activate), hopper
// minecarts under a pot and a shelf, a dropper into a shelf, and a saved pot
// and shelf with items.
const potShelfWorld = () => {
  const world = new World();
  world.fill([-1, 0, -3], [17, 0, 4], 'minecraft:stone');
  world.set(0, 2, 0, 'minecraft:decorated_pot', { cracked: 'false', facing: 'north', waterlogged: 'false' });
  world.set(0, 1, 0, 'minecraft:hopper', { facing: 'east', enabled: 'true' });
  world.set(1, 1, 0, 'minecraft:chest', { facing: 'north', type: 'single', waterlogged: 'false' });
  world.set(3, 1, 0, 'minecraft:decorated_pot', { cracked: 'true', facing: 'east', waterlogged: 'false' });
  world.set(3, 1, 1, 'minecraft:comparator', { facing: 'north', mode: 'compare', powered: 'false' });
  world.set(3, 1, 2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(6, 1, 0, 'minecraft:oak_shelf', { facing: 'south', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  world.set(6, 1, -1, 'minecraft:comparator', { facing: 'south', mode: 'compare', powered: 'false' });
  world.set(6, 1, -2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(6, 1, 1, 'minecraft:comparator', { facing: 'north', mode: 'compare', powered: 'false' });
  world.set(6, 1, 2, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(9, 2, 0, 'minecraft:spruce_shelf', { facing: 'north', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  world.set(9, 1, 0, 'minecraft:hopper', { facing: 'east', enabled: 'true' });
  world.set(10, 1, 0, 'minecraft:barrel', { facing: 'up', open: 'false' });
  world.set(8, 1, 3, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.set(12, 2, 0, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  world.set(12, 1, 0, 'minecraft:decorated_pot', { cracked: 'false', facing: 'south', waterlogged: 'false' });
  world.set(15, 2, 0, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  world.set(15, 1, 0, 'minecraft:bamboo_shelf', { facing: 'west', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  world.set(16, 1, 0, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  world.set(17, 1, 0, 'minecraft:redstone_lamp', { lit: 'false' });
  world.set(0, 1, 3, 'minecraft:decorated_pot', { cracked: 'false', facing: 'north', waterlogged: 'false' });
  world.set(2, 1, 3, 'minecraft:cherry_shelf', { facing: 'east', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  for (let x = 4; x <= 8; x++) world.set(x, 1, 4, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' });
  world.set(5, 2, 4, 'minecraft:decorated_pot', { cracked: 'false', facing: 'north', waterlogged: 'false' });
  world.set(7, 2, 4, 'minecraft:birch_shelf', { facing: 'south', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  world.set(13, 1, 3, 'minecraft:dropper', { facing: 'east', triggered: 'false' });
  world.set(13, 1, 2, 'minecraft:lever', { face: 'floor', facing: 'north', powered: 'false' });
  world.set(14, 1, 3, 'minecraft:pale_oak_shelf', { facing: 'west', powered: 'false', side_chain: 'unconnected', waterlogged: 'false' });
  world.finalize();
  return world;
};
const potShelfProbes = [[0, 2, 0], [0, 1, 0], [1, 1, 0], [3, 1, 0], [3, 1, 2], [6, 1, 0], [6, 1, -2], [6, 1, 2], [9, 2, 0], [10, 1, 0], [8, 1, 3], [12, 1, 0], [15, 1, 0], [17, 1, 0], [0, 1, 3], [2, 1, 3],
  [5, 2, 4], [7, 2, 4], [13, 1, 3], [14, 1, 3]];
const potShelfRequest = {
  probes: potShelfProbes, inventory_probes: [[0, 2, 0], [1, 1, 0], [3, 1, 0], [6, 1, 0], [9, 2, 0], [10, 1, 0], [12, 1, 0], [15, 1, 0], [0, 1, 3], [2, 1, 3], [5, 2, 4], [7, 2, 4], [13, 1, 3], [14, 1, 3]],
  inventory_missing: 'report', world_inventories: true, item_entities: true, samples: sequence(61), settle_ticks: 1,
  block_entities: [
    blockEntity([0, 1, 3], 'decorated_pot', 'sherds:["minecraft:archer_pottery_sherd","minecraft:brick","minecraft:prize_pottery_sherd"],item:{id:"minecraft:diamond",count:3}'),
    blockEntity([2, 1, 3], 'shelf', 'Items:[{Slot:1b,id:"minecraft:apple",count:2},{Slot:2b,id:"minecraft:stick",count:64}],align_items_to_bottom:1b'),
    blockEntity([5, 2, 4], 'decorated_pot', 'item:{id:"minecraft:emerald",count:9}'),
    blockEntity([7, 2, 4], 'shelf', `Items:[${slotted(0, 'coal', 3)},${slotted(2, 'flint', 2)}]`),
    blockEntity([13, 1, 3], 'dropper', `Items:[${slotted(0, 'cobblestone', 20)},${slotted(4, 'apple', 3)}]`),
  ],
  entities: [
    cartTag(100, [5.5, 1.0625, 4.5], 'hopper_minecart', ''),
    cartTag(200, [7.5, 1.0625, 4.5], 'hopper_minecart', `Items:[${slotted(0, 'coal', 62)}]`),
  ],
  actions: [
    { tick: 2, type: 'insert', at: [0, 2, 0], slot: 0, item: 'minecraft:cobblestone', count: 5 },
    { tick: 2, type: 'insert', at: [0, 2, 0], slot: 1, item: 'minecraft:cobblestone', count: 1 },
    { tick: 3, type: 'insert', at: [3, 1, 0], slot: 0, item: 'minecraft:ender_pearl', count: 16 },
    { tick: 4, type: 'insert', at: [3, 1, 0], slot: 0, item: 'minecraft:ender_pearl', count: 1 },
    { tick: 4, type: 'insert', at: [3, 1, 0], slot: 0, item: 'minecraft:stone', count: 1 },
    { tick: 8, type: 'extract', at: [3, 1, 0], slot: 0, count: 6 },
    { tick: 3, type: 'insert', at: [6, 1, 0], slot: 0, item: 'minecraft:stone', count: 64 },
    { tick: 5, type: 'insert', at: [6, 1, 0], slot: 2, item: 'minecraft:egg', count: 3 },
    { tick: 5, type: 'insert', at: [6, 1, 0], slot: 3, item: 'minecraft:egg', count: 1 },
    { tick: 6, type: 'insert', at: [6, 1, 0], slot: 2, item: 'minecraft:egg', count: 14 },
    { tick: 12, type: 'extract', at: [6, 1, 0], slot: 0, count: 64 },
    { tick: 3, type: 'insert', at: [9, 2, 0], slot: 1, item: 'minecraft:redstone', count: 4 },
    { tick: 3, type: 'insert', at: [12, 2, 0], slot: 0, item: 'minecraft:gold_ingot', count: 10 },
    { tick: 3, type: 'insert', at: [15, 2, 0], slot: 0, item: 'minecraft:iron_ingot', count: 7 },
    { tick: 20, type: 'drain', at: [3, 1, 0] }, { tick: 25, type: 'drain', at: [6, 1, 0] }, { tick: 40, type: 'drain', at: [0, 1, 3] },
    { tick: 41, type: 'extract', at: [2, 1, 3], slot: 2, count: 10 },
    { tick: 3, type: 'toggle', at: [13, 1, 2] }, { tick: 6, type: 'toggle', at: [13, 1, 2] }, { tick: 9, type: 'toggle', at: [13, 1, 2] }, { tick: 12, type: 'toggle', at: [13, 1, 2] },
    { tick: 15, type: 'toggle', at: [13, 1, 2] }, { tick: 18, type: 'toggle', at: [13, 1, 2] }, { tick: 21, type: 'toggle', at: [13, 1, 2] }, { tick: 24, type: 'toggle', at: [13, 1, 2] },
  ],
};
// ---- vehicles: boats on land and in water

const boatTag = (n, pos, id = 'oak_boat', extra = '') => cartTag(n, pos, id, extra);
// A pool of water with a floor, a flow into it, land beside it, a bubble column, a lily pad and a lava pool.
const boatPoolWorld = (lily) => () => {
  const world = new World();
  ground(world, -3, -3, 30, 24);
  // The pool: x 2..9, z 2..7 sunk one block (the ground at y = 0 stays under it): water at y = 1..2.
  for (let x = 2; x <= 9; x++) for (let z = 2; z <= 7; z++) { world.set(x, 1, z, 'minecraft:water', { level: '0' }); world.set(x, 2, z, 'minecraft:water', { level: '0' }); }
  // Walls.
  for (let x = 1; x <= 10; x++) for (const z of [1, 8]) { world.set(x, 1, z, 'minecraft:stone'); world.set(x, 2, z, 'minecraft:stone'); }
  for (let z = 1; z <= 8; z++) for (const x of [1, 10]) { world.set(x, 1, z, 'minecraft:stone'); world.set(x, 2, z, 'minecraft:stone'); }
  // A bubble column (soul sand under water) and a lily pad.
  world.set(5, 0, 5, 'minecraft:soul_sand');
  world.set(5, 1, 5, 'minecraft:bubble_column', { drag: 'false' });
  if (lily) world.set(4, 3, 3, 'minecraft:lily_pad');
  // Land: stairs and slabs for boats to meet.
  world.set(14, 1, 3, 'minecraft:stone');
  world.set(14, 1, 5, 'minecraft:ice');
  world.set(14, 1, 7, 'minecraft:packed_ice');
  world.set(14, 1, 9, 'minecraft:soul_sand');
  world.set(14, 1, 11, 'minecraft:blue_ice');
  // A lava pool.
  for (let x = 18; x <= 20; x++) for (let z = 2; z <= 4; z++) world.set(x, 1, z, 'minecraft:lava', { level: '0' });
  // A water source over a ledge and a flowing slope.
  world.set(24, 3, 3, 'minecraft:water', { level: '0' });
  world.set(24, 2, 3, 'minecraft:stone');
  world.finalize();
  return world;
};
const boatPoolRequest = {
  probes: [[5, 1, 5], [4, 3, 3], [24, 3, 3], [14, 1, 3]], world_inventories: true, item_entities: true, samples: sequence(140), settle_ticks: 1,
  entities: [
    boatTag(100, [3.5, 2.5, 4.5], 'oak_boat'),
    boatTag(200, [5.5, 3.0, 5.5], 'birch_boat', 'Motion:[0.0d,0.2d,0.0d]'),
    boatTag(300, [8.5, 2.0, 3.5], 'spruce_chest_boat', `Motion:[-0.1d,0.0d,0.05d],Items:[${slotted(0, 'cobblestone', 64)},${slotted(2, 'emerald', 3)}]`),
    boatTag(400, [14.5, 3.0, 3.5], 'bamboo_raft'),
    boatTag(500, [14.5, 3.0, 5.5], 'jungle_boat', 'Motion:[0.3d,0.0d,0.0d]'),
    boatTag(600, [14.5, 3.0, 7.5], 'acacia_boat', 'Motion:[0.0d,-0.5d,0.0d]'),
    boatTag(700, [14.5, 3.0, 9.5], 'dark_oak_boat'),
    boatTag(800, [14.5, 3.0, 11.5], 'cherry_boat', 'Motion:[0.1d,0.0d,0.1d]'),
    boatTag(900, [19.5, 3.0, 3.5], 'mangrove_boat'),
    boatTag(1000, [4.5, 3.0, 3.5], 'pale_oak_chest_boat'),
    boatTag(1100, [24.5, 4.0, 3.5], 'bamboo_chest_raft', `Items:[${slotted(5, 'diamond', 2)}]`),
    boatTag(1300, [14.0, 3.0, 3.5], 'oak_boat'),
  ],
};

// ---- vehicles: paintings on walls

const PAINTING_FACING = { north: [0, -1, 2], south: [0, 1, 0], west: [-1, 0, 1], east: [1, 0, 3] };
const PAINTING_CCW = { north: [-1, 0], west: [0, 1], south: [1, 0], east: [0, -1] };
const PAINTING_SIZE = { kebab: [1, 1], bomb: [1, 1], fighters: [4, 2], donkey_kong: [4, 3], skeleton: [4, 3], wither: [2, 2], sea: [2, 1], pool: [2, 1], backyard: [3, 4], graham: [1, 2], pigscene: [4, 4], bouquet: [3, 3] };
// A painting in the cell `cell` (the block in front of its wall), facing `facing`, of the variant: its Pos is the middle of its box, as the server computes it.
const paintingTag = (n, cell, facing, variant, extra = '') => {
  const [dx, dz, facingId] = PAINTING_FACING[facing];
  const [w, h] = PAINTING_SIZE[variant] || [1, 1];
  const [cx, cz] = PAINTING_CCW[facing];
  const offW = w % 2 === 0 ? 0.5 : 0.0;
  const offH = h % 2 === 0 ? 0.5 : 0.0;
  const centerOf = [cell[0] + 0.5, cell[1] + 0.5, cell[2] + 0.5];
  const c = [centerOf[0] + -0.46875 * dx + offW * cx, centerOf[1] + -0.46875 * 0 + offH * 1, centerOf[2] + -0.46875 * dz + offW * cz];
  const size = [dx !== 0 ? 0.0625 : w, h, dz !== 0 ? 0.0625 : w];
  const pos = c.map((v, i) => { const min = v - size[i] / 2.0; const max = v + size[i] / 2.0; return min + (max - min) * 0.5; });
  return { at: pos, data_version: NOW, snbt: `{id:"minecraft:painting",UUID:[I;${n},${n + 1},${n + 2},${n + 3}],Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],block_pos:[I;${cell[0]},${cell[1]},${cell[2]}],facing:${facingId}b,variant:"minecraft:${variant}"${extra ? `,${extra}` : ''}}` };
};
// A wall of stone with paintings of several sizes, one with a block missing behind it, TNT blowing up part of the wall, a comparator
// reading an item frame beside them, and a sculk sensor that hears them break.
const paintingWallWorld = () => {
  const world = new World();
  ground(world, -3, -3, 30, 24);
  // A hole behind the 2 x 2 painting at (4..5, 3..4): the painting pops at its next check.
  for (let x = 0; x <= 14; x++) for (let y = 1; y <= 6; y++) if (!(x === 5 && y === 3)) world.set(x, y, 5, 'minecraft:stone');
  // TNT in front of the wall at x = 10, with redstone.
  world.set(10, 1, 3, 'minecraft:tnt');
  world.set(10, 1, 2, 'minecraft:redstone_block');
  world.set(13, 1, 3, 'minecraft:stone');
  world.set(12, 1, 3, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  wire(world, 11, 1, 3);
  // Side walls for the east and west faces.
  for (let y = 1; y <= 4; y++) { world.set(18, y, 2, 'minecraft:stone'); world.set(18, y, 3, 'minecraft:stone'); world.set(18, y, 4, 'minecraft:stone'); world.set(22, y, 3, 'minecraft:stone'); }
  world.finalize();
  return world;
};
const paintingWallRequest = {
  probes: [[10, 1, 3], [13, 1, 3], [12, 1, 3], [11, 1, 3], [5, 3, 5]], item_entities: true, world_inventories: true, living_entities: true, samples: sequence(260), settle_ticks: 1,
  entities: [
    paintingTag(100, [1, 2, 4], 'north', 'kebab'),
    paintingTag(200, [5, 3, 4], 'north', 'wither'),
    paintingTag(300, [7, 3, 4], 'north', 'fighters'),
    paintingTag(400, [9, 3, 4], 'north', 'donkey_kong'),
    paintingTag(500, [12, 3, 4], 'north', 'sea'),
    paintingTag(600, [3, 5, 4], 'north', 'graham'),
    paintingTag(700, [17, 2, 3], 'west', 'pool'),
    paintingTag(800, [17, 2, 3], 'west', 'pool'),
    paintingTag(900, [21, 2, 3], 'east', 'pool'),
    paintingTag(1000, [0, 6, 4], 'north', 'does_not_exist'),
    frameTag(1100, [14, 2, 4], 'north', '{id:"minecraft:compass",count:1}'),
  ],
};

const paintingBareWorld = () => {
  const world = new World();
  ground(world, -3, -3, 12, 12);
  world.finalize();
  return world;
};
const paintingBareRequest = {
  probes: [[0, 1, 0]], item_entities: true, samples: sequence(130), settle_ticks: 1,
  entities: [paintingTag(100, [5, 1, 4], 'north', 'kebab'), paintingTag(200, [7, 1, 4], 'north', 'sea'), frameTag(300, [9, 1, 4], 'north', '{id:"minecraft:compass",count:1}')],
};

// Farmland that turns to dirt pushes the paintings in its block up (Block.pushEntitiesUp, Entity.teleportRelative): Painting.snapTo is
// only BlockAttachedEntity.setPos, which hangs the painting at the block it was moved into and puts it at the middle of its box. A bottom
// slab above each farmland makes it fail to survive and is in the box the painting is moved to, so each pops at its next check (the
// box it moved to decides that).
const paintingPushedWorld = () => {
  const world = new World();
  ground(world, -3, -3, 12, 12);
  for (const z of [3, 6]) {
    world.set(3, 1, z, 'minecraft:farmland', { moisture: '0' });
    world.set(3, 2, z, 'minecraft:stone_slab', { type: 'bottom', waterlogged: 'false' });
    world.set(4, 1, z, 'minecraft:stone');
    world.set(4, 2, z, 'minecraft:stone');
  }
  world.finalize();
  return world;
};
const paintingPushedRequest = {
  probes: [[3, 1, 3], [3, 1, 6]], item_entities: true, samples: sequence(130), settle_ticks: 1,
  entities: [paintingTag(100, [3, 1, 3], 'west', 'kebab'), paintingTag(200, [3, 1, 6], 'west', 'sea')],
};

// An armor stand at pos (its feet), with extra saved tags.
const standTag = (n, pos, extra = '') => ({
  at: pos, data_version: NOW,
  snbt: `{id:"minecraft:armor_stand",UUID:[I;${n},${n + 1},${n + 2},${n + 3}],Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f]${extra ? `,${extra}` : ''}}`,
});
const standBareWorld = () => {
  const world = new World();
  ground(world, -3, -3, 12, 12);
  world.finalize();
  return world;
};
const standBareRequest = {
  probes: [[0, 1, 0]], item_entities: true, world_inventories: true, living_entities: true, samples: sequence(40), settle_ticks: 1,
  entities: [standTag(100, [2.5, 4.0, 2.5]), standTag(200, [5.5, 1.0, 2.5], 'Small:1b,ShowArms:1b,equipment:{mainhand:{id:"minecraft:stone_sword",count:1},head:{id:"minecraft:iron_helmet",count:1}}'), standTag(300, [8.5, 1.0, 2.5], 'Marker:1b,Invisible:1b')],
};

// Armor stands: TNT around stands of every kind, lava, water and burning, pressure plates, a minecart, a dead stand, ice, a ladder.
const standsWorld = () => {
  const world = new World();
  ground(world, -3, -3, 40, 40);
  world.set(10, 1, 3, 'minecraft:tnt');
  world.set(10, 1, 2, 'minecraft:redstone_block');
  // Lava, and water.
  for (let x = 18; x <= 20; x++) for (let z = 2; z <= 4; z++) world.set(x, 1, z, 'minecraft:lava', { level: '0' });
  for (let x = 2; x <= 5; x++) for (let z = 12; z <= 15; z++) { world.set(x, 1, z, 'minecraft:water', { level: '0' }); world.set(x, 2, z, 'minecraft:water', { level: '0' }); }
  for (let x = 1; x <= 6; x++) for (const z of [11, 16]) { world.set(x, 1, z, 'minecraft:stone'); world.set(x, 2, z, 'minecraft:stone'); }
  for (let z = 11; z <= 16; z++) for (const x of [1, 6]) { world.set(x, 1, z, 'minecraft:stone'); world.set(x, 2, z, 'minecraft:stone'); }
  // Plates.
  world.set(3, 1, 20, 'minecraft:stone_pressure_plate', { powered: 'false' });
  world.set(6, 1, 20, 'minecraft:oak_pressure_plate', { powered: 'false' });
  world.set(9, 1, 20, 'minecraft:oak_pressure_plate', { powered: 'false' });
  // A minecart on a rail.
  for (let x = 0; x <= 10; x++) world.set(x, 1, 26, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' });
  // Ice, soul sand.
  for (let x = 0; x <= 15; x++) world.set(x, 0, 30, 'minecraft:packed_ice');
  for (let x = 0; x <= 15; x++) world.set(x, 0, 33, 'minecraft:soul_sand');
  // A ladder against a wall.
  for (let y = 1; y <= 6; y++) { world.set(28, y, 2, 'minecraft:stone'); world.set(28, y, 3, 'minecraft:ladder', { facing: 'south', waterlogged: 'false' }); }
  // A sculk sensor hearing it all.
  world.set(11, 1, 9, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  world.finalize();
  return world;
};
const standsRequest = {
  probes: [[10, 1, 3], [3, 1, 20], [6, 1, 20], [9, 1, 20], [11, 1, 9]], item_entities: true, world_inventories: true, living_entities: true, samples: sequence(220), settle_ticks: 1,
  entities: [
    standTag(100, [8.5, 1.0, 3.5], 'ShowArms:1b,equipment:{mainhand:{id:"minecraft:stone_sword",count:1},head:{id:"minecraft:iron_helmet",count:1},feet:{id:"minecraft:diamond_boots",count:1}}'),
    standTag(200, [12.5, 1.0, 3.5], 'Small:1b,equipment:{head:{id:"minecraft:golden_helmet",count:1},legs:{id:"minecraft:chainmail_leggings",count:1}}'),
    standTag(300, [10.5, 1.0, 6.5], 'Invisible:1b,equipment:{chest:{id:"minecraft:leather_chestplate",count:1}}'),
    standTag(400, [6.5, 1.0, 3.5], 'Marker:1b,equipment:{head:{id:"minecraft:stone",count:3}}'),
    standTag(500, [14.5, 1.0, 1.5], 'Invulnerable:1b,equipment:{head:{id:"minecraft:stone",count:3}}'),
    standTag(600, [10.5, 1.0, 5.5], 'NoGravity:1b,equipment:{feet:{id:"minecraft:iron_boots",count:1}}'),
    standTag(700, [9.5, 1.0, 8.5], 'Pose:{Head:[10.0f,20.0f,30.0f],LeftArm:[-10.0f,0.0f,-10.0f]},DisabledSlots:5'),
    standTag(800, [19.5, 1.0, 3.5]),
    standTag(900, [22.5, 3.0, 3.5]),
    standTag(1000, [18.5, 6.0, 2.5], 'equipment:{offhand:{id:"minecraft:stone",count:2}}'),
    standTag(1100, [3.5, 5.0, 13.5]),
    standTag(1200, [4.5, 1.0, 14.5], 'Air:100s'),
    standTag(1300, [24.5, 1.0, 3.5], 'Fire:100s,equipment:{head:{id:"minecraft:iron_helmet",count:1}}'),
    standTag(1400, [26.5, 1.0, 3.5], 'Fire:100s,Invisible:1b'),
    standTag(1500, [3.5, 1.0, 20.5]),
    standTag(1600, [6.5, 1.0, 20.5], 'Marker:1b'),
    standTag(1700, [9.5, 1.0, 20.5]),
    cartTag(1800, [4.5, 1.0625, 26.5]),
    standTag(1900, [4.9, 1.0, 26.5]),
    standTag(2000, [12.5, 1.0, 12.5], 'Health:0.0f'),
    standTag(2100, [14.5, 1.0, 12.5], 'HurtTime:10s,Health:7.5f'),
    standTag(2200, [1.5, 1.0, 30.5], 'Motion:[0.8d,0.0d,0.0d]'),
    standTag(2300, [1.5, 1.0, 33.5], 'Motion:[0.8d,0.0d,0.0d]'),
    standTag(2400, [28.5, 6.0, 3.5]),
    standTag(2500, [34.5, 1.0, 20.5], 'Rotation:[400.0f,0.0f],Pose:{Head:[400.0f,0.0f,0.0f]}'),
  ],
};

// Stands in the void, stands with little health burning, a stand carried by water.
const standVoidWorld = () => {
  const world = new World();
  ground(world, 10, -3, 20, 8);
  world.finalize();
  return world;
};
const standVoidRequest = {
  probes: [[0, 1, 0]], item_entities: true, world_inventories: true, living_entities: true, samples: sequence(200), settle_ticks: 1,
  entities: [
    standTag(100, [2.5, 3.0, 2.5], 'equipment:{head:{id:"minecraft:iron_helmet",count:1}}'),
    standTag(200, [4.5, 3.0, 2.5]),
    standTag(300, [12.5, 1.0, 2.5], 'Health:4.0f,Fire:100s,equipment:{head:{id:"minecraft:iron_helmet",count:1},mainhand:{id:"minecraft:stick",count:5}}'),
    standTag(400, [14.5, 1.0, 2.5], 'Health:4.5f,Fire:30s'),
  ],
};

// Imports the server refuses: no block entity there, another type's id, a future version, an entity it does not take.
const importFailures = [
  ['no-block-entity', (w) => { w.set(0, 1, 0, 'minecraft:stone'); }, { block_entities: [blockEntity([0, 1, 0], 'chest')] }],
  ['id-mismatch', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { block_entities: [blockEntity([0, 1, 0], 'barrel', 'Items:[]')] }],
  ['future-version', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { block_entities: [{ at: [0, 1, 0], data_version: 99999, snbt: '{Items:[]}' }] }],
  ['no-id', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { block_entities: [{ at: [0, 1, 0], data_version: NOW, snbt: '{Items:[{Slot:1b,id:"minecraft:stone",count:3}]}' }] }],
  ['entity-type', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { entities: [{ at: [0.5, 2, 0.5], data_version: NOW, snbt: '{id:"minecraft:tnt",UUID:[I;1,2,3,4],Pos:[0.5d,2.0d,0.5d]}' }] }],
  ['entity-pos', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { entities: [{ at: [0.5, 2, 0.5], data_version: NOW, snbt: '{id:"minecraft:item",UUID:[I;1,2,3,4],Pos:[0.5d,3.0d,0.5d],Item:{id:"minecraft:stone",count:1}}' }] }],
  ['entity-no-uuid', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { entities: [{ at: [0.5, 2, 0.5], data_version: NOW, snbt: '{id:"minecraft:item",Pos:[0.5d,2.0d,0.5d],Item:{id:"minecraft:stone",count:1}}' }] }],
  ['entity-future', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { entities: [{ at: [0.5, 2, 0.5], data_version: 99999, snbt: '{id:"minecraft:item",UUID:[I;1,2,3,4],Pos:[0.5d,2.0d,0.5d],Item:{id:"minecraft:stone",count:1}}' }] }],
  ['entity-empty-item', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { entities: [{ at: [0.5, 2, 0.5], data_version: NOW, snbt: '{id:"minecraft:item",UUID:[I;1,2,3,4],Pos:[0.5d,2.0d,0.5d]}' }] }],
  ['entity-duplicate', (w) => { w.set(0, 1, 0, 'minecraft:chest'); }, { entities: [{ at: [0.5, 2, 0.5], data_version: NOW, snbt: '{id:"minecraft:item",UUID:[I;1,2,3,4],Pos:[0.5d,2.0d,0.5d],Item:{id:"minecraft:stone",count:1}}' }, { at: [0.5, 2, 0.5], data_version: NOW, snbt: '{id:"minecraft:item",UUID:[I;1,2,3,4],Pos:[0.5d,2.0d,0.5d],Item:{id:"minecraft:stone",count:1}}' }] }],
].map(([name, build, args]) => ({
  name: `imports/fails-${name}`,
  world: () => { const world = new World(); ground(world, 0, 0, 2, 2); build(world); world.finalize(); return world; },
  args: { probes: [[0, 1, 0]], samples: sequence(3), settle_ticks: 1, ...args },
  placementMode: 'strict',
}));

module.exports = [
  { name: 'imports/lectern', world: importedLecternWorld, args: importedLecternRequest, placementMode: 'strict' },
  { name: 'imports/misc', world: importedMiscWorld, args: importedMiscRequest, placementMode: 'strict' },
  { name: 'imports/misc-updates', world: importedMiscWorld, args: importedMiscRequest, placementMode: 'updates' },
  { name: 'imports/entities', world: importedEntitiesWorld, args: importedEntitiesRequest, placementMode: 'strict' },
  { name: 'imports/entities-updates', world: importedEntitiesWorld, args: importedEntitiesRequest, placementMode: 'updates' },
  { name: 'imports/frames', world: importedFramesWorld, args: importedFramesRequest, placementMode: 'strict' },
  { name: 'imports/frames-updates', world: importedFramesWorld, args: importedFramesRequest, placementMode: 'updates' },
  { name: 'imports/frames-wet', world: importedFramesWaterWorld(true), args: importedFramesWaterRequest, placementMode: 'updates' },
  { name: 'imports/frames-dry', world: importedFramesWaterWorld(false), args: importedFramesWaterRequest, placementMode: 'updates' },
  { name: 'imports/supplied', world: suppliedWorld, args: suppliedRequest },
  ...importFailures,
  { name: 'vehicles/cart-loop', world: cartLoopWorld, args: cartLoopRequest, placementMode: 'strict' },
  { name: 'vehicles/cart-loop-updates', world: cartLoopWorld, args: cartLoopRequest, placementMode: 'updates' },
  { name: 'vehicles/cart-hoppers', world: cartHoppersWorld, args: cartHoppersRequest, placementMode: 'strict' },
  { name: 'vehicles/cart-hoppers-updates', world: cartHoppersWorld, args: cartHoppersRequest, placementMode: 'updates' },
  { name: 'vehicles/cart-tnt', world: cartTntWorld, args: cartTntRequest, placementMode: 'strict' },
  { name: 'vehicles/cart-tnt-updates', world: cartTntWorld, args: cartTntRequest, placementMode: 'updates' },
  { name: 'vehicles/boat-pool', world: boatPoolWorld(true), args: boatPoolRequest, placementMode: 'strict' },
  { name: 'vehicles/boat-pool-updates', world: boatPoolWorld(false), args: boatPoolRequest, placementMode: 'updates' },
  { name: 'vehicles/boat-pool-dry', world: boatPoolWorld(false), args: boatPoolRequest, placementMode: 'strict' },
  { name: 'vehicles/painting-wall', world: paintingWallWorld, args: paintingWallRequest, placementMode: 'strict' },
  { name: 'vehicles/painting-wall-updates', world: paintingWallWorld, args: paintingWallRequest, placementMode: 'updates' },
  { name: 'vehicles/painting-bare', world: paintingBareWorld, args: paintingBareRequest, placementMode: 'strict' },
  { name: 'vehicles/painting-pushed', world: paintingPushedWorld, args: paintingPushedRequest, placementMode: 'updates' },
  { name: 'vehicles/stand-bare', world: standBareWorld, args: standBareRequest, placementMode: 'strict' },
  { name: 'vehicles/stand-void', world: standVoidWorld, args: standVoidRequest, placementMode: 'strict' },
  { name: 'vehicles/stands', world: standsWorld, args: standsRequest, placementMode: 'strict' },
  { name: 'vehicles/stands-updates', world: standsWorld, args: standsRequest, placementMode: 'updates' },
  { name: 'vehicles/cart-tracks', world: cartTracksWorld, args: cartTracksRequest, placementMode: 'strict' },
  { name: 'vehicles/cart-tracks-updates', world: cartTracksWorld, args: cartTracksRequest, placementMode: 'updates' },
  { name: 'imports/containers', world: importedContainersWorld, args: importedContainersBase, placementMode: 'strict' },
  { name: 'imports/containers-updates', world: importedContainersWorld, args: importedContainersBase, placementMode: 'updates' },
  { name: 'imports/components', world: importedComponentsWorld, args: importedComponentsRequest, placementMode: 'strict' },
  { name: 'imports/components-updates', world: importedComponentsWorld, args: importedComponentsRequest, placementMode: 'updates' },
  { name: 'block-version/old-chain', world: blockVersionWorld('minecraft:chain'), args: { ...blockVersionRequest, block_data_version: 3218 }, placementMode: 'strict' },
  { name: 'block-version/current-iron-chain', world: blockVersionWorld('minecraft:iron_chain'), args: blockVersionRequest, placementMode: 'strict' },
  { name: 'custom-item-supply/custom-items', world: customItemWorld, args: customItemRequest },
  { name: 'dynamic-inventory/baseline', world: dynamicInventoryWorld, args: dynamicInventoryBase },
  { name: 'dynamic-inventory/observed', world: dynamicInventoryWorld, args: { ...dynamicInventoryBase, inventory_probes: [[1, 1, 0], [4, 0, 0]], inventory_missing: 'report', item_entities: true } },
  ...(process.env.MCBENCH_CORPUS ? corpusFixtures() : []),
  { name: 'use-input/blocks', world: useWorld(false), args: useRequest },
  { name: 'use-input/blocks-strict', world: useWorld(false), args: useRequest, placementMode: 'strict' },
  { name: 'use-input/shrieker', world: useWorld(true), args: useRequest },
  { name: 'use-input/lectern-pages', world: importedLecternWorld, args: lecternPagesRequest },
  { name: 'use-input/lectern-pages-strict', world: importedLecternWorld, args: lecternPagesRequest, placementMode: 'strict' },
  { name: 'inventory-verify/note-block-and-inventories', world: inventoryVerifyWorld, args: inventoryVerifyRequest },
  { name: 'inventory-verify/input-errors', world: inputErrorWorld, args: { probes: inputErrorBlocks.map((_, index) => [index * 2, 1, 0]), samples: [0, 20], settle_ticks: 1, actions: inputErrorActions } },
  { name: 'item-supply/drop', world: stoneWorld, args: itemSupplyRequest },
  { name: 'item-supply/oversized', world: stoneWorld, args: { ...itemSupplyRequest, actions: [{ ...itemSupplyRequest.actions[0], item: 'minecraft:ender_pearl', count: 17 }] } },
  { name: 'lectern-input/pages', world: lecternWorld('true'), args: lecternInputRequest },
  { name: 'lectern-supply/books', world: lecternWorld('false'), args: lecternSupplyRequest },
  { name: 'lectern-book/take-and-restore', world: lecternBookWorld, args: lecternBookRequest, placementMode: 'strict' },
  { name: 'lectern-book/take-and-restore-updates', world: lecternBookWorld, args: lecternBookRequest },
  { name: 'lectern-book/supply', world: lecternSupplyWorld, args: lecternSupplyNowRequest },
  { name: 'lectern-book/supply-strict', world: lecternSupplyWorld, args: lecternSupplyNowRequest, placementMode: 'strict' },
  { name: 'living-census/observed', world: livingCensusWorld, args: { ...livingCensusRequest, living_entities: true } },
  { name: 'living-census/silent', world: livingCensusWorld, args: { ...livingCensusRequest, living_entities: false } },
  { name: 'passenger-import/minecart-dolphin', world: passengerWorld, args: passengerRequest, placementMode: 'strict' },
  { name: 'scheduled-ticks/saved', world: scheduledTicksWorld, args: scheduledTicksRequest, placementMode: 'strict' },
  { name: 'scheduled-ticks/empty', world: scheduledTicksWorld, args: { ...scheduledTicksRequest, scheduled_ticks: { ...savedTicks, entries: [] } }, placementMode: 'strict' },
  { name: 'versioned-verifier/native-mix', world: versionedWorld, args: versionedRequest, placementMode: 'strict' },
  { name: 'minecraft-verify.integration/buttons-toggle-gravity', world: buttonsWorld, args: buttonsRequest },
  { name: 'minecraft-verify.integration/dense-samples', world: buttonsWorld, args: { ...buttonsRequest, samples: sequence(71) } },
  { name: 'minecraft-verify.integration/press-non-button', world: buttonsWorld, args: { probes: [[0, 1, 1], [100, 6, 100]], samples: [0], actions: [{ tick: 0, type: 'press', at: [0, 1, 1] }] } },
  { name: 'minecraft-verify.integration/large-probes', world: buttonsWorld, args: { probes: Array.from({ length: 512 }, () => [16, 1, -2]), samples: [0, 20], actions: [{ tick: 0, type: 'toggle', at: [16, 1, 0] }] } },
  { name: 'minecraft-verify.integration/drain', world: drainWorld, args: drainRequest },
  { name: 'minecraft-verify.integration/falling-sand', world: fallingWorld, args: { probes: [[0, 1, 0], [0, 2, 0], [0, 3, 0], [0, 4, 0]], samples: sequence(31), settle_ticks: 1 } },
  { name: 'brushable-fall/break', world: brushableFallWorld, args: brushableFallRequest },
  { name: 'trial-spawner/states', world: trialSpawnerWorld, args: trialSpawnerRequest },
  { name: 'command-blocks/saved', world: commandBlocksWorld, args: commandBlocksRequest, placementMode: 'strict' },
  { name: 'command-blocks/saved-updates', world: commandBlocksWorld, args: commandBlocksRequest, placementMode: 'updates' },
  { name: 'beacons/saved', world: beaconsWorld, args: beaconsRequest, placementMode: 'strict' },
  { name: 'beacons/saved-updates', world: beaconsWorld, args: beaconsRequest, placementMode: 'updates' },
  { name: 'saved-specials/blocks', world: savedSpecialsWorld, args: savedSpecialsRequest, placementMode: 'strict' },
  { name: 'saved-specials/blocks-updates', world: savedSpecialsWorld, args: savedSpecialsRequest, placementMode: 'updates' },
  { name: 'composter-sensor/output', world: composterSensorWorld(true), args: composterSensorRequest(true) },
  { name: 'composter-sensor/input', world: composterSensorWorld(false), args: composterSensorRequest(false) },
  { name: 'sniffer-egg-sensor/place', world: snifferEggSensorWorld, args: { probes: [[1, 1, 0], [1, 1, 2], [5, 1, 0], [8, 1, 2]], samples: sequence(41), settle_ticks: 1 } },
  { name: 'change-sensor/self', world: changeSensorWorld, args: changeSensorRequest, placementMode: 'strict' },
  { name: 'tilt-sensor/leaves', world: tiltSensorWorld, args: tiltSensorRequest, placementMode: 'strict' },
  { name: 'shelf-sensor/lever', world: shelfSensorWorld, args: shelfSensorRequest },
  { name: 'pot-shelf/containers', world: potShelfWorld, args: potShelfRequest },
  { name: 'pot-shelf/containers-strict', world: potShelfWorld, args: potShelfRequest, placementMode: 'strict' },
  ...require('./contact-fixtures').fixtures(),
  ...require('./recipe-fixtures').fixtures(),
  ...require('./orb-fixtures').fixtures(),
  ...require('./mob-fixtures').fixtures(),
];
Object.assign(module.exports, { cartTag, boatTag, paintingTag, standTag });
