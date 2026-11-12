'use strict';

// Verifier fixtures for what items do in the workstations: crafters crafting
// every recipe the server holds (and the special recipes with the data they
// need), furnaces, blast furnaces and smokers smelting, brewing stands
// brewing, jukeboxes playing, dispensers firing every behaviour the engine
// models, hoppers feeding all of them. Fixtures are named recipes/<case>;
// scripts/verifier-fixtures.js lists them. The ingredients come from the
// engine's dump (core/minecraft/<version>/blocks.txt), chosen from a seeded
// random, so the cases are the same every run.

const fs = require('fs');
const path = require('path');
const { World } = require('../server/world');

const ROOT = path.join(__dirname, '..');
const NOW = 4556;
const sequence = (count) => Array.from({ length: count }, (_, index) => index);

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
  return { next, int: (n) => Math.floor(next() * n), pick: (list) => list[Math.floor(next() * list.length)], chance: (p) => next() < p };
}

// The registry dump's lines by kind.
let dump = null;
function registry() {
  if (dump) return dump;
  dump = { recipes: [], cooking: [], fuel: new Map(), songs: [], potionMixes: [], containerMixes: [], containers: [], items: [], itemTags: new Map(), remainders: new Map(), dispense: new Map(), compost: new Map(), maxStack: new Map(), potions: [] };
  require('../server/minecraft-tables').checkTables('1.21.10');
  for (const line of fs.readFileSync(path.join(ROOT, 'core', 'minecraft', '1.21.10', 'blocks.txt'), 'utf8').split('\n')) {
    const p = line.split('\t');
    switch (p[0]) {
      case 'item': dump.items.push(p[1]); dump.maxStack.set(p[1], Number(p[2])); break;
      case 'recipe': dump.recipes.push(p); break;
      case 'cooking': dump.cooking.push({ type: p[1], id: p[2], ingredient: p[3].split(','), result: p[4], experience: Number(p[5]), time: Number(p[6]) }); break;
      case 'fuel': dump.fuel.set(p[1], Number(p[2])); break;
      case 'jukebox': dump.songs.push({ key: p[1], seconds: Number(p[2]), comparator: Number(p[3]) }); break;
      case 'potionmix': dump.potionMixes.push({ from: p[1], ingredient: p[2].split(','), to: p[3] }); break;
      case 'containermix': dump.containerMixes.push({ from: p[1], ingredient: p[2].split(','), to: p[3] }); break;
      case 'brewcontainer': dump.containers.push(p[1]); break;
      case 'remainder': dump.remainders.set(p[1], p[2]); break;
      case 'itemtag': dump.itemTags.set(p[1], p[2] === '-' ? [] : p[2].split(',')); break;
      case 'dispense': dump.dispense.set(p[1], p[3]); break;
      case 'compost': dump.compost.set(p[1], Number(p[2])); break;
      case 'entries': if (p[1] === 'minecraft:potion') dump.potions = p[2].split(','); break;
      default:
    }
  }
  return dump;
}

const slotted = (slot, item, count, components) => `{Slot:${slot}b,id:"${item}",count:${count}${components ? `,components:${components}` : ''}}`;
const blockEntity = (at, id, fields) => ({ at, data_version: NOW, snbt: `{id:"minecraft:${id}"${fields ? `,${fields}` : ''}}` });
const insert = (tick, at, slot, item, count, snbt) => ({ tick, type: 'insert', at, slot, item, count, ...(snbt ? { stack: { data_version: NOW, snbt } } : {}) });
const potionStack = (item, potion) => `{id:"${item}",count:1,components:{"minecraft:potion_contents":{potion:"${potion}"}}}`;

// A fixture built when first asked for: the world and the request, once.
function lazy(name, build, placementMode) {
  let built = null;
  const get = () => (built ||= build());
  return {
    name: `recipes/${name}`,
    ...(placementMode ? { placementMode } : {}),
    world: () => get().world,
    get args() { return get().args; },
  };
}

// --- Crafters crafting recipes ---

// A crafter with a lever on its east side, facing north.
function crafter(world, x, z, orientation = 'north_up') {
  world.set(x, 1, z, 'minecraft:crafter', { orientation, triggered: 'false', crafting: 'false' });
  world.set(x + 1, 1, z, 'minecraft:lever', { face: 'wall', facing: 'east', powered: 'false' });
}

// The grid cells (0 to 8) a recipe's ingredients go in, and the item in each.
function recipeGrid(p, r, mirror) {
  const kind = p[1];
  if (kind === 'shaped') {
    const width = Number(p[3]);
    const height = Number(p[4]);
    const cells = p[5].split(';');
    const dx = r.int(4 - width);
    const dy = r.int(4 - height);
    const grid = new Array(9).fill(null);
    for (let y = 0; y < height; y++) {
      for (let x = 0; x < width; x++) {
        const cell = cells[mirror ? width - 1 - x + y * width : x + y * width];
        if (cell === '-') continue;
        grid[(x + dx) + (y + dy) * 3] = r.pick(cell.split(','));
      }
    }
    return grid;
  }
  const slots = sequence(9);
  const grid = new Array(9).fill(null);
  for (const ingredient of p[3].split(';')) {
    const at = slots.splice(r.int(slots.length), 1)[0];
    grid[at] = r.pick(ingredient.split(','));
  }
  return grid;
}

// Crafters side by side, each with a recipe's ingredients (counts of one to
// three: the crafter spends one of each), each powered by its lever at tick 3.
function craftersWorld(grids, extraActions = [], { spacing = 4, orientation = 'north_up' } = {}) {
  const r = random(7);
  const width = Math.ceil(Math.sqrt(grids.length));
  const world = new World();
  world.fill([-2, 0, -2], [width * spacing, 0, Math.ceil(grids.length / width) * spacing + 2], 'minecraft:stone');
  const actions = [];
  const probes = [];
  grids.forEach((grid, index) => {
    const x = (index % width) * spacing;
    const z = Math.floor(index / width) * spacing + 2;
    crafter(world, x, z, orientation);
    grid.forEach((item, slot) => {
      if (!item) return;
      const stack = typeof item === 'string' ? { item } : item;
      actions.push(insert(1, [x, 1, z], slot, stack.item, stack.count ?? 1 + r.int(3), stack.snbt));
    });
    actions.push({ tick: 3, type: 'toggle', at: [x + 1, 1, z] });
    probes.push([x, 1, z]);
  });
  world.finalize();
  return { world, args: { probes, inventory_probes: probes, inventory_missing: 'report', item_entities: true, world_inventories: true, settle_ticks: 1, samples: [0, 6, 20], actions: [...actions, ...extraActions] } };
}

