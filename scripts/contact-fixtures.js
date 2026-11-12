'use strict';

// Verifier fixtures for what the blocks entities touch do to them: entityInside
// (tripwires, fire, soul fire, powder snow, cauldrons, cobwebs, cactus, campfires,
// sweet berry bushes, frogspawn, portals), fallOn (farmland, hay, beds, pointed
// dripstone, turtle eggs), stepOn, and the collision shapes that depend on the
// entity (scaffolding, powder snow). Each block is met by every kind of
// entity the engine has: items, falling blocks, armor stands, boats and minecarts,
// experience orbs (every row has two, one burning),
// dropped from above or standing in it. Fixtures are named contact/<case>;
// scripts/verifier-fixtures.js lists them.

const { World } = require('../server/world');

const NOW = 4556;
const sequence = (count) => Array.from({ length: count }, (_, index) => index);
const ground = (world, x0, z0, x1, z1) => world.fill([x0, 0, z0], [x1, 0, z1], 'minecraft:stone');

const uuid = (n) => `[I;${n},${n + 1},${n + 2},${n + 3}]`;
const itemTag = (n, pos, item, extra) => ({ at: pos, data_version: NOW, snbt: `{id:"minecraft:item",UUID:${uuid(n)},Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],Item:${item},Age:0s,PickupDelay:0s${extra ? `,${extra}` : ''}}` });
const entityTag = (n, id, pos, extra = '') => ({ at: pos, data_version: NOW, snbt: `{id:"minecraft:${id}",UUID:${uuid(n)},Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f]${extra ? `,${extra}` : ''}}` });

// The kinds of entity a cell holds, by name: where each stands in the cell
// (x, z the block's corner), at the height y.
const KINDS = {
  item: (n, [x, z], y) => itemTag(n, [x + 0.5, y, z + 0.5], '{id:"minecraft:diamond",count:3}'),
  stick: (n, [x, z], y) => itemTag(n, [x + 0.5, y, z + 0.5], '{id:"minecraft:stick",count:2}'),
  netherite: (n, [x, z], y) => itemTag(n, [x + 0.5, y, z + 0.5], '{id:"minecraft:netherite_ingot",count:1}'),
  stand: (n, [x, z], y) => entityTag(n, 'armor_stand', [x + 0.5, y, z + 0.5]),
  boots: (n, [x, z], y) => entityTag(n, 'armor_stand', [x + 0.5, y, z + 0.5], 'equipment:{feet:{id:"minecraft:leather_boots",count:1}}'),
  invisible: (n, [x, z], y) => entityTag(n, 'armor_stand', [x + 0.5, y, z + 0.5], 'Invisible:1b'),
  marker: (n, [x, z], y) => entityTag(n, 'armor_stand', [x + 0.5, y, z + 0.5], 'Marker:1b'),
  small: (n, [x, z], y) => entityTag(n, 'armor_stand', [x + 0.5, y, z + 0.5], 'Small:1b'),
  boat: (n, [x, z], y) => entityTag(n, 'oak_boat', [x + 0.5, y, z + 0.5]),
  cart: (n, [x, z], y) => entityTag(n, 'minecart', [x + 0.5, y, z + 0.5]),
  tnt: (n, [x, z], y) => entityTag(n, 'tnt_minecart', [x + 0.5, y, z + 0.5]),
  chest: (n, [x, z], y) => entityTag(n, 'chest_minecart', [x + 0.5, y, z + 0.5]),
  burning: (n, [x, z], y) => itemTag(n, [x + 0.5, y, z + 0.5], '{id:"minecraft:diamond",count:3}', 'Fire:100s'),
  burningstand: (n, [x, z], y) => entityTag(n, 'armor_stand', [x + 0.5, y, z + 0.5], 'Fire:100s'),
  orb: (n, [x, z], y) => entityTag(n, 'experience_orb', [x + 0.5, y, z + 0.5], 'Value:3,Count:1,Age:0s,Health:5s'),
  burningorb: (n, [x, z], y) => entityTag(n, 'experience_orb', [x + 0.5, y, z + 0.5], 'Value:17,Count:2,Age:0s,Health:3s,Fire:100s'),
};
// Every cell row is met by experience orbs too.
const ORBS = ['orb', 'burningorb'];
// A falling block is made by a block with nothing under it.
const FALLING = 'sand';

