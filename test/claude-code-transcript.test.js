'use strict';

// A deleted chat takes its Claude Code session transcript with it. HOME is a
// temporary folder, so Claude Code's ~/.claude here is a test one.
const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-transcript-test-'));
process.env.HOME = path.join(root, 'home');
process.env.MCBENCH_DATA_DIR = path.join(root, 'data');
const harness = require('../server/harness');
const claudeCode = require('../server/claude-code');
const { CWD, transcriptFile } = claudeCode._internal;

after(() => fs.rmSync(root, { recursive: true, force: true }));

test('Claude Code files a session of the shared working directory under its dashed name', () => {
  assert.equal(CWD, path.join(root, 'home', '.cache', 'mcbench', 'claude-cwd'));
  const dashed = path.join(root, 'home', '.cache', 'mcbench', 'claude-cwd').replace(/[^a-zA-Z0-9]/g, '-');
  assert.equal(transcriptFile('s-1'), path.join(root, 'home', '.claude', 'projects', dashed, 's-1.jsonl'));
  claudeCode.configure({ home: path.join(root, 'config') });
  assert.equal(transcriptFile('s-1'), path.join(root, 'config', 'projects', dashed, 's-1.jsonl'));
  claudeCode.configure({});
});

test('deleting a chat deletes its session transcript, once its process has exited, and no other file', async () => {
  const folder = path.dirname(transcriptFile('x'));
  fs.mkdirSync(folder, { recursive: true });
  const files = ['chat-session', 'other-session', 'chat-session-2'].map((id) => path.join(folder, `${id}.jsonl`));
  for (const file of files) fs.writeFileSync(file, '{}\n');
  fs.mkdirSync(path.join(folder, 'chat-session'));
  harness.writeSession('chat', 'claude-code', { sessionId: 'chat-session', cwd: CWD, lastSeen: 'm1' });
  // The chat's warm Claude Code process, which exits a moment after its kill.
  let exit;
  const proc = { conversationId: 'chat', kill: () => new Promise((resolve) => { exit = () => { harness.untrack(proc); resolve(); }; }) };
  harness.track(proc);
  const destroyed = harness.destroy('chat');
  assert.equal(harness.readSession('chat', 'claude-code'), null);
  assert.equal(fs.existsSync(files[0]), true, 'kept while its process may still write it');
  exit();
  await destroyed;
  assert.deepEqual(files.map((file) => fs.existsSync(file)), [false, true, true]);
  assert.equal(fs.existsSync(path.join(folder, 'chat-session')), true, 'the session folder beside it stays');
  // A chat with no Claude Code session has nothing to delete.
  await harness.destroy('never-used');
  assert.deepEqual(files.map((file) => fs.existsSync(file)), [false, true, true]);
});