// The special recipes with the stacks they need.
function specialGrids() {
  const stack = (item, snbt, count = 1) => ({ item, count, snbt });
  const damaged = (item, damage, enchantments) => stack(item, `{id:"${item}",count:1,components:{"minecraft:damage":${damage}${enchantments ? `,"minecraft:enchantments":${enchantments}` : ''}}}`);
  const book = (generation) => stack('minecraft:written_book', `{id:"minecraft:written_book",count:1,components:{"minecraft:written_book_content":{title:"T",author:"A",generation:${generation},pages:["p"]}}}`);
  return [
    ['armor-dye', [null, 'minecraft:leather_chestplate', null, 'minecraft:red_dye', 'minecraft:blue_dye', 'minecraft:yellow_dye']],
    ['armor-dye-dyed', [stack('minecraft:leather_helmet', '{id:"minecraft:leather_helmet",count:1,components:{"minecraft:dyed_color":1234567}}'), 'minecraft:green_dye', null, null, null, null, 'minecraft:white_dye']],
    ['banner-duplicate', ['minecraft:white_banner', 'minecraft:white_banner']],
    ['book-cloning', [book(0), 'minecraft:writable_book', 'minecraft:writable_book']],
    ['book-cloning-copy', [book(1), 'minecraft:writable_book']],
    ['book-cloning-too-old', [book(2), 'minecraft:writable_book']],
    ['decorated-pot', [null, 'minecraft:brick', null, 'minecraft:angler_pottery_sherd', null, 'minecraft:brick', null, 'minecraft:heart_pottery_sherd']],
    ['firework-rocket', ['minecraft:paper', 'minecraft:gunpowder', 'minecraft:gunpowder', 'minecraft:firework_star']],
    ['firework-rocket-stars', ['minecraft:paper', 'minecraft:gunpowder', 'minecraft:firework_star', 'minecraft:firework_star']],
    ['firework-star', ['minecraft:gunpowder', 'minecraft:red_dye', 'minecraft:blue_dye', 'minecraft:glowstone_dust', 'minecraft:diamond', 'minecraft:feather']],
    ['firework-star-creeper', ['minecraft:gunpowder', 'minecraft:lime_dye', 'minecraft:creeper_head']],
    ['firework-star-fade', ['minecraft:firework_star', 'minecraft:orange_dye', 'minecraft:black_dye']],
    ['map-cloning', ['minecraft:map', 'minecraft:filled_map']],
    ['map-extending', ['minecraft:paper', 'minecraft:paper', 'minecraft:paper', 'minecraft:paper', 'minecraft:filled_map', 'minecraft:paper', 'minecraft:paper', 'minecraft:paper', 'minecraft:paper']],
    ['repair-item', [damaged('minecraft:iron_pickaxe', 200), damaged('minecraft:iron_pickaxe', 230)]],
    ['repair-curses', [damaged('minecraft:iron_pickaxe', 200, '{"minecraft:binding_curse":1,"minecraft:unbreaking":2}'), damaged('minecraft:iron_pickaxe', 230, '{"minecraft:vanishing_curse":1}')]],
    ['repair-different', [damaged('minecraft:iron_pickaxe', 100), damaged('minecraft:iron_axe', 30)]],
    ['shield-decoration', ['minecraft:shield', 'minecraft:blue_banner']],
    ['tipped-arrow', ['minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow', stack('minecraft:lingering_potion', potionStack('minecraft:lingering_potion', 'minecraft:healing')), 'minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow']],
    ['tipped-arrow-plain', ['minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow', 'minecraft:lingering_potion', 'minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow', 'minecraft:arrow']],
  ];
}

function crafting() {
  const shaped = (seed, mirror, slice) => {
    const r = random(seed);
    return registry().recipes.filter((p) => p[1] !== 'other').map((p) => recipeGrid(p, r, mirror && r.chance(0.5))).filter((_, index) => slice(index));
  };
  const fixtures = [];
  const parts = 4;
  for (let part = 0; part < parts; part++) {
    fixtures.push(lazy(`crafters-all-${part}`, () => craftersWorld(shaped(11 + part, part % 2 === 1, (index) => index % parts === part))));
  }
  fixtures.push(lazy('crafters-special', () => craftersWorld(specialGrids().map(([, grid]) => grid))));
  return fixtures;
}

// --- Furnaces, blast furnaces and smokers ---

const FURNACE_OF = { 'minecraft:smelting': 'minecraft:furnace', 'minecraft:blasting': 'minecraft:blast_furnace', 'minecraft:smoking': 'minecraft:smoker' };

// A row of workstations: the floor, a block of the kind at each position
// (spacing apart) with the states, and a probe at it.
function stationsWorld(count, spacing, place, { width = 12 } = {}) {
  const world = new World();
  const columns = Math.min(count, width);
  const rows = Math.ceil(count / columns);
  world.fill([-2, 0, -2], [columns * spacing + 2, 0, rows * spacing + 2], 'minecraft:stone');
  const probes = [];
  for (let index = 0; index < count; index++) {
    const x = (index % columns) * spacing;
    const z = Math.floor(index / columns) * spacing;
    place(world, x, z, index);
    probes.push([x, 1, z]);
  }
  return { world, probes };
}

