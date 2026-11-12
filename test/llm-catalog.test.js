'use strict';

// Own file: the model catalog is cached per process.
const { test } = require('node:test');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
process.env.MCBENCH_SETTINGS_FILE = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settings-test-')), 'settings.json');
const assert = require('node:assert');

const { chatCompletion, modelInfo } = require('../server/llm');
const { transientCompletionError } = require('../server/completion-retry');

const CATALOG_URL = 'https://openrouter.ai/api/v1/models';
const STREAM = [
  { type: 'message_start', message: { usage: { input_tokens: 1, output_tokens: 0 } } },
  { type: 'content_block_delta', index: 0, delta: { type: 'text_delta', text: 'ok' } },
  { type: 'message_delta', delta: { stop_reason: 'end_turn' }, usage: { output_tokens: 1 } },
  { type: 'message_stop' },
].map((event) => `data: ${JSON.stringify(event)}\n\n`).join('');

// Both run before the catalog is cached; a failed fetch caches nothing.
test('concurrent catalog lookups share one fetch', async () => {
  const previousFetch = global.fetch;
  let fetches = 0;
  global.fetch = async (url) => {
    assert.strictEqual(url, CATALOG_URL);
    fetches++;
    await new Promise((resolve) => setTimeout(resolve, 20));
    return new Response('', { status: 502 });
  };
  try {
    const failed = await Promise.allSettled([modelInfo('openrouter', 'openai/gpt-5.5'), modelInfo('openrouter', 'openai/gpt-5.5')]);
    assert.deepStrictEqual(failed.map((r) => r.status), ['rejected', 'rejected']);
    assert.strictEqual(fetches, 1);
  } finally {
    global.fetch = previousFetch;
  }
});

test('an OpenRouter outage is not a transient Anthropic error, and a model is looked up once', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.ANTHROPIC_API_KEY;
  process.env.ANTHROPIC_API_KEY = 'test-key';
  const calls = [];
  let catalogUp = false;
  global.fetch = async (url) => {
    calls.push(url);
    if (url === CATALOG_URL) {
      return catalogUp
        ? Response.json({ data: [{ id: 'anthropic/fixture-2', reasoning: { supported_efforts: ['low', 'high'] } }] })
        : new Response('unavailable', { status: 503 });
    }
    return new Response(STREAM);
  };
  const options = { provider: 'anthropic', model: 'fixture-2', messages: [{ role: 'user', content: 'Reply' }] };
  try {
    const error = await chatCompletion(options).then(() => null, (e) => e);
    assert.match(error.message, /catalog could not describe fixture-2/);
    assert.strictEqual(transientCompletionError(error), false);
    assert.ok(!calls.includes('https://api.anthropic.com/v1/messages'));
    catalogUp = true;
    assert.strictEqual((await chatCompletion(options)).content, 'ok');
    // Later requests skip the catalog, even after it goes down again.
    catalogUp = false;
    calls.length = 0;
    assert.strictEqual((await chatCompletion(options)).content, 'ok');
    assert.deepStrictEqual(calls, ['https://api.anthropic.com/v1/messages']);
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.ANTHROPIC_API_KEY;
    else process.env.ANTHROPIC_API_KEY = previousKey;
  }
});

