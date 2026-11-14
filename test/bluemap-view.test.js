'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

// The viewer's click cast (web/bluemap-view.mjs) on a map baked as the app
// bakes it, with no lowres level: a stand-in scene graph of BlueMap objects
// and a raycaster stand-in that answers the intersections a test gives it.

// A BlueMap Object3D: its onClick bubbles to its parent, as BlueMap's does,
// unless the test gives it one that takes the click.
function object(parent, { visible = true, depthTest = true, takes = false } = {}) {
  const made = {
    parent, visible, material: { depthTest }, clicks: [],
    onClick(event) {
      this.clicks.push(event);
      return takes || (this.parent ? this.parent.onClick(event) : false);
    },
  };
  return made;
}

function viewer(intersections) {
  const sceneParent = object(null);
  const scene = Object.assign(object(sceneParent), { position: { x: -10000, y: 0, z: 10000 }, updated: 0, updateMatrixWorld() { this.updated += 1; } });
  const markers = object(null);
  const cast = {};
  const ray = { direction: { x: 0, y: -1, z: 0 } };
  const raycaster = {
    ray,
    setFromCamera(coords, camera) { cast.coords = { ...coords }; cast.camera = camera; },
    intersectObjects(objects, recursive) {
      cast.objects = objects;
      cast.recursive = recursive;
      cast.scenePosition = { ...scene.position };
      cast.sceneUpdated = scene.updated;
      return intersections({ scene, markers });
    },
  };
  const camera = { updated: 0, updateMatrixWorld() { this.updated += 1; } };
  // BlueMap's map, whose prototype patchViewer gives its terrain height.
  const map = Object.assign(Object.create({ terrainHeightAt: () => 0 }), {
    isLoaded: true,
    hiresTileManager: { scene, sceneParent },
    loadMapArea() {},
  });
  // The bake has no lowres level; reading one is the defect this guards.
  Object.defineProperty(map, 'lowresTileManager', { get() { throw new Error('a lowres level was read'); } });
  return {
    view: {
      map, camera, raycaster, markers,
      rootElement: { clientWidth: 200, clientHeight: 100, getBoundingClientRect: () => ({ left: 100, top: 50 }) },
    },
    scene, markers, cast, ray,
  };
}

const hitOn = (target, distance) => ({ object: target, distance, point: { x: 1.5, y: 65.1, z: 0.5 } });

test('the cast answers the hires hit on a map with no lowres levels', async () => {
  const { mapInteraction } = await import('../web/bluemap-view.mjs');
  let tile;
  let entry;
  const v = viewer(({ scene }) => {
    tile = object(scene);
    entry = hitOn(tile, 3);
    return [entry];
  });
  const data = { doubleTap: false };
  const detail = mapInteraction(v.view, { x: 150, y: 75 }, data);
  assert.deepEqual(v.cast.coords, { x: -0.5, y: 0.5 }, 'the screen point within the viewer, as BlueMap places it');
  assert.equal(v.cast.camera, v.view.camera);
  assert.equal(v.view.camera.updated, 1);
  assert.deepEqual(v.cast.objects, [v.scene, v.markers], 'the hires tiles and the markers');
  assert.equal(v.cast.recursive, true);
  assert.deepEqual(v.cast.scenePosition, { x: 0, y: 0, z: 0 }, 'the hires scene is back in world coordinates before the cast');
  assert.equal(v.cast.sceneUpdated, 1);
  assert.equal(detail.hiresHit, entry);
  assert.equal(detail.hit, entry);
  assert.equal(detail.ray, v.ray);
  assert.equal(detail.data, data);
  assert.deepEqual(detail.intersections, [entry]);
  assert.equal(tile.clicks.length, 1, 'the first visible hit is offered to its object');
});

