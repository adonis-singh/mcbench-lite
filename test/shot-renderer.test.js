'use strict';

// The renderer's states, with the browser faked: a warm-up is bounded like
// every other stage, and a run that ends for any reason closes its browser.

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');

process.env.MCBENCH_SHOT_STAGE_TIMEOUT_MS = '40';

const jpeg = `data:image/jpeg;base64,${Buffer.from('drawn').toString('base64')}`;
const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// How `promise` ended within `ms`: 'resolved', its error, or 'pending'.
const within = (promise, ms) => Promise.race([promise.then(() => 'resolved', (error) => error), pause(ms).then(() => 'pending')]);

// A fresh shot.js on fake browsers: `behavior()` is asked at each launch
// ('healthy', 'stalls': the page never answers, 'no page': newPage fails,
// 'never closes': the browser's close never answers).
function renderer(t, behavior) {
  const playwright = require('playwright-core');
  const browsers = [];
  // No headless shell is installed for these: the launch is faked.
  t.mock.method(require('../server/tools'), 'headlessShell', async () => '/no/headless-shell');
  t.mock.method(playwright.chromium, 'launch', async () => {
    const mode = behavior();
    const browser = new EventEmitter();
    const page = new EventEmitter();
    const made = { page, closes: 0 };
    browsers.push(made);
    browser.newPage = async () => { if (mode === 'no page') throw new Error('fixture: no page'); return page; };
    browser.close = () => { made.closes += 1; return mode === 'never closes' ? new Promise(() => {}) : Promise.resolve(); };
    page.goto = async () => {};
    page.context = () => ({ browser: () => browser });
    page.evaluate = (fn, specs) => (mode === 'stalls' ? new Promise(() => {})
      : Promise.resolve(fn.toString().includes('__shot.frames') ? specs.map(() => jpeg) : undefined));
    return browser;
  });
  delete require.cache[require.resolve('../server/shot')];
  const shot = require('../server/shot');
  const { World } = require('../server/world');
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  return { ...shot, browsers, render: () => shot.renderShots('fixture', [shot.resolveShot({ view: 'front' }, world)]) };
}

test('a warm-up that stalls fails like any stage, closes its browser, and lets the next screenshot through', async (t) => {
  let mode = 'stalls';
  const { warmRenderer, render, closeRenderer, browsers } = renderer(t, () => mode);
  const warming = warmRenderer();
  const queued = render();
  while (!browsers.length) await pause(1);
  mode = 'healthy';
  const outcome = await within(warming, 2000);
  assert.match(String(outcome.message), /Screenshot initialization stalled/);
  assert.equal(browsers[0].closes, 1, 'the stalled browser is closed');
  assert.equal(await within(queued, 2000), 'resolved');
  assert.equal(browsers.length, 2, 'on a browser of its own');
  await closeRenderer();
  assert.deepEqual(browsers.map((browser) => browser.closes), [1, 1]);
});

test('closeRenderer ends although a warm-up never answers', async (t) => {
  const { warmRenderer, closeRenderer, browsers } = renderer(t, () => 'stalls');
  const warming = warmRenderer();
  warming.catch(() => {});
  while (!browsers.length) await pause(1);
  assert.equal(await within(closeRenderer(), 2000), 'resolved');
  assert.deepEqual(browsers.map((browser) => browser.closes), [1]);
});

test('a page that crashes with no screenshot under way has its browser closed', async (t) => {
  const { render, closeRenderer, browsers } = renderer(t, () => 'healthy');
  await render();
  browsers[0].page.emit('crash');
  assert.equal(browsers[0].closes, 1, 'the crashed page\'s browser');
  await render();
  assert.equal(browsers.length, 2);
  await closeRenderer();
  assert.deepEqual(browsers.map((browser) => browser.closes), [1, 1], 'no browser is left open');
});

test('a browser whose page cannot be opened is closed', async (t) => {
  let mode = 'no page';
  const { render, closeRenderer, browsers } = renderer(t, () => mode);
  await assert.rejects(render(), /fixture: no page/);
  assert.equal(browsers[0].closes, 1);
  mode = 'healthy';
  await render();
  await closeRenderer();
  assert.deepEqual(browsers.map((browser) => browser.closes), [1, 1]);
});

test('closeRenderer fails, within a stage\'s time, on a browser whose close never answers', async (t) => {
  const { render, closeRenderer, browsers } = renderer(t, () => 'never closes');
  await render();
  const outcome = await within(closeRenderer(), 2000);
  assert.match(String(outcome.message), /The screenshot browser did not close within 0\.04s/);
  assert.equal(browsers[0].closes, 1, 'its close was asked for');
  await assert.rejects(render(), /The screenshot renderer was shut down/);
  assert.equal(browsers.length, 1, 'and no browser opens again');
});
