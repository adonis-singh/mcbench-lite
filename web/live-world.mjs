// The live world of the build on show, in the page. Each viewer (a tab) runs
// the world of the build it shows in an engine of its own: the Rust engine as
// WebAssembly in a Web Worker (core-engine.mjs), ticking at Minecraft's 20 a
// second, with no Minecraft server and nothing shared with other tabs. The
// server only says what to show (live-views.js): the build's version, and the
// commands that take the world the page holds to the build's latest version,
// the very import the live world has always been given (live-placement.js).
// The page places them: afresh when it holds nothing of this build, else on
// the world as it is, so an edit keeps the water flowing and the clocks
// ticking.
//
// What the engine writes goes straight to the viewer: the blueprint of each
// placement (a base, which the mesher loads) and the patch of each tick (which
// it applies, growing or shrinking the world with it), with the entities (see
// onFrame). A world that reaches what the engine does not model
// ("unported: ...") stops, with the reason shown where the build is; a fresh
// placement starts it again.

import { Changes, InputBlocks, emptyBlueprint, readBlueprint, readPatch } from './live-patch.mjs';

const POLL_MS = 1000;
const BUSY_POLL_MS = 50;
const TICKS_DRAWN_MS = 250;
// Bases kept as URLs: the viewer's workers may still be fetching an old one.
const BASES_KEPT = 4;

// The world's phase, and the phases each can go to (show and the end start a
// world over in idle or waiting):
//   idle      no view on show
//   waiting   a view, in step with the server's latest version or about to be
//   syncing   a sync in flight, bringing the world to the view's version
//   failed    the last sync failed (`failure` says why): none runs until the
//             server has another version of the view or a restart is asked for
const NEXT = {
  idle: [],
  waiting: ['syncing'],
  syncing: ['waiting', 'failed'],
  failed: ['syncing', 'waiting'],
};

// Whether the world runs, and what each state can go to (a world placed
// afresh starts over, playing):
//   playing   ticking at Minecraft's 20 a second, once it is placed
//   paused    stepped one tick at a time
//   stopped   the engine stopped it (`engineError` says why): it neither
//             plays nor steps until it is placed afresh
const RUN_NEXT = {
  playing: ['paused', 'stopped'],
  paused: ['playing', 'stopped'],
  stopped: [],
};

// Whether a player stands in the world, and what each state can go to (the
// worker holds the same fact, core-worker.mjs, and takes requests in the
// order the page asks them):
//   none       no player
//   spawning   its first spawn is asked of the engine and not answered yet
//   standing   it stands in the world (`playerView` is what it was last
//              spawned with); a respawn keeps it standing
const PLAYER_NEXT = {
  none: ['spawning'],
  spawning: ['standing', 'none'],
  standing: ['standing', 'none'],
};

export class LiveWorld {
  // `host`: what the page provides, so the world runs under tests as well:
  //   json(path, { method, body }) -> parsed JSON (rejects with the server's error)
  //   startEngine(wasmUrl) -> a CoreEngine
  //   blobUrl(bytes) / revoke(url) -> a URL for a blueprint's bytes
  //   sleep(ms) -> a promise
  constructor(host) {
    this.host = host;
    this.listeners = new Set();
    this.frameListeners = new Set();
    this.goneListeners = new Set();
    this.key = null;
    // Bumped when what is in flight stops counting (another view, another
    // engine, the end): each reply checks the generation it was asked in.
    this.generation = 0;
    // The engine, once one is asked for: { address, started, engine } (engine
    // null until started resolves).
    this.slot = null;
    this.server = { assets: {}, core: null, coreError: null };
    this.pollError = null; // why the last look at the server failed
    this.bases = 0;
    this.urls = [];
    this.waiter = null;
    this.woken = false;
    this.emitted = 0;
    this.trailing = false;
    // Player requests, one after another (a removal waits for the spawn before it).
    this.players = Promise.resolve();
    this.disposed = false;
    this.reset();
    this.state = this.snapshot();
  }

  get engine() {
    return this.slot?.engine ?? null;
  }