// A world of rows of cells: row r (z = 4 r + 2) holds the setup of the block
// under test, column c (x = 4 c + 2) the kind of entity that meets it. `setup(world,
// x, z, kind)` puts the blocks of a cell in (the block at y = 1; the ground is at y = 0).
// Entities start at the row's `y` (3 blocks above the ground block by default); a
// falling block starts at the same height.
function contact({ rows, kinds: own, ground: base = null }) {
  const kinds = [...own, ...ORBS];
  const startOf = (row) => row.y ?? 4.0;
  const world = () => {
    const w = new World();
    ground(w, 0, 0, 4 * kinds.length + 2, 4 * rows.length + 2);
    rows.forEach((row, r) => {
      kinds.forEach((kind, c) => {
        const x = 4 * c + 2;
        const z = 4 * r + 2;
        if (base) w.set(x, 0, z, base);
        row.setup(w, x, z, kind);
        if (kind === FALLING) w.set(x, Math.floor(startOf(row)), z, 'minecraft:sand');
      });
    });
    w.finalize();
    return w;
  };
  let n = 1000;
  const entities = [];
  rows.forEach((row, r) => {
    kinds.forEach((kind, c) => {
      if (kind === FALLING) return;
      entities.push(KINDS[kind]((n += 10), [4 * c + 2, 4 * r + 2], startOf(row)));
    });
  });
  return { world, entities };
}

const request = (entities, samples, extra = {}) => ({
  probes: [[2, 1, 2]], item_entities: true, world_inventories: true, living_entities: true, samples: sequence(samples), settle_ticks: 1, entities, ...extra,
});

// One fixture of a row set: the world and the request.
function make(name, spec, samples, extra) {
  const { world, entities } = contact(spec);
  const probes = [];
  spec.rows.forEach((row, r) => [...spec.kinds, ...ORBS].forEach((kind, c) => { for (const dy of row.probes ?? [0]) probes.push([4 * c + 2, 1 + dy, 4 * r + 2]); }));
  return { name: `contact/${name}`, world, args: { ...request(entities, samples, extra), probes }, placementMode: spec.placementMode };
}

const ALL = ['item', 'stand', 'boat', 'cart', FALLING];
const MOBLESS = ['item', 'stand', 'boat', 'cart', 'tnt', FALLING];
const FIRE = { age: '0', north: 'false', east: 'false', south: 'false', west: 'false', up: 'false' };
const WIRE = { attached: 'false', disarmed: 'false', powered: 'false', north: 'false', east: 'false', south: 'false', west: 'false' };
const set = (id, states) => (w, x, z) => w.set(x, 1, z, id, states);

