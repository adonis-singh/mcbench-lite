'use strict';

// Today's Settings routes through server/index.js's handleApi, no socket:
//   node -r det.js -r startup.js settings.js <ops.json> <out.json>   (MCBENCH_DATA_DIR: a clone)
// Each op is { get: route }, { del: route } or { put: edits }: a PUT sends
// the page's draft (web/app.js readSettings and editable: the last
// GET /api/settings body, each provider's apiKey '' where it has a key)
// with `edits` applied (titleModel, shareRepo, and per provider any of
// enabled, apiKey, hiddenModels, modelOrder). Writes [{ status, body,
// settings }] in order, `settings` the sha256 of settings.json's bytes
// after the op (the file holds keys: only its hash leaves this script).

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');

const W = process.env.MCBENCH_NODE_DIFF;
const server = (name) => path.join(W, 'server', name);
const [opsFile, outFile] = process.argv.slice(2);

const llm = require(server('llm'));
llm.chatCompletion = async () => { throw new Error('no model in the settings'); };
require(server('assets')).getBlockRegistry = async () => ({});
const sandbox = require(server('sandbox'));
sandbox.reset = async () => {};
sandbox.shutdown = async () => {};
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
  res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid'))
    .catch((error) => { res.status = error.statusCode || 500; res.body = { error: error.message }; })
    .then(() => res);
}

function fileHash() {
  const file = process.env.MCBENCH_SETTINGS_FILE;
  if (!fs.existsSync(file)) return null;
  return crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
}

async function main() {
  const out = [];
  let saved = null;
  for (const op of JSON.parse(fs.readFileSync(opsFile, 'utf8'))) {
    let res;
    if (op.get) {
      res = await request('GET', op.get);
      if (op.get === '/api/settings') saved = res.body;
    } else if (op.del) {
      res = await request('DELETE', op.del);
    } else {
      const draft = { ...saved, providers: Object.fromEntries(Object.entries(saved.providers).map(([id, connection]) => [id, { ...connection, ...(connection.envKey ? { apiKey: '' } : {}) }])) };
      const { providers = {}, ...rest } = op.put;
      Object.assign(draft, rest);
      for (const [id, fields] of Object.entries(providers)) Object.assign(draft.providers[id], fields);
      res = await request('PUT', '/api/settings', draft);
    }
    out.push({ status: res.status ?? 200, body: res.body ?? null, settings: fileHash() });
  }
  fs.writeFileSync(outFile, JSON.stringify(out));
  store.close();
  process.exit(0);
}

main().catch((error) => { console.error(error); process.exit(1); });
