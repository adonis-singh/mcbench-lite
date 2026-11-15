'use strict';

// Community builds of the corpus MCBENCH_CORPUS names (directories separated
// by the path delimiter; each build is <dir>/<set>/imports/<build>/artifact.json)
// and the baked pack of the app's data directory, for tests of the mesher on
// real worlds: they find no builds while MCBENCH_CORPUS is unset.

const fs = require('node:fs');
const path = require('node:path');

const DATA = path.resolve(process.env.MCBENCH_DATA_DIR || path.join(__dirname, '..', '..', 'data'));
const CORPUS = process.env.MCBENCH_CORPUS ? process.env.MCBENCH_CORPUS.split(path.delimiter).map((dir) => path.resolve(dir)) : [];

function communityBuilds() {
  const imports = [];
  for (const root of CORPUS) {
    for (const set of fs.readdirSync(root)) {
      const folder = path.join(root, set, 'imports');
      if (!fs.existsSync(folder)) continue;
      for (const build of fs.readdirSync(folder)) {
        const file = path.join(folder, build, 'artifact.json');
        if (fs.existsSync(file)) imports.push({ file, size: fs.statSync(file).size });
      }
    }
  }
  return imports.filter(({ size }) => size > 100e3 && size < 4.5e6).sort((a, b) => a.size - b.size);
}

// An artifact's blocks as "x,y,z" -> state name, at Minecraft heights.
function communityBlocks(file) {
  const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
  const blocks = new Map();
  for (const { pos: [x, y, z], id, states } of artifact.blocks) {
    const properties = Object.entries(states || {}).sort(([a], [b]) => (a < b ? -1 : 1));
    blocks.set(`${x},${y + 64},${z}`, properties.length ? `${id}[${properties.map(([k, v]) => `${k}=${v}`).join(',')}]` : id);
  }
  return blocks;
}

// A file of the data directory's bake (its web/mcbench-pack.bin, or
// render-layers.tsv beside it), or null where nothing is baked.
function bakedFile(name) {
  const root = path.join(DATA, 'minecraft-live', 'bluemap');
  if (!fs.existsSync(root)) return null;
  const found = fs.readdirSync(root).map((id) => path.join(root, id, name)).find((file) => fs.existsSync(file));
  return found ? fs.readFileSync(found) : null;
}

const bakedPack = () => bakedFile(path.join('web', 'mcbench-pack.bin'));

module.exports = { communityBuilds, communityBlocks, bakedPack, bakedFile };