  // The world held for the view on show: nothing yet.
  reset() {
    this.phase = this.key ? 'waiting' : 'idle';
    this.failure = null; // { version, message } of the failed sync, in phase failed
    this.restarting = false; // a restart asked for, until a sync has placed the world afresh for it
    this.running = 'playing';
    this.engineError = null; // why the engine stopped the world, while it is stopped
    this.version = null; // of the view the engine's world was placed from
    this.shownVersion = null; // of the view the viewer shows (a build with no blocks has no world)
    this.view = null; // the server's latest description of the view
    this.ticks = 0;
    this.base = null; // { number, url, view, empty }
    this.changes = new Changes();
    this.entities = [];
    this.inputs = new InputBlocks();
    this.syncError = null;
    this.player = 'none';
    this.playerView = null; // the view the creative player was spawned with, while it stands in the world
  }

  to(phase, failure = null) {
    if (!NEXT[this.phase].includes(phase)) throw new Error(`The live world cannot go from ${this.phase} to ${phase}`);
    if ((phase === 'failed') !== (failure !== null)) throw new Error('A failed sync, and only one, has a failure');
    this.phase = phase;
    this.failure = failure;
  }

  runTo(running, reason = null) {
    if (!RUN_NEXT[this.running].includes(running)) throw new Error(`The live world cannot go from ${this.running} to ${running}`);
    if ((running === 'stopped') !== (reason !== null)) throw new Error('A stopped world, and only one, has a reason');
    this.running = running;
    this.engineError = reason;
  }

  playerTo(player, view = null) {
    if (!PLAYER_NEXT[this.player].includes(player)) throw new Error(`The player cannot go from ${this.player} to ${player}`);
    if ((player === 'standing') !== (view !== null)) throw new Error('A standing player, and only one, has a view');
    this.player = player;
    this.playerView = view;
  }

  // The status the page draws from (app.js): the shape the server's live
  // world has always reported, from the page's own world.
  snapshot() {
    const view = this.base?.view ?? null;
    const { assets, coreError } = this.server;
    const error = this.failure?.message ?? this.engineError ?? this.pollError;
    let status = 'idle';
    if (this.key) status = error ? 'error' : !this.base ? 'starting' : this.phase === 'syncing' ? 'syncing' : 'running';
    return {
      status,
      conversationId: this.key,
      revision: this.version,
      address: null,
      error: error ?? coreError ?? null,
      syncError: this.syncError,
      progress: null,
      phase: this.phase === 'syncing' ? 'placing' : null,
      // The last sync failed: only a restart (or another version) tries again.
      syncFailed: this.phase === 'failed',
      frame: this.view?.frame ?? 0,
      minecraftVersion: assets.minecraftVersion ?? null,
      yOffset: assets.yOffset ?? 64,
      viewerUrl: assets.viewerUrl ?? null,
      mesher: assets.mesher ?? null,
      viewerError: assets.viewerError ?? null,
      viewerRevision: this.base?.number ?? null,
      viewerBlueprint: this.base?.url ?? null,
      viewerMask: view?.mask ?? null,
      cameraHash: view?.cameraHash ?? null,
      bounds: view?.bounds ?? null,
      blockCount: view?.blockCount ?? null,
      layout: view?.layout ?? null,
      previewView: false,
      live: this.base && !this.base.empty ? { playing: this.running === 'playing', stopped: this.running === 'stopped', ticks: this.ticks } : null,
    };
  }

  emit() {
    this.emitted = performance.now();
    this.state = this.snapshot();
    for (const listener of this.listeners) listener(this.state);
  }

  // An emit now, or, within TICKS_DRAWN_MS of the last, one when that has
  // passed: the tick count is drawn a few times a second, and the last count
  // is always drawn.
  emitSoon() {
    const wait = TICKS_DRAWN_MS - (performance.now() - this.emitted);
    if (wait <= 0) return this.emit();
    if (this.trailing) return;
    this.trailing = true;
    this.host.sleep(wait).then(() => {
      this.trailing = false;
      this.emit();
    });
  }

  subscribe(listener) {
    this.listeners.add(listener);
    listener(this.state);
    return () => this.listeners.delete(listener);
  }

  // Each tick of the world, to the viewer: `listener({ base, patch, entities,
  // view, ticks })`, `base` the number of the base the patch applies to, and
  // the player's view if one is in it.
  onFrame(listener) {
    this.frameListeners.add(listener);
    return () => this.frameListeners.delete(listener);
  }

  // The world on show since its base, as one frame, for a viewer that took
  // the base later: the patch of everything that happened since (null when
  // nothing did) and the entities.
  accumulated() {
    if (!this.base || this.base.empty) return null;
    return { base: this.base.number, patch: this.changes.bounds ? this.changes.patch() : null, entities: this.entities };
  }

  // --- the view on show ---