// Furnaces smelting what their recipes take, fuelled by what burns.
function furnaces(seed, slice) {
  const r = random(seed);
  const cooking = registry().cooking.filter((recipe) => FURNACE_OF[recipe.type]).filter((_, index) => slice(index));
  const fuels = [...registry().fuel.keys()];
  const cases = cooking.map((recipe) => ({ recipe, fuel: r.pick(fuels), fuelCount: 1 + r.int(3), count: 1 + r.int(r.chance(0.2) ? 40 : 3), alt: r.pick(recipe.ingredient) }));
  const { world, probes } = stationsWorld(cases.length, 3, (w, x, z, index) => w.set(x, 1, z, FURNACE_OF[cases[index].recipe.type], { facing: 'north', lit: 'false' }));
  world.finalize();
  const actions = [];
  cases.forEach((c, index) => {
    const at = probes[index];
    actions.push(insert(1 + (index % 3), at, 0, c.alt, c.count));
    actions.push(insert(1 + (index % 4), at, 1, c.fuel, c.fuelCount));
  });
  return { world, args: { probes, inventory_probes: probes, inventory_missing: 'report', world_inventories: true, item_entities: true, settle_ticks: 1, samples: [0, 5, 60, 110, 210, 400], actions } };
}

// --- Brewing stands ---

function brewing(seed) {
  const r = random(seed);
  const dumped = registry();
  const mixes = [
    ...dumped.potionMixes.map((mix) => ({ container: 'minecraft:potion', potion: mix.from, ingredient: r.pick(mix.ingredient) })),
    ...dumped.containerMixes.map((mix) => ({ container: mix.from, potion: 'minecraft:water', ingredient: r.pick(mix.ingredient) })),
    ...dumped.containerMixes.map((mix) => ({ container: mix.from, potion: 'minecraft:healing', ingredient: r.pick(mix.ingredient) })),
  ];
  const { world, probes } = stationsWorld(mixes.length, 3, (w, x, z) => w.set(x, 1, z, 'minecraft:brewing_stand', { has_bottle_0: 'false', has_bottle_1: 'false', has_bottle_2: 'false' }));
  world.finalize();
  const actions = [];
  mixes.forEach((mix, index) => {
    const at = probes[index];
    const bottles = 1 + r.int(3);
    for (let slot = 0; slot < bottles; slot++) {
      const second = r.chance(0.3);
      actions.push(insert(1, at, slot, second ? 'minecraft:glass_bottle' : mix.container, 1, second ? undefined : potionStack(mix.container, mix.potion)));
    }
    actions.push(insert(2, at, 3, mix.ingredient, 1 + r.int(3)));
    actions.push(insert(2 + (index % 3), at, 4, 'minecraft:blaze_powder', 1 + r.int(2)));
  });
  return { world, args: { probes, inventory_probes: probes, inventory_missing: 'report', world_inventories: true, item_entities: true, settle_ticks: 1, samples: [0, 3, 200, 401, 405, 450, 801, 830], actions } };
}

// --- Jukeboxes ---

function jukeboxes() {
  const songs = registry().songs;
  const discs = [...registry().items].filter((item) => /^minecraft:music_disc_|disc_fragment/.test(item));
  const { world, probes } = stationsWorld(discs.length + 2, 3, (w, x, z) => {
    w.set(x, 1, z, 'minecraft:jukebox', { has_record: 'false' });
    w.set(x + 1, 1, z, 'minecraft:comparator', { facing: 'east', mode: 'compare', powered: 'false' });
    w.set(x + 2, 1, z, 'minecraft:redstone_lamp', { lit: 'false' });
  }, { width: 10 });
  world.finalize();
  const actions = [];
  discs.forEach((disc, index) => actions.push(insert(1 + (index % 5), probes[index], 0, disc, 1)));
  return { world, args: { probes: [...probes, ...probes.map(([x, y, z]) => [x + 1, y, z]), ...probes.map(([x, y, z]) => [x + 2, y, z])], inventory_probes: probes, inventory_missing: 'report', world_inventories: true, item_entities: true, settle_ticks: 1, samples: [0, 5, 20, 21, 100, 1500, 1520, 3700], actions } };
}

// --- Hoppers feeding and draining the workstations ---

const TARGETS = {
  furnace: ['minecraft:furnace', { facing: 'north', lit: 'false' }],
  blast: ['minecraft:blast_furnace', { facing: 'north', lit: 'false' }],
  smoker: ['minecraft:smoker', { facing: 'north', lit: 'false' }],
  brewing: ['minecraft:brewing_stand', { has_bottle_0: 'false', has_bottle_1: 'false', has_bottle_2: 'false' }],
  jukebox: ['minecraft:jukebox', { has_record: 'false' }],
  shelf: ['minecraft:chiseled_bookshelf', { facing: 'north', slot_0_occupied: 'false', slot_1_occupied: 'false', slot_2_occupied: 'false', slot_3_occupied: 'false', slot_4_occupied: 'false', slot_5_occupied: 'false' }],
  composter: ['minecraft:composter', { level: '0' }],
  barrel: ['minecraft:barrel', { facing: 'up', open: 'false' }],
};

const chest = { facing: 'north', type: 'single', waterlogged: 'false' };

