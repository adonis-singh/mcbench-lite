'use strict';

// Verifier fixtures for entities in portals. An entity in a nether portal
// goes to Paper's Nether on its first tick there (Entity.handlePortal,
// teleportCrossDimension) and is gone from the build's world; one on its
// portal cooldown waits in it; an end portal and an end gateway with no exit
// portal lead nowhere. Each kind of entity the engines have is summoned in a
// portal or carried into one (by a piston, a dropper, a dispenser, its own
// fall, rails), with what reads it (a detector rail and its comparator, a
// hopper under a hopper minecart, pressure plates, a tripwire); a rider goes
// with its vehicle. After an entity has gone to the Nether, Paper's Nether
// draws from the entities' shared random, so the engines refuse a draw of it
// (portal/then-drop and portal/dropper-twice are refused for that). An end
// gateway with an exit portal moves an entity to the exit (gateway/<case>).
//
// A fixture marked `nether` sends an entity to the Nether: Paper's Nether
// goes on drawing from the shared random after the experiment, so
// scripts/ticker-parity.js runs it on a server of its own. Fixtures are named
// portal/<case> and gateway/<case>; scripts/verifier-fixtures.js lists them.

const { World } = require('../server/world');

const NOW = 4556;
const sequence = (count) => Array.from({ length: count }, (_, index) => index);
const uuid = (n) => `[I;${n},${n + 1},${n + 2},${n + 3}]`;
const tag = (n, id, pos, extra = '') => ({
  at: pos, data_version: NOW,
  snbt: `{id:"minecraft:${id}",UUID:${uuid(n)},Pos:[${pos[0]}d,${pos[1]}d,${pos[2]}d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f]${extra ? `,${extra}` : ''}}`,
});
const item = (n, pos, stack = '{id:"minecraft:diamond",count:3}', extra = '') => tag(n, 'item', pos, `Item:${stack},Age:0s,PickupDelay:0s${extra ? `,${extra}` : ''}`);
const blockEntity = (at, id, fields) => ({ at, data_version: NOW, snbt: `{id:"minecraft:${id}"${fields ? `,${fields}` : ''}}` });
const LEVER = { face: 'floor', facing: 'north', powered: 'false' };

// A nether portal two wide and three high along x, in its obsidian frame:
// the portal blocks at x0 and x0 + 1, z, from y b + 1 up; `top` false leaves
// the frame open above (a portal block keeps until a neighbour changes).
function portal(w, x0, z, b = 0, { top = true, topLeft = 'minecraft:obsidian' } = {}) {
  for (let y = b; y <= b + 4; y++) { w.set(x0 - 1, y, z, 'minecraft:obsidian'); w.set(x0 + 2, y, z, 'minecraft:obsidian'); }
  w.set(x0, b, z, 'minecraft:obsidian');
  w.set(x0 + 1, b, z, 'minecraft:obsidian');
  if (top) { w.set(x0, b + 4, z, topLeft); w.set(x0 + 1, b + 4, z, 'minecraft:obsidian'); }
  for (let y = b + 1; y <= b + 3; y++) { w.set(x0, y, z, 'minecraft:nether_portal', { axis: 'x' }); w.set(x0 + 1, y, z, 'minecraft:nether_portal', { axis: 'x' }); }
}

// Stone in front of (z + 1 ...) a portal at ground level.
const ground = (w, x0, x1, z0, z1) => w.fill([x0, 0, z0], [x1, 0, z1], 'minecraft:stone');

const request = (extra) => ({ probes: [[0, 0, 0]], samples: sequence(41), settle_ticks: 1, item_entities: true, living_entities: true, world_inventories: true, ...extra });

