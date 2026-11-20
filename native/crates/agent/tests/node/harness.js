'use strict';

// The Node side of mcbench-agent's harness differential: each case of
// harness_cases.py (one chat per CLI, six turns) through today's chat route
// (handleApi → handleChat → executeTurn → runTurn → llm.chatCompletion →
// claude-code.js / codex-cli.js and harness.js's relay → the bridge → the
// fake CLI, mcbench-providers' tests/fakes/fake_cli.py). Tool bodies are
// replay.js's; a title goes to an API provider answered once the turn ends.
//
//   node -r det.js harness.js <checkout> <cases.json> <dir>
//
// Writes <dir>/<cli>/{bin, data} and <dir>/out.json: per case the chat id,
// each turn's NDJSON lines (turn_snapshot left out) and the fake's record.

const fs = require('node:fs');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');

const [repo, casesFile, dir] = process.argv.slice(2);
const cases = JSON.parse(fs.readFileSync(casesFile, 'utf8'));
const server = (name) => path.join(repo, 'server', name);
const FAKE = path.join(repo, 'native', 'crates', 'providers', 'tests', 'fakes', 'fake_cli.py');

// One data folder for every case, the chats told apart by id.
process.env.MCBENCH_DATA_DIR = path.join(dir, 'data');
process.env.MCBENCH_SETTINGS_FILE = path.join(dir, 'settings.json');

const replay = require('./replay.js');
let current = null;
let app = null;

const settings = require(server('settings'));
const { get, publicSettings } = settings;
settings.get = () => ({ ...get(), titleModel: { provider: 'openrouter', model: 'title/model', effort: 'low' } });
settings.publicSettings = () => {
  const out = publicSettings();
  for (const id of ['claude-code', 'codex-cli']) out.providers[id] = { ...out.providers[id], enabled: true, connected: true };
  return out;
};
const llm = require(server('llm'));
const real = llm.chatCompletion;
llm.chatCompletion = async (options) => {
  if (options.provider !== 'openrouter') return real(options);
  // The title, once its chat's turn has ended (as on the Rust side).
  const id = current.id;
  for (let hold; (hold = app.reservations.get(id));) await hold.done;
  return { content: 'A title', telemetry: { provider: 'openrouter', model: options.model, effort: options.effort || null, startedAt: '', wallMs: 0, firstDeltaMs: null, contextBytes: 0, messageCount: 0, usage: null, contextTokens: null } };
};
llm.modelInfo = async () => ({ imageInput: true });
require(server('assets')).getBlockRegistry = async () => ({});
const sandbox = require(server('sandbox'));
sandbox.reset = async () => {};
sandbox.syncAttachments = () => {};
sandbox.shutdown = async () => {};
require(server('prompts')).apply = () => replay.nextTool('prompt_library').result.prompts;
replay.loadAgent();
const claudeCode = require(server('claude-code'));
const codexCli = require(server('codex-cli'));
const { LAUNCHERS } = codexCli._internal;
LAUNCHERS.linux = LAUNCHERS.darwin;
const store = require(server('store'));
app = require(server('index'));

function request(method, pathname, body) {
  const req = Readable.from(body === undefined ? [] : [Buffer.from(JSON.stringify(body))]);
  Object.assign(req, { method, url: pathname, headers: {} });
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status) => { res.status = status; };
  res.write = (line) => { res.lines.push(line.replace(/\n$/, '')); return true; };
  res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => { res.status = error.statusCode || 500; res.body = { error: error.message }; }).then(() => res);
}

async function main() {
  const out = [];
  for (const c of cases) {
    const bin = path.join(dir, c.cli, 'bin');
    fs.mkdirSync(bin, { recursive: true });
    const program = path.join(bin, c.cli);
    fs.copyFileSync(FAKE, program);
    fs.chmodSync(program, 0o755);
    fs.writeFileSync(path.join(bin, 'script.json'), JSON.stringify(c.script));
    if (c.cli === 'claude') claudeCode.configure({ binary: program });
    else codexCli.configure({ binary: program, home: path.join(dir, 'codex-login') });
    const id = (await request('POST', '/api/conversations', { provider: c.provider, model: c.model })).body.id;
    const turns = [];
    for (const turn of c.turns) {
      current = { tools: structuredClone(turn.tools), id, stop: () => app.reservations.turn(id).controller.abort() };
      replay.use(current);
      const res = await request('POST', `/api/conversations/${id}/chat`, { provider: c.provider, model: c.model, resolution: 1, text: turn.text });
      for (let hold; (hold = app.reservations.get(id));) await hold.done;
      for (let i = 0; i < 20; i += 1) await new Promise(setImmediate);
      turns.push({ status: res.status ?? 200, error: res.body?.error ?? null, events: res.lines.filter((line) => !line.startsWith('{"type":"turn_snapshot"')), left: current.tools.length });
    }
    const record = path.join(bin, 'record.jsonl');
    out.push({ chat: id, turns, fake: fs.existsSync(record) ? fs.readFileSync(record, 'utf8').split('\n').filter(Boolean) : [] });
  }
  await require(server('harness')).shutdownHarnesses();
  await new Promise((resolve) => setTimeout(resolve, 300));
  fs.writeFileSync(path.join(dir, 'out.json'), JSON.stringify(out));
  store.close();
  process.exit(0);
}

main().catch((error) => { console.error(error); process.exit(1); });
