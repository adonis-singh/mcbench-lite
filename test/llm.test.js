'use strict';

const { test } = require('node:test');
// A fresh data directory: these tests must not read the developer's own settings.
process.env.MCBENCH_SETTINGS_FILE = require('node:path').join(require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(), 'mcbench-settings-test-')), 'settings.json');
const assert = require('node:assert');

const { chatCompletion, resolveProvider, _internal } = require('../server/llm');
const {
  anthropicRequest, anthropicBody, dataImage, remoteMessages, readSse, catalogIds, infoFromCatalogEntry, claudeCodeModels,
} = _internal;

test('remoteMessages strips provider-private response fields', () => {
  const out = remoteMessages([{
    role: 'assistant',
    content: '',
    reasoning: 'visible summary',
    reasoning_details: [{ type: 'reasoning.encrypted', data: 'opaque' }],
    finish_reason: 'tool_calls',
    tool_calls: [{ id: 'call_123', function: { name: 'build', arguments: '{}' } }],
  }]);
  assert.deepStrictEqual(out, [{
    role: 'assistant',
    content: '',
    tool_calls: [{ id: 'call_123', function: { name: 'build', arguments: '{}' } }],
  }]);
});

test('dataImage parses base64 image URLs', () => {
  assert.deepStrictEqual(dataImage('data:image/png;base64,YWJj'), {
    media_type: 'image/png',
    data: 'YWJj',
  });
  assert.throws(() => dataImage('https://example.com/image.png'));
});

test('readSse handles CRLF records split across chunks', async () => {
  const encoder = new TextEncoder();
  const chunks = ['event: ping\r\n', 'data: {"ok":', 'true}\r\n\r\n'];
  const response = {
    body: {
      async *[Symbol.asyncIterator]() {
        for (const chunk of chunks) yield encoder.encode(chunk);
      },
    },
  };
  const events = [];
  await readSse(response, (event) => events.push(event));
  assert.deepStrictEqual(events, [{ ok: true }]);
});

test('anthropicRequest translates system, images, tools, and tool history', () => {
  const request = anthropicRequest([
    { role: 'system', content: 'Build carefully.' },
    {
      role: 'user',
      content: [
        { type: 'text', text: 'Copy this' },
        { type: 'image_url', image_url: { url: 'data:image/jpeg;base64,YWJj' } },
      ],
    },
    {
      role: 'assistant',
      content: 'Starting',
      tool_calls: [{
        id: 'tool_1',
        type: 'function',
        function: { name: 'build', arguments: '{"summary":"house"}' },
      }],
    },
    { role: 'tool', tool_call_id: 'tool_1', content: '{"ok":true}' },
  ], [{
    type: 'function',
    function: {
      name: 'build',
      description: 'Build blocks',
      parameters: { type: 'object' },
    },
  }]);

  assert.strictEqual(request.system, 'Build carefully.');
  assert.deepStrictEqual(request.messages[0].content[1], {
    type: 'image',
    source: { type: 'base64', media_type: 'image/jpeg', data: 'YWJj' },
  });
  assert.deepStrictEqual(request.messages[1].content[1], {
    type: 'tool_use',
    id: 'tool_1',
    name: 'build',
    input: { summary: 'house' },
  });
  assert.deepStrictEqual(request.messages[2], {
    role: 'user',
    content: [{ type: 'tool_result', tool_use_id: 'tool_1', content: '{"ok":true}' }],
  });
  assert.deepStrictEqual(request.tools, [{
    name: 'build',
    description: 'Build blocks',
    input_schema: { type: 'object' },
    eager_input_streaming: true,
  }]);
});

test('anthropicRequest drops empty text blocks and replays signed thinking', () => {
  const request = anthropicRequest([
    {
      role: 'assistant',
      content: '', // pure tool-call turn: no text block allowed
      reasoning: 'summary text',
      reasoning_details: [{ type: 'thinking', thinking: 'plan the roof', signature: 'sig123' }],
      tool_calls: [{ id: 't1', type: 'function', function: { name: 'build', arguments: '{}' } }],
    },
    { role: 'tool', tool_call_id: 't1', content: '{"ok":true}' },
  ], []);
  assert.deepStrictEqual(request.messages[0].content, [
    { type: 'thinking', thinking: 'plan the roof', signature: 'sig123' },
    { type: 'tool_use', id: 't1', name: 'build', input: {} },
  ]);
});

