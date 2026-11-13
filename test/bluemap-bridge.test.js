'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

// The BlueMap iframe's bridge (web/bluemap-bridge.js) in a stand-in window:
// the page's messages in, what it posts to the page out, and BlueMap, the
// tile sync and the entity layer as stand-ins that record what they are told.
const SOURCE = fs.readFileSync(path.join(__dirname, '..', 'web', 'bluemap-bridge.js'), 'utf8');

function element() {
  const parts = new Map();
  return {
    hidden: false, style: {}, dataset: {}, textContent: '', innerHTML: '', title: '',
    classList: { toggle() {}, add() {}, remove() {} },
    appendChild(child) { return child; },
    setAttribute() {}, addEventListener() {}, after() {},
    querySelector(selector) {
      if (!parts.has(selector)) parts.set(selector, element());
      return parts.get(selector);
    },
    querySelectorAll() { return []; },
    closest() { return null; },
  };
}

// `broken`: the tile sync's module fails to load, with that message.
// `clock`: { now } the iframe's Date.now reads, for a test that moves time on.
function bridge(t, { broken = null, clock = Date } = {}) {
  const posted = [];
  const listeners = new Map();
  const created = [];
  const groups = [];
  const timers = new Set();
  const cleared = new Set();
  const refreshes = [];
  const patches = [];
  const entitySets = [];
  const animations = [];
  // What the tile sync's next patch fails with, and a refresh it waits for.
  const tiles = { patchFails: null, held: null };
  const later = (handle) => { handle.unref?.(); return handle; };
  const parent = { postMessage: (message) => posted.push(message) };
  const window = {
    parent,
    location: { origin: 'http://app', hash: '', href: 'http://app/bluemap/' },
    navigator: { platform: 'Linux', hardwareConcurrency: 4 },
    addEventListener(type, listener) {
      if (!listeners.has(type)) listeners.set(type, []);
      listeners.get(type).push(listener);
    },
    removeEventListener(type, listener) {
      listeners.set(type, (listeners.get(type) || []).filter((other) => other !== listener));
    },
    Date: { now: () => clock.now() },
    setInterval: (fn, ms) => later(setInterval(fn, ms)),
    clearInterval,
    setTimeout: (fn, ms) => { const handle = later(setTimeout(fn, ms)); timers.add(handle); return handle; },
    clearTimeout: (handle) => { cleared.add(handle); clearTimeout(handle); },
    requestAnimationFrame: (fn) => later(setTimeout(() => fn(performance.now()), 0)),
    cancelAnimationFrame: clearTimeout,
    performance,
    console,
    document: {
      head: element(), body: element(), pointerLockElement: null,
      // The document's listeners are kept apart from the window's.
      addEventListener(type, listener) { window.addEventListener(`document:${type}`, listener); },
      exitPointerLock() { this.pointerLockElement = null; },
      createElement() { const made = element(); created.push(made); return made; },
      getElementById: () => null, querySelector: () => null,
    },
    __import: async (specifier) => {
      if (specifier === '/bluemap-tiles.mjs') {
        if (broken) throw new Error(broken);
        return {
          BlueMapTileSync: class {
            async refresh(view, options) {
              refreshes.push({ view, options });
              await tiles.held;
            }
            async patch(list) {
              patches.push(list);
              if (!tiles.patchFails) return;
              const message = tiles.patchFails;
              tiles.patchFails = null;
              throw new Error(message);
            }
          },
          MesherClient: class { static poolSize() { return 2; } },
        };
      }
      if (specifier === '/bluemap-view.mjs') {
        return { patchViewer() {} };
      }
      if (specifier === '/entities.mjs') {
        return { EntityLayer: class { set(list) { entitySets.push(list); } }, loadEntityAssets: async () => ({}) };
      }
      throw new Error(`no module ${specifier}`);
    },
  };
  window.window = window;
  window.navigator = window.navigator;
  const context = vm.createContext(window);
  vm.runInContext(SOURCE.replace(/\bimport\(/g, '__import('), context);
  // The page's message to the iframe.
  const send = (data) => {
    for (const listener of listeners.get('message') || []) listener({ source: parent, origin: 'http://app', data });
  };
  // An event on the iframe's window, to its listeners in order until one stops it.
  const fire = (type, event) => {
    let stopped = false;
    const fired = { preventDefault() {}, stopImmediatePropagation() { stopped = true; }, ...event };
    for (const listener of [...(listeners.get(type) || [])]) {
      if (stopped) break;
      listener(fired);
    }
  };
  // BlueMap's animate: its frames are the test's to run (finish), and a
  // cancel tells the animation it did not finish.
  const animate = (step, ms, ended) => {
    const animation = {
      ms,
      cancelled: false,
      cancel() { this.cancelled = true; if (ended) ended(false); },
      finish() {
        if (this.cancelled) return;
        step(1);
        if (ended) ended(true);
      },
    };
    animations.push(animation);
    return animation;
  };
  // BlueMap's controls manager: it stops the controls it gives up and starts
  // the ones it is given.
  class ControlsManager {
    constructor() {
      this.held = null;
      this.position = { x: 1, y: 70, z: 1, set(x, y, z) { this.placed = [x, y, z]; } };
      this.distance = 100;
      this.rotation = 0;
      this.angle = 1;
    }
    set controls(given) {
      if (this.held && this.held.stop) this.held.stop();
      this.held = given;
      if (given && given.start) given.start(this);
    }
    get controls() { return this.held; }
    update() {}
    updateCamera() {}
  }
  class Vector3 {
    constructor(x, y, z) { Object.assign(this, { x, y, z }); }
    clone() { return new Vector3(this.x, this.y, this.z); }
    cross() { return this; }
    applyAxisAngle() { return this; }
    normalize() { return this; }
    multiplyScalar() { return this; }
    sub() { return this; }
    dot(other) { return this.x * other.x + this.y * other.y + this.z * other.z; }
    negate() { return Object.assign(this, { x: -this.x, y: -this.y, z: -this.z }); }
    addScaledVector(other, scale) { return Object.assign(this, { x: this.x + other.x * scale, y: this.y + other.y * scale, z: this.z + other.z * scale }); }
    floor() { return Object.assign(this, { x: Math.floor(this.x), y: Math.floor(this.y), z: Math.floor(this.z) }); }
  }
  class Vector2 {
    constructor(x, y) { Object.assign(this, { x, y }); }
  }
  // A group of the marker scene: what the bridge adds to it and takes out.
  class Group {
    constructor() { this.children = []; groups.push(this); }
    add(child) { this.children.push(child); }
    remove(child) { this.children = this.children.filter((other) => other !== child); }
  }
  class ExtrudeMarker {
    constructor(id) { this.id = id; }
    updateFromData(data) { this.data = data; }
    traverse() {}
    dispose() {}
  }
  // BlueMap, ready: its app on the map controls, and its moves between modes.
  const ready = () => {
    const manager = new ControlsManager();
    const viewer = {
      data: { loadedCenter: { x: 0, y: 0 }, loadedLowresViewDistance: 0 },
      map: Object.assign(Object.create({ terrainHeightAt: () => 0 }), { isLoaded: true, loadMapArea() {}, hiresMaterial: null }),
      renderer: { setOpaqueSort() {}, domElement: element() },
      render() {}, redraw() {}, updateLoadedMapArea() {},
      // BlueMap's raycast: the top of the block at x, 63, z under the screen point x, z.
      handleMapInteraction(point) {
        fire('bluemapMapInteraction', {
          stopPropagation() {},
          detail: { hiresHit: { face: { normal: new Vector3(0, 1, 0) }, point: new Vector3(point.x, 64, point.y) }, ray: { direction: new Vector3(0, -1, 0) } },
        });
      },
      controlsManager: manager,
      camera: { fov: 70, aspect: 1, updateProjectionMatrix() {} },
      markers: { add() {} },
      stats: { begin() {}, update() {} },
    };
    const app = {
      mapViewer: viewer,
      events: { addEventListener() {} },
      mapControls: { map: true },
      freeFlightControls: { spectator: true },
      appState: { controls: { state: 'perspective' } },
      viewAnimation: null,
      perspectiveViews: 0,
      // A move between modes, as BlueMap makes it: the last one is cancelled,
      // the controls come off at once, and the new mode's go on when the
      // move ends, unless it was cancelled.
      move(ms, state, controls) {
        if (this.viewAnimation) this.viewAnimation.cancel();
        manager.controls = null;
        this.viewAnimation = animate(() => {}, ms, (finished) => { if (finished) manager.controls = controls(); });
        this.appState.controls.state = state;
      },
      setPerspectiveView(ms) {
        this.perspectiveViews += 1;
        this.move(ms, 'perspective', () => this.mapControls);
      },
      setFreeFlight(ms) {
        this.move(ms, 'free', () => this.freeFlightControls);
      },
    };
    manager.controls = app.mapControls;
    window.BlueMap = { Three: { Group, Vector3, Vector2 }, ExtrudeMarker, animate, EasingFunctions: { easeInOutQuad: (progress) => progress } };
    window.bluemap = app;
    return app;
  };
  // Player mode, as the end of BlueMap's move to free flight starts it.
  const enterPlayer = (app) => {
    const manager = app.mapViewer.controlsManager;
    manager.position.placed = undefined;
    manager.controls = app.freeFlightControls;
    return manager;
  };
  const until = async (check, what) => {
    for (let i = 0; i < 500; i += 1) {
      if (check()) return;
      await new Promise((resolve) => setTimeout(resolve, 5));
    }
    throw new Error(`Timed out waiting for ${what}`);
  };
  const of = (type) => posted.filter((message) => message.type === type);
  // The boxes drawn over the map: the selections', and the drag's.
  const boxes = () => groups.flatMap((group) => group.children).filter((child) => child instanceof ExtrudeMarker);
  // The player HUD, once the bridge has made it.
  const hud = () => created.find((made) => made.id === 'mcbench-player');
  t.after(() => { delete window.bluemap; });
  return { send, fire, ready, enterPlayer, posted, of, until, refreshes, patches, entitySets, animations, tiles, timers, cleared, boxes, hud, document: window.document };
}

const view = (x) => {
  const v = new Float64Array(28);
  v[0] = x; v[3] = x; v[1] = 64; v[4] = 64; v[15] = 1.62;
  return v;
};

test('the iframe says it is ready only once BlueMap and the tile sync are', async (t) => {
  const b = bridge(t);
  b.send({ type: 'mcbench-lite:ping' });
  await new Promise((resolve) => setTimeout(resolve, 40));
  assert.deepEqual(b.of('mcbench-lite:bluemap-ready'), [], 'a ping before BlueMap is ready goes unanswered');
  b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.send({ type: 'mcbench-lite:ping' });
  assert.equal(b.of('mcbench-lite:bluemap-ready').length, 2, 'a ping once ready is answered');
});

test('a viewer that could not start says so with why, once and when asked, and never that it is ready', async (t) => {
  const b = bridge(t, { broken: 'The tile sync failed to load' });
  b.ready();
  await b.until(() => b.of('mcbench-lite:boot-failed').length === 1, 'the failed boot told');
  assert.equal(JSON.stringify(b.of('mcbench-lite:boot-failed')[0]), JSON.stringify({ type: 'mcbench-lite:boot-failed', error: 'The tile sync failed to load' }));
  await new Promise((resolve) => setTimeout(resolve, 40));
  assert.equal(b.of('mcbench-lite:boot-failed').length, 1, 'told once');
  // The page asks once the iframe has loaded: it may have missed the telling.
  b.send({ type: 'mcbench-lite:ping' });
  assert.equal(b.of('mcbench-lite:boot-failed').length, 2);
  assert.equal(b.of('mcbench-lite:bluemap-ready').length, 0, 'a viewer that did not start is not ready');
});

test('BlueMap not ready in time is a failed boot, told to the page', async (t) => {
  const clock = { at: Date.now(), now() { return this.at; } };
  const b = bridge(t, { clock });
  await new Promise((resolve) => setTimeout(resolve, 40));
  assert.deepEqual(b.posted, [], 'nothing is told while it boots');
  clock.at += 20001;
  await b.until(() => b.of('mcbench-lite:boot-failed').length === 1, 'the failed boot told');
  assert.equal(b.of('mcbench-lite:boot-failed')[0].error, 'BlueMap viewer did not become ready');
  assert.equal(b.of('mcbench-lite:bluemap-ready').length, 0);
});

test('a refresh clears the entities of the view before, and a frame with nothing since its base draws only entities', async (t) => {
  const b = bridge(t);
  b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.send({ type: 'mcbench-lite:refresh', key: 'a:1', blueprint: 'blob:1', fresh: true });
  await b.until(() => b.of('mcbench-lite:refreshed').length === 1, 'a shown');
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([1]), entities: [{ id: 'cow' }] });
  await b.until(() => b.patches.length === 1, 'the patch');
  assert.deepEqual(b.entitySets.at(-1), [{ id: 'cow' }]);
  b.send({ type: 'mcbench-lite:refresh', key: 'b:2', blueprint: 'blob:2', fresh: true });
  assert.equal(JSON.stringify(b.entitySets.at(-1)), '[]', 'another view clears the entities at once (an array of the iframe\'s)');
  b.send({ type: 'mcbench-lite:frame', key: 'b:2', patch: null, entities: [{ id: 'pig' }] });
  await b.until(() => b.of('mcbench-lite:refreshed').length === 2, 'b shown');
  assert.deepEqual(b.entitySets.at(-1), [{ id: 'pig' }]);
  assert.equal(b.patches.length, 1, 'a frame without a patch patches nothing');
});

