'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
process.env.MCBENCH_SETTINGS_FILE = require('node:path').join(require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(), 'mcbench-settings-test-')), 'settings.json');
const qwencloud = require('../server/qwencloud');
const settings = require('../server/settings');
const { chatCompletion, listModels, modelInfo, resolveProvider } = require('../server/llm');

test('QwenCloud picker excludes old aliases, snapshots, other vendors, and non-chat models', () => {
  const models = qwencloud.recentModels([
    { id: 'qwen3.7-plus', created: 100 },
    { id: 'qwen3.8-max-0902', created: 200 },
    { id: 'qwen3.8-flash', created: 150 },
    { id: 'qwen3.5-plus', created: 999 },
    { id: 'qwen3.7-max-2026-05-17', created: 999 },
    { id: 'qwen-plus', created: 999 },
    { id: 'qwen3.7-text-embedding', created: 999 },
    { id: 'qwen-image-3.0', created: 999 },
    { id: 'qwen-audio-3.0-asr-flash', created: 999 },
    { id: 'kimi-k3', created: 999 },
  ]);
  assert.deepEqual(models.map((model) => model.id), ['qwen3.8-max-0902', 'qwen3.8-flash', 'qwen3.7-plus']);
  assert.ok(models.every((model) => model.tools && !model.imageOutput));
  assert.equal(models[0].created, 200);
  assert.deepEqual(qwencloud.recentModels([]), []);
});

test('QwenCloud capabilities gate vision and thinking independently of OpenRouter', async () => {
  const previousFetch = global.fetch;
  global.fetch = async () => { throw new Error('Must not depend on another provider catalog'); };
  try {
    const textOnly = await modelInfo('qwencloud', 'qwen3.8-2.4t-a95b');
    assert.equal(textOnly.imageInput, false);
    assert.equal(textOnly.mandatoryReasoning, true);
    assert.deepEqual(textOnly.efforts, ['low', 'medium', 'xhigh']);
    const vision = await modelInfo('qwencloud', 'qwen3.8-flash');
    assert.equal(vision.imageInput, true);
    assert.deepEqual(vision.efforts, ['none', 'low', 'medium', 'xhigh']);
    assert.equal(vision.defaultEffort, 'xhigh');
    assert.equal((await modelInfo('qwencloud', 'qwen3.7-max')).imageInput, false);
    assert.equal((await modelInfo('qwencloud', 'unrecognized-model')).known, false);
  } finally {
    global.fetch = previousFetch;
  }
});

test('QwenCloud thinking controls never mix native effort with token budgets', () => {
  assert.deepEqual(qwencloud.requestOptions('qwen3.8-flash', 'none'), {
    enable_thinking: false, preserve_thinking: true,
  });
  for (const [input, expected] of [['low', 'low'], ['minimal', 'low'], ['medium', 'medium'], ['high', 'xhigh'], ['max', 'xhigh']]) {
    assert.deepEqual(qwencloud.requestOptions('qwen3.8-max', input), {
      enable_thinking: true, preserve_thinking: true, reasoning_effort: expected,
    });
  }
  assert.deepEqual(qwencloud.requestOptions('qwen3.7-plus', 'medium'), {
    enable_thinking: true, preserve_thinking: true, thinking_budget: 16384,
  });
  assert.deepEqual(qwencloud.requestOptions('qwen3.8-27b'), { enable_thinking: true });
  assert.throws(() => qwencloud.requestOptions('qwen3.8-2.4t-a95b', 'none'), /requires thinking/);
  assert.throws(() => qwencloud.requestOptions('qwen3.7-plus', 'invalid'), /Unsupported/);
});

