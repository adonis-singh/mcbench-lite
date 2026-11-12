'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { _internal, cursorCompletion } = require('../server/cursor');
const { pendingResult } = require('../server/harness');

const { expandModels, modelSelection, renderPrompt, CursorSession } = _internal;

const CATALOG = [
  {
    id: 'claude-opus-5-5', displayName: 'Claude Opus 5.5',
    parameters: [
      { id: 'context', values: [{ value: '300k' }, { value: '1m' }] },
      { id: 'effort', values: ['low', 'medium', 'high', 'xhigh', 'max'].map((value) => ({ value })) },
      { id: 'fast', values: [{ value: 'false' }, { value: 'true' }] },
    ],
    variants: [{ isDefault: true, params: [{ id: 'context', value: '1m' }, { id: 'effort', value: 'medium' }, { id: 'fast', value: 'false' }] }],
  },
  {
    id: 'gpt-5.5', displayName: 'GPT-5.5',
    parameters: [{ id: 'reasoning', values: ['none', 'low', 'medium', 'high', 'extra-high'].map((value) => ({ value })) }],
    variants: [{ isDefault: true, params: [{ id: 'reasoning', value: 'medium' }] }],
  },
  {
    id: 'claude-sonnet-4', displayName: 'Claude Sonnet 4',
    parameters: [{ id: 'thinking', values: [{ value: 'false' }, { value: 'true' }] }],
    variants: [{ isDefault: true, params: [{ id: 'thinking', value: 'false' }, { id: 'context', value: '200k' }] }],
  },
  { id: 'default', displayName: 'Auto', parameters: [], variants: [{ isDefault: true, params: [] }] },
];

test('Cursor catalog lists fast modes separately and maps each native effort parameter', () => {
  const models = expandModels(CATALOG);
  assert.deepEqual([...models.keys()], ['claude-opus-5-5', 'claude-opus-5-5-fast', 'gpt-5.5', 'claude-sonnet-4', 'default']);
  assert.equal(models.get('claude-opus-5-5-fast').name, 'Claude Opus 5.5 Fast');
  assert.deepEqual(models.get('claude-opus-5-5').info.efforts, ['low', 'medium', 'high', 'xhigh', 'max']);
  assert.equal(models.get('claude-opus-5-5').info.defaultEffort, 'medium');
  assert.equal(models.get('claude-opus-5-5').info.mandatoryReasoning, true);
  assert.deepEqual(models.get('gpt-5.5').info.efforts, ['none', 'low', 'medium', 'high', 'xhigh']);
  assert.deepEqual(models.get('claude-sonnet-4').info.efforts, ['none', 'on']);
  assert.equal(models.get('claude-sonnet-4').info.defaultEffort, 'none');
  assert.equal(models.get('default').info.reasoning, false);
  assert.ok([...models.values()].every((model) => model.info.tools === true));
});

test('Cursor model selection keeps default variant settings and applies the chosen effort and speed', () => {
  const models = expandModels(CATALOG);
  assert.deepEqual(modelSelection(models.get('claude-opus-5-5-fast'), 'max'), {
    id: 'claude-opus-5-5',
    params: [{ id: 'context', value: '1m' }, { id: 'effort', value: 'max' }, { id: 'fast', value: 'true' }],
  });
  assert.deepEqual(modelSelection(models.get('gpt-5.5'), 'xhigh').params, [{ id: 'reasoning', value: 'extra-high' }]);
  assert.deepEqual(modelSelection(models.get('claude-sonnet-4'), 'on').params, [{ id: 'thinking', value: 'true' }, { id: 'context', value: '200k' }]);
  assert.deepEqual(modelSelection(models.get('claude-sonnet-4'), '').params, [{ id: 'thinking', value: 'false' }, { id: 'context', value: '200k' }]);
  assert.throws(() => modelSelection(models.get('claude-opus-5-5'), 'none'), /does not support reasoning effort none/);
});

