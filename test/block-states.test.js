'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { World } = require('../server/world');
const { BlockStates } = require('../server/block-states');
const { testBlockStates } = require('./helpers/block-table');

// A table of states written for these tests: a cube with six full faces that
// blocks all light, one that also gives light, a clear block, a block that
// gives light and blocks none, and a slab whose half sides close a face only
// beside the other half (face shapes 2 and 3).
const CUBE = { full: 63, lightBlock: 15 };
const states = testBlockStates([
  { name: 'test:cube', ...CUBE },
  { name: 'test:lamp[lit=false]', ...CUBE },
  { name: 'test:lamp[lit=true]', ...CUBE, emission: 15 },
  { name: 'test:clear' },
  { name: 'test:glow[level=9,wet=false]', emission: 9 },
  { name: 'test:glow[level=9,wet=true]', emission: 9, lightBlock: 1 },
  { name: 'test:glow[level=3,wet=false]', emission: 3 },
  { name: 'test:glow[level=3,wet=true]', emission: 3, lightBlock: 1 },
  { name: 'test:slab[type=bottom]', full: 1, faces: [1, 0, 2, 2, 2, 2] },
  { name: 'test:slab[type=top]', full: 2, faces: [0, 1, 3, 3, 3, 3] },
], { shapes: 4, pairs: [[2, 3], [3, 2]] });
const fact = (id, properties = {}) => {
  const state = states.idOf(id, properties);
  return { emission: states.emission[state], lightBlock: states.lightBlock[state], full: states.fullFaces[state] };
};

test('a state is found by its block and properties, its block\'s default filling the properties it leaves out', () => {
  assert.equal(states.idOf('minecraft:air', {}), 0);
  assert.deepEqual(fact('test:glow'), { emission: 9, lightBlock: 0, full: 0 });
  assert.deepEqual(fact('test:glow', { wet: 'true' }), { emission: 9, lightBlock: 1, full: 0 });
  assert.deepEqual(fact('test:glow', { wet: 'true', level: '3' }), { emission: 3, lightBlock: 1, full: 0 });
  assert.deepEqual([fact('test:lamp'), fact('test:lamp', { lit: 'true' })], [{ emission: 0, lightBlock: 15, full: 63 }, { emission: 15, lightBlock: 15, full: 63 }]);
  assert.throws(() => states.idOf('test:glow', { level: '16' }), /no block state test:glow\[level=16,wet=false\]/);
  assert.throws(() => states.idOf('test:nothing', {}), /no block test:nothing/);
});

// The page's reading of the bytes (web/viewer.js loadResources): each part
// on four bytes.
function read(bytes) {
  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length);
  const [count, shapes, rows, maskBytes, nameBytes] = new Uint32Array(buffer, 0, 5);
  let at = 20;
  const take = (Type, length) => {
    const part = new Type(buffer, at, length);
    at += Math.ceil(part.byteLength / 4) * 4;
    return part;
  };
  const parts = {
    count, shapes,
    fluid: take(Uint16Array, count), skipRow: take(Uint16Array, count), skipColumn: take(Uint16Array, count),
    skipStart: take(Uint32Array, rows), skipFirst: take(Uint16Array, rows), skipLength: take(Uint16Array, rows),
    emission: take(Uint8Array, count), lightBlock: take(Uint8Array, count), fullFaces: take(Uint8Array, count), draw: take(Uint8Array, count),
    faces: take(Uint8Array, count * 6), occludes: take(Uint8Array, shapes * shapes), skipMasks: take(Uint8Array, maskBytes),
    fluidNames: JSON.parse(Buffer.from(take(Uint8Array, nameBytes)).toString()),
  };
  assert.equal(at, buffer.byteLength, 'the page reads every byte');
  return parts;
}

test('the bytes the page reads hold every state\'s facts and the face shape pairs', () => {
  const page = read(states.bytes());
  assert.deepEqual([page.count, page.shapes], [states.count, 4]);
  const bottom = states.idOf('test:slab', { type: 'bottom' });
  const top = states.idOf('test:slab', { type: 'top' });
  const glow = states.idOf('test:glow', {});
  assert.deepEqual([page.emission[glow], page.lightBlock[glow], page.fullFaces[bottom]], [9, 0, 1]);
  const { faces, occludes, shapes } = page;
  // A bottom half beside a top half closes the face between them; two bottom halves do not.
  const east = (state) => faces[state * 6 + 5];
  const west = (state) => faces[state * 6 + 4];
  assert.equal(occludes[east(bottom) * shapes + west(top)], 1);
  assert.equal(occludes[east(bottom) * shapes + west(bottom)], 0);
  assert.equal(occludes[faces[bottom * 6] * shapes], 1, 'its full bottom closes the face below it alone');
});