// The smallest case: a one-wide portal (no frame Minecraft would light, but
// placed it keeps), and a minecart falling into it from the frame's top.
const minimalWorld = () => {
  const w = new World();
  for (let y = 1; y <= 5; y++) { w.set(13, y, 2, 'minecraft:obsidian'); w.set(15, y, 2, 'minecraft:obsidian'); }
  w.set(14, 1, 2, 'minecraft:obsidian');
  w.set(14, 5, 2, 'minecraft:obsidian');
  for (const y of [2, 3, 4]) w.set(14, y, 2, 'minecraft:nether_portal', { axis: 'x' });
  w.finalize();
  return w;
};
const minimalRequest = {
  probes: [[14, 2, 2], [14, 3, 2], [14, 4, 2]], samples: sequence(21), settle_ticks: 1, world_inventories: true,
  entities: [tag(100, 'minecart', [14.5, 5.5, 2.5])],
};

// One portal (open above, with `top` false) and the entities in it
// (INSIDE), before it (OUTSIDE, on the stone in front) or above it (ABOVE,
// falling in some ticks after the import).
const oneWorld = (extra = () => {}, top = true) => () => {
  const w = new World();
  ground(w, -2, 5, 3, 6);
  portal(w, 1, 2, 0, { top });
  extra(w);
  w.finalize();
  return w;
};
const INSIDE = [2.0, 1.0, 2.5];
const OUTSIDE = [2.0, 1.0, 3.5];
const ABOVE = [2.0, 9.0, 2.5];
const one = (name, entities, extra = {}, world = oneWorld(undefined, false)) => ({
  name: `portal/${name}`, world, nether: true,
  args: request({ probes: [[1, 1, 2], [2, 3, 2]], entities, ...extra }),
});

// Each kind of entity the verifier imports, falling into a portal open
// above: an item, an armor stand, a boat, each kind of minecart (holding
// items, fuel, TNT), an experience orb; and an item summoned in a framed
// portal, with another of the same stack before it that stays, with nothing
// to merge with. (Primed TNT, which the verifier does not import, is a
// dispenser's: portal/dispenser-tnt.)
const kinds = () => [
  one('item', [item(200, INSIDE), item(210, OUTSIDE)], {}, oneWorld()),
  one('falling-item', [item(205, ABOVE)]),
  one('stand', [tag(220, 'armor_stand', ABOVE)]),
  one('boat', [tag(230, 'oak_boat', ABOVE)]),
  one('minecart', [tag(240, 'minecart', ABOVE)]),
  one('chest-minecart', [tag(250, 'chest_minecart', ABOVE, 'Items:[{Slot:0b,id:"minecraft:diamond",count:5},{Slot:26b,id:"minecraft:stick",count:2}]')]),
  one('hopper-minecart', [tag(260, 'hopper_minecart', ABOVE, 'Items:[{Slot:0b,id:"minecraft:diamond",count:5}]')]),
  one('furnace-minecart', [tag(270, 'furnace_minecart', ABOVE, 'Fuel:200s')]),
  one('tnt-minecart', [tag(280, 'tnt_minecart', ABOVE)]),
  // An orb is observed by what it would land on: wooden pressure plates in
  // the portal's bottom row.
  one('orb', [tag(300, 'experience_orb', ABOVE, 'Value:3,Count:1,Age:0s,Health:5s')], { probes: [[1, 1, 2], [2, 1, 2], [1, 2, 2]] }, oneWorld((w) => {
    w.set(1, 1, 2, 'minecraft:oak_pressure_plate', { powered: 'false' });
    w.set(2, 1, 2, 'minecraft:oak_pressure_plate', { powered: 'false' });
  }, false)),
];

// A minecart with a dolphin riding it (the one passenger the verifier
// imports): the dolphin goes with it, put off and moved as a passenger.
const rider = () => one('rider', [{
  at: ABOVE, data_version: NOW,
  snbt: `{id:"minecraft:minecart",UUID:${uuid(310)},Pos:[2.0d,9.0d,2.5d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],Passengers:[{id:"minecraft:dolphin",UUID:${uuid(320)},Pos:[2.0d,9.0d,2.5d],NoAI:1b,Air:4800s,Moistness:2400}]}`,
}]);