const FIRST = () => [
  // Fire and soul fire: entities that burn and are hurt; a stand catches fire; a TNT minecart ignites.
  make('fire', {
    kinds: [...MOBLESS, 'netherite', 'invisible'],
    rows: [
      { setup: set('minecraft:fire', FIRE), y: 1.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:soul_soil'); w.set(x, 1, z, 'minecraft:soul_fire'); }, y: 1.0 },
    ],
  }, 240),
  make('fire-dropped', { kinds: ALL, rows: [{ setup: set('minecraft:fire', FIRE) }] }, 160),
  // Tripwires between hooks: each cell is a hook, a tripwire and a hook along x; the entity meets the wire.
  make('tripwire', {
    kinds: ['item', 'stand', 'marker', 'boat', 'cart', FALLING],
    rows: [
      {
        setup: (w, x, z) => {
          w.set(x - 1, 1, z, 'minecraft:tripwire_hook', { facing: 'east', attached: 'false', powered: 'false' });
          w.set(x, 1, z, 'minecraft:tripwire', { ...WIRE, attached: 'true', east: 'true', west: 'true' });
          w.set(x + 1, 1, z, 'minecraft:tripwire_hook', { facing: 'west', attached: 'false', powered: 'false' });
        },
        y: 2.0,
      },
      { setup: set('minecraft:tripwire', WIRE), y: 2.0 },
      { setup: set('minecraft:tripwire', { ...WIRE, attached: 'true' }), y: 3.0 },
    ],
  }, 100),
  // Powder snow: entities fall into it (from enough height to be stopped by it), stand in it, burn in it.
  make('powder-snow', {
    kinds: ['item', 'stand', 'boots', 'boat', 'cart', FALLING, 'burning', 'burningstand'],
    rows: [
      { setup: set('minecraft:powder_snow'), y: 4.0 },
      { setup: (w, x, z) => { w.set(x, 1, z, 'minecraft:powder_snow'); w.set(x, 2, z, 'minecraft:powder_snow'); }, y: 9.0 },
      { setup: set('minecraft:powder_snow'), y: 1.2 },
    ],
  }, 160),
  // Scaffolding: stable and unstable, with and without the bottom, in columns.
  make('scaffolding', {
    kinds: ['item', 'stand', 'boots', 'boat', 'cart', FALLING],
    rows: [
      { setup: set('minecraft:scaffolding', { bottom: 'false', distance: '0', waterlogged: 'false' }), y: 4.0 },
      { setup: set('minecraft:scaffolding', { bottom: 'true', distance: '3', waterlogged: 'false' }), y: 4.0 },
      { setup: set('minecraft:scaffolding', { bottom: 'false', distance: '3', waterlogged: 'true' }), y: 4.0 },
      { setup: (w, x, z) => { w.set(x, 1, z, 'minecraft:scaffolding', { bottom: 'false', distance: '1', waterlogged: 'false' }); w.set(x, 2, z, 'minecraft:scaffolding', { bottom: 'true', distance: '4', waterlogged: 'false' }); }, y: 5.0 },
      { setup: set('minecraft:scaffolding', { bottom: 'true', distance: '2', waterlogged: 'false' }), y: 1.1 },
    ],
  }, 120),
];

const MORE = () => [
  // Cauldrons: water at every level, powder snow, lava; entities fall in, a burning one is put out.
  make('cauldrons', {
    kinds: ['item', 'stand', 'boat', 'cart', FALLING, 'burning', 'burningstand'],
    rows: [
      { setup: set('minecraft:cauldron'), y: 3.0 },
      { setup: set('minecraft:water_cauldron', { level: '1' }), y: 3.0 },
      { setup: set('minecraft:water_cauldron', { level: '2' }), y: 3.0 },
      { setup: set('minecraft:water_cauldron', { level: '3' }), y: 3.0 },
      { setup: set('minecraft:powder_snow_cauldron', { level: '3' }), y: 3.0 },
      { setup: set('minecraft:powder_snow_cauldron', { level: '1' }), y: 3.0 },
      { setup: set('minecraft:lava_cauldron'), y: 3.0 },
    ],
  }, 160),
  // Cobwebs and cactus.
  make('web-cactus', {
    kinds: ALL,
    rows: [
      { setup: set('minecraft:cobweb'), y: 4.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:sand'); w.set(x, 1, z, 'minecraft:cactus', { age: '0' }); }, y: 3.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:sand'); w.set(x, 1, z, 'minecraft:cactus', { age: '0' }); w.set(x, 2, z, 'minecraft:cactus', { age: '0' }); }, y: 4.0 },
    ],
  }, 160),
  // What entities land on: farmland, hay, a bed, pointed dripstone, turtle eggs, slime, honey.
  make('landing', {
    kinds: ['item', 'stand', 'small', 'boat', 'cart', 'tnt', FALLING],
    rows: [
      { setup: set('minecraft:farmland', { moisture: '3' }), y: 12.0 },
      { setup: set('minecraft:hay_block', { axis: 'y' }), y: 12.0 },
      { setup: (w, x, z) => { w.set(x, 1, z, 'minecraft:white_bed', { facing: 'east', occupied: 'false', part: 'foot' }); w.set(x + 1, 1, z, 'minecraft:white_bed', { facing: 'east', occupied: 'false', part: 'head' }); }, y: 12.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:dripstone_block'); w.set(x, 1, z, 'minecraft:pointed_dripstone', { thickness: 'tip', vertical_direction: 'up', waterlogged: 'false' }); }, y: 12.0 },
      { setup: set('minecraft:turtle_egg', { eggs: '3', hatch: '0' }), y: 8.0 },
      { setup: set('minecraft:turtle_egg', { eggs: '1', hatch: '0' }), y: 3.0 },
      { setup: set('minecraft:slime_block'), y: 12.0 },
      { setup: set('minecraft:honey_block'), y: 12.0 },
    ],
  }, 200),
  // Blocks that act on living entities (the armor stand) or on falling blocks.
  make('plants', {
    kinds: ['item', 'stand', 'boat', 'cart', FALLING],
    rows: [
      { setup: (w, x, z) => w.set(x, 1, z, 'minecraft:campfire', { facing: 'north', lit: 'true', signal_fire: 'false', waterlogged: 'false' }), y: 3.0 },
      { setup: (w, x, z) => w.set(x, 1, z, 'minecraft:soul_campfire', { facing: 'north', lit: 'true', signal_fire: 'false', waterlogged: 'false' }), y: 3.0 },
      { setup: (w, x, z) => w.set(x, 1, z, 'minecraft:campfire', { facing: 'north', lit: 'false', signal_fire: 'false', waterlogged: 'false' }), y: 3.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:dirt'); w.set(x, 1, z, 'minecraft:sweet_berry_bush', { age: '3' }); }, y: 3.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:dirt'); w.set(x, 1, z, 'minecraft:sweet_berry_bush', { age: '0' }); }, y: 3.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:dirt'); w.set(x, 1, z, 'minecraft:wither_rose'); }, y: 3.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:water', { level: '0' }); w.set(x, 1, z, 'minecraft:frogspawn'); }, y: 3.0 },
      { setup: (w, x, z) => { w.set(x, 0, z, 'minecraft:dirt'); w.set(x, 1, z, 'minecraft:open_eyeblossom'); }, y: 3.0 },
    ],
  }, 160),
  // What entities rest on: stepOn of redstone ore, sculk sensors and shriekers, turtle eggs, magma, slime.
  make('stepping', {
    kinds: ['item', 'stand', 'small', 'boat', 'cart', FALLING],
    rows: [
      { setup: set('minecraft:redstone_ore', { lit: 'false' }), y: 3.0 },
      { setup: set('minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' }), y: 3.0 },
      { setup: set('minecraft:calibrated_sculk_sensor', { facing: 'north', power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' }), y: 3.0 },
      { setup: set('minecraft:sculk_shrieker', { can_summon: 'true', shrieking: 'false', waterlogged: 'false' }), y: 3.0 },
      { setup: set('minecraft:turtle_egg', { eggs: '4', hatch: '0' }), y: 3.0 },
      { setup: set('minecraft:magma_block'), y: 3.0 },
      { setup: set('minecraft:soul_sand'), y: 3.0 },
      { setup: set('minecraft:honey_block'), y: 2.2 },
    ],
  }, 200),
  // Portals: an entity inside a nether portal, an end portal or an end gateway.
  make('portals', {
    kinds: ['item', 'stand', 'boat', 'cart', FALLING],
    rows: [
      {
        setup: (w, x, z) => {
          // A portal one block wide, three high, in an obsidian frame (axis x).
          for (const dy of [0, 1, 2, 3, 4]) { w.set(x - 1, 1 + dy, z, 'minecraft:obsidian'); w.set(x + 1, 1 + dy, z, 'minecraft:obsidian'); }
          w.set(x, 1, z, 'minecraft:obsidian');
          w.set(x, 5, z, 'minecraft:obsidian');
          for (const dy of [1, 2, 3]) w.set(x, 1 + dy, z, 'minecraft:nether_portal', { axis: 'x' });
        },
        y: 5.5,
      },
      { setup: set('minecraft:end_portal'), y: 3.0 },
      { setup: set('minecraft:end_gateway'), y: 3.0 },
    ],
  }, 100),
];

module.exports = { fixtures: () => [...FIRST(), ...MORE()] };
