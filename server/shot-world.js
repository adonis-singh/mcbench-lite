// The build as Minecraft makes it, which see_build draws and the build tool
// reports (agent.js) and reads back (mc.settled). The engine places the
// conversation's blocks as the live import does (Minecraft's own block
// updates: connections, shapes, blocks that cannot stay), runs ticks (falling
// blocks fall, water and lava spread, scheduled ticks fire), and the
// blueprint it then writes becomes the World the screenshot page draws, with
// the states the engine set. A fresh placement of the whole blueprint is what
// a settle is, so a world is settled once for each number of ticks asked: the
// settles of a conversation's world are kept until the world changes. Several
// numbers of ticks asked at once are one run of the engine to the largest,
// with the world written at each (ticking, and writing the world, are the
// same as a separate run to that number). The engines are the screenshots'
// own, so the live world on show is never disturbed. Settles of different
// conversations run at once on a pool of engines (engine-pool.js); those of
// one conversation run in the order they were asked.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { coreEngine } = require('./engine');
const { EnginePool, poolSize } = require('./engine-pool');
const { minecraftLive } = require('./minecraft-live');
const { importCommands } = require('./live-placement');
const { blueprintWorld } = require('./blueprint');
const { World, MAX_COORD } = require('./world');
const { DEFAULT_SETTLE_TICKS } = require('./minecraft-dimension');
const { BUDGETS, BUDGET_ENV } = require('./verification-contract');

let pool = null;
// What is kept of each conversation settled, the one settled last at the
// end: `worlds`, the settles of its worlds, the latest first and at most
// two (a build settles its candidate before it is applied, and if it is not,
// the world it started from is still the conversation's): for each, the world
// it settled (the same object, at the same revision, or it is settled
// again), and by ticks the settles running (`pending`) and done (`done`); a
// tick is in one of the two at most. And `latest`, its latest engine world:
// what the next settle's palette and unchanged chunks are taken from.
// A conversation is kept or it is not: one dropped leaves with all of it,
// and a settle of it that ends later writes to what was dropped, which
// nothing here holds.
const kept = new Map();
const KEPT_WORLDS = 2;
const entryOf = (conversationId, world) => kept.get(conversationId)?.worlds.find((entry) => entry.world === world && entry.revision === world.revision);
// The conversations settled most recently stay, so a long-running server
// does not hold the settled world of every conversation it ever built.
const KEPT_CONVERSATIONS = 16;
const CHANGE_EXAMPLES = 8;
const KEPT_CLIPS = 8;

// A member of the pool: a Rust engine in a folder of its own, started now.
// One whose engine died is retired by the pool (alive), never restarted.
function makeMember(slot) {
  const engine = coreEngine();
  const warming = engine.warm({ folder: path.join(minecraftLive.engineFolder, slot === 0 ? 'shot' : `shot-${slot}`) });
  warming.catch(() => {});
  return { engine, warming, alive: () => engine.alive, stop: () => engine.stop() };
}

// A benchmark's attempts settle on its shared engine pool: `makePool()`
// answers an object with the EnginePool interface (run, drop, acquire,
// release, stop) whose members hold an engine with place and tick.
const shotOptions = { makePool: null };

function shotPool() {
  pool ||= shotOptions.makePool ? shotOptions.makePool() : new EnginePool({ size: poolSize(), make: makeMember, wallMs: BUDGETS.wallMs });
  return pool;
}

// A number of ticks within what test_build may simulate (the operator's
// MCBENCH_VERIFY_MAX_SIMULATION_TICKS budget).
function checkTicks(value, label) {
  if (!Number.isInteger(value) || value < 0) throw new Error(`${label} must be a whole number of ticks, 0 or more`);
  if (value > BUDGETS.simulationTicks) {
    throw new Error(`${label} is ${value} ticks; the budget of simulated ticks, the same as test_build's, is ${BUDGETS.simulationTicks} (${BUDGET_ENV.simulationTicks})`);
  }
  return value;
}

// Chunks of `world` whose blocks equal `previous`'s (same palette) become
// `previous`'s chunk objects, with their versions, so the screenshot page
// keeps their meshes.
function adoptUnchanged(world, previous) {
  if (!previous) return;
  for (const [key, chunk] of world.chunks) {
    const before = previous.chunks.get(key);
    if (!before || before.count !== chunk.count) continue;
    const a = before.view();
    const b = chunk.view();
    let same = true;
    for (let index = 0; index < a.length && same; index += 1) same = a[index] === b[index];
    if (!same) continue;
    world.chunks.set(key, before);
    world.ownedChunks.delete(key);
  }
}

