'use strict';

const { test } = require('node:test');
// A fresh data directory: these tests must not read the developer's own settings.
process.env.MCBENCH_SETTINGS_FILE = require('node:path').join(require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(), 'mcbench-settings-test-')), 'settings.json');
const assert = require('node:assert');

const settings = require('../server/settings');
const { normalizeModelFavorites } = settings._internal;

test('settings expose provider connections without chat defaults', () => {
  const publicSettings = settings.publicSettings();
  // No title model chosen: each chat is titled by its own model.
  assert.strictEqual(publicSettings.titleModel, null);
  assert.ok(Array.isArray(publicSettings.modelFavorites));
  assert.strictEqual(publicSettings.activeProvider, undefined);
  for (const provider of Object.values(publicSettings.providers)) {
    assert.strictEqual(provider.model, undefined);
    assert.strictEqual(typeof provider.connected, 'boolean');
  }
  assert.deepStrictEqual(publicSettings.providers.codex, {
    label: 'ChatGPT',
    connected: require('../server/codex-cli').storedLogin(),
    envKey: undefined,
    auth: 'cli-login',
    harness: false,
    enabled: true,
    hiddenModels: [],
    modelOrder: [],
  });
  // CLI harnesses are keyless: their connection is the CLI's own login.
  for (const id of ['claude-code', 'codex-cli']) {
    assert.strictEqual(publicSettings.providers[id].auth, 'cli-login');
    assert.strictEqual(publicSettings.providers[id].envKey, undefined);
    assert.strictEqual(publicSettings.providers[id].login, undefined);
    assert.deepStrictEqual(settings.get().providers[id], { enabled: true, hiddenModels: [], modelOrder: [] });
  }
  // Cursor is a harness too (SDK agent, key or login), grouped with the CLIs.
  assert.deepStrictEqual(Object.entries(publicSettings.providers).filter(([, p]) => p.harness).map(([id]) => id), ['cursor', 'claude-code', 'codex-cli']);
});

test('the Settings screen gets one short state line per provider', () => {
  const { summary } = settings._internal;
  assert.strictEqual(summary('openai', { enabled: false, envKey: 'OPENAI_API_KEY', keySource: 'saved' }), 'Disabled');
  assert.strictEqual(summary('codex', { enabled: true, account: 'a@b.c', plan: 'ChatGPT Pro' }), 'Signed in · ChatGPT Pro');
  assert.strictEqual(summary('claude-code', { enabled: true, account: 'a@b.c', plan: null }), 'Signed in as a@b.c');
  assert.strictEqual(summary('openai', { enabled: true, envKey: 'OPENAI_API_KEY', keySource: 'saved' }), 'Key saved');
  assert.strictEqual(summary('openai', { enabled: true, envKey: 'OPENAI_API_KEY', keySource: 'env' }), 'Key from OPENAI_API_KEY');
  assert.strictEqual(summary('openai', { enabled: true, envKey: 'OPENAI_API_KEY', keySource: null }), 'No key');
  assert.strictEqual(summary('cursor', { enabled: true, envKey: 'CURSOR_API_KEY', keySource: null, signedIn: true }), 'Signed in with Cursor');
  assert.strictEqual(summary('cursor', { enabled: true, envKey: 'CURSOR_API_KEY', keySource: null, signedIn: false }), 'No key or login');
  assert.strictEqual(summary('codex-cli', { enabled: true, account: null }), 'Not signed in');
  const details = settings.details();
  for (const [id, entry] of Object.entries(details.providers)) {
    assert.strictEqual(typeof entry.summary, 'string', id);
    assert.strictEqual(typeof entry.connected, 'boolean', id);
  }
});

test('model favorites are normalized before persistence', () => {
  assert.deepStrictEqual(normalizeModelFavorites([
    'codex::gpt-5.6-sol',
    'codex::gpt-5.6-sol',
    'openrouter::openai/gpt-5.6-luna',
    'unknown::model',
    'openai::',
    'codex::  gpt-5.5  ',
    null,
  ]), [
    'codex::gpt-5.6-sol',
    'openrouter::openai/gpt-5.6-luna',
    'codex::gpt-5.5',
  ]);
});

test('the binary a CLI harness runs is the environment\'s, and no settings update names one', () => {
  const claudeCode = require('../server/claude-code');
  const current = settings.get();
  const providers = Object.fromEntries(Object.entries(current.providers).map(([id, p]) => [id, { ...p }]));
  providers['claude-code'] = { ...providers['claude-code'], binary: '/tmp/not-claude', home: '/tmp/elsewhere' };
  settings.update({ providers, titleModel: current.titleModel, shareRepo: current.shareRepo });
  assert.strictEqual(claudeCode.runtime.binary, process.env.MCBENCH_CLAUDE_BINARY || 'claude');
  assert.strictEqual('binary' in settings.get().providers['claude-code'], false);
});