// A table of what a block is drawn as: a pane that skips rendering beside
// a pane it joins to the east and west (bits 16 and 32) and beside panes
// above and below (1, 2), panes that join nothing, water that skips beside
// itself and a waterlogged stair in every direction, a waterlogged stair,
// and glass drawn blended.
const ALL = 63;
const drawn = testBlockStates([
  { name: 'test:pane[joined=false]' },
  { name: 'test:pane[joined=true]' },
  { name: 'test:water[level=0]', model: false, fluid: 'test:water[level=0]', fluidBlended: true },
  { name: 'test:water[level=1]', model: false, fluid: 'test:water[level=1]', fluidBlended: true },
  { name: 'test:stair[waterlogged=false]' },
  { name: 'test:stair[waterlogged=true]', fluid: 'test:water[level=0]', fluidBlended: true },
  { name: 'test:glass', blended: true },
], {
  skips: [
    ['test:pane[joined=true]', 'test:pane[joined=true]', 1 | 2 | 16 | 32],
    ['test:pane[joined=false]', 'test:pane[joined=false]', 1 | 2],
    ['test:pane[joined=false]', 'test:pane[joined=true]', 1 | 2],
    ['test:water[level=0]', 'test:water[level=0]', ALL], ['test:water[level=0]', 'test:water[level=1]', ALL], ['test:water[level=0]', 'test:stair[waterlogged=true]', ALL],
    ['test:water[level=1]', 'test:water[level=0]', ALL], ['test:water[level=1]', 'test:water[level=1]', ALL], ['test:water[level=1]', 'test:stair[waterlogged=true]', ALL],
  ],
});

test('skipRendering is answered for each ordered pair of states and direction as the table holds it, and the page reads the same', () => {
  const id = (name) => drawn.ids.get(name);
  const joined = id('test:pane[joined=true]');
  const alone = id('test:pane[joined=false]');
  const water = id('test:water[level=0]');
  const stair = id('test:stair[waterlogged=true]');
  const dry = id('test:stair[waterlogged=false]');
  const expected = (a, b, direction) => {
    if (a === joined && b === joined) return [0, 1, 4, 5].includes(direction);
    if (a === alone && (b === alone || b === joined)) return direction <= 1;
    if ([water, id('test:water[level=1]')].includes(a)) return [water, id('test:water[level=1]'), stair].includes(b);
    return false;
  };
  const page = read(drawn.bytes());
  const pageSkips = (a, b, direction) => {
    const row = page.skipRow[a];
    const at = page.skipColumn[b] - page.skipFirst[row];
    return at >= 0 && at < page.skipLength[row] && ((page.skipMasks[page.skipStart[row] + at] >> direction) & 1) === 1;
  };
  for (let a = 0; a < drawn.count; a++) {
    for (let b = 0; b < drawn.count; b++) {
      for (let direction = 0; direction < 6; direction++) {
        assert.equal(drawn.skips(a, b, direction), expected(a, b, direction), `${a} beside ${b} to ${direction}`);
        assert.equal(pageSkips(a, b, direction), expected(a, b, direction), `the page: ${a} beside ${b} to ${direction}`);
      }
    }
  }
  assert.ok(!drawn.skips(joined, alone, 4), 'a joined pane does not skip beside one that is not');
  assert.ok(!drawn.skips(stair, water, 1) && !drawn.skips(dry, water, 1), 'a stair does not skip its faces beside water');
});

