'use strict';

// A small entity index in McbenchEntityAssets.java's format, for testing the
// entity layer without Minecraft's assets: a few blocks, one item sprite,
// and a texture for each entity model.

const TEXTURES = {
  'block/stone': [0, 0, 16, 16],
  'block/grass_top': [16, 0, 16, 16],
  'block/grass_side': [32, 0, 16, 16],
  'block/dirt': [48, 0, 16, 16],
  'block/tnt_side': [64, 0, 16, 16],
  'item/redstone': [80, 0, 16, 16],
  'painting/back': [96, 0, 16, 16],
  'painting/alban': [112, 0, 32, 16],
  'entity/minecart': [0, 16, 64, 32],
  'entity/boat/oak': [64, 16, 128, 64],
  'entity/armorstand/wood': [0, 48, 64, 64],
  'entity/projectiles/arrow': [192, 0, 32, 32],
};

const face = (texture, extra = {}) => ({ uv: [0, 0, 16, 16], texture, ...extra });
const cube = (faces, from = [0, 0, 0], to = [16, 16, 16]) => ({ from, to, faces });
const all = (texture) => cube(Object.fromEntries(['down', 'up', 'north', 'south', 'west', 'east'].map((name) => [name, face(texture)])));

function entityIndex() {
  return {
    atlas: { width: 256, height: 128 },
    textures: structuredClone(TEXTURES),
    models: {
      'block/stone': { elements: [all('block/stone')] },
      'block/grass_block': {
        elements: [cube({
          down: face('block/dirt'),
          up: face('block/grass_top', { tinted: true }),
          north: face('block/grass_side'),
          south: face('block/grass_side'),
          west: face('block/grass_side'),
          east: face('block/grass_side'),
        })],
      },
      'block/tnt': { elements: [all('block/tnt_side')] },
      // A slab whose west face is rotated a quarter turn, and a tilted plane.
      'block/slab': { elements: [cube({ north: face('block/stone'), west: face('block/stone', { rotation: 90 }) }, [0, 0, 0], [16, 8, 16])] },
      'block/tilted': { elements: [{ from: [0, 0, 8], to: [16, 16, 8], rotation: { origin: [8, 8, 8], axis: 'y', angle: 45 }, faces: { north: face('block/stone') } }] },
      'block/post': { elements: [all('block/stone')] },
      'block/side': { elements: [cube({ north: face('block/stone') }, [7, 0, 0], [9, 16, 8])] },
      'block/frame': { elements: [cube({ north: face('block/stone'), south: face('block/stone') }, [3, 3, 15.5], [13, 13, 16])] },
    },
    blocks: {
      'minecraft:stone': { variants: { '': { model: 'block/stone' } } },
      'minecraft:grass_block': { variants: { '': { model: 'block/grass_block' } }, tint: [145, 189, 89] },
      'minecraft:tnt': { variants: { '': { model: 'block/tnt' } } },
      'minecraft:slab': { variants: { 'facing=north': { model: 'block/slab' }, 'facing=east': { model: 'block/slab', y: 90 } } },
      'minecraft:tilted': { variants: { '': { model: 'block/tilted' } } },
      'minecraft:fence': {
        multipart: [
          { apply: { model: 'block/post' } },
          { when: { north: 'true' }, apply: { model: 'block/side' } },
          { when: { OR: [{ east: 'true' }, { south: 'true' }] }, apply: { model: 'block/side', y: 90 } },
        ],
      },
      'minecraft:item_frame': { variants: { 'map=false': { model: 'block/frame' } } },
      'minecraft:glow_item_frame': { variants: { 'map=false': { model: 'block/frame' } } },
      'minecraft:air': {},
    },
    items: {
      'minecraft:redstone': { sprite: 'item/redstone' },
      'minecraft:stone': { model: 'block/stone' },
    },
    entities: {
      'minecraft:minecart': 'entity/minecart',
      'minecraft:oak_boat': 'entity/boat/oak',
      'minecraft:armor_stand': 'entity/armorstand/wood',
      'minecraft:arrow': 'entity/projectiles/arrow',
    },
  };
}

module.exports = { entityIndex };
