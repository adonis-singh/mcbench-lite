'use strict';

// What the page says of the first start (web/setup-status.mjs), from the
// view GET /api/setup gives (server/setup.js).
const assert = require('node:assert/strict');
const { test } = require('node:test');

const setup = require('../server/setup');

const load = () => import('../web/setup-status.mjs');

test('a download says how far it is, in MB of its total when the server said', async () => {
  const { megabytes } = await load();
  assert.equal(megabytes({ done: 84_200_000, total: 136_400_000 }), '84 of 136 MB');
  assert.equal(megabytes({ done: 12_600_000, total: null }), '13 MB');
  assert.equal(megabytes({ done: 0, total: 280_000_000 }), '0 of 280 MB');
});

test('a step reads as what it makes, or what its work does while it runs', async () => {
  const { stepRow } = await load();
  const step = { id: 'jdk', title: 'Java', doing: 'Downloading Java', optional: false };
  assert.deepEqual(stepRow({ ...step, state: 'waiting' }), { id: 'jdk', state: 'waiting', optional: false, label: 'Java', value: 'Waiting', note: null });
  assert.deepEqual(stepRow({ ...step, state: 'running', detail: 'Downloading Temurin', progress: { done: 84e6, total: 136e6 } }),
    { id: 'jdk', state: 'running', optional: false, label: 'Downloading Java', value: '84 of 136 MB', note: 'Downloading Temurin' });
  assert.deepEqual(stepRow({ ...step, state: 'running' }), { id: 'jdk', state: 'running', optional: false, label: 'Downloading Java', value: '', note: null });
  assert.deepEqual(stepRow({ ...step, state: 'done', seconds: 2.4 }), { id: 'jdk', state: 'done', optional: false, label: 'Java', value: 'Done', note: null });
  // A failure says what the server said.
  assert.deepEqual(stepRow({ ...step, state: 'failed', error: 'fetch failed' }), { id: 'jdk', state: 'failed', optional: false, label: 'Java', value: 'Failed', note: 'fetch failed' });
  assert.throws(() => stepRow({ ...step, state: 'paused' }), /cannot be paused/);
});

test('the count is of the steps the app needs; an optional step that cannot be done is no failure', async () => {
  const { setupStatus, heldLine } = await load();
  setup.reset();
  await setup.track('jdk', () => {});
  await setup.track('rust', () => {});
  await assert.rejects(setup.track('shell', async () => { throw new Error('Docker is not installed'); }, { unavailable: () => true }));
  const status = setupStatus(setup.view());
  assert.deepEqual([status.ready, status.done, status.total, status.failed], [false, 2, 9, false]);
  assert.deepEqual(status.rows.map((row) => row.id), setup.STEPS.map((step) => step.id));
  assert.deepEqual(status.rows.at(-1), { id: 'shell', state: 'unavailable', optional: true, label: "Model's shell", value: 'Not available', note: 'Docker is not installed' });
  assert.equal(heldLine(status), 'Sends when the app is ready. 2 of 9 done.');

  await assert.rejects(setup.track('browser', async () => { throw new Error('fetch failed'); }));
  const failed = setupStatus(setup.view());
  assert.equal(failed.failed, true);
  assert.equal(heldLine(failed), 'Not sent yet: a step failed. Try it again above.');
  setup.reset();
});

test("an error's web addresses become links, without the bracket or full stop after them", async () => {
  const { linkParts } = await load();
  assert.deepEqual(linkParts('install Docker Desktop (https://www.docker.com/products/docker-desktop/) on a Mac.'), [
    { text: 'install Docker Desktop (' },
    { url: 'https://www.docker.com/products/docker-desktop/' },
    { text: ') on a Mac.' },
  ]);
  assert.deepEqual(linkParts('see https://docs.docker.com/engine/install/.'), [{ text: 'see ' }, { url: 'https://docs.docker.com/engine/install/' }, { text: '.' }]);
  assert.deepEqual(linkParts('fetch failed'), [{ text: 'fetch failed' }]);
});
