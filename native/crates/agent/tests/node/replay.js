'use strict';

// The loop replayed through today's server: `cases` writes the replay cases
// from the stored chats of a data folder (and the synthetic ones), `run`
// runs each through server/index.js's chat route (handleChat, executeTurn,
// runTurn) with a replay model and replay tool bodies, and writes what it
// saw: each case's NDJSON event lines and the requests the model was given.
// The Rust side (tests/replay_diff.rs) runs the same cases through
// mcbench_agent's Turns on another clone; the two folders' rows are then
// compared with dump.js.
//
//   node -r det.js replay.js cases <cases.json>     (MCBENCH_DATA_DIR: a clone, read only)
//   node -r det.js replay.js run <cases.json> <out.json>   (MCBENCH_DATA_DIR: the clone it changes)
//
// The model (llm.chatCompletion) answers a case's steps in order: a stored
// assistant message (its deltas: the text, the reasoning, each call whole),
// a stored error (thrown with its telemetry), or a stop (the reply's text
// streamed, the turn stopped, the failed completion's telemetry thrown).
// Past the last step it answers '' (with PAST's telemetry). A title request (no tools) is answered
// "A title". The tool bodies (executeBuild, executeTestBuild,
// executeSeeBuild, executeBash, executeViewImage, promptLibrary.apply) are
// replaced by ones that give, in call order, the results the case stored;
// agent.js is otherwise today's: it is compiled from its own source with
// those five function names pointed at the replay, a substitution checked
// to happen exactly once each.
//
// With MCBENCH_REPLAY_TOOLS=real the tools are today's own bodies but two:
// build (the bwrap build sandbox and the engine children of
// MCBENCH_TOOLS_DIR), test_build (today's engine verifier on them),
// view_image (Pillow's normalising replaced by the file's own bytes, as
// mcbench-tools' recorded sandbox does) and the prompt library run for
// real; bash and see_build still answer the case's stored results. A case's
// tool entries are then only those of bash and see_build. Today's sandbox
// never reaches Docker: a docker command fails as if it were not
// installed.

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');

const W = process.env.MCBENCH_NODE_DIFF;
const server = (name) => path.join(W, 'server', name);
const [mode, casesFile, outFile] = process.argv.slice(2);

const sha = (text) => crypto.createHash('sha256').update(text).digest('hex');
const REAL = process.env.MCBENCH_REPLAY_TOOLS === 'real';
// With the real tools, the verifier's wall times (`metrics.*Ms`, and
// `reportBytes`, the size of a report holding them) are compared by
// presence: each such number is 0 in what is hashed (the Rust side does the
// same with the same pattern).
const WALL = /(Ms|reportBytes)((?:\\)*)":(\s*)-?[0-9][0-9.eE+-]*/g;
const wallFree = (text) => (REAL ? text.replace(WALL, '$1$2":$30') : text);
const STUBBED = REAL ? ['see_build', 'bash'] : ['build', 'test_build', 'see_build', 'bash', 'view_image', 'prompt_library'];
if (REAL) {
  const childProcess = require('node:child_process');
  const spawn = childProcess.spawn;
  childProcess.spawn = (command, ...rest) => {
    if (command === 'docker') throw Object.assign(new Error('spawn docker ENOENT'), { code: 'ENOENT' });
    return spawn(command, ...rest);
  };
}

// ----- cases -----

const TELEMETRY_KEYS = ['provider', 'model', 'effort', 'startedAt', 'wallMs', 'firstDeltaMs', 'contextBytes', 'messageCount', 'usage'];

// A stored completion's telemetry as a provider gives one today: a stored
// row the v13 migration left without contextTokens gets null.
function telemetryOf(t) {
  if (!t) return null;
  if (t.failed) return t;
  const out = {};
  for (const key of TELEMETRY_KEYS) out[key] = t[key];
  out.contextTokens = t.contextTokens ?? null;
  return out;
}

function reply(message) {
  const out = { content: message.content };
  for (const key of ['reasoning', 'reasoning_details', 'finish_reason']) if (message[key] !== undefined) out[key] = message[key];
  out.telemetry = telemetryOf(message.telemetry);
  for (const key of ['notices', 'plan', 'rateLimit', 'tool_calls']) if (message[key] !== undefined) out[key] = message[key];
  return out;
}

