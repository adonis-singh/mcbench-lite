'use strict';

// The Java sources today's runs compile, read from the repository so that
// extract.sh compiles the same ones: the engine's (server/engine.js
// SOURCES) and the bake's (server/bluemap.js BAKE, read from its text: the
// module does not export it). Prints two lines, `engine` and `bake`, each
// with its files' absolute paths separated by tabs.
//
//   node sources.js <repository>

const fs = require('fs');
const path = require('path');

const repo = path.resolve(process.argv[2]);
const { SOURCES } = require(path.join(repo, 'server', 'engine.js'));
const bluemap = fs.readFileSync(path.join(repo, 'server', 'bluemap.js'), 'utf8');
const found = bluemap.match(/^const BAKE = \[([^\]]*)\]\.map\(\(name\) => path\.join\(__dirname, name\)\);$/m);
if (!found) throw new Error('server/bluemap.js has no BAKE line of the form extract.sh reads: read how it compiles the bake now');
const bake = [...found[1].matchAll(/'([^']+\.java)'/g)].map((match) => path.join(repo, 'server', match[1]));
if (!bake.length) throw new Error('server/bluemap.js BAKE names no source');
console.log(['engine', ...SOURCES].join('\t'));
console.log(['bake', ...bake].join('\t'));