  // Starts looking at the server, until `dispose`.
  async begin() {
    while (!this.disposed) {
      let busy = false;
      const generation = this.generation;
      try {
        busy = await this.look();
      } catch (error) {
        if (generation === this.generation) {
          this.pollError = String(error.message || error);
          this.emit();
        }
      }
      if (!this.woken) {
        await new Promise((resolve) => {
          this.waiter = resolve;
          this.host.sleep(busy ? BUSY_POLL_MS : POLL_MS).then(resolve);
        });
      }
      this.woken = false;
      this.waiter = null;
    }
  }

  // Shows the view `key` (a conversation id or a bench round) from now on, or
  // none: the world held for the view before is let go.
  async show(key) {
    if (key === this.key) return;
    this.generation += 1;
    this.key = key;
    // Asked before anything of the new view, so the old world's last frames
    // come before it.
    const paused = this.engine?.run(false);
    const urls = this.urls.splice(0);
    this.reset();
    this.emit();
    this.wake();
    await paused;
    for (const url of urls) this.host.revoke(url);
  }

  // Looks at the server now, not at the next poll: the page knows the build
  // on show changed (a build call finished, a revert, a branch). A wake
  // during a look makes another look right after it.
  wake() {
    this.woken = true;
    this.waiter?.();
  }

  // One look at the server: the viewer's assets, the engine module, and the
  // view's latest version, which the world is brought to. True while the page
  // is still catching up (look again soon).
  async look() {
    const generation = this.generation;
    const key = this.key;
    const status = await this.host.json(`/api/minecraft/status${key ? `?view=${encodeURIComponent(key)}` : ''}`);
    if (generation !== this.generation) return true;
    const { core, coreError, view, ...assets } = status;
    this.server = { assets, core, coreError };
    this.view = view;
    this.pollError = null;
    this.syncError = view?.syncError ?? null;
    // Another engine module: the world goes with the old engine, and the view
    // is placed afresh in the new one.
    if (core && this.slot && core !== this.slot.address) {
      this.dropEngine();
      this.generation += 1;
      if (this.phase === 'syncing' || this.phase === 'failed') this.to('waiting');
      this.version = null;
      this.shownVersion = null;
      this.playerGone();
    }
    this.pump();
    this.emit();
    return this.phase === 'syncing' || (!core && !coreError);
  }

  // Whether the world is behind what it should show: the view's latest
  // version or a restart asked for. Neither is tried again on the version a
  // sync failed on: a restart asked after the failure does that (control).
  behind() {
    if (!this.key || !this.view || !this.server.core) return false;
    if (this.view.version === this.failure?.version) return false;
    return this.restarting || this.view.version !== this.shownVersion;
  }

  // Starts a sync when the world is behind and none is in flight. A sync
  // that ends looks again, so what was asked for during it follows.
  pump() {
    if (this.phase === 'syncing' || !this.behind()) return;
    const generation = this.generation;
    const view = this.view;
    this.to('syncing');
    this.sync(generation, view).catch((error) => {
      if (generation !== this.generation) return;
      this.to('failed', { version: view.version, message: String(error.message || error) });
      this.emit();
      this.pump();
    });
  }

  // --- the engines ---

  // The engine of the server's module, started on the first sync that needs
  // it. A failed start is tried again by the next sync that needs one.
  engineReady() {
    if (!this.slot) {
      const slot = { address: this.server.core, started: null, engine: null };
      slot.started = this.host.startEngine(slot.address).then((engine) => {
        engine.onframe = (frame) => this.receive(frame.ticks, frame.bytes, JSON.parse(frame.entities), frame.view);
        engine.onfailed = (message) => this.stopped(engine, message);
        slot.engine = engine;
        return engine;
      }, (error) => {
        if (this.slot === slot) this.slot = null;
        throw error;
      });
      this.slot = slot;
    }
    return this.slot.started;
  }

  // The engine goes (once started, if it is starting); nothing more comes from it.
  dropEngine() {
    this.slot.started.then((engine) => engine.terminate(), () => {});
    this.slot = null;
  }

