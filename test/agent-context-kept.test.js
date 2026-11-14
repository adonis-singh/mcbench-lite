'use strict';

// The requests a long chat sends are the same bytes whether the history's
// sizes and archived exchanges are worked out afresh at every request (as
// the code before they were kept did, copied below) or kept between
// requests, and the archive files written are the same.

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-context-kept-'));
process.env.MCBENCH_DATA_DIR = root;
const { contextMessages } = require('../server/agent-context');
const sandbox = require('../server/sandbox');
after(() => fs.rmSync(root, { recursive: true, force: true }));

let writeOld = () => {};
  const OLD_IMAGE_BYTES = 6_000;
  function oldMessageBytes(message) {
    const images = Array.isArray(message.content)
      ? message.content.filter(part => part.type === 'image_url' || (part.type === 'attachment' && part.attachment.type.startsWith('image/'))).length
      : 0;
    return Buffer.byteLength(JSON.stringify(message)) + images * OLD_IMAGE_BYTES;
  }

  // Complete tool exchanges, oldest first: the call, its results, and the
  // screenshots they showed. Each knows the stub that archives it.
  function oldToolExchanges(conversation, messages) {
    const exchanges = [];
    for (let i = 0; i < messages.length; i++) {
      const first = messages[i];
      if (!first.tool_calls?.length) continue;
      const ids = new Set(first.tool_calls.map(call => call.id));
      let end = i + 1;
      while (end < messages.length && messages[end].role === 'tool' && ids.has(messages[end].tool_call_id)) end++;
      if (end - i - 1 !== ids.size) continue;
      while (end < messages.length && messages[end].seeBuild) end++;
      const original = messages.slice(i, end);
      const text = JSON.stringify(original);
      const file = `context/${crypto.createHash('sha256').update(text).digest('hex').slice(0, 24)}.json`;
      const outcomes = original.filter(m => m.role === 'tool').map(m => {
        const call = first.tool_calls.find(c => c.id === m.tool_call_id);
        let result;
        try { result = JSON.parse(m.content); } catch { /* plain terminal output */ }
        return { tool: call.function.name, ...(result ? {
          ok: result.ok, worldRevision: result.worldRevision, report: result.report, reports: result.tests?.map(test => test.report),
          error: typeof result.error === 'string' ? result.error.slice(0, 400) : undefined,
        } : {}) };
      });
      const replacement = { role: 'assistant', content:
        `${typeof first.content === 'string' ? first.content : ''}\n[Earlier tool exchange archived at /work/${file}. Outcomes: ${JSON.stringify(outcomes)}. Read that file with bash if the original program, observations, or output is needed.]` };
      const bytes = original.reduce((sum, m) => sum + oldMessageBytes(m), 0);
      // An already compact exchange is kept as it is.
      const saves = Math.max(0, bytes - oldMessageBytes(replacement));
      exchanges.push({ start: i, end, text, file, replacement, saves });
      i = end - 1;
    }
    return exchanges;
  }

  // The history a request sends. Over the budget, the oldest complete tool
  // exchanges are archived to /work (the UI and database keep the original)
  // until half the budget is left, never user instructions, the newest
  // `keepExchanges` exchanges, or half of a call and its results. What is sent
  // only ever grows between two archivings: each request in the history is
  // replayed (one before every assistant message, and this one), so an
  // exchange archived once is archived at every later request. Signed thinking
  // is bound to everything before it: the thinking of turns kept across an
  // archiving goes, and the thinking since stays valid.
  function oldContextMessages(conversation, { maxBytes = 512_000, keepExchanges = 6 } = {}) {
    const messages = conversation.messages.filter(m => m.role !== 'error');
    const sizes = messages.map(oldMessageBytes);
    const exchanges = oldToolExchanges(conversation, messages);
    const requests = [...messages.flatMap((m, i) => (m.role === 'assistant' ? [i] : [])), messages.length];
    let sent = 0;
    let next = 0;
    let saved = 0;
    let archived = 0;
    let cut = 0;
    for (const at of requests) {
      while (next < at) sent += sizes[next++];
      if (sent - saved <= maxBytes) continue;
      const complete = exchanges.filter(exchange => exchange.end <= at).length;
      while (archived < complete - keepExchanges && sent - saved > maxBytes / 2) saved += exchanges[archived++].saves;
      cut = at;
    }
    if (sent - saved > maxBytes) {
      throw new Error(`Protected user messages and recent tool exchanges exceed MCBENCH_CONTEXT_MAX_BYTES (${maxBytes}). Increase that setting or continue in a new chat; history was not silently discarded.`);
    }
    const output = [];
    let index = 0;
    for (const exchange of exchanges.slice(0, archived).filter(e => e.saves > 0)) {
      output.push(...messages.slice(index, exchange.start));
      writeOld(conversation.id, exchange.file, exchange.text);
      output.push(exchange.replacement);
      index = exchange.end;
    }
    output.push(...messages.slice(index));
    const kept = new Set(messages.slice(0, cut));
    return output.map((message) => {
      if (!message.reasoning_details || !kept.has(message)) return message;
      const { reasoning_details, ...rest } = message;
      return rest;
    });
  }


