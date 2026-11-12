'use strict';

// Verifier fixtures for the mobs a build can make, run for hundreds of ticks:
// what a dispenser's spawn eggs and mob buckets spawn, silverfish from
// infested blocks, bees leaving and finding their hives, golems from carved
// pumpkins in a village's points of interest. Each mob gets a glass pen of
// its own (water for water mobs) so that nothing another mob does reaches it.
// Fixtures are named mobs/<case>; scripts/verifier-fixtures.js lists them.
//
// Paper's brains are not all reproducible: every Sensor starts its scan
// phase from Sensor.RANDOM and every RunOne behaviour shuffles with a
// RandomSource of its own, neither seeded by anything, so two seeded runs of
// Paper itself disagree once such a mob strolls (allays, armadillos, camels,
// copper golems, frogs, goats, sniffers and villagers on land, a few dozen to
// a few hundred ticks in, an allay two ticks after it spawns). Those are
// compared until the tick after they spawn (BRAIN_TICKS), which still takes
// them through spawning and their first tick; the others, goal mobs and the
// axolotls and tadpoles in water (which agree across runs), over TICKS. A
// brain's draws from the entities' shared random would change any other
// mob's, which is one more reason each mob is alone in its world.
//
// The verifier's world is peaceful: monsters (from eggs, or the silverfish
// infested blocks release) are removed at their first tick.

const { World } = require('../server/world');

const NOW = 4556;
const sequence = (count) => Array.from({ length: count }, (_, index) => index);
const slotted = (slot, item, count) => `{Slot:${slot}b,id:"minecraft:${item}",count:${count}}`;
const blockEntity = (at, id, fields) => ({ at, data_version: NOW, snbt: `{id:"minecraft:${id}"${fields ? `,${fields}` : ''}}` });
const TICKS = 400;
// The dispenser fires at tick 6 (its lever is pulled at tick 2).
const BRAIN_TICKS = 7;

// A pen: a stone floor at y 1 with a dispenser facing up in its middle, a
// lever under the dispenser, glass walls and a glass roof; `water` fills it.
// The interior is x and z 0..size-1, y 2..height+1.
const pen = (world, { size = 7, height = 4, water = false, floor = 'minecraft:stone' } = {}) => {
  const middle = size >> 1;
  world.fill([-1, 1, -1], [size, 1, size], floor);
  world.fill([-1, 2, -1], [size, height + 2, -1], 'minecraft:glass');
  world.fill([-1, 2, size], [size, height + 2, size], 'minecraft:glass');
  world.fill([-1, 2, 0], [-1, height + 2, size - 1], 'minecraft:glass');
  world.fill([size, 2, 0], [size, height + 2, size - 1], 'minecraft:glass');
  world.fill([0, height + 2, 0], [size - 1, height + 2, size - 1], 'minecraft:glass');
  if (water) world.fill([0, 2, 0], [size - 1, height + 1, size - 1], 'minecraft:water', { level: '0' });
  world.set(middle, 1, middle, 'minecraft:dispenser', { facing: 'up', triggered: 'false' });
  world.set(middle, 0, middle, 'minecraft:lever', { face: 'ceiling', facing: 'north', powered: 'false' });
  return middle;
};

// One item dispensed into a pen of its own, `ticks` sampled.
const dispensed = (item, options = {}, ticks = TICKS) => {
  const size = options.size || 7;
  const middle = size >> 1;
  return {
    world: () => {
      const w = new World();
      pen(w, options);
      w.finalize();
      return w;
    },
    args: {
      probes: [[middle, 1, middle]], inventory_probes: [[middle, 1, middle]], living_entities: true, item_entities: true,
      samples: sequence(ticks + 1), settle_ticks: 1,
      block_entities: [blockEntity([middle, 1, middle], 'dispenser', `Items:[${slotted(4, item, 1)}]`)],
      actions: [{ tick: 2, type: 'toggle', at: [middle, 0, middle] }],
    },
  };
};

// Mobs whose AI is goals only, from spawn eggs, on land and in water.
const GOAL_MOBS = ['bat', 'bee', 'cat', 'chicken', 'cow', 'donkey', 'fox', 'horse', 'iron_golem', 'llama', 'mooshroom', 'mule', 'ocelot', 'panda', 'parrot', 'pig', 'polar_bear', 'rabbit', 'sheep', 'skeleton_horse', 'snow_golem', 'strider', 'trader_llama', 'turtle', 'wandering_trader', 'wolf', 'zombie_horse'];
const WATER_GOAL_MOBS = ['cod', 'dolphin', 'glow_squid', 'pufferfish', 'salmon', 'squid', 'tropical_fish'];
// Mobs with brains whose strolls Paper does not reproduce (see above), and
// brain mobs in water that it does.
const BRAIN_MOBS = ['allay', 'armadillo', 'camel', 'copper_golem', 'frog', 'goat', 'sniffer', 'villager'];
const WATER_BRAIN_MOBS = ['axolotl', 'tadpole'];
// Monsters, which a peaceful world removes at their first tick.
const MONSTERS = ['creeper', 'endermite', 'hoglin', 'piglin', 'silverfish', 'slime', 'zombie', 'zombified_piglin'];
const BUCKETS = ['cod', 'pufferfish', 'salmon', 'tropical_fish'];
const WATER_BRAIN_BUCKETS = ['axolotl', 'tadpole'];

