'use strict';

// Provider connections configured in the UI, persisted outside source control.
// API keys never leave the server after they are submitted.

const fs = require('fs');
const path = require('path');
const cursor = require('./cursor');
const claudeCode = require('./claude-code');
const codexCli = require('./codex-cli');
const providerLogin = require('./provider-login');
const { DATA_DIR } = require('./storage');
const tools = require('./tools');

// The data folder's own settings: a server on a scratch data folder never
// touches the checkout's keys. Batch workers, which move MCBENCH_DATA_DIR per
// attempt, name the checkout's file in MCBENCH_SETTINGS_FILE.
const file = path.resolve(process.env.MCBENCH_SETTINGS_FILE || path.join(DATA_DIR, 'settings.json'));

// `envKey` marks providers that take an API key. The others are keyless and
// run under a CLI's login: ChatGPT sign-in uses the Codex CLI's login file
// in-process (llm.js), and the CLI harnesses run the CLI itself.
// `harness` marks entries that run the vendor's own agent loop with mcbench's
// tools attached (Cursor through its SDK, Claude Code and Codex through their
// CLIs) rather than mcbench's loop calling a model API.
const PROVIDERS = {
  openrouter: { label: 'OpenRouter', envKey: 'OPENROUTER_API_KEY' },
  openai: { label: 'OpenAI', envKey: 'OPENAI_API_KEY' },
  codex: { label: 'ChatGPT', auth: 'cli-login' },
  anthropic: { label: 'Anthropic', envKey: 'ANTHROPIC_API_KEY' },
  groq: { label: 'Groq', envKey: 'GROQ_API_KEY' },
  google: { label: 'Google', envKey: 'GOOGLE_API_KEY' },
  qwencloud: { label: 'QwenCloud', envKey: 'QWENCLOUD_API_KEY' },
  cursor: { label: 'Cursor', envKey: 'CURSOR_API_KEY', auth: 'api-key-or-login', harness: true },
  'claude-code': { label: 'Claude Code', auth: 'cli-login', harness: true, runtime: { binary: 'claude', binaryEnv: 'MCBENCH_CLAUDE_BINARY', homeEnv: 'CLAUDE_CONFIG_DIR', home: '~' } },
  'codex-cli': { label: 'Codex CLI', auth: 'cli-login', harness: true, runtime: { binary: 'codex', binaryEnv: 'MCBENCH_CODEX_BINARY', homeEnv: 'CODEX_HOME', home: '~/.codex' } },
};
const RUNTIMES = { 'claude-code': claudeCode, 'codex-cli': codexCli };
const LOGINS = { codex: codexCli.storedLogin, cursor: cursor.storedLogin, 'claude-code': claudeCode.storedLogin, 'codex-cli': codexCli.storedLogin };

// Per provider: its key (API providers), whether it is offered at all, and
// which of its models the picker hides and in what order it lists them. A CLI
// harness's binary and config directory are the server's environment's
// (runtime.binaryEnv and runtime.homeEnv), never a setting: the server runs
// that binary, so nothing that can reach the settings may name it.
const strings = value => (Array.isArray(value) ? [...new Set(value.filter(entry => typeof entry === 'string' && entry))] : []);
function providerSettings(id, saved = {}, previous) {
  const provider = PROVIDERS[id];
  const key = typeof saved.apiKey === 'string' && saved.apiKey.trim() ? saved.apiKey.trim() : previous?.apiKey || '';
  return {
    ...(provider.envKey ? { apiKey: key } : {}),
    enabled: saved.enabled !== false,
    hiddenModels: strings(saved.hiddenModels),
    modelOrder: strings(saved.modelOrder),
  };
}

// A CLI's program is the one its environment variable names, else the one
// on PATH, else the one cli-install.js put in the tools folder (tools.js).
function applyRuntimes() {
  for (const [id, runtime] of Object.entries(RUNTIMES)) {
    const { binary, binaryEnv } = PROVIDERS[id].runtime;
    runtime.configure({ binary: process.env[binaryEnv] || tools.cliProgram(binary), home: '' });
  }
}

// A CLI was installed into the tools folder: the harnesses run it, and
// Settings reads its version again.
function refreshRuntimes() {
  applyRuntimes();
  versions.clear();
}

const saved = fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : {};
let settings = {
  providers: Object.fromEntries(Object.keys(PROVIDERS).map((id) => [id, providerSettings(id, saved.providers?.[id])])),
  // null until the user picks one: each chat is titled by its own model.
  titleModel: saved.titleModel ? { ...saved.titleModel, effort: saved.titleModel.effort || 'low' } : null,
  modelFavorites: normalizeModelFavorites(saved.modelFavorites),
  // The GitHub repository ("owner/name") shared builds are published to
  // (share.js), or null.
  shareRepo: saved.shareRepo ?? null,
};
applyRuntimes();

function normalizeModelFavorites(value) {
  if (!Array.isArray(value)) return [];
  const favorites = [];
  for (const entry of value) {
    if (typeof entry !== 'string') continue;
    const separator = entry.indexOf('::');
    const provider = entry.slice(0, separator);
    const model = entry.slice(separator + 2).trim();
    const key = `${provider}::${model}`;
    if (separator < 1 || !Object.hasOwn(PROVIDERS, provider) || !model || favorites.includes(key)) continue;
    favorites.push(key);
  }
  return favorites.slice(0, 500);
}