test('a long chat sends byte-identical requests with its sizes and archives kept', (t) => {
  const written = [];
  t.mock.method(sandbox, 'writeWorkFile', (id, file, text) => written.push([id, file, text]));
  const oldWritten = [];
  writeOld = (id, file, text) => oldWritten.push([id, file, text]);
  const conversation = { id: 'long', messages: [] };
  let checked = 0;
  let refused = 0;
  // The same request, or the same refusal of a history over the budget.
  const same = (options) => {
    let error = null;
    let expected;
    try { expected = JSON.stringify(oldContextMessages(conversation, options)); } catch (e) { error = e.message; }
    if (error) {
      refused += 1;
      assert.throws(() => contextMessages(conversation, options), { message: error });
    } else assert.equal(JSON.stringify(contextMessages(conversation, options)), expected);
  };
  const call = (i, name, args) => ({ id: `c${i}`, type: 'function', function: { name, arguments: JSON.stringify(args) } });
  for (let turn = 0; turn < 40; turn++) {
    conversation.messages.push({ id: crypto.randomUUID(), role: 'user', content: `Turn ${turn}: build more.` });
    for (let step = 0; step < 4; step++) {
      const i = turn * 4 + step;
      const name = ['build', 'see_build', 'test_build', 'bash'][step];
      conversation.messages.push({ id: crypto.randomUUID(), role: 'assistant', content: step ? '' : `Working on ${turn}`,
        ...(turn % 3 === 0 ? { reasoning_details: [{ type: 'thinking', thinking: 'x', signature: `s${i}` }] } : {}),
        tool_calls: [call(i, name, { program: 'p'.repeat(800 + (i % 7) * 300) })] });
      // Before its result is in, the call is half an exchange.
      for (const options of [{}, { maxBytes: 60_000, keepExchanges: 4 }]) same(options);
      conversation.messages.push({ id: crypto.randomUUID(), role: 'tool', tool_call_id: `c${i}`, content: JSON.stringify({ ok: true, worldRevision: i, logs: ['l'.repeat(400)] }) });
      if (name === 'see_build') {
        conversation.messages.push({ id: crypto.randomUUID(), role: 'user', seeBuild: true, content: [{ type: 'text', text: '[see_build] Screenshots' },
          { type: 'image_url', image_url: { url: `/api/images/a${i}.jpg` } }, { type: 'image_url', image_url: { url: `/api/images/b${i}.jpg` } }] });
      }
      for (const options of [{}, { maxBytes: 60_000, keepExchanges: 4 }, { maxBytes: 20_000, keepExchanges: 2 }]) {
        same(options);
        checked += 1;
      }
    }
    conversation.messages.push({ id: crypto.randomUUID(), role: 'assistant', content: `Done ${turn}` });
  }
  assert.equal(checked, 40 * 4 * 3);
  assert.deepEqual(written, oldWritten);
  assert.ok(written.length > 1000, `archive files written: ${written.length}`);
  assert.ok(refused > 0 && refused < checked, `refused ${refused}`);
});

test('an archive the model changed or removed in /work is written again at the next request, one left alone is not', (t) => {
  const conversation = { id: 'rewrite', messages: [{ id: 'u', role: 'user', content: 'Build.' }] };
  for (let i = 0; i < 12; i++) {
    conversation.messages.push({ id: `a${i}`, role: 'assistant', content: '', tool_calls: [{ id: `t${i}`, type: 'function', function: { name: 'build', arguments: JSON.stringify({ program: 'x'.repeat(5000) }) } }] },
      { id: `r${i}`, role: 'tool', tool_call_id: `t${i}`, content: JSON.stringify({ ok: true, logs: ['z'.repeat(5000)] }) });
  }
  const options = { maxBytes: 45_000, keepExchanges: 3 };
  const writes = t.mock.method(sandbox, 'writeWorkFile');
  const first = JSON.stringify(contextMessages(conversation, options));
  const files = writes.mock.calls.map((call) => call.arguments[1]);
  assert.ok(files.length >= 3, `archives written: ${files.length}`);
  const read = sandbox.makeReadFile('rewrite');
  const archived = files.map((file) => read(file));
  assert.equal(JSON.stringify(contextMessages(conversation, options)), first);
  assert.equal(writes.mock.callCount(), files.length, 'untouched archives are not written again');
  // The model removes one and edits another (bash in /work).
  const dir = sandbox.workDir('rewrite');
  fs.rmSync(path.join(dir, files[0]));
  fs.writeFileSync(path.join(dir, files[1]), 'edited');
  assert.equal(JSON.stringify(contextMessages(conversation, options)), first);
  assert.deepEqual(writes.mock.calls.slice(files.length).map((call) => call.arguments[1]).sort(), [files[0], files[1]].sort());
  assert.deepEqual(files.map((file) => read(file)), archived);
});
