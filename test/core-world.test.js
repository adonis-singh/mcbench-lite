'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const { build } = require('../scripts/build-wasm');

// The Rust engine as WebAssembly (core/, web/core-world.mjs). Its bytes are
// checked against the native program by scripts/wasm-parity.js; these are
// the API's own rules. Builds the wasm (cached by the sources' hash).
const COMMANDS = [
  'fill 0 64 0 5 64 5 minecraft:stone',
  'setblock 1 65 1 minecraft:lever[face=floor,facing=north,powered=false]',
  'setblock 2 65 1 minecraft:redstone_wire',
  'setblock 3 65 1 minecraft:redstone_lamp',
].map((line) => `${line}\n`).join('');

let wasm;
let CoreWorld;
test.before(async () => {
  ({ CoreWorld } = await import('../web/core-world.mjs'));
  wasm = await WebAssembly.compile(fs.readFileSync(await build()));
});

test('a placement returns the blueprint, a tick the patch and the entities', async () => {
  const { readPatch } = await import('../web/live-patch.mjs');
  const world = new CoreWorld(wasm);
  const blueprint = world.place(COMMANDS);
  // The palette's size comes first: air, stone, lever, wire, lamp.
  assert.strictEqual(new DataView(blueprint.buffer, blueprint.byteOffset).getInt32(0), 5);
  const idle = world.tick(1);
  assert.deepStrictEqual(readPatch(idle.bytes), { entries: [], bounds: [0, 64, 0, 5, 65, 5], columns: 1, changes: [] });
  assert.strictEqual(idle.entities, '[]');
  world.input('toggle', 1, 65, 1, 0);
  assert.match(world.state(), /^time 1$/m);
  const lit = world.tick(2);
  assert.ok(readPatch(lit.bytes).changes.length > 0);
});

test('a failure Minecraft reported rejects and the world goes on', () => {
  const world = new CoreWorld(wasm);
  world.place(COMMANDS);
  assert.throws(() => world.input('toggle', 0, 64, 0, 0), /^Error: The engine failed: java\.lang\.IllegalArgumentException: No toggle input at 0, 64, 0/);
  assert.ok(world.tick(1).bytes.length > 0);
});

test('a world with no blocks is refused as the Java engine refuses it', () => {
  const world = new CoreWorld(wasm);
  assert.throws(() => world.place('fill 0 64 0 1 64 1 minecraft:air\n'), /The import places no blocks/);
});