test('a hit on something hidden, itself or by an ancestor, is skipped', async () => {
  const { mapInteraction } = await import('../web/bluemap-view.mjs');
  let hiddenTile;
  let behind;
  const v = viewer(({ scene, markers }) => {
    const hiddenGroup = object(markers, { visible: false });
    hiddenTile = object(scene, { visible: false });
    const markerInHiddenGroup = object(hiddenGroup, { takes: true });
    behind = hitOn(object(scene), 4);
    return [hitOn(hiddenTile, 1), hitOn(markerInHiddenGroup, 2), behind];
  });
  const detail = mapInteraction(v.view, { x: 200, y: 100 }, {});
  assert.equal(detail.hiresHit, behind, 'the first visible hires hit');
  assert.equal(detail.hit, behind);
  assert.equal(hiddenTile.clicks.length, 0, 'a hidden object is not offered the click');
  assert.equal(detail.intersections.length, 3, 'every intersection is reported, as BlueMap does');
});

test('nothing is hit when the ray hits nothing', async () => {
  const { mapInteraction } = await import('../web/bluemap-view.mjs');
  const v = viewer(() => []);
  const detail = mapInteraction(v.view, { x: 120, y: 60 }, {});
  assert.equal(detail.hit, null);
  assert.equal(detail.hiresHit, null);
});

test('a marker in front that takes the click answers nothing; one behind is not offered it unless drawn over everything', async () => {
  const { mapInteraction } = await import('../web/bluemap-view.mjs');
  const front = viewer(({ scene, markers }) => [hitOn(object(markers, { takes: true }), 1), hitOn(object(scene), 2)]);
  assert.equal(mapInteraction(front.view, { x: 200, y: 100 }, {}), null, 'BlueMap\'s popup cube takes the click');

  let hidden;
  let overlay;
  const back = viewer(({ scene, markers }) => {
    hidden = object(markers, { takes: true });
    overlay = object(markers, { depthTest: false });
    return [hitOn(object(scene), 1), hitOn(hidden, 2), hitOn(overlay, 3)];
  });
  const detail = mapInteraction(back.view, { x: 200, y: 100 }, {});
  assert.notEqual(detail, null);
  assert.equal(hidden.clicks.length, 0, 'a marker behind the first hit, drawn with the depth test, is not offered it');
  assert.equal(overlay.clicks.length, 1, 'one drawn without the depth test is');
});

test('no map loaded: no cast', async () => {
  const { mapInteraction } = await import('../web/bluemap-view.mjs');
  const v = viewer(() => { throw new Error('cast without a map'); });
  v.view.map = null;
  assert.equal(mapInteraction(v.view, { x: 0, y: 0 }, {}), null);
});

test('patched, BlueMap\'s own clicks go through the cast and tell its listeners the hires hit', async (t) => {
  const { patchViewer } = await import('../web/bluemap-view.mjs');
  // patchViewer's idle-frame gate listens on the window and draws on animation frames.
  globalThis.window = { addEventListener() {} };
  globalThis.requestAnimationFrame = () => 0;
  t.after(() => { delete globalThis.window; delete globalThis.requestAnimationFrame; });
  let entry;
  const v = viewer(({ scene }) => {
    entry = hitOn(object(scene), 2);
    return [entry];
  });
  const events = new EventTarget();
  const told = [];
  events.addEventListener('bluemapMapInteraction', (event) => told.push(event.detail));
  Object.assign(v.view, {
    events,
    data: { loadedCenter: { x: 0, y: 0 }, loadedLowresViewDistance: 0 },
    renderer: { setOpaqueSort() {} },
    render() {}, redraw() {},
    controlsManager: {},
  });
  patchViewer({ mapViewer: v.view, events: { addEventListener() {} } }, () => null);
  v.view.handleMapInteraction({ x: 200, y: 100 }, { doubleTap: true });
  assert.equal(told.length, 1);
  assert.equal(told[0].hiresHit, entry);
  assert.equal(told[0].hit, entry);
  assert.deepEqual(told[0].data, { doubleTap: true }, 'a double tap stays one, for BlueMap\'s zoom');
});
