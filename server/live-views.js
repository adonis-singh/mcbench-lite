'use strict';

// What each viewer is shown, as versions of a build: the server keeps no
// world for a viewer (the page runs the live world in its own engine, see
// web/live-world.mjs), only the builds it shows. A view is a conversation or
// a bench round (the page names it by `key`); it has a version, a number
// that changes whenever its build changes enough to place (the blocks differ),
// and the page asks for what takes the world it holds, at version `since`, to
// the latest: the same commands the live import has always sent
// (live-placement.js), a fresh placement when the page holds nothing or a
// version the server no longer keeps.

const { importCommands } = require('./live-placement');
const { renderMask, cameraHash } = require('./bluemap');
const { Y_OFFSET, validateWorldHeight } = require('./minecraft-dimension');

// Versions kept per view: a page that fell further behind places afresh.
const VERSIONS_KEPT = 16;
// A bench round's view is composed (its builds compared) at most this often;
// a conversation's is cheap to look at every time.
const LOOK_MS = 250;
// Views kept at all: the least recently looked at go first.
const VIEWS_KEPT = 64;

class LiveViews {
  // `resolve(key)` gives { world, layout } for a view, or throws (an error
  // with `statusCode` for a view that does not exist).
  constructor({ resolve, yOffset = Y_OFFSET, now = Date.now } = {}) {
    this.resolve = resolve;
    this.yOffset = yOffset;
    this.now = now;
    this.views = new Map(); // key -> { versions: [{ version, world }], source, revision, checked, descriptor }
    this.counter = 0;
    // Camera moves: a view's number goes up when a finished turn changed
    // what is worth looking at (see frame).
    this.frames = new Map();
  }

  // The view's latest version, made if its build changed since the last look.
  look(key) {
    let view = this.views.get(key);
    if (view && key.startsWith('bench:') && this.now() - view.checked < LOOK_MS) return view;
    const { world, layout } = this.resolve(key);
    if (!view) this.views.set(key, view = { versions: [], source: null, revision: null, checked: 0, descriptor: null });
    // Most recently looked at last.
    this.views.delete(key);
    this.views.set(key, view);
    while (this.views.size > VIEWS_KEPT) this.views.delete(this.views.keys().next().value);
    view.checked = this.now();
    const unchanged = view.source === world && view.revision === world.revision && JSON.stringify(view.layout) === JSON.stringify(layout || null);
    if (unchanged && view.descriptor) return view;
    validateWorldHeight(world, this.yOffset);
    const reference = world.reference();
    const last = view.versions.at(-1);
    view.source = world;
    view.revision = world.revision;
    view.layout = layout || null;
    // The build as the page's engine places it: the same blocks give the same version.
    if (last && JSON.stringify(last.layout) === JSON.stringify(view.layout) && !importCommands(reference, last.world, this.yOffset).keys.length) return view;
    const bounds = reference.bounds();
    view.versions.push({ version: ++this.counter, world: reference, layout: view.layout });
    while (view.versions.length > VERSIONS_KEPT) view.versions.shift();
    view.descriptor = {
      key,
      version: this.counter,
      mask: renderMask(bounds),
      cameraHash: cameraHash(bounds, this.yOffset),
      bounds,
      blockCount: reference.size(),
      layout: view.layout,
    };
    return view;
  }

  // The view as the page lists it (without the commands).
  // A build that cannot be placed (taller than the world) leaves the viewer
  // with the version before it, and says why.
  describe(key) {
    let syncError = null;
    try {
      this.look(key);
    } catch (error) {
      if (!this.views.get(key)?.descriptor) throw error;
      syncError = `Minecraft kept the previous build: ${String(error.message || error)}`;
    }
    return { ...this.views.get(key).descriptor, frame: this.frames.get(this.frameKey(key)) || 0, syncError };
  }

  // The commands that take the world at version `since` (or nothing) to the
  // latest: { version, fresh, commands, blockCount }, `commands` the import's lines
  // as text.
  imports(key, since) {
    this.describe(key);
    const view = this.views.get(key);
    const latest = view.versions.at(-1);
    const held = since == null ? null : view.versions.find((entry) => entry.version === since);
    const blockCount = latest.world.size();
    if (held === latest) return { version: latest.version, fresh: false, commands: '', blockCount };
    // A version's fresh placement is the same for every page that opens it.
    if (!held) {
      latest.fresh ??= importCommands(latest.world, null, this.yOffset).commands.map((command) => `${command}\n`).join('');
      return { version: latest.version, fresh: true, commands: latest.fresh, blockCount };
    }
    const { commands } = importCommands(latest.world, held.world, this.yOffset);
    return { version: latest.version, fresh: false, commands: commands.map((command) => `${command}\n`).join(''), blockCount };
  }

  frameKey(key) {
    return key.startsWith('bench:') ? key.split(':').slice(0, 2).join(':') : key;
  }

  // Asks the viewers of a view to move the camera to it once its current
  // version is on screen. Benches count for all their rounds.
  frame(key) {
    const frameKey = this.frameKey(key);
    this.frames.set(frameKey, (this.frames.get(frameKey) || 0) + 1);
  }
}

module.exports = { LiveViews, VERSIONS_KEPT };
