'use strict';

// The calls the replay runs through today's executeToolCall (replay.js) and
// through mcbench-tools (examples/replay.rs): every stored call of a data
// folder of the tools this crate ports (build, test_build, view_image,
// bash) and the generated calls of generated.js, one JSON line each:
//
//   { key, source: 'stored'|'generated', conversation, model, chat,
//     imageInput, name, arguments, before: manifest text, chunks: folder the
//     before world's chunk files are read from, stored: stored content,
//     record: { exec, callTests, runs } }
//
// `record` is what the fakes answer: bash's sandbox run (the stored
// transcript as `formatted`, today's formatResult text being the sandbox
// crate's to prove), and test_build's experiments (each stored test's
// report, or its failure). Generated calls also write their /work files
// and chunk files into the data folder given, which both sides then clone.
//
//   MCBENCH_DATA_DIR=<scratch> node prepare.js <data folder> <out.jsonl>
//
// The output holds real chat text: it stays in the scratch folder.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const Database = require('better-sqlite3');

const root = path.resolve(__dirname, '../../../../..');
const [dataDir, outFile] = process.argv.slice(2);
if (!dataDir || !outFile) throw new Error('usage: node prepare.js <data folder> <out.jsonl>');
const { World } = require(path.join(root, 'server/world.js'));
const { callTests } = require(path.join(root, 'server/minecraft-verify.js'));
const generated = require('./generated.js');

const PORTED = new Set(['build', 'test_build', 'view_image', 'bash']);
const out = fs.openSync(outFile, 'w');
const write = (value) => fs.writeSync(out, `${JSON.stringify(value)}\n`);

// test_build's records: the call's tests as callTests reads them, and each
// experiment's report as stored (its `report` path left out: the tool adds
// it), or its failure.
function verifyRecord(args, stored) {
  let tests;
  try {
    tests = { tests: callTests(args) };
  } catch (error) {
    tests = { error: { message: error.message, ...(error.budget ? { budget: error.budget, code: error.code } : {}) } };
  }
  let content = null;
  try { content = JSON.parse(stored); } catch { /* not JSON */ }
  const runs = (content?.tests || []).map((test) => {
    if (!test.ok) return { error: { message: test.error, ...(test.budget ? { budget: test.budget, code: test.code } : {}) } };
    const { report, ...rest } = test;
    return { report: rest };
  });
  return { callTests: tests, runs };
}

// bash's record: what sandbox.exec answered for the stored result.
function execRecord(stored) {
  let content;
  try { content = JSON.parse(stored); } catch { return { formatted: stored, storage: null }; }
  if (content.ok) return { formatted: content.text, storage: null };
  if (typeof content.output === 'string') return { formatted: content.output, storage: content.error };
  return { throw: content.error };
}

const db = new Database(path.join(dataDir, 'mcbench.sqlite3'), { readonly: true, fileMustExist: true });
const conversations = new Map(db.prepare('SELECT id, model FROM conversations').all().map((row) => [row.id, row]));
const rows = db.prepare('SELECT id, conversation_id, parent_id, position, payload_json, world_manifest_json FROM messages ORDER BY conversation_id, position').all();
const byId = new Map(rows.map((row) => [row.id, row]));
const calls = new Map();
const counts = { stored: 0, noWorld: 0, generated: 0 };
for (const row of rows) {
  const msg = JSON.parse(row.payload_json);
  if (msg.role === 'assistant') for (const call of msg.tool_calls || []) calls.set(call.id, call);
  if (msg.role !== 'tool') continue;
  const call = calls.get(msg.tool_call_id);
  if (!call || !PORTED.has(call.function.name)) continue;
  const parent = byId.get(row.parent_id);
  if (!parent?.world_manifest_json) { counts.noWorld += 1; continue; }
  let args = null;
  try { args = JSON.parse(call.function.arguments || '{}'); } catch { /* the loop refuses it */ }
  const name = call.function.name;
  write({
    key: `${row.conversation_id}/${call.id}`,
    source: 'stored',
    conversation: row.conversation_id,
    model: conversations.get(row.conversation_id)?.model ?? '',
    chat: false,
    imageInput: true,
    name,
    arguments: call.function.arguments,
    before: parent.world_manifest_json,
    chunks: path.join(path.resolve(dataDir), 'worlds', row.conversation_id, 'chunks'),
    stored: msg.content,
    record: name === 'bash' ? { exec: execRecord(msg.content) } : name === 'test_build' && args ? verifyRecord(args, msg.content) : {},
  });
  counts.stored += 1;
}

// Generated calls: their worlds' chunk files in a folder of their own (the
// stores remove chunk files no stored world names), their /work files in
// the data folder's sandbox.
const genChunks = path.join(path.dirname(path.resolve(outFile)), 'generated-chunks');
fs.mkdirSync(genChunks, { recursive: true });
const writeChunk = (pairs) => {
  const payload = JSON.stringify(pairs);
  const hash = crypto.createHash('sha256').update(payload).digest('hex').slice(0, 24);
  fs.writeFileSync(path.join(genChunks, `${hash}.json`), payload);
  return hash;
};
const manifestOf = (world) => JSON.stringify(world.serializeManifest(writeChunk));
for (const call of generated.calls({ World, dataDir: path.resolve(dataDir) })) {
  const name = call.name;
  let args = null;
  try { args = JSON.parse(call.arguments || '{}'); } catch { /* refused */ }
  write({
    key: `generated/${call.label}`,
    source: 'generated',
    conversation: call.conversation,
    model: call.model ?? 'generated-model',
    chat: call.chat ?? false,
    imageInput: call.imageInput ?? true,
    name,
    arguments: call.arguments,
    before: manifestOf(call.world ?? new World()),
    chunks: genChunks,
    stored: null,
    record: call.record ?? (name === 'test_build' && args ? { ...verifyRecord(args, 'null'), runs: call.runs ?? [] } : {}),
  });
  counts.generated += 1;
}
fs.closeSync(out);
console.log(JSON.stringify(counts));
