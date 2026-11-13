'use strict';

// Its own settings file: never the checkout's, which holds the owner's keys.
process.env.MCBENCH_SETTINGS_FILE = require('node:path').join(require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(), 'mcbench-settings-test-')), 'settings.json');

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
// A fake `codex` binary whose `debug models` prints a catalogue, read by settings at load.
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-codex-test-'));
const CATALOGUE = { models: [
  { slug: 'gpt-7-nova', display_name: 'GPT-7 Nova', visibility: 'list', priority: 1, supported_reasoning_levels: [{ effort: 'low' }, { effort: 'high' }], default_reasoning_level: 'high', input_modalities: ['text', 'image'], context_window: 400000 },
  { slug: 'gpt-6-astra', display_name: 'GPT-6 Astra', visibility: 'list', priority: 2, supported_reasoning_levels: [{ effort: 'high' }], default_reasoning_level: 'high', input_modalities: ['text'], context_window: 272000 },
  { slug: 'codex-internal', display_name: 'Internal', visibility: 'hide', priority: 3 },
] };
const fakeCodex = path.join(scratch, 'codex');
fs.writeFileSync(fakeCodex, `#!/bin/sh\nprintf '%s' '${JSON.stringify(CATALOGUE)}'\n`, { mode: 0o755 });
process.env.MCBENCH_CODEX_BINARY = fakeCodex;

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { codexRequest, codexSessionFetch, codexCompletion } = require('../server/codex');
const llm = require('../server/llm');
const settings = require('../server/settings');
const { _internal: { readSse, codexHandlers } } = llm;

// A CODEX_HOME for one test: with a stand-in auth.json (never a real login) or
// none, restored afterwards. Returns the auth file path the provider will use.
function codexHome(t, { signedIn }) {
  const home = fs.mkdtempSync(path.join(scratch, 'home-'));
  const authFile = path.join(home, 'auth.json');
  if (signedIn) fs.writeFileSync(authFile, '{}');
  const previous = process.env.CODEX_HOME;
  process.env.CODEX_HOME = home;
  t.after(() => {
    if (previous === undefined) delete process.env.CODEX_HOME;
    else process.env.CODEX_HOME = previous;
    codexHandlers.delete(authFile);
  });
  return authFile;
}

// Any network request fails the test: ChatGPT sign-in must stay in-process.
function noNetwork(t) {
  const calls = [];
  const previous = global.fetch;
  global.fetch = async (url) => { calls.push(String(url)); throw new Error(`unexpected network request to ${url}`); };
  t.after(() => { global.fetch = previous; });
  return calls;
}

const completed = 'data: {"type":"response.completed","response":{"output":[],"usage":{"input_tokens":7,"output_tokens":2}}}\n\n';

test('ChatGPT requests go to the in-process handler, never the network', async (t) => {
  const authFile = codexHome(t, { signedIn: true });
  const network = noNetwork(t);
  const seen = [];
  codexHandlers.set(authFile, async (request) => {
    seen.push({ method: request.method, pathname: new URL(request.url).pathname, headers: Object.fromEntries(request.headers), body: await request.json() });
    return new Response(`data: {"type":"response.output_text.delta","delta":"ok"}\n\ndata: {"type":"response.output_item.done","output_index":0,"item":{"type":"message","content":[{"type":"output_text","text":"ok"}]}}\n\n${completed}`, { headers: { 'content-type': 'text/event-stream' } });
  });
  const result = await llm.chatCompletion({ provider: 'codex', model: 'gpt-6-astra', effort: 'high', messages: [{ role: 'user', content: 'Reply with ok' }], tools: [] });
  assert.equal(result.content, 'ok');
  assert.equal(result.telemetry.usage.inputTokens, 7);
  assert.equal(seen.length, 1);
  assert.equal(seen[0].method, 'POST');
  assert.equal(seen[0].pathname, '/v1/responses');
  assert.equal(seen[0].headers.authorization, undefined);
  assert.equal(seen[0].body.model, 'gpt-6-astra');
  assert.deepEqual(seen[0].body.reasoning, { effort: 'high', summary: 'auto' });
  assert.equal(seen[0].body.store, false);
  assert.deepEqual(network, []);
});

