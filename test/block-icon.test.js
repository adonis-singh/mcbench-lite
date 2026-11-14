'use strict';

const { test } = require('node:test');
const assert = require('node:assert');

const { _internal: { decodePng, encodePng, particleTexture, icon, loadAssets } } = require('../server/block-icon');

test('PNGs round-trip through the encoder and decoder', () => {
  const pixels = Buffer.from(Array.from({ length: 2 * 2 * 4 }, (_, index) => (index * 37) & 255));
  const { width, pixels: decoded } = decodePng(encodePng(2, pixels));
  assert.strictEqual(width, 2);
  assert.deepStrictEqual(decoded, pixels);
});

test('a block shows the texture its model breaks into', () => {
  const definitions = {
    oak_stairs: { variants: { 'facing=east': { model: 'minecraft:block/oak_stairs' } } },
    oak_fence: { multipart: [{ apply: { model: 'minecraft:block/oak_fence_post' } }] },
  };
  const models = {
    'block/stairs': { textures: { particle: '#side' } },
    'block/oak_stairs': { parent: 'minecraft:block/stairs', textures: { side: 'minecraft:block/oak_planks' } },
    'block/fence_post': { textures: { particle: '#texture' } },
    'block/oak_fence_post': { parent: 'block/fence_post', textures: { texture: 'block/oak_planks' } },
  };
  assert.strictEqual(particleTexture('oak_stairs', { definitions, models }), 'block/oak_planks');
  assert.strictEqual(particleTexture('oak_fence', { definitions, models }), 'block/oak_planks');
});

test('icons are square first frames multiplied by their colour', async () => {
  const loaded = await loadAssets();
  const water = decodePng(icon('water', loaded, 0xffffff));
  assert.strictEqual(water.pixels.length, water.width * water.width * 4);
  const tinted = decodePng(icon('water', loaded, 0x3f76e4));
  for (let at = 0; at < water.pixels.length; at += 4) {
    assert.deepStrictEqual([...tinted.pixels.subarray(at, at + 4)], [
      Math.round(water.pixels[at] * 0x3f / 255), Math.round(water.pixels[at + 1] * 0x76 / 255), Math.round(water.pixels[at + 2] * 0xe4 / 255), water.pixels[at + 3],
    ]);
  }
});
