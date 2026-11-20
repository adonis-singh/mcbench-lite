'use strict';

// Today's side of the replay: each call prepare.js wrote, run through
// server/agent.js executeToolCall as the turn loop runs it, with the real
// build sandbox, the engine for settles and test_build's engine verifier.
// Two things are fakes, the same fakes examples/replay.rs uses: bash's
// sandbox answers the call's record (sandbox.exec), and view_image's
// normaliser answers the file's own bytes (Pillow's step of
// sandbox.viewImage). One JSON line per call:
//
//   { key, content, screenshots, images, manifest, start, result, progress,
//     exec, threw }
//
//   MCBENCH_DATA_DIR=<clone> MCBENCH_TOOLS_DIR=<tools> node replay.js <calls.jsonl> <out.jsonl>

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const root = path.resolve(__dirname, '../../../../..');
const [callsFile, outFile] = process.argv.slice(2);
if (!callsFile || !outFile || !process.env.MCBENCH_DATA_DIR) throw new Error('usage: MCBENCH_DATA_DIR=<clone> node replay.js <calls.jsonl> <out.jsonl>');
// Today's sandbox must not reach Docker on a machine the owner uses (its
// idle reaper lists containers every minute): a docker command fails here as
// if Docker were not installed, which the reaper ignores.
const childProcess = require('child_process');
const spawn = childProcess.spawn;
childProcess.spawn = (command, ...rest) => {
  if (command === 'docker') throw Object.assign(new Error('spawn docker ENOENT'), { code: 'ENOENT' });
  return spawn(command, ...rest);
};
const server = (name) => require(path.join(root, 'server', name));
const { World } = server('world.js');
const sandbox = server('sandbox.js');
const agent = server('agent.js');
const { getBlockRegistry } = server('assets.js');

// The server's lines of runCall, from agent.js's own text (as shown.js).
const agentText = fs.readFileSync(path.join(root, 'server/agent.js'), 'utf8');
const between = (from, to) => {
  const start = agentText.indexOf(from);
  const end = agentText.indexOf(to, start);
  if (start < 0 || end < 0) throw new Error(`agent.js no longer has ${JSON.stringify(from)}`);
  return agentText.slice(start, end);
};
const serverStart = new Function('call', `${between('const DEFAULT_SHOTS = ', ';\n')};\n${between('const name = call.function && call.function.name;', '      emit({\n        type: \'tool_start\'')}\nreturn { summary, ${between('program: args && (', '\n').replace(/,$/, '')}, args, result };`);
const serverResult = new Function('name', 'result', 'shownImages', `return { ok: result.ok, cancelled: result.cancelled, error: result.error, diff: result.diff, ${between("output: name === 'bash'", '\n').replace(/,$/, '')}, images: (name === 'see_build' || name === 'view_image') && result.ok ? shownImages.map((img) => img.url) : undefined };`);

let current = null;
let exec = null;
sandbox.exec = async (conversationId, command, timeoutMs, { network }) => {
  exec = { command, timeoutMs: Math.min(Math.max(Number(timeoutMs) || sandbox.DEFAULT_TIMEOUT_MS, 1000), sandbox.MAX_TIMEOUT_MS), network };
  const answer = current.record.exec;
  if (answer.throw !== undefined) throw new Error(answer.throw);
  return { exitCode: 0, formatted: answer.formatted, ...(answer.storage ? { storage: answer.storage } : {}) };
};
sandbox.viewImage = async (conversationId, relPath) => {
  const file = sandbox.resolveWorkPath(conversationId, relPath);
  if (!fs.existsSync(file)) throw new Error(`No such file in /work: ${relPath}`);
  return fs.readFileSync(file);
};
// Removing /work files happens in the box today; the fake removes them on
// the host, as examples/replay.rs's does.
sandbox.removeWorkFiles = async (conversationId, relPaths) => {
  for (const relPath of relPaths) fs.rmSync(path.join(sandbox.workDir(conversationId), relPath), { force: true });
};
// prompt_library's file is the data folder's.

const manifestOf = (world) => JSON.stringify(world.serializeManifest((pairs) => crypto.createHash('sha256').update(JSON.stringify(pairs)).digest('hex').slice(0, 24)));

(async () => {
  const blockRegistry = await getBlockRegistry();
  const out = fs.openSync(outFile, 'w');
  const lines = fs.readFileSync(callsFile, 'utf8').split('\n').filter(Boolean);
  let done = 0;
  for (const line of lines) {
    current = JSON.parse(line);
    exec = null;
    const world = World.fromManifest(JSON.parse(current.before), (hash) => JSON.parse(fs.readFileSync(path.join(current.chunks, `${hash}.json`), 'utf8')));
    const conversation = { id: current.conversation, model: current.model, world };
    const call = { id: current.key, function: { name: current.name, arguments: current.arguments } };
    const start = serverStart(call);
    const progress = [];
    let threw = null;
    let executed = null;
    if (start.result) {
      executed = { result: start.result };
    } else {
      try {
        executed = await agent.executeToolCall(conversation, current.name, start.args, {
          blockRegistry, imageInput: current.imageInput, signal: new AbortController().signal, chat: current.chat,
          onProgress: (update) => progress.push(update.message),
        });
      } catch (error) {
        threw = String(error && error.message || error);
      }
    }
    const shown = executed && serverResult(current.name, executed.result, executed.screenshots?.images || executed.images || []);
    fs.writeSync(out, `${JSON.stringify({
      key: current.key,
      content: executed ? agent.toolMessageContent(current.name, executed.result) : null,
      screenshots: executed?.screenshots ? { text: executed.screenshots.text, images: executed.screenshots.images.map((image) => image.url) } : null,
      images: (executed?.images || []).map((image) => image.url),
      manifest: manifestOf(conversation.world),
      start: { summary: start.summary, program: start.program ?? null },
      result: shown && Object.fromEntries(Object.entries(shown).map(([key, value]) => [key, value ?? null])),
      progress,
      exec,
      threw,
    })}\n`);
    done += 1;
    if (done % 50 === 0) console.error(`replay: ${done} of ${lines.length}`);
  }
  fs.closeSync(out);
  console.error(`replay: ${done} calls`);
  process.exit(0);
})().catch((error) => { console.error(error); process.exit(1); });
