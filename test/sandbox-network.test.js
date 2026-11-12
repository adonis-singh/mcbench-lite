'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const childProcess = require('node:child_process');

process.env.MCBENCH_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-network-'));
const sandbox = require('../server/sandbox');
const { systemPrompt } = require('../server/prompt');
const { _internal: { toolsForImageInput } } = require('../server/agent');
const bash = () => toolsForImageInput(true).find(t => t.function.name === 'bash');

test('the chat app keeps the one-shot networked bash mode', () => {
  assert.equal(sandbox.networkAccess(), true);
  assert.ok(bash().function.parameters.properties.network);
  assert.match(systemPrompt(), /network:true/);
});

test('benchmark mode removes network from the schema and refuses it in execution', async (t) => {
  sandbox.disableNetwork();
  const tool = bash();
  assert.equal(tool.function.parameters.properties.network, undefined);
  assert.equal(tool.function.parameters.additionalProperties, false);
  assert.doesNotMatch(JSON.stringify(tool), /internet|network:/i);
  assert.match(tool.function.description, /No network access/);
  assert.doesNotMatch(systemPrompt(), /network:true/);
  const spawn = t.mock.method(childProcess, 'spawn', () => { throw new Error('docker must not be started'); });
  await assert.rejects(sandbox.exec('offline', 'curl https://example.com', 1000, { network: true }), /Network access is disabled/);
  assert.equal(spawn.mock.callCount(), 0);
});
