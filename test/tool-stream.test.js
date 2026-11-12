'use strict';

const test = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { pathToFileURL } = require('node:url');

async function loadToolStream() {
  return import(pathToFileURL(path.join(__dirname, '..', 'web', 'tool-stream.mjs')).href);
}

test('partial tool arguments expose and retain program text as it streams', async () => {
  const { mergeToolCallDelta } = await loadToolStream();
  let item = {
    kind: 'tool',
    callIndex: 0,
    callId: 'call-1',
    tool: 'build',
    summary: '',
    status: 'writing',
    argsRaw: '',
  };

  const fragments = [
    '{"summary":"Build portrait","program":"const wall = mc.fill(',
    "'black_concrete', [0, 0, 0],",
    ' [10, 20, 1]);\\nreturn wall;"}',
  ];
  const observed = [];
  for (const args of fragments) {
    item = mergeToolCallDelta(item, { index: 0, id: 'call-1', name: 'build', args });
    observed.push(item.program);
  }

  assert.equal(observed[0], 'const wall = mc.fill(');
  assert.equal(observed[1], "const wall = mc.fill('black_concrete', [0, 0, 0],");
  assert.equal(observed[2], "const wall = mc.fill('black_concrete', [0, 0, 0], [10, 20, 1]);\nreturn wall;");
  assert.equal(item.summary, 'Build portrait');
});

test('complete string-wrapped tool arguments keep their program', async () => {
  const { parseToolArgs } = await loadToolStream();
  const nested = JSON.stringify({
    name: 'build',
    arguments: JSON.stringify({ summary: 'Build frame', program: 'return mc.setBlock(0, 0, 0, "stone");' }),
  });

  assert.deepEqual(parseToolArgs(nested), {
    summary: 'Build frame',
    program: 'return mc.setBlock(0, 0, 0, "stone");',
    path: undefined,
  });
});

test('a streaming summary is the outer object\'s, never a nested one', async () => {
  const { parseToolArgs } = await loadToolStream();
  assert.equal(parseToolArgs('{"tests":[{"summary":"nested"}],"summary":"top').summary, 'top');
  assert.equal(parseToolArgs('{"program":"const a = {\\"summary\\": \\"fake\\"};').summary, undefined);
  assert.equal(parseToolArgs('{"options":{"summary":"inner"},"program":"x').summary, undefined);
  assert.equal(parseToolArgs('{"summary":"half \\u00').summary, 'half ');
});