test('a use input opens a door and a gate the way the player looks, and iron takes none', async () => {
  const { readPatch, readBlueprint } = await import('../web/live-patch.mjs');
  const world = new CoreWorld(wasm);
  const blueprint = world.place([
    'fill 0 64 0 5 64 5 minecraft:stone\n',
    'setblock 1 65 1 minecraft:oak_door[facing=south,half=lower,hinge=left,open=false,powered=false]\n',
    'setblock 1 66 1 minecraft:oak_door[facing=south,half=upper,hinge=left,open=false,powered=false]\n',
    'setblock 3 65 1 minecraft:oak_fence_gate[facing=north,in_wall=false,open=false,powered=false]\n',
    'setblock 5 65 1 minecraft:iron_door[facing=south,half=lower,hinge=left,open=false,powered=false]\n',
    'setblock 5 66 1 minecraft:iron_door[facing=south,half=upper,hinge=left,open=false,powered=false]\n',
  ].join(''));
  const text = ({ id, properties }) => `${id}[${properties.map(([k, v]) => `${k}=${v}`).join(',')}]`;
  const palette = readBlueprint(blueprint).palette.map(text);
  palette.push(...readPatch(world.tick(1).bytes).entries.map(text));
  // A player looking south (yaw 0) opens a gate that faces north toward them: it turns to face south.
  world.input('use', 3, 65, 1, 0);
  world.input('use', 1, 66, 1, 90);
  const patch = readPatch(world.tick(1).bytes);
  palette.push(...patch.entries.map(text));
  const at = (x, y, z) => palette[patch.changes.find((change) => change[0] === x && change[1] === y && change[2] === z)?.[3]];
  assert.match(at(3, 65, 1), /oak_fence_gate\[facing=south,in_wall=false,open=true/);
  assert.match(at(1, 65, 1), /oak_door\[.*open=true/);
  assert.match(at(1, 66, 1), /oak_door\[.*half=upper.*open=true/);
  // Iron cannot be opened by hand: an error Minecraft reports, and the world goes on.
  assert.throws(() => world.input('use', 5, 65, 1, 0), /IllegalArgumentException: No use input at 5, 65, 1/);
  assert.ok(readPatch(world.tick(1).bytes));
});

test('a request the engine does not model spends the world until a fresh placement', () => {
  const world = new CoreWorld(wasm);
  world.place(COMMANDS);
  world.place('setblock 4 65 4 minecraft:infested_stone\nsetblock 5 65 4 minecraft:tnt[unstable=false]\nsetblock 5 65 3 minecraft:redstone_block\n', { fresh: false });
  assert.throws(() => world.tick(80), /^Error: The engine failed: unported: InfestedBlock.spawnAfterBreak/);
  assert.ok(world.refused.startsWith('unported: '));
  assert.throws(() => world.tick(1), /^Error: The engine failed: unported: an earlier request/);
  assert.throws(() => world.state(), /unported: an earlier request/);
  world.place(COMMANDS);
  assert.strictEqual(world.refused, null);
  assert.ok(world.tick(1).bytes.length > 0);
});

// Water falling off a ledge and a piston pushing a block into the next chunk
// column: the patches grow the bounds and the built columns, and once the
// water's source is taken away the fall dries up and they shrink back. Each
// patch carries the bounds and the count of built columns of the blocks
// after it, and applied to the mesher's world it draws every tile as a fresh
// load of those blocks does.
test('ticks that move blocks past the bounds and into a new chunk column patch the world to grow and shrink it', async (t) => {
  const { bakedPack } = require('./helpers/community');
  const pack = bakedPack();
  if (!pack) return t.skip('no baked pack in the data directory');
  const { readPatch, readBlueprint } = await import('../web/live-patch.mjs');
  const { Mesher } = await import('../web/mesher.mjs');
  const { MODULE } = require('../scripts/build-renderer');
  const { blocksBlueprint } = require('../server/blueprint');
  const world = new CoreWorld(wasm);
  const mask = { minX: -100, maxX: 100, minZ: -100, maxZ: 100 };
  const [patched, fresh] = await Promise.all([Mesher.create(fs.readFileSync(MODULE), pack), Mesher.create(fs.readFileSync(MODULE), pack)]);
  // The blocks the stream says the world holds: "x,y,z" -> state name.
  const name = ({ id, properties }) => (properties.length ? `${id}[${properties.map(([k, v]) => `${k}=${v}`).join(',')}]` : id);
  let palette = [];
  const blocks = new Map();
  const rebase = (blueprint) => {
    const { palette: entries, sections } = readBlueprint(blueprint);
    palette = entries.map(name);
    blocks.clear();
    for (const { x, y, z, indices } of sections) indices.forEach((index, i) => { if (index) blocks.set(`${x * 16 + (i & 15)},${y * 16 + (i >> 8)},${z * 16 + ((i >> 4) & 15)}`, palette[index]); });
    patched.load(blueprint, mask, false);
  };
  const extent = () => {
    const points = [...blocks.keys()].map((key) => key.split(',').map(Number));
    const bounds = [0, 1, 2].map((axis) => Math.min(...points.map((p) => p[axis]))).concat([0, 1, 2].map((axis) => Math.max(...points.map((p) => p[axis]))));
    return { bounds, columns: new Set(points.map(([x, , z]) => `${x >> 4},${z >> 4}`)).size };
  };
  const seen = [];
  const tick = (label) => {
    const { bytes } = world.tick(1);
    const patch = readPatch(bytes);
    palette.push(...patch.entries.map(name));
    for (const [x, y, z, index] of patch.changes) {
      if (index) blocks.set(`${x},${y},${z}`, palette[index]);
      else blocks.delete(`${x},${y},${z}`);
    }
    assert.deepStrictEqual({ bounds: patch.bounds, columns: patch.columns }, extent(), `${label}: the bounds and built columns of the blocks`);
    seen.push(patch);
    const { tiles } = patched.patch(bytes);
    if (seen.length % 25 === 0) {
      const loaded = fresh.load(blocksBlueprint([...blocks].map(([key, state]) => [...key.split(',').map(Number), state])), mask, false);
      assert.deepStrictEqual(tiles, loaded.tiles, `${label}: the tiles`);
      for (const [x, z] of tiles) assert.ok(Buffer.from(patched.draw(x, z)).equals(Buffer.from(fresh.draw(x, z))), `${label}: tile ${x},${z}`);
    }
  };
  rebase(world.place([
    'fill 0 -40 0 15 -40 3 minecraft:stone',
    'setblock 13 -39 0 minecraft:redstone_block',
    'setblock 14 -39 0 minecraft:piston[facing=east,extended=false]',
    'setblock 15 -39 0 minecraft:stone',
    'setblock 4 -34 6 minecraft:stone',
    'setblock 4 -33 6 minecraft:water',
  ].map((line) => `${line}\n`).join('')));
  for (let n = 1; n <= 75; n += 1) tick(`tick ${n}`);
  assert.ok(seen.some((patch) => patch.bounds[3] === 16 && patch.columns === 2), 'the pushed block builds the next chunk column (the base reaches x 15, in one column)');
  const fallen = seen.at(-1).bounds[1];
  assert.ok(fallen < -44, `the water falls past the bottom of the bounds (${fallen})`);
  rebase(world.place('setblock 4 -33 6 minecraft:air\n', { fresh: false }));
  for (let n = 1; n <= 400 && seen.at(-1).bounds[1] !== -40; n += 1) tick(`drying ${n}`);
  assert.strictEqual(seen.at(-1).bounds[1], -40, 'the fall dried up: the bounds are the stone\'s again');
  assert.ok(seen.some((patch) => patch.bounds[1] === -64), 'the water reached the floor of the world, and spread there');
});

test('two worlds of one wasm are independent', () => {
  const [a, b] = [new CoreWorld(wasm), new CoreWorld(wasm)];
  a.place(COMMANDS);
  b.place(COMMANDS);
  a.input('toggle', 1, 65, 1, 0);
  a.tick(3);
  b.tick(3);
  assert.notStrictEqual(a.state(), b.state());
});

test('an exported world reads back, with the existing region reader, as the blueprint the engine wrote', async () => {
  const { worldZip } = await import('../web/world-export.mjs');
  const fs = require('fs');
  const os = require('os');
  const path = require('path');
  const zlib = require('zlib');
  const { readZipEntry } = require('../server/zip-entry');
  const { readBlockStates, readNbt } = require('../server/region');
  const { forEachBlueprintBlock } = require('../server/blueprint');
  const world = new CoreWorld(wasm);
  // Blocks over a chunk border and below zero, a chest (a block entity), water (a fluid tick).
  const blueprint = world.place([COMMANDS, 'fill -20 70 -20 20 70 20 minecraft:stone\n', 'setblock 3 71 3 minecraft:chest[facing=north]\n', 'setblock 5 72 5 minecraft:water\n'].join(''));
  world.input('toggle', 1, 65, 1, 0);
  world.tick(7);
  const save = world.export();
  assert.strictEqual(save.gameTime, 7n);
  const { bytes } = await worldZip({ name: 'Test build', gameTime: save.gameTime, chunks: save.chunks }, [0, 80, 0]);
  const zipFile = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-export-')), 'world.zip');
  fs.writeFileSync(zipFile, bytes);
  const level = readNbt(zlib.gunzipSync(readZipEntry(zipFile, 'Test build/level.dat'))).Data;
  assert.strictEqual(level.DataVersion, 4556);
  assert.strictEqual(level.Time, 7n);
  assert.deepStrictEqual([...level.spawn.pos], [0, 80, 0]);
  // The region folder, unpacked and read as the viewer's reader reads a Minecraft save.
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-export-region-'));
  const names = new Set(save.chunks.map((chunk) => `r.${chunk.x >> 5}.${chunk.z >> 5}.mca`));
  assert.ok(names.size > 1, 'the build spans regions');
  for (const name of names) fs.writeFileSync(path.join(folder, name), readZipEntry(zipFile, `Test build/region/${name}`));
  const saved = readBlockStates(folder);
  const expected = new Map();
  forEachBlueprintBlock(Buffer.from(blueprint), (x, y, z, id, properties, state) => expected.set(`${x},${y},${z}`, state));
  // The blueprint is the placement; the toggled lever and the tick since changed some blocks.
  assert.ok(expected.size > 100);
  assert.strictEqual(saved.get('1,65,1').startsWith('minecraft:lever[') && saved.get('1,65,1').includes('powered=true'), true);
  assert.strictEqual(saved.get('3,71,3').startsWith('minecraft:chest['), true);
  assert.strictEqual(saved.get('-20,70,-20'), 'minecraft:stone');
  let same = 0;
  for (const [position, state] of expected) {
    if (saved.get(position) === state) same += 1;
  }
  assert.ok(same >= expected.size - 4, `${same} of ${expected.size} blocks are as placed`);
  assert.ok(saved.size >= expected.size - 4);
});

test('the exported world carries the data pack the engine\'s own dimension needs', async () => {
  const { dimensionPack } = await import('../web/world-export.mjs');
  const fs = require('fs');
  const os = require('os');
  const path = require('path');
  const { writeDimensionPack } = require('../server/minecraft-dimension');
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-pack-'));
  writeDimensionPack(folder);
  for (const [file, text] of dimensionPack()) assert.strictEqual(fs.readFileSync(path.join(folder, 'world', file), 'utf8'), text, file);
});

test('a creative player walks in the world, and its view comes with each tick', () => {
  const world = new CoreWorld(wasm);
  world.place('fill 0 63 0 10 63 10 minecraft:stone\n');
  world.tick(2);
  const spawned = world.spawnPlayer(2.5, 64, 2.5, 0, 0, false);
  assert.strictEqual(spawned.length, 28);
  assert.deepStrictEqual([...spawned.slice(0, 3)], [2.5, 64, 2.5]);
  // Forward (key 1), facing south (yaw 0): z grows, x stays.
  let view;
  for (let i = 0; i < 10; i += 1) view = world.playerTick(1, 0, 0).view;
  assert.ok(view[2] > 3.5, `z is ${view[2]}`);
  assert.strictEqual(view[0], 2.5);
  assert.strictEqual(view[21], 1, 'on the ground');
  // The player is a client: the world's own state is what it was without it.
  assert.match(world.state(), /^time 12$/m);
  world.removePlayer();
  assert.throws(() => world.playerTick(1, 0, 0), /PTICK before PSPAWN/);
  // A failure, not a refusal: the world goes on.
  assert.match(world.state(), /^time 12$/m);
  world.tick(1);
});
