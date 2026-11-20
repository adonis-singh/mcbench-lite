'use strict';

// Today's reading routes through server/index.js's handleApi, no socket:
//   node -r det.js -r startup.js reads.js <out.json>   (MCBENCH_DATA_DIR: a clone)
// For every chat and bench of the folder, and the folder's lists, writes
// { route: body } with each route's status and JSON body (the chat list's
// stream: its first line). The model and the sandbox are stubbed as the
// route tests stub them; nothing here calls a model.

const fs = require('node:fs');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');

const W = process.env.MCBENCH_NODE_DIFF;
const server = (name) => path.join(W, 'server', name);
const [outFile, queriesFile] = process.argv.slice(2);

const llm = require(server('llm'));
llm.chatCompletion = async () => { throw new Error('no model in the reads'); };
require(server('assets')).getBlockRegistry = async () => ({});
const sandbox = require(server('sandbox'));
sandbox.reset = async () => {};
sandbox.shutdown = async () => {};
const store = require(server('store'));
const app = require(server('index'));

function request(method, pathname) {
  const req = Readable.from([]);
  Object.assign(req, { method, url: pathname, headers: {} });
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status) => { res.status = status; };
  res.write = (line) => { res.lines.push(JSON.parse(line)); return true; };
  res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid'))
    .catch((error) => { res.status = error.statusCode || 500; res.body = { error: error.message }; })
    .then(() => res);
}

async function main() {
  const out = {};
  const get = async (route) => {
    const res = await request('GET', route);
    out[route] = { status: res.status ?? 200, body: res.body ?? res.lines[0] ?? null };
  };
  await get('/api/conversations/stream');
  for (const { id } of store.listConversationIds()) await get(`/api/conversations/${id}`);
  for (const { id } of store.listBenchIds()) await get(`/api/benches/${id}`);
  for (const route of ['/api/archive', '/api/leaderboard', '/api/prompts', '/api/activity', '/api/config', '/api/settings']) await get(route);
  for (const q of JSON.parse(fs.readFileSync(queriesFile, 'utf8'))) {
    for (const scope of ['build', 'bench']) await get(`/api/search?scope=${scope}&q=${encodeURIComponent(q)}`);
  }
  fs.writeFileSync(outFile, JSON.stringify(out));
  store.close();
  process.exit(0);
}

main().catch((error) => { console.error(error); process.exit(1); });