test('every end of player mode is told to the page as a leave, the world letting the player go included', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.enterPlayer(app);
  assert.equal(b.of('mcbench-lite:player-spawn').length, 1);
  b.send({ type: 'mcbench-lite:player', view: view(2) });
  b.send({ type: 'mcbench-lite:player', gone: true });
  assert.equal(app.perspectiveViews, 1, 'back to the map');
  // A spawn may still be on its way: the page's removal waits for it.
  assert.equal(b.of('mcbench-lite:player-leave').length, 1, 'the leave goes to the page');
  // Gone again with no player: nothing.
  b.send({ type: 'mcbench-lite:player', gone: true });
  assert.equal(app.perspectiveViews, 1);
  assert.equal(b.of('mcbench-lite:player-leave').length, 1);
  b.enterPlayer(app);
  b.send({ type: 'mcbench-lite:player', view: view(3) });
  app.setPerspectiveView(500);
  assert.equal(b.of('mcbench-lite:player-leave').length, 2);
});

test('the world letting the player go while its respawn is on its way still sends the leave that removes it', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.enterPlayer(app);
  b.send({ type: 'mcbench-lite:player', view: view(2) });
  // It falls out of the world: a respawn is asked and not answered yet.
  const fallen = view(2);
  fallen[1] = -200;
  b.send({ type: 'mcbench-lite:player', view: fallen, tick: 1 });
  assert.equal(b.of('mcbench-lite:player-spawn').length, 2);
  b.send({ type: 'mcbench-lite:player', gone: true });
  assert.equal(b.of('mcbench-lite:player-leave').length, 1);
  // The respawn's answer, after the leave: the camera stays on the map.
  b.send({ type: 'mcbench-lite:player', view: view(5) });
  assert.equal(app.mapViewer.controlsManager.controls, null);
});