test('an update that cannot be written leaves the settings in memory as the file has them', () => {
  const fs = require('node:fs');
  const before = settings.get();
  // A folder where the temporary file goes: the write fails.
  const temp = `${process.env.MCBENCH_SETTINGS_FILE}.${process.pid}.tmp`;
  const refused = `The settings were not saved: ${temp} already exists. Remove it and save again.`;
  fs.mkdirSync(temp);
  try {
    assert.throws(() => settings.update({ providers: { openai: { apiKey: 'sk-never-saved' } }, titleModel: { provider: 'openai', model: 'gpt-fixture', effort: 'high' }, shareRepo: null }), { message: refused });
    assert.strictEqual(settings.get(), before);
    assert.throws(() => settings.updateModelFavorites(['codex::gpt-5.5']), { message: refused });
    assert.strictEqual(settings.get(), before);
  } finally {
    fs.rmdirSync(temp);
  }
  assert.deepStrictEqual(settings.updateModelFavorites(['codex::gpt-5.5']), ['codex::gpt-5.5']);
  assert.deepStrictEqual(JSON.parse(fs.readFileSync(process.env.MCBENCH_SETTINGS_FILE, 'utf8')), settings.get());
});

test('a link planted at the temporary file\'s name fails the save, and the keys never reach its target', () => {
  const fs = require('node:fs');
  const path = require('node:path');
  const before = settings.get();
  const temp = `${process.env.MCBENCH_SETTINGS_FILE}.${process.pid}.tmp`;
  const planted = path.join(path.dirname(temp), 'another-account.json');
  fs.writeFileSync(planted, '');
  fs.symlinkSync(planted, temp);
  try {
    assert.throws(() => settings.update({ providers: { openai: { apiKey: 'sk-fixture-planted' } }, titleModel: null, shareRepo: null }), {
      message: `The settings were not saved: ${temp} already exists. Remove it and save again.`,
    });
    assert.strictEqual(fs.readFileSync(planted, 'utf8'), '');
    assert.strictEqual(fs.lstatSync(process.env.MCBENCH_SETTINGS_FILE).isSymbolicLink(), false);
    assert.strictEqual(settings.get(), before);
  } finally {
    fs.unlinkSync(temp);
    fs.unlinkSync(planted);
  }
});

test('provider names inherited from Object.prototype are not providers', () => {
  const { providers } = settings.get();
  for (const name of ['constructor', 'toString', '__proto__', 'hasOwnProperty']) {
    assert.throws(() => settings.update({ providers, titleModel: { provider: name, model: 'm' }, shareRepo: null }), { message: `Unknown title provider: ${name}` });
  }
  assert.deepStrictEqual(normalizeModelFavorites(['constructor::m', 'toString::m', '__proto__::m', 'openai::gpt-fixture']), ['openai::gpt-fixture']);
});

test('Google takes an API key, from Settings or GOOGLE_API_KEY', () => {
  const previous = process.env.GOOGLE_API_KEY;
  process.env.GOOGLE_API_KEY = 'from-the-shell';
  try {
    assert.deepStrictEqual(settings.PROVIDERS.google, { label: 'Google', envKey: 'GOOGLE_API_KEY' });
    assert.deepStrictEqual(settings.get().providers.google, { apiKey: '', enabled: true, hiddenModels: [], modelOrder: [] });
    const google = settings.details().providers.google;
    assert.strictEqual(google.connected, true);
    assert.strictEqual(google.keySource, 'env');
    assert.strictEqual(google.summary, 'Key from GOOGLE_API_KEY');
    delete process.env.GOOGLE_API_KEY;
    assert.strictEqual(settings.details().providers.google.summary, 'No key');
  } finally {
    if (previous === undefined) delete process.env.GOOGLE_API_KEY;
    else process.env.GOOGLE_API_KEY = previous;
  }
});

test('the title model is the chat\'s own until one is chosen, and a chosen one is kept', () => {
  const { providers } = settings.get();
  assert.strictEqual(settings.details().titleModel, null);
  assert.throws(() => settings.update({ providers, titleModel: { provider: 'nowhere', model: 'm' }, shareRepo: null }), /Unknown title provider: nowhere/);
  assert.throws(() => settings.update({ providers, titleModel: { provider: 'google', model: ' ' }, shareRepo: null }), /Choose a title model/);
  settings.update({ providers, titleModel: { provider: 'google', model: 'gemini-3.8-flash', effort: 'low' }, shareRepo: null });
  assert.deepStrictEqual(settings.details().titleModel, { provider: 'google', model: 'gemini-3.8-flash', effort: 'low' });
  settings.update({ providers, titleModel: null, shareRepo: null });
  assert.strictEqual(settings.get().titleModel, null);
  assert.strictEqual(JSON.parse(require('node:fs').readFileSync(process.env.MCBENCH_SETTINGS_FILE, 'utf8')).titleModel, null);
});

test('the repository for shared builds is null until one is set, and is owner/name', () => {
  const { providers } = settings.get();
  assert.strictEqual(settings.details().shareRepo, null);
  assert.throws(() => settings.update({ providers, titleModel: null, shareRepo: 'not a repo' }), /must be owner\/name/);
  assert.throws(() => settings.update({ providers, titleModel: null }), /must be owner\/name/);
  settings.update({ providers, titleModel: null, shareRepo: ' owner/builds.site ' });
  assert.strictEqual(settings.details().shareRepo, 'owner/builds.site');
  assert.strictEqual(JSON.parse(require('node:fs').readFileSync(process.env.MCBENCH_SETTINGS_FILE, 'utf8')).shareRepo, 'owner/builds.site');
  settings.update({ providers, titleModel: null, shareRepo: null });
  assert.strictEqual(settings.get().shareRepo, null);
});