// Carried in: sand falling into a portal open above it; an armor stand a
// piston pushes in; an item a dropper drops into it, TNT a dispenser primes
// in it and an armor stand a dispenser stands in it (each fired by a lever
// at tick 2).
const carried = () => [
  {
    name: 'portal/falling-sand', nether: true,
    world: oneWorld((w) => w.set(1, 9, 2, 'minecraft:sand'), false),
    args: request({ probes: [[1, 1, 2], [1, 3, 2], [1, 9, 2]] }),
  },
  {
    name: 'portal/piston', nether: true,
    world: oneWorld((w) => {
      w.set(1, 1, 4, 'minecraft:piston', { facing: 'north', extended: 'false' });
      w.set(1, 1, 5, 'minecraft:lever', LEVER);
    }),
    args: request({ probes: [[1, 1, 4], [1, 1, 3]], entities: [tag(330, 'armor_stand', [1.5, 1.0, 3.5])], actions: [{ tick: 2, type: 'toggle', at: [1, 1, 5] }] }),
  },
  ...[['dropper', 'dropper', '{Slot:0b,id:"minecraft:diamond",count:1}'], ['dispenser-tnt', 'dispenser', '{Slot:0b,id:"minecraft:tnt",count:1}'], ['dispenser-stand', 'dispenser', '{Slot:0b,id:"minecraft:armor_stand",count:1}']].map(([name, block, stack]) => ({
    name: `portal/${name}`, nether: true,
    world: oneWorld((w) => {
      w.set(1, 1, 3, `minecraft:${block}`, { facing: 'north', triggered: 'false' });
      w.set(1, 1, 4, 'minecraft:lever', LEVER);
    }),
    args: request({
      probes: [[1, 1, 3], [1, 1, 2]], inventory_probes: [[1, 1, 3]],
      block_entities: [blockEntity([1, 1, 3], block, `Items:[${stack}]`)],
      actions: [{ tick: 2, type: 'toggle', at: [1, 1, 4] }],
    }),
  })),
];