test('catalogIds maps native provider slugs onto catalog ids', () => {
  assert.deepStrictEqual(catalogIds('openrouter', 'openai/gpt-5.5'), ['openai/gpt-5.5']);
  assert.deepStrictEqual(catalogIds('groq', 'openai/gpt-oss-120b'), ['openai/gpt-oss-120b']);
  assert.deepStrictEqual(catalogIds('openai', 'gpt-5.5'), ['openai/gpt-5.5']);
  assert.deepStrictEqual(catalogIds('codex', 'gpt-5.6-sol'), ['openai/gpt-5.6-sol']);
  assert.deepStrictEqual(
    catalogIds('anthropic', 'claude-sonnet-4-6-20261001'),
    ['anthropic/claude-sonnet-4-6', 'anthropic/claude-sonnet-4.6'],
  );
});

test('Anthropic requests summarize adaptive thinking whenever the model has effort levels, never a token budget, and cache the conversation', () => {
  const messages = [{ role: 'system', content: 'S' }, { role: 'user', content: 'hi' }];
  const thinking = { type: 'adaptive', display: 'summarized', block_binding: { prefix_mismatch_behavior: 'error' } };
  const body = anthropicBody({ model: 'claude-opus-5-5', messages, tools: [], effort: 'high', adaptive: true });
  assert.deepStrictEqual(body.thinking, thinking);
  assert.deepStrictEqual(body.cache_control, { type: 'ephemeral' });
  assert.deepStrictEqual(body.output_config, { effort: 'high' });
  assert.strictEqual(body.max_tokens, 32768);
  // No chosen effort: the API's default effort, still with a summary.
  const unset = anthropicBody({ model: 'claude-opus-5-5', messages, tools: [], effort: '', adaptive: true });
  assert.deepStrictEqual(unset.thinking, thinking);
  assert.strictEqual(unset.output_config, undefined);
  // A model without effort levels (budget-token thinking) keeps its API defaults.
  const legacy = anthropicBody({ model: 'claude-opus-4-5-20251101', messages, tools: [], effort: '', adaptive: false });
  assert.strictEqual(legacy.thinking, undefined);
  assert.strictEqual(legacy.output_config, undefined);
});

test('harness providers resolve native ids through their vendor catalog', () => {
  assert.deepStrictEqual(catalogIds('claude-code', 'claude-opus-5-5'), ['anthropic/claude-opus-5-5', 'anthropic/claude-opus-5.5']);
  assert.deepStrictEqual(catalogIds('codex-cli', 'gpt-6-astra'), ['openai/gpt-6-astra']);
  const entry = (id, created) => [id, { id, name: `Anthropic: ${id}`, created }];
  const byId = new Map([
    entry('anthropic/claude-opus-5.5', 5), entry('anthropic/claude-opus-5.5:batch', 5), entry('anthropic/claude-sonnet-4-6', 4), ['openai/gpt-5.5', { id: 'openai/gpt-5.5', created: 9 }],
  ]);
  assert.deepStrictEqual(claudeCodeModels(byId).map((m) => [m.id, m.created]), [['claude-opus-5-5', 5], ['claude-sonnet-4-6', 4]]);
});

test('ChatGPT sign-in resolves to a keyless provider with no network address', (t) => {
  // A stand-in Codex login, never the developer's own.
  const home = require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(), 'mcbench-codex-home-'));
  require('node:fs').writeFileSync(require('node:path').join(home, 'auth.json'), '{}');
  const previous = process.env.CODEX_HOME;
  process.env.CODEX_HOME = home;
  t.after(() => { if (previous === undefined) delete process.env.CODEX_HOME; else process.env.CODEX_HOME = previous; });
  const provider = resolveProvider('codex');
  assert.strictEqual(provider.baseUrl, undefined);
  assert.strictEqual(provider.protocol, 'responses');
  assert.strictEqual(provider.apiKey, '');
  assert.strictEqual(provider.apiKeyOptional, true);
});

