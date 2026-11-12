'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');

process.env.MCBENCH_SHOT_STAGE_TIMEOUT_MS = '40';

test('renderer failures release the queue even when evaluate and close never finish', async t => {
  const playwright = require('playwright-core');
  let mode = 'crash';
  let launches = 0;
  t.mock.method(playwright.chromium, 'launch', async () => {
    launches++;
    const behavior = mode;
    const browser = new EventEmitter();
    const page = new EventEmitter();
    browser.newPage = async () => page;
    browser.close = () => new Promise(() => {});
    page.goto = async () => {};
    page.context = () => ({ browser: () => browser });
    page.evaluate = async (fn, specs) => {
      if (behavior === 'healthy') return fn.toString().includes('__shot.frames') ? specs.map(() => `data:image/jpeg;base64,${Buffer.from('verified').toString('base64')}`) : undefined;
      if (behavior === 'crash') setImmediate(() => page.emit('crash'));
      if (behavior === 'disconnect') setImmediate(() => browser.emit('disconnected'));
      if (behavior === 'pageerror') setImmediate(() => page.emit('pageerror', new Error('fixture script failed')));
      return new Promise(() => {});
    };
    page.locator = () => ({ screenshot: async () => Buffer.from('verified') });
    return browser;
  });

  const { World } = require('../server/world');
  const bounds = new World();
  bounds.set(0, 0, 0, 'minecraft:stone');
  bounds.set(1, 1, 1, 'minecraft:stone');
  for (const [failure, expected] of [
    ['crash', /renderer crashed/],
    ['disconnect', /renderer disconnected/],
    ['pageerror', /fixture script failed/],
    ['silent', /initialization stalled/],
  ]) {
    delete require.cache[require.resolve('../server/shot')];
    const { renderShots, resolveShot } = require('../server/shot');
  const shots = (world, views) => views.map((view) => resolveShot({ view }, world));
    mode = failure;
    await assert.rejects(renderShots(failure, shots(bounds, ['front'])), expected);
    mode = 'healthy';
    const [image] = await renderShots('recovered', shots(bounds, ['front']));
    assert.equal(image.toString(), 'verified');

  }
  assert.equal(launches, 8);
});
