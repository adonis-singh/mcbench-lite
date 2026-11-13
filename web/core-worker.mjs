// The live world's engine, off the page's main thread: the Rust engine as
// WebAssembly (core-world.mjs) in a Web Worker. It takes requests one at a
// time, in order, each answered with { id, ... } or { id, error }:
//   { id, type: 'load', wasm }
//       the first request: fetches and compiles the module
//   { id, type: 'place', commands, fresh }
//       places the commands (text, one per line) on an empty world or on the
//       world as it is; answers { bytes } with the blueprint
//   { id, type: 'edit', commands }
//       places the commands on the world as it is; answers { bytes, entities }
//       with the patch of the blocks changed since the last request (the base
//       stays the one the last placement wrote)
//   { id, type: 'tick', ticks }
//       answers { bytes, entities, view }: the patch of the blocks changed
//       since the last request (web/live-patch.mjs)
//   { id, type: 'input', input: 'toggle'|'press'|'use', x, y, z, yaw }
//   { id, type: 'player', spawn: { x, y, z, yaw, pitch, flying } | null }
//       puts the creative player in the world (null removes it); answers { view }
//   { id, type: 'player', input: { keys, taps, yaw, pitch } }
//       what the player holds from the next tick on (the keys as flags: 1
//       forward, 2 backward, 4 left, 8 right, 16 jump, 32 shift, 64 sprint,
//       and the look); a world with a player ticks it with this on every tick.
//       `taps` are keys pressed since the last input: the next tick sees them
//       held even if they were let go before it, as a key tapped between two
//       of the client's ticks is (a double-tapped jump is two presses)
//   { id, type: 'state' }               answers { text }
//   { id, type: 'export' }              answers { gameTime, chunks } (CoreWorld.export)
//   { id, type: 'run', playing }        ticks the world at Minecraft's 20 a second
//       (or stops); answers { ticks }. Each tick is a frame, below.
//   { id, type: 'step' }                one tick while not playing (a frame too)
// and tells, with no request to answer:
//   { type: 'frame', ticks, bytes, entities, view }
//       a tick of the running world; `ticks` since its fresh placement, `view`
//       the player after it (Float64Array of 28, see CoreWorld.spawnPlayer) when
//       the world has one
//   { type: 'failed', error }
//       the world stopped: a tick failed (running or stepped), a fresh
//       placement failed, or a request spent the world. A stopped world
//       refuses to play or step, and tells nothing more, until a fresh
//       placement
// A failure answers "The engine failed: <reason>". A trap of the module (a
// reason starting "unported:" is a world the engine does not model) spends
// the world, and only a fresh placement goes on.
import { CoreWorld } from './core-world.mjs';

const TICK_MS = 50;
// A world that fell this far behind does not race to catch up.
const LAG_MS = 1000;

let world = null;

// The ticks run since the fresh placement.
let ticks = 0;
// Whether the world runs, and what each state can go to (a fresh placement
// starts the world over, paused):
//   paused    it ticks only when stepped
//   playing   a tick every 50 ms
//   stopped   a failure stopped it (stop): it neither plays nor steps
const NEXT = {
  paused: ['playing', 'stopped'],
  playing: ['paused', 'stopped'],
  stopped: [],
};
let running = 'paused';
let timer = null;

function to(next) {
  if (!NEXT[running].includes(next)) throw new Error(`The world cannot go from ${running} to ${next}`);
  running = next;
}

// The player in the world, and what it holds. A fresh placement takes it:
// the page spawns it again where it stood and gives it what it held
// (live-world.mjs placeAfresh).
let player = null; // { keys, taps, yaw, pitch }

function frame(result) {
  ticks += 1;
  const transfer = [result.bytes.buffer];
  if (result.view) transfer.push(result.view.buffer);
  self.postMessage({ type: 'frame', ticks, bytes: result.bytes, entities: result.entities, view: result.view }, transfer);
}

// One tick of the world, with its player if it has one.
function worldTick() {
  if (!player) return world.tick(1);
  const keys = player.keys | player.taps;
  player.taps = 0;
  return world.playerTick(keys, player.yaw, player.pitch);
}

// The world stops, and the page is told why.
function stop(error) {
  to('stopped');
  clearTimeout(timer);
  self.postMessage({ type: 'failed', error: String(error.message || error) });
}

