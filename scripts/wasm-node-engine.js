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

// Each module file compiled once, as the page compiles each module once.
const modules = new Map();
async function startNodeEngine(wasmFile) {
  const { CoreEngine } = await import('../web/core-engine.mjs');
  if (!modules.has(wasmFile)) modules.set(wasmFile, WebAssembly.compile(fs.readFileSync(wasmFile)));
  return CoreEngine.load(await modules.get(wasmFile), new NodeWorker());
}

module.exports = { startNodeEngine };