test('a streamed ChatGPT response reaches the turn intact, chunk by chunk', async (t) => {
  const authFile = codexHome(t, { signedIn: true });
  noNetwork(t);
  const call = { type: 'function_call', id: 'fc_1', call_id: 'call_1', name: 'build', arguments: '{"program":"mc.set([0,0,0], \\"stone\\")"}' };
  const events = [
    { type: 'response.output_text.delta', delta: 'Building ' },
    { type: 'response.output_text.delta', delta: 'now' },
    { type: 'response.output_item.added', output_index: 1, item: { ...call, arguments: '' } },
    { type: 'response.function_call_arguments.delta', output_index: 1, delta: call.arguments.slice(0, 12) },
    { type: 'response.function_call_arguments.delta', output_index: 1, delta: call.arguments.slice(12) },
    { type: 'response.output_item.done', output_index: 0, item: { type: 'message', content: [{ type: 'output_text', text: 'Building now' }] } },
    { type: 'response.output_item.done', output_index: 1, item: call },
  ];
  // Split mid-record and released one chunk at a time, after the previous one was read.
  const text = events.map((event) => `data: ${JSON.stringify(event)}\n\n`).join('') + completed;
  const chunks = [];
  for (let i = 0; i < text.length; i += 37) chunks.push(text.slice(i, i + 37));
  let pulled = 0;
  codexHandlers.set(authFile, async () => new Response(new ReadableStream({
    async pull(controller) {
      await new Promise((resolve) => setImmediate(resolve));
      if (pulled === chunks.length) controller.close();
      else controller.enqueue(new TextEncoder().encode(chunks[pulled++]));
    },
  }), { headers: { 'content-type': 'text/event-stream' } }));
  const deltas = [];
  const result = await llm.chatCompletion({ provider: 'codex', model: 'gpt-6-astra', messages: [{ role: 'user', content: 'Build' }], tools: [], onDelta: (delta) => deltas.push(delta) });
  assert.equal(pulled, chunks.length);
  assert.equal(result.content, 'Building now');
  assert.deepEqual(deltas.filter((delta) => delta.content).map((delta) => delta.content), ['Building ', 'now']);
  assert.equal(deltas.filter((delta) => delta.toolCall).map((delta) => delta.toolCall.args).join(''), call.arguments);
  assert.equal(result.tool_calls[0].id, 'call_1');
  assert.equal(result.tool_calls[0].function.arguments, call.arguments);
});

test('without a Codex login ChatGPT is disconnected and says to sign in from Settings', async (t) => {
  const authFile = codexHome(t, { signedIn: false });
  const network = noNetwork(t);
  codexHandlers.set(authFile, async () => { throw new Error('the handler must not be called'); });
  const entry = settings.publicSettings().providers.codex;
  assert.equal(entry.connected, false);
  assert.equal(entry.baseUrl, undefined);
  assert.equal(settings.details().providers.codex.summary, 'Not signed in');
  const message = /ChatGPT is not signed in: sign in to it in Settings/;
  await assert.rejects(llm.chatCompletion({ provider: 'codex', model: 'gpt-6-astra', messages: [{ role: 'user', content: 'hi' }], tools: [] }), message);
  await assert.rejects(llm.listModels('codex'), message);
  assert.deepEqual(network, []);
});

test('the real handler runs in-process and reads the login from CODEX_HOME', async (t) => {
  // A login file without tokens: the handler fails before any request leaves.
  codexHome(t, { signedIn: true });
  const network = noNetwork(t);
  await assert.rejects(llm.chatCompletion({ provider: 'codex', model: 'gpt-6-astra', messages: [{ role: 'user', content: 'hi' }], tools: [] }), /ChatGPT access token not found/);
  assert.deepEqual(network, []);
});

