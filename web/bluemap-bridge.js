(function () {
  'use strict';

  var panelTimer = null;
  var EXPAND_GLYPH = '<svg viewBox="0 0 30 30" aria-hidden="true"><path d="M17 5h8v8l-3-3-4 4-2-2 4-4zM13 25H5v-8l3 3 4-4 2 2-4 4z"/></svg>';
  var COLLAPSE_GLYPH = '<svg viewBox="0 0 30 30" aria-hidden="true"><path d="M25 7l-4 4 3 3h-8V6l3 3 4-4zM5 23l4-4-3-3h8v8l-3-3-4 4z"/></svg>';
  var MAC = /Mac|iPhone|iPad/.test(navigator.platform);

  // A key press as the app names its shortcuts (comboOf in app.js).
  function comboOf(event) {
    if (['Meta', 'Control', 'Alt', 'Shift'].indexOf(event.key) >= 0) return null;
    var key = /^Key[A-Z]$/.test(event.code) ? event.code.slice(3)
      : /^Digit\d$/.test(event.code) ? event.code.slice(5)
        : event.key.length === 1 ? event.key.toUpperCase() : event.key;
    var mod = MAC ? event.metaKey : event.ctrlKey;
    if (!mod && !event.altKey && !(MAC && event.ctrlKey) && !/^F\d{1,2}$/.test(key)) return null;
    return [mod && 'Mod', MAC && event.ctrlKey && 'Ctrl', event.altKey && 'Alt', event.shiftKey && 'Shift', key].filter(Boolean).join('+');
  }
  var uiState = {
    exportable: false,
    download: null,
    labels: [],
    selectable: false,
    selections: [],
    bounds: null,
    binds: [],
    typeAhead: false,
    full: false,
  };

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

  function installPanelStyles() {
    if (document.getElementById('mcbench-bluemap-styles')) return;
    var style = document.createElement('style');
    style.id = 'mcbench-bluemap-styles';
    style.textContent = [
      '#mcbench-panel{display:flex;align-items:stretch;height:2em;flex:none;pointer-events:auto;background:var(--theme-bg);color:var(--theme-fg-light);white-space:nowrap}',
      '#mcbench-panel[hidden]{display:none!important}',
      // Its buttons are <button> and <a>, BlueMap's own are <div>: without
      // the browser's button border, padding and font they are the same size
      // and face as BlueMap's.
      '#mcbench-panel>.svg-button{appearance:none;box-sizing:border-box;margin:0;padding:0;border:0;border-radius:0;font:inherit;color:inherit;text-decoration:none}',
      '#mcbench-panel>.svg-button[hidden]{display:none!important}',
      '.mcbench-spectating>.svg-button.active[data-mcbench-player]{background-color:var(--theme-bg)}',
      '#mcbench-player{position:fixed;inset:0;pointer-events:none;z-index:20;font:12px/1.3 system-ui,sans-serif;color:#f2efe8}',
      '#mcbench-player[hidden],#mcbench-player [hidden]{display:none!important}',
      '.mcbench-crosshair{position:absolute;left:50%;top:50%;width:16px;height:16px;margin:-8px 0 0 -8px;mix-blend-mode:difference}',
      '.mcbench-crosshair::before,.mcbench-crosshair::after{content:"";position:absolute;background:#fff}',
      '.mcbench-crosshair::before{left:7px;top:0;width:2px;height:16px}',
      '.mcbench-crosshair::after{top:7px;left:0;height:2px;width:16px}',
      '.mcbench-player-hint,.mcbench-player-note{position:absolute;left:50%;transform:translateX(-50%);padding:5px 10px;border-radius:7px;background:rgba(12,12,12,.62);max-width:calc(100% - 32px);text-align:center}',
      '.mcbench-player-hint{bottom:64px;color:#d8d3c8}',
      '.mcbench-player-note{top:58px;color:#ffd7a8}',
      '#mcbench-panel>[hidden]{display:none!important}',
      '#mcbench-panel>:not([hidden])~*{border-left:solid 1px var(--theme-bg-light)}',
      '.mcbench-tag{transform:translate(-50%,-100%);padding:3px 8px;border-radius:6px;background:rgba(12,12,12,.86);color:#f5f5f5;font:500 12px/1.35 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;white-space:nowrap;pointer-events:none;box-shadow:0 0 0 1px rgba(255,255,255,.1)}',
      '.mcbench-icon{display:block;transform:translate(-50%,-50%);width:20px;height:20px;border-radius:4px;background:#f5f5f5;box-shadow:0 0 0 2px #f5f5f5,0 1px 6px 1px rgba(0,0,0,.55);image-rendering:pixelated;pointer-events:none}',
      '.mcbench-icon.empty{background:rgba(12,12,12,.86);box-shadow:inset 0 0 0 1.5px #f5f5f5}',
      'body.mcbench-selecting canvas{cursor:crosshair}',
    ].join('');
    document.head.appendChild(style);
  }

  function updatePanel(panel) {
    var select = panel.querySelector('#mcbench-select');
    var download = panel.querySelector('#mcbench-download');
    var exportWorld = panel.querySelector('#mcbench-export');
    var full = panel.querySelector('#mcbench-full');
    full.innerHTML = uiState.full ? COLLAPSE_GLYPH : EXPAND_GLYPH;
    full.title = uiState.full ? 'Back beside the chat (Esc)' : 'Fill the window';
    full.setAttribute('aria-label', full.title);
    select.hidden = !uiState.selectable;
    select.classList.toggle('active', selecting);
    select.setAttribute('aria-pressed', String(selecting));
    download.hidden = !uiState.download;
    if (uiState.download) download.href = uiState.download;
    exportWorld.hidden = !uiState.exportable;
  }

  function ensurePanel() {
    var controlBar = document.querySelector('.control-bar');
    if (!controlBar) return false;
    installPanelStyles();
    var panel = document.getElementById('mcbench-panel');
    if (!panel) {
      panel = document.createElement('div');
      panel.id = 'mcbench-panel';
      panel.setAttribute('aria-label', 'mcbench world controls');

      var select = document.createElement('button');
      select.id = 'mcbench-select';
      select.type = 'button';
      select.className = 'svg-button';
      select.title = 'Select blocks to refer to in your message (Esc to stop, right-click a box to remove it)';
      select.setAttribute('aria-label', 'Select blocks');
      select.innerHTML = '<svg viewBox="0 0 30 30" aria-hidden="true"><path d="M4.5 11V7A2.5 2.5 0 0 1 7 4.5h4v3H7.5V11zM19 4.5h4A2.5 2.5 0 0 1 25.5 7v4h-3V7.5H19zM25.5 19v4a2.5 2.5 0 0 1-2.5 2.5h-4v-3h3.5V19zM11 25.5H7A2.5 2.5 0 0 1 4.5 23v-4h3v3.5H11zM12.5 11h5a1.5 1.5 0 0 1 1.5 1.5v5a1.5 1.5 0 0 1-1.5 1.5h-5a1.5 1.5 0 0 1-1.5-1.5v-5a1.5 1.5 0 0 1 1.5-1.5z"/></svg>';
      select.addEventListener('click', function () { setSelecting(!selecting); });
      panel.appendChild(select);

      var download = document.createElement('a');
      download.id = 'mcbench-download';
      download.className = 'svg-button';
      download.setAttribute('download', '');
      download.title = 'Download as a WorldEdit schematic (.schem)';
      download.setAttribute('aria-label', download.title);
      download.innerHTML = '<svg viewBox="0 0 30 30" aria-hidden="true"><path d="M13.5 4.5h3v10.4l3.4-3.4 2.1 2.1-7 7-7-7 2.1-2.1 3.4 3.4zM5 20h3v2.5h14V20h3v4a1.5 1.5 0 0 1-1.5 1.5h-17A1.5 1.5 0 0 1 5 24z"/></svg>';
      panel.appendChild(download);

      var exportWorld = document.createElement('button');
      exportWorld.id = 'mcbench-export';
      exportWorld.type = 'button';
      exportWorld.className = 'svg-button';
      exportWorld.title = 'Export as a Minecraft world (a zip to unpack in .minecraft/saves)';
      exportWorld.setAttribute('aria-label', exportWorld.title);
      exportWorld.innerHTML = '<svg viewBox="0 0 30 30" aria-hidden="true"><path d="M15 3.5l9.5 5.2v10.6L15 24.5l-9.5-5.2V8.7zm0 2.3L8.2 9.6 15 13.4l6.8-3.8zm-7.3 5.5v6.8l6.3 3.5v-6.8zm14.6 0l-6.3 3.5v6.8l6.3-3.5z"/></svg>';
      exportWorld.addEventListener('click', function () {
        window.parent.postMessage({ type: 'mcbench-lite:export' }, window.location.origin);
      });
      panel.appendChild(exportWorld);

      var full = document.createElement('button');
      full.id = 'mcbench-full';
      full.type = 'button';
      full.className = 'svg-button';
      full.addEventListener('click', function () {
        window.parent.postMessage({ type: 'mcbench-lite:full', on: !uiState.full }, window.location.origin);
      });
      panel.appendChild(full);
    }
    if (panel.parentNode !== controlBar) controlBar.appendChild(panel);
    updatePanel(panel);
    return true;
  }

  function schedulePanel() {
    if (ensurePanel() || panelTimer) return;
    var started = Date.now();
    panelTimer = setInterval(function () {
      if (ensurePanel() || Date.now() - started > 20000) {
        clearInterval(panelTimer);
        panelTimer = null;
      }
    }, 100);
  }

  window.addEventListener('message', function (event) {
    var data = event.data;
    if (event.source !== window.parent || event.origin !== window.location.origin || !data) return;
    // The page asks how the boot went once the iframe has loaded: it may
    // have missed the telling (bootTo).
    if (data.type === 'mcbench-lite:ping') {
      tellBoot();
      return;
    }
    if (data.type === 'mcbench-lite:ui') {
      uiState = {
        exportable: data.exportable === true,
        download: data.download ? String(data.download) : null,
        labels: Array.isArray(data.labels) ? data.labels : [],
        selectable: data.selectable === true,
        selections: Array.isArray(data.selections) ? data.selections : [],
        bounds: data.bounds,
        binds: data.binds,
        typeAhead: data.typeAhead,
        full: data.full,
        playable: data.playable === true,
      };
      if (!uiState.playable) leavePlayerMode('Player mode needs a running world.');
      if (!uiState.selectable) setSelecting(false);
      schedulePanel();
      drawOverlay();
      return;
    }
    if (data.type === 'mcbench-lite:toggle-select') {
      if (uiState.selectable) setSelecting(!selecting);
      return;
    }
    if (data.type === 'mcbench-lite:fly') {
      fly(data.min, data.max);
      return;
    }
    if (data.type === 'mcbench-lite:player') {
      // The world let the player go (its engine gone, or a fresh placement
      // that failed): player mode ends. Its leave still goes to the page,
      // which then takes out any player a spawn still on its way stands in
      // the world. A fresh placement that succeeds keeps the player, and its
      // ticks count again from 1.
      if (data.gone) {
        leavePlayerMode('The player is no longer in the world.');
        return;
      }
      // A spawn's answer (a view without a tick, or its error). The page
      // answers spawns in the order they were asked, and a spawn asked since
      // supersedes this answer and the ticks before its own.
      if (data.tick === undefined) spawnsPending -= 1;
      if (spawnsPending > 0) return;
      if (data.error) leavePlayerMode(data.error);
      else playerTicked(data.view, data.tick);
      return;
    }
    if (data.type === 'mcbench-lite:frame' && typeof data.key === 'string') {
      // A tick of the world on show: a patch to apply after the ones before
      // it, and the entities. Ticks of a view the viewer has since left, or
      // could not show, are dropped.
      // A frame of a base with nothing since it carries no patch.
      if (link.key !== data.key || link.state === 'failed') return;
      if (data.patch) link.patches.push(new Uint8Array(data.patch));
      entityFrame = data.entities;
      if (entityLayer) entityLayer.set(entityFrame);
      drive();
      return;
    }
    if (data.type !== 'mcbench-lite:refresh' || typeof data.key !== 'string') return;
    // Another build: its world is placed afresh, without the player.
    if (data.fresh) leavePlayerMode(null);
    // The page sends the ticks since this base after it, so those waiting
    // are old, and so are the entities: the frame after it draws them again.
    entityFrame = [];
    if (entityLayer) entityLayer.set(entityFrame);
    // The page asks for a view until it is answered: one still waiting or
    // loading loads once. Any other is loaded, the one on show and the one
    // that failed included.
    if (link.key === data.key && (link.state === 'queued' || link.state === 'loading')) link.patches = [];
    else queueView(data);
    drive();
  });

  // ---- the viewer's boot ----
  // BlueMap, the tile sync, the flight modes and the overlays are set up once,
  // when the iframe loads (syncPromise). The boot is in one state, which goes
  // only where BOOT_NEXT says:
  //   booting   they are being set up: the page is told nothing
  //   ready     they are: the page sends its views
  //   failed    one of them could not be (`error`): the viewer shows nothing,
  //             and is not ready, until the page loads the iframe again
  // The page is told the state the boot ends in once, and again when it asks.
  var BOOT_NEXT = {
    booting: ['ready', 'failed'],
    ready: [],
    failed: [],
  };
  var boot = { state: 'booting', error: null };

  function bootTo(state, error) {
    if (BOOT_NEXT[boot.state].indexOf(state) < 0) throw new Error('The viewer cannot go from ' + boot.state + ' to ' + state);
    if ((state === 'failed') !== (error !== null)) throw new Error('A failed boot, and only one, has an error');
    boot = { state: state, error: error };
    tellBoot();
  }

  function tellBoot() {
    if (boot.state === 'ready') window.parent.postMessage({ type: 'mcbench-lite:bluemap-ready' }, window.location.origin);
    if (boot.state === 'failed') window.parent.postMessage({ type: 'mcbench-lite:boot-failed', error: boot.error }, window.location.origin);
  }

  // The entity layer (entities.mjs), and the latest list streamed before it was ready.
  var entityLayer = null;
  var entityFrame = null;
  var syncPromise = waitForBlueMap().then(async function (app) {
    var module = await import('/bluemap-tiles.mjs');
    var spawn = function () { return new Worker('/mesher-worker.mjs', { type: 'module' }); };
    var sync = new module.BlueMapTileSync(app, new module.MesherClient(spawn, module.MesherClient.poolSize(navigator.hardwareConcurrency)));
    (await import('/bluemap-view.mjs')).patchViewer(app, function () { return uiState.bounds; });
    installFlightModes(app);
    var entities = await import('/entities.mjs');
    entityLayer = new entities.EntityLayer({ BlueMap: window.BlueMap, app: app, assets: await entities.loadEntityAssets(window.location.href) });
    if (entityFrame) entityLayer.set(entityFrame);
    overlay = new window.BlueMap.Three.Group();
    app.mapViewer.markers.add(overlay);
    schedulePanel();
    drawOverlay();
    return sync;
  });
  syncPromise.then(
    function () { bootTo('ready', null); },
    function (error) { bootTo('failed', String(error.message || error)); }
  );

  // ---- the view on show, and the live world's frames ----
  // The page sends a view (a refresh: a base to load) and, running the live
  // world (web/live-world.mjs), each tick on it as a frame: a patch to apply
  // after the ones before, and the entities. The link to the page's view is
  // in one state, which goes only where LINK_NEXT says:
  //   none      no view asked for
  //   queued    a view waits for the loader
  //   loading   its base loads
  //   shown     the base is in the viewer, with every patch applied so far
  //   patching  the patches that came since are being applied
  //   failed    the load or a patch failed (`error`, told to the page once):
  //             what the viewer holds is unknown, and the view's ticks are
  //             dropped until a view is asked for, this one again included
  var LINK_NEXT = {
    none: ['queued'],
    queued: ['queued', 'loading'],
    loading: ['queued', 'shown', 'failed'],
    shown: ['queued', 'patching'],
    patching: ['queued', 'shown', 'failed'],
    failed: ['queued'],
  };
  var link = { state: 'none', key: null, request: null, afresh: false, patches: [], error: null };

  function linkTo(state, error) {
    if (LINK_NEXT[link.state].indexOf(state) < 0) throw new Error('The view cannot go from ' + link.state + ' to ' + state);
    if ((state === 'failed') !== (error !== null)) throw new Error('A failed view, and only one, has an error');
    link.state = state;
    link.error = error;
  }

  // A view to load, in place of the one before: the ticks waiting are the
  // old view's. After a failure nothing the viewer holds is kept, so the
  // tiles are loaded afresh.
  function queueView(data) {
    var afresh = link.state === 'failed';
    linkTo('queued', null);
    link = { state: 'queued', key: data.key, request: data, afresh: afresh, patches: [], error: null };
  }

  // The loader takes one turn at a time, and each message that leaves it
  // work asks for one: a view loads first, then the patches that arrived for
  // it, all of them at once. A view left during its turn (another was
  // queued) answers nothing: the turns after it are the new view's.
  var turns = Promise.resolve();
  function drive() {
    turns = turns.then(turn);
  }

  async function turn() {
    var mine = link;
    try {
      if (mine.state === 'queued') {
        linkTo('loading', null);
        var sync = await syncPromise;
        if (link !== mine) return;
        if (mine.request.cameraHash && window.location.hash !== mine.request.cameraHash) {
          window.location.hash = mine.request.cameraHash;
        }
        await sync.refresh(mine.request, { fresh: mine.request.fresh === true || mine.afresh });
        if (link !== mine) return;
        linkTo('shown', null);
        window.parent.postMessage({ type: 'mcbench-lite:refreshed', key: mine.key }, window.location.origin);
      }
      if (mine.state === 'shown' && mine.patches.length) {
        linkTo('patching', null);
        var patches = mine.patches;
        mine.patches = [];
        await (await syncPromise).patch(patches);
        if (link !== mine) return;
        linkTo('shown', null);
      }
    } catch (error) {
      if (link !== mine) return;
      linkTo('failed', String(error.message || error));
      mine.patches = [];
      window.parent.postMessage({ type: 'mcbench-lite:refresh-error', key: mine.key, error: mine.error }, window.location.origin);
    }
  }

  // ---- overlays: build names in a comparison, selection boxes in a chat ----
  // They live in a plain group under the marker scene: BlueMap's marker-file
  // sync removes marker sets it does not know, but leaves other objects alone.
  var overlay = null;
  var dragMarker = null;
  var BONE = { r: 245, g: 245, b: 245 };

  function escapeHtml(text) {
    return String(text).replace(/[&<>"']/g, function (c) { return '&#' + c.charCodeAt(0) + ';'; });
  }

  function htmlMarker(id, position, html, marker) {
    if (!marker) {
      marker = new window.BlueMap.HtmlMarker(id);
      overlay.add(marker);
    }
    marker.updateFromData({ position: position, html: html, anchor: { x: 0, y: 0 }, classes: [], listed: false });
    return marker;
  }

  // HtmlMarker cleans up only its inner element, and its dispose() fails
  // once that is gone, so its outer element is removed here.
  function release(marker) {
    overlay.remove(marker);
    if (marker.isHtmlMarker) marker.elementObject.element.remove();
    else marker.dispose();
  }

  // An outlined box over whole blocks, min and max inclusive (Minecraft coordinates).
  function boxMarker(id, min, max, fillAlpha, marker) {
    if (!marker) {
      marker = new window.BlueMap.ExtrudeMarker(id);
      overlay.add(marker);
    }
    var x1 = min[0]; var x2 = max[0] + 1; var z1 = min[2]; var z2 = max[2] + 1;
    // ExtrudeMarker pads its shape by 0.01; this lands the faces on the block edges.
    marker.updateFromData({
      position: { x: x1, y: min[1], z: z1 },
      shape: [{ x: x1 - 0.02, z: z1 - 0.02 }, { x: x2, z: z1 - 0.02 }, { x: x2, z: z2 }, { x: x1 - 0.02, z: z2 }],
      shapeMinY: min[1],
      shapeMaxY: max[1] + 1,
      depthTest: true,
      lineWidth: 2,
      listed: false,
      lineColor: Object.assign({ a: 1 }, BONE),
      fillColor: Object.assign({ a: fillAlpha }, BONE),
    });
    // Picks pass through overlays to the blocks behind them.
    marker.traverse(function (child) { child.raycast = function () {}; });
    return marker;
  }

  // Markers persist by id between draws; ones no longer wanted are released.
  var drawn = new Map();

  function drawOverlay() {
    if (!overlay) return;
    var wanted = new Set();
    function place(id, draw) {
      wanted.add(id);
      drawn.set(id, draw(drawn.get(id)));
    }
    uiState.labels.forEach(function (label, index) {
      place('mcbench-label-' + index, function (marker) {
        return htmlMarker('mcbench-label-' + index, { x: label.x, y: label.y, z: label.z }, '<div class="mcbench-tag">' + escapeHtml(label.text) + '</div>', marker);
      });
    });
    uiState.selections.forEach(function (selection) {
      place('mcbench-box-' + selection.key, function (marker) {
        return boxMarker('mcbench-box-' + selection.key, selection.min, selection.max, selection.lit ? 0.22 : 0.08, marker);
      });
      if (!selection.label) return;
      place('mcbench-icon-' + selection.key, function (marker) {
        return htmlMarker('mcbench-icon-' + selection.key,
          { x: (selection.min[0] + selection.max[0] + 1) / 2, y: selection.max[1] + 1.6, z: (selection.min[2] + selection.max[2] + 1) / 2 },
          selection.icon ? '<img class="mcbench-icon" alt="" src="' + escapeHtml(selection.icon) + '">' : '<div class="mcbench-icon empty"></div>', marker);
      });
    });
    drawn.forEach(function (marker, id) {
      if (wanted.has(id)) return;
      release(marker);
      drawn.delete(id);
    });
    window.bluemap.mapViewer.redraw();
  }

  // ---- the camera's mode ----
  // BlueMap takes the controls off its controls manager when it starts a
  // move between modes and gives it the new mode's when the move ends; a
  // move cancelled on the way would leave it with none, and the camera with
  // no input. The mode is what the manager was last given (installFlightModes
  // hears of it), and BlueMap goes from any to any:
  //   changing   none: BlueMap is not there yet, or is moving between modes
  //   map        BlueMap's map controls (perspective or flat)
  //   player     player mode
  //   spectator  BlueMap's own free flight
  var CAMERA_MODES = ['changing', 'map', 'player', 'spectator'];
  var cameraMode = 'changing';
  // The build a fly was asked to while changing: it runs when the change ends.
  var flyWaiting = null;

  function cameraTo(mode) {
    if (CAMERA_MODES.indexOf(mode) < 0) throw new Error('The camera has no mode ' + mode);
    cameraMode = mode;
    if (mode === 'changing' || !flyWaiting) return;
    var waiting = flyWaiting;
    flyWaiting = null;
    fly(waiting.min, waiting.max);
  }

  // Frames one build: the player is moved to look at it, any other camera
  // flown to it. During a change of mode it waits, so nothing cancels
  // BlueMap's move.
  function fly(min, max) {
    if (cameraMode === 'changing') flyWaiting = { min: min, max: max };
    else if (cameraMode === 'player') teleportTo(min, max);
    else flyTo(min, max);
  }

  // ---- camera: frame one build, whatever the controls ----
  // The camera sits at position - direction * distance, looking at position.
  // Map controls look at a point from a distance; free flight has no
  // distance, so position is the eye. Either way the build's bounding sphere
  // ends up filling the view, seen from the current heading.
  var flight = null;
  function flyTo(min, max) {
    if (!overlay) return;
    var app = window.bluemap;
    var viewer = app.mapViewer;
    var controls = viewer.controlsManager;
    var Three = window.BlueMap.Three;
    var center = new Three.Vector3((min[0] + max[0] + 1) / 2, (min[1] + max[1] + 1) / 2, (min[2] + max[2] + 1) / 2);
    var radius = Math.hypot(max[0] - min[0] + 1, max[1] - min[1] + 1, max[2] - min[2] + 1) / 2;
    var vertical = viewer.camera.fov * Math.PI / 180;
    var horizontal = 2 * Math.atan(Math.tan(vertical / 2) * viewer.camera.aspect);
    // A fifth to spare keeps labels over builds at the edges in view.
    var distance = Math.max(radius / Math.sin(Math.min(vertical, horizontal) / 2) * 1.2, 12);
    var free = app.appState.controls.state === 'free';
    var from = { x: controls.position.x, y: controls.position.y, z: controls.position.z, distance: controls.distance, angle: controls.angle };
    var to;
    if (free) {
      // Looking down at the build from a little above it.
      var angle = Math.min(Math.max(controls.angle, 0.45), 1.3);
      var heading = new Three.Vector3(Math.sin(controls.rotation), 0, -Math.cos(controls.rotation));
      var look = heading.clone().applyAxisAngle(new Three.Vector3(0, 1, 0).cross(heading), Math.PI / 2 - angle);
      var eye = center.clone().sub(look.normalize().multiplyScalar(distance));
      to = { x: eye.x, y: eye.y, z: eye.z, angle: angle };
    } else {
      // Map controls lower the target toward y 0 by distance / 500, which
      // brings the camera that much nearer the build; stand back to match.
      to = { x: center.x, z: center.z, distance: distance / (1 - center.y * Math.cos(controls.angle) / 500) };
    }
    if (flight) flight.cancel();
    // BlueMap's zoom to a double-clicked point; its moves between modes have
    // ended (fly).
    if (app.viewAnimation) app.viewAnimation.cancel();
    flight = window.BlueMap.animate(function (progress) {
      viewer.redraw();
      var eased = window.BlueMap.EasingFunctions.easeInOutQuad(progress);
      var mix = function (a, b) { return a + (b - a) * eased; };
      controls.position.x = mix(from.x, to.x);
      controls.position.z = mix(from.z, to.z);
      if (free) {
        controls.position.y = mix(from.y, to.y);
        controls.angle = mix(from.angle, to.angle);
      } else {
        controls.distance = mix(from.distance, to.distance);
      }
    }, 700);
  }

  // ---- the two free-flight modes ----
  // BlueMap's free-flight button is player mode (PlayerControls); an eye
  // button after it is BlueMap's own free flight, a spectator that flies
  // through everything. BlueMap takes app.freeFlightControls when its move
  // to eye level ends, so that answers with the mode last chosen.
  var flightMode = 'player';
  var PLAYER_TITLE = 'Player mode: walk and fly as a Minecraft player';
  var SPECTATOR_TITLE = 'Spectator: fly through everything';
  var EYE_GLYPH = '<svg viewBox="0 0 30 30" aria-hidden="true"><path d="M15 8.5C8.6 8.5 4 15 4 15s4.6 6.5 11 6.5S26 15 26 15s-4.6-6.5-11-6.5zm0 11a4.5 4.5 0 1 1 0-9 4.5 4.5 0 0 1 0 9zm0-7a2.5 2.5 0 1 0 0 5 2.5 2.5 0 0 0 0-5z"/></svg>';
  function installFlightModes(app) {
    var spectator = app.freeFlightControls;
    var playerControls = new PlayerControls();
    Object.defineProperty(app, 'freeFlightControls', {
      configurable: true,
      get: function () { return flightMode === 'spectator' ? spectator : playerControls; },
    });
    // The controls BlueMap gives its manager, and takes off it, are the
    // camera's mode.
    var manager = app.mapViewer.controlsManager;
    var controls = Object.getOwnPropertyDescriptor(Object.getPrototypeOf(manager), 'controls');
    var modeOf = function (given) {
      return !given ? 'changing' : given === playerControls ? 'player' : given === spectator ? 'spectator' : 'map';
    };
    Object.defineProperty(manager, 'controls', {
      configurable: true,
      get: function () { return controls.get.call(this); },
      set: function (given) {
        controls.set.call(this, given);
        cameraTo(modeOf(given));
      },
    });
    cameraTo(modeOf(manager.controls));
    var free = function () { return app.appState.controls.state === 'free'; };
    var eye = document.createElement('div');
    eye.id = 'mcbench-spectate';
    eye.className = 'svg-button';
    eye.title = SPECTATOR_TITLE;
    eye.innerHTML = EYE_GLYPH;
    eye.addEventListener('click', function () {
      flightMode = 'spectator';
      app.setFreeFlight(500);
    });
    // The free-flight button chooses player mode before BlueMap acts on it.
    document.addEventListener('click', function (event) {
      var button = event.target.closest && event.target.closest('.controls-switch > .svg-button');
      if (button && button !== eye && button.querySelector('svg') && button.dataset.mcbenchPlayer) flightMode = 'player';
    }, true);
    // The buttons follow the mode: BlueMap marks its button active in either.
    var sync = function () {
      var switches = document.querySelector('.controls-switch');
      if (!switches) return;
      var buttons = switches.querySelectorAll(':scope > .svg-button:not(#mcbench-spectate)');
      var person = buttons[buttons.length - 1];
      if (!person) return;
      person.dataset.mcbenchPlayer = '1';
      if (person.title !== PLAYER_TITLE) person.title = PLAYER_TITLE;
      if (eye.parentNode !== switches || eye.previousSibling !== person) person.after(eye);
      switches.classList.toggle('mcbench-spectating', flightMode === 'spectator' && free());
      eye.classList.toggle('active', flightMode === 'spectator' && free());
    };
    setInterval(sync, 200);
    sync();
  }

  // The player, moved to look at a build as flyTo frames it.
  function teleportTo(min, max) {
    var Three = window.BlueMap.Three;
    var viewer = window.bluemap.mapViewer;
    var center = new Three.Vector3((min[0] + max[0] + 1) / 2, (min[1] + max[1] + 1) / 2, (min[2] + max[2] + 1) / 2);
    var radius = Math.hypot(max[0] - min[0] + 1, max[1] - min[1] + 1, max[2] - min[2] + 1) / 2;
    var vertical = viewer.camera.fov * Math.PI / 180;
    var horizontal = 2 * Math.atan(Math.tan(vertical / 2) * viewer.camera.aspect);
    var distance = Math.max(radius / Math.sin(Math.min(vertical, horizontal) / 2) * 1.2, 12);
    var angle = Math.min(Math.max((90 - player.pitch) * Math.PI / 180, 0.45), 1.3);
    var rotation = (player.yaw + 180) * Math.PI / 180;
    var heading = new Three.Vector3(Math.sin(rotation), 0, -Math.cos(rotation));
    var look = heading.clone().applyAxisAngle(new Three.Vector3(0, 1, 0).cross(heading), Math.PI / 2 - angle);
    var eye = center.clone().sub(look.normalize().multiplyScalar(distance));
    player.pitch = pitchOf(angle);
    spawnPlayer(eye.x, eye.y - 1.62, eye.z, true);
  }

  // ---- player mode: BlueMap's free flight is a creative Minecraft player ----
  // The page's engine holds the player (core/src/player, the client's own
  // movement) and ticks it with the world. This sends it the keys held and
  // the look, and puts the camera at its eyes, between its last two ticks for
  // the time since the last one. The look follows the mouse at once, as in
  // Minecraft; the next tick takes it.
  var PLAYER_KEYS = { KeyW: 1, KeyS: 2, KeyA: 4, KeyD: 8, Space: 16, ShiftLeft: 32, ShiftRight: 32, ControlLeft: 64, ControlRight: 64 };
  var LOOK_PER_PIXEL = 0.15; // degrees, at Minecraft's default mouse sensitivity
  var REACH = 5; // blocks: a creative player's block interaction range
  var TICK_MS = 50;
  // Minecraft kills a player who falls below the world's bottom less 64
  // (y -128 in the overworld) and respawns it; here it goes back to where it
  // joined.
  var VOID_Y = -128;
  var player = null;
  // Spawns asked of the page and not answered yet.
  var spawnsPending = 0;

  function PlayerControls() {
    // BlueMap's settings set these speeds on its free-flight controls.
    this.mouseRotate = { speedCapture: 0, speedRight: 0 };
    this.mouseAngle = { speedCapture: 0, speedRight: 0 };
    // BlueMap's player-marker buttons read what the controls follow: nothing.
    this.data = { followingPlayer: null };
  }
  // BlueMap's rotation is the heading (sin r, 0, -cos r) and its angle the
  // turn down from straight up; Minecraft's yaw faces (-sin y, 0, cos y) and
  // its pitch looks down from level.
  var yawOf = function (rotation) { return rotation * 180 / Math.PI - 180; };
  var pitchOf = function (angle) { return 90 - angle * 180 / Math.PI; };
  // The yaw a click is made with: the player's look in player mode, else the
  // camera's heading. A fence gate opens away from the side its user faces.
  var lookYaw = function () { return player ? player.yaw : yawOf(window.bluemap.mapViewer.controlsManager.rotation); };
  PlayerControls.prototype.start = function (manager) {
    var eye = manager.position;
    player = {
      manager: manager, view: null,
      // The physical keys held, by code: two can hold one of the engine's keys.
      held: {}, taps: 0, dirty: false,
      yaw: yawOf(manager.rotation), pitch: Math.max(-90, Math.min(90, pitchOf(manager.angle))),
      spawn: [eye.x, eye.y - 1.62, eye.z], respawning: false,
      camera: null, tick: null, clock: null, bob: null, fov: window.bluemap.mapViewer.camera.fov,
    };
    // BlueMap places the camera only when its values change and has no roll:
    // in player mode it places it every frame, and the view bob goes on after.
    var updateCamera = Object.getPrototypeOf(manager).updateCamera;
    manager.isValueChanged = function () { return true; };
    manager.updateCamera = function () {
      updateCamera.call(this);
      if (player && player.bob) bobCamera(this.camera, player.bob);
    };
    drawPlayerHud();
    spawnPlayer(eye.x, eye.y - 1.62, eye.z, true);
  };
  PlayerControls.prototype.stop = function () {
    if (!player) return;
    var camera = window.bluemap.mapViewer.camera;
    camera.fov = player.fov;
    camera.updateProjectionMatrix();
    delete player.manager.isValueChanged;
    delete player.manager.updateCamera;
    player = null;
    drawPlayerHud();
    if (document.pointerLockElement) document.exitPointerLock();
    window.parent.postMessage({ type: 'mcbench-lite:player-leave' }, window.location.origin);
  };
  PlayerControls.prototype.update = function () {
    if (!player) return;
    var manager = player.manager;
    if (player.dirty) sendPlayerInput();
    manager.distance = 0;
    manager.ortho = 0;
    var v = player.view;
    var c = player.camera;
    if (!v || !c) {
      manager.rotation = (player.yaw + 180) * Math.PI / 180;
      manager.angle = (90 - player.pitch) * Math.PI / 180;
      manager.tilt = 0;
      window.bluemap.mapViewer.redraw();
      return;
    }
    // The partial tick: how far the steady clock is past the latest tick.
    var t = player.clock === null ? 1 : Math.max(0, Math.min(1, (performance.now() - (player.clock + player.tick * TICK_MS)) / TICK_MS));
    var mix = function (from, to) { return from + (to - from) * t; };
    // Camera.setup: the eyes between the last two ticks, eye height eased.
    manager.position.set(mix(v[3], v[0]), mix(v[4], v[1]) + mix(c.eyeO, c.eye), mix(v[5], v[2]));
    manager.rotation = (player.yaw + 180) * Math.PI / 180;
    manager.angle = (90 - player.pitch) * Math.PI / 180;
    manager.tilt = 0;
    // GameRenderer.bobView: the view sways with the steps.
    var walk = -(c.walk + (c.walk - c.walkO) * t);
    player.bob = { walk: walk, bob: mix(c.bobO, c.bob) };
    // GameRenderer.getFov: 70 times the eased modifier, less under water or lava.
    var fov = FOV * mix(c.fovO, c.fov) * (v[24] || v[25] ? 0.85714287 : 1);
    var camera = window.bluemap.mapViewer.camera;
    if (Math.abs(camera.fov - fov) > 1e-4) {
      camera.fov = fov;
      camera.updateProjectionMatrix();
    }
    window.bluemap.mapViewer.redraw();
  };

  function spawnPlayer(x, y, z, flying) {
    player.spawn = [x, y, z];
    spawnsPending += 1;
    window.parent.postMessage({ type: 'mcbench-lite:player-spawn', x: x, y: y, z: z, yaw: player.yaw, pitch: player.pitch, flying: flying }, window.location.origin);
  }

  // The keys held, the keys pressed since the last send (the engine's next
  // tick sees those held however short the press), and the look.
  function sendPlayerInput() {
    player.dirty = false;
    window.parent.postMessage({ type: 'mcbench-lite:player-input', keys: heldKeys(), taps: player.taps, yaw: player.yaw, pitch: player.pitch }, window.location.origin);
    player.taps = 0;
  }

  // The engine's keys the player holds: each while any of its physical keys is.
  function heldKeys() {
    var keys = 0;
    for (var code in player.held) keys |= PLAYER_KEYS[code];
    return keys;
  }

  // Player mode ends with a note, and the camera goes back to the map.
  function leavePlayerMode(note) {
    if (!player) return;
    window.bluemap.setPerspectiveView(500);
    if (note) showPlayerNote(note);
  }

  // Minecraft's first-person camera (the 1.21.10 client): what it keeps each
  // tick, and what a frame draws between two ticks (update above).
  var FOV = 70; // the default field of view, vertical, in degrees
  // The field-of-view modifier the player asks for
  // (AbstractClientPlayer.getFieldOfViewModifier, effect scale 1): 1.1 flying,
  // times (movement speed / walking speed + 1) / 2, where sprinting adds 30%.
  var fovModifier = function (view) { return (view[17] ? 1.1 : 1) * ((view[18] ? 1.3 : 1) + 1) / 2; };
  function cameraJoined(view) {
    player.camera = { walk: 0, walkO: 0, bob: 0, bobO: 0, eye: view[15], eyeO: view[15], fov: fovModifier(view), fovO: fovModifier(view) };
  }
  function cameraTick(view, before) {
    var c = player.camera;
    // ClientAvatarState.tick, then LocalPlayer.move: 0.6 times the distance
    // moved across the ground.
    c.walkO = c.walk;
    c.walk += Math.hypot(view[0] - view[3], view[2] - view[5]) * 0.6;
    // AbstractClientPlayer.updateBob, at the tick's start: the ground and
    // motion the tick began with.
    var target = before[21] && !before[20] ? Math.min(0.1, Math.hypot(before[6], before[8])) : 0;
    c.bobO = c.bob;
    c.bob += (target - c.bob) * 0.4;
    // Camera.tick: the eye height eases halfway to the player's.
    c.eyeO = c.eye;
    c.eye += (view[15] - c.eye) * 0.5;
    // GameRenderer.tickFov: the modifier eases halfway, within 0.1 to 1.5.
    c.fovO = c.fov;
    c.fov = Math.max(0.1, Math.min(1.5, c.fov + (fovModifier(view) - c.fov) * 0.5));
  }

  // GameRenderer.bobView, whose pose multiplies the projection: a shift, a
  // roll and a nod in view space. The camera takes each the other way round.
  function bobCamera(camera, pose) {
    var w = pose.walk * Math.PI, b = pose.bob;
    camera.rotateX(-Math.abs(Math.cos(w - 0.2) * b) * 5 * Math.PI / 180);
    camera.rotateZ(-Math.sin(w) * b * 3 * Math.PI / 180);
    camera.translateX(-Math.sin(w) * b * 0.5);
    camera.translateY(Math.abs(Math.cos(w) * b));
  }

  // A tick of the world with the player in it (`tick` its number; none for
  // the view a placement answers with, which the camera starts from).
  function playerTicked(view, tick) {
    if (!player) return;
    if (tick === undefined || !player.camera) {
      cameraJoined(view);
      player.tick = null;
      player.clock = null;
    } else {
      cameraTick(view, player.view);
      // A steady clock for the partial tick: tick n falls at clock + n ticks,
      // eased toward when ticks arrive, so a late message does not jerk the view.
      var now = performance.now();
      var at = now - tick * TICK_MS;
      player.clock = player.clock === null || Math.abs(at - player.clock) > 3 * TICK_MS ? at : player.clock + (at - player.clock) * 0.1;
      player.tick = tick;
    }
    player.view = view;
    if (view[1] < VOID_Y && !player.respawning) {
      player.respawning = true;
      showPlayerNote('Fell out of the world');
      spawnPlayer(player.spawn[0], player.spawn[1], player.spawn[2], true);
    } else if (view[1] >= VOID_Y) {
      player.respawning = false;
    }
  }

  function playerKey(event, down) {
    var bit = PLAYER_KEYS[event.code];
    if (!player || !bit) return false;
    // A key pressed with Alt or the command key, or into a field, is not the
    // player's. One the player holds is let go by its keyup wherever that
    // lands and whatever else is held by then.
    if (down || !player.held[event.code]) {
      if (event.metaKey || event.altKey) return false;
      if (event.target.closest && event.target.closest('input, textarea, select, [contenteditable]')) return false;
    }
    event.preventDefault();
    event.stopImmediatePropagation();
    if (down) {
      if (event.repeat || player.held[event.code]) return true;
      var held = heldKeys() & bit;
      player.held[event.code] = true;
      // The other key of the two already holds it.
      if (held) return true;
      player.taps |= bit;
    } else {
      delete player.held[event.code];
    }
    sendPlayerInput();
    return true;
  }
  window.addEventListener('keydown', function (event) { playerKey(event, true); }, true);
  window.addEventListener('keyup', function (event) { playerKey(event, false); }, true);
  // Focus leaving the frame lets every key go.
  window.addEventListener('blur', function () {
    if (!player || !heldKeys()) return;
    player.held = {};
    sendPlayerInput();
  });
  document.addEventListener('mousemove', function (event) {
    if (!player || document.pointerLockElement !== window.bluemap.mapViewer.renderer.domElement) return;
    player.yaw += event.movementX * LOOK_PER_PIXEL;
    player.pitch = Math.max(-90, Math.min(90, player.pitch + event.movementY * LOOK_PER_PIXEL));
    player.dirty = true;
  });
  // In player mode a click takes the mouse for looking around; with it taken,
  // a right click uses the block at the crosshair, within reach. Nothing is
  // broken or placed.
  window.addEventListener('pointerdown', function (event) {
    if (!player) return;
    var canvas = window.bluemap.mapViewer.renderer.domElement;
    if (event.target !== canvas) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (document.pointerLockElement !== canvas) {
      // Chrome refuses the mouse until the window has the focus this click
      // gives it; the hint stays and the next click takes it.
      canvas.requestPointerLock().catch(drawPlayerHud);
      return;
    }
    if (event.button !== 2 || !player.view) return;
    var rect = canvas.getBoundingClientRect();
    var block = pick(rect.left + rect.width / 2, rect.top + rect.height / 2);
    var v = player.view;
    if (block && Math.hypot(block[0] + 0.5 - v[0], block[1] + 0.5 - (v[1] + v[15]), block[2] + 0.5 - v[2]) <= REACH + 0.87) {
      window.parent.postMessage({ type: 'mcbench-lite:interact', x: block[0], y: block[1], z: block[2], yaw: lookYaw() }, window.location.origin);
    }
  }, true);
  window.addEventListener('contextmenu', function (event) { if (player) event.preventDefault(); }, true);
  // A double-click on the map makes the viewer fill the window, or not, in any mode.
  window.addEventListener('dblclick', function (event) {
    if (event.target !== window.bluemap.mapViewer.renderer.domElement) return;
    window.parent.postMessage({ type: 'mcbench-lite:full', on: !uiState.full }, window.location.origin);
  }, true);
  document.addEventListener('pointerlockchange', drawPlayerHud);

  var playerHud = null;
  // The note on show, or null: it shows for 5 s, or until the next one
  // replaces it, in player mode or after it.
  var playerNote = null;
  var noteTimer = null;
  // The HUD, drawn from the player as it is now: the hint while there is one,
  // the crosshair while it has the mouse, and the note, which outlives it.
  function drawPlayerHud() {
    if (!playerHud) {
      playerHud = document.createElement('div');
      playerHud.id = 'mcbench-player';
      playerHud.innerHTML = '<div class="mcbench-crosshair"></div><div class="mcbench-player-hint"></div><div class="mcbench-player-note"></div>';
      document.body.appendChild(playerHud);
    }
    var locked = player !== null && document.pointerLockElement === window.bluemap.mapViewer.renderer.domElement;
    var hint = playerHud.querySelector('.mcbench-player-hint');
    hint.textContent = locked
      ? 'WASD move · Space jump (twice: fly) · Shift sneak · Ctrl sprint · Right-click use · Esc frees the mouse'
      : 'Click to look around';
    hint.hidden = !player;
    playerHud.querySelector('.mcbench-crosshair').hidden = !locked;
    var note = playerHud.querySelector('.mcbench-player-note');
    note.textContent = playerNote === null ? '' : playerNote;
    note.hidden = playerNote === null;
    playerHud.hidden = !player && playerNote === null;
  }
  function showPlayerNote(text) {
    playerNote = text;
    clearTimeout(noteTimer);
    noteTimer = setTimeout(function () { playerNote = null; drawPlayerHud(); }, 5000);
    drawPlayerHud();
  }

  // ---- selecting blocks: drag from one block to another ----
  var selecting = false;
  // The drag on its way, or null: { pointer, start, current, frame }. It is
  // its pointer's: the box follows that pointer, whose release selects it.
  var drag = null;

  function setSelecting(on) {
    if (selecting === on) return;
    selecting = on;
    document.body.classList.toggle('mcbench-selecting', on);
    endDrag(false);
    var panel = document.getElementById('mcbench-panel');
    if (panel) updatePanel(panel);
  }

  // The solid block under a screen point. BlueMap's own interaction raycast
  // runs synchronously; its event is caught before the block popup sees it.
  function pick(clientX, clientY) {
    var detail = null;
    var grab = function (event) { detail = event.detail; event.stopPropagation(); };
    window.addEventListener('bluemapMapInteraction', grab, true);
    try {
      window.bluemap.mapViewer.handleMapInteraction(new window.BlueMap.Three.Vector2(clientX, clientY), { mcbench: true });
    } finally {
      window.removeEventListener('bluemapMapInteraction', grab, true);
    }
    var hit = detail && detail.hiresHit;
    if (!hit || !hit.face) return null;
    var normal = hit.face.normal.clone();
    if (normal.dot(detail.ray.direction) > 0) normal.negate();
    var block = hit.point.clone().addScaledVector(normal, -0.01).floor();
    return [block.x, block.y, block.z];
  }

  function dragBox() {
    return {
      min: [0, 1, 2].map(function (axis) { return Math.min(drag.start[axis], drag.current[axis]); }),
      max: [0, 1, 2].map(function (axis) { return Math.max(drag.start[axis], drag.current[axis]); }),
    };
  }

  function endDrag(commit) {
    if (!drag) return;
    var box = dragBox();
    cancelAnimationFrame(drag.frame);
    drag = null;
    if (dragMarker) {
      release(dragMarker);
      dragMarker = null;
    }
    window.bluemap.mapViewer.redraw();
    if (commit) window.parent.postMessage({ type: 'mcbench-lite:selection', min: box.min, max: box.max }, window.location.origin);
  }

  function onCanvas(event) {
    return selecting && overlay && event.button === 0 && event.target === window.bluemap.mapViewer.renderer.domElement;
  }

  window.addEventListener('pointerdown', function (event) {
    if (flight) { flight.cancel(); flight = null; }
    if (!onCanvas(event)) return;
    // Left drags draw a box instead of panning; right drag and the wheel still move the camera.
    event.stopPropagation();
    event.preventDefault();
    // Another pointer down during a drag (a second touch) starts none. The
    // drag's own pointer down again lost its release (outside the frame):
    // that drag ends, with its box, and selects nothing.
    if (drag && drag.pointer !== event.pointerId) return;
    endDrag(false);
    var block = pick(event.clientX, event.clientY);
    if (!block) return;
    drag = { pointer: event.pointerId, start: block, current: block, frame: 0 };
    dragMarker = boxMarker('mcbench-drag', block, block, 0.16);
    window.bluemap.mapViewer.redraw();
  }, true);
  window.addEventListener('mousedown', function (event) {
    if (onCanvas(event)) event.stopPropagation();
  }, true);
  window.addEventListener('pointermove', function (event) {
    if (!drag || drag.pointer !== event.pointerId || drag.frame) return;
    var x = event.clientX;
    var y = event.clientY;
    drag.frame = requestAnimationFrame(function () {
      if (!drag) return;
      drag.frame = 0;
      var block = pick(x, y);
      if (!block) return;
      drag.current = block;
      var box = dragBox();
      boxMarker('mcbench-drag', box.min, box.max, 0.16, dragMarker);
      window.bluemap.mapViewer.redraw();
    });
  }, true);
  window.addEventListener('pointerup', function (event) { if (drag && drag.pointer === event.pointerId) endDrag(true); }, true);
  window.addEventListener('pointercancel', function (event) { if (drag && drag.pointer === event.pointerId) endDrag(false); }, true);
  window.addEventListener('keydown', function (event) {
    // Escape stops selecting, and frees the mouse the player holds (the
    // browser does that). Any other is the page's: it closes the topmost of
    // its layers, the viewer filling the window among them (layers.mjs).
    if (event.key === 'Escape') {
      if (selecting) setSelecting(false);
      else if (!player || !document.pointerLockElement) window.parent.postMessage({ type: 'mcbench-lite:escape' }, window.location.origin);
    }
    // The app's shortcuts work here too: it cannot hear keys pressed in this frame.
    var combo = comboOf(event);
    if (!combo || uiState.binds.indexOf(combo) < 0) return;
    event.preventDefault();
    window.parent.postMessage({ type: 'mcbench-lite:bind', combo: combo }, window.location.origin);
  });

  // BlueMap's own camera keys (moving, flying up and down, zooming): they
  // always stay with BlueMap.
  var CAMERA_KEYS = ['KeyW', 'KeyA', 'KeyS', 'KeyD', 'Space', 'NumpadAdd', 'NumpadSubtract'];

  // Other characters typed here go to the app's message box, which then
  // takes the focus, so the rest of the message follows. The app turns this
  // off when the viewer fills the window.
  window.addEventListener('keydown', function (event) {
    if (player || !uiState.typeAhead || event.isComposing || event.metaKey || event.ctrlKey || event.altKey || event.key.length !== 1) return;
    if (CAMERA_KEYS.indexOf(event.code) >= 0) return;
    if (event.target.closest && event.target.closest('input, textarea, select, [contenteditable]')) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    window.parent.postMessage({ type: 'mcbench-lite:type', key: event.key }, window.location.origin);
  }, true);

  // ---- clicking a block: a lever is pulled, a button pressed, a door used ----
  // A left click that is not a drag, outside selecting. The page knows which
  // blocks take a click (the engine's world is its own); it ignores the rest.
  var leftDown = null;
  window.addEventListener('pointerdown', function (event) {
    leftDown = event.button === 0 && !selecting && !player && event.target === window.bluemap.mapViewer.renderer.domElement
      ? { x: event.clientX, y: event.clientY, at: Date.now() } : null;
  }, true);
  window.addEventListener('pointerup', function (event) {
    var down = leftDown;
    leftDown = null;
    if (!down || event.button !== 0 || selecting || Date.now() - down.at > 500 || Math.hypot(event.clientX - down.x, event.clientY - down.y) > 4) return;
    var block = pick(event.clientX, event.clientY);
    if (block) window.parent.postMessage({ type: 'mcbench-lite:interact', x: block[0], y: block[1], z: block[2], yaw: lookYaw() }, window.location.origin);
  }, true);

  // ---- right-click a box in the draft to take it out ----
  var rightDown = null;
  window.addEventListener('pointerdown', function (event) {
    rightDown = event.button === 2 && !player ? { x: event.clientX, y: event.clientY } : null;
  }, true);
  window.addEventListener('pointerup', function (event) {
    var down = rightDown;
    rightDown = null;
    // A right drag turns the camera; only a click removes.
    if (!down || event.button !== 2 || Math.hypot(event.clientX - down.x, event.clientY - down.y) > 4) return;
    var Three = window.BlueMap.Three;
    var viewer = window.bluemap.mapViewer;
    var rect = viewer.renderer.domElement.getBoundingClientRect();
    var raycaster = new Three.Raycaster();
    raycaster.setFromCamera(new Three.Vector2((event.clientX - rect.left) / rect.width * 2 - 1, (rect.top - event.clientY) / rect.height * 2 + 1), viewer.camera);
    var nearest = null;
    var nearestDistance = Infinity;
    uiState.selections.forEach(function (selection) {
      if (!selection.removable) return;
      var box = new Three.Box3(new Three.Vector3().fromArray(selection.min), new Three.Vector3(selection.max[0] + 1, selection.max[1] + 1, selection.max[2] + 1));
      var hit = raycaster.ray.intersectBox(box, new Three.Vector3());
      var distance = hit && hit.distanceTo(raycaster.ray.origin);
      if (hit && distance < nearestDistance) { nearest = selection; nearestDistance = distance; }
    });
    if (nearest) window.parent.postMessage({ type: 'mcbench-lite:deselect', key: nearest.key }, window.location.origin);
  }, true);
  // The parent chooses the chat before any view is drawn.
})();
