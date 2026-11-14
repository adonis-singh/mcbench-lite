'use strict';

const { BlockStates } = require('../../server/block-states');

// A block table in McbenchBlockTable.java's format, of states written for a
// test (not Minecraft's: what is under test is the code's handling of a
// table). `states`, by block, a block's first state its default:
//   { name: 'test:slab[type=bottom]', emission, lightBlock, full, faces, fluid, model }
// emission and lightBlock 0 unless given; `full`, the full faces as bits over
// down, up, north, south, west, east (0 unless given); `faces`, the six face
// shapes light is tested through (all 0, the empty shape, unless given);
// `fluid`, the name of the state of its fluid (none unless given); `model`,
// whether it is drawn as a model (unless it is false).
// Face shape 1 is full; the shapes past it are partial, and of those only
// the ordered pairs in `pairs` close a face together. `skips`, the states
// that skip rendering beside others: [state, neighbour, directions], the
// directions as bits like `full`'s; each state named first is a row class
// of its own, each named second a column class.
function blockTable(states, { shapes = 2, pairs = [], skips = [] } = {}) {
  const names = states.map(({ name }) => name);
  const idOf = (name) => {
    const id = names.indexOf(name);
    if (id < 0) throw new Error(`No test state ${name}`);
    return id;
  };
  const rowNames = [...new Set(skips.map(([a]) => a))];
  const columnNames = [...new Set(skips.map(([, b]) => b))];
  const lines = [];
  let block = null;
  for (const { name, emission = 0, lightBlock = 0, full = 0, faces = [0, 0, 0, 0, 0, 0], fluid, model = true } of states) {
    const id = name.split('[')[0];
    if (id !== block) lines.push(`default\t${id}\t${name}`);
    block = id;
    const skip = `${rowNames.indexOf(name) + 1},${columnNames.indexOf(name) + 1}`;
    lines.push([name, emission, lightBlock, full, faces.join(','), fluid ? idOf(fluid) : 0, model ? 1 : 0, skip].join('\t'));
  }
  const closes = (a, b) => a === 1 || b === 1 || pairs.some(([p, q]) => p === a && q === b);
  let bits = '';
  for (let a = 0; a < shapes; a++) for (let b = 0; b < shapes; b++) bits += closes(a, b) ? '1' : '0';
  lines.push(`faceoccludes\t${shapes}\t${bits}`);
  const rows = rowNames.map((a) => {
    const masks = columnNames.map((b) => skips.filter(([p, q]) => p === a && q === b).reduce((mask, [, , directions]) => mask | directions, 0));
    return [1, ...masks].join(',');
  });
  lines.push(['skiprendering', rowNames.length + 1, columnNames.length + 1, '0', ...rows].join('\t'));
  return `${lines.join('\n')}\n`;
}

// Render layers in McbenchRenderLayers.java's format for such states: a
// state's model is drawn blended if `blended`, its fluid if `fluidBlended`.
function renderLayers(states) {
  const lines = ['layer\tOPAQUE\t0', 'layer\tBLENDED\t1'];
  for (const { name, blended = false, fluidBlended = false } of states) lines.push(`${name}\t${blended ? 1 : 0}\t${fluidBlended ? 1 : 0}`);
  return `${lines.join('\n')}\n`;
}

// The BlockStates of a block table's text (Minecraft's own, in tests of
// what it holds of light, faces, fluids and skipRendering), read with render
// layers that draw every state opaque.
function opaqueStates(table) {
  const names = table.split('\n').filter((line) => line && !/^(default|faceoccludes|skiprendering)\t/.test(line)).map((line) => line.split('\t')[0]);
  return new BlockStates(table, renderLayers(names.map((name) => ({ name }))));
}

// The BlockStates (server/block-states.js) of such a table, air first.
function testBlockStates(states, options) {
  const all = [{ name: 'minecraft:air', model: false }, ...states];
  return new BlockStates(blockTable(all, options), renderLayers(all));
}

module.exports = { blockTable, renderLayers, opaqueStates, testBlockStates };
