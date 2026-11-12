'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-agent-retry-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const store = require('../server/store');
const llm = require('../server/llm');
let respond;
llm.chatCompletion = options => respond(options);
llm.modelInfo = async () => ({ imageInput: true });
const { runTurn } = require('../server/agent');

after(() => { store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

// A completion that fails as an overloaded API does once, then answers.
function overloadedOnce() {
  const calls = [];
  respond = async (options) => {
    calls.push(options.provider);
    if (calls.length === 1) throw new Error('Claude Code success: API Error: 529 {"type":"error","error":{"type":"overloaded_error"}}');
    return { content: 'Done.' };
  };
  return calls;
}

test('a harness provider\'s completion runs once: its CLI already retried, and a second attempt would replay the turn in a new session', async () => {
  for (const provider of ['claude-code', 'codex-cli', 'cursor']) {
    const conversation = store.createConversation(provider, 'fixture');
    store.appendMessage(conversation, { role: 'user', content: 'go' });
    const calls = overloadedOnce();
    const events = [];
    await assert.rejects(runTurn(conversation, {}, event => events.push(event), new AbortController().signal, () => {}), /overloaded_error/);
    assert.deepEqual(calls, [provider]);
    assert.equal(events.some(event => event.type === 'completion_retry'), false);
  }
});

test('a model API provider\'s completion is retried after a transient error', async () => {
  const conversation = store.createConversation('anthropic', 'fixture');
  store.appendMessage(conversation, { role: 'user', content: 'go' });
  const calls = overloadedOnce();
  const events = [];
  await runTurn(conversation, {}, event => events.push(event), new AbortController().signal, () => {});
  assert.deepEqual(calls, ['anthropic', 'anthropic']);
  assert.equal(events.filter(event => event.type === 'completion_retry').length, 1);
  assert.equal(conversation.messages.at(-1).content, 'Done.');
});