test('the answer to a spawn superseded by a later one, and the ticks before it, leave the camera alone', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.enterPlayer(app);
  app.setPerspectiveView(500);
  const manager = b.enterPlayer(app);
  assert.equal(b.of('mcbench-lite:player-spawn').length, 2);
  // The first spawn's answer and a tick of its player, then the second's.
  b.send({ type: 'mcbench-lite:player', view: view(10) });
  b.send({ type: 'mcbench-lite:player', view: view(11), tick: 1 });
  manager.controls.update();
  assert.equal(manager.position.placed, undefined, 'no view yet: the camera waits for the spawn');
  b.send({ type: 'mcbench-lite:player', view: view(20) });
  manager.controls.update();
  assert.equal(manager.position.placed[0], 20);
  // Ticks of the player now in the world move it.
  b.send({ type: 'mcbench-lite:player', view: view(21), tick: 1 });
  manager.controls.update();
  assert.equal(manager.position.placed[0], 21);
});

test('a restart of the same build keeps player mode: a new base, and ticks counting again from 1, move the player on', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.send({ type: 'mcbench-lite:refresh', key: 'a:1', blueprint: 'blob:1', fresh: true });
  await b.until(() => b.of('mcbench-lite:refreshed').length === 1, 'a shown');
  const manager = b.enterPlayer(app);
  b.send({ type: 'mcbench-lite:player', view: view(10) });
  b.send({ type: 'mcbench-lite:player', view: view(11), tick: 400 });
  // The world placed afresh: a new base of the same build, and the player's ticks from 1.
  b.send({ type: 'mcbench-lite:refresh', key: 'a:2', blueprint: 'blob:2', fresh: false });
  await b.until(() => b.of('mcbench-lite:refreshed').length === 2, 'the new base shown');
  b.send({ type: 'mcbench-lite:player', view: view(12), tick: 1 });
  manager.controls.update();
  assert.equal(manager.controls, app.freeFlightControls, 'still in player mode');
  assert.equal(manager.position.placed[0], 12);
  assert.equal(app.perspectiveViews, 0, 'the camera stays with the player');
  assert.equal(b.hud().querySelector('.mcbench-player-note').hidden, true, 'no note');
  assert.equal(b.of('mcbench-lite:player-leave').length, 0);
});