test('QwenCloud authenticates its native catalog and keeps credentials server-side', async () => {
  const connection = settings.get().providers.qwencloud;
  const previousKey = connection.apiKey;
  const previousFetch = global.fetch;
  connection.apiKey = 'qwen-test-key';
  global.fetch = async (url, options) => {
    assert.equal(url, 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1/models');
    assert.equal(options.headers.authorization, 'Bearer qwen-test-key');
    return Response.json({ data: [{ id: 'qwen3.8-flash', created: 100 }, { id: 'qwen-turbo', created: 999 }] });
  };
  try {
    assert.equal(resolveProvider('qwencloud').protocol, 'openai');
    assert.deepEqual((await listModels('qwencloud')).map((model) => model.id), ['qwen3.8-flash']);
    const publicSettings = settings.publicSettings();
    assert.equal(publicSettings.providers.qwencloud.connected, true);
    assert.equal(publicSettings.providers.qwencloud.label, 'QwenCloud');
    assert.equal(publicSettings.providers.qwencloud.apiKey, undefined);
    assert.ok(!JSON.stringify(publicSettings).includes('qwen-test-key'));
  } finally {
    connection.apiKey = previousKey;
    global.fetch = previousFetch;
  }
});

test('QwenCloud streams thinking and tool calls, then replays reasoning with tool results and images', async () => {
  const connection = settings.get().providers.qwencloud;
  const previousKey = connection.apiKey;
  const previousFetch = global.fetch;
  connection.apiKey = 'qwen-test-key';
  const bodies = [];
  const sse = (events) => new Response(events.map((event) => `data: ${JSON.stringify(event)}\n\n`).join(''));
  global.fetch = async (url, options) => {
    assert.equal(url, 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions');
    assert.equal(options.headers.authorization, 'Bearer qwen-test-key');
    const body = JSON.parse(options.body);
    bodies.push(body);
    if (bodies.length === 1) {
      return sse([
        { choices: [{ delta: { reasoning_content: 'Inspect the shape.' } }] },
        { choices: [{ delta: { tool_calls: [{ index: 0, id: 'call-build', function: { name: 'build', arguments: '{"program":' } }] } }] },
        { choices: [{ delta: { tool_calls: [{ index: 0, function: { arguments: '"sample"}' } }] }, finish_reason: 'tool_calls' }] },
      ]);
    }
    return sse([{ choices: [{ delta: { content: 'Verified.' }, finish_reason: 'stop' }] }]);
  };
  try {
    const user = { role: 'user', content: [{ type: 'text', text: 'Build this' }, { type: 'image_url', image_url: { url: 'data:image/png;base64,YWJj' } }] };
    const tools = [{ type: 'function', function: { name: 'build', parameters: { type: 'object' } } }];
    const deltas = [];
    const first = await chatCompletion({ provider: 'qwencloud', model: 'qwen3.8-flash', effort: 'low', messages: [user], tools, onDelta: (delta) => deltas.push(delta) });
    assert.equal(first.reasoning, 'Inspect the shape.');
    assert.equal(first.finish_reason, 'tool_calls');
    assert.deepEqual(JSON.parse(first.tool_calls[0].function.arguments), { program: 'sample' });
    assert.equal(deltas[0].reasoning, first.reasoning);
    assert.equal(deltas[2].toolCall.id, 'call-build');
    assert.equal(bodies[0].reasoning_effort, 'low');
    assert.equal(bodies[0].thinking_budget, undefined);
    assert.deepEqual(bodies[0].messages[0], user);
    assert.deepEqual(bodies[0].tools, tools);
    const toolResult = { role: 'tool', tool_call_id: 'call-build', content: '{"ok":true}' };
    const second = await chatCompletion({ provider: 'qwencloud', model: 'qwen3.8-flash', effort: 'none', messages: [user, first, toolResult] });
    assert.equal(second.content, 'Verified.');
    assert.equal(bodies[1].enable_thinking, false);
    assert.equal(bodies[1].reasoning_effort, undefined);
    assert.equal(bodies[1].messages[1].reasoning_content, 'Inspect the shape.');
    assert.equal(bodies[1].messages[1].reasoning, undefined);
    assert.equal(bodies[1].messages[1].finish_reason, undefined);
    assert.deepEqual(bodies[1].messages[2], toolResult);
  } finally {
    connection.apiKey = previousKey;
    global.fetch = previousFetch;
  }
});
