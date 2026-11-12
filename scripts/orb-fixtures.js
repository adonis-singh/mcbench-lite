'use strict';

// Verifier fixtures for experience orbs (net.minecraft.world.entity.ExperienceOrb)
// and the golems a carved pumpkin makes. Orbs are imported in every state their
// saved data can give (value, count, age, health, fire, gravity, motion, Paper's
// ExpData) and meet what moves and hurts them: gravity and the ground's
// friction, water and lava, fire, TNT, pistons, slime, ice, soul sand, a nether
// portal, the void; orbs of a value in the same group merge (enough of them that
// the ids fall in one group of the 40), and furnaces with recipes used give orbs
// when TNT breaks them (into orbs already there, of the values they split into).
// Pressure plates, tripwires and sculk sensors show what the orbs touch.
// Fixtures are named orbs/<case>; scripts/verifier-fixtures.js lists them.

const { World } = require('../server/world');

const NOW = 4556;
const sequence = (count) => Array.from({ length: count }, (_, index) => index);
const ground = (world, x0, z0, x1, z1, id = 'minecraft:stone') => world.fill([x0, 0, z0], [x1, 0, z1], id);
const uuid = (n) => `[I;${n},${n + 1},${n + 2},${n + 3}]`;

// An orb at pos with saved fields.
const orbTag = (n, pos, fields = 'Value:3') => ({
  at: pos, data_version: NOW,
  snbt: `{id:"minecraft:experience_orb",UUID:${uuid(n)},Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f]${fields ? `,${fields}` : ''}}`,
});
const blockEntity = (at, id, fields) => ({ at, data_version: NOW, snbt: `{id:"minecraft:${id}"${fields ? `,${fields}` : ''}}` });
const PLATE = { powered: 'false' };
const WEIGHTED = { power: '0' };
const LAMP = { lit: 'false' };

