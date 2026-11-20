'use strict';

// Preloaded (`node -r det.js -r startup.js <script>`): when a script loads
// server/index.js, what `startHttpServer` does before it listens follows at
// once (the turns the last run stopped in the middle of end as errors, then
// every bench is advanced), as the native core does when it starts; index.js
// itself already purged the archive at load. Nothing listens.

const path = require('node:path');
const Module = require('node:module');

const W = process.env.MCBENCH_NODE_DIFF;
const index = path.join(W, 'server', 'index.js');
let started = false;
const load = Module._load;
Module._load = function startup(request, parent, isMain) {
  const exports = load.call(this, request, parent, isMain);
  if (!started && Module._resolveFilename(request, parent, isMain) === index) {
    started = true;
    const store = require(path.join(W, 'server', 'store'));
    for (const id of store.interruptUnfinished()) console.log(`[startup] ended the turn the server stopped in ${id}`);
    for (const bench of store.listBenches()) exports.advanceBench(bench.id);
  }
  return exports;
};
