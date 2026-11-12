'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { chromium } = require('playwright-core');
const { World } = require('../server/world');

// Uses the app's static assets, but supplies an isolated world over a browser
// route. No model calls or mutations to saved builds are made by this test.
// The renderer sends the access token of MCBENCH_DATA_DIR (server/auth.js):
// run it with the data folder of the server MCBENCH_UI_URL names.
test(
  'real WebGL context loss rebuilds the renderer and reproduces all views',
  {
    skip: process.env.MCBENCH_UI_INTEGRATION !== '1',
    timeout: 60000,
  },
  async (t) => {
    const originalPort = process.env.PORT;
    process.env.PORT = new URL(
      process.env.MCBENCH_UI_URL || 'http://127.0.0.1:3020',
    ).port;
    t.after(() => {
      if (originalPort === undefined) delete process.env.PORT;
      else process.env.PORT = originalPort;
    });
    const world = new World();
    for (let x = 0; x < 3; x++) {
      for (let y = 0; y < 3; y++)
        world.set(
          x * 2,
          y,
          0,
          `minecraft:${['red', 'blue', 'yellow'][x]}_concrete`,
          {},
        );
    }
    const events = [
      { type: 'meta', bounds: world.bounds(), removed: [], renderLod: null },
      ...[...world.chunks.keys()].map((key) => ({
        type: 'chunk',
        chunk: world.chunkSnapshot(key),
      })),
    ];
    const launch = chromium.launch.bind(chromium);
    let launches = 0;
    let captureCount = 0;
    let loseContext = false;
    const loadRequests = [];
    t.mock.method(chromium, 'launch', async (options) => {
      launches++;
      const browser = await launch(options);
      const newPage = browser.newPage.bind(browser);
      browser.newPage = async (options) => {
        const page = await newPage(options);
        await page.route(
          '**/api/conversations/context-fixture/world',
          (route) => {
            loadRequests.push(route.request().postDataJSON());
            return route.fulfill({
              contentType: 'application/x-ndjson',
              body:
                events.map((event) => JSON.stringify(event)).join('\n') + '\n',
            });
          },
        );
        const evaluate = page.evaluate.bind(page);
        page.evaluate = async (fn, arg) => {
          if (
            fn.toString().includes('__shot.frames') &&
            loseContext &&
            ++captureCount === 1
          ) {
            await evaluate(() =>
              document
                .querySelector('#c')
                .getContext('webgl2')
                .getExtension('WEBGL_lose_context')
                .loseContext(),
            );
          }
          return evaluate(fn, arg);
        };
        return page;
      };
      return browser;
    });
    const { renderShots, resolveShot, closeRenderer } = require('../server/shot');
    const shots = (shown, views) => views.map((view) => resolveShot({ view }, shown));
    t.after(closeRenderer);
    const angles = ['iso_se', 'iso_nw', 'front', 'top'];
    const baseline = await renderShots(
      'context-fixture',
      shots(world, angles),
    );
    const baselineLaunches = launches;
    loseContext = true;
    const recovered = await renderShots(
      'context-fixture',
      shots(world, angles),
    );
    assert.equal(launches, baselineLaunches + 1);
    assert.equal(recovered.length, 4);
    assert.deepEqual(
      loadRequests.at(-1).known,
      {},
      'a fresh context needs a full world reload',
    );
    recovered.forEach((image, i) => {
      assert.ok(
        image.length > 10000,
        'fixture should render more than a blank placeholder',
      );
      assert.deepEqual(
        image,
        baseline[i],
        `recovered ${angles[i]} must match the healthy rendering`,
      );
    });
  },
);
