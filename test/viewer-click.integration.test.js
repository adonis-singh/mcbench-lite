'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { chromium } = require('playwright-core');
const { token } = require('../server/auth');
const tools = require('../server/tools');
const { blocksBlueprint } = require('../server/blueprint');
const { renderMask } = require('../server/bluemap');

// BlueMap's own web app, as the server bakes it (no lowres level), with the
// app's bridge, opened on its own: the bridge's parent is then its own
// window, so the test speaks the page's messages to it and hears its
// answers. A click on a lever must name the lever's block to the page, and
// BlueMap's own tap must reach its listeners with the hires hit; neither may
// throw. Run it with the data folder of the server MCBENCH_UI_URL names (its
// token) and that server's tools folder.
test(
  'a click on a lever in the real BlueMap viewer names the lever to the page',
  {
    skip: process.env.MCBENCH_UI_INTEGRATION !== '1',
    timeout: 90000,
  },
  async (t) => {
    // The app under test is named, never assumed: the default port is the
    // one a person's own app runs on.
    if (!process.env.MCBENCH_UI_URL) throw new Error('Set MCBENCH_UI_URL to the address of the running app to test (and MCBENCH_DATA_DIR to its data folder)');
    const origin = new URL(process.env.MCBENCH_UI_URL).origin;
    const headers = { authorization: `Bearer ${token()}` };
    const status = await (await fetch(`${origin}/api/minecraft/status`, { headers })).json();
    assert.equal(status.viewerError, null);

    // A redstone lamp, the stone beside it, and a floor lever on the stone, at Minecraft heights.
    const blocks = [
      [0, 64, 0, 'minecraft:redstone_lamp[lit=false]'],
      [1, 64, 0, 'minecraft:stone'],
      [1, 65, 0, 'minecraft:lever[face=floor,facing=north,powered=false]'],
    ];
    const bounds = { min: [0, 64, 0], max: [1, 65, 0] };

    const browser = await chromium.launch({ executablePath: await tools.headlessShell(), args: ['--enable-unsafe-swiftshader'] });
    t.after(() => browser.close());
    const page = await browser.newPage({ viewport: { width: 900, height: 700 }, extraHTTPHeaders: headers });
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.stack || error.message));
    // What the bridge tells its parent, here its own window.
    await page.addInitScript(() => {
      window.__told = [];
      window.addEventListener('message', (event) => window.__told.push(event.data));
    });
    const told = (type) => page.evaluate((type) => window.__told.filter((message) => message.type === type), type);
    const until = async (check, what) => {
      for (let i = 0; i < 600; i++) {
        if (await check()) return;
        await page.waitForTimeout(50);
      }
      throw new Error(`Timed out waiting for ${what}`);
    };
    await page.goto(`${origin}${status.viewerUrl}`, { waitUntil: 'domcontentloaded' });
    await until(async () => (await told('mcbench-lite:bluemap-ready')).length > 0, 'the viewer to be ready');

    await page.evaluate(({ bytes, mask, mesher, bounds }) => {
      const post = (message) => window.postMessage(message, window.location.origin);
      post({ type: 'mcbench-lite:ui', exportable: false, playable: false, download: null, labels: [], selectable: true, selections: [], binds: [], typeAhead: false, full: false, bounds });
      post({
        type: 'mcbench-lite:refresh',
        key: 'click:1',
        blueprint: URL.createObjectURL(new Blob([new Uint8Array(bytes)])),
        mask,
        wasm: mesher.wasm,
        pack: mesher.pack,
        frame: null,
        fresh: true,
      });
    }, { bytes: [...blocksBlueprint(blocks)], mask: renderMask(bounds), mesher: status.mesher, bounds });
    await until(async () => (await told('mcbench-lite:refreshed')).length > 0, 'the world to be shown');

    // The camera south of the build, looking down at it; BlueMap's own
    // taps tell its listeners what they hit.
    await page.evaluate(() => {
      const viewer = window.bluemap.mapViewer;
      const manager = viewer.controlsManager;
      manager.position.set(1, 64.5, 0.5);
      manager.rotation = 0.35;
      manager.angle = 1.05;
      manager.distance = 9;
      viewer.redraw();
      window.__taps = [];
      window.bluemap.events.addEventListener('bluemapMapInteraction', (event) => {
        const hit = event.detail.hiresHit;
        window.__taps.push(hit && hit.point.clone().addScaledVector(event.detail.ray.direction, 0.01).floor().toArray());
      });
    });
    await page.waitForTimeout(1500);
    const onScreen = (point) => page.evaluate((point) => {
      const viewer = window.bluemap.mapViewer;
      viewer.camera.updateMatrixWorld();
      const v = new window.BlueMap.Three.Vector3(...point).project(viewer.camera);
      const rect = viewer.renderer.domElement.getBoundingClientRect();
      return { x: rect.left + (v.x + 1) / 2 * rect.width, y: rect.top + (1 - v.y) / 2 * rect.height };
    }, point);

    const lever = await onScreen([1.5, 65.12, 0.5]);
    await page.mouse.click(lever.x, lever.y);
    await until(async () => (await told('mcbench-lite:interact')).length > 0, 'the click to be told');
    const [interact] = await told('mcbench-lite:interact');
    assert.deepEqual([interact.x, interact.y, interact.z], [1, 65, 0], 'the lever\'s block');
    await until(() => page.evaluate(() => window.__taps.length > 0), 'BlueMap\'s tap');
    assert.deepEqual(await page.evaluate(() => window.__taps), [[1, 65, 0]], 'BlueMap\'s own tap hits the lever too');

    // A click on the sky names nothing.
    await page.mouse.move(10, 10);
    await page.waitForTimeout(600);
    await page.mouse.click(20, 120);
    await page.waitForTimeout(800);
    assert.equal((await told('mcbench-lite:interact')).length, 1);

    // The selection tool: a drag from the lamp to the lever selects both.
    const lamp = await onScreen([0.5, 64.5, 1.0]);
    await page.click('#mcbench-select');
    await page.mouse.move(lamp.x, lamp.y);
    await page.mouse.down();
    await page.mouse.move((lamp.x + lever.x) / 2, (lamp.y + lever.y) / 2);
    await page.waitForTimeout(100);
    await page.mouse.move(lever.x, lever.y);
    await page.waitForTimeout(100);
    await page.mouse.up();
    await until(async () => (await told('mcbench-lite:selection')).length > 0, 'the selection');
    const [selection] = await told('mcbench-lite:selection');
    assert.deepEqual([selection.min, selection.max], [[0, 64, 0], [1, 65, 0]]);

    assert.deepEqual(errors, []);
  },
);