// Where the settled world's block ids differ from the authored world's:
// counts of blocks removed, added and replaced, with a few of each.
function idChanges(authored, settled) {
  const changes = { removed: 0, added: 0, replaced: 0, examples: { removed: [], added: [], replaced: [] } };
  const keys = new Set([...authored.chunks.keys(), ...settled.chunks.keys()]);
  const empty = new Uint16Array(4096);
  // Each palette entry's block id as a number (air is 0), so cells compare as integers.
  const numbers = new Map();
  const idsOf = (palette) => Int32Array.from(palette.entries, (entry) => {
    if (!entry) return 0;
    if (!numbers.has(entry.id)) numbers.set(entry.id, numbers.size + 1);
    return numbers.get(entry.id);
  });
  const authoredIds = idsOf(authored.palette);
  const settledIds = idsOf(settled.palette);
  for (const key of keys) {
    const before = authored.chunks.get(key)?.view() || empty;
    const after = settled.chunks.get(key)?.view() || empty;
    const [cx, cy, cz] = World.coordsOf(key);
    for (let index = 0; index < 4096; index += 1) {
      const was = authoredIds[before[index]];
      const now = settledIds[after[index]];
      if (was === now) continue;
      const kind = !now ? 'removed' : !was ? 'added' : 'replaced';
      changes[kind] += 1;
      if (changes.examples[kind].length < CHANGE_EXAMPLES) {
        const at = [cx * 16 + (index & 15), cy * 16 + (index >> 8), cz * 16 + ((index >> 4) & 15)];
        const wasId = authored.palette.get(before[index])?.id;
        const nowId = settled.palette.get(after[index])?.id;
        changes.examples[kind].push(kind === 'removed' ? { at, was: wasId } : kind === 'added' ? { at, now: nowId } : { at, was: wasId, now: nowId });
      }
    }
  }
  return changes;
}

// The blocks of `world` inside `clip` ({min, max}), as a world of the same
// palette: chunks wholly inside are `world`'s own chunk objects.
function clipWorld(world, clip, previous) {
  const out = new World({ palette: world.palette });
  for (const [key, chunk] of world.chunks) {
    const origin = World.coordsOf(key).map((c) => c * 16);
    if (origin.some((o, i) => o + 15 < clip.min[i] || o > clip.max[i])) continue;
    if (origin.every((o, i) => o >= clip.min[i] && o + 15 <= clip.max[i])) {
      out.chunks.set(key, chunk);
      out.blockCount += chunk.count;
      continue;
    }
    chunk.forEach((index, value) => {
      const at = [origin[0] + (index & 15), origin[1] + (index >> 8), origin[2] + ((index >> 4) & 15)];
      if (at.every((v, i) => v >= clip.min[i] && v <= clip.max[i])) {
        const entry = world.palette.get(value);
        out.set(at[0], at[1], at[2], entry.id, entry.states);
      }
    });
  }
  adoptUnchanged(out, previous);
  return out;
}

// Settles `world` for each of `ticks` (sorted, no repeats, each checked) in
// one run of the engine, and answers { ticks: { world: the engine's World
// after that many ticks, changes: which block ids changed from `world`,
// timings (ms) } }. `chat` is what is kept of the conversation (`kept`).
function runSettle(chat, conversationId, world, ticks) {
  const pool = shotPool();
  const job = async (member) => {
    const timings = {};
    const time = async (name, operation) => {
      const started = process.hrtime.bigint();
      const result = await operation();
      timings[name] = (timings[name] || 0) + Number(process.hrtime.bigint() - started) / 1e6;
      return result;
    };
    const files = new Map();
    let scratch = null;
    try {
      // The engine refuses a placement of no blocks; an empty world settles to itself.
      if (world.size() > 0) {
        try {
          await time('warm', () => member.warming);
        } catch (error) {
          pool.drop(member);
          throw error;
        }
        const engine = member.engine;
        scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-shot-'));
        const file = (name) => path.join(scratch, name);
        await time('commands', async () => {
          const { commands } = importCommands(world, null, minecraftLive.yOffset);
          fs.writeFileSync(file('commands.txt'), commands.map((command) => `${command}\n`).join(''));
          fs.writeFileSync(file('none.txt'), '');
        });
        try {
          await time('place', () => engine.place(file('commands.txt'), file('placed.bin'), { fresh: true }));
          let at = 0;
          for (const tick of ticks) {
            if (tick > at) {
              // Nothing reads light after the last tick: the blocks are written next.
              await time('ticks', () => engine.tick(tick - at, file('patch.bin'), file('entities.json'), { light: false }));
              at = tick;
            }
            if (tick > 0) {
              // A placement of no commands writes the world as it now is.
              await time('write', () => engine.place(file('none.txt'), file(`settled-${tick}.bin`), { fresh: false }));
            }
            files.set(tick, tick === 0 ? file('placed.bin') : file(`settled-${tick}.bin`));
          }
        } catch (error) {
          // An engine that stopped (it ran out of memory, or was killed) is
          // replaced, not handed to the next settle.
          if (!engine.alive) pool.drop(member);
          throw error;
        }
      }
      const results = {};
      for (const tick of ticks) {
        const previous = chat.latest;
        const settled = await time('read', async () => {
          const out = files.has(tick)
            ? blueprintWorld(fs.readFileSync(files.get(tick)), minecraftLive.yOffset, previous?.world.palette)
            : new World({ palette: previous?.world.palette });
          adoptUnchanged(out, previous?.world);
          return out;
        });
        const changes = await time('compare', async () => idChanges(world, settled));
        const result = { world: settled, changes, timings, ticks: tick, clips: new Map(), previousClips: previous?.clips || new Map() };
        chat.latest = result;
        results[tick] = result;
      }
      return results;
    } finally {
      if (scratch) fs.rmSync(scratch, { recursive: true, force: true });
    }
  };
  return pool.run(conversationId, job, { member: world.size() > 0 });
}

