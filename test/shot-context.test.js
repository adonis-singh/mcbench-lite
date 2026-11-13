'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');

test('context loss reloads the world and retries the whole inspection once', async t => {
  const playwright = require('playwright-core');
  const pages = [];
  let alwaysLose = false;
  // No headless shell is installed for these: the launch is faked.
  t.mock.method(require('../server/tools'), 'headlessShell', async () => '/no/headless-shell');
  t.mock.method(playwright.chromium, 'launch', async () => {
    const browser = new EventEmitter();
    const page = new EventEmitter();
    const state = { loads: 0, frames: 0, closed: false };
    pages.push(state);
    const lose = alwaysLose || pages.length === 1;
    browser.newPage = async () => page;
    browser.close = async () => { state.closed = true; };
    page.goto = async () => {};
    page.context = () => ({ browser: () => browser });
    // All shots of one world are drawn in one call; the context is lost on
    // the second shot of the first page.
    page.evaluate = async (fn, specs) => {
      if (fn.toString().includes('__shot.load')) state.loads++;
      if (!fn.toString().includes('__shot.frames')) return;
      return specs.map(() => {
        state.frames++;
        if (lose && state.frames === 2) throw new Error('Screenshot WebGL context lost');
        return `data:image/jpeg;base64,${Buffer.from(`page ${pages.length} frame ${state.frames}`).toString('base64')}`;
      });
    };
    return browser;
  });
  const { renderShots, resolveShot, closeRenderer } = require('../server/shot');
    const shots = (world, views) => views.map((view) => resolveShot({ view }, world));
  t.after(closeRenderer);
  const { World } = require('../server/world');
  const bounds = new World();
  bounds.set(0, 0, 0, 'minecraft:stone');
  bounds.set(1, 1, 1, 'minecraft:stone');
  const images = await renderShots('fixture', shots(bounds, ['front', 'top']));
  assert.deepEqual(images.map(image => image.toString()), ['page 2 frame 1', 'page 2 frame 2']);
  assert.equal(pages.length, 2);
  assert.equal(pages[0].closed, true);
  assert.deepEqual(pages.map(page => page.loads), [1, 1]);

  // A closed renderer stays closed; a fresh one starts from no page.
  await closeRenderer();
  await assert.rejects(renderShots('fixture', shots(bounds, ['front'])), /renderer was shut down/);
  delete require.cache[require.resolve('../server/shot')];
  const fresh = require('../server/shot');
  t.after(fresh.closeRenderer);
  alwaysLose = true;
  await assert.rejects(fresh.renderShots('fixture', shots(bounds, ['front', 'top'])), /WebGL context lost/);
  assert.equal(pages.length, 4, 'persistent context loss must stop after one retry');
  assert.ok(pages.slice(2).every(page => page.closed));
});

test('a page that closed is opened again, and a render queued behind closeRenderer opens no browser', async t => {
  const playwright = require('playwright-core');
  const browsers = [];
  // No headless shell is installed for these: the launch is faked.
  t.mock.method(require('../server/tools'), 'headlessShell', async () => '/no/headless-shell');
  t.mock.method(playwright.chromium, 'launch', async () => {
    const browser = new EventEmitter();
    const page = new EventEmitter();
    browsers.push(page);
    browser.newPage = async () => page;
    browser.close = async () => {};
    page.goto = async () => {};
    page.context = () => ({ browser: () => browser });
    page.evaluate = async (fn, specs) => {
      if (fn.toString().includes('__shot.init')) await hold?.();
      return fn.toString().includes('__shot.frames') ? specs.map(() => `data:image/jpeg;base64,${Buffer.from('x').toString('base64')}`) : undefined;
    };
    return browser;
  });
  let hold = null;
  delete require.cache[require.resolve('../server/shot')];
  const { renderShots, resolveShot, closeRenderer } = require('../server/shot');
  t.after(closeRenderer);
  const { World } = require('../server/world');
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  const shot = () => [resolveShot({ view: 'front' }, world)];
  await renderShots('fixture', shot());
  // The page closes on its own (neither a crash nor a disconnect).
  browsers[0].emit('close');
  await renderShots('fixture', shot());
  assert.equal(browsers.length, 2);
  // A render queued behind a running one, with the renderer closed meanwhile.
  let started;
  let release;
  hold = () => new Promise((resolve) => { started(); release = resolve; });
  const begun = new Promise((resolve) => { started = resolve; });
  const running = renderShots('fixture', shot());
  const queued = renderShots('fixture', shot());
  await begun;
  hold = null;
  const closing = closeRenderer();
  release();
  await running;
  await assert.rejects(queued, /renderer was shut down/);
  await closing;
  assert.equal(browsers.length, 2);
});
