'use strict';

// Lays out the extractor's work folder as today's separate runs lay out
// theirs, with the repository's own functions: BlueMap's configuration
// (bluemap.js configFiles, the download folder `data` beside it) and the
// engine's world data pack (minecraft-dimension.js). Fails unless the
// Minecraft version and biome extract.sh passes are the repository's.
//
//   node prep.js <repository> <work folder> <minecraft version> <biome>

const fs = require('fs');
const path = require('path');

const [repo, work] = process.argv.slice(2, 4).map((arg) => path.resolve(arg));
const [version, biome] = process.argv.slice(4, 6);
if (!repo || !work || !version || !biome) throw new Error('usage: node prep.js <repository> <work folder> <minecraft version> <biome>');
const { MC_VERSION } = require(path.join(repo, 'server', 'assets.js'));
const { BIOME, writeDimensionPack } = require(path.join(repo, 'server', 'minecraft-dimension.js'));
if (version !== MC_VERSION || biome !== BIOME) throw new Error(`extract.sh names Minecraft ${version} and ${biome}; the repository's are ${MC_VERSION} and ${BIOME}`);
// bluemap.js reads the mesher module when it loads; only its configuration is used here.
const { configFiles } = require(path.join(repo, 'server', 'bluemap.js'));

fs.mkdirSync(work, { recursive: true });
for (const [name, text] of Object.entries(configFiles(path.join(work, 'data')))) {
  fs.mkdirSync(path.dirname(path.join(work, name)), { recursive: true });
  fs.writeFileSync(path.join(work, name), text);
}
writeDimensionPack(work);