// Whether a call reaches a tool body (executeToolCall), given its arguments.
function reaches(call, imageInput, chat) {
  let args;
  try { args = JSON.parse(call.function.arguments || '{}'); } catch { return false; }
  if (!args || typeof args !== 'object' || Array.isArray(args)) return false;
  const name = call.function.name;
  if (name === 'prompt_library') return chat;
  if (['build', 'test_build', 'bash'].includes(name)) return true;
  if (['see_build', 'view_image'].includes(name)) return imageInput;
  return false;
}

function storedResult(name, message) {
  if (name === 'bash' && message.telemetry?.ok === true) {
    try { const parsed = JSON.parse(message.content); if (parsed && typeof parsed === 'object' && 'ok' in parsed) return parsed; } catch { /* the transcript */ }
    return { ok: true, text: message.content };
  }
  return JSON.parse(message.content);
}

// The steps and tool results of the turn that follows prompt `p`.
function caseOf(chat, messages, p, imageInput) {
  const steps = [];
  const tools = [];
  let i = p + 1;
  while (i < messages.length && !(messages[i].role === 'user' && !messages[i].seeBuild)) {
    const message = messages[i];
    if (message.role === 'error') {
      steps.push({ kind: 'error', message: message.content, telemetry: telemetryOf(message.telemetry) });
      i += 1;
      continue;
    }
    if (message.role !== 'assistant') { i += 1; continue; }
    if (message.stopped) {
      steps.push({ kind: 'stop', reply: message.reply ?? '', telemetry: message.telemetry ?? null });
      i += 1;
      continue;
    }
    if (!message.telemetry) return null;
    steps.push({ kind: 'message', message: reply(message) });
    const calls = message.tool_calls || [];
    let j = i + 1;
    const results = new Map();
    while (j < messages.length && messages[j].role === 'tool') { results.set(messages[j].tool_call_id, messages[j]); j += 1; }
    const shots = [];
    while (j < messages.length && messages[j].seeBuild) { shots.push(messages[j]); j += 1; }
    for (const call of calls) {
      const stored = results.get(call.id);
      const name = call.function.name;
      if (!reaches(call, imageInput, true)) continue;
      if (!stored) return null;
      let result;
      try { result = storedResult(name, stored); } catch { return null; }
      const entry = { name, result };
      if (name === 'see_build' && result.ok) {
        const attached = typeof result.note === 'string' && result.note.includes('attached in the next message');
        if (attached) {
          const shot = shots.shift();
          if (!shot) return null;
          entry.screenshots = { text: shot.content[0].text, images: shot.content.slice(1).map((part) => part.image_url.url) };
        } else {
          entry.images = (result.shots || []).map((s) => `/api/images/${chat}/${path.basename(s.image)}`);
        }
      }
      if (name === 'view_image' && result.ok) {
        const shot = shots.shift();
        if (!shot) return null;
        entry.screenshots = { text: shot.content[0].text, images: shot.content.slice(1).map((part) => part.image_url.url) };
      }
      if (name === 'test_build') entry.progress = true;
      tools.push(entry);
    }
    i = j;
  }
  return steps.length ? { steps, tools } : null;
}

const call = (id, name, args) => ({ id, type: 'function', function: { name, arguments: typeof args === 'string' ? args : JSON.stringify(args) } });
const tele = (extra = {}) => ({ provider: 'openrouter', model: 'replay/model', effort: null, startedAt: '2026-11-17T00:00:00.000Z', wallMs: 10, firstDeltaMs: 5, contextBytes: 100, messageCount: 2, usage: { inputTokens: 10, outputTokens: 5, cachedInputTokens: 0, cacheWriteTokens: 0, reasoningTokens: null }, ...extra });
const said = (content, extra = {}) => ({ kind: 'message', message: { content, finish_reason: 'stop', telemetry: tele({ contextTokens: 10 }), ...extra } });
const calling = (content, calls, extra = {}) => ({ kind: 'message', message: { content, finish_reason: 'tool_calls', telemetry: tele({ contextTokens: 10 }), ...extra, tool_calls: calls } });