const PNG = 'data:image/png;base64,iVBORw0KGgo=';

test('Cursor prompts carry the system prompt, prior transcript, images, and the new request', () => {
  const { text, images } = renderPrompt([
    { role: 'system', content: 'You build in Minecraft.' },
    { role: 'user', content: 'Build a hut.' },
    { role: 'assistant', content: 'Starting.', tool_calls: [{ id: 'call-1', type: 'function', function: { name: 'build', arguments: '{"summary":"hut"}' } }] },
    { role: 'tool', tool_call_id: 'call-1', content: '{"ok":true}' },
    { role: 'user', content: [{ type: 'text', text: '[see_build] Screenshots of the current build:' }, { type: 'image_url', image_url: { url: PNG } }] },
    { role: 'assistant', content: 'Done.' },
    { role: 'user', content: [{ type: 'text', text: 'Make the roof dark oak.' }, { type: 'image_url', image_url: { url: PNG } }] },
  ]);
  assert.match(text, /^<instructions>\nYou build in Minecraft\.\n<\/instructions>/);
  assert.match(text, /Called build \(call-1\) with \{"summary":"hut"\}/);
  assert.match(text, /Result of build \(call-1\):\n\{"ok":true\}/);
  assert.match(text, /\[Image 1\][\s\S]*<\/conversation>\n\nMake the roof dark oak\.\n\[Image 2\]$/);
  assert.deepEqual(images, [{ mimeType: 'image/png', data: 'iVBORw0KGgo=' }, { mimeType: 'image/png', data: 'iVBORw0KGgo=' }]);
});

test('A mid-turn restart continues from the latest tool result instead of inventing a request', () => {
  const { text } = renderPrompt([
    { role: 'system', content: 'S' },
    { role: 'user', content: 'Build a hut.' },
    { role: 'assistant', content: '', tool_calls: [{ id: 'call-1', type: 'function', function: { name: 'see_build', arguments: '{}' } }] },
    { role: 'tool', tool_call_id: 'call-1', content: '{"ok":true}' },
    { role: 'user', content: [{ type: 'text', text: '[see_build] Screenshots' }, { type: 'image_url', image_url: { url: PNG } }] },
  ]);
  assert.match(text, /<\/conversation>\n\nContinue the current task from the latest tool results above\.$/);
});

test('Pending tool results include screenshots attached after the tool message', () => {
  const messages = [
    { role: 'assistant', content: '', tool_calls: [{ id: 'call-1', type: 'function', function: { name: 'see_build', arguments: '{}' } }] },
    { role: 'tool', tool_call_id: 'call-1', content: '{"ok":true}' },
    { role: 'user', content: [{ type: 'text', text: '[see_build]' }, { type: 'image_url', image_url: { url: PNG } }] },
  ];
  assert.deepEqual(pendingResult(messages, 'call-1'), [
    { type: 'text', text: '{"ok":true}' }, { type: 'text', text: '[see_build]' }, { type: 'image', mimeType: 'image/png', data: 'iVBORw0KGgo=' },
  ]);
  assert.equal(pendingResult(messages, 'call-2'), null);
  assert.equal(pendingResult([...messages, { role: 'assistant', content: 'x' }], 'call-1'), null);
});

// A session following an agent's run, as start leaves it.
function follow(session, agent, run) {
  session.agent = agent;
  session.follow(run);
}

function fakeRun() {
  let finish;
  const done = new Promise((resolve) => { finish = resolve; });
  return { run: { wait: () => done, supports: () => true, cancel: async () => { finish({ status: 'cancelled' }); } }, finish };
}

const TOOL = { type: 'function', function: { name: 'build', description: 'Build', parameters: { type: 'object' } } };