test('catalog metadata normalizes model picker capabilities', () => {
  assert.deepStrictEqual(infoFromCatalogEntry({
    supported_parameters: ['tools', 'reasoning'],
    reasoning: {
      mandatory: false,
      default_effort: 'medium',
      supported_efforts: ['high', 'low', 'medium'],
    },
  }), {
    known: true,
    imageInput: null,
    imageOutput: false,
    tools: true,
    reasoning: true,
    efforts: ['low', 'medium', 'high'],
    defaultEffort: 'medium',
    mandatoryReasoning: false,
    contextWindow: null,
  });
  assert.strictEqual(infoFromCatalogEntry({
    architecture: { output_modalities: ['image', 'text'] },
  }).imageOutput, true);
  assert.strictEqual(infoFromCatalogEntry({
    architecture: { input_modalities: ['text', 'image'] },
  }).imageInput, true);
  assert.strictEqual(infoFromCatalogEntry({
    architecture: { input_modalities: ['text'] },
  }).imageInput, false);
  assert.deepStrictEqual(infoFromCatalogEntry(null), {
    known: false,
    imageInput: null,
    imageOutput: false,
    tools: null,
    reasoning: false,
    efforts: [],
    defaultEffort: null,
    mandatoryReasoning: false,
    contextWindow: null,
  });
  assert.strictEqual(infoFromCatalogEntry({ context_length: 1000000 }).contextWindow, 1000000);
});

test('chatCompletion routes an explicit provider and low reasoning effort', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.OPENROUTER_API_KEY;
  process.env.OPENROUTER_API_KEY = 'test-key';
  global.fetch = async (url, options) => {
    assert.strictEqual(url, 'https://openrouter.ai/api/v1/chat/completions');
    assert.strictEqual(options.headers.authorization, 'Bearer test-key');
    const body = JSON.parse(options.body);
    assert.strictEqual(body.model, 'openai/gpt-5.6-luna');
    assert.strictEqual(body.reasoning_effort, 'low');
    assert.strictEqual(body.tools, undefined);
    return new Response([
      'data: {"choices":[{"delta":{"content":"Horse Build"}}]}',
      '',
      'data: {"choices":[{"finish_reason":"stop","delta":{}}]}',
      '',
    ].join('\n'));
  };
  try {
    const result = await chatCompletion({
      provider: 'openrouter',
      model: 'openai/gpt-5.6-luna',
      effort: 'low',
      messages: [{ role: 'user', content: 'Name a horse build' }],
      tools: [],
    });
    assert.strictEqual(result.content, 'Horse Build');
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.OPENROUTER_API_KEY;
    else process.env.OPENROUTER_API_KEY = previousKey;
  }
});

test('streamed tool deltas retain stable call ids', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.OPENROUTER_API_KEY;
  process.env.OPENROUTER_API_KEY = 'test-key';
  global.fetch = async () => new Response([
    'data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call-one","function":{"name":"build","arguments":"{\\"summary\\":\\"one\\"}"}}]}}]}',
    '',
    'data: {"choices":[{"delta":{"tool_calls":[{"index":1,"id":"call-two","function":{"name":"bash","arguments":"{\\"command\\":\\"pwd\\"}"}}]}}]}',
    '',
    'data: {"choices":[{"finish_reason":"tool_calls","delta":{}}]}',
    '',
  ].join('\n'));
  const deltas = [];
  try {
    await chatCompletion({
      provider: 'openrouter',
      model: 'test/tool-model',
      messages: [{ role: 'user', content: 'Use tools' }],
      tools: [{ type: 'function', function: { name: 'build', parameters: {} } }],
      onDelta: (delta) => deltas.push(delta),
    });
    assert.deepStrictEqual(deltas.map((delta) => delta.toolCall.id), ['call-one', 'call-two']);
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.OPENROUTER_API_KEY;
    else process.env.OPENROUTER_API_KEY = previousKey;
  }
});

// As Gemini's OpenAI-compatible endpoint streams them (checked live): each
// call whole in one chunk with no index, a thought signature on the first
// call of a parallel batch, and finish_reason "stop" even with tool calls.
const GEMINI_TOOL_STREAM = [
  'data: {"choices":[{"delta":{"role":"assistant","tool_calls":[{"extra_content":{"google":{"thought_signature":"sig-one"}},"function":{"arguments":"{\\"city\\":\\"Paris\\"}","name":"get_weather"},"id":"call-one","type":"function"}]},"index":0}]}',
  '',
  'data: {"choices":[{"delta":{"role":"assistant","tool_calls":[{"function":{"arguments":"{\\"city\\":\\"Tokyo\\"}","name":"get_weather"},"id":"call-two","type":"function"}]},"index":0}]}',
  '',
  'data: {"choices":[{"delta":{"role":"assistant"},"finish_reason":"stop","index":0}],"usage":{"completion_tokens":32,"prompt_tokens":60,"total_tokens":92}}',
  '',
].join('\n');

