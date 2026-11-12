'use strict';

// The settings file belongs to the data folder the server runs on: a server
// started on a scratch data folder must never read or rewrite the checkout's
// keys. MCBENCH_SETTINGS_FILE names another file outright.
const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

delete process.env.MCBENCH_SETTINGS_FILE;
const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settings-data-'));
process.env.MCBENCH_DATA_DIR = dataDir;

function load() {
  for (const name of ['settings', 'storage']) delete require.cache[require.resolve(`../server/${name}`)];
  return require('../server/settings');
}

test('settings are read from and written to the data folder', () => {
  fs.writeFileSync(path.join(dataDir, 'settings.json'), JSON.stringify({ providers: { openai: { apiKey: 'scratch-key' } } }));
  const settings = load();
  assert.strictEqual(settings.get().providers.openai.apiKey, 'scratch-key');
  settings.updateModelFavorites(['google::gemini-3.8-flash']);
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(path.join(dataDir, 'settings.json'), 'utf8')).modelFavorites, ['google::gemini-3.8-flash']);
});

test('MCBENCH_SETTINGS_FILE names the settings file whatever the data folder', () => {
  const file = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settings-file-')), 'named.json');
  fs.writeFileSync(file, JSON.stringify({ providers: { openai: { apiKey: 'named-key' } } }));
  process.env.MCBENCH_SETTINGS_FILE = file;
  try {
    assert.strictEqual(load().get().providers.openai.apiKey, 'named-key');
  } finally {
    delete process.env.MCBENCH_SETTINGS_FILE;
  }
});
