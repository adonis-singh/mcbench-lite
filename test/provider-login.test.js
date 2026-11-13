'use strict';

// Signing in from Settings, through the routes of server/index.js: the CLIs
// are the fakes in test/fixtures (what the real ones print, copied from
// them), named by MCBENCH_CLAUDE_BINARY and MCBENCH_CODEX_BINARY, and the
// Cursor SDK is a fake module. Also the hosted gateway's hooks: MCBENCH_BIND,
// /api/activity and `hosted` in /api/config.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');
const { execFileSync } = require('node:child_process');
const { after, test, mock } = require('node:test');

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-provider-login-'));
const claudeDir = path.join(scratch, 'claude');
const codexHome = path.join(scratch, 'codex');
fs.mkdirSync(claudeDir);
fs.mkdirSync(codexHome);
Object.assign(process.env, {
  MCBENCH_DATA_DIR: path.join(scratch, 'data'),
  MCBENCH_SETTINGS_FILE: path.join(scratch, 'data', 'settings.json'),
  CLAUDE_CONFIG_DIR: claudeDir,
  CODEX_HOME: codexHome,
  MCBENCH_CLAUDE_BINARY: path.join(__dirname, 'fixtures', 'fake-claude.js'),
  MCBENCH_CODEX_BINARY: path.join(__dirname, 'fixtures', 'fake-codex.js'),
});

// The Cursor SDK's login: it hands over its address, then waits until the
// test settles it or the signal aborts it, as the real one polls cursor.com.
const cursorAuth = path.join(scratch, 'cursor-auth.json');
const cursorSdk = {
  pending: null,
  getDefaultSdkAuthPath: () => cursorAuth,
  Cursor: {
    auth: {
      login({ openBrowser, onLoginUrl, signal }) {
        assert.equal(openBrowser, false);
        onLoginUrl('https://cursor.com/loginDeepControl?challenge=c&uuid=u&mode=login&redirectTarget=sdk');
        return new Promise((resolve, reject) => {
          cursorSdk.pending = { resolve: () => { fs.writeFileSync(cursorAuth, '{}'); resolve({ apiKey: 'never-kept', apiKeyExpiresAtMs: 0 }); }, reject };
          signal.addEventListener('abort', () => reject(new Error('Login was cancelled.')));
        });
      },
      async logout() { fs.rmSync(cursorAuth, { force: true }); },
    },
  },
};
require.cache[require.resolve('@cursor/sdk')] = { id: '@cursor/sdk', loaded: true, exports: cursorSdk };

require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const store = require('../server/store');
const { Edit } = require('../server/turn-stream');
const app = require('../server/index');
const { PINS } = require('../server/tools');

after(() => fs.rmSync(scratch, { recursive: true, force: true }));

// Every line the server logs while these tests run, to check none carries a code.
const logged = [];
for (const level of ['log', 'info', 'warn', 'error']) {
  const original = console[level];
  console[level] = (...args) => { logged.push(args.map(String).join(' ')); original(...args); };
}

function call(method, pathname, body) {
  const req = Readable.from(body === undefined ? [] : [Buffer.from(JSON.stringify(body))]);
  Object.assign(req, { method, url: pathname, headers: {} });
  const res = new EventEmitter();
  res.writeHead = (status) => { res.status = status; res.headersSent = true; };
  res.end = (payload) => { res.raw = payload || ''; if (payload) res.body = JSON.parse(payload); res.emit('close'); };
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => {
    res.status = error.statusCode || 500;
    res.body = { error: error.message };
    res.raw = JSON.stringify(res.body);
  }).then(() => res);
}

