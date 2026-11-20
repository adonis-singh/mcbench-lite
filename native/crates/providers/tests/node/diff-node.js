'use strict';

// The Node side of mcbench-providers' differential test: runs every case of
// cases.json through today's server/llm.js against the replay server and
// writes what it saw.
//
//   node diff-node.js --server <checkout>/server --cases cases.json --out <dir>
//
// <dir>/requests.jsonl is the replay server's log of every request, and
// <dir>/results.jsonl has one line per case: the deltas in order and the
// assistant message (in the order agent.js stores it) or the error, with the
// three clock fields of the telemetry written as fixed values. Nothing leaves
// 127.0.0.1: every provider address is rewritten to the replay server.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const readline = require('node:readline');
const { normalizeTelemetry, storedMessage } = require('./shape');

const args = process.argv.slice(2);
const arg = (name) => { const at = args.indexOf(name); if (at < 0) throw new Error(`missing ${name}`); return args[at + 1]; };
const serverDir = path.resolve(arg('--server'));
const { keys, cases } = JSON.parse(fs.readFileSync(arg('--cases'), 'utf8'));
const out = path.resolve(arg('--out'));
fs.mkdirSync(out, { recursive: true });

// A scratch data folder, settings file and CODEX_HOME: never the developer's.
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-providers-diff-'));
const codexHome = path.join(scratch, 'codex');
fs.mkdirSync(codexHome);
Object.assign(process.env, {
  MCBENCH_DATA_DIR: path.join(scratch, 'data'),
  MCBENCH_SETTINGS_FILE: path.join(scratch, 'settings.json'),
  CODEX_HOME: codexHome,
  MCBENCH_CODEX_BINARY: path.join(__dirname, 'fake-codex-models.js'),
  OPENROUTER_API_KEY: keys.openrouter,
  OPENAI_API_KEY: keys.openai,
  ANTHROPIC_API_KEY: keys.anthropic,
  GROQ_API_KEY: keys.groq,
  GOOGLE_API_KEY: keys.google,
  QWENCLOUD_API_KEY: keys.qwencloud,
});

async function main() {
  const replay = spawn(process.execPath, [path.join(__dirname, 'replay.js'), '--log', path.join(out, 'requests.jsonl')], { stdio: ['ignore', 'pipe', 'inherit'] });
  const first = await new Promise((resolve) => readline.createInterface({ input: replay.stdout }).once('line', resolve));
  const origin = `http://127.0.0.1:${JSON.parse(first).port}`;
  const realFetch = global.fetch;
  // https://host/path -> http://127.0.0.1:port/host/path
  global.fetch = (input, init) => {
    const url = String(input instanceof Request ? input.url : input);
    const rewritten = url.replace(/^https:\/\//, `${origin}/`);
    if (rewritten === url) throw new Error(`a request that is not https: ${url}`);
    return realFetch(input instanceof Request ? new Request(rewritten, input) : rewritten, init);
  };
  const control = (pathname, body) => realFetch(`${origin}${pathname}`, { method: 'POST', body: JSON.stringify(body) }).then((res) => res.text());

  const llm = require(path.join(serverDir, 'llm'));
  const results = fs.openSync(path.join(out, 'results.jsonl'), 'w');
  for (const entry of cases) {
    if (entry.codexAuth !== null) fs.writeFileSync(path.join(codexHome, 'auth.json'), entry.codexAuth);
    const saved = {};
    for (const [name, value] of Object.entries(entry.env)) { saved[name] = process.env[name]; process.env[name] = value; }
    await control('/__case', { id: entry.id, routes: entry.routes });
    const record = { case: entry.id };
    try {
      if (entry.kind === 'complete') {
        const deltas = [];
        const controller = new AbortController();
        record.deltas = deltas;
        try {
          const message = await llm.chatCompletion({
            provider: entry.provider, model: entry.model, effort: entry.effort ?? undefined,
            conversationId: entry.conversationId ?? undefined, messages: entry.messages,
            tools: entry.tools.map((text) => JSON.parse(text)), signal: controller.signal,
            onDelta: (delta) => {
              deltas.push(delta);
              if (entry.cancelAfterDeltas !== null && deltas.length === entry.cancelAfterDeltas) controller.abort();
            },
          });
          record.message = storedMessage(message);
        } catch (error) {
          record.error = { message: error.message, reply: error.reply ?? null, telemetry: error.telemetry ? normalizeTelemetry(error.telemetry) : null };
        }
      } else if (entry.kind === 'listModels') {
        record.models = await llm.listModels(entry.provider);
      } else if (entry.kind === 'modelInfo') {
        record.info = await llm.modelInfo(entry.provider, entry.model);
      }
    } catch (error) {
      record.error = { message: error.message, reply: null, telemetry: null };
    }
    if (entry.provider === 'codex') record.authAfter = fs.existsSync(path.join(codexHome, 'auth.json')) ? fs.readFileSync(path.join(codexHome, 'auth.json'), 'utf8') : null;
    for (const [name, value] of Object.entries(saved)) { if (value === undefined) delete process.env[name]; else process.env[name] = value; }
    fs.writeSync(results, `${JSON.stringify(record)}\n`);
  }
  fs.closeSync(results);
  await control('/__end', {});
  await new Promise((resolve) => replay.on('exit', resolve));
  fs.rmSync(scratch, { recursive: true, force: true });
}

main().catch((error) => { console.error(error); process.exit(1); });
