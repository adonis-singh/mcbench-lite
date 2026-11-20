(function () {
  'use strict';

  // The shared viewer's bridge (server/share-viewer.js), which BlueMap loads
  // from the viewer's settings.json. Its address names a share's build.json
  // (?build=<build.json>, relative to the viewer), which lists one build or
  // several (a bench's, one per model); it shows the first with the app's
  // own tile sync and mesher, and another when the share page around it
  // asks ({ type: 'mcbench-share:show', index }). It tells the page, each
  // message naming the build it is about by `index`, that the viewer runs
  // ({ type: 'mcbench-share:loading' }), that it starts to show a build
  // ('mcbench-share:showing'), that the build starts to be on screen
  // ('mcbench-share:shown'), how far its drawing has come
  // ('mcbench-share:progress', drawn, total) or why it is not shown
  // ('mcbench-share:failed', error). There is no live world, no selection
  // and no player mode: a visitor moves the camera with BlueMap's own
  // controls, in one of two modes the page can switch between
  // ({ type: 'mcbench-share:set-mode', mode }): 'fly', BlueMap's free
  // flight, or 'orbit', its map controls held over the build. The bridge
  // tells the page the mode it is in ({ type: 'mcbench-share:mode', mode }),
  // and goes back to the opening view of the build on show when asked
  // ({ type: 'mcbench-share:reset' }). It tells the page of the visitor's
  // first input in the viewer ({ type: 'mcbench-share:input' }). Every address is relative to the
  // viewer, which GitHub Pages serves under /<repo>/.
  function tell(message) {
    window.parent.postMessage(message, window.location.origin);
  }
  // The share page knows from this that the viewer runs.
  tell({ type: 'mcbench-share:loading', index: 0 });

  function here(name) {
    return new URL(name, document.baseURI).href;
  }

  function waitForBlueMap() {
    return new Promise(function (resolve, reject) {
      var started = Date.now();
      var timer = setInterval(function () {
        if (window.bluemap && window.bluemap.mapViewer && window.bluemap.mapViewer.map?.isLoaded && window.BlueMap) {
          clearInterval(timer);
          resolve(window.bluemap);
        } else if (Date.now() - started > 20000) {
          clearInterval(timer);
          reject(new Error('BlueMap viewer did not become ready'));
        }
      }, 16);
    });
  }

  async function json(url) {
    var response = await fetch(url);
    if (!response.ok) throw new Error(url + ' is unavailable (HTTP ' + response.status + ')');
    return response.json();
  }

  // BlueMap's own menus and buttons are made for a full window; the share
  // fills a 480 px frame or a phone, so only the map shows. Its controls
  // are on the map itself: drag to move, right drag or two fingers to
  // rotate and tilt, wheel or pinch to zoom.
  function hideChrome() {
    var style = document.createElement('style');
    style.textContent = '#app{display:none!important}html,body{overscroll-behavior:none}';
    document.head.appendChild(style);
  }

  // A view of a build's box (`bounds`) from the direction `rotation` and
  // `angle`: at the distance the preview (server/shot.js) takes to keep all
  // 8 corners of the box in this frame with MARGIN to spare, the box's
  // picture centered in it. BlueMap's map camera looks along its heading
  // (sin rotation, 0, -cos rotation), turned down from straight down by
  // `angle`, at a target `distance` ahead. The opening view is the
  // preview's (iso_se): from the south-east, 30 degrees below the horizon.
  var ROTATION = -Math.PI / 4;
  var ANGLE = Math.PI / 2 - Math.PI / 6;
  var MARGIN = 1.08;
  function fit(bounds, camera, rotation, angle) {
    var min = bounds.min;
    var max = bounds.max;
    var dot = function (a, b) { return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]; };
    var heading = [Math.sin(rotation), 0, -Math.cos(rotation)];
    var forward = [heading[0] * Math.sin(angle), -Math.cos(angle), heading[2] * Math.sin(angle)];
    var right = [Math.cos(rotation), 0, Math.sin(rotation)];
    var up = [
      right[1] * forward[2] - right[2] * forward[1],
      right[2] * forward[0] - right[0] * forward[2],
      right[0] * forward[1] - right[1] * forward[0],
    ];
    var tall = Math.tan(camera.fov * Math.PI / 360) / MARGIN;
    var wide = tall * camera.aspect;
    var corners = [];
    for (var corner = 0; corner < 8; corner += 1) {
      corners.push([0, 1, 2].map(function (i) { return corner >> i & 1 ? max[i] + 1 : min[i]; }));
    }
    // As the preview frames it: the nearest eye on the line through the
    // box's center that sees every corner, a corner at x along right (or
    // up) and f along forward from the center being in view when
    // |x| <= k (distance + f), k the frame's half width (or height) per unit
    // of depth.
    var center = [0, 1, 2].map(function (i) { return (min[i] + max[i] + 1) / 2; });
    var depth = dot(center, forward) - Math.max.apply(null, corners.map(function (p) {
      var offset = [0, 1, 2].map(function (i) { return p[i] - center[i]; });
      return Math.max(Math.abs(dot(offset, right)) / wide, Math.abs(dot(offset, up)) / tall) - dot(offset, forward);
    }));
    // Perspective draws the near part of the box larger, so that picture
    // sits off the middle of the frame. The eye moves across, at the same
    // depth e (the same perspective), until the corners are as far out on
    // one side as on the other: that spread falls as the eye's c along the
    // axis grows, so halving the range in which every corner stays in view
    // finds it. An empty range: no eye at that depth sees every corner.
    var range = function (direction, k, e) {
      return [
        Math.max.apply(null, corners.map(function (p) { return dot(p, direction) - k * (dot(p, forward) - e); })),
        Math.min.apply(null, corners.map(function (p) { return dot(p, direction) + k * (dot(p, forward) - e); })),
      ];
    };
    var middle = function (direction, k, e) {
      var bounds = range(direction, k, e);
      for (var step = 0; step < 50; step += 1) {
        var c = (bounds[0] + bounds[1]) / 2;
        var seen = corners.map(function (p) { return (dot(p, direction) - c) / (dot(p, forward) - e); });
        if (Math.max.apply(null, seen) + Math.min.apply(null, seen) > 0) bounds[0] = c;
        else bounds[1] = c;
      }
      return (bounds[0] + bounds[1]) / 2;
    };
    // Centered, the picture has room on every side, so the eye comes nearer
    // in steps of 2% while every corner stays in view and the build's near
    // end is drawn at most five times the scale of its far end (the preview
    // draws a wide flat build at about four): nearer still, a flat build
    // fits the frame only by swelling its near corner, and BlueMap's 75
    // degree view would reach that with the eye a few blocks off it.
    var fits = function (e) {
      var depths = corners.map(function (p) { return dot(p, forward) - e; });
      var across = range(right, wide, e);
      var down = range(up, tall, e);
      return across[0] <= across[1] && down[0] <= down[1] && 5 * Math.min.apply(null, depths) >= Math.max.apply(null, depths);
    };
    var ahead = dot(center, forward);
    for (var steps = 0; steps < 100 && fits(ahead - (ahead - depth) * 0.98); steps += 1) depth = ahead - (ahead - depth) * 0.98;
    var eye = [0, 1, 2].map(function (i) { return right[i] * middle(right, wide, depth) + up[i] * middle(up, tall, depth) + forward[i] * depth; });
    // The target is where the eye's line of sight meets the build's middle
    // height.
    var height = (min[1] + max[1] + 1) / 2;
    var distance = (eye[1] - height) / -forward[1];
    return { eye: eye, target: eye.map(function (value, i) { return value + forward[i] * distance; }), distance: distance };
  }

  // The camera's state for `view` (fit's) in `mode`: in orbit the target
  // and the eye's distance from it, in fly the eye itself at distance 0.
  // BlueMap puts its camera at position - distance along the direction of
  // rotation and angle, so both draw the same picture.
  function placement(view, mode, rotation, angle) {
    if (mode === 'fly') return { position: view.eye, distance: 0, rotation: rotation, angle: angle };
    return { position: view.target, distance: view.distance, rotation: rotation, angle: angle };
  }


  // Past this angle from straight down an orbit view's eye nears the
  // build's middle height and its distance runs off to infinity, so a
  // visitor looking nearly level gets an orbit view at this angle. Free
  // flight has no target and takes any angle.
  var STEEPEST = 80 * Math.PI / 180;

  // In orbit, the camera's target stays at the view's height and over the
  // box of the build on show (`held`: its bounds and the orbit view's
  // target), wherever a visitor drags it: the world holds that build alone,
  // and BlueMap's own rule (3 blocks over the terrain under the target,
  // lowered toward y 0 as the camera backs off) sinks it into the void
  // beside the build. Zooming out stops at a few times the view's distance
  // (look sets it). These are the map controls' own: free flight has no
  // bounds.
  function holdCamera(app, held) {
    var clamp = function (value, a, b) { return Math.min(Math.max(value, a), b); };
    app.mapControls.mapHeight.update = function () {
      var position = this.manager.position;
      var min = held.bounds.min;
      var max = held.bounds.max;
      var target = held.target;
      position.x = clamp(position.x, Math.min(min[0], target[0]), Math.max(max[0] + 1, target[0]));
      position.z = clamp(position.z, Math.min(min[2], target[2]), Math.max(max[2] + 1, target[2]));
      position.y = target[1];
    };
  }

  // Puts the camera in `state` (placement's) under the controls of `mode`
  // at once: no move between modes is animated (BlueMap's setFreeFlight
  // and setPerspectiveView would level the camera and slide it), so the
  // next frame drawn is already the view.
  function place(app, mode, state) {
    if (app.viewAnimation) app.viewAnimation.cancel();
    var manager = app.mapViewer.controlsManager;
    manager.controls = null;
    manager.position.set(state.position[0], state.position[1], state.position[2]);
    manager.distance = state.distance;
    manager.rotation = state.rotation;
    manager.angle = state.angle;
    manager.ortho = 0;
    manager.tilt = 0;
    if (mode === 'fly') {
      manager.controls = app.freeFlightControls;
      app.appState.controls.state = 'free';
    } else {
      app.mapControls.reset();
      manager.controls = app.mapControls;
      app.appState.controls.state = 'perspective';
    }
    app.mapViewer.redraw();
  }

  // Points the camera at `bounds` from `rotation` and `angle`, in `held`'s
  // mode, and in orbit holds it over that box (from the first build shown
  // on: until then BlueMap's own controls run as they start).
  function look(app, held, bounds, rotation, angle) {
    if (!held.bounds) holdCamera(app, held);
    held.bounds = bounds;
    if (held.mode === 'fly') {
      place(app, 'fly', placement(fit(bounds, app.mapViewer.camera, rotation, angle), 'fly', rotation, angle));
      return;
    }
    var steep = Math.min(angle, STEEPEST);
    var view = fit(bounds, app.mapViewer.camera, rotation, steep);
    held.target = view.target;
    app.mapControls.maxDistance = view.distance * 4;
    place(app, 'orbit', placement(view, 'orbit', rotation, steep));
  }

  // The mode a share opens in: free flight, except on a device whose only
  // pointer is a finger. BlueMap's free flight turns the view by touch but
  // moves the camera only by keys, so there it opens in orbit, which moves
  // by touch.
  function openingMode(matchMedia) {
    return matchMedia('(pointer: coarse) and (hover: none)').matches ? 'orbit' : 'fly';
  }

  // How far drawing of build `index` has come, for the share page's
  // progress line: the build's tiles (the tile sync's) and how many of them
  // BlueMap has on screen, told after each tile it loads until every one
  // is. Tracking another build, or none (null), stops telling of the last.
  function progressOf(app, sync) {
    var count = null;
    function stop() {
      if (count) app.events.removeEventListener('bluemapMapTileLoaded', count);
      count = null;
    }
    return function (index) {
      stop();
      if (index === null) return;
      count = function () {
        var drawn = 0;
        sync.assets.forEach(function (asset, key) {
          var tile = sync.manager.tiles.get(key);
          if (tile && tile.loaded) drawn += 1;
        });
        tell({ type: 'mcbench-share:progress', index: index, drawn: drawn, total: sync.assets.size });
        if (drawn === sync.assets.size) stop();
      };
      app.events.addEventListener('bluemapMapTileLoaded', count);
      count();
    };
  }

  // The build.json that `page` (the viewer's address, at
  // <site>/viewer/<id>/) names in ?build=. Only one in the site's own b/
  // folder is shown: any other address throws, so no one can show content
  // of theirs under the owner's site.
  function buildAddress(page) {
    var address = new URL(new URL(page).searchParams.get('build'), page);
    var builds = new URL('../../b/', page);
    if (address.origin !== builds.origin || !address.pathname.startsWith(builds.pathname)) {
      throw new Error(address.href + ' is not a build of this site');
    }
    return address;
  }

  var buildUrl = null;

  // The build the page asks for (the first, until it asks for another) and
  // the one being shown or on screen (null: none, or the last one failed).
  var wanted = 0;
  var current = null;
  // The camera: its mode, the bounds of the build on show (null before the
  // first) and the orbit view's target.
  var held = { mode: openingMode(window.matchMedia), bounds: null, target: null };
  // Set once the viewer is ready: show shows the wanted build, setMode and
  // reset act on the build on show.
  var show = null;
  var setMode = null;
  var reset = null;
  window.addEventListener('message', function (event) {
    if (event.source !== window.parent || event.origin !== window.location.origin) return;
    if (event.data.type === 'mcbench-share:show') {
      wanted = event.data.index;
      if (show) show();
    } else if (event.data.type === 'mcbench-share:set-mode') {
      if (setMode) setMode(event.data.mode);
      else held.mode = event.data.mode;
    } else if (event.data.type === 'mcbench-share:reset') {
      if (reset) reset();
    }
  });

  var shareJson = new Promise(function (resolve) {
    buildUrl = buildAddress(window.location.href);
    resolve(json(buildUrl.href));
  });
  Promise.all([waitForBlueMap(), shareJson, import(here('bluemap-tiles.mjs')), import(here('bluemap-view.mjs'))])
    .then(function (loaded) {
      var app = loaded[0];
      var share = loaded[1];
      var tiles = loaded[2];
      hideChrome();
      var spawn = function () { return new Worker(here('mesher-worker.mjs'), { type: 'module' }); };
      var sync = new tiles.BlueMapTileSync(app, new tiles.MesherClient(spawn, tiles.MesherClient.poolSize(navigator.hardwareConcurrency)));
      // The camera is held by holdCamera, not by the terrain under it.
      loaded[3].patchViewer(app, function () { return null; });
      var progress = progressOf(app, sync);
      // A click in free flight would take the whole screen and lock the
      // pointer (BlueMap's onMouseUp): in a share the strip and links would
      // go, and X's frame allows neither. Dragging turns the view without it.
      app.freeFlightControls.onMouseUp = function () {};
      tell({ type: 'mcbench-share:mode', mode: held.mode });
      // The page's hint of the controls goes at the visitor's first input
      // in the viewer.
      var inputs = ['pointerdown', 'keydown', 'wheel'];
      var input = function () {
        inputs.forEach(function (type) { window.removeEventListener(type, input, true); });
        tell({ type: 'mcbench-share:input' });
      };
      inputs.forEach(function (type) { window.addEventListener(type, input, true); });

      // Orbit fits the build from the direction the camera looks; fly
      // stays where the camera is, as it is.
      setMode = function (mode) {
        if (mode === held.mode) return;
        held.mode = mode;
        tell({ type: 'mcbench-share:mode', mode: mode });
        if (!held.bounds) return;
        var manager = app.mapViewer.controlsManager;
        if (mode === 'orbit') {
          look(app, held, held.bounds, manager.rotation, manager.angle);
          return;
        }
        var eye = app.mapViewer.camera.position;
        place(app, 'fly', { position: [eye.x, eye.y, eye.z], distance: 0, rotation: manager.rotation, angle: manager.angle });
      };
      reset = function () {
        if (held.bounds) look(app, held, held.bounds, ROTATION, ANGLE);
      };

      // Each build's blueprint, fetched when it is first shown and kept
      // (unzipped, at a blob: address the mesher's worker reads) so showing
      // it again fetches nothing. A failed fetch is not kept.
      var blueprints = new Map();
      function blueprint(index) {
        if (!blueprints.has(index)) {
          var address = new URL(share.builds[index].blueprint, buildUrl).href;
          var fetched = fetch(address).then(async function (response) {
            if (!response.ok) throw new Error(address + ' is unavailable (HTTP ' + response.status + ')');
            var blob = await new Response(response.body.pipeThrough(new DecompressionStream('gzip'))).blob();
            return URL.createObjectURL(blob);
          });
          fetched.catch(function () { blueprints.delete(index); });
          blueprints.set(index, fetched);
        }
        return blueprints.get(index);
      }

      // Shows the wanted build, then any asked for meanwhile, one at a time
      // (the tile sync takes one refresh at a time): the last asked wins,
      // and nothing more is told of one asked for before it. A build's
      // world replaces the last one's as a chat's does in the app (a fresh
      // refresh: the old tiles go and the new build streams in nearest
      // first). The first build shown opens on the preview's view; each
      // after it is fitted from the direction the visitor was looking.
      var running = false;
      show = async function () {
        if (running) return;
        running = true;
        while (current !== wanted) {
          var index = wanted;
          current = index;
          progress(null);
          tell({ type: 'mcbench-share:showing', index: index });
          try {
            var address = await blueprint(index);
            if (wanted !== index) continue;
            var entry = share.builds[index];
            var controls = app.mapViewer.controlsManager;
            if (held.bounds) look(app, held, entry.bounds, controls.rotation, controls.angle);
            else look(app, held, entry.bounds, ROTATION, ANGLE);
            await sync.refresh({ blueprint: address, mask: entry.mask, wasm: here('mesher.wasm.gz'), pack: here('mcbench-pack.bin.gz') }, { fresh: true });
            if (wanted !== index) continue;
            // The tile sync's refresh ends once the first tile is on
            // screen; the rest stream in nearest first.
            tell({ type: 'mcbench-share:shown', index: index });
            progress(index);
          } catch (error) {
            if (wanted !== index) continue;
            current = null;
            tell({ type: 'mcbench-share:failed', index: index, error: String(error.message || error) });
            break;
          }
        }
        running = false;
      };
      show();
    })
    .catch(function (error) {
      tell({ type: 'mcbench-share:failed', index: wanted, error: String(error.message || error) });
    });
})();