// Orbs falling on stone, ice, soul sand, slime and honey, in water and lava,
// burning, with no gravity, moving, invulnerable, silent, about to despawn,
// low on health, of every value bracket; a wooden plate, a weighted plate, a
// tripwire and a sculk sensor they land on.
const physicsWorld = () => {
  const w = new World();
  ground(w, -2, -2, 40, 30);
  w.fill([2, 0, 4], [8, 0, 4], 'minecraft:packed_ice');
  w.fill([2, 0, 6], [8, 0, 6], 'minecraft:soul_sand');
  w.fill([2, 0, 8], [8, 0, 8], 'minecraft:slime_block');
  w.fill([2, 0, 10], [8, 0, 10], 'minecraft:honey_block');
  // A pool of water two deep, a pool of lava, walled.
  w.fill([11, 1, 1], [17, 3, 7], 'minecraft:glass');
  w.fill([12, 1, 2], [16, 2, 6], 'minecraft:water', { level: '0' });
  w.fill([11, 1, 9], [17, 2, 15], 'minecraft:glass');
  w.fill([12, 1, 10], [16, 1, 14], 'minecraft:lava', { level: '0' });
  // Flowing water down a slope.
  w.fill([20, 1, 2], [26, 1, 2], 'minecraft:stone');
  w.set(20, 2, 2, 'minecraft:water', { level: '0' });
  // Plates, a tripwire, a sculk sensor, each with a lamp it lights.
  w.set(2, 1, 14, 'minecraft:oak_pressure_plate', PLATE);
  w.set(2, 1, 15, 'minecraft:redstone_lamp', LAMP);
  w.set(5, 1, 14, 'minecraft:light_weighted_pressure_plate', WEIGHTED);
  w.set(5, 1, 15, 'minecraft:redstone_lamp', LAMP);
  w.set(8, 1, 14, 'minecraft:stone_pressure_plate', PLATE);
  w.set(8, 1, 15, 'minecraft:redstone_lamp', LAMP);
  w.set(1, 1, 18, 'minecraft:tripwire_hook', { facing: 'east', attached: 'false', powered: 'false' });
  w.fill([2, 1, 18], [4, 1, 18], 'minecraft:tripwire', { attached: 'true', disarmed: 'false', powered: 'false', north: 'false', south: 'false', east: 'true', west: 'true' });
  w.set(5, 1, 18, 'minecraft:tripwire_hook', { facing: 'west', attached: 'false', powered: 'false' });
  w.set(8, 1, 18, 'minecraft:sculk_sensor', { power: '0', sculk_sensor_phase: 'inactive', waterlogged: 'false' });
  // A stone block an orb is inside, a column with a gap.
  w.set(20, 1, 10, 'minecraft:stone');
  w.set(23, 1, 10, 'minecraft:stone');
  w.set(23, 3, 10, 'minecraft:stone');
  w.set(26, 1, 10, 'minecraft:oak_slab', { type: 'bottom', waterlogged: 'false' });
  // A nether portal.
  for (const dy of [0, 1, 2, 3, 4]) { w.set(29, 1 + dy, 20, 'minecraft:obsidian'); w.set(31, 1 + dy, 20, 'minecraft:obsidian'); }
  w.set(30, 1, 20, 'minecraft:obsidian');
  w.set(30, 5, 20, 'minecraft:obsidian');
  for (const dy of [1, 2, 3]) w.set(30, 1 + dy, 20, 'minecraft:nether_portal', { axis: 'x' });
  // Fire on netherrack, a cobweb, powder snow.
  w.set(20, 1, 20, 'minecraft:netherrack');
  w.set(20, 2, 20, 'minecraft:fire', { age: '0', north: 'false', east: 'false', south: 'false', west: 'false', up: 'false' });
  w.set(23, 1, 20, 'minecraft:cobweb');
  w.set(26, 1, 20, 'minecraft:powder_snow');
  w.finalize();
  return w;
};
const physicsEntities = () => {
  let n = 100;
  const at = (x, y, z, fields) => orbTag((n += 10), [x, y, z], fields);
  return [
    at(2.5, 3.0, 2.5, 'Value:1'),
    at(4.5, 6.0, 2.5, 'Value:7,Count:3'),
    at(6.5, 1.0, 2.5, 'Value:2477,Count:1,Motion:[0.3d,0.2d,-0.1d]'),
    at(2.5, 3.0, 4.5, 'Value:17,Motion:[0.4d,0.0d,0.0d]'),
    at(2.5, 3.0, 6.5, 'Value:17,Motion:[0.4d,0.0d,0.0d]'),
    at(2.5, 6.0, 8.5, 'Value:37'),
    at(5.5, 3.0, 8.5, 'Value:73,Motion:[0.0d,-0.5d,0.0d]'),
    at(2.5, 3.0, 10.5, 'Value:149'),
    at(13.5, 4.0, 3.5, 'Value:3'),
    at(15.5, 1.5, 5.5, 'Value:307,Count:2'),
    at(13.5, 4.0, 11.5, 'Value:617'),
    at(15.5, 2.5, 13.5, 'Value:1237,Health:20s'),
    at(20.5, 3.0, 2.5, 'Value:3'),
    at(2.5, 3.0, 14.5, 'Value:1'),
    at(5.5, 3.0, 14.5, 'Value:1'),
    at(5.6, 3.5, 14.4, 'Value:1'),
    at(8.5, 3.0, 14.5, 'Value:1'),
    at(3.5, 3.0, 18.5, 'Value:3'),
    at(8.5, 4.0, 18.5, 'Value:3'),
    at(20.5, 1.2, 10.5, 'Value:3'),
    at(23.5, 2.1, 10.5, 'Value:3'),
    at(26.5, 1.2, 10.5, 'Value:3'),
    at(30.5, 6.0, 20.5, 'Value:3'),
    at(20.5, 5.0, 20.5, 'Value:3'),
    at(23.5, 4.0, 20.5, 'Value:3'),
    at(26.5, 4.0, 20.5, 'Value:3'),
    at(30.5, 2.0, 2.5, 'Value:3,Fire:60s'),
    at(32.5, 2.0, 2.5, 'Value:3,Fire:30s,Health:1s'),
    at(34.5, 5.0, 2.5, 'Value:3,NoGravity:1b,Motion:[0.0d,0.1d,0.05d]'),
    at(36.5, 2.0, 2.5, 'Value:3,Age:5990s'),
    at(38.5, 2.0, 2.5, 'Value:3,Invulnerable:1b,Fire:60s'),
    at(30.5, 2.0, 5.5, 'Value:3,Silent:1b'),
    at(32.5, 2.0, 5.5, 'Value:5,Count:7,Age:-3s,Health:2s'),
    at(34.5, 2.0, 5.5, 'Value:0'),
    at(36.5, 2.0, 5.5, 'Count:4'),
    at(38.5, 2.0, 5.5, 'Value:3,"Paper.ExpData":{source:[I;1,2,3,4],trigger:[I;5,6,7,8],reason:"FURNACE"}'),
    at(30.5, 2.0, 8.5, 'Value:3,"Paper.ExpData":{reason:"BLOCK_BREAK"}'),
    at(32.5, 2.0, 8.5, 'Value:3,"Paper.ExpData":{}'),
    at(34.5, 2.0, 8.5, 'Value:3,CustomName:"orb",fall_distance:4.0d'),
  ];
};
const physicsRequest = {
  probes: [[2, 1, 14], [2, 1, 15], [5, 1, 14], [5, 1, 15], [8, 1, 14], [8, 1, 15], [1, 1, 18], [5, 1, 18], [8, 1, 18], [20, 2, 20], [26, 1, 20]],
  item_entities: true, living_entities: true, samples: sequence(260), settle_ticks: 1, entities: physicsEntities(),
};

