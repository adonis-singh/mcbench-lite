'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-agent-screenshots-'));
process.env.MCBENCH_DATA_DIR = dataDir;
require('./helpers/fake-engines').installFakeEngines();
// The page that draws shots is not under test.
require('../server/shot').renderShots = async (conversationId, shots) => shots.map(() => Buffer.from('jpeg'));
const store = require('../server/store');
const sandbox = require('../server/sandbox');
const llm = require('../server/llm');
let respond;
llm.chatCompletion = options => respond(options);
llm.modelInfo = async () => ({ imageInput: true });
const { runTurn } = require('../server/agent');

after(async () => { await require('../server/shot-world').stopShotEngines(); store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

test('a call that throws leaves the screenshots earlier results promised, after a result for every call and before the error', async () => {
  const conversation = store.createConversation('openai', 'fixture');
  store.appendMessage(conversation, { role: 'user', content: 'go' });
  const call = (id, name, args) => ({ id, type: 'function', function: { name, arguments: JSON.stringify(args) } });
  respond = async () => ({ tool_calls: [
    call('place', 'build', { summary: 'floor', program: 'module.exports = mc => mc.set([0,0,0],"stone")' }),
    call('look', 'see_build', {}),
    call('write', 'build', { summary: 'notes', program: 'module.exports = mc => mc.writeFile("notes.txt", "x")' }),
    call('later', 'bash', { command: 'ls' }),
  ] });
  const events = [];
  const writeWorkFile = sandbox.writeWorkFile;
  sandbox.writeWorkFile = () => { throw new Error('disk failed'); };
  try {
    await assert.rejects(runTurn(conversation, { stone: [{}, {}] }, event => events.push(event), new AbortController().signal, () => {}), /disk failed/);
  } finally {
    sandbox.writeWorkFile = writeWorkFile;
  }
  // As executeTurn ends a failed turn (server/index.js).
  store.endTurn(store.getConversation(conversation.id), { role: 'error', content: 'disk failed' });

  const stored = conversation.messages.slice(1);
  assert.equal(conversation.unsaved.size, 0);
  assert.deepEqual(stored.map(m => m.seeBuild ? 'screenshots' : m.role), ['assistant', 'tool', 'tool', 'tool', 'tool', 'screenshots', 'error']);
  assert.deepEqual(stored.slice(1, 5).map(m => m.tool_call_id), ['place', 'look', 'write', 'later']);
  const looked = JSON.parse(stored[2].content);
  assert.equal(looked.note, '2 screenshot(s) attached in the next message, in shot order');
  const shown = events.find(event => event.type === 'tool_result' && event.callId === 'look').images;
  assert.equal(shown.length, 2);
  assert.deepEqual(stored[5].content, [
    { type: 'text', text: '[see_build] Screenshots of the current build:' },
    ...shown.map(url => ({ type: 'image_url', image_url: { url } })),
  ]);
  for (const unanswered of stored.slice(3, 5)) {
    assert.deepEqual(JSON.parse(unanswered.content), { ok: false, interrupted: true, error: 'This tool call did not finish (disk failed). Its side effects are unknown; inspect the build or workspace before retrying a change.' });
  }
  assert.equal(stored[6].content, 'disk failed');
});