test('a state\'s fluid, whether its model is drawn, and the layers they are drawn in reach the page', () => {
  const id = (name) => drawn.ids.get(name);
  const page = read(drawn.bytes());
  const water = id('test:water[level=0]');
  assert.deepEqual([page.fluid[id('test:stair[waterlogged=true]')], page.fluid[water], page.fluid[id('test:water[level=1]')], page.fluid[id('test:stair[waterlogged=false]')], page.fluid[0]],
    [water, water, id('test:water[level=1]'), 0, 0]);
  assert.deepEqual(page.fluidNames, { [water]: 'test:water[level=0]', [id('test:water[level=1]')]: 'test:water[level=1]' });
  // 1 drawn as a model, 2 the model blended, 4 the fluid blended.
  assert.deepEqual([page.draw[0], page.draw[water], page.draw[id('test:stair[waterlogged=true]')], page.draw[id('test:stair[waterlogged=false]')], page.draw[id('test:glass')]],
    [0, 4, 1 | 4, 1, 1 | 2]);
});

test('render layers that name another state than the block table are refused', () => {
  const { blockTable, renderLayers } = require('./helpers/block-table');
  const table = [{ name: 'minecraft:air', model: false }, { name: 'test:a' }, { name: 'test:b' }];
  assert.throws(() => new BlockStates(blockTable(table), renderLayers([table[0], table[2], table[1]])), /render layers name state 1 test:b, the block table test:a/);
  assert.throws(() => new BlockStates(blockTable(table), renderLayers(table.slice(0, 2))), /render layers have 2 states, the block table 3/);
});

// The blocks of a world's chunk as see_build's page is sent them.
const shown = (world, key) => {
  const chunk = world.chunkSnapshot(key, states);
  return chunk.blocks.map(([x, y, z, index]) => `${x},${y},${z} ${chunk.palette[index].id}`).sort();
};

test('a block is left out only where it and the six blocks around it have six full faces', () => {
  const world = new World();
  world.fill([0, 0, 0], [2, 2, 2], 'test:cube');
  const key = World.keyOf(0, 0, 0);
  assert.equal(shown(world, key).length, 26);
  assert.ok(!shown(world, key).includes('1,1,1 test:cube'));
  // A block that gives light, a clear one or a slab beside it shows it.
  for (const [id, properties] of [['test:glow', {}], ['test:clear', {}], ['test:slab', { type: 'bottom' }]]) {
    world.set(1, 2, 1, id, properties);
    assert.ok(shown(world, key).includes('1,1,1 test:cube'), id);
  }
  // A lit cube hidden on every side gives light no one sees: left out.
  world.set(1, 2, 1, 'test:cube');
  world.set(1, 1, 1, 'test:lamp', { lit: 'true' });
  assert.ok(!shown(world, key).some((block) => block.startsWith('1,1,1 ')));
  // Each palette entry carries its state id.
  const chunk = world.chunkSnapshot(key, states);
  assert.deepEqual(chunk.palette.map(({ id, state }) => [id, state]), [['test:cube', states.idOf('test:cube', {})]]);
});

test('a chunk\'s version changes where a block across its faces hides or shows its edge, and only there', () => {
  // A wall of cubes across the border between two chunks at x = 15 | 16.
  const world = new World();
  world.fill([14, 0, 0], [17, 2, 2], 'test:cube');
  const left = World.keyOf(0, 0, 0);
  const right = World.keyOf(1, 0, 0);
  const before = world.shownVersion(left, states);
  assert.ok(!shown(world, left).includes('15,1,1 test:cube'));
  // Inside the other chunk, past its edge layer: the same version, the same blocks.
  world.set(17, 1, 1, 'test:clear');
  assert.equal(world.shownVersion(left, states), before);
  // On its edge layer: the block across shows.
  world.set(16, 1, 1, 'test:clear');
  const after = world.shownVersion(left, states);
  assert.notEqual(after, before);
  assert.ok(shown(world, left).includes('15,1,1 test:cube'));
  assert.equal(world.chunkSnapshot(left, states).version, after);
  // Put back: the version it had.
  world.set(16, 1, 1, 'test:cube');
  assert.equal(world.shownVersion(left, states), before);
  // The other chunk gone: nothing hides the edge.
  for (let x = 16; x <= 17; x++) for (let y = 0; y <= 2; y++) for (let z = 0; z <= 2; z++) world.set(x, y, z, 'minecraft:air');
  assert.ok(!world.chunks.has(right) || world.chunks.get(right).count === 0);
  assert.notEqual(world.shownVersion(left, states), before);
  assert.ok(shown(world, left).includes('15,1,1 test:cube'));
});