// Clusters of orbs of one value (enough that ids 40 apart meet), with
// clusters of mixed values and counts; merging happens every 20 ticks.
const mergeWorld = () => {
  const w = new World();
  ground(w, -2, -2, 12, 12);
  w.set(10, 1, 10, 'minecraft:oak_pressure_plate', PLATE);
  w.set(10, 1, 11, 'minecraft:redstone_lamp', LAMP);
  w.finalize();
  return w;
};
const mergeEntities = () => {
  const entities = [];
  let n = 1000;
  for (let i = 0; i < 90; i++) entities.push(orbTag((n += 10), [2.5 + (i % 3) * 0.1, 1.0 + (i % 5) * 0.05, 2.5], `Value:${i % 2 ? 3 : 7},Count:${1 + (i % 4)},Age:${i * 7}s`));
  for (let i = 0; i < 45; i++) entities.push(orbTag((n += 10), [6.5, 3.0 + i * 0.02, 6.5 + (i % 2) * 0.3], 'Value:1'));
  for (let i = 0; i < 42; i++) entities.push(orbTag((n += 10), [10.5, 1.0, 10.5], 'Value:17'));
  return entities;
};
const mergeRequest = { probes: [[10, 1, 10], [10, 1, 11]], samples: sequence(120), settle_ticks: 1, entities: mergeEntities() };