test('Agent tool calls surface one at a time and resolve with the next completion result', async () => {
  const session = new CursorSession();
  const { run, finish } = fakeRun();
  let closed = false;
  follow(session, { close: () => { closed = true; } }, run);
  const { build } = session.tools([TOOL]);
  const deltas = [];
  const first = session.next((delta) => deltas.push(delta));
  session.update({ type: 'text-delta', text: 'Placing blocks.' });
  const results = [build.execute({ summary: 'a' }, { toolCallId: 'tool-a' }), build.execute({ summary: 'b' }, { toolCallId: 'tool-b' })];
  const message = await first;
  assert.equal(message.content, 'Placing blocks.');
  assert.deepEqual(message.tool_calls, [{ id: 'tool-a', type: 'function', function: { name: 'build', arguments: '{"summary":"a"}' } }]);
  assert.deepEqual(deltas.map((delta) => Object.keys(delta)[0]), ['content', 'toolCall']);
  session.deliver([{ type: 'text', text: 'ok a' }]);
  assert.deepEqual(await results[0], { content: [{ type: 'text', text: 'ok a' }] });
  const second = await session.next();
  assert.equal(second.content, '');
  assert.equal(second.tool_calls[0].id, 'tool-b');
  session.deliver([{ type: 'text', text: 'ok b' }]);
  await results[1];
  const last = session.next();
  session.update({ type: 'text-delta', text: 'Finished.' });
  session.update({ type: 'turn-ended', usage: { inputTokens: 10, outputTokens: 4, cacheReadTokens: 6, cacheWriteTokens: 0 } });
  finish({ status: 'finished', result: 'Finished.' });
  const final = await last;
  assert.equal(final.content, 'Finished.');
  assert.equal(final.finish_reason, 'stop');
  assert.deepEqual(final.usage, { input_tokens: 10, output_tokens: 4, cache_read_input_tokens: 6, cache_creation_input_tokens: 0 });
  assert.equal(closed, true);
});

test('Aborting the turn cancels the Cursor run and releases waiting tool calls', async () => {
  const controller = new AbortController();
  const session = new CursorSession(controller.signal);
  const { run } = fakeRun();
  let cancelled = false;
  run.cancel = async () => { cancelled = true; };
  follow(session, { close: () => {} }, run);
  const { build } = session.tools([TOOL]);
  const pendingCall = build.execute({}, { toolCallId: 'tool-a' });
  await session.next();
  const waiting = session.next();
  controller.abort(new Error('stopped'));
  await assert.rejects(waiting, /stopped/);
  assert.equal((await pendingCall).isError, true);
  assert.equal(cancelled, true);
});

test('A failed Cursor run rejects the completion with its error', async () => {
  const session = new CursorSession();
  const { run, finish } = fakeRun();
  follow(session, { close: () => {} }, run);
  const waiting = session.next();
  finish({ status: 'error', error: { message: 'quota exhausted' } });
  await assert.rejects(waiting, /Cursor run error: quota exhausted/);
});

// The SDK's agent factory and model list, replaced for one test.
async function withSdk(fakes, body) {
  const sdk = require('@cursor/sdk');
  const original = { create: sdk.Agent.create, list: sdk.Cursor.models.list };
  sdk.Agent.create = fakes.create;
  sdk.Cursor.models.list = fakes.list || (async () => CATALOG);
  try { await body(); } finally {
    sdk.Agent.create = original.create;
    sdk.Cursor.models.list = original.list;
  }
}

const request = (extra) => ({ conversationId: 'cursor-conv', model: 'gpt-5.5', messages: [{ role: 'user', content: 'Build a hut.' }], tools: [TOOL], ...extra });

test('A stop while Cursor resolves its model is honored: no agent is created', async () => {
  let created = 0;
  await withSdk({ create: async () => { created++; } }, async () => {
    const controller = new AbortController();
    const completion = cursorCompletion({ apiKey: null }, request({ signal: controller.signal }));
    controller.abort(new Error('stopped'));
    await assert.rejects(completion, /stopped/);
    assert.equal(created, 0);
    // Already stopped: no session is made at all.
    await assert.rejects(cursorCompletion({ apiKey: null }, request({ signal: controller.signal })), /stopped/);
  });
});