function synthetic(image) {
  const failed = tele({ usage: null, failed: true, cancelled: false });
  return [
    { label: 'arguments that are not JSON, not an object, or empty', imageInput: true, steps: [
      calling('Trying.', [call('s1a', 'build', '{bad'), call('s1b', 'build', '[1,2]'), call('s1c', 'bash', '5'), call('s1d', 'bash', '"x"'), call('s1e', 'bash', 'null'), call('s1f', 'bash', '0'), call('s1g', 'build', '')]),
      said('Done.'),
    ], tools: [{ name: 'build', result: { ok: false, error: 'build tool called with an empty program' } }] },
    { label: 'an unknown tool and a vision tool for a text-only model', imageInput: false, steps: [
      calling('', [call('s2a', 'teleport', {}), call('s2b', 'see_build', {}), call('s2c', 'view_image', { path: 'a.png' })]),
      said('Done.'),
    ], tools: [] },
    { label: 'the prompt library in a chat, read and refused', imageInput: true, steps: [
      calling('', [call('s3a', 'prompt_library', {}), call('s3b', 'prompt_library', { save: [{ title: 'x' }] })]),
      said('Done.'),
    ], tools: [{ name: 'prompt_library', result: { ok: true, prompts: [{ id: 'p', title: 'P', text: 't', tags: [], favorite: false, attachments: [] }] } }, { name: 'prompt_library', error: 'A prompt needs a title and text' }] },
    { label: 'a provider error with telemetry', imageInput: true, steps: [
      { kind: 'error', message: 'LLM request failed (HTTP 400): bad request', telemetry: failed },
    ], tools: [] },
    { label: 'a transient error, retried', imageInput: true, steps: [
      { kind: 'error', message: 'LLM request failed (HTTP 503): unavailable', telemetry: failed },
      said('Recovered.'),
    ], tools: [] },
    { label: 'a stop while the reply streams', imageInput: true, steps: [
      calling('Building.', [call('s6a', 'bash', { command: 'ls\nmore' })]),
      { kind: 'stop', reply: 'Half a repl', telemetry: tele({ usage: null, failed: true, cancelled: true }) },
    ], tools: [{ name: 'bash', result: { ok: true, text: '$ ls\nfile\n' } }] },
    { label: 'a batch of every tool with screenshots and progress', imageInput: true, steps: [
      calling('All at once.', [
        call('s7a', 'bash', { command: 'echo hi' }),
        call('s7b', 'test_build', { tests: [{ ticks: 1 }, { ticks: 2 }] }),
        call('s7c', 'see_build', { shots: [{ view: 'top' }, { eye: [1, 2, 3], target: [0, 0, 0] }, null] }),
        call('s7d', 'view_image', { path: 'images/x.png' }),
        call('s7e', 'see_build', { attach: false }),
        call('s7f', 'build', { summary: 'Floor', program: 'module.exports = mc => {}' }),
      ], { reasoning: 'Thinking it over.', reasoning_details: [{ type: 'thinking', thinking: 't', signature: 's' }], notices: [{ level: 'info', text: 'A notice' }], plan: [{ text: 'Step', status: 'active' }], rateLimit: { status: 'allowed' } }),
      said('Done.', { reasoning: 'Done thinking.' }),
    ], tools: [
      { name: 'bash', result: { ok: true, text: 'hi\n' } },
      { name: 'test_build', result: { ok: true, tests: [{ ok: true, report: '/work/verification/a.json' }] }, progress: true },
      { name: 'see_build', result: { ok: true, worldRevision: 1, minecraft: [], shots: [], note: '2 screenshot(s) attached in the next message, in shot order' }, screenshots: { text: '[see_build] Screenshots of the current build:', images: [image, image] } },
      { name: 'view_image', result: { ok: true, note: 'image attached in the next message' }, screenshots: { text: '[view_image] /work/images/x.png:', images: [image] } },
      { name: 'see_build', result: { ok: true, worldRevision: 1, minecraft: [], shots: [], note: 'Shots saved under /work/images only (attach: false)' }, images: [image] },
      { name: 'build', result: { ok: true, worldRevision: 2, changed: true, diff: { added: 1, removed: 0, changed: 0 }, totalBlocks: 1, bounds: null, logs: [] } },
    ] },
    { label: 'a call with Gemini\'s thought signature', imageInput: true, steps: [
      calling('', [{ ...call('s8a', 'bash', { command: 'pwd' }), extra_content: { google: { thought_signature: 'sig' } } }]),
      said(''),
    ], tools: [{ name: 'bash', result: { ok: false, error: 'Docker is not running' } }] },
  ].map((c) => ({ chat: null, provider: 'openrouter', model: 'replay/model', effort: '', resolution: 1, text: `Synthetic: ${c.label}`, ...c }));
}

