'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');

test('queued screenshots cancel without touching another render; active cancellation closes only the renderer and recovers', async t => {
  const playwright = require('playwright-core');
  let closeCount = 0;
  let enter;
  let release;
  let entered = new Promise(resolve => { enter = resolve; });
  let gate = new Promise(resolve => { release = resolve; });
  t.mock.method(playwright.chromium, 'launch', async () => {
    const browser = { on() {}, async close() { closeCount++; release(); }, async newPage() { return page; } };
    const page = {
      on() {}, async goto() {}, context() { return { browser: () => browser }; },
      async evaluate(fn, specs) {
        if (!fn.toString().includes('__shot.frames')) return;
        enter();
        await gate;
        return specs.map(() => `data:image/jpeg;base64,${Buffer.from('fixture').toString('base64')}`);
      },
    };
    return browser;
  });
  const { renderShots, resolveShot } = require('../server/shot');
  const shots = (world, views) => views.map((view) => resolveShot({ view }, world));
  const { World } = require('../server/world');
  const bounds = new World();
  bounds.set(0, 0, 0, 'minecraft:stone');
  bounds.set(1, 1, 1, 'minecraft:stone');
  const first = renderShots('first', shots(bounds, ['front']));
  await entered;
  const waiting = new AbortController();
  const second = renderShots('second', shots(bounds, ['front']), waiting.signal);
  waiting.abort();
  await assert.rejects(second, /abort/i);
  assert.equal(closeCount, 0);
  release();
  await first;
  entered = new Promise(resolve => { enter = resolve; });
  gate = new Promise(resolve => { release = resolve; });
  const active = new AbortController();
  const third = renderShots('third', shots(bounds, ['front']), active.signal);
  await entered;
  active.abort();
  await assert.rejects(third, /abort/i);
  assert.ok(closeCount >= 1);
  assert.equal((await renderShots('fourth', shots(bounds, ['front']))).length, 1);
});