test('A Cursor agent that fails to send is closed with its session', async () => {
  let closed = false;
  await withSdk({ create: async () => ({ send: async () => { throw new Error('send failed'); }, close: () => { closed = true; } }) }, async () => {
    await assert.rejects(cursorCompletion({ apiKey: null }, request()), /send failed/);
    assert.equal(closed, true);
    // The failed session is gone: the next completion starts a fresh agent.
    await assert.rejects(cursorCompletion({ apiKey: null }, request()), /send failed/);
  });
  await withSdk({ create: async () => { throw new Error('create failed'); } }, async () => {
    await assert.rejects(cursorCompletion({ apiKey: null }, request()), /create failed/);
  });
});

test('Concurrent Cursor catalogue lookups share one fetch', async () => {
  let lists = 0;
  await withSdk({ list: async () => { lists++; return CATALOG; } }, async () => {
    const [a, b] = await Promise.all([_internal.models('fresh-key'), _internal.models('fresh-key')]);
    assert.equal(a, b);
    assert.equal(lists, 1);
  });
});

// An agent whose run streams `updates` through onDelta once send has
// returned it, then waits until it is cancelled.
function streamingAgent(updates, record) {
  return async () => ({
    send: async (_prompt, { onDelta }) => {
      const { run, finish } = fakeRun();
      run.cancel = async () => { record.cancelled = true; finish({ status: 'cancelled' }); };
      setImmediate(() => { for (const update of updates) onDelta({ update }); });
      return run;
    },
    close: () => { record.closed = true; },
  });
}

const started = (toolCall) => ({ type: 'tool-call-started', callId: 'c1', modelCallId: 'm1', toolCall });
const mcp = (providerIdentifier, toolName) => ({ type: 'mcp', args: { providerIdentifier, toolName, args: {} } });

// Cursor is told to offer only mcbench's tools; as with Claude Code and Codex,
// a run that starts any other tool ends with an error naming it.
test('A Cursor run that starts a tool outside mcbench ends with an error naming it', async () => {
  const cases = [
    [{ type: 'shell', args: { command: 'cat ~/.ssh/id_rsa' } }, /Cursor used a tool outside mcbench: shell/],
    [{ type: 'read', args: { path: '/etc/passwd' } }, /outside mcbench: read/],
    [mcp('github', 'create_issue'), /outside mcbench: mcp github create_issue/],
    [mcp('custom-user-tools', 'not_offered'), /outside mcbench: mcp custom-user-tools not_offered/],
    // A tool the SDK has no type for reaches onDelta as the protocol's value.
    [{ case: 'webFetchToolCall', value: {} }, /outside mcbench: webFetchToolCall/],
  ];
  for (const [index, [toolCall, error]] of cases.entries()) {
    const record = {};
    await withSdk({ create: streamingAgent([{ type: 'text-delta', text: 'Looking.' }, started(toolCall)], record) }, async () => {
      await assert.rejects(cursorCompletion({ apiKey: null }, request({ conversationId: `cursor-tools-${index}` })), error);
    });
    assert.deepEqual(record, { cancelled: true, closed: true }, String(error));
  }
});

test('A Cursor run that starts only mcbench custom tools goes on', async () => {
  const record = {};
  let agentTools;
  await withSdk({ create: async (options) => {
    agentTools = options;
    return streamingAgent([started(mcp('custom-user-tools', 'build')), { type: 'text-delta', text: 'Building.' }], record)();
  } }, async () => {
    const session = new CursorSession();
    await session.start(request(), { apiKey: null });
    const waiting = session.next();
    await new Promise((resolve) => setImmediate(resolve));
    assert.equal(session.state, 'running');
    assert.equal(session.text, 'Building.');
    session.close();
    await assert.rejects(waiting, /Cursor run closed/);
  });
  assert.deepEqual(agentTools.tools, ['mcp']);
  assert.deepEqual(Object.keys(agentTools.local.customTools), ['build']);
});
