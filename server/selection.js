'use strict';

// Names a box the user picked in the viewer by what it mostly holds and
// where it sits in the build: "Dark oak stairs, top".

const { World, MAX_COORD } = require('./world');

function blockName(id) {
  const words = id.replace(/^minecraft:/, '').split('_').join(' ');
  return words[0].toUpperCase() + words.slice(1);
}

function placement(bounds, min, max) {
  const center = (axis) => (min[axis] + max[axis]) / 2;
  const share = (axis) => {
    const extent = bounds.max[axis] - bounds.min[axis] + 1;
    return extent < 3 ? 0.5 : (center(axis) - bounds.min[axis] + 0.5) / extent;
  };
  const y = share(1);
  const x = share(0);
  const z = share(2);
  const vertical = y > 2 / 3 ? 'top' : y < 1 / 3 ? 'base' : '';
  // North is -z and west is -x in Minecraft.
  const side = [z < 1 / 3 ? 'north' : z > 2 / 3 ? 'south' : '', x < 1 / 3 ? 'west' : x > 2 / 3 ? 'east' : '']
    .filter(Boolean).join('-');
  return [vertical, side && `${side} side`].filter(Boolean).join(', ') || 'middle';
}

function isBox(value) {
  return Array.isArray(value) && value.length === 3 && value.every(Number.isInteger);
}

function describeSelection(world, min, max) {
  if (!isBox(min) || !isBox(max) || min.some((value, axis) => value > max[axis])
    || [...min, ...max].some((value) => Math.abs(value) > MAX_COORD)) {
    throw Object.assign(new Error(`A selection needs integer corners within +/-${MAX_COORD} with min at most max`), { statusCode: 400 });
  }
  const counts = new Map();
  let blocks = 0;
  // The world's chunks inside the box, in the order a walk over the box's
  // chunks meets them: the cost follows the build, not the box.
  const low = min.map((value) => value >> 4);
  const high = max.map((value) => value >> 4);
  const keys = [];
  for (const key of world.chunks.keys()) {
    const at = World.coordsOf(key);
    if (at.every((value, axis) => value >= low[axis] && value <= high[axis])) keys.push({ key, at });
  }
  keys.sort(({ at: a }, { at: b }) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2]);
  for (const { key } of keys) {
    world.forEachInChunk(key, (x, y, z, entry) => {
      if (x < min[0] || x > max[0] || y < min[1] || y > max[1] || z < min[2] || z > max[2]) return;
      blocks += 1;
      counts.set(entry.id, (counts.get(entry.id) || 0) + 1);
    });
  }
  if (!blocks) return { blocks, block: null, label: 'Empty space' };
  const [material] = [...counts].reduce((best, next) => (next[1] > best[1] ? next : best));
  return { blocks, block: material, label: `${blockName(material)}, ${placement(world.bounds(), min, max)}` };
}

// A selection part of a message, as the composer sends it.
function readSelection(value) {
  const { letter, label, min, max, blocks, block } = value || {};
  if (typeof letter !== 'string' || !/^[A-Z]$/.test(letter) || typeof label !== 'string' || !label
    || !isBox(min) || !isBox(max) || !Number.isInteger(blocks) || (block !== null && !/^minecraft:[a-z0-9_]+$/.test(block))) {
    throw Object.assign(new Error('Each selection needs a letter, a label, integer corners, a block count and its main block'), { statusCode: 400 });
  }
  return { letter, label: label.slice(0, 120), min, max, blocks, block };
}

module.exports = { describeSelection, readSelection };
