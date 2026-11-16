'use strict';

// The page's CoreEngine (web/core-engine.mjs) over a Node worker thread that
// runs the real web/core-worker.mjs: the worker's code outside a browser, for
// the tests and the parity scripts.

const fs = require('fs');
const path = require('path');
const { Worker } = require('worker_threads');

class NodeWorker {
  constructor() {
    this.worker = new Worker(path.join(__dirname, 'wasm-worker-shim.mjs'));
    this.worker.on('message', (data) => this.onmessage?.({ data }));
    this.worker.on('error', (error) => this.onerror?.({ message: error.message }));
  }

  postMessage(message, transfer) {
    this.worker.postMessage(message, transfer);
  }

  terminate() {
    this.worker.terminate();
  }
}

// Minecraft's tables the module reads, as the page hands them over
// (web/core-world.mjs): the bytes of blocks.txt and loot.json of `folder`,
// the tables folder this machine made unless named.
function engineTables(folder = require('../server/engine').tablesFolder()) {
  return { blocks: fs.readFileSync(path.join(folder, 'blocks.txt')), loot: fs.readFileSync(path.join(folder, 'loot.json')) };
}

// Each module file compiled once, as the page compiles each module once.
const modules = new Map();
function compiledModule(wasmFile) {
  if (!modules.has(wasmFile)) modules.set(wasmFile, WebAssembly.compile(fs.readFileSync(wasmFile)));
  return modules.get(wasmFile);
}

// The module file this checkout runs (server/engine.js installEngine), once
// its tables are made: a checkout without them fails here, saying so.
async function engineFile() {
  const { module } = await require('../server/engine').installEngine();
  require('../server/engine').tablesFolder();
  return module;
}

// The module this checkout runs (server/engine.js installEngine), compiled,
// and its tables: what a CoreWorld is made of.
async function engineModule() {
  const { module } = await require('../server/engine').installEngine();
  return { module: await compiledModule(module), tables: engineTables() };
}

// A CoreEngine over the module file `wasmFile` (this checkout's, unless
// named) in a Node worker thread, with the tables of `tablesFolder` (this
// machine's, unless named). The tables are read before the worker is made,
// so tables that cannot be read leave no worker behind.
async function startNodeEngine(wasmFile = null, tablesFolder = undefined) {
  const { CoreEngine } = await import('../web/core-engine.mjs');
  const file = wasmFile || await engineFile();
  const tables = engineTables(tablesFolder);
  return CoreEngine.load(await compiledModule(file), tables, new NodeWorker());
}

module.exports = { startNodeEngine, engineModule, engineFile, engineTables };
