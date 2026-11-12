'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

test('the highlight stays on its item when groups arrive in front of it', async () => {
  const { highlighted } = await import('../web/command.mjs');
  const local = ['new-chat', 'new-bench', 'settings'];
  // Nothing chosen yet: the first item.
  const first = highlighted(local, null);
  assert.equal(first, 'new-chat');
  // The server's builds and benches land in front: the same item, not the same position.
  const all = ['build:1', 'build:2', 'bench:1', ...local];
  assert.equal(highlighted(all, first), 'new-chat');
  // An item that left the list gives way to the first.
  assert.equal(highlighted(all, 'prompt:gone'), 'build:1');
  assert.equal(highlighted([], 'new-chat'), null);
});

test('the arrows step from the highlighted item and wrap round', async () => {
  const { stepped } = await import('../web/command.mjs');
  const keys = ['build:1', 'new-chat', 'settings'];
  assert.equal(stepped(keys, 'new-chat', 1), 'settings');
  assert.equal(stepped(keys, 'new-chat', -1), 'build:1');
  assert.equal(stepped(keys, 'settings', 1), 'build:1');
  assert.equal(stepped(keys, 'build:1', -1), 'settings');
  // From no choice, or one no longer listed, the step is from the first.
  assert.equal(stepped(keys, null, 1), 'new-chat');
  assert.equal(stepped(keys, 'gone', -1), 'settings');
});