// Whether a request that failed stopped the world: a fresh placement (which
// started it over, paused), and on a world not stopped before it a step
// whose tick failed (a playing world refuses to step without ticking) or any
// request that spent the world.
function stops(request, stopped) {
  if (request.type === 'place' && request.fresh) return true;
  if (stopped) return false;
  return (request.type === 'step' && running !== 'playing') || world.refused !== null;
}

// Ticks on the clock: tick `n` is due n * 50 ms after the world started
// running, so a late timer is made up for and a slow tick does not push the
// ones after it back; a world over a second behind starts counting again.
function run(start = performance.now(), n = 0) {
  const due = start + n * TICK_MS;
  const wait = due - performance.now();
  timer = setTimeout(() => {
    if (running !== 'playing') return;
    try {
      frame(worldTick());
    } catch (error) {
      return stop(error);
    }
    const now = performance.now();
    if (now - (start + (n + 1) * TICK_MS) > LAG_MS) run(now, 0);
    else run(start, n + 1);
  }, Math.max(0, wait));
}

async function handle(request) {
  if (request.type === 'load') {
    const response = await fetch(request.wasm);
    if (!response.ok) throw new Error(`${request.wasm} is unavailable (HTTP ${response.status})`);
    world = new CoreWorld(await WebAssembly.compile(await response.arrayBuffer()));
    return [{}];
  }
  switch (request.type) {
    case 'place': {
      if (request.fresh) {
        ticks = 0;
        player = null;
        running = 'paused';
        clearTimeout(timer);
      }
      const bytes = world.place(request.commands, { fresh: request.fresh });
      return [{ bytes }, [bytes.buffer]];
    }
    case 'edit': {
      const result = world.edit(request.commands);
      return [{ bytes: result.bytes, entities: result.entities }, [result.bytes.buffer]];
    }
    case 'tick': {
      // A world with a player ticks the player with it, one tick at a time.
      if (player && request.ticks !== 1) throw new Error('A world with a player ticks one tick at a time');
      const result = player ? worldTick() : world.tick(request.ticks);
      const transfer = [result.bytes.buffer];
      if (result.view) transfer.push(result.view.buffer);
      return [{ bytes: result.bytes, entities: result.entities, view: result.view }, transfer];
    }
    case 'step':
      if (running === 'playing') throw new Error('Pause the live world to step it');
      if (running === 'stopped') throw new Error('The world cannot step while stopped');
      frame(worldTick());
      return [{ ticks }];
    case 'input':
      world.input(request.input, request.x, request.y, request.z, request.yaw);
      return [{}];
    case 'player':
      if (request.input) {
        if (!player) throw new Error('There is no player in the world');
        const { keys, taps, yaw, pitch } = request.input;
        player = { keys, taps: player.taps | taps, yaw, pitch };
        return [{}];
      }
      if (request.spawn) {
        const { x, y, z, yaw, pitch, flying } = request.spawn;
        const view = world.spawnPlayer(x, y, z, yaw, pitch, flying);
        player = { keys: 0, taps: 0, yaw, pitch };
        return [{ view }, [view.buffer]];
      }
      // A spent world's player went with it.
      if (world.refused === null) world.removePlayer();
      player = null;
      return [{}];
    case 'state':
      return [{ text: world.state() }];
    case 'export': {
      // A save that reaches what the engine does not model spends the world as a tick would.
      const save = world.export();
      return [{ gameTime: String(save.gameTime), chunks: save.chunks.map(({ x, z, nbt }) => ({ x, z, nbt: nbt.slice() })) }];
    }
    case 'run':
      // A stopped world refuses to play; told to pause, it stays as it is.
      if (request.playing && running !== 'playing') to('playing');
      if (!request.playing && running === 'playing') to('paused');
      clearTimeout(timer);
      if (running === 'playing') run();
      return [{ ticks }];
    default:
      throw new Error(`Unknown request ${request.type}`);
  }
}

// Requests are handled in the order they arrived.
let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  queue = queue.then(async () => {
    const stopped = running === 'stopped';
    try {
      const [reply, transfer] = await handle(data);
      self.postMessage({ id: data.id, ...reply }, transfer || []);
    } catch (error) {
      if (data.type !== 'load' && stops(data, stopped)) stop(error);
      self.postMessage({ id: data.id, error: String(error.message || error) });
    }
  });
};