function writeCases() {
  const store = require(server('store'));
  const cases = [];
  let image = null;
  const ids = store.listConversationIds().filter((c) => !c.archived).map((c) => c.id).sort();
  for (const id of ids) {
    const conversation = store.getConversation(id);
    const messages = conversation.messages;
    image ??= messages.flatMap((m) => (Array.isArray(m.content) ? m.content : [])).find((part) => part.type === 'image_url')?.image_url.url;
    const prompts = messages.map((m, i) => [m, i]).filter(([m]) => m.role === 'user' && !m.seeBuild && typeof m.content === 'string').map(([, i]) => i);
    for (const p of [...new Set([prompts[0], prompts.at(-1)])].filter((p) => p !== undefined)) {
      if (!conversation.checkpoints.has(messages[p].id)) continue;
      const imageInput = cases.length % 3 !== 2;
      const made = caseOf(id, messages, p, imageInput);
      if (!made) continue;
      cases.push({ chat: id, prompt: p, text: messages[p].content, provider: conversation.provider, model: conversation.model, effort: conversation.effort || '', resolution: conversation.resolution, imageInput, ...made });
    }
  }
  cases.push(...synthetic(image));
  fs.writeFileSync(casesFile, JSON.stringify(cases));
  store.close();
}

// ----- run -----

// What the model answers past a case's last step (a retried error's next
// attempt): an empty answer with this telemetry.
const PAST = { provider: 'openrouter', model: '', effort: null, startedAt: '', wallMs: 0, firstDeltaMs: null, contextBytes: 0, messageCount: 0, usage: null, contextTokens: null };

let current = null; // the case running: { steps, tools, requests, stop, id }
let app = null; // server/index.js, once loaded

function nextTool(name) {
  const entry = current.tools.shift();
  if (!entry || entry.name !== name) throw new Error(`replay: ${name} was called where the case has ${entry ? entry.name : 'no call'}`);
  return entry;
}

const replay = {
  build: async () => nextTool('build').result,
  test_build: async (conversation, args, signal, verifier, onUpdate = () => {}) => {
    const entry = nextTool('test_build');
    if (entry.progress) onUpdate({ phase: 'replay', message: 'Test 1: replayed' });
    return entry.result;
  },
  see_build: async () => {
    const entry = nextTool('see_build');
    if (!entry.result.ok) return entry.result;
    const { note, ...rest } = entry.result;
    const urls = entry.screenshots ? entry.screenshots.images : entry.images;
    return { ...rest, images: urls.map((url) => ({ url })), attach: Boolean(entry.screenshots) };
  },
  bash: async () => {
    const entry = nextTool('bash');
    // A stop from inside the call: the same point on both sides.
    if (entry.stop) current.stop();
    return entry.result;
  },
  view_image: async (conversation, args) => {
    const entry = nextTool('view_image');
    if (!entry.result.ok) return entry.result;
    return { ok: true, images: entry.screenshots.images.map((url) => ({ angle: String(args.path), url })) };
  },
};

function loadAgent() {
  const file = server('agent.js');
  let source = fs.readFileSync(file, 'utf8');
  for (const [fn, key] of [['executeBuild', 'build'], ['executeTestBuild', 'test_build'], ['executeSeeBuild', 'see_build'], ['executeBash', 'bash'], ['executeViewImage', 'view_image']].filter(([, key]) => STUBBED.includes(key))) {
    const head = `async function ${fn}(`;
    if (source.split(head).length !== 2) throw new Error(`replay: agent.js has not exactly one ${head}`);
    source = source.replace(head, `async function ${fn}(...args) { return globalThis.__replay[${JSON.stringify(key)}](...args); }\nasync function ${fn}Replaced(`);
  }
  globalThis.__replay = replay;
  const agent = new Module(file, module);
  agent.filename = file;
  agent.paths = Module._nodeModulePaths(path.dirname(file));
  agent._compile(source, file);
  agent.loaded = true;
  require.cache[file] = agent;
}

function deltas(onDelta, message) {
  if (message.content) onDelta({ content: message.content });
  if (message.reasoning) onDelta({ reasoning: message.reasoning });
  (message.tool_calls || []).forEach((c, index) => onDelta({ toolCall: { index, id: c.id, name: c.function.name, args: c.function.arguments } }));
}

