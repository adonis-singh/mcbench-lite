'use strict';

// How runTurn (server/agent.js) leaves a turn: a stop at any step throws the
// abort for the caller to record, tools after a stop do nothing, and a tool
// whose own machinery fails still leaves a saved result for every call of
// its batch.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const Database = require('better-sqlite3');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-turn-lifecycle-'));
process.env.MCBENCH_DATA_DIR = dataDir;
// Builds settle on no engine here: Minecraft keeps the blueprint as placed.
const shotWorld = require('../server/shot-world');
shotWorld.currentTicks = () => 0;
shotWorld.settle = async (id, world, ticks) => new Map(ticks.map((tick) => [tick, {
  world, ticks: tick, changes: { removed: 0, added: 0, replaced: 0, examples: [] }, timings: {}, cached: false,
}]));
const llm = require('../server/llm');
let respond;
llm.chatCompletion = (options) => respond(options);
llm.modelInfo = async () => ({ imageInput: true });
const store = require('../server/store');
const sandbox = require('../server/sandbox');
const prompts = require('../server/prompts');
const { runTurn, _internal: { apiMessages } } = require('../server/agent');

after(() => { store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

const REGISTRY = { stone: [{}, {}] };
const call = (id, name, args) => ({ id, function: { name, arguments: JSON.stringify(args) } });

function chat(text) {
  const conversation = store.createConversation('openai', 'fixture');
  store.appendMessage(conversation, { role: 'user', content: text }, conversation.world.reference());
  store.save(conversation);
  return conversation;
}

function stored(conversation) {
  const db = new Database(store.DATABASE_FILE, { readonly: true });
  try {
    return db.prepare('SELECT payload_json FROM messages WHERE conversation_id = ? ORDER BY position').pluck().all(conversation.id).map((row) => JSON.parse(row));
  } finally {
    db.close();
  }
}

test('a stop during a tool throws the abort, and the calls after it do nothing', async (t) => {
  const conversation = chat('Build a hut');
  const controller = new AbortController();
  const library = prompts.list();
  respond = async () => ({ tool_calls: [
    call('wait', 'bash', { command: 'sleep 100' }),
    call('after', 'prompt_library', { save: [{ title: 'Added after the stop', text: 'x' }] }),
  ] });
  t.mock.method(sandbox, 'exec', (id, command, timeout, { signal }) => new Promise((resolve, reject) => {
    signal.addEventListener('abort', () => reject(new Error('killed')));
    controller.abort();
  }));
  await assert.rejects(runTurn(conversation, REGISTRY, () => {}, controller.signal, () => {}, { chat: true }), { name: 'AbortError' });
  const results = conversation.messages.filter((message) => message.role === 'tool').map((message) => [message.tool_call_id, JSON.parse(message.content).cancelled]);
  assert.deepEqual(results, [['wait', true], ['after', true]]);
  assert.deepEqual(prompts.list(), library, 'prompt_library does nothing once the turn is stopped');
  // The caller (server/index.js executeTurn) records the stop, flagged.
  assert.equal(conversation.messages.some((message) => message.content === '(stopped by user)'), false);
});

test('a tool whose machinery fails ends the turn; ending it answers every call of its batch', async (t) => {
  const conversation = chat('Build a hut and note it');
  respond = async () => ({ tool_calls: [
    call('build', 'build', { summary: 'Hut', program: 'module.exports = mc => { mc.set([0, 0, 0], "stone"); mc.writeFile("notes.txt", "hut"); };' }),
    call('later', 'bash', { command: 'cat notes.txt' }),
  ] });
  t.mock.method(sandbox, 'writeWorkFile', () => { throw new Error('the disk went away'); });
  await assert.rejects(runTurn(conversation, REGISTRY, () => {}, new AbortController().signal, () => {}), /the disk went away/);
  // What executeTurn (server/index.js) does with the error.
  store.endTurn(conversation, { role: 'error', content: 'the disk went away' });
  const results = stored(conversation).filter((message) => message.role === 'tool').map((message) => [message.tool_call_id, JSON.parse(message.content)]);
  assert.deepEqual(results.map(([id, result]) => [id, result.interrupted]), [['build', true], ['later', true]]);
  assert.match(results[0][1].error, /^This tool call did not finish \(the disk went away\)/);
  assert.equal(stored(conversation).at(-1).role, 'error');
  // The next request answers every call it sends.
  const sent = apiMessages(conversation);
  assert.deepEqual(sent.filter((message) => message.role === 'tool').map((message) => message.tool_call_id), ['build', 'later']);
});

test('a stopped turn answers the calls it left as cancelled', () => {
  const conversation = chat('Build a hut');
  store.appendMessage(conversation, { role: 'assistant', content: '', tool_calls: [call('done', 'bash', {}), call('left', 'bash', {})] });
  store.appendMessage(conversation, { role: 'tool', tool_call_id: 'done', content: 'ok' });
  store.endTurn(conversation, { role: 'assistant', content: '(stopped by user)', stopped: true });
  assert.deepEqual(stored(conversation).slice(2).map((message) => [message.role, message.tool_call_id, message.role === 'tool' && message.content !== 'ok' ? JSON.parse(message.content).cancelled : undefined]),
    [['tool', 'done', undefined], ['tool', 'left', true], ['assistant', undefined, undefined]]);
});
