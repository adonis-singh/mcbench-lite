'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const NONE = { command: false, lightbox: false, settings: false, viewer: false, share: false };

test('Escape closes the topmost open layer, in the order they stack', async () => {
  const { LAYERS, topLayer } = await import('../web/layers.mjs');
  assert.deepStrictEqual(LAYERS.map((layer) => layer.id), ['command', 'lightbox', 'settings', 'viewer', 'share']);
  assert.equal(topLayer(NONE), null);
  assert.equal(topLayer({ ...NONE, viewer: true }), 'viewer');
  assert.equal(topLayer({ ...NONE, viewer: true, settings: true }), 'settings');
  assert.equal(topLayer({ ...NONE, settings: true, lightbox: true }), 'lightbox');
  // The command panel is over whatever it was opened on.
  assert.equal(topLayer({ ...NONE, settings: true, command: true }), 'command');
  assert.equal(topLayer({ ...NONE, viewer: true, command: true }), 'command');
  assert.equal(topLayer({ command: true, lightbox: true, settings: true, viewer: true, share: true }), 'command');
  // The Share panel sits in the chat header, under the viewer filling the window.
  assert.equal(topLayer({ ...NONE, share: true }), 'share');
  assert.equal(topLayer({ ...NONE, share: true, viewer: true }), 'viewer');
  // Closing the top one uncovers the next.
  assert.equal(topLayer({ command: false, lightbox: true, settings: true, viewer: true, share: false }), 'lightbox');
});

test('binds that act on the app wait while a modal layer covers it', async () => {
  const { bindRuns, modalOpen } = await import('../web/layers.mjs');
  const actions = ['build', 'bench', 'new', 'sidebar', 'select'];
  for (const action of actions) assert.equal(bindRuns(action, NONE), true);
  for (const layer of ['command', 'lightbox', 'settings']) {
    const open = { ...NONE, [layer]: true };
    assert.equal(modalOpen(open), true);
    for (const action of actions) assert.equal(bindRuns(action, open), false, `${action} under ${layer}`);
    // The search bind opens the command panel over a layer, and closes it.
    assert.equal(bindRuns('search', open), true);
  }
  // The viewer filling the window is still the app.
  assert.equal(modalOpen({ ...NONE, viewer: true }), false);
  for (const action of [...actions, 'search']) assert.equal(bindRuns(action, { ...NONE, viewer: true }), true);
  // So is the Share panel.
  assert.equal(modalOpen({ ...NONE, share: true }), false);
  for (const action of [...actions, 'search']) assert.equal(bindRuns(action, { ...NONE, share: true }), true);
});