test('ChatGPT lists the live Codex catalogue, not a fixed model list', async (t) => {
  const authFile = codexHome(t, { signedIn: true });
  noNetwork(t);
  codexHandlers.set(authFile, async () => { throw new Error('the model list does not go through the handler'); });
  const models = await llm.listModels('codex');
  assert.deepEqual(models.map((model) => model.id), ['gpt-7-nova', 'gpt-6-astra']);
  assert.deepEqual(models[0].efforts, ['low', 'high']);
  assert.equal(models[0].imageInput, true);
  assert.equal((await llm.modelInfo('codex', 'gpt-7-nova')).contextWindow, 400000);
});

test('Codex replays tool results, images, and encrypted reasoning without remote state', () => {
  const reasoning = { type: 'reasoning', id: 'rs_1', encrypted_content: 'opaque', summary: [] };
  const body = codexRequest({
    model: 'gpt-6-astra', effort: 'high',
    messages: [
      { role: 'system', content: 'Build carefully' },
      { role: 'user', content: 'Build a platform' },
      { role: 'assistant', content: '', reasoning_details: [reasoning], tool_calls: [
        { id: 'call_1', function: { name: 'build', arguments: '{"program":"code"}' } },
      ] },
      { role: 'tool', tool_call_id: 'call_1', content: '{"ok":true}' },
      { role: 'user', content: [
        { type: 'text', text: 'Inspect this' },
        { type: 'image_url', image_url: { url: 'data:image/png;base64,YWJj' } },
      ] },
    ],
    tools: [{ type: 'function', function: { name: 'build', parameters: { type: 'object' } } }],
  });
  assert.deepEqual(body.input, [
    { role: 'user', content: 'Build a platform' }, reasoning,
    { type: 'function_call', call_id: 'call_1', name: 'build', arguments: '{"program":"code"}' },
    { type: 'function_call_output', call_id: 'call_1', output: '{"ok":true}' },
    { role: 'user', content: [
      { type: 'input_text', text: 'Inspect this' },
      { type: 'input_image', image_url: 'data:image/png;base64,YWJj', detail: 'high' },
      // A message that ends with an image is closed with text: the backend
      // reads no cache for a request whose input ends with an image.
      { type: 'input_text', text: '[End of images]' },
    ] },
  ]);
  assert.equal(body.instructions, 'Build carefully');
  assert.equal(body.store, false);
  assert.deepEqual(body.reasoning, { effort: 'high', summary: 'auto' });
  assert.equal(body.tools[0].name, 'build');
});

test('a tool result\'s screenshots go inside its function output, as the Codex CLI sends them, and a stray one fails', () => {
  const body = codexRequest({ model: 'gpt-6-astra', messages: [
    { role: 'user', content: 'Build and look' },
    { role: 'assistant', content: '', tool_calls: [{ id: 'call_1', function: { name: 'see_build', arguments: '{}' } }] },
    { role: 'tool', tool_call_id: 'call_1', content: '{"ok":true}' },
    { role: 'user', seeBuild: true, content: [{ type: 'text', text: '[see_build] Screenshots of the current build:' }, { type: 'image_url', image_url: { url: 'data:image/jpeg;base64,YWJj' } }] },
  ] });
  assert.deepEqual(body.input.at(-1), { type: 'function_call_output', call_id: 'call_1', output: [
    { type: 'input_text', text: '{"ok":true}' },
    { type: 'input_text', text: '[see_build] Screenshots of the current build:' },
    { type: 'input_image', image_url: 'data:image/jpeg;base64,YWJj', detail: 'high' },
  ] });
  assert.throws(() => codexRequest({ model: 'gpt-6-astra', messages: [{ role: 'user', seeBuild: true, content: [{ type: 'text', text: 'x' }] }] }), /must follow a tool result/);
  const { remoteMessages } = llm._internal;
  assert.deepEqual(remoteMessages([{ role: 'user', seeBuild: true, content: 'x' }]), [{ role: 'user', content: 'x' }]);
});

