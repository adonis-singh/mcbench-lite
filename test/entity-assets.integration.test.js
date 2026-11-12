'use strict';

const assert = require('node:assert/strict');
const { execFileSync } = require('node:child_process');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { test } = require('node:test');

const { MC_VERSION } = require('../server/assets');

// Reads Minecraft's client jar and BlueMap's jar from a bake's folder
// (MCBENCH_DATA_DIR, as the app has it), so run after the app made one.
const DATA = path.resolve(process.env.MCBENCH_DATA_DIR || path.join(__dirname, '..', 'data'), 'minecraft-live', 'bluemap');

test('the bake writes an atlas and an index that name only what they hold', { skip: process.env.MCBENCH_ENTITY_INTEGRATION !== '1', timeout: 300000 }, async (t) => {
  const { EntityIndex, blockMesh, EntityModels } = await import('../web/entity-models.mjs');
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-entity-assets-'));
  t.after(() => fs.rmSync(folder, { recursive: true, force: true }));
  const bluemap = path.join(DATA, 'bluemap-5.22-cli.jar');
  execFileSync(process.env.MCBENCH_JAVAC || 'javac', ['-proc:none', '-classpath', bluemap, '-d', folder, path.join(__dirname, '..', 'server', 'McbenchEntityAssets.java')]);
  const atlas = path.join(folder, 'atlas.png');
  const indexFile = path.join(folder, 'index.json');
  execFileSync(process.env.MCBENCH_BLUEMAP_JAVA || 'java', ['-cp', [bluemap, folder].join(path.delimiter), 'McbenchEntityAssets',
    path.join(DATA, 'data', `minecraft-client-${MC_VERSION}.jar`), atlas, indexFile]);

  const png = fs.readFileSync(atlas);
  assert.equal(png.readUInt32BE(16), 2048, 'the atlas is 2048 wide');
  const index = JSON.parse(fs.readFileSync(indexFile, 'utf8'));
  assert.equal(png.readUInt32BE(20), index.atlas.height, 'the index holds the atlas height');
  const entityIndex = new EntityIndex(index); // names only what it holds

  // Every block with variants has a drawing, except air. (A multipart block
  // with no properties given draws only its parts that need none.)
  const empty = Object.keys(index.blocks).filter((id) => index.blocks[id].variants && blockMesh(entityIndex, id) === null);
  assert.deepEqual(empty, ['minecraft:air', 'minecraft:cave_air', 'minecraft:void_air']);

  // The kinds the entity layer draws have what it reads.
  for (const type of ['minecraft:minecart', 'minecraft:oak_boat', 'minecraft:bamboo_raft', 'minecraft:oak_chest_boat', 'minecraft:armor_stand', 'minecraft:arrow']) {
    assert.ok(index.entities[type], `a texture for ${type}`);
  }
  assert.ok(index.textures['painting/back'] && index.textures['painting/alban']);
  assert.deepEqual(index.items['minecraft:redstone'], { sprite: 'item/redstone' });
  assert.equal(index.items['minecraft:stone'].model, 'block/stone');
  assert.deepEqual(index.blocks['minecraft:sand'], { variants: { '': { model: 'block/sand' } } });
  assert.deepEqual(index.blocks['minecraft:grass_block'].tint, [145, 189, 89]);
  assert.ok(index.blocks['minecraft:chest'].variants[''].model.startsWith('special/chest/'), 'chests are drawn from their entity texture');
  assert.ok(index.models['block/item_frame'] && index.models['block/glow_item_frame']);

  const models = new EntityModels(entityIndex);
  const base = { id: 1, pos: [0, 0, 0], yaw: 0, pitch: 0, width: 1, height: 1 };
  for (const type of ['minecraft:minecart', 'minecraft:oak_boat', 'minecraft:bamboo_raft', 'minecraft:oak_chest_boat', 'minecraft:armor_stand', 'minecraft:arrow']) {
    assert.ok(models.describe({ ...base, type }).parts[0].mesh, `${type} is drawn from a model`);
  }
});