test('Gemini tool calls come apart without an index and send their thought signatures back', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.GOOGLE_API_KEY;
  process.env.GOOGLE_API_KEY = 'test-key';
  const bodies = [];
  global.fetch = async (url, options) => {
    assert.strictEqual(url, 'https://generativelanguage.googleapis.com/v1beta/openai/chat/completions');
    assert.strictEqual(options.headers.authorization, 'Bearer test-key');
    bodies.push(JSON.parse(options.body));
    return new Response(bodies.length === 1 ? GEMINI_TOOL_STREAM : 'data: {"choices":[{"delta":{"content":"18 in both"},"finish_reason":"stop","index":0}]}\n\n');
  };
  const tools = [{ type: 'function', function: { name: 'get_weather', parameters: {} } }];
  const question = { role: 'user', content: 'Weather in Paris and Tokyo?' };
  try {
    const first = await chatCompletion({ provider: 'google', model: 'gemini-3.8-flash', effort: 'low', messages: [question], tools });
    assert.strictEqual(bodies[0].reasoning_effort, 'low');
    assert.deepStrictEqual(first.tool_calls, [
      { id: 'call-one', type: 'function', function: { name: 'get_weather', arguments: '{"city":"Paris"}' }, extra_content: { google: { thought_signature: 'sig-one' } } },
      { id: 'call-two', type: 'function', function: { name: 'get_weather', arguments: '{"city":"Tokyo"}' } },
    ]);
    const { telemetry, ...assistant } = first;
    const history = [question, assistant, ...first.tool_calls.map((call) => ({ role: 'tool', tool_call_id: call.id, content: '{"c":18}' }))];
    assert.strictEqual((await chatCompletion({ provider: 'google', model: 'gemini-3.8-flash', messages: history, tools })).content, '18 in both');
    assert.deepStrictEqual(bodies[1].messages[1].tool_calls, first.tool_calls);
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.GOOGLE_API_KEY;
    else process.env.GOOGLE_API_KEY = previousKey;
  }
});

test('a Gemini thought signature stays with Google: other providers get the bare call', () => {
  const call = { id: 'call-one', type: 'function', function: { name: 'build', arguments: '{}' } };
  const messages = [{ role: 'assistant', content: '', tool_calls: [{ ...call, extra_content: { google: { thought_signature: 'sig' } } }] }];
  assert.deepStrictEqual(remoteMessages(messages)[0].tool_calls, [call]);
  assert.deepStrictEqual(remoteMessages(messages, { signatures: true })[0].tool_calls, messages[0].tool_calls);
});

test('Gemini errors arrive as a one-element array and read as their message', () => {
  const body = '[{"error":{"code":400,"message":"Function call is missing a thought_signature","status":"INVALID_ARGUMENT"}}]';
  assert.strictEqual(_internal.providerErrorMessage({ apiKey: 'fixture-key' }, 400, body), 'LLM request failed (HTTP 400): Function call is missing a thought_signature');
});

test('a provider error that quotes the API key reaches the caller with [key] in its place', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.OPENAI_API_KEY;
  const key = ['sk', 'proj', 'FAKEfixture0123456789abcdefFAKE'].join('-');
  process.env.OPENAI_API_KEY = key;
  // OpenAI's 401 quotes the key it was sent (it shows only the ends of a
  // real one; the whole key is quoted here to test every copy).
  global.fetch = async () => new Response(JSON.stringify({
    error: { message: `Incorrect API key provided: ${key}. You can find your API key at https://platform.openai.com/account/api-keys. (${key})`, type: 'invalid_request_error', code: 'invalid_api_key' },
  }), { status: 401 });
  try {
    await assert.rejects(chatCompletion({ provider: 'openai', model: 'gpt-fixture', messages: [{ role: 'user', content: 'hi' }], tools: [] }), {
      message: 'LLM request failed (HTTP 401): Incorrect API key provided: [key]. You can find your API key at https://platform.openai.com/account/api-keys. ([key])',
    });
    // A key past the first 400 characters of a non-JSON answer is replaced before the answer is cut.
    const cut = _internal.providerErrorMessage({ apiKey: key }, 500, `${'x'.repeat(390)}${key}`);
    assert.ok(!cut.includes(key.slice(0, 10)), cut);
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.OPENAI_API_KEY;
    else process.env.OPENAI_API_KEY = previousKey;
  }
});

test('provider names inherited from Object.prototype are unknown providers', async () => {
  for (const name of ['constructor', 'toString', '__proto__']) {
    assert.throws(() => resolveProvider(name), { message: `Unknown provider: ${name}` });
    await assert.rejects(require('../server/llm').modelInfo(name, 'm'), { message: `Unknown provider: ${name}` });
  }
});