const sse = (events) => new Response(events.map((event) => `data: ${JSON.stringify(event)}\n\n`).join(''));

test('a chat\'s Codex requests carry its id where the Codex CLI puts its session id; a one-shot carries none', async (t) => {
  const id = '6f1c2a40-1111-4c3e-9d2b-0a5c8e7f6b21';
  const messages = [{ role: 'system', content: 'Name this chat' }, { role: 'user', content: 'Build a platform' }];
  assert.equal(codexRequest({ model: 'gpt-6-astra', messages, conversationId: id }).prompt_cache_key, id);
  assert.equal('prompt_cache_key' in codexRequest({ model: 'gpt-6-astra', messages }), false);
  // The OAuth bridge sends requests upstream through codexSessionFetch, which
  // adds the session headers from the body's key.
  const sent = [];
  t.mock.method(globalThis, 'fetch', async (url, init) => { sent.push({ url, headers: Object.fromEntries(new Headers(init.headers)) }); return new Response('{}'); });
  const upstream = 'https://chatgpt.com/backend-api/codex/responses';
  await codexSessionFetch(upstream, { method: 'POST', headers: new Headers({ authorization: 'Bearer x' }), body: JSON.stringify(codexRequest({ model: 'gpt-6-astra', messages, conversationId: id })) });
  await codexSessionFetch(upstream, { method: 'POST', headers: new Headers({ authorization: 'Bearer x' }), body: JSON.stringify(codexRequest({ model: 'gpt-6-astra', messages })) });
  await codexSessionFetch('https://auth.openai.com/oauth/token', { method: 'POST', body: 'grant_type=refresh_token' });
  assert.deepEqual(sent[0].headers, { authorization: 'Bearer x', 'session-id': id, 'thread-id': id });
  assert.deepEqual(sent[1].headers, { authorization: 'Bearer x' });
  assert.equal(sent[2].url, 'https://auth.openai.com/oauth/token');
});

test('Codex streams tool arguments with stable IDs and retains completed reasoning for replay', async () => {
  const reasoning = { type: 'reasoning', id: 'rs_1', encrypted_content: 'opaque', summary: [{ type: 'summary_text', text: 'Preparing' }] };
  const call = { type: 'function_call', id: 'fc_1', call_id: 'call_1', name: 'build', arguments: '{"program":"code"}' };
  const deltas = [];
  const message = await codexCompletion({}, { model: 'gpt-6-astra', messages: [], onDelta: (delta) => deltas.push(delta) }, async () => sse([
    { type: 'response.reasoning_summary_text.delta', item_id: 'rs_1', summary_index: 0, delta: 'Preparing' },
    { type: 'response.output_item.added', output_index: 1, item: { ...call, arguments: '' } },
    { type: 'response.function_call_arguments.delta', output_index: 1, delta: '{"program":' },
    { type: 'response.function_call_arguments.delta', output_index: 1, delta: '"code"}' },
    { type: 'response.output_item.done', output_index: 0, item: reasoning },
    { type: 'response.output_item.done', output_index: 1, item: call },
    { type: 'response.completed', response: { output: [], usage: { input_tokens: 123, output_tokens: 45, input_tokens_details: { cached_tokens: 100 } } } },
  ]), readSse);
  assert.equal(message.usage.input_tokens, 123);
  assert.equal(message.usage.input_tokens_details.cached_tokens, 100);
  assert.equal(message.tool_calls[0].id, 'call_1');
  assert.equal(message.tool_calls[0].function.arguments, call.arguments);
  assert.equal(message.finish_reason, 'tool_calls');
  assert.deepEqual(message.reasoning_details, [reasoning]);
  assert.equal(message.reasoning, 'Preparing');
  const toolDeltas = deltas.filter((delta) => delta.toolCall).map((delta) => delta.toolCall);
  assert.ok(toolDeltas.every((delta) => delta.id === 'call_1' && delta.index === 0 && delta.name === 'build'));
  assert.equal(toolDeltas.map((delta) => delta.args).join(''), call.arguments);
});

