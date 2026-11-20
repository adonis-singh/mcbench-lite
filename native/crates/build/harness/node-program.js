'use strict';

// node node-program.js <program.js>: runs a build program in this Node
// (in-process, mc-dsl.js runProgram on an empty world) and prints its logs,
// for comparing a probe's output across Node versions and platforms.

const fs = require('fs');
const path = require('path');

const W = process.env.W || path.resolve(__dirname, '../../../..');
const { runProgram } = require(path.join(W, 'server/mc-dsl.js'));
const { World } = require(path.join(W, 'server/world.js'));

const { logs } = runProgram(fs.readFileSync(process.argv[2], 'utf8'), new World(), { stone: [{}, {}] }, null);
for (const line of logs) console.log(line);
console.log(`node ${process.version} v8 ${process.versions.v8} ${process.platform} ${process.arch}`);