// Furnaces with recipes used (counts whose experience is a whole number:
// Math.random is not drawn), broken by TNT a redstone block primes: their
// orbs at the furnace's center, merged into orbs there of their value; orbs
// near the TNT hurt and thrown. Blocks with experience (ores, sculk) broken by
// the same explosions give none (no player caused them).
const furnaceWorld = () => {
  const w = new World();
  ground(w, -4, -4, 30, 12);
  const cells = [
    [2, 'minecraft:furnace'], [8, 'minecraft:blast_furnace'], [14, 'minecraft:smoker'], [20, 'minecraft:furnace'], [26, 'minecraft:furnace'],
  ];
  for (const [x, id] of cells) {
    w.set(x, 1, 2, id, { facing: 'north', lit: 'false' });
    w.set(x + 2, 1, 2, 'minecraft:tnt', { unstable: 'false' });
    w.set(x + 2, 1, 4, 'minecraft:redstone_lamp', LAMP);
    w.set(x, 1, 6, 'minecraft:coal_ore');
    w.set(x + 1, 1, 6, 'minecraft:redstone_ore', { lit: 'false' });
    w.set(x + 2, 1, 6, 'minecraft:sculk_catalyst', { bloom: 'false' });
    w.set(x, 2, 6, 'minecraft:diamond_ore');
    w.set(x + 1, 2, 6, 'minecraft:sculk');
  }
  // A lever beside each TNT primes it when toggled.
  for (const [x] of cells) w.set(x + 3, 1, 2, 'minecraft:lever', { face: 'floor', facing: 'east', powered: 'false' });
  w.finalize();
  return w;
};
const recipes = (pairs) => `RecipesUsed:{${pairs.map(([id, count]) => `"${id}":${count}`).join(',')}}`;
const furnaceRequest = {
  probes: [[2, 1, 2], [8, 1, 2], [14, 1, 2], [20, 1, 2], [26, 1, 2], [4, 1, 4], [10, 1, 4]], samples: sequence(140), settle_ticks: 1,
  block_entities: [
    blockEntity([2, 1, 2], 'furnace', recipes([['minecraft:iron_ingot_from_smelting_raw_iron', 10]])),
    blockEntity([8, 1, 2], 'blast_furnace', recipes([['minecraft:iron_ingot_from_blasting_raw_iron', 20]])),
    blockEntity([14, 1, 2], 'smoker', recipes([['minecraft:cooked_beef_from_smoking', 20]])),
    blockEntity([20, 1, 2], 'furnace', recipes([['minecraft:diamond_from_smelting_diamond_ore', 3]])),
    blockEntity([26, 1, 2], 'furnace', recipes([['minecraft:not_a_recipe', 3], ['minecraft:iron_ingot_from_smelting_iron_ore', 0], ['minecraft:iron_ingot_from_nuggets', 4]])),
  ],
  entities: [
    orbTag(500, [2.5, 1.5, 2.5], 'Value:7'),
    orbTag(510, [2.6, 1.6, 2.4], 'Value:3,Count:2'),
    orbTag(520, [8.5, 1.2, 3.5], 'Value:7'),
    orbTag(530, [20.5, 2.5, 3.5], 'Value:7'),
    orbTag(540, [4.5, 2.5, 4.5], 'Value:1'),
  ],
  actions: [5, 11, 17, 23, 29].map((x, i) => ({ tick: 3 + i * 9, type: 'toggle', at: [x, 1, 2] })),
};

