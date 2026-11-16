// The page's handle on the engine worker (core-worker.mjs): the same
// requests as server/engine.js's Engine (place, tick, input, state), with
// bytes instead of files, and the running world's frames. Requests are
// answered in the order they were made.
//
//   const engine = await CoreEngine.start('/core/<version>-<tables hash>/');
//   const { bytes, inputs } = await engine.place(commands);  // the blueprint
//   engine.onframe = ({ ticks, bytes, entities, inputs }) => ...; // bytes: the patch
//   await engine.run(true);                                  // 20 ticks a second
//   await engine.input('toggle', x, y, z, yaw);
//   const taken = await engine.click(x, y, z, yaw);          // 'toggle', ..., or null
//
// A request the engine does not model rejects with "The engine failed:
// unported: ...". The world is spent then; only place(commands, { fresh:
// true }) goes on. A world that stopped (`onfailed` says why) refuses
// run(true) and step() until then.

// The bytes of a file the server serves for the engine.
async function fetched(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} is unavailable (HTTP ${response.status})`);
  return new Uint8Array(await response.arrayBuffer());
}

// Each engine's module, compiled, and Minecraft's tables it reads, by the
// address the server serves them at (server/core-wasm.js): engine.wasm,
// blocks.txt and loot.json under it. One that failed is let go, so the next
// start fetches it again.
const engines = new Map();
function loaded(address) {
  if (!engines.has(address)) {
    const loading = Promise.all([
      fetched(new URL('engine.wasm', address).href).then((bytes) => WebAssembly.compile(bytes)),
      fetched(new URL('blocks.txt', address).href),
      fetched(new URL('loot.json', address).href),
    ]).then(([module, blocks, loot]) => ({ module, tables: { blocks, loot } }));
    loading.catch(() => engines.delete(address));
    engines.set(address, loading);
  }
  return engines.get(address);
}

export class CoreEngine {
  constructor(worker) {
    this.worker = worker;
    this.requests = new Map();
    this.counter = 0;
    // Why the worker ended, once it has: every request then fails with it.
    this.dead = null;
    // Called with each frame of the running world, and with the failure that
    // stopped it (a failed tick, a request that spent the world, or the
    // worker itself failing, which ends it).
    this.onframe = null;
    this.onfailed = null;
    worker.onmessage = ({ data }) => {
      if (data.type === 'frame') return this.onframe?.(data);
      if (data.type === 'failed') return this.onfailed?.(data.error);
      const request = this.requests.get(data.id);
      this.requests.delete(data.id);
      if (data.error !== undefined) request.reject(new Error(data.error));
      else request.resolve(data);
    };
    worker.onerror = (event) => {
      const error = new Error(`The engine worker failed: ${event.message}`);
      this.terminate(error);
      this.onfailed?.(error.message);
    };
  }

  // Starts a worker with the engine at `address` (a URL ending in /). The
  // page compiles each module and fetches its tables once, and hands them to
  // every worker it starts, so a start is a worker's and not a compilation's
  // (an engine per view, live-world.mjs).
  static async start(address) {
    const { module, tables } = await loaded(new URL(address, globalThis.location?.href).href);
    return CoreEngine.load(module, tables, new Worker(new URL('./core-worker.mjs', import.meta.url), { type: 'module' }));
  }

  // Starts a worker with a compiled module and its tables ({ blocks, loot },
  // bytes: CoreWorld). A start that fails ends its worker: nothing else
  // holds it.
  static async load(module, tables, worker) {
    const engine = new CoreEngine(worker);
    try {
      await engine.request({ type: 'load', module, tables });
    } catch (error) {
      engine.terminate(error);
      throw error;
    }
    return engine;
  }

  request(message, transfer = []) {
    if (this.dead) return Promise.reject(this.dead);
    const id = ++this.counter;
    const answered = new Promise((resolve, reject) => this.requests.set(id, { resolve, reject }));
    this.worker.postMessage({ ...message, id }, transfer);
    return answered;
  }

  // Places the commands (the live import's lines, as text) on an empty world
  // or, with fresh false, on the world as it is. Resolves with { bytes,
  // inputs }: the blueprint, and the input each entry of its palette takes
  // ('toggle', 'press', 'use' or null: CoreWorld.inputs).
  async place(commands, { fresh = true } = {}) {
    const { bytes, inputs } = await this.request({ type: 'place', commands, fresh });
    return { bytes, inputs };
  }

  // Places the commands on the world as it is, without a new base. Resolves
  // with { bytes, entities, inputs }: the patch of the blocks changed since
  // the last request, on the base the last placement wrote.
  async edit(commands) {
    const { bytes, entities, inputs } = await this.request({ type: 'edit', commands });
    return { bytes, entities: JSON.parse(entities), inputs };
  }

  // Ticks the world. Resolves with { bytes, entities, inputs, view }: the
  // patch of the blocks changed since the last request, the entities as an
  // array, the input each palette entry the patch adds takes, and the player
  // after the tick when the world has one.
  async tick(ticks = 1) {
    const { bytes, entities, inputs, view } = await this.request({ type: 'tick', ticks });
    return { bytes, entities: JSON.parse(entities), inputs, view };
  }

  // Starts or stops the world running at 20 ticks a second; resolves with
  // the ticks run since its placement. Its frames go to `onframe`.
  async run(playing) {
    return (await this.request({ type: 'run', playing })).ticks;
  }

  // One tick of a world that is not running; its frame goes to `onframe`.
  async step() {
    return (await this.request({ type: 'step' })).ticks;
  }

  // A player's input at x, y, z by a player looking with `yaw` (degrees,
  // Minecraft's: 0 south, 90 west): 'toggle' a lever, 'press' a button, or
  // 'use' a door, trapdoor, fence gate, repeater, comparator, note block,
  // daylight detector, redstone dust or decorated pot.
  async input(type, x, y, z, yaw) {
    await this.request({ type: 'input', input: type, x, y, z, yaw });
  }

  // A click on the block at x, y, z by a player looking with `yaw`: the input
  // its state takes, if any. Resolves with the input the block took, or null
  // when it takes none or passes on it, which changes nothing (CoreWorld.click).
  async click(x, y, z, yaw) {
    return (await this.request({ type: 'click', x, y, z, yaw })).input;
  }

  // Puts the creative player (the client's own movement, core/src/player) in
  // the world at x, y, z, looking as yaw and pitch (degrees) say. Resolves with
  // its view. From then on each tick of the world is also the player's, with
  // what setPlayerInput last gave, and the frames carry the view.
  async spawnPlayer({ x, y, z, yaw = 0, pitch = 0, flying = true }) {
    return (await this.request({ type: 'player', spawn: { x, y, z, yaw, pitch, flying } })).view;
  }

  // What the player holds from the next tick on: `keys` as flags (1 forward, 2
  // backward, 4 left, 8 right, 16 jump, 32 shift, 64 sprint), `taps` the keys
  // pressed since the last input (the next tick sees them held), and the look.
  async setPlayerInput({ keys, taps, yaw, pitch }) {
    await this.request({ type: 'player', input: { keys, taps, yaw, pitch } });
  }

  async removePlayer() {
    await this.request({ type: 'player', spawn: null });
  }

  // Where a player can stand on top of the columns of the box `min` to `max`
  // nearest to the feet position `near`: the feet [x, y, z], or null when no
  // column has a spot (CoreWorld.standingSpot).
  async standingSpot(min, max, near) {
    return (await this.request({ type: 'stand', min, max, near })).spot;
  }

  // What the world holds besides its blocks, as text.
  async state() {
    return (await this.request({ type: 'state' })).text;
  }

  // The world as a Minecraft save holds it (web/world-export.mjs):
  // { gameTime (BigInt), chunks: [{ x, z, nbt }] }.
  async export() {
    const { gameTime, chunks } = await this.request({ type: 'export' });
    return { gameTime: BigInt(gameTime), chunks };
  }

  // Ends the worker: what is still asked of it fails with `error`, as does
  // everything asked after, and nothing more comes from it.
  terminate(error = new Error('The engine was stopped')) {
    if (this.dead) return;
    this.dead = error;
    this.worker.onmessage = null;
    this.worker.onerror = null;
    this.worker.terminate();
    for (const request of this.requests.values()) request.reject(error);
    this.requests.clear();
  }
}
