'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const { BlockStates } = require('../server/block-states');
const { testBlockStates } = require('./helpers/block-table');

// A table of states written for these tests: a cube with six full faces that
// blocks all light, one that also gives light, a clear block, a block that
// gives light and blocks none, and a slab whose half sides close a face only
// beside the other half (face shapes 2 and 3), and hide each other only
// beside the same half (cull shapes 2, the bottom half's sides, and 3, the
// top half's).
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
  { name: 'test:slab[type=bottom]', full: 1, faces: [1, 0, 2, 2, 2, 2], cull: [1, 0, 2, 2, 2, 2] },
  { name: 'test:slab[type=top]', full: 2, faces: [0, 1, 3, 3, 3, 3], cull: [0, 1, 3, 3, 3, 3] },
], { shapes: 4, pairs: [[2, 3], [3, 2]], cullShapes: 4, hidePairs: [[2, 2], [3, 3]] });
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

test('every block is named by the name the table gives it', () => {
  assert.equal(states.names.get('test:glow'), 'Test test:glow');
  assert.equal(states.names.get('minecraft:air'), 'Test minecraft:air');
});

test('every state\'s facts and the face shape pairs are read from the table', () => {
  const page = states;
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
  // A bottom half's side is hidden beside a bottom half, not beside a top
  // half; any face is hidden beside a full one, and none beside air.
  const { cull, hides, cullShapes } = page;
  const tableHides = (a, b, direction) => hides[cull[a * 6 + direction] * cullShapes + cull[b * 6 + (direction ^ 1)]] === 1;
  const cube = states.idOf('test:cube', {});
  for (const [a, b, direction, hidden] of [[bottom, bottom, 5, true], [bottom, top, 5, false], [top, top, 4, true], [bottom, cube, 2, true], [bottom, 0, 2, false], [cube, bottom, 5, false], [cube, bottom, 1, true]]) {
    assert.equal(states.hidesFace(a, b, direction), hidden, `${a} beside ${b} to ${direction}`);
    assert.equal(tableHides(a, b, direction), hidden, `the cull shapes: ${a} beside ${b} to ${direction}`);
  }
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
  { name: 'test:banner', renderer: 'test:banner' },
], {
  skips: [
    ['test:pane[joined=true]', 'test:pane[joined=true]', 1 | 2 | 16 | 32],
    ['test:pane[joined=false]', 'test:pane[joined=false]', 1 | 2],
    ['test:pane[joined=false]', 'test:pane[joined=true]', 1 | 2],
    ['test:water[level=0]', 'test:water[level=0]', ALL], ['test:water[level=0]', 'test:water[level=1]', ALL], ['test:water[level=0]', 'test:stair[waterlogged=true]', ALL],
    ['test:water[level=1]', 'test:water[level=0]', ALL], ['test:water[level=1]', 'test:water[level=1]', ALL], ['test:water[level=1]', 'test:stair[waterlogged=true]', ALL],
  ],
});

test('skipRendering is answered for each ordered pair of states and direction as the table holds it', () => {
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
  for (let a = 0; a < drawn.count; a++) {
    for (let b = 0; b < drawn.count; b++) {
      for (let direction = 0; direction < 6; direction++) {
        assert.equal(drawn.skips(a, b, direction), expected(a, b, direction), `${a} beside ${b} to ${direction}`);
      }
    }
  }
  assert.ok(!drawn.skips(joined, alone, 4), 'a joined pane does not skip beside one that is not');
  assert.ok(!drawn.skips(stair, water, 1) && !drawn.skips(dry, water, 1), 'a stair does not skip its faces beside water');
});

test('a state\'s fluid, whether its model is drawn, the layers they are drawn in and whether a block entity renderer draws it are read', () => {
  const id = (name) => drawn.ids.get(name);
  const page = drawn;
  const water = id('test:water[level=0]');
  assert.deepEqual([page.fluid[id('test:stair[waterlogged=true]')], page.fluid[water], page.fluid[id('test:water[level=1]')], page.fluid[id('test:stair[waterlogged=false]')], page.fluid[0]],
    [water, water, id('test:water[level=1]'), 0, 0]);
  // 1 drawn as a model, 2 the model blended, 4 the fluid blended, 16 drawn
  // through a block entity renderer.
  assert.deepEqual([page.draw[0], page.draw[water], page.draw[id('test:stair[waterlogged=true]')], page.draw[id('test:stair[waterlogged=false]')], page.draw[id('test:glass')], page.draw[id('test:banner')]],
    [0, 4, 1 | 4, 1, 1 | 2, 1 | 16]);
});

test('render layers that name another state than the block table are refused', () => {
  const { blockTable, renderLayers } = require('./helpers/block-table');
  const table = [{ name: 'minecraft:air', model: false }, { name: 'test:a' }, { name: 'test:b' }];
  assert.throws(() => new BlockStates(blockTable(table), renderLayers([table[0], table[2], table[1]])), /render layers name state 1 test:b, the block table test:a/);
  assert.throws(() => new BlockStates(blockTable(table), renderLayers(table.slice(0, 2))), /render layers have 2 states, the block table 3/);
});