async function completion(options) {
  current.requests.push({
    conversationId: options.conversationId ?? null, model: options.model, effort: options.effort ?? null, chat: options.chat ?? false,
    tools: sha(JSON.stringify(options.tools)), messages: sha(wallFree(JSON.stringify(options.messages))), bytes: Buffer.byteLength(wallFree(JSON.stringify(options.messages))),
  });
  // A title is answered once its chat's turn has ended, on both sides, so
  // that it is stored (and indexed for search) at one point of the turn.
  if (!options.tools.length) {
    const id = current.id;
    for (let hold; (hold = app.reservations.get(id));) await hold.done;
    return { content: 'A title' };
  }
  const step = current.steps.shift();
  if (!step) return { content: '', telemetry: PAST };
  if (step.kind === 'message') {
    deltas(options.onDelta, step.message);
    return structuredClone(step.message);
  }
  if (step.kind === 'error') throw Object.assign(new Error(step.message), step.telemetry ? { telemetry: step.telemetry } : {});
  if (step.reply) options.onDelta({ content: step.reply });
  current.stop();
  throw Object.assign(new Error('This operation was aborted'), step.telemetry ? { telemetry: step.telemetry } : {}, step.reply ? { reply: step.reply } : {});
}

async function run() {
  const cases = JSON.parse(fs.readFileSync(casesFile, 'utf8'));
  const llm = require(server('llm'));
  llm.chatCompletion = completion;
  llm.modelInfo = async () => ({ imageInput: current.imageInput });
  llm.closeHarnessSession = () => {};
  if (!REAL) require(server('assets')).getBlockRegistry = async () => ({});
  const sandbox = require(server('sandbox'));
  sandbox.reset = async () => {};
  sandbox.syncAttachments = () => {};
  sandbox.shutdown = async () => {};
  if (REAL) {
    sandbox.viewImage = async (conversationId, relPath) => {
      const file = sandbox.resolveWorkPath(conversationId, relPath);
      if (!fs.existsSync(file)) throw new Error(`No such file in /work: ${relPath}`);
      return fs.readFileSync(file);
    };
    sandbox.removeWorkFiles = async (conversationId, relPaths) => {
      for (const relPath of relPaths) fs.rmSync(path.join(sandbox.workDir(conversationId), relPath), { force: true });
    };
  } else {
    const prompts = require(server('prompts'));
    prompts.apply = () => {
      const entry = nextTool('prompt_library');
      if (entry.error) throw new Error(entry.error);
      return entry.result.prompts;
    };
  }
  loadAgent();
  const store = require(server('store'));
  app = require(server('index'));
  const request = (method, pathname, body) => {
    const req = Readable.from(body === undefined ? [] : [Buffer.from(JSON.stringify(body))]);
    Object.assign(req, { method, url: pathname, headers: {} });
    const res = new EventEmitter();
    res.lines = [];
    res.writeHead = (status) => { res.status = status; };
    res.write = (line) => { res.lines.push(line.replace(/\n$/, '')); return true; };
    res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
    return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => { res.status = error.statusCode || 500; res.body = { error: error.message }; }).then(() => res);
  };
  const out = [];
  for (const c of cases) {
    current = { ...structuredClone(c), requests: [] };
    current.tools = current.tools.filter((entry) => STUBBED.includes(entry.name));
    let id;
    if (c.chat) {
      const source = store.getConversation(c.chat);
      id = crypto.randomUUID();
      store.forkConversation(source, c.prompt, source.checkpoints.get(source.messages[c.prompt].id), id);
    } else {
      id = (await request('POST', '/api/conversations', { provider: c.provider, model: c.model })).body.id;
    }
    current.stop = () => app.reservations.turn(id).controller.abort();
    current.id = id;
    const res = await request('POST', `/api/conversations/${id}/chat`, { provider: c.provider, model: c.model, effort: c.effort, resolution: c.resolution, text: c.text });
    for (let hold; (hold = app.reservations.get(id));) await hold.done;
    for (let i = 0; i < 20; i += 1) await new Promise(setImmediate);
    out.push({ chat: id, status: res.status ?? 200, error: res.body?.error ?? null, events: res.lines.filter((line) => !line.startsWith('{"type":"turn_snapshot"')), requests: current.requests, left: { steps: current.steps.length, tools: current.tools.length } });
  }
  fs.writeFileSync(outFile, JSON.stringify(out));
  store.close();
  process.exit(0);
}

// tests/node/harness.js uses the replay tool bodies.
module.exports = { loadAgent, nextTool, use: (c) => { current = c; } };

if (require.main === module) {
  if (mode === 'cases') writeCases();
  else if (mode === 'run') run().catch((error) => { console.error(error); process.exit(1); });
  else throw new Error(`replay.js: unknown mode ${mode}`);
}
