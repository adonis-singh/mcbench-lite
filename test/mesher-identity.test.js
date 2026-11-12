'use strict';

// The mesher module must draw exactly the bytes it drew before it was made
// faster: test/fixtures/mesher-reference.wasm is the module built from
// renderer/ at 5c9c031, and every tile of every world below is drawn by
// both and compared byte for byte, and so is the indexed tile the module
// makes of each against the reference's tile indexed by
// test/fixtures/prbm-reference.mjs (shareQuadCorners, the viewer's indexing
// before the module did it).
//
// Real worlds are the community builds of the corpus MCBENCH_CORPUS names,
// drawn with the baked pack of the app's data directory
// (test/helpers/community.js): skipped where there are none.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

const { MODULE } = require('../scripts/build-renderer');
const { blocksBlueprint } = require('../server/blueprint');
const { renderMask } = require('../server/bluemap');
const { communityBuilds, communityBlocks, bakedPack } = require('./helpers/community');
const { mesherPack } = require('./helpers/mesher-pack');

const REFERENCE = fs.readFileSync(path.join(__dirname, 'fixtures', 'mesher-reference.wasm'));
const CURRENT = fs.readFileSync(MODULE);

// The blueprint and render mask the app gives blocks ("x,y,z" -> state name).
function viewOf(blocks) {
  const points = [...blocks.keys()].map((key) => key.split(',').map(Number));
  const bounds = { min: [0, 1, 2].map((i) => Math.min(...points.map((p) => p[i]))), max: [0, 1, 2].map((i) => Math.max(...points.map((p) => p[i]))) };
  return { blueprint: blocksBlueprint([...blocks].map(([key, name]) => [...key.split(',').map(Number), name])), mask: renderMask(bounds) };
}

// The reference module takes a blueprint whole (its `load`, from before the
// module read blueprints in parts).
function loadWhole(mesher, blueprint, mask) {
  mesher.check(mesher.module.load(mesher.give(blueprint), blueprint.length, mask.minX, mask.maxX, mask.minZ, mask.maxZ, 0));
  const [tiles, changed] = mesher.lists(2);
  return { tiles, changed };
}

// Draws every tile of a view with both modules and compares them; returns
// how many tiles and bytes were compared.
async function sameBytes(label, pack, { blueprint, mask }) {
  const { Mesher } = await import('../web/mesher.mjs');
  const { shareQuadCorners: shareReference } = await import('./fixtures/prbm-reference.mjs');
  const [reference, current] = await Promise.all([Mesher.create(REFERENCE, pack), Mesher.create(CURRENT, pack)]);
  const expected = loadWhole(reference, blueprint, mask);
  const actual = current.load(blueprint, mask, false);
  assert.deepEqual(actual.tiles, expected.tiles, `${label}: the same tiles`);
  assert.deepEqual(actual.changed, expected.changed, `${label}: the same changed tiles`);
  let bytes = 0;
  for (const [x, z] of expected.tiles) {
    const was = Buffer.from(reference.draw(x, z));
    const now = Buffer.from(current.draw(x, z));
    assert.ok(was.equals(now), `${label}: tile ${x},${z} differs (${was.length} bytes before, ${now.length} now)`);
    assert.ok(Buffer.from(shareReference(was)).equals(Buffer.from(current.drawIndexed(x, z))), `${label}: tile ${x},${z} indexes differently`);
    bytes += now.length;
  }
  return { tiles: expected.tiles.length, bytes };
}

test('the module draws the reference module\'s bytes for rooms, roofs and lights', async () => {
  const names = ['minecraft:stone', 'minecraft:glass', 'minecraft:glowstone', 'minecraft:lamp[lit=true]', 'minecraft:lamp'];
  let state = 7;
  const next = () => { state = (state * 1664525 + 1013904223) >>> 0; return state / 2 ** 32; };
  const blocks = new Map();
  for (let x = -20; x < 40; x += 1) for (let z = -12; z < 30; z += 1) {
    blocks.set(`${x},64,${z}`, 'minecraft:stone');
    if (next() < 0.3) blocks.set(`${x},${65 + Math.floor(next() * 6)},${z}`, names[Math.floor(next() * names.length)]);
  }
  const { tiles, bytes } = await sameBytes('rooms', mesherPack(), viewOf(blocks));
  assert.ok(tiles >= 4 && bytes > 100_000, `the sample draws faces (${tiles} tiles, ${bytes} bytes)`);
});

test('the module draws the reference module\'s bytes for every tile of community builds', async (t) => {
  const pack = bakedPack();
  const builds = communityBuilds();
  if (!pack || builds.length < 8) return t.skip('no baked pack in the data directory and community builds in MCBENCH_CORPUS');
  // Builds across the size range.
  const chosen = Array.from({ length: 8 }, (_, i) => builds[Math.floor(i * (builds.length - 1) / 7)]);
  let tiles = 0;
  let bytes = 0;
  for (const { file } of chosen) {
    const compared = await sameBytes(path.basename(path.dirname(file)), pack, viewOf(communityBlocks(file)));
    tiles += compared.tiles;
    bytes += compared.bytes;
  }
  console.log(`${chosen.length} community builds, ${tiles} tiles, ${bytes} bytes identical`);
});
