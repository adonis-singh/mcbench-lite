'use strict';

// Today's bench and prompt library routes through server/index.js's
// handleApi, no socket:
//   node -r det.js -r startup.js bench.js <ops.json> <out.json>   (MCBENCH_DATA_DIR: a clone)
// Each op is { method, path, body? }; after each, the script waits until
// nothing is busy (GET /api/activity: no turn, edit or title under way, no
// round queued). Writes [{ status, body, text }] in order, `text` the
// body's JSON as the route wrote it.
//
// The model: a title request (no tools) is answered 'A title'; a model
// named fail/... throws 'The model failed'; any other answers 'Done.' with
// fixed telemetry (tests/core_bench_diff.rs answers the same).
//
// In an op's path and body, `$bench:N` is the id of the Nth bench this run
// made, `$round:N:i` the id of round i of its line, `$node:N:k` of its
// kth stored round, `$member:N:s` its model in slot s.

const fs = require('node:fs');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');

const W = process.env.MCBENCH_NODE_DIFF;
const server = (name) => path.join(W, 'server', name);
const [opsFile, outFile] = process.argv.slice(2);

const TELEMETRY = {
  provider: 'openrouter', model: 'diff/model', effort: null, startedAt: '2026-11-17T00:00:00.000Z', wallMs: 10, firstDeltaMs: 5,
  contextBytes: 100, messageCount: 2, usage: { inputTokens: 10, outputTokens: 5, cachedInputTokens: 0, cacheWriteTokens: 0, reasoningTokens: null }, contextTokens: 10,
};
const llm = require(server('llm'));
llm.chatCompletion = async (options) => {
  if (!options.tools.length) return { content: 'A title' };
  if (options.model.startsWith('fail/')) throw new Error('The model failed');
  options.onDelta({ content: 'Done.' });
  return { content: 'Done.', finish_reason: 'stop', telemetry: structuredClone(TELEMETRY) };
};
llm.modelInfo = async () => ({ imageInput: true });
llm.closeHarnessSession = () => {};
require(server('assets')).getBlockRegistry = async () => ({});
const sandbox = require(server('sandbox'));
sandbox.reset = async () => {};
sandbox.shutdown = async () => {};
sandbox.syncAttachments = () => {};
const store = require(server('store'));
const app = require(server('index'));

function request(method, pathname, body) {
  const req = Readable.from(body == null ? [] : [Buffer.from(JSON.stringify(body))]);
  Object.assign(req, { method, url: pathname, headers: { 'content-type': 'application/json' } });
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status) => { res.status = status; };
  res.setHeader = () => {};
  res.write = (line) => { res.lines.push(JSON.parse(line)); return true; };
  res.end = (payload) => { if (payload) { res.text = String(payload); res.body = JSON.parse(payload); } res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid'))
    .catch((error) => { res.status = error.statusCode || 500; res.body = { error: error.message }; res.text = JSON.stringify(res.body); })
    .then(() => res);
}

const benches = [];
function resolve(text) {
  return text
    .replace(/\$bench:(\d+)/g, (_, n) => benches[n])
    .replace(/\$round:(\d+):(\d+)/g, (_, n, i) => store.getBench(benches[n]).rounds[i].id)
    .replace(/\$node:(\d+):(\d+)/g, (_, n, k) => store.getBench(benches[n]).nodes[k].id)
    .replace(/\$member:(\d+):(\d+)/g, (_, n, s) => store.benchMembers(benches[n])[s].id);
}

async function idle() {
  for (;;) {
    const res = await request('GET', '/api/activity');
    if (!res.body.busy) return;
    await new Promise((done) => setTimeout(done, 2));
  }
}

async function main() {
  const out = [];
  for (const { method, path: route, body } of JSON.parse(fs.readFileSync(opsFile, 'utf8'))) {
    const res = await request(method, resolve(route), body == null ? body : JSON.parse(resolve(JSON.stringify(body))));
    if (method === 'POST' && route === '/api/benches' && res.body?.id) benches.push(res.body.id);
    await idle();
    out.push({ status: res.status ?? 200, body: res.body ?? null, text: res.text ?? null });
  }
  fs.writeFileSync(outFile, JSON.stringify(out));
  store.close();
  process.exit(0);
}

main().catch((error) => { console.error(error); process.exit(1); });