// The ticks `world` was settled with by the build that made it (what
// see_build shows and mc.settled reads), or the default for a world nothing
// has settled. The world keeps them (World.settleTicks), saved with it.
function currentTicks(world) {
  return world.settleTicks ?? DEFAULT_SETTLE_TICKS;
}

// The settles of `world`, the conversation's current world, for each of
// `ticks`: those made before are kept (`cached`); the rest are made now in
// one run. `primary`, when given, becomes the world's settle ticks
// (currentTicks). Answers a Map from ticks to settle.
async function settle(conversationId, world, ticks, { primary } = {}) {
  const wanted = [...new Set(ticks)].sort((a, b) => a - b);
  for (const tick of wanted) checkTicks(tick, 'ticks');
  if (primary !== undefined) world.settleTicks = checkTicks(primary, 'ticks');
  const chat = kept.get(conversationId) || { worlds: [], latest: null };
  const entry = entryOf(conversationId, world) || { world, revision: world.revision, pending: new Map(), done: new Map() };
  const missing = wanted.filter((tick) => !entry.pending.has(tick) && !entry.done.has(tick));
  if (missing.length) {
    const run = runSettle(chat, conversationId, world, missing);
    for (const tick of missing) {
      const pending = run.then((results) => results[tick]);
      entry.pending.set(tick, pending);
      pending.then((result) => {
        entry.pending.delete(tick);
        entry.done.set(tick, result);
      }, () => {
        entry.pending.delete(tick);
        // A world that could not be settled is not kept.
        if (!entry.pending.size && !entry.done.size) chat.worlds = chat.worlds.filter((other) => other !== entry);
      });
    }
  }
  chat.worlds = [entry, ...chat.worlds.filter((other) => other !== entry)].slice(0, KEPT_WORLDS);
  kept.delete(conversationId);
  kept.set(conversationId, chat);
  for (const oldest of [...kept.keys()].slice(0, Math.max(0, kept.size - KEPT_CONVERSATIONS))) kept.delete(oldest);
  const results = await Promise.all(wanted.map((tick) => entry.done.get(tick) ?? entry.pending.get(tick)));
  return new Map(wanted.map((tick, i) => [tick, { ...results[i], cached: !missing.includes(tick) }]));
}

// Starts the first engines, so the first settle does not wait for them.
async function warmShotEngines() {
  const warmed = shotPool();
  const member = await warmed.acquire();
  warmed.release(member);
  await member.warming;
}

async function stopShotEngines() {
  const running = pool;
  pool = null;
  await running?.stop();
}

// The engine world of `conversation`'s current world after `tick` ticks
// (its settle ticks when omitted), or the part of it inside `clip`
// ({from, to}); null when that was not settled.
function shotWorld(conversation, clip, tick) {
  const corner = (value) => Array.isArray(value) && value.length === 3 && value.every((v) => Number.isInteger(v) && Math.abs(v) <= MAX_COORD);
  if (clip != null && !(corner(clip.from) && corner(clip.to))) {
    throw Object.assign(new Error(`A clip needs from and to corners of three integers within +/-${MAX_COORD}`), { statusCode: 400 });
  }
  const result = entryOf(conversation.id, conversation.world)?.done.get(tick ?? currentTicks(conversation.world));
  if (!result) return null;
  if (!clip) return result.world;
  const box = { min: clip.from.map((v, i) => Math.min(v, clip.to[i])), max: clip.from.map((v, i) => Math.max(v, clip.to[i])) };
  const key = `${box.min.join(',')}:${box.max.join(',')}`;
  if (!result.clips.has(key)) {
    result.clips.set(key, clipWorld(result.world, box, result.previousClips.get(key)));
    // A settle keeps the clips shot most recently, not every box ever asked.
    if (result.clips.size > KEPT_CLIPS) result.clips.delete(result.clips.keys().next().value);
  }
  return result.clips.get(key);
}

module.exports = { shotOptions, settle, currentTicks, checkTicks, shotWorld, warmShotEngines, stopShotEngines, DEFAULT_SETTLE_TICKS, _internal: { idChanges } };