test('a player note replaces the one before, and the earlier one\'s timer with it', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.enterPlayer(app);
  const before = new Set(b.timers);
  b.send({ type: 'mcbench-lite:player', error: 'The first note' });
  const first = [...b.timers].find((handle) => !before.has(handle));
  b.enterPlayer(app);
  b.send({ type: 'mcbench-lite:player', error: 'The second note' });
  assert.ok(b.cleared.has(first), 'the first note\'s timer is cancelled');
});

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

test('a patch that fails is told once, and the same view asked for again loads afresh and takes its ticks', async (t) => {
  const b = bridge(t);
  b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.send({ type: 'mcbench-lite:refresh', key: 'a:1', blueprint: 'blob:1', fresh: true });
  await b.until(() => b.of('mcbench-lite:refreshed').length === 1, 'a shown');
  b.tiles.patchFails = 'The mesher stopped: out of memory';
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([1]), entities: [] });
  await b.until(() => b.of('mcbench-lite:refresh-error').length === 1, 'the failure told');
  assert.equal(JSON.stringify(b.of('mcbench-lite:refresh-error')[0]), JSON.stringify({ type: 'mcbench-lite:refresh-error', key: 'a:1', error: 'The mesher stopped: out of memory' }));
  // Ticks of the view it could not show are dropped.
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([2]), entities: [] });
  await pause(30);
  assert.equal(b.patches.length, 1);
  // The page asks for the same view again, and sends what the world did
  // since its base: nothing the viewer holds is kept.
  b.send({ type: 'mcbench-lite:refresh', key: 'a:1', blueprint: 'blob:1', fresh: false });
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([3]), entities: [{ id: 'cow' }] });
  await b.until(() => b.of('mcbench-lite:refreshed').length === 2, 'a shown again');
  assert.equal(b.refreshes.length, 2);
  assert.equal(b.refreshes[1].options.fresh, true, 'the tiles are loaded afresh');
  await b.until(() => b.patches.length === 2, 'the patch since the base');
  assert.deepEqual([...b.patches[1][0]], [3]);
  // And the ticks after it apply again.
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([4]), entities: [] });
  await b.until(() => b.patches.length === 3, 'the next tick');
  assert.equal(b.of('mcbench-lite:refresh-error').length, 1, 'the failure was told once');
});