// What goes into a rig's chests, by target: the items for the top (and the
// bottom hopper's), and for the side.
function rigItems(kind, r) {
  const dumped = registry();
  const smeltable = (type) => dumped.cooking.filter((recipe) => recipe.type === type).flatMap((recipe) => recipe.ingredient);
  const fuels = [...dumped.fuel.keys()];
  const stacks = (items, n) => Array.from({ length: n }, () => ({ item: r.pick(items), count: 1 + r.int(r.chance(0.2) ? 30 : 4) }));
  switch (kind) {
    case 'furnace': return { top: stacks([...smeltable('minecraft:smelting'), 'minecraft:stone', 'minecraft:bucket', 'minecraft:water_bucket', 'minecraft:coal'], 6), side: stacks([...fuels, 'minecraft:bucket', 'minecraft:stick', 'minecraft:dirt'], 4) };
    case 'blast': return { top: stacks([...smeltable('minecraft:blasting'), 'minecraft:stone'], 6), side: stacks([...fuels, 'minecraft:bucket', 'minecraft:sponge'], 4) };
    case 'smoker': return { top: stacks([...smeltable('minecraft:smoking'), 'minecraft:wet_sponge', 'minecraft:stone'], 6), side: stacks([...fuels, 'minecraft:bucket', 'minecraft:dirt'], 4) };
    case 'brewing': return {
      top: [...stacks(['minecraft:nether_wart', 'minecraft:redstone', 'minecraft:glowstone_dust', 'minecraft:gunpowder', 'minecraft:dragon_breath', 'minecraft:sugar', 'minecraft:dirt'], 3), ...stacks(['minecraft:glass_bottle', 'minecraft:blaze_powder'], 2)],
      side: [{ item: 'minecraft:blaze_powder', count: 1 + r.int(3) }, ...stacks(['minecraft:glass_bottle', 'minecraft:blaze_powder', 'minecraft:stick'], 3), { item: 'minecraft:potion', count: 1, snbt: potionStack('minecraft:potion', r.pick(['minecraft:water', 'minecraft:awkward'])) }],
    };
    case 'jukebox': return { top: stacks(dumped.items.filter((item) => /music_disc_/.test(item)), 5), side: stacks(dumped.items.filter((item) => /music_disc_/.test(item)), 3) };
    case 'shelf': return { top: stacks(['minecraft:book', 'minecraft:enchanted_book', 'minecraft:writable_book', 'minecraft:written_book', 'minecraft:knowledge_book', 'minecraft:dirt', 'minecraft:paper'], 8), side: stacks(['minecraft:book', 'minecraft:writable_book'], 3) };
    case 'composter': return { top: stacks([...dumped.compost.keys(), 'minecraft:dirt', 'minecraft:bone_meal'], 10), side: stacks([...dumped.compost.keys()], 4) };
    default: return { top: stacks(['minecraft:stone', 'minecraft:dirt', 'minecraft:book', 'minecraft:gold_ingot'], 5), side: stacks(['minecraft:stone', 'minecraft:coal'], 3) };
  }
}