test('usage normalizes provider token accounting without treating unavailable values as zero', () => {
  const { normalizeUsage } = _internal;
  assert.equal(normalizeUsage(undefined), null);
  assert.deepStrictEqual(normalizeUsage({ prompt_tokens: 100, completion_tokens: 20, prompt_tokens_details: { cached_tokens: 80 }, completion_tokens_details: { reasoning_tokens: 10 } }), {
    inputTokens: 100, outputTokens: 20, cachedInputTokens: 80, cacheWriteTokens: null, reasoningTokens: 10,
  });
  assert.strictEqual(normalizeUsage({ input_tokens: 10, cache_creation_input_tokens: 20, cache_read_input_tokens: 30 }).inputTokens, 60);
});

test('request telemetry survives success/failure and incomplete tool streams never execute', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.OPENROUTER_API_KEY;
  process.env.OPENROUTER_API_KEY = 'test-key';
  const options = { provider: 'openrouter', model: 'fixture', effort: 'max', messages: [{ role: 'user', content: 'Run' }] };
  try {
    global.fetch = async (_, options) => {
      assert.strictEqual(JSON.parse(options.body).stream_options.include_usage, true);
      return new Response('data: {"choices":[{"delta":{"content":"ok"},"finish_reason":"stop"}]}\n\ndata: {"choices":[],"usage":{"prompt_tokens":123,"completion_tokens":4,"prompt_tokens_details":{"cached_tokens":99}}}\n\n');
    };
    const result = await chatCompletion(options);
    assert.strictEqual(result.telemetry.usage.inputTokens, 123);
    assert.strictEqual(result.telemetry.usage.cachedInputTokens, 99);
    assert.strictEqual(result.telemetry.effort, 'max');
    assert.ok(result.telemetry.wallMs >= result.telemetry.firstDeltaMs);
    global.fetch = async () => new Response('data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"unfinished","function":{"name":"build","arguments":"{}"}}]}}]}\n\n');
    await assert.rejects(chatCompletion(options), error => {
      assert.match(error.message, /without a finish reason/);
      assert.strictEqual(error.telemetry.failed, true);
      return true;
    });
    // A completion cut off after streaming text keeps that text as `reply`.
    global.fetch = async () => new Response('data: {"choices":[{"delta":{"content":"First the "}}]}\n\ndata: {"choices":[{"delta":{"content":"floor"}}]}\n\n');
    await assert.rejects(chatCompletion(options), error => error.reply === 'First the floor');
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.OPENROUTER_API_KEY;
    else process.env.OPENROUTER_API_KEY = previousKey;
  }
});


test('Anthropic collects input/cache/output usage and requires its completion event', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.ANTHROPIC_API_KEY;
  process.env.ANTHROPIC_API_KEY = 'test-key';
  const events = [
    { type: 'message_start', message: { usage: { input_tokens: 10, cache_creation_input_tokens: 20, cache_read_input_tokens: 30, output_tokens: 0 } } },
    { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'ok' } },
    { type: 'message_delta', delta: { stop_reason: 'end_turn' }, usage: { output_tokens: 4 } },
    { type: 'message_stop' },
  ];
  const options = { provider: 'anthropic', model: 'fixture', messages: [{ role: 'user', content: 'Reply' }] };
  const bodies = [];
  try {
    // The catalog lists the model's effort levels, so its thinking is summarized.
    global.fetch = async (url, init) => {
      if (url === 'https://openrouter.ai/api/v1/models') return Response.json({ data: [{ id: 'anthropic/fixture', reasoning: { supported_efforts: ['low', 'high'] } }] });
      bodies.push(JSON.parse(init.body));
      return new Response(events.map(e => `data: ${JSON.stringify(e)}\n\n`).join(''));
    };
    const result = await chatCompletion(options);
    assert.deepStrictEqual(bodies[0].thinking, { type: 'adaptive', display: 'summarized', block_binding: { prefix_mismatch_behavior: 'error' } });
    assert.strictEqual(result.telemetry.usage.inputTokens, 60);
    assert.strictEqual(result.telemetry.usage.outputTokens, 4);
    assert.strictEqual(result.telemetry.usage.cachedInputTokens, 30);
    events.pop();
    await assert.rejects(chatCompletion(options), /before message_stop/);
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.ANTHROPIC_API_KEY;
    else process.env.ANTHROPIC_API_KEY = previousKey;
  }
});