test('a view asked for again while it loads loads once, and one left while it loads answers nothing', async (t) => {
  const b = bridge(t);
  b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  let release;
  b.tiles.held = new Promise((resolve) => { release = resolve; });
  b.send({ type: 'mcbench-lite:refresh', key: 'a:1', blueprint: 'blob:1', fresh: true });
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([1]), entities: [] });
  await b.until(() => b.refreshes.length === 1, 'a loading');
  // The page asks again until it is answered, and sends what happened since the base again.
  b.send({ type: 'mcbench-lite:refresh', key: 'a:1', blueprint: 'blob:1', fresh: true });
  b.send({ type: 'mcbench-lite:frame', key: 'a:1', patch: new Uint8Array([2]), entities: [] });
  // Another view, while a still loads.
  b.send({ type: 'mcbench-lite:refresh', key: 'a:2', blueprint: 'blob:2', fresh: false });
  b.tiles.held = null;
  release();
  await b.until(() => b.of('mcbench-lite:refreshed').length === 1, 'the other view shown');
  await pause(30);
  assert.deepEqual(b.refreshes.map(({ view }) => view.key), ['a:1', 'a:2'], 'a loaded once');
  assert.deepEqual(b.of('mcbench-lite:refreshed').map(({ key }) => key), ['a:2']);
  assert.deepEqual(b.patches, [], 'the ticks of the view left are dropped');
});

