'use strict';

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-context-'));
process.env.MCBENCH_DATA_DIR = root;
const { contextMessages } = require('../server/agent-context');
const sandbox = require('../server/sandbox');
after(() => fs.rmSync(root, { recursive: true, force: true }));

test('context offloads whole old exchanges, retains user instructions and recent signed reasoning, and exposes lossless archives', () => {
  const conversation = { id: 'fixture', messages: [{ role: 'user', content: 'Keep the original red border.' }] };
  for (let i = 0; i < 12; i++) {
    if (i === 4) conversation.messages.push({ role: 'user', content: 'Make the floor blue.' });
    conversation.messages.push({ role: 'assistant', content: `Step ${i}`, reasoning_details: [{ type: 'thinking', thinking: 'fixture', signature: `signed-${i}` }], tool_calls: [
      { id: `t${i}`, function: { name: 'build', arguments: JSON.stringify({ program: 'x'.repeat(5000) }) } },
    ] }, { role: 'tool', tool_call_id: `t${i}`, content: JSON.stringify({ ok: true, worldRevision: i, logs: ['z'.repeat(5000)] }) });
  }
  const before = JSON.stringify(conversation.messages);
  const result = contextMessages(conversation, { maxBytes: 45_000, keepExchanges: 3 });
  assert.ok(Buffer.byteLength(JSON.stringify(result)) <= 45_000);
  assert.deepEqual(result.filter(m => m.role === 'user'), conversation.messages.filter(m => m.role === 'user'));
  const plain = (m) => { const { reasoning_details, ...rest } = m; return rest; };
  assert.deepEqual(result.slice(-6).map(plain), conversation.messages.slice(-6).map(plain));
  const stub = result.find(m => m.content.includes?.('Earlier tool exchange archived'));
  const file = stub.content.match(/\/work\/context\/[a-f0-9]+\.json/)[0];
  assert.deepEqual(JSON.parse(sandbox.makeReadFile('fixture')(file)), conversation.messages.slice(1, 3));
  assert.equal(JSON.stringify(conversation.messages), before);
  assert.deepEqual(contextMessages(conversation, { maxBytes: 45_000, keepExchanges: 3 }), result, 'stable archives preserve the replay prefix');
  for (const message of result.filter(m => m.role === 'tool')) {
    assert.ok(result.some(m => m.tool_calls?.some(c => c.id === message.tool_call_id)));
  }
});

// What each request of a turn sent, replayed as the turn grew.
function requestsOf(messages, options) {
  const sent = [];
  for (let i = 1; i <= messages.length; i++) {
    if (i < messages.length && messages[i].role !== 'assistant') continue;
    sent.push(contextMessages({ id: 'grow', messages: messages.slice(0, i) }, options));
  }
  return sent;
}

test('what a request sends only grows until an archiving, which drops the thinking it would invalidate', () => {
  const messages = [{ role: 'user', content: 'Build a castle.' }];
  for (let i = 0; i < 30; i++) {
    messages.push({ role: 'assistant', content: `Step ${i}`, reasoning_details: [{ type: 'thinking', thinking: 'fixture', signature: `signed-${i}` }], tool_calls: [
      { id: `t${i}`, function: { name: 'see_build', arguments: JSON.stringify({ program: 'x'.repeat(3000) }) } },
    ] }, { role: 'tool', tool_call_id: `t${i}`, content: JSON.stringify({ ok: true, worldRevision: i }) },
    { role: 'user', seeBuild: true, content: [{ type: 'text', text: `shot ${i}` }, { type: 'image_url', image_url: { url: `/api/images/shot-${i}.jpg` } }] });
  }
  const options = { maxBytes: 60_000, keepExchanges: 3 };
  const sent = requestsOf(messages, options);
  let archivings = 0;
  for (let n = 1; n < sent.length; n++) {
    const [before, now] = [sent[n - 1], sent[n]];
    const grew = before.every((message, i) => JSON.stringify(message) === JSON.stringify(now[i]));
    if (!grew) {
      archivings += 1;
      // An archiving keeps no thinking from before it.
      assert.ok(now.every(m => !m.reasoning_details));
      continue;
    }
    // Between archivings every signed block is replayed exactly as made.
    assert.equal(now.filter(m => m.reasoning_details).length, before.filter(m => m.reasoning_details).length + 1);
  }
  // Each archiving frees half the budget, so it comes every few requests, not every one.
  assert.ok(archivings >= 2 && archivings <= sent.length / 3, `archivings: ${archivings}`);
  // The last request is the same history contextMessages gives on its own.
  assert.deepEqual(sent.at(-1), contextMessages({ id: 'grow', messages }, options));
});

test('unmatched tool calls and protected user requirements are never silently trimmed', () => {
  const conversation = { id: 'fixture', messages: [{ role: 'user', content: 'important '.repeat(4000) }] };
  assert.throws(() => contextMessages(conversation, { maxBytes: 16_000 }), /Protected user messages/);
  assert.equal(contextMessages(conversation, { maxBytes: 100_000 })[0], conversation.messages[0]);
});
