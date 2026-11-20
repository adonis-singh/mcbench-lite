'use strict';

// Today's routes of the window's commands through server/index.js's
// handleApi, no socket:
//   node -r det.js -r startup.js commands.js <ops.json> <out.json>   (MCBENCH_DATA_DIR: a clone)
// Each op is { method, path, body? }; writes [{ status, body, lines }] in
// order (`lines`: a chat POST's NDJSON). The model and the sandbox are
// stubbed as the route tests stub them: a turn that starts fails at its
// model, so it ends in a stored error.

const fs = require('node:fs');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');

const W = process.env.MCBENCH_NODE_DIFF;
const server = (name) => path.join(W, 'server', name);
const [opsFile, outFile] = process.argv.slice(2);

const llm = require(server('llm'));
llm.chatCompletion = async () => { throw new Error('no model in the commands'); };
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
  res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid'))
    .catch((error) => { res.status = error.statusCode || 500; res.body = { error: error.message }; })
    .then(() => res);
}

// `$error:<chat>` in an op's text is the id of that chat's last error
// message, made by an earlier op's turn.
function resolve(text) {
  return text.replace(/\$error:([0-9a-f-]+)/g, (_, chat) => {
    const errors = store.getConversation(chat).messages.filter((m) => m.role === 'error');
    return errors[errors.length - 1].id;
  });
}

async function main() {
  const out = [];
  for (const { method, path: route, body } of JSON.parse(fs.readFileSync(opsFile, 'utf8'))) {
    const res = await request(method, resolve(route), body == null ? body : JSON.parse(resolve(JSON.stringify(body))));
    out.push({ status: res.status ?? 200, body: res.body ?? res.lines[0] ?? null, lines: res.lines });
  }
  fs.writeFileSync(outFile, JSON.stringify(out));
  store.close();
  process.exit(0);
}

main().catch((error) => { console.error(error); process.exit(1); });