test('a fly asked for while BlueMap moves between modes waits for the move to end, which nothing cancels', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  const manager = app.mapViewer.controlsManager;
  // BlueMap's perspective button: the controls come off for the move.
  app.setPerspectiveView(500);
  const move = app.viewAnimation;
  assert.equal(manager.controls, null);
  b.send({ type: 'mcbench-lite:fly', min: [0, 64, 0], max: [4, 68, 4] });
  assert.equal(move.cancelled, false, 'BlueMap\'s move goes on');
  assert.deepEqual(b.animations, [move], 'the fly has not started');
  move.finish();
  assert.equal(manager.controls, app.mapControls, 'the camera takes input again');
  assert.equal(b.animations.length, 2, 'the fly runs once the move has ended');
  assert.equal(b.animations[1].ms, 700);
  // Into player mode: the fly asked during the move stands the player before the build.
  app.setFreeFlight(500);
  b.send({ type: 'mcbench-lite:fly', min: [0, 64, 0], max: [4, 68, 4] });
  assert.equal(app.viewAnimation.cancelled, false);
  assert.equal(b.of('mcbench-lite:player-spawn').length, 0);
  app.viewAnimation.finish();
  assert.equal(manager.controls, app.freeFlightControls);
  assert.equal(b.of('mcbench-lite:player-spawn').length, 2, 'player mode\'s spawn, then the fly\'s');
  // With the mode settled a fly runs at once.
  b.send({ type: 'mcbench-lite:fly', min: [0, 64, 0], max: [4, 68, 4] });
  assert.equal(b.of('mcbench-lite:player-spawn').length, 3);
});

const key = (code, extra) => ({ code, key: code, metaKey: false, altKey: false, ctrlKey: false, shiftKey: false, repeat: false, target: element(), ...extra });

test('a key the player holds is let go by its keyup whatever else is held by then, and a key two share is held while either is', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.enterPlayer(app);
  const inputs = () => b.of('mcbench-lite:player-input').map(({ keys, taps }) => [keys, taps]);
  // W held, Alt pressed, W let go.
  b.fire('keydown', key('KeyW'));
  b.fire('keydown', key('AltLeft', { altKey: true }));
  b.fire('keyup', key('KeyW', { altKey: true }));
  assert.deepEqual(inputs(), [[1, 1], [0, 0]]);
  // A key pressed with Alt down is not the player's, nor is its keyup.
  b.fire('keydown', key('KeyA', { altKey: true }));
  b.fire('keyup', key('KeyA', { altKey: true }));
  assert.equal(inputs().length, 2);
  // D let go over a field the focus moved to.
  b.fire('keydown', key('KeyD'));
  b.fire('keyup', key('KeyD', { target: { closest: () => ({}) } }));
  assert.deepEqual(inputs().slice(2), [[8, 8], [0, 0]]);
  // Both shifts: the second adds nothing, and sneak lasts until both are let go.
  b.fire('keydown', key('ShiftLeft'));
  b.fire('keydown', key('ShiftRight'));
  assert.deepEqual(inputs().slice(4), [[32, 32]]);
  b.fire('keyup', key('ShiftLeft'));
  assert.equal(inputs().at(-1)[0], 32, 'the right shift still holds it');
  b.fire('keyup', key('ShiftRight'));
  assert.equal(inputs().at(-1)[0], 0);
  // A repeat and a second press of a held key send nothing, and a blur lets every key go.
  b.fire('keydown', key('Space'));
  b.fire('keydown', key('Space', { repeat: true }));
  b.fire('keydown', key('ControlLeft'));
  const sent = inputs().length;
  assert.deepEqual(inputs().slice(-2), [[16, 16], [16 | 64, 64]]);
  b.fire('blur', {});
  assert.deepEqual(inputs().slice(sent), [[0, 0]]);
  b.fire('blur', {});
  assert.equal(inputs().length, sent + 1);
});