function hopperRigs(seed, kinds, perKind) {
  const r = random(seed);
  const rigs = kinds.flatMap((kind) => Array.from({ length: perKind }, () => kind));
  const { world, probes } = stationsWorld(rigs.length, 6, (w, x, z, index) => {
    const [block, states] = TARGETS[rigs[index]];
    w.set(x, 1, z, 'minecraft:chest', chest);
    w.set(x, 2, z, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
    w.set(x, 3, z, block, states);
    w.set(x, 4, z, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
    w.set(x, 5, z, 'minecraft:chest', chest);
    if (rigs[index] !== 'composter' || r.chance(0.5)) {
      w.set(x + 1, 4, z, 'minecraft:chest', chest);
      w.set(x + 1, 3, z, 'minecraft:hopper', { facing: 'west', enabled: 'true' });
    }
    // A comparator reading the target, and a lamp.
    w.set(x - 1, 3, z, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
    w.set(x - 2, 3, z, 'minecraft:redstone_lamp', { lit: 'false' });
    w.set(x - 1, 2, z, 'minecraft:stone');
  }, { width: 8 });
  // The floor under the rigs is stone; the chests above are probed.
  world.finalize();
  const actions = [];
  const watched = [];
  rigs.forEach((kind, index) => {
    const [x, , z] = probes[index];
    const items = rigItems(kind, r);
    items.top.forEach((stack, slot) => actions.push(insert(1, [x, 5, z], slot, stack.item, stack.count, stack.snbt)));
    items.side.forEach((stack, slot) => actions.push(insert(1, [x + 1, 4, z], slot, stack.item, stack.count, stack.snbt)));
    watched.push([x, 1, z], [x, 3, z], [x, 5, z], [x + 1, 4, z], [x - 2, 3, z]);
  });
  return { world, args: { probes: watched, inventory_probes: watched, inventory_missing: 'report', world_inventories: true, item_entities: true, settle_ticks: 1, samples: [0, 3, 8, 20, 60, 130, 260, 400], actions } };
}

// --- Dispensers firing their behaviours ---

const DIRS = { east: [1, 0, 0], west: [-1, 0, 0], north: [0, 0, -1], south: [0, 0, 1], up: [0, 1, 0], down: [0, -1, 0] };

// Targets in front of a dispenser: what is placed at the position (x, y, z), and
// around it, by name.
const TARGETS_FOR_DISPENSERS = {
  air: (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:stone'); },
  'air-over-air': () => {},
  'air-over-water': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:water', { level: '0' }); w.set(x, y - 2, z, 'minecraft:stone'); },
  stone: (w, x, y, z) => { w.set(x, y, z, 'minecraft:stone'); },
  water: (w, x, y, z) => { w.set(x, y, z, 'minecraft:water', { level: '0' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  'flowing-water': (w, x, y, z) => { w.set(x, y, z, 'minecraft:water', { level: '3' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  lava: (w, x, y, z) => { w.set(x, y, z, 'minecraft:lava', { level: '0' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  'flowing-lava': (w, x, y, z) => { w.set(x, y, z, 'minecraft:lava', { level: '5' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  slab: (w, x, y, z) => { w.set(x, y, z, 'minecraft:oak_slab', { type: 'bottom', waterlogged: 'false' }); },
  'wet-slab': (w, x, y, z) => { w.set(x, y, z, 'minecraft:oak_slab', { type: 'bottom', waterlogged: 'true' }); },
  'double-slab': (w, x, y, z) => { w.set(x, y, z, 'minecraft:oak_slab', { type: 'double', waterlogged: 'false' }); },
  stairs: (w, x, y, z) => { w.set(x, y, z, 'minecraft:oak_stairs', { facing: 'north', half: 'bottom', shape: 'straight', waterlogged: 'true' }); },
  kelp: (w, x, y, z) => { w.set(x, y, z, 'minecraft:kelp', { age: '3' }); w.set(x, y + 1, z, 'minecraft:water', { level: '0' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  'kelp-plant': (w, x, y, z) => { w.set(x, y, z, 'minecraft:kelp_plant'); w.set(x, y + 1, z, 'minecraft:kelp', { age: '10' }); w.set(x, y + 2, z, 'minecraft:water', { level: '0' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  'kelp-capped': (w, x, y, z) => { w.set(x, y, z, 'minecraft:kelp', { age: '25' }); w.set(x, y + 1, z, 'minecraft:stone'); w.set(x, y - 1, z, 'minecraft:stone'); },
  seagrass: (w, x, y, z) => { w.set(x, y, z, 'minecraft:seagrass'); w.set(x, y + 1, z, 'minecraft:water', { level: '0' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  tnt: (w, x, y, z) => { w.set(x, y, z, 'minecraft:tnt', { unstable: 'false' }); },
  campfire: (w, x, y, z) => { w.set(x, y, z, 'minecraft:campfire', { facing: 'north', lit: 'false', signal_fire: 'false', waterlogged: 'false' }); },
  'candle-cake': (w, x, y, z) => { w.set(x, y, z, 'minecraft:candle_cake', { lit: 'false' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  netherrack: (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:netherrack'); },
  'soul-soil': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:soul_soil'); },
  'wool-fuel': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:white_wool'); w.set(x + 1, y, z, 'minecraft:white_wool'); },
  anchor0: (w, x, y, z) => { w.set(x, y, z, 'minecraft:respawn_anchor', { charges: '0' }); },
  anchor3: (w, x, y, z) => { w.set(x, y, z, 'minecraft:respawn_anchor', { charges: '3' }); },
  anchor4: (w, x, y, z) => { w.set(x, y, z, 'minecraft:respawn_anchor', { charges: '4' }); },
  copper: (w, x, y, z) => { w.set(x, y, z, 'minecraft:copper_block'); },
  'cut-copper-stairs': (w, x, y, z) => { w.set(x, y, z, 'minecraft:weathered_cut_copper_stairs', { facing: 'east', half: 'top', shape: 'straight', waterlogged: 'false' }); },
  'copper-door': (w, x, y, z) => { w.set(x, y, z, 'minecraft:copper_door', { facing: 'north', half: 'lower', hinge: 'left', open: 'false', powered: 'false' }); w.set(x, y + 1, z, 'minecraft:copper_door', { facing: 'north', half: 'upper', hinge: 'left', open: 'false', powered: 'false' }); },
  dirt: (w, x, y, z) => { w.set(x, y, z, 'minecraft:dirt'); },
  'coarse-dirt': (w, x, y, z) => { w.set(x, y, z, 'minecraft:coarse_dirt'); },
  'rooted-dirt': (w, x, y, z) => { w.set(x, y, z, 'minecraft:rooted_dirt'); },
  rail: (w, x, y, z) => { w.set(x, y, z, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  'rail-below': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:rail', { shape: 'east_west', waterlogged: 'false' }); w.set(x, y - 2, z, 'minecraft:stone'); },
  'rail-slope': (w, x, y, z) => { w.set(x, y, z, 'minecraft:powered_rail', { shape: 'ascending_east', powered: 'false', waterlogged: 'false' }); w.set(x, y - 1, z, 'minecraft:stone'); },
  'rail-slope-below': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:activator_rail', { shape: 'ascending_north', powered: 'false', waterlogged: 'false' }); w.set(x, y - 2, z, 'minecraft:stone'); },
  beehive: (w, x, y, z) => { w.set(x, y, z, 'minecraft:beehive', { facing: 'north', honey_level: '5' }); },
  'beehive-empty': (w, x, y, z) => { w.set(x, y, z, 'minecraft:beehive', { facing: 'north', honey_level: '2' }); },
  'snow-blocks': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:snow_block'); w.set(x, y - 2, z, 'minecraft:snow_block'); w.set(x, y - 3, z, 'minecraft:stone'); },
  'soul-sand-T': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:soul_sand'); w.set(x, y - 2, z, 'minecraft:soul_sand'); w.set(x + 1, y - 2, z, 'minecraft:soul_sand'); w.set(x - 1, y - 2, z, 'minecraft:soul_sand'); w.set(x, y - 3, z, 'minecraft:stone'); },
  'obsidian-frame': (w, x, y, z) => { for (let dz = -1; dz <= 2; dz++) { w.set(x, y - 1, z + dz, 'minecraft:obsidian'); w.set(x, y + 3, z + dz, 'minecraft:obsidian'); } for (let dy = 0; dy <= 3; dy++) { w.set(x, y - 1 + dy, z - 1, 'minecraft:obsidian'); w.set(x, y - 1 + dy, z + 2, 'minecraft:obsidian'); } },
  cauldron: (w, x, y, z) => { w.set(x, y, z, 'minecraft:cauldron'); },
  'water-cauldron': (w, x, y, z) => { w.set(x, y, z, 'minecraft:water_cauldron', { level: '2' }); },
  'powder-snow': (w, x, y, z) => { w.set(x, y, z, 'minecraft:powder_snow'); },
  'bubble-column': (w, x, y, z) => { w.set(x, y, z, 'minecraft:bubble_column', { drag: 'true' }); w.set(x, y - 1, z, 'minecraft:soul_sand'); },
  'wet-sponge': (w, x, y, z) => { w.set(x, y, z, 'minecraft:wet_sponge'); },
  'falling-block': () => {},
};

// Crops and other plants that bone meal grows: the ground they stand on, then the plant.
const farm = (w, x, y, z, plant, states) => { w.set(x, y - 1, z, 'minecraft:farmland', { moisture: '7' }); w.set(x, y, z, plant, states); };
const GROWTH_TARGETS = {
  'wheat-0': (w, x, y, z) => farm(w, x, y, z, 'minecraft:wheat', { age: '0' }),
  'wheat-5': (w, x, y, z) => farm(w, x, y, z, 'minecraft:wheat', { age: '5' }),
  'wheat-7': (w, x, y, z) => farm(w, x, y, z, 'minecraft:wheat', { age: '7' }),
  'carrots-2': (w, x, y, z) => farm(w, x, y, z, 'minecraft:carrots', { age: '2' }),
  'potatoes-4': (w, x, y, z) => farm(w, x, y, z, 'minecraft:potatoes', { age: '4' }),
  'beetroots-0': (w, x, y, z) => farm(w, x, y, z, 'minecraft:beetroots', { age: '0' }),
  'beetroots-2': (w, x, y, z) => farm(w, x, y, z, 'minecraft:beetroots', { age: '2' }),
  'beetroots-3': (w, x, y, z) => farm(w, x, y, z, 'minecraft:beetroots', { age: '3' }),
  'torchflower-0': (w, x, y, z) => farm(w, x, y, z, 'minecraft:torchflower_crop', { age: '0' }),
  'torchflower-1': (w, x, y, z) => farm(w, x, y, z, 'minecraft:torchflower_crop', { age: '1' }),
  'melon-stem-1': (w, x, y, z) => farm(w, x, y, z, 'minecraft:melon_stem', { age: '1' }),
  'pumpkin-stem-0': (w, x, y, z) => farm(w, x, y, z, 'minecraft:pumpkin_stem', { age: '0' }),
  'stem-6': (w, x, y, z) => farm(w, x, y, z, 'minecraft:pumpkin_stem', { age: '6' }),
  'stem-7': (w, x, y, z) => farm(w, x, y, z, 'minecraft:pumpkin_stem', { age: '7' }),
  'cocoa-0': (w, x, y, z) => { w.set(x + 1, y, z, 'minecraft:jungle_log', { axis: 'y' }); w.set(x, y, z, 'minecraft:cocoa', { age: '0', facing: 'west' }); },
  'cocoa-1': (w, x, y, z) => { w.set(x + 1, y, z, 'minecraft:jungle_log', { axis: 'y' }); w.set(x, y, z, 'minecraft:cocoa', { age: '1', facing: 'west' }); },
  'cocoa-2': (w, x, y, z) => { w.set(x + 1, y, z, 'minecraft:jungle_log', { axis: 'y' }); w.set(x, y, z, 'minecraft:cocoa', { age: '2', facing: 'west' }); },
  'berries-0': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:dirt'); w.set(x, y, z, 'minecraft:sweet_berry_bush', { age: '0' }); },
  'berries-2': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:dirt'); w.set(x, y, z, 'minecraft:sweet_berry_bush', { age: '2' }); },
  'berries-3': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:dirt'); w.set(x, y, z, 'minecraft:sweet_berry_bush', { age: '3' }); },
  'short-grass': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:grass_block', { snowy: 'false' }); w.set(x, y, z, 'minecraft:short_grass'); },
  fern: (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:dirt'); w.set(x, y, z, 'minecraft:fern'); },
  'short-grass-covered': (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:grass_block', { snowy: 'false' }); w.set(x, y, z, 'minecraft:short_grass'); w.set(x, y + 1, z, 'minecraft:stone'); },
  sunflower: (w, x, y, z) => { w.set(x, y - 1, z, 'minecraft:grass_block', { snowy: 'false' }); w.set(x, y, z, 'minecraft:sunflower', { half: 'lower' }); w.set(x, y + 1, z, 'minecraft:sunflower', { half: 'upper' }); },
  'rose-bush-top': (w, x, y, z) => { w.set(x, y - 2, z, 'minecraft:grass_block', { snowy: 'false' }); w.set(x, y - 1, z, 'minecraft:rose_bush', { half: 'lower' }); w.set(x, y, z, 'minecraft:rose_bush', { half: 'upper' }); },
};
Object.assign(TARGETS_FOR_DISPENSERS, GROWTH_TARGETS);

// [item, snbt or undefined, targets]: what a dispenser fires at what.
function dispenserCases() {
  const everything = Object.keys(TARGETS_FOR_DISPENSERS).filter((target) => !(target in GROWTH_TARGETS));
    const cases = [];
  const add = (item, targets, snbt, count = 1, extra) => targets.forEach((target) => cases.push({ item, target, snbt, count, extra }));
  const refused = ({ item, target }) => /_bucket$/.test(item) && /cod|salmon|puffer|tropical|axolotl|tadpole/.test(item) && target !== 'stone' || item === 'minecraft:bone_meal' && target === 'water' || target === 'tnt' && !/stick|redstone|oak_planks|:bucket$|:tnt$/.test(item) || target === 'powder-snow' && item !== 'minecraft:bucket' || /cauldron/.test(target) || target === 'powder-snow' && item !== 'minecraft:bucket' || item === 'minecraft:bone_meal' && target === 'rooted-dirt' || item === 'minecraft:carved_pumpkin' && target === 'snow-blocks' || item === 'minecraft:wither_skeleton_skull' && target === 'snow-blocks';
  for (const bucket of ['minecraft:water_bucket', 'minecraft:lava_bucket']) add(bucket, [...everything.filter((t) => t !== 'falling-block')]);
  add('minecraft:bucket', everything.filter((t) => t !== 'falling-block'));
  add('minecraft:bucket', ['water', 'lava', 'wet-slab', 'powder-snow', 'bubble-column'], undefined, 3);
  add('minecraft:water_bucket', ['air', 'water', 'stone'], undefined, 3);
  add('minecraft:powder_snow_bucket', ['air', 'air-over-air', 'stone', 'water', 'slab', 'wet-slab', 'kelp', 'rail']);
  for (const fish of ['cod_bucket', 'salmon_bucket', 'pufferfish_bucket', 'tropical_fish_bucket', 'axolotl_bucket', 'tadpole_bucket']) add(`minecraft:${fish}`, ['stone', 'lava']);
  add('minecraft:flint_and_steel', everything.filter((t) => t !== 'falling-block' && t !== 'tnt'));
  add('minecraft:flint_and_steel', ['air', 'campfire'], '{id:"minecraft:flint_and_steel",count:1,components:{"minecraft:damage":63}}');
  // Unbreaking removes some of the damage by the level's random.
  for (const [level, damage] of [[1, 10], [2, 40], [3, 62], [3, 63], [1, 63], [5, 62]]) {
    add('minecraft:flint_and_steel', ['air', 'campfire'], `{id:"minecraft:flint_and_steel",count:1,components:{"minecraft:damage":${damage},"minecraft:enchantments":{"minecraft:unbreaking":${level},"minecraft:sharpness":2}}}`);
  }
  add('minecraft:bone_meal', everything.filter((t) => t !== 'falling-block'), undefined, 3);
  add('minecraft:bone_meal', Object.keys(GROWTH_TARGETS).filter((t) => !/stem-/.test(t)), undefined, 4);
  // A stem that one more bone meal ripens random ticks: one is dispensed.
  add('minecraft:bone_meal', ['melon-stem-1', 'pumpkin-stem-0', 'stem-7'], undefined, 1);
  add('minecraft:tnt', ['air', 'stone', 'water', 'tnt']);
  add('minecraft:glowstone', ['anchor0', 'anchor3', 'anchor4', 'stone', 'air'], undefined, 5);
  add('minecraft:honeycomb', ['copper', 'cut-copper-stairs', 'copper-door', 'stone', 'air', 'lava'], undefined, 3);
  add('minecraft:glass_bottle', ['air', 'water', 'flowing-water', 'stone', 'beehive', 'beehive-empty', 'lava', 'wet-slab'], undefined, 3);
  add('minecraft:potion', ['dirt', 'coarse-dirt', 'rooted-dirt', 'stone', 'air'], potionStack('minecraft:potion', 'minecraft:water'), 1);
  add('minecraft:potion', ['dirt', 'air'], potionStack('minecraft:potion', 'minecraft:healing'));
  add('minecraft:potion', ['dirt'], undefined);
  for (const box of ['shulker_box', 'red_shulker_box', 'blue_shulker_box']) add(`minecraft:${box}`, ['air', 'air-over-air', 'stone', 'water', 'lava', 'falling-block'], undefined, 2);
  add('minecraft:shulker_box', ['air'], '{id:"minecraft:shulker_box",count:1,components:{"minecraft:container":[{slot:0,item:{id:"minecraft:diamond",count:5}},{slot:5,item:{id:"minecraft:stick",count:9}}],"minecraft:custom_name":"Box"}}');
  for (const cart of ['minecart', 'chest_minecart', 'furnace_minecart', 'tnt_minecart', 'hopper_minecart']) add(`minecraft:${cart}`, ['air', 'stone', 'air-over-air', 'rail', 'rail-below', 'rail-slope', 'rail-slope-below']);
  for (const boat of ['oak_boat', 'bamboo_raft', 'cherry_chest_boat']) add(`minecraft:${boat}`, ['air', 'stone', 'water', 'air-over-water', 'flowing-water']);
  add('minecraft:shears', ['air', 'beehive', 'beehive-empty']);
  add('minecraft:shears', ['air'], '{id:"minecraft:shears",count:1,components:{"minecraft:damage":237}}');
  add('minecraft:brush', ['air']);
  add('minecraft:chest', ['air', 'stone']);
  for (const armor of ['iron_helmet', 'leather_boots', 'elytra', 'turtle_helmet', 'skeleton_skull', 'creeper_head', 'saddle', 'diamond_horse_armor', 'wolf_armor', 'white_harness', 'shield']) add(`minecraft:${armor}`, ['air', 'stone']);
  add('minecraft:carved_pumpkin', ['air', 'stone', 'snow-blocks', 'air-over-air']);
  add('minecraft:wither_skeleton_skull', ['air', 'stone', 'snow-blocks']);
  add('minecraft:stick', ['air'], undefined, 5);
  add('minecraft:redstone', ['air', 'stone'], undefined, 2);
  add('minecraft:oak_planks', ['air'], undefined, 64);
  return cases.filter((c) => !refused(c));
}

// What the engine refuses: entities and blocks it does not model.
function unportedCases() {
  const cases = [];
  const add = (item, targets, snbt, count = 1) => targets.forEach((target) => cases.push({ item, target, snbt, count }));
  for (const fish of ['cod_bucket', 'salmon_bucket', 'pufferfish_bucket', 'tropical_fish_bucket', 'axolotl_bucket', 'tadpole_bucket']) add(`minecraft:${fish}`, ['air', 'water']);
  add('minecraft:wither_skeleton_skull', ['soul-sand-T']);
  add('minecraft:carved_pumpkin', ['snow-blocks']);
  add('minecraft:bone_meal', ['rooted-dirt', 'stem-6']);
  add('minecraft:flint_and_steel', ['tnt']);
  for (const item of ['arrow', 'spectral_arrow', 'egg', 'snowball', 'experience_bottle', 'splash_potion', 'firework_rocket', 'fire_charge', 'wind_charge', 'creeper_spawn_egg', 'armor_stand']) add(`minecraft:${item}`, ['air']);
  add('minecraft:bone_meal', ['water', 'stone']);
  return cases;
}

function dispensers(slice, mutate, source = dispenserCases) {
  const only = process.env.MCBENCH_DISPENSER_ONLY ? new RegExp(process.env.MCBENCH_DISPENSER_ONLY) : null;
  const range = process.env.MCBENCH_DISPENSER_RANGE ? process.env.MCBENCH_DISPENSER_RANGE.split('-').map(Number) : null;
  const cases = source().filter((c, index) => slice(index) && (!only || only.test(`${c.item} ${c.target}`))).filter((_, index) => !range || index >= range[0] && index < range[1]);
  const columns = 8;
  const spacing = 16;
  const world = new World();
  const rows = Math.ceil(cases.length / columns);
  world.fill([-4, 0, -4], [columns * spacing + 4, 0, rows * spacing + 4], 'minecraft:stone');
  const actions = [];
  const probes = [];
  cases.forEach((c, index) => {
    const x = (index % columns) * spacing + 2;
    const z = Math.floor(index / columns) * spacing + 2;
    // Random by what the case is, so that a filtered run keeps each case as it was.
    const r = random([...`${c.item} ${c.target}`].reduce((hash, ch) => (hash * 31 + ch.charCodeAt(0)) | 0, 7));
    const direction = c.target === 'air-over-water' || c.target === 'falling-block' ? 'east' : r.pick(['east', 'east', 'east', 'north', 'west', 'south']);
    const [dx, , dz] = DIRS[direction];
    const target = [x + dx, 1, z + dz];
    world.set(x, 1, z, 'minecraft:dispenser', { facing: direction, triggered: 'false' });
    // A lever on the side the dispenser's front does not face.
    const side = direction === 'east' || direction === 'west' ? ['north', 0, -1] : ['east', 1, 0];
    world.set(x + side[1], 1, z + side[2], 'minecraft:lever', { face: 'wall', facing: side[0], powered: 'false' });
    // The target is built for a dispenser facing east; turned to the facing in use by mirroring its offsets.
    const build = TARGETS_FOR_DISPENSERS[c.target];
    const shim = {
      set: (px, py, pz, id, states) => {
        const rx = px - target[0];
        const rz = pz - target[2];
        const [wx, wz] = direction === 'east' ? [rx, rz] : direction === 'west' ? [-rx, -rz] : direction === 'north' ? [rz, -rx] : [-rz, rx];
        world.set(target[0] + wx, py, target[2] + wz, id, states);
      },
    };
    build(shim, target[0], 1, target[2]);
    // A pumpkin facing: nothing to do.
    if (c.target === 'falling-block') world.set(target[0], 6, target[2], 'minecraft:sand');
    actions.push(insert(1, [x, 1, z], 0, c.item, c.count, c.snbt));
    // A second, different stack so the dispenser's random slot has a choice now and then.
    if (r.chance(0.25) && c.item !== 'minecraft:flint_and_steel' && !/powder.snow/.test(c.item + c.target)) actions.push(insert(1, [x, 1, z], 1 + r.int(8), r.pick(['minecraft:stick', 'minecraft:apple', 'minecraft:water_bucket', 'minecraft:bucket']), 1 + r.int(2)));
    actions.push({ tick: 3 + r.int(3), type: 'toggle', at: [x + side[1], 1, z + side[2]] });
    actions.push({ tick: 12, type: 'toggle', at: [x + side[1], 1, z + side[2]] });
    actions.push({ tick: 15, type: 'toggle', at: [x + side[1], 1, z + side[2]] });
    probes.push([x, 1, z], target);
  });
  if (mutate) mutate(world, actions, probes);
  world.finalize();
  return { world, args: { probes, inventory_probes: probes.filter((_, index) => index % 2 === 0), inventory_missing: 'report', world_inventories: true, item_entities: true, settle_ticks: 1, samples: [0, 6, 14, 30], actions } };
}

// --- Droppers and dispensers feeding composters, jukeboxes, furnaces and shelves ---

function dropperRigs(seed) {
  const r = random(seed);
  const kinds = ['composter', 'jukebox', 'furnace', 'shelf', 'brewing'];
  const rigs = kinds.flatMap((kind) => Array.from({ length: 6 }, () => kind));
  const { world, probes } = stationsWorld(rigs.length, 6, (w, x, z, index) => {
    const [block, states] = TARGETS[rigs[index]];
    w.set(x, 1, z, block, rigs[index] === 'composter' && r.chance(0.5) ? { level: String(r.int(8)) } : states);
    w.set(x, 2, z, 'minecraft:dropper', { facing: 'down', triggered: 'false' });
    w.set(x + 1, 2, z, 'minecraft:lever', { face: 'wall', facing: 'east', powered: 'false' });
    w.set(x, 3, z, 'minecraft:dispenser', { facing: 'down', triggered: 'false' });
    w.set(x + 1, 3, z, 'minecraft:lever', { face: 'wall', facing: 'east', powered: 'false' });
    w.set(x - 1, 1, z, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
    w.set(x - 2, 1, z, 'minecraft:redstone_lamp', { lit: 'false' });
  }, { width: 6 });
  world.finalize();
  const actions = [];
  const watched = [];
  rigs.forEach((kind, index) => {
    const [x, , z] = probes[index];
    const items = rigItems(kind, r);
    items.top.slice(0, 9).forEach((stack, slot) => {
      actions.push(insert(1, [x, 2, z], slot, stack.item, stack.count, stack.snbt));
    });
    items.side.slice(0, 5).forEach((stack, slot) => actions.push(insert(1, [x, 3, z], slot, stack.item, stack.count, stack.snbt)));
    for (let k = 0; k < 8; k++) {
      actions.push({ tick: 3 + k * 4, type: 'toggle', at: [x + 1, 2, z] });
      actions.push({ tick: 4 + k * 4, type: 'toggle', at: [x + 1, 3, z] });
    }
    watched.push([x, 1, z], [x, 2, z], [x, 3, z], [x - 2, 1, z]);
  });
  return { world, args: { probes: watched, inventory_probes: watched.filter((_, index) => index % 4 !== 3), inventory_missing: 'report', world_inventories: true, item_entities: true, settle_ticks: 1, samples: [0, 10, 40, 80], actions } };
}

module.exports = { registry, random, slotted, blockEntity, insert, potionStack, lazy, crafter, craftersWorld, recipeGrid, sequence, NOW, fixtures: () => [
  ...crafting(),
  lazy('furnaces-0', () => furnaces(21, (index) => index % 2 === 0)),
  lazy('furnaces-1', () => furnaces(22, (index) => index % 2 === 1)),
  lazy('brewing', () => brewing(31)),
  lazy('jukeboxes', () => jukeboxes()),
  lazy('hoppers-furnaces', () => hopperRigs(41, ['furnace', 'blast', 'smoker'], 8)),
  lazy('hoppers-brewing', () => hopperRigs(42, ['brewing'], 16)),
  lazy('hoppers-jukebox', () => hopperRigs(43, ['jukebox'], 12)),
  lazy('hoppers-shelf', () => hopperRigs(44, ['shelf'], 12)),
  lazy('hoppers-composter', () => hopperRigs(45, ['composter', 'barrel'], 12)),
  ...[0, 1, 2, 3].map((part) => lazy(`dispensers-${part}`, () => dispensers((index) => index % 4 === part))),
  lazy('droppers', () => dropperRigs(71)),
  lazy('dispensers-unported', () => dispensers(() => true, undefined, unportedCases)),
] };