async function until(provider, check) {
  const deadline = Date.now() + 5000;
  for (;;) {
    const res = await call('GET', `/api/provider-logins/${provider}`);
    assert.equal(res.status, 200, res.raw);
    if (check(res.body)) return res.body;
    if (Date.now() > deadline) throw new Error(`The sign-in did not get there: ${res.raw}`);
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
}

const settingsOf = async (provider) => (await call('GET', '/api/settings?refresh=1')).body.providers[provider];

test('Claude Code signs in with a pasted code that reaches the CLI and no response or log', async () => {
  assert.equal((await settingsOf('claude-code')).account, null);
  // No sign-in started yet.
  assert.equal((await call('GET', '/api/provider-logins/claude-code')).raw, 'null');
  assert.deepEqual((await settingsOf('claude-code')).signIn, { methods: ['claudeai', 'console'], program: { name: 'claude', title: PINS.claude.title, mb: PINS.claude.mb }, current: null, installed: true });
  const started = await call('POST', '/api/provider-logins', { provider: 'claude-code', method: 'claudeai' });
  assert.equal(started.status, 200, started.raw);
  assert.equal(started.body.state, 'starting');
  const waiting = await until('claude-code', (view) => view.state === 'waiting' && view.wantsInput);
  assert.match(waiting.url, /^https:\/\/claude\.com\/cai\/oauth\/authorize\?code=true&.*state=fake-state$/);
  assert.equal(waiting.userCode, null);
  // The CLI was told to open no browser itself.
  assert.equal(fs.readFileSync(path.join(claudeDir, 'fake-browser'), 'utf8'), 'true');

  // A second start while one runs is refused.
  const second = await call('POST', '/api/provider-logins', { provider: 'claude-code', method: 'console' });
  assert.equal(second.status, 409);

  // A line that is not a whole code: the CLI says so and waits on.
  const partial = await call('POST', '/api/provider-logins/claude-code/input', { text: 'half-a-code' });
  assert.equal(partial.status, 200);
  assert.ok(!partial.raw.includes('half-a-code'));
  const refused = await until('claude-code', (view) => view.message);
  assert.equal(refused.message, 'Invalid code. Please make sure the full code was copied.');
  assert.equal(refused.state, 'waiting');

  const code = `good#${crypto.randomUUID()}`;
  const sent = await call('POST', '/api/provider-logins/claude-code/input', { text: code });
  assert.equal(sent.status, 200);
  assert.ok(!sent.raw.includes(code));
  const done = await until('claude-code', (view) => view.state !== 'waiting');
  assert.equal(done.state, 'done');
  assert.equal(fs.readFileSync(path.join(claudeDir, 'fake-received'), 'utf8'), code);
  assert.ok(!JSON.stringify(done).includes(code));
  assert.ok(!logged.some((line) => line.includes(code) || line.includes('half-a-code')));
  const signedIn = await settingsOf('claude-code');
  assert.equal(signedIn.account, 'person@example.com');
  assert.equal(signedIn.plan, 'Claude Max 20x');
  assert.equal(signedIn.signIn.current.state, 'done');
  assert.ok(!JSON.stringify(signedIn).includes(code));

  // Signing out runs `claude auth logout`, which removes the login.
  const out = await call('POST', '/api/provider-logouts', { provider: 'claude-code' });
  assert.equal(out.status, 200, out.raw);
  assert.equal((await settingsOf('claude-code')).account, null);
});

test('a Claude Code sign-in that fails shows the CLI\'s own last lines', async () => {
  const started = await call('POST', '/api/provider-logins', { provider: 'claude-code', method: 'console' });
  assert.equal(started.status, 200, started.raw);
  const waiting = await until('claude-code', (view) => view.state === 'waiting' && view.wantsInput);
  assert.match(waiting.url, /^https:\/\/platform\.claude\.com\/oauth\/authorize\?/);
  await call('POST', '/api/provider-logins/claude-code/input', { text: 'bad#state' });
  const failed = await until('claude-code', (view) => view.state !== 'waiting');
  assert.equal(failed.state, 'failed');
  assert.match(failed.error, /Login failed: Request failed with status code 400$/);
  assert.equal((await settingsOf('claude-code')).account, null);
});

test('Claude Code offers both of its sign-in methods and no other', async () => {
  const res = await call('POST', '/api/provider-logins', { provider: 'claude-code', method: 'sso' });
  assert.equal(res.status, 400);
  assert.match(res.body.error, /claudeai or console/);
  assert.equal((await call('POST', '/api/provider-logins', { provider: 'openai' })).status, 400);
  assert.equal((await call('GET', '/api/provider-logins/openrouter')).status, 400);
});

test('a code is passed only while the CLI asks for one, as one line', async () => {
  await call('POST', '/api/provider-logins', { provider: 'claude-code', method: 'claudeai' });
  await until('claude-code', (view) => view.wantsInput);
  const twoLines = await call('POST', '/api/provider-logins/claude-code/input', { text: 'a#b\nc#d' });
  assert.equal(twoLines.status, 400);
  assert.ok(!twoLines.raw.includes('a#b'));
  // Cancelling ends the CLI; the sign-in reads cancelled and takes no code.
  const cancelled = await call('DELETE', '/api/provider-logins/claude-code');
  assert.equal(cancelled.body.state, 'cancelled');
  assert.equal((await call('POST', '/api/provider-logins/claude-code/input', { text: 'good#x' })).status, 409);
  await new Promise((resolve) => setTimeout(resolve, 200));
  assert.equal((await call('GET', '/api/provider-logins/claude-code')).body.state, 'cancelled');
  assert.ok(!fs.existsSync(path.join(claudeDir, '.claude.json')));
});

test('Codex shows the address and one-time code, and the login serves ChatGPT and Codex CLI', async () => {
  assert.equal((await settingsOf('codex')).account, null);
  const started = await call('POST', '/api/provider-logins', { provider: 'codex-cli' });
  assert.equal(started.status, 200, started.raw);
  const waiting = await until('codex', (view) => view.state === 'waiting');
  assert.deepEqual(waiting, { state: 'waiting', url: 'https://auth.openai.com/codex/device', userCode: 'FAKE-C0DE1', wantsInput: false, message: null, error: null });
  assert.equal(fs.readFileSync(path.join(codexHome, 'fake-browser'), 'utf8'), 'true');
  // One login for both: ChatGPT cannot start a second.
  assert.equal((await call('POST', '/api/provider-logins', { provider: 'codex' })).status, 409);
  assert.equal((await call('POST', '/api/provider-logouts', { provider: 'codex' })).status, 409);
  fs.writeFileSync(path.join(codexHome, 'fake-approve'), '');
  const done = await until('codex-cli', (view) => view.state !== 'waiting');
  assert.equal(done.state, 'done');
  fs.rmSync(path.join(codexHome, 'fake-approve'));
  for (const provider of ['codex', 'codex-cli']) {
    const entry = await settingsOf(provider);
    assert.equal(entry.account, 'person@example.com');
    assert.equal(entry.plan, 'ChatGPT Pro');
  }
  assert.equal((await call('POST', '/api/provider-logouts', { provider: 'codex' })).status, 200);
  assert.equal((await settingsOf('codex-cli')).account, null);
  // Signed out already: `codex logout` says so and exits 0.
  assert.equal((await call('POST', '/api/provider-logouts', { provider: 'codex-cli' })).status, 200);
});

test('a Codex sign-in that is denied shows the CLI\'s message', async () => {
  await call('POST', '/api/provider-logins', { provider: 'codex' });
  await until('codex', (view) => view.state === 'waiting');
  fs.writeFileSync(path.join(codexHome, 'fake-deny'), '');
  const failed = await until('codex', (view) => view.state !== 'waiting');
  fs.rmSync(path.join(codexHome, 'fake-deny'));
  assert.equal(failed.state, 'failed');
  assert.match(failed.error, /Error logging in: device code exchange failed: the code was denied$/);
});

test('a sign-in nobody finishes ends after 10 minutes', async () => {
  mock.timers.enable({ apis: ['setTimeout'] });
  try {
    await call('POST', '/api/provider-logins', { provider: 'codex' });
    mock.timers.tick(10 * 60_000 - 1);
    assert.notEqual((await call('GET', '/api/provider-logins/codex')).body.state, 'failed');
    mock.timers.tick(1);
    const ended = (await call('GET', '/api/provider-logins/codex')).body;
    assert.equal(ended.state, 'failed');
    assert.match(ended.error, /not finished within 10 minutes/);
  } finally {
    mock.timers.reset();
  }
  // The process is ended, and a new sign-in can start.
  const again = await call('POST', '/api/provider-logins', { provider: 'codex' });
  assert.equal(again.status, 200, again.raw);
  await call('DELETE', '/api/provider-logins/codex');
});

test('Cursor signs in through its SDK, which opens no browser, and signs out', async () => {
  assert.equal((await settingsOf('cursor')).signedIn, false);
  await call('POST', '/api/provider-logins', { provider: 'cursor' });
  const waiting = await until('cursor', (view) => view.state === 'waiting');
  assert.match(waiting.url, /^https:\/\/cursor\.com\/loginDeepControl\?/);
  cursorSdk.pending.resolve();
  const done = await until('cursor', (view) => view.state !== 'waiting');
  assert.equal(done.state, 'done');
  assert.ok(!JSON.stringify(done).includes('never-kept'));
  assert.equal((await settingsOf('cursor')).signedIn, true);
  assert.equal((await call('POST', '/api/provider-logouts', { provider: 'cursor' })).status, 200);
  assert.equal((await settingsOf('cursor')).signedIn, false);
  // A cancelled SDK login ends its poll.
  await call('POST', '/api/provider-logins', { provider: 'cursor' });
  await until('cursor', (view) => view.state === 'waiting');
  assert.equal((await call('DELETE', '/api/provider-logins/cursor')).body.state, 'cancelled');
});

test('Settings show no terminal command for a sign-in', async () => {
  const { providers } = (await call('GET', '/api/settings')).body;
  for (const entry of Object.values(providers)) assert.equal(entry.login, undefined);
});

test('the server listens on MCBENCH_BIND, and on loopback when it is unset', () => {
  assert.equal(app.BIND, '127.0.0.1');
  const env = { ...process.env, MCBENCH_BIND: '0.0.0.0', MCBENCH_DATA_DIR: path.join(scratch, 'bind-data') };
  const bound = execFileSync(process.execPath, ['-e', 'process.stdout.write(require("./server/index").BIND)'], { cwd: path.join(__dirname, '..'), env, encoding: 'utf8' });
  assert.equal(bound, '0.0.0.0');
});

test('/api/activity is busy while work is held or a bench round is queued, idle otherwise', async () => {
  assert.deepEqual((await call('GET', '/api/activity')).body, { busy: false });
  const [hold] = app.reservations.take(['activity-test'], () => new Edit(), 'busy');
  assert.deepEqual((await call('GET', '/api/activity')).body, { busy: true });
  app.reservations.release('activity-test', hold);
  assert.deepEqual((await call('GET', '/api/activity')).body, { busy: false });
  // A bench whose first round no model has started yet.
  const { bench } = store.createBench(crypto.randomUUID(), {
    title: 'Queued', blind: false, content: 'A well',
    members: [{ provider: 'openrouter', model: 'queued/a', effort: null }, { provider: 'openrouter', model: 'queued/b', effort: null }],
  });
  assert.deepEqual((await call('GET', '/api/activity')).body, { busy: true });
  store.archiveBench(bench.id);
  assert.deepEqual((await call('GET', '/api/activity')).body, { busy: false });
});

test('/api/config says hosted only on a hosted instance', async () => {
  assert.equal((await call('GET', '/api/config')).body.hosted, undefined);
  process.env.MCBENCH_HOSTED = '1';
  try {
    assert.equal((await call('GET', '/api/config')).body.hosted, true);
  } finally {
    delete process.env.MCBENCH_HOSTED;
  }
});