  // Brings the world to `view`'s version (`view`, the server's description
  // when the sync began, is what the base is drawn with). A restart asks for
  // the whole world, and stays asked for until its placement is.
  async sync(generation, view) {
    const restart = this.restarting;
    const held = this.version;
    this.emit();
    const since = restart || held === null ? '' : `&since=${held}`;
    const imported = await this.host.json(`/api/minecraft/import?view=${encodeURIComponent(this.key)}${since}`);
    if (generation !== this.generation) return;
    // The commands are for the world they were asked from and the version
    // `view` describes. With that world gone meanwhile (stopped), or a later
    // version made between the poll and the import, they are placed on
    // nothing: the page looks again, and the sync after it asks again.
    if (this.version !== held || imported.version !== view.version) {
      this.to('waiting');
      this.wake();
      return;
    }
    const { fresh } = imported;
    // A world placed afresh starts over: one that stopped on an error plays again.
    if (fresh && this.running === 'stopped') {
      this.running = 'playing';
      this.engineError = null;
    }
    if (imported.blockCount === 0) return this.showEmpty(imported, view, generation, restart);
    let blueprint = null;
    let engine = null;
    if (fresh || imported.commands !== '') {
      engine = await this.engineReady();
      if (generation !== this.generation) return;
      // An edit within the area the viewer draws (its render mask) reaches it
      // as a patch on the base it holds, as a tick does; one past it needs a
      // new base, drawn with the new mask.
      if (!fresh && JSON.stringify(this.base.view.mask) === JSON.stringify(view.mask)) {
        const { bytes, entities } = await engine.edit(imported.commands);
        if (generation !== this.generation) return;
        this.base.view = view;
        this.receive(this.ticks, bytes, entities);
      } else {
        // A world placed afresh holds no player: it is let go before the
        // placement is asked, so no input asked after it reaches a player
        // the placement took. The world held goes with it, and the restart
        // is carried out.
        if (fresh) {
          this.playerGone();
          this.version = null;
          if (restart) this.restarting = false;
        }
        blueprint = await engine.place(imported.commands, { fresh });
        if (generation !== this.generation) return;
      }
    }
    if (fresh) this.ticks = 0;
    this.version = imported.version;
    this.shownVersion = imported.version;
    if (blueprint) this.publishBase(blueprint, view);
    // A world placed afresh runs (a paused one stays paused).
    if (fresh && this.running === 'playing') {
      await engine.run(true);
      if (generation !== this.generation) return;
    }
    this.to('waiting');
    this.emit();
    this.pump();
  }

  // A build with no blocks has nothing to place and nothing to tick: the
  // viewer shows an empty world, and the next build is placed afresh.
  async showEmpty(imported, view, generation, restart) {
    if (this.engine) await this.engine.run(false);
    if (generation !== this.generation) return;
    if (restart) this.restarting = false;
    this.version = null;
    this.shownVersion = imported.version;
    this.ticks = 0;
    this.publishBase(emptyBlueprint(), view);
    this.base.empty = true;
    this.to('waiting');
    this.emit();
    this.pump();
  }

  // A new base for the viewer: the blueprint a placement wrote, to be loaded.
  // The caller emits it once the sync has ended, so the page never sees a
  // base still marked syncing.
  publishBase(blueprint, view) {
    this.bases += 1;
    const url = this.host.blobUrl(blueprint);
    this.urls.push(url);
    while (this.urls.length > BASES_KEPT) this.host.revoke(this.urls.shift());
    this.base = { number: this.bases, url, view, empty: false };
    this.changes = new Changes();
    this.inputs.load(readBlueprint(blueprint));
  }

  // --- frames ---

  // A tick of the world: its patch. Ticks the engine ran before it heard of
  // another view (or of an empty one) are of a world no longer on show.
  receive(ticks, bytes, entities, view = null) {
    if (!this.base || this.base.empty) return;
    this.ticks = ticks;
    this.entities = entities;
    const patch = readPatch(bytes);
    this.changes.add(patch);
    this.inputs.apply(patch);
    for (const listener of this.frameListeners) listener({ base: this.base.number, patch: bytes, entities: this.entities, view, ticks });
    this.emitSoon();
  }

  // The engine stopped the world: the error is shown where the build is,
  // until the world is placed afresh. The first reason stands: what a stopped
  // world fails with after it says less. An engine whose worker failed is
  // gone, with its world: the next sync starts another.
  stopped(engine, message) {
    if (this.running !== 'stopped') this.runTo('stopped', `The live world stopped: ${message}`);
    if (engine.dead) {
      this.slot = null;
      this.version = null;
      this.playerGone();
    }
    this.emit();
  }

  // The engine of the world on show.
  shownEngine() {
    if (!this.engine || !this.base || this.base.empty) throw new Error('There is no live world on show');
    return this.engine;
  }