test('Codex separates summary parts without splitting tokens or repeating completed text', async () => {
  const items = [
    { type: 'reasoning', id: 'rs_1', summary: [
      { type: 'summary_text', text: '**Planning the layout**' },
      { type: 'summary_text', text: '**Sizing the canvas**' },
    ] },
    { type: 'reasoning', id: 'rs_2', summary: [
      { type: 'summary_text', text: '**Choosing materials**' },
    ] },
  ];
  const events = [];
  for (const item of items) {
    item.summary.forEach((part, summary_index) => {
      events.push({ type: 'response.reasoning_summary_part.added', item_id: item.id, summary_index });
      for (const delta of [part.text.slice(0, 8), part.text.slice(8)]) {
        events.push({ type: 'response.reasoning_summary_text.delta', item_id: item.id, summary_index, delta });
      }
      events.push({ type: 'response.reasoning_summary_text.done', item_id: item.id, summary_index, text: part.text });
      events.push({ type: 'response.reasoning_summary_part.done', item_id: item.id, summary_index, part });
    });
    events.push({ type: 'response.output_item.done', item });
  }
  events.push({ type: 'response.completed', response: { output: [] } });
  let streamed = '';
  const message = await codexCompletion({}, {
    model: 'gpt-6-astra', messages: [], onDelta: (delta) => { streamed += delta.reasoning; },
  }, async () => sse(events), readSse);
  const expected = '**Planning the layout**\n\n**Sizing the canvas**\n\n**Choosing materials**';
  assert.equal(streamed, expected);
  assert.equal(message.reasoning, expected);
  assert.deepEqual(message.reasoning_details, items);
});

test('Codex fails on upstream errors and truncated streams', async () => {
  const options = { model: 'gpt-6-astra', messages: [] };
  await assert.rejects(codexCompletion({}, options, async () => sse([
    { type: 'response.failed', response: { error: { message: 'Quota exceeded' } } },
  ]), readSse), /Quota exceeded/);
  await assert.rejects(codexCompletion({}, options, async () => sse([
    { type: 'response.output_text.delta', delta: 'Partial answer' },
  ]), readSse), /ended before response.completed/);
});

test('tool progress guard ignores arbitrary whitespace in code strings and resets on meaningful JSON',()=>{
 const {toolArgumentProgress}=require('../server/codex');const guard=toolArgumentProgress();
 assert.doesNotThrow(()=>guard(JSON.stringify({program:' '.repeat(100000)+'\\"\n\t'+' '.repeat(100000)})));
 for(let i=0;i<100;i++)assert.doesNotThrow(()=>guard(' '.repeat(16000)+',0'));
 const stalled=toolArgumentProgress();stalled('{"probes":[');stalled(' '.repeat(16000));assert.throws(()=>stalled('\n'.repeat(385)),/tool argument stream stalled/);
 const split=toolArgumentProgress();split('{"program":"\\');split('"'+' '.repeat(100000));assert.doesNotThrow(()=>split('"}'));
});
test('degenerate tool stream is cancelled rather than waiting for all remaining output',async()=>{
 const item={type:'function_call',call_id:'call_stalled',name:'test_build',arguments:''};let cancelled=false,index=0;
 const events=[{type:'response.output_item.added',output_index:0,item},{type:'response.function_call_arguments.delta',output_index:0,delta:'{"probes":['},{type:'response.function_call_arguments.delta',output_index:0,delta:' '.repeat(16385)}];
 const response=new Response(new ReadableStream({pull(controller){const event=events[index++]||{type:'response.function_call_arguments.delta',output_index:0,delta:' '};controller.enqueue(new TextEncoder().encode(`data: ${JSON.stringify(event)}\n\n`));},cancel(){cancelled=true;}}));
 await assert.rejects(codexCompletion({}, {model:'gpt-6-astra',messages:[]},async()=>response,readSse),/tool argument stream stalled/);assert.equal(cancelled,true);
});
