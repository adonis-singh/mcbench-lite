'use strict';

// A failure outside any request (an uncaught exception or an unhandled
// rejection) is fatal: server/index.js logs it, runs the one shutdown for at
// most its deadline, and exits 1. Its own file, since that shutdown closes
// the store.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');
const { test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-server-fatal-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const llm = require('../server/llm');
// A turn's model never answers, and does not notice the stop either.
llm.chatCompletion = (options) => (options.tools.length ? new Promise(() => {}) : Promise.resolve({ content: 'A title' }));
llm.modelInfo = async () => ({ imageInput: true });
const sandbox = require('../server/sandbox');
sandbox.reset = async () => {};
sandbox.shutdown = async () => {};
const app = require('../server/index');

function call(method, pathname, body) {
  const req = Readable.from(body === undefined ? [] : [Buffer.from(JSON.stringify(body))]);
  Object.assign(req, { method, url: pathname, headers: {} });
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status) => { res.status = status; res.headersSent = true; };
  res.write = (line) => { res.lines.push(JSON.parse(line)); return true; };
  res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => {
    res.status = error.statusCode || 500;
    res.body = { error: error.message };
  }).then(() => res);
}
const tick = () => new Promise(setImmediate);

test('a failure outside a request logs [fatal], stops serving, and exits 1 once the shutdown ends or its deadline passes', async (t) => {
  const { body: { id } } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'chat/model' });
  call('POST', `/api/conversations/${id}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build a hut' });
  await tick();
  assert.ok(app.reservations.turn(id), 'a turn is running');
  const { body: { id: other } } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'chat/model' });

  const proc = new EventEmitter();
  const exits = [];
  proc.exit = (code) => exits.push(code);
  app.exitOnFatal(proc);
  const logged = t.mock.method(console, 'error', () => {});
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const failure = new Error('boom');
  proc.emit('uncaughtException', failure);
  assert.deepEqual(logged.mock.calls[0].arguments, ['[fatal]', failure]);
  assert.equal(app.reservations.turn(id).controller.signal.aborted, true, 'the shutdown is under way');
  const refused = await call('POST', `/api/conversations/${other}/chat`, { provider: 'openrouter', model: 'chat/model', resolution: 1, text: 'Build' });
  assert.deepEqual([refused.status, refused.body.error], [503, 'The server is shutting down'], 'no turn starts');

  // The turn never ends, so the shutdown does not either: the deadline exits.
  t.mock.timers.tick(app.FATAL_SHUTDOWN_MS - 1);
  await tick();
  assert.deepEqual(exits, []);
  t.mock.timers.tick(1);
  await tick();
  assert.deepEqual(exits, [1]);

  // A rejection nobody handled is the same.
  const reason = new Error('lost');
  proc.emit('unhandledRejection', reason);
  assert.deepEqual(logged.mock.calls.at(-1).arguments, ['[fatal]', reason]);
  t.mock.timers.tick(app.FATAL_SHUTDOWN_MS);
  await tick();
  assert.deepEqual(exits, [1, 1]);
});
