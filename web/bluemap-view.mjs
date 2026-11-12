// How BlueMap draws the app's builds, for both of its viewers: the app's
// (bluemap-bridge.js) and the shared one (share/bridge.js).

// Whole builds fit comfortably in hires range, so render everything hires.
// Must stay under 1600: TileManager's 100x100 tile map (the lowres shader's
// "is hires loaded here" lookup) only tracks +/-50 tiles of 32 blocks.
const HIRES_VIEW_DISTANCE = 1500;

// mcbench builds are small floating islands over a void world - the worst
// case for BlueMap's lowres heightmap tiles, which drape "melted curtain"
// walls between build columns and the void. Stock BlueMap caps hires at a
// cookie-saved radius (default 160) and past camera distance 1000 neither
// loads nor draws hires at all. Load hires over the whole build instead,
// and clamp the distance the render pass sees below all of BlueMap's
// "camera is close" thresholds (hires draw at <1000, lowres hidden under
// loaded hires at <1000, empty void lowres columns discarded at <800), so
// the hires build is what renders at every zoom level.
// `bounds()`: the shown world's bounds ({ min, max }, Minecraft
// coordinates), or null (see holdHeightOverBuilds).
export function patchViewer(app, bounds) {
  const viewer = app.mapViewer;
  if (viewer.mcbenchPatched) return;
  viewer.mcbenchPatched = true;

  viewer.updateLoadedMapArea = function () {
    if (!viewer.map) return;
    viewer.data.loadedHiresViewDistance = HIRES_VIEW_DISTANCE;
    viewer.map.loadMapArea(
      viewer.data.loadedCenter.x,
      viewer.data.loadedCenter.y,
      viewer.data.loadedHiresViewDistance,
      viewer.data.loadedLowresViewDistance
    );
  };

  // Draw opaque geometry nearest first. BlueMap's shader discards
  // transparent texels, which turns off the GPU's own hidden-surface
  // removal, so depth tests against nearer tiles are what skip hidden
  // fragments. Within one tile (equal distance) materials keep three.js's
  // order, so coplanar overlays draw exactly as before: same pixels.
  viewer.renderer.setOpaqueSort(function (a, b) {
    return (a.groupOrder - b.groupOrder) || (a.renderOrder - b.renderOrder)
      || (a.z - b.z) || (a.material.id - b.material.id) || (a.id - b.id);
  });

  const render = viewer.render.bind(viewer);
  viewer.render = function (delta) {
    const controls = viewer.controlsManager;
    Object.defineProperty(controls, 'distance', {
      value: Math.min(controls.distance, 799),
      configurable: true,
    });
    try {
      render(delta);
    } finally {
      delete controls.distance;
    }
  };

  viewer.updateLoadedMapArea();
  gateIdleFrames(app);
  holdHeightOverBuilds(viewer.map, bounds);
}

// Map controls keep the camera's target 3 blocks above the terrain under
// it. Between the parts of a build, or between a bench's builds, that is
// the void far below them, and the camera sinks under the build. Within the
// shown world's bounds the terrain counts as reaching at least their middle,
// so the target stays level with the build wherever it points.
function holdHeightOverBuilds(map, bounds) {
  const proto = Object.getPrototypeOf(map);
  const terrainHeightAt = proto.terrainHeightAt;
  proto.terrainHeightAt = function (x, z) {
    const height = terrainHeightAt.call(this, x, z);
    const box = bounds();
    if (!box || x < box.min[0] || x > box.max[0] + 1 || z < box.min[2] || z > box.max[2] + 1) return height;
    return Math.max(height || 0, (box.min[1] + box.max[1] + 1) / 2 - 3);
  };
}

// BlueMap renders every frame for a second after anything changes, and at
// 20 fps forever after, so animated textures keep moving. Idle frames only
// change the picture when an animated texture on screen reaches its next
// frame, so they are drawn then and no other time: never for a still build,
// every 400 ms for a lantern, still 20 fps for an animation that blends
// between frames. Whatever else changes the picture wakes the loop: camera
// moves (BlueMap's own redraw), tile loads and unloads (BlueMap listens for
// a tile event its map never sends), texture images arriving, input,
// resizing and camera links. The controls (which re-measure the terrain
// under the camera every frame) and the frame-rate meter also run only
// while awake: a still camera has nothing for them to do.
function gateIdleFrames(app) {
  const viewer = app.mapViewer;
  let inView = []; // the animations of textures on screen
  let scanQueued = false;

  function scan() {
    scanQueued = false;
    const map = viewer.map;
    inView = [];
    if (!map || !map.hiresMaterial) return;
    const materials = map.hiresMaterial;
    materials.forEach(function (material) {
      const image = material.uniforms.textureImage.value.image;
      if (!image.complete) image.addEventListener('load', changed, { once: true });
    });
    // An animation shares its uniform objects with its material.
    const shown = new Set();
    map.hiresTileManager.tiles.forEach(function (tile) {
      if (!tile.model) return;
      tile.model.geometry.groups.forEach(function (group) {
        const material = materials[group.materialIndex];
        if (material.uniforms.animationFrameHeight.value < 1) shown.add(material.uniforms.animationFrameIndex);
      });
    });
    inView = map.animations.filter(function (animation) { return shown.has(animation.uniforms.animationFrameIndex); });
  }

  // Milliseconds until an animation on screen changes, or Infinity.
  function nextAnimationFrame() {
    let wait = Infinity;
    inView.forEach(function (animation) {
      wait = Math.min(wait, animation.data.interpolate ? 0 : animation.frameTime - animation.frameDelta);
    });
    return Math.max(wait, 50);
  }

  function changed() {
    viewer.redraw();
    if (scanQueued) return;
    scanQueued = true;
    requestAnimationFrame(scan);
  }

  app.events.addEventListener('bluemapMapTileLoaded', changed);
  app.events.addEventListener('bluemapMapTileUnloaded', changed);
  ['pointerdown', 'wheel', 'keydown', 'resize', 'hashchange'].forEach(function (type) {
    window.addEventListener(type, viewer.redraw, { passive: true });
  });

  // BlueMap's loop with its idle branch gated; the rest is unchanged.
  viewer.renderLoop = function (now) {
    requestAnimationFrame(viewer.renderLoop);
    if (viewer.lastFrame <= 0) viewer.lastFrame = now;
    const delta = now - viewer.lastFrame;
    const awake = Date.now() - viewer.lastRedrawChange < 1000;
    if (awake && viewer.map != null) viewer.controlsManager.update(delta, viewer.map);
    if (awake || delta > nextAnimationFrame()) {
      viewer.stats.begin();
      viewer.lastFrame = now;
      viewer.render(delta);
      viewer.stats.update();
    }
  };
  changed();
}