// Pistons push orbs, and a moving block carries them; a hopper under them
// takes nothing.
const pistonWorld = () => {
  const w = new World();
  ground(w, -2, -2, 14, 8);
  for (const z of [1, 4]) {
    w.set(1, 1, z, 'minecraft:piston', { facing: 'east', extended: 'false' });
    w.set(0, 1, z, 'minecraft:lever', { face: 'wall', facing: 'west', powered: 'false' });
  }
  w.set(1, 1, 7, 'minecraft:sticky_piston', { facing: 'up', extended: 'false' });
  w.set(2, 1, 7, 'minecraft:lever', { face: 'wall', facing: 'east', powered: 'false' });
  w.set(1, 2, 7, 'minecraft:slime_block');
  w.set(6, 1, 4, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  w.finalize();
  return w;
};
const pistonRequest = {
  probes: [[1, 1, 1], [1, 1, 4], [1, 1, 7], [6, 1, 4]], inventory_probes: [[6, 1, 4]], samples: sequence(80), settle_ticks: 1,
  entities: [orbTag(700, [2.5, 1.0, 1.5], 'Value:3'), orbTag(710, [2.3, 1.0, 4.5], 'Value:7'), orbTag(720, [1.5, 3.0, 7.5], 'Value:1'), orbTag(730, [6.5, 2.0, 4.5], 'Value:1')],
  actions: [{ tick: 5, type: 'toggle', at: [0, 1, 1] }, { tick: 9, type: 'toggle', at: [0, 1, 4] }, { tick: 13, type: 'toggle', at: [2, 1, 7] }, { tick: 30, type: 'toggle', at: [0, 1, 1] }],
};

// Golem patterns a pumpkin completes as it is placed (the placement in
// `updates` mode): the golems spawn, the patterns clear, a copper block turns
// into a copper chest, and the golems live for 400 ticks (the Rust engine
// refuses their AI, which is not ported). A copper golem's brain is not
// reproducible in Paper itself once it strolls (see scripts/mob-fixtures.js),
// and its draws from the entities' shared random then change every other
// mob's: it is boxed in glass where it cannot move (`boxed`) and alone, and a
// free copper golem, alone or with the others, is compared only for the two
// ticks after it spawns. One world of each kind, one of snow and iron, and
// one of all three.
const golemWorld = (kinds) => () => {
  const w = new World();
  ground(w, -2, -2, 14, 6);
  if (kinds.includes('snow')) {
    w.set(1, 1, 1, 'minecraft:snow_block');
    w.set(1, 2, 1, 'minecraft:snow_block');
    w.set(1, 3, 1, 'minecraft:carved_pumpkin', { facing: 'south' });
  }
  if (kinds.includes('iron')) {
    w.set(5, 1, 1, 'minecraft:iron_block');
    w.fill([4, 2, 1], [6, 2, 1], 'minecraft:iron_block');
    w.set(5, 3, 1, 'minecraft:jack_o_lantern', { facing: 'south' });
  }
  if (kinds.includes('copper')) {
    w.set(10, 1, 1, 'minecraft:waxed_exposed_copper');
    w.set(10, 2, 1, 'minecraft:carved_pumpkin', { facing: 'west' });
    w.set(10, 1, 2, 'minecraft:oxidized_copper_chest', { facing: 'west', type: 'single', waterlogged: 'false' });
  }
  if (kinds.includes('boxed')) {
    for (const [x, z] of [[9, 1], [11, 1], [10, 0], [10, 2]]) w.set(x, 2, z, 'minecraft:glass');
    w.set(10, 3, 1, 'minecraft:glass');
  }
  w.finalize();
  return w;
};
const golemRequest = (ticks) => ({ probes: [[1, 1, 1], [5, 2, 1], [10, 1, 1], [10, 1, 2]], living_entities: true, samples: sequence(ticks + 1), settle_ticks: 1 });

function fixtures() {
  return [
    { name: 'orbs/physics', world: physicsWorld, args: physicsRequest, placementMode: 'strict' },
    { name: 'orbs/physics-updates', world: physicsWorld, args: physicsRequest, placementMode: 'updates' },
    { name: 'orbs/merge', world: mergeWorld, args: mergeRequest, placementMode: 'strict' },
    { name: 'orbs/furnaces', world: furnaceWorld, args: furnaceRequest, placementMode: 'strict' },
    { name: 'orbs/furnaces-updates', world: furnaceWorld, args: furnaceRequest, placementMode: 'updates' },
    { name: 'orbs/pistons', world: pistonWorld, args: pistonRequest, placementMode: 'strict' },
    { name: 'golems/patterns', world: golemWorld(['snow', 'iron', 'copper']), args: golemRequest(2), placementMode: 'updates' },
    { name: 'golems/snow-iron', world: golemWorld(['snow', 'iron']), args: golemRequest(400), placementMode: 'updates' },
    { name: 'golems/snow', world: golemWorld(['snow']), args: golemRequest(400), placementMode: 'updates' },
    { name: 'golems/iron', world: golemWorld(['iron']), args: golemRequest(400), placementMode: 'updates' },
    { name: 'golems/copper', world: golemWorld(['copper', 'boxed']), args: golemRequest(400), placementMode: 'updates' },
    { name: 'golems/copper-free', world: golemWorld(['copper']), args: golemRequest(2), placementMode: 'updates' },
  ];
}

module.exports = { fixtures, orbTag };