// Keys live here: readable by the user only, and written whole then renamed
// into place so a crash never leaves a file that fails to load. The settings
// in memory become `next` only once the file holds it: a write that fails
// leaves the server on the settings a restart would load. The temporary file
// is created new ('wx'): anything already at its name, such as a link another
// account planted to receive the keys, fails the save.
function save(next) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const temp = `${file}.${process.pid}.tmp`;
  try {
    fs.writeFileSync(temp, JSON.stringify(next, null, 2), { mode: 0o600, flag: 'wx' });
  } catch (error) {
    if (error.code === 'EEXIST') throw new Error(`The settings were not saved: ${temp} already exists. Remove it and save again.`);
    throw error;
  }
  fs.renameSync(temp, file);
  settings = next;
}

function get() {
  return settings;
}

function update(next) {
  const providers = Object.fromEntries(Object.keys(PROVIDERS).map((id) => [id, providerSettings(id, next.providers?.[id], settings.providers[id])]));
  save({
    providers,
    titleModel: next.titleModel === null ? null : titleModelSetting(next.titleModel),
    modelFavorites: settings.modelFavorites,
    shareRepo: next.shareRepo === null ? null : shareRepoSetting(next.shareRepo),
  });
  applyRuntimes();
  versions.clear();
  return settings;
}

function titleModelSetting(value) {
  const provider = String(value?.provider || '').trim();
  const model = String(value?.model || '').trim();
  if (!Object.hasOwn(PROVIDERS, provider)) throw new Error(`Unknown title provider: ${provider}`);
  if (!model) throw new Error('Choose a title model');
  return { provider, model, effort: String(value.effort || '').trim() };
}

function shareRepoSetting(value) {
  const repo = String(value).trim();
  if (!/^[\w.-]+\/[\w.-]+$/.test(repo)) throw new Error('The repository for shared builds must be owner/name');
  return repo;
}

function updateModelFavorites(value) {
  if (!Array.isArray(value)) throw new Error('favorites must be an array');
  save({ ...settings, modelFavorites: normalizeModelFavorites(value) });
  return settings.modelFavorites;
}

// ChatGPT sign-in and the CLI harnesses are connected when their CLI login exists.
function connected(id) {
  const provider = PROVIDERS[id];
  if (provider.auth === 'cli-login') return LOGINS[id]();
  return Boolean(keySource(id)) || (provider.auth === 'api-key-or-login' && LOGINS[id]());
}

function publicSettings() {
  return {
    titleModel: settings.titleModel,
    modelFavorites: settings.modelFavorites,
    shareRepo: settings.shareRepo,
    providers: Object.fromEntries(Object.entries(PROVIDERS).map(([id, provider]) => [
      id,
      {
        label: provider.label,
        connected: connected(id),
        envKey: provider.envKey,
        auth: provider.auth || 'api-key',
        harness: Boolean(provider.harness),
        enabled: settings.providers[id].enabled,
        hiddenModels: settings.providers[id].hiddenModels,
        modelOrder: settings.providers[id].modelOrder,
      },
    ])),
  };
}

// What the Settings screen shows per provider beyond the connection flag:
// where a key comes from, who is signed in and on what plan, and the CLI
// version. CLI versions are read once per process; a CLI that is not
// installed has no version.
const versions = new Map();
function cliVersion(id, read, pattern) {
  if (!versions.has(id)) {
    let value = null;
    try { value = pattern.exec(read())?.[1] || null; } catch { /* not installed */ }
    versions.set(id, value);
  }
  return versions.get(id);
}

function keySource(id) {
  if (settings.providers[id].apiKey) return 'saved';
  if ((process.env[PROVIDERS[id].envKey] || '').trim()) return 'env';
  return null;
}

function details({ refresh = false } = {}) {
  if (refresh) { versions.clear(); codexCli.forgetModels(); }
  const pub = publicSettings();
  const codex = codexCli.account();
  const claude = claudeCode.account();
  for (const [id, entry] of Object.entries(pub.providers)) {
    const provider = PROVIDERS[id];
    if (provider.envKey) entry.keySource = keySource(id);
    if (provider.runtime) {
      entry.runtime = { ...provider.runtime, binaryValue: process.env[provider.runtime.binaryEnv] || '',
        resolvedHome: RUNTIMES[id] === claudeCode ? claudeCode.configDir() : codexCli.codexHome() };
    }
    if (id === 'codex' || id === 'codex-cli') Object.assign(entry, { account: codex?.email || null, plan: codex?.plan || null, version: cliVersion('codex', codexCli.version, /(\d+\.\d+\.\d+)/) });
    if (id === 'claude-code') Object.assign(entry, { account: claude?.email || null, plan: claude?.plan || null, version: cliVersion('claude', claudeCode.version, /(\d+\.\d+\.\d+)/) });
    if (id === 'cursor') Object.assign(entry, { signedIn: cursor.storedLogin(), version: cliVersion('cursor', cursor.version, /(.+)/) });
    // Signing in from Settings (provider-login.js); a CLI with no version is
    // not installed.
    const signIn = providerLogin.describe(id);
    if (signIn) entry.signIn = { ...signIn, installed: Boolean(entry.version) };
    entry.summary = summary(id, entry);
  }
  return { ...pub, checkedAt: new Date().toISOString() };
}

// One short line per provider for the Settings list; the plan says more at a
// glance than the address, which the detail pane shows.
function summary(id, entry) {
  if (!entry.enabled) return 'Disabled';
  if (entry.account) return entry.plan ? `Signed in · ${entry.plan}` : `Signed in as ${entry.account}`;
  if (entry.keySource === 'saved') return 'Key saved';
  if (entry.keySource === 'env') return `Key from ${entry.envKey}`;
  if (entry.signedIn) return 'Signed in with Cursor';
  if (entry.envKey) return id === 'cursor' ? 'No key or login' : 'No key';
  return 'Not signed in';
}

module.exports = {
  PROVIDERS,
  get,
  update,
  refreshRuntimes,
  updateModelFavorites,
  publicSettings,
  details,
  _internal: { normalizeModelFavorites, summary },
};
