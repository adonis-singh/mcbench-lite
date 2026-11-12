'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// The page's handle on the engine worker (web/core-engine.mjs) when the
// worker ends: terminated by the page, or failed on its own.
function fakeWorker() {
  return {
    posted: [],
    terminated: false,
    onmessage: null,
    onerror: null,
    postMessage(message) { this.posted.push(message); },
    terminate() { this.terminated = true; },
  };
}

test('a terminated engine fails what is still asked of it and everything asked after', async () => {
  const { CoreEngine } = await import('../web/core-engine.mjs');
  const worker = fakeWorker();
  const engine = new CoreEngine(worker);
  const pending = engine.run(true);
  engine.terminate();
  assert.ok(worker.terminated);
  await assert.rejects(pending, /The engine was stopped/);
  await assert.rejects(engine.step(), /The engine was stopped/);
  assert.equal(worker.posted.length, 1, 'nothing more is sent to it');
  engine.terminate();
});

test('a start whose module does not load ends its worker', async () => {
  const { CoreEngine } = await import('../web/core-engine.mjs');
  const worker = fakeWorker();
  const starting = CoreEngine.start('http://app/core/missing.wasm', worker);
  assert.deepEqual(worker.posted, [{ type: 'load', wasm: 'http://app/core/missing.wasm', id: 1 }]);
  worker.onmessage({ data: { id: 1, error: 'http://app/core/missing.wasm is unavailable (HTTP 503)' } });
  await assert.rejects(starting, /is unavailable \(HTTP 503\)/);
  assert.ok(worker.terminated, 'the worker of a failed start is ended');
  assert.equal(worker.onmessage, null);
  // A start that loads keeps its worker.
  const kept = fakeWorker();
  const started = CoreEngine.start('http://app/core/a.wasm', kept);
  kept.onmessage({ data: { id: 1 } });
  assert.equal((await started).dead, null);
  assert.equal(kept.terminated, false);
});

test('a worker that fails ends the engine: what was asked fails, the failure is told, and nothing more comes from it', async () => {
  const { CoreEngine } = await import('../web/core-engine.mjs');
  const worker = fakeWorker();
  const engine = new CoreEngine(worker);
  const told = [];
  engine.onfailed = (message) => told.push(message);
  const pending = engine.place('setblock 0 64 0 minecraft:stone\n');
  worker.onerror({ message: 'out of memory' });
  await assert.rejects(pending, /The engine worker failed: out of memory/);
  assert.deepEqual(told, ['The engine worker failed: out of memory']);
  assert.ok(worker.terminated);
  assert.equal(worker.onmessage, null, 'a reply posted before it ended reaches nothing');
  assert.ok(engine.dead);
  await assert.rejects(engine.run(false), /The engine worker failed/);
});
