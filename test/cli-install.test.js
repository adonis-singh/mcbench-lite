'use strict';

// Installing the Claude Code or Codex CLI from Connect an AI
// (server/cli-install.js): only when the program is not found, only into
// the tools folder, and the app runs the installed one from then on. The
// download itself is tools.js's (test/tools.test.js); here it is faked.
const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-cli-install-'));
process.env.MCBENCH_TOOLS_DIR = path.join(scratch, 'tools');
process.env.MCBENCH_SETTINGS_FILE = path.join(scratch, 'settings.json');
delete process.env.MCBENCH_CLAUDE_BINARY;
delete process.env.MCBENCH_CODEX_BINARY;
const bin = path.join(scratch, 'bin');
fs.mkdirSync(bin);
const previousPath = process.env.PATH;
process.env.PATH = bin;
after(() => {
  process.env.PATH = previousPath;
  fs.rmSync(scratch, { recursive: true, force: true });
});

const tools = require('../server/tools');
const cliInstall = require('../server/cli-install');
const claudeCode = require('../server/claude-code');

// The install as tools.js makes it: the program in its version's folder.
function fakeInstall(t, { fail = null } = {}) {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  t.mock.method(tools, 'install', async (name, { onProgress }) => {
    onProgress({ done: 5, total: 10 });
    await gate;
    if (fail) throw new Error(fail);
    const { folder, program } = tools.installed(name);
    fs.mkdirSync(path.dirname(program), { recursive: true });
    fs.writeFileSync(program, '#!/bin/sh\necho 9.9.9\n', { mode: 0o755 });
    assert.ok(fs.existsSync(folder));
    return program;
  });
  return release;
}

const settled = () => new Promise((resolve) => setImmediate(resolve));

test('a CLI that is not found is installed into the tools folder, and the app runs it from there', async (t) => {
  assert.equal(cliInstall.state('claude'), null);
  const release = fakeInstall(t);
  assert.deepEqual(cliInstall.start('claude'), { state: 'running', progress: { done: 5, total: 10 }, error: null });
  assert.deepEqual(cliInstall.start('claude'), cliInstall.state('claude'), 'one install at a time');
  assert.equal(tools.install.mock.callCount(), 1);
  release();
  await settled();
  assert.deepEqual(cliInstall.state('claude'), { state: 'done', progress: null, error: null });
  assert.equal(claudeCode.authCommand(['login']).file, tools.installed('claude').program);
  assert.throws(() => cliInstall.start('claude'), { statusCode: 409 }, 'installed in the tools folder already');
});

test('an install that fails says why, and can be started again', async (t) => {
  const release = fakeInstall(t, { fail: 'downloaded with sha256 0000, but mcbench-lite expects 1111' });
  cliInstall.start('codex');
  release();
  await settled();
  assert.deepEqual(cliInstall.state('codex'), { state: 'failed', progress: null, error: 'downloaded with sha256 0000, but mcbench-lite expects 1111' });
  assert.equal(fs.existsSync(tools.installed('codex').folder), false);
});

test('a CLI on PATH, or named by its variable, is the one the app runs, and is not installed', (t) => {
  fs.writeFileSync(path.join(bin, 'codex'), '#!/bin/sh\n', { mode: 0o755 });
  t.after(() => fs.rmSync(path.join(bin, 'codex')));
  assert.throws(() => cliInstall.start('codex'), { statusCode: 409, message: 'codex is installed already, and the app runs that one' });
  t.after(() => { delete process.env.MCBENCH_CODEX_BINARY; });
  process.env.MCBENCH_CODEX_BINARY = '/somewhere/codex';
  assert.throws(() => cliInstall.start('codex'), { statusCode: 409 });
});

test('the page names a CLI, never a path', () => {
  for (const name of ['/bin/sh', '../claude', 'cursor', '']) {
    assert.throws(() => cliInstall.start(name), { statusCode: 400, message: `No CLI named ${name} can be installed: claude or codex` });
  }
});