// Infested blocks that TNT breaks, each giving a silverfish (removed, the
// world being peaceful, at its first tick).
const infestedWorld = () => {
  const w = new World();
  w.fill([-2, 0, -2], [12, 0, 6], 'minecraft:stone');
  const infested = ['infested_stone', 'infested_cobblestone', 'infested_stone_bricks', 'infested_mossy_stone_bricks', 'infested_cracked_stone_bricks', 'infested_chiseled_stone_bricks'];
  infested.forEach((id, i) => w.set(i * 2, 1, 2, `minecraft:${id}`));
  w.set(5, 1, 0, 'minecraft:tnt', { unstable: 'false' });
  w.set(6, 1, 0, 'minecraft:lever', { face: 'floor', facing: 'east', powered: 'false' });
  w.set(11, 1, 2, 'minecraft:infested_deepslate', { axis: 'y' });
  w.finalize();
  return w;
};
const infestedRequest = {
  probes: [[0, 1, 2], [4, 1, 2], [10, 1, 2], [11, 1, 2]], living_entities: true, item_entities: true, samples: sequence(TICKS + 1), settle_ticks: 1,
  actions: [{ tick: 2, type: 'toggle', at: [6, 1, 0] }],
};

// Bees in a hive and a nest (points of interest the placement registers),
// leaving at noon for the flowers and finding a hive again.
const beeWorld = () => {
  const w = new World();
  w.fill([-6, 0, -6], [14, 0, 14], 'minecraft:grass_block', { snowy: 'false' });
  w.set(0, 1, 0, 'minecraft:beehive', { facing: 'south', honey_level: '0' });
  w.set(8, 1, 8, 'minecraft:bee_nest', { facing: 'north', honey_level: '4' });
  w.set(4, 1, 2, 'minecraft:poppy');
  w.set(5, 1, 2, 'minecraft:dandelion');
  w.set(3, 1, 6, 'minecraft:cornflower');
  w.finalize();
  return w;
};
const bee = (ticks, min) => `{entity_data:{id:"minecraft:bee"},ticks_in_hive:${ticks},min_ticks_in_hive:${min}}`;
const beeRequest = {
  probes: [[0, 1, 0], [8, 1, 8]], living_entities: true, item_entities: true, samples: sequence(TICKS + 1), settle_ticks: 1,
  block_entities: [
    blockEntity([0, 1, 0], 'beehive', `bees:[${bee(0, 20)},${bee(10, 40)}]`),
    blockEntity([8, 1, 8], 'beehive', `bees:[${bee(0, 60)}]`),
  ],
};

// An iron golem a pumpkin makes among a village's points of interest
// (a bell, beds, workstations: none occupied, no villager having claimed one).
const villageWorld = () => {
  const w = new World();
  w.fill([-12, 0, -12], [20, 0, 20], 'minecraft:grass_block', { snowy: 'false' });
  w.set(4, 1, 4, 'minecraft:bell', { attachment: 'floor', facing: 'north', powered: 'false' });
  w.set(0, 1, 8, 'minecraft:white_bed', { facing: 'north', part: 'foot', occupied: 'false' });
  w.set(0, 1, 7, 'minecraft:white_bed', { facing: 'north', part: 'head', occupied: 'false' });
  w.set(8, 1, 0, 'minecraft:composter', { level: '0' });
  w.set(9, 1, 0, 'minecraft:lectern', { facing: 'north', has_book: 'false', powered: 'false' });
  w.set(10, 1, 0, 'minecraft:smithing_table');
  w.set(6, 1, 9, 'minecraft:iron_block');
  w.fill([5, 2, 9], [7, 2, 9], 'minecraft:iron_block');
  w.set(6, 3, 9, 'minecraft:carved_pumpkin', { facing: 'south' });
  w.finalize();
  return w;
};
const villageRequest = { probes: [[4, 1, 4], [0, 1, 8], [8, 1, 0]], living_entities: true, samples: sequence(TICKS + 1), settle_ticks: 1 };

function fixtures() {
  return [
    ...GOAL_MOBS.map((mob) => ({ name: `mobs/egg-${mob}`, ...dispensed(`${mob}_spawn_egg`, mob === 'iron_golem' ? { size: 9 } : {}) })),
    ...WATER_GOAL_MOBS.map((mob) => ({ name: `mobs/egg-${mob}`, ...dispensed(`${mob}_spawn_egg`, { water: true, size: 9 }) })),
    ...BRAIN_MOBS.map((mob) => ({ name: `mobs/egg-${mob}`, ...dispensed(`${mob}_spawn_egg`, {}, BRAIN_TICKS) })),
    ...WATER_BRAIN_MOBS.map((mob) => ({ name: `mobs/egg-${mob}`, ...dispensed(`${mob}_spawn_egg`, { water: true }) })),
    ...MONSTERS.map((mob) => ({ name: `mobs/egg-${mob}`, ...dispensed(`${mob}_spawn_egg`, {}, 20) })),
    ...BUCKETS.map((fish) => ({ name: `mobs/bucket-${fish}`, ...dispensed(`${fish}_bucket`, { size: 9 }) })),
    ...BUCKETS.map((fish) => ({ name: `mobs/bucket-${fish}-water`, ...dispensed(`${fish}_bucket`, { water: true, size: 9 }) })),
    ...WATER_BRAIN_BUCKETS.map((mob) => ({ name: `mobs/bucket-${mob}`, ...dispensed(`${mob}_bucket`, { water: true }) })),
    { name: 'mobs/infested', world: infestedWorld, args: infestedRequest, placementMode: 'strict' },
    { name: 'mobs/bees', world: beeWorld, args: beeRequest, placementMode: 'strict' },
    { name: 'mobs/village-golem', world: villageWorld, args: villageRequest, placementMode: 'updates' },
  ];
}

module.exports = { fixtures };
