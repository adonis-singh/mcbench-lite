'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');

const { resolveShot } = require('../server/shot');
const { World } = require('../server/world');

// Eight framing points per chunk: 126 x 126 chunks is about 127,000 points,
// more than a call can take as spread arguments.
test('a world of about 16,000 chunks is framed whole', () => {
  const world = new World();
  for (let i = 0; i < 126; i += 1) for (let j = 0; j < 126; j += 1) world.set(16 * i, 0, 16 * j, 'minecraft:stone', {});
  assert.equal(world.chunks.size, 15876);
  const top = resolveShot({ view: 'top', projection: 'orthographic' }, world, []);
  assert.deepEqual(top.described.center.map(Math.round), [1001, 1, 1001]);
  assert.ok(top.described.widthBlocks >= 2001);
  assert.ok(resolveShot({}, world, []).camera.focusDist > 0);
});

// The entities of the tick shot are framed with the blocks: a minecart past
// the build's edge is in the picture, its box cut only to a clip or region
// the shot was given.
test('an entity past the blocks is framed with them, and a clip cuts it as it cuts the blocks', () => {
  const world = new World();
  for (let x = 0; x < 8; x += 1) world.set(x, 0, 0, 'minecraft:stone', {});
  const cart = { id: 1, type: 'minecraft:minecart', pos: [12.5, 1, 0.5], yaw: 0, pitch: 0, width: 0.98, height: 0.7 };
  const ortho = (entities, extra = {}) => resolveShot({ view: 'front', projection: 'orthographic', ...extra }, world, entities).described;
  const without = ortho([]);
  const framed = ortho([cart]);
  assert.ok(framed.center[0] - framed.widthBlocks / 2 <= 0 && framed.center[0] + framed.widthBlocks / 2 >= 12.99, `the cart's box (to x 12.99) is in the picture: ${JSON.stringify(framed)}`);
  assert.ok(without.center[0] + without.widthBlocks / 2 < 12, 'without it the blocks alone are framed');
  const clipped = ortho([cart], { clip: { from: [0, 0, 0], to: [7, 2, 0] } });
  assert.deepEqual(clipped.center, ortho([], { clip: { from: [0, 0, 0], to: [7, 2, 0] } }).center, 'a clip that leaves the cart out frames as without it');
});

// A shared build's preview is square (share.js); see_build's sizes stay its own.
test('a caller\'s own sizes frame a square shot, and see_build takes only its sizes', () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone', {});
  const square = resolveShot({ view: 'iso_se', size: 'preview' }, world, [], { preview: [1024, 1024] });
  assert.deepEqual([square.width, square.height], [1024, 1024]);
  assert.throws(() => resolveShot({ size: 'preview' }, world, []), /size must be large or small/);
  assert.deepEqual([resolveShot({}, world, []).width, resolveShot({}, world, []).height], [2184, 1640]);
});

// A bench's card (share.js): every shot gets a cell, rows split the width
// evenly, and the cells tile the square exactly.
test('a card of 1 to 8 shots is one row for 1 or 2 and two rows otherwise, the top one holding the extra shot', () => {
  const { montageCells } = require('../server/shot');
  const cell = (x, y, width, height) => ({ x, y, width, height });
  const expected = {
    1: [cell(0, 0, 1024, 1024)],
    2: [cell(0, 0, 512, 1024), cell(512, 0, 512, 1024)],
    3: [cell(0, 0, 512, 512), cell(512, 0, 512, 512), cell(0, 512, 1024, 512)],
    4: [cell(0, 0, 512, 512), cell(512, 0, 512, 512), cell(0, 512, 512, 512), cell(512, 512, 512, 512)],
    5: [cell(0, 0, 341, 512), cell(341, 0, 342, 512), cell(683, 0, 341, 512), cell(0, 512, 512, 512), cell(512, 512, 512, 512)],
    6: [cell(0, 0, 341, 512), cell(341, 0, 342, 512), cell(683, 0, 341, 512), cell(0, 512, 341, 512), cell(341, 512, 342, 512), cell(683, 512, 341, 512)],
    7: [cell(0, 0, 256, 512), cell(256, 0, 256, 512), cell(512, 0, 256, 512), cell(768, 0, 256, 512), cell(0, 512, 341, 512), cell(341, 512, 342, 512), cell(683, 512, 341, 512)],
    8: [0, 256, 512, 768].map((x) => cell(x, 0, 256, 512)).concat([0, 256, 512, 768].map((x) => cell(x, 512, 256, 512))),
  };
  for (let count = 1; count <= 8; count += 1) {
    const cells = montageCells(count, 1024);
    assert.deepEqual(cells, expected[count], `${count} shots`);
    assert.equal(cells.reduce((area, { width, height }) => area + width * height, 0), 1024 * 1024, `${count} shots cover the card`);
  }
  assert.throws(() => montageCells(0, 1024), /at least one shot/);
});