test('a note shown as player mode ends is drawn without the crosshair and hint of the player that left', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.enterPlayer(app);
  // The player takes the mouse.
  b.document.pointerLockElement = app.mapViewer.renderer.domElement;
  b.fire('document:pointerlockchange', {});
  const hud = b.hud();
  const crosshair = hud.querySelector('.mcbench-crosshair');
  const hint = hud.querySelector('.mcbench-player-hint');
  const note = hud.querySelector('.mcbench-player-note');
  assert.equal(hud.hidden, false);
  assert.equal(crosshair.hidden, false);
  assert.match(hint.textContent, /Esc frees the mouse/);
  // The world lets the player go: player mode ends with a note.
  b.send({ type: 'mcbench-lite:player', gone: true });
  assert.equal(b.document.pointerLockElement, null);
  assert.equal(hud.hidden, false, 'the note shows');
  assert.equal(note.hidden, false);
  assert.equal(note.textContent, 'The player is no longer in the world.');
  assert.equal(crosshair.hidden, true, 'no crosshair without a player');
  assert.equal(hint.hidden, true, 'nor the hint');
  // In player mode again, the hint is the one for a mouse not taken.
  b.enterPlayer(app);
  assert.equal(hint.hidden, false);
  assert.equal(hint.textContent, 'Click to look around');
  assert.equal(crosshair.hidden, true);
});

const ui = (extra) => ({ type: 'mcbench-lite:ui', labels: [], selections: [], binds: [], selectable: true, playable: true, ...extra });

test('an Escape the viewer has no use for goes to the page, which closes its topmost layer: the viewer closes none itself', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  const escapes = () => b.of('mcbench-lite:escape').length;
  // Filling the window: the page decides what Escape closes.
  b.send(ui({ full: true }));
  b.fire('keydown', key('Escape'));
  assert.equal(escapes(), 1);
  assert.deepEqual(b.of('mcbench-lite:full'), [], 'the viewer does not leave the window by itself');
  // Beside the chat too: a layer may be open over the app.
  b.send(ui({ full: false }));
  b.fire('keydown', key('Escape'));
  assert.equal(escapes(), 2);
  // While selecting, Escape stops selecting, and the next one is the page's.
  b.send({ type: 'mcbench-lite:toggle-select' });
  b.fire('keydown', key('Escape'));
  assert.equal(escapes(), 2);
  b.fire('keydown', key('Escape'));
  assert.equal(escapes(), 3);
  // In player mode with the mouse taken, Escape frees the mouse.
  b.enterPlayer(app);
  b.document.pointerLockElement = app.mapViewer.renderer.domElement;
  b.fire('keydown', key('Escape'));
  assert.equal(escapes(), 3);
  b.document.pointerLockElement = null;
  b.fire('keydown', key('Escape'));
  assert.equal(escapes(), 4);
});

test('a selection drag is its pointer\'s: another pointer starts no second box, and a drag whose release was lost ends when its pointer goes down again', async (t) => {
  const b = bridge(t);
  const app = b.ready();
  await b.until(() => b.of('mcbench-lite:bluemap-ready').length === 1, 'ready');
  b.send(ui());
  b.send({ type: 'mcbench-lite:toggle-select' });
  const canvas = app.mapViewer.renderer.domElement;
  const pointer = (pointerId, x, z) => ({ pointerId, button: 0, target: canvas, clientX: x, clientY: z, stopPropagation() {} });
  const selections = () => b.of('mcbench-lite:selection').map(({ min, max }) => [[...min], [...max]]);
  b.fire('pointerdown', pointer(1, 2, 3));
  assert.equal(b.boxes().length, 1);
  // A second touch: no drag of its own, and its moves and release are not the drag's.
  b.fire('pointerdown', pointer(2, 9, 9));
  assert.equal(b.boxes().length, 1, 'one box');
  b.fire('pointermove', pointer(2, 20, 20));
  await pause(10);
  b.fire('pointerup', pointer(2, 20, 20));
  assert.deepEqual(selections(), []);
  assert.equal(b.boxes().length, 1, 'the drag goes on');
  b.fire('pointermove', pointer(1, 5, 7));
  await pause(10);
  b.fire('pointerup', pointer(1, 5, 7));
  assert.deepEqual(selections(), [[[2, 63, 3], [5, 63, 7]]]);
  assert.equal(b.boxes().length, 0, 'the box goes with the drag');
  // A release outside the frame never arrives: the pointer's next press starts over.
  b.fire('pointerdown', pointer(1, 2, 3));
  b.fire('pointerdown', pointer(1, 8, 8));
  assert.equal(b.boxes().length, 1, 'the lost drag\'s box is gone');
  b.fire('pointerup', pointer(1, 8, 8));
  assert.deepEqual(selections().slice(1), [[[8, 63, 8], [8, 63, 8]]]);
  assert.equal(b.boxes().length, 0);
});