  // --- the creative player ---
  // The client's own movement (core/src/player) in the browser's engine: the
  // player-mode controls drive it. Frames carry its view (listeners of
  // onFrame get `view`, 28 numbers, see CoreWorld.spawnPlayer). Player
  // requests run one after another.

  // `listener()` when the world lets its player go on its own (placed afresh,
  // or its engine gone), not on removePlayer or another view.
  onPlayerGone(listener) {
    this.goneListeners.add(listener);
    return () => this.goneListeners.delete(listener);
  }

  // A spawn not answered yet is let go too: the engine takes requests in
  // order, so the placement (or the end) that lets it go comes after it.
  playerGone() {
    if (this.player === 'none') return;
    this.playerTo('none');
    for (const listener of this.goneListeners) listener();
  }

  playerTurn(work) {
    const turn = this.players.then(work);
    this.players = turn.catch(() => {});
    return turn;
  }

  // Rejects when the player does not stand in the world on show: the spawn
  // failed, or the world let the player go before the engine answered.
  spawnPlayer(spawn) {
    return this.playerTurn(async () => {
      const engine = this.shownEngine();
      if (this.player === 'none') this.playerTo('spawning');
      let view;
      try {
        view = await engine.spawnPlayer(spawn);
      } catch (error) {
        if (this.player === 'spawning') this.playerTo('none');
        throw error;
      }
      if (this.player === 'none') throw new Error('The world let the player go before it stood in it');
      this.playerTo('standing', view);
      return view;
    });
  }

  // Input for a player no longer in the world (a fresh placement took it)
  // goes nowhere.
  setPlayerInput(input) {
    return this.playerTurn(async () => {
      if (this.playerView) await this.engine.setPlayerInput(input);
    });
  }

  // Takes the player out of the world, at any time: after the spawn asked
  // before it, and with nothing to do when no player stands in the world.
  removePlayer() {
    return this.playerTurn(async () => {
      if (this.player !== 'standing') return;
      await this.engine.removePlayer();
      // The world may have let it go meanwhile.
      if (this.player === 'standing') this.playerTo('none');
    });
  }

  // --- blocks ---

  // A click on the block at x, y, z by a player looking with `yaw` (degrees,
  // Minecraft's): a lever is pulled, a button pressed, a door, trapdoor,
  // fence gate, repeater, comparator, note block, daylight detector, decorated
  // pot or dot of redstone dust used (inputOf).
  // Resolves false for any other block. The yaw is a 32-bit float on
  // every engine.
  async input(x, y, z, yaw) {
    const kind = this.inputs.at(x, y, z);
    if (!kind) return false;
    await this.shownEngine().input(kind, x, y, z, Math.fround(yaw));
    return true;
  }

  // Play, pause, one tick while paused, or the build placed afresh (after
  // the sync in flight, if one is). A stopped world takes only the last.
  async control(action) {
    if (action === 'restart') {
      this.restarting = true;
      // Asked after a sync failed, it tries that version again.
      if (this.phase === 'failed') this.to('waiting');
      this.pump();
      this.emit();
      return;
    }
    const engine = this.shownEngine();
    if (action === 'play' || action === 'pause') {
      const running = action === 'play' ? 'playing' : 'paused';
      if (this.running !== running) this.runTo(running);
      this.emit();
      await engine.run(running === 'playing');
    } else if (action === 'step') {
      if (this.running === 'stopped') throw new Error('The live world cannot step while stopped');
      await engine.step();
    } else {
      throw new Error(`Unknown live world action: ${action}`);
    }
  }

  // The build as a Minecraft world save, as it stands: { folder, bytes } (a zip).
  async exportWorld(name) {
    const engine = this.shownEngine();
    const { bounds } = this.base.view;
    const { yOffset } = this.state;
    const { worldZip } = await import('./world-export.mjs');
    const save = await engine.export();
    const spawn = [Math.round((bounds.min[0] + bounds.max[0]) / 2), Math.min(959, bounds.max[1] + yOffset + 8), Math.round((bounds.min[2] + bounds.max[2]) / 2)];
    return worldZip({ name, gameTime: save.gameTime, chunks: save.chunks }, spawn);
  }

  // --- ending ---

  dispose() {
    this.disposed = true;
    this.generation += 1;
    this.wake();
    if (this.slot) this.dropEngine();
    for (const url of this.urls.splice(0)) this.host.revoke(url);
    this.base = null;
    this.player = 'none';
    this.playerView = null;
    this.listeners.clear();
    this.frameListeners.clear();
    this.goneListeners.clear();
  }
}
