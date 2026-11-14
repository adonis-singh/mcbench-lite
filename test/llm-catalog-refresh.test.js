'use strict';

// Own file: the model catalog is held per process. The server fetches it
// when it starts and every five minutes (startCatalog); a turn reads the one
// fetched last and never waits for a refresh.
const { test } = require('node:test');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
process.env.MCBENCH_SETTINGS_FILE = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settings-test-')), 'settings.json');
const assert = require('node:assert');

const { modelInfo, startCatalog, stopCatalog } = require('../server/llm');

const CATALOG_URL = 'https://openrouter.ai/api/v1/models';
const entry = (vision) => ({ data: [{ id: 'openai/fixture', architecture: { input_modalities: vision ? ['text', 'image'] : ['text'] }, supported_parameters: ['tools'] }] });

test('the catalog is fetched at start and on a timer; turns read the last one, and a failed refresh leaves it', async (t) => {
  t.mock.timers.enable({ apis: ['setInterval'] });
  const previousFetch = global.fetch;
  const answers = [entry(true), entry(false), null];
  let fetches = 0;
  global.fetch = async (url) => {
    assert.strictEqual(url, CATALOG_URL);
    const answer = answers[fetches++];
    return answer ? Response.json(answer) : new Response('unavailable', { status: 503 });
  };
  const errors = [];
  t.mock.method(console, 'error', (...args) => errors.push(args.join(' ')));
  try {
    startCatalog();
    // A turn before the first fetch has finished waits for that fetch.
    assert.strictEqual((await modelInfo('openrouter', 'openai/fixture')).imageInput, true);
    assert.strictEqual((await modelInfo('openrouter', 'openai/fixture')).imageInput, true);
    assert.strictEqual(fetches, 1);
    t.mock.timers.tick(5 * 60 * 1000);
    await new Promise(setImmediate);
    assert.strictEqual(fetches, 2);
    assert.strictEqual((await modelInfo('openrouter', 'openai/fixture')).imageInput, false);
    t.mock.timers.tick(5 * 60 * 1000);
    await new Promise(setImmediate);
    assert.strictEqual(fetches, 3);
    assert.match(errors.join('\n'), /\[model-catalog\] Model catalog fetch failed with status 503/);
    assert.strictEqual((await modelInfo('openrouter', 'openai/fixture')).imageInput, false);
    assert.strictEqual(fetches, 3, 'a turn does not fetch while a catalog is held');
  } finally {
    stopCatalog();
    global.fetch = previousFetch;
  }
});