// What reads an entity that goes: a hopper minecart rolling slowly off a
// detector rail over a hopper (the hopper pulls from it, a comparator reads
// it through the rail) into a portal; an armor stand on a stone pressure
// plate, an item on a wooden one and an armor stand on a tripwire, each
// standing at a portal's edge (in the plate or wire and the portal at once).
const readersWorld = () => {
  const w = new World();
  ground(w, -3, 30, 3, 7);
  portal(w, 1, 2);
  w.set(1, 0, 4, 'minecraft:hopper', { facing: 'down', enabled: 'true' });
  w.set(1, 1, 3, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' });
  w.set(1, 1, 4, 'minecraft:detector_rail', { shape: 'north_south', powered: 'false', waterlogged: 'false' });
  w.set(1, 1, 5, 'minecraft:rail', { shape: 'north_south', waterlogged: 'false' });
  w.set(2, 1, 4, 'minecraft:comparator', { facing: 'west', mode: 'compare', powered: 'false' });
  portal(w, 7, 2);
  w.set(7, 1, 3, 'minecraft:stone_pressure_plate', { powered: 'false' });
  portal(w, 13, 2);
  w.set(13, 1, 3, 'minecraft:oak_pressure_plate', { powered: 'false' });
  portal(w, 19, 2);
  w.set(17, 1, 3, 'minecraft:stone');
  w.set(18, 1, 3, 'minecraft:tripwire_hook', { facing: 'east', attached: 'true', powered: 'false' });
  for (const x of [19, 20]) w.set(x, 1, 3, 'minecraft:tripwire', { attached: 'true', disarmed: 'false', powered: 'false', north: 'false', south: 'false', east: 'true', west: 'true' });
  w.set(21, 1, 3, 'minecraft:tripwire_hook', { facing: 'west', attached: 'true', powered: 'false' });
  w.set(22, 1, 3, 'minecraft:stone');
  w.finalize();
  return w;
};
const readers = () => ({
  name: 'portal/readers', world: readersWorld, nether: true,
  args: request({
    probes: [[1, 1, 4], [2, 1, 4], [1, 0, 4], [7, 1, 3], [13, 1, 3], [18, 1, 3], [19, 1, 3], [21, 1, 3]], inventory_probes: [[1, 0, 4]], samples: sequence(61),
    entities: [
      tag(400, 'hopper_minecart', [1.5, 1.0, 5.2], 'Items:[{Slot:0b,id:"minecraft:diamond",count:5},{Slot:1b,id:"minecraft:stick",count:3}]'),
      tag(410, 'armor_stand', [7.5, 1.0, 3.0]),
      item(420, [13.5, 1.0, 3.0]),
      tag(430, 'armor_stand', [19.5, 1.0, 3.0]),
    ].map((entity, index) => index ? entity : { ...entity, snbt: entity.snbt.replace('Motion:[0.0d,0.0d,0.0d]', 'Motion:[0.0d,0.0d,-0.06d]') }),
  }),
});

// Waiting: an armor stand and an item on their portal cooldown stand in a
// portal, the cooldown kept up while they are in it, until a piston pushes
// a block of the frame away at tick 20 and the portal breaks.
const waitingWorld = () => {
  const w = new World();
  ground(w, -2, 5, 3, 6);
  portal(w, 1, 2, 0, { topLeft: 'minecraft:stone' });
  w.set(1, 4, 3, 'minecraft:piston', { facing: 'north', extended: 'false' });
  w.fill([0, 1, 3], [0, 3, 3], 'minecraft:stone');
  w.set(0, 4, 3, 'minecraft:lever', LEVER);
  w.finalize();
  return w;
};
const waiting = () => ({
  name: 'portal/waiting', world: waitingWorld,
  args: request({
    probes: [[1, 1, 2], [1, 3, 2], [1, 4, 2], [1, 4, 1]], samples: sequence(61),
    entities: [tag(500, 'armor_stand', [1.5, 1.0, 2.5], 'PortalCooldown:200'), item(510, [2.5, 1.0, 2.5], undefined, 'PortalCooldown:5')],
    actions: [{ tick: 20, type: 'toggle', at: [0, 4, 3] }],
  }),
});

// The end portal and an end gateway with no exit portal: entities fall into
// them, are in the portal (the cooldown starts; a gateway's cooldown starts
// and its block event goes out) and stay.
const endWorld = () => {
  const w = new World();
  ground(w, 0, 18, 0, 6);
  for (let c = 0; c < 4; c++) { w.set(4 * c + 2, 1, 2, 'minecraft:end_portal'); w.set(4 * c + 2, 1, 5, 'minecraft:end_gateway'); }
  w.finalize();
  return w;
};
const endKinds = (z, n) => [item(n, [2.5, 3.0, z + 0.5]), tag(n + 10, 'armor_stand', [6.5, 3.0, z + 0.5]), tag(n + 20, 'oak_boat', [10.5, 3.0, z + 0.5]), tag(n + 30, 'minecart', [14.5, 3.0, z + 0.5])];
const end = () => ({
  name: 'portal/end', world: endWorld,
  args: request({ probes: [2, 6, 10, 14].flatMap((x) => [[x, 1, 2], [x, 1, 5]]), samples: sequence(61), entities: [...endKinds(2, 600), ...endKinds(5, 700)] }),
});

// Refused: a draw of the shared random after an entity went to the Nether
// (an item dropped once the minecart has gone; a dropper firing a second
// time once its first item has gone).
const refused = () => [
  { name: 'portal/then-drop', world: minimalWorld, nether: true, args: { ...minimalRequest, item_entities: true, actions: [{ tick: 10, type: 'drop', at: [14.5, 8, 4.5], item: 'minecraft:stick', count: 1, velocity: [0, 0, 0] }] } },
  {
    name: 'portal/dropper-twice', nether: true,
    world: oneWorld((w) => {
      w.set(1, 1, 3, 'minecraft:dropper', { facing: 'north', triggered: 'false' });
      w.set(1, 1, 4, 'minecraft:lever', LEVER);
    }),
    args: request({
      probes: [[1, 1, 3]], block_entities: [blockEntity([1, 1, 3], 'dropper', 'Items:[{Slot:0b,id:"minecraft:diamond",count:2}]')],
      actions: [{ tick: 2, type: 'toggle', at: [1, 1, 4] }, { tick: 10, type: 'toggle', at: [1, 1, 4] }, { tick: 14, type: 'toggle', at: [1, 1, 4] }],
    }),
  },
];

// ---- End gateways with an exit portal: an entity in one goes to its exit
// in the build's world (EndGatewayBlock.getPortalDestination,
// TheEndGatewayBlockEntity.getPortalPosition, Entity.teleportSameDimension),
// keeping its motion and rotation, and the gateway cools down for 40 ticks.
// Block entities hold the exit as the save does: in Minecraft's coordinates
// (the build's y plus 64).

const MC_Y = 64;
const gateway = (at, exit, fields = '') => blockEntity(at, 'end_gateway', `exit_portal:[I;${exit[0]},${exit[1] + MC_Y},${exit[2]}]${fields ? `,${fields}` : ''}`);
const EXACT = 'ExactTeleport:1b';
const gatewayRequest = (extra) => request({ samples: sequence(61), ...extra });

// Each kind of entity falling into a gateway whose exit is exact, five
// blocks along on the same ground.
const GATEWAY_KINDS = [
  ['item', (n, pos) => item(n, pos)],
  ['stand', (n, pos) => tag(n, 'armor_stand', pos)],
  ['boat', (n, pos) => tag(n, 'oak_boat', pos)],
  ['minecart', (n, pos) => tag(n, 'minecart', pos)],
  ['chest', (n, pos) => tag(n, 'chest_minecart', pos, 'Items:[{Slot:0b,id:"minecraft:diamond",count:5}]')],
  ['hopper', (n, pos) => tag(n, 'hopper_minecart', pos, 'Items:[{Slot:0b,id:"minecraft:stick",count:2}]')],
  ['furnace', (n, pos) => tag(n, 'furnace_minecart', pos, 'Fuel:200s')],
  ['tnt', (n, pos) => tag(n, 'tnt_minecart', pos)],
  ['orb', (n, pos) => tag(n, 'experience_orb', pos, 'Value:3,Count:1,Age:0s,Health:5s')],
];
const groundWorld = (x1, z1, extra = () => {}) => () => {
  const w = new World();
  w.fill([-2, 0, -2], [x1, 0, z1], 'minecraft:stone');
  extra(w);
  w.finalize();
  return w;
};
const gatewayKinds = () => ({
  name: 'gateway/kinds',
  world: groundWorld(8 * GATEWAY_KINDS.length, 4, (w) => GATEWAY_KINDS.forEach((_, c) => w.set(8 * c + 1, 1, 1, 'minecraft:end_gateway'))),
  args: gatewayRequest({
    probes: GATEWAY_KINDS.flatMap((_, c) => [[8 * c + 1, 1, 1], [8 * c + 5, 0, 1]]),
    block_entities: GATEWAY_KINDS.map((_, c) => gateway([8 * c + 1, 1, 1], [8 * c + 5, 1, 1], EXACT)),
    entities: GATEWAY_KINDS.map(([, make], c) => make(800 + 10 * c, [8 * c + 1.5, 3.0, 1.5])),
  }),
});

// Exits that are not exact: the place findExitPosition finds, on the
// tallest full block within five columns of the exit (not its own, not
// bedrock), or above the exit where there is none.
const nonExactWorld = groundWorld(24, 8, (w) => {
  w.set(1, 1, 1, 'minecraft:end_gateway');
  w.fill([4, 1, 1], [4, 6, 1], 'minecraft:bedrock');
  w.fill([8, 1, 1], [8, 3, 1], 'minecraft:stone');
  w.fill([6, 1, 3], [6, 4, 3], 'minecraft:glass');
  w.fill([6, 1, 1], [6, 7, 1], 'minecraft:stone');
  w.set(14, 1, 1, 'minecraft:end_gateway');
});
const nonExact = () => ({
  name: 'gateway/not-exact', world: nonExactWorld,
  args: gatewayRequest({
    probes: [[1, 1, 1], [6, 5, 3], [14, 1, 1]],
    block_entities: [gateway([1, 1, 1], [6, 1, 1]), gateway([14, 1, 1], [20, 1, 6])],
    entities: [item(900, [1.5, 3.0, 1.5]), tag(910, 'armor_stand', [14.5, 3.0, 1.5])],
  }),
});

// A gateway cooling down: a second item arrives while the first one's
// cooldown lasts and goes when it ends; a gateway about to start its own
// cooldown (age 2390 of 2400) holds an item that comes then. Entities
// summoned in a gateway go on their first tick.
const coolingDown = () => ({
  name: 'gateway/cooldown',
  world: groundWorld(24, 4, (w) => { for (const x of [1, 9, 17]) w.set(x, 1, 1, 'minecraft:end_gateway'); }),
  args: gatewayRequest({
    probes: [[1, 1, 1], [9, 1, 1], [17, 1, 1]],
    block_entities: [gateway([1, 1, 1], [5, 1, 1], EXACT), gateway([9, 1, 1], [13, 1, 1], `${EXACT},Age:2390L`), gateway([17, 1, 1], [21, 1, 1], EXACT)],
    entities: [item(1000, [1.5, 3.0, 1.5]), item(1010, [1.5, 4.5, 1.5], '{id:"minecraft:stick",count:1}'), item(1020, [9.5, 5.0, 1.5]), item(1030, [17.5, 1.0, 1.5]), tag(1040, 'armor_stand', [17.5, 1.0, 1.5])],
  }),
});

// Exits at gateways: one gateway's exit is another's, whose exit is the
// first (the entity, on its portal cooldown, stays at the second, which
// cools down again and again); a gateway whose exit is itself.
const backAndForth = () => ({
  name: 'gateway/back',
  world: groundWorld(20, 4, (w) => { for (const x of [1, 6, 14]) w.set(x, 1, 1, 'minecraft:end_gateway'); }),
  args: gatewayRequest({
    probes: [[1, 1, 1], [6, 1, 1], [14, 1, 1]], samples: sequence(101),
    block_entities: [gateway([1, 1, 1], [6, 1, 1], EXACT), gateway([6, 1, 1], [1, 1, 1], EXACT), gateway([14, 1, 1], [14, 1, 1], EXACT)],
    entities: [item(1100, [1.5, 3.0, 1.5]), tag(1110, 'armor_stand', [14.5, 3.0, 1.5])],
  }),
});

// Carried in: an item a dropper drops into a gateway, sand falling into
// one (it lands at the exit), an armor stand a piston pushes into one, TNT a
// dispenser primes in one (having used a portal, its explosion at the exit
// leaves the nether portal beside it: PrimedTnt.USED_PORTAL_DAMAGE_CALCULATOR).
const carriedWorld = groundWorld(40, 6, (w) => {
  w.set(1, 1, 1, 'minecraft:end_gateway');
  w.set(1, 1, 2, 'minecraft:dropper', { facing: 'north', triggered: 'false' });
  w.set(1, 1, 3, 'minecraft:lever', LEVER);
  w.set(9, 1, 1, 'minecraft:end_gateway');
  w.set(9, 5, 1, 'minecraft:sand');
  w.set(17, 1, 1, 'minecraft:end_gateway');
  w.set(17, 1, 3, 'minecraft:piston', { facing: 'north', extended: 'false' });
  w.set(17, 1, 4, 'minecraft:lever', LEVER);
  w.set(25, 1, 1, 'minecraft:end_gateway');
  w.set(25, 1, 2, 'minecraft:dispenser', { facing: 'north', triggered: 'false' });
  w.set(25, 1, 3, 'minecraft:lever', LEVER);
  // A one-wide nether portal in its frame beside the TNT's exit.
  for (let y = 1; y <= 5; y++) { w.set(32, y, 4, 'minecraft:obsidian'); w.set(34, y, 4, 'minecraft:obsidian'); }
  w.set(33, 5, 4, 'minecraft:obsidian');
  for (const y of [1, 2, 3, 4]) w.set(33, y, 4, 'minecraft:nether_portal', { axis: 'x' });
});
const carriedGateway = () => ({
  name: 'gateway/carried', world: carriedWorld,
  args: gatewayRequest({
    probes: [[1, 1, 1], [9, 1, 1], [13, 1, 1], [17, 1, 3], [33, 1, 4], [33, 2, 4], [32, 1, 2], [1, 1, 2], [25, 1, 2]], inventory_probes: [[1, 1, 2], [25, 1, 2]], samples: sequence(121),
    block_entities: [
      gateway([1, 1, 1], [5, 1, 1], EXACT), gateway([9, 1, 1], [13, 1, 1], EXACT), gateway([17, 1, 1], [21, 1, 1], EXACT), gateway([25, 1, 1], [32, 1, 2], EXACT),
      blockEntity([1, 1, 2], 'dropper', 'Items:[{Slot:0b,id:"minecraft:diamond",count:1}]'),
      blockEntity([25, 1, 2], 'dispenser', 'Items:[{Slot:0b,id:"minecraft:tnt",count:1}]'),
    ],
    entities: [tag(1200, 'armor_stand', [17.5, 1.0, 2.5])],
    actions: [{ tick: 2, type: 'toggle', at: [1, 1, 3] }, { tick: 2, type: 'toggle', at: [17, 1, 4] }, { tick: 2, type: 'toggle', at: [25, 1, 3] }],
  }),
});

// A minecart with its dolphin rider going through a gateway: the dolphin
// goes with it, as a passenger (Entity.teleportSameDimension).
const gatewayRider = () => ({
  name: 'gateway/rider', world: groundWorld(8, 4, (w) => w.set(1, 1, 1, 'minecraft:end_gateway')),
  args: gatewayRequest({
    probes: [[1, 1, 1]], block_entities: [gateway([1, 1, 1], [5, 1, 1], EXACT)],
    entities: [{
      at: [1.5, 3.0, 1.5], data_version: NOW,
      snbt: `{id:"minecraft:minecart",UUID:${uuid(1300)},Pos:[1.5d,3.0d,1.5d],Motion:[0.0d,0.0d,0.0d],Rotation:[0.0f,0.0f],Passengers:[{id:"minecraft:dolphin",UUID:${uuid(1310)},Pos:[1.5d,3.0d,1.5d],NoAI:1b,Air:4800s,Moistness:2400}]}`,
    }],
  }),
});

// Refused: exits in chunks the build does not hold (exact, and not exact,
// whose search reads the void there), where the portal's chunk ticket would
// make chunks tick for 300 ticks that the verifier's tickets do not.
const gatewayFar = () => ({
  name: 'gateway/far', world: groundWorld(20, 4, (w) => { w.set(1, 1, 1, 'minecraft:end_gateway'); w.set(9, 1, 1, 'minecraft:end_gateway'); }),
  args: gatewayRequest({
    probes: [[1, 1, 1], [9, 1, 1]],
    block_entities: [gateway([1, 1, 1], [300, 1, 1], EXACT), gateway([9, 1, 1], [9, 1, 300])],
    entities: [item(1400, [1.5, 3.0, 1.5]), item(1410, [9.5, 3.0, 1.5])],
  }),
});

module.exports = {
  fixtures: () => [
    { name: 'portal/minimal', world: minimalWorld, args: minimalRequest, nether: true },
    ...kinds(), rider(), ...carried(), readers(), waiting(), end(), ...refused(),
    gatewayKinds(), nonExact(), coolingDown(), backAndForth(), carriedGateway(), gatewayRider(), gatewayFar(),
  ],
};
