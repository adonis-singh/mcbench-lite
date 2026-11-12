'use strict';

// How a blueprint becomes the vanilla commands that place it: which chunks
// differ from what Minecraft already has, and each chunk's blocks as exact
// cuboids. This decides what an import produces (the page's engine places the
// commands, see live-views.js).

const { CHUNK_SIZE, World } = require('./world');
const { Y_OFFSET } = require('./minecraft-dimension');

const FRAGILE_BLOCK = /(?:^|:)(?:.*(?:torch|lantern|ladder|vine|button|lever|sign|banner|rail|carpet|flower|sapling|bush|roots|sprouts|fungus|candle|door|bed|sand|gravel|concrete_powder|anvil|water|lava|kelp|seagrass|lily_pad|sugar_cane|cactus|bamboo|scaffolding|tripwire|pressure_plate|coral|snow)$)/;

function blockSpec(entry) {
  const states = Object.entries(entry.states || {})
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, value]) => `${key}=${value}`);
  return `${entry.id}${states.length ? `[${states.join(',')}]` : ''}`;
}

function cubeCommand(spec, from, to) {
  if (from.every((value, index) => value === to[index])) {
    return `setblock ${from.join(' ')} ${spec}`;
  }
  return `fill ${from.join(' ')} ${to.join(' ')} ${spec}`;
}

// Greedy exact cuboids inside one 16³ chunk. A solid chunk becomes one fill;
// detailed surfaces remain setblocks. This keeps normal LLM edits fast without
// installing/reloading a datapack or restarting the Minecraft server.
function compileChunk(world, key, yOffset = Y_OFFSET, previousWorld = null) {
  const chunk = world.chunks.get(key);
  const previous = previousWorld?.chunks.get(key);
  const current = chunk ? chunk.view() : new Uint16Array(CHUNK_SIZE ** 3);
  const before = previous?.view();
  const blocks = new Int32Array(current.length).fill(-1);
  const specs = new Map([[0, 'minecraft:air']]);
  const oldSpecs = new Map([[0, 'minecraft:air']]);
  const specAt = (palette, index, cache) => {
    if (!cache.has(index)) cache.set(index, blockSpec(palette.get(index)));
    return cache.get(index);
  };
  for (let i = 0; i < current.length; i += 1) {
    const old = before ? before[i] : 0;
    const next = current[i];
    if ((world.palette === previousWorld?.palette || !previous) && old === next) continue;
    if (previous && specAt(world.palette, next, specs) === specAt(previousWorld.palette, old, oldSpecs)) continue;
    blocks[i] = next;
  }
  const indexOf = (x, y, z) => (y * CHUNK_SIZE + z) * CHUNK_SIZE + x;
  const visited = new Uint8Array(blocks.length);
  const [cx, cy, cz] = World.coordsOf(key);
  const ox = cx * CHUNK_SIZE; const oy = cy * CHUNK_SIZE; const oz = cz * CHUNK_SIZE;
  const removed = [];
  const solid = [];
  const fragile = [];
  for (let y = 0; y < CHUNK_SIZE; y += 1) {
    for (let z = 0; z < CHUNK_SIZE; z += 1) {
      for (let x = 0; x < CHUNK_SIZE; x += 1) {
        const at = indexOf(x, y, z);
        const paletteIndex = blocks[at];
        if (paletteIndex === -1 || visited[at]) continue;

        let x2 = x;
        while (x2 + 1 < CHUNK_SIZE) {
          const next = indexOf(x2 + 1, y, z);
          if (visited[next] || blocks[next] !== paletteIndex) break;
          x2 += 1;
        }
        let z2 = z;
        zLoop: while (z2 + 1 < CHUNK_SIZE) {
          for (let xx = x; xx <= x2; xx += 1) {
            const next = indexOf(xx, y, z2 + 1);
            if (visited[next] || blocks[next] !== paletteIndex) break zLoop;
          }
          z2 += 1;
        }
        let y2 = y;
        yLoop: while (y2 + 1 < CHUNK_SIZE) {
          for (let zz = z; zz <= z2; zz += 1) {
            for (let xx = x; xx <= x2; xx += 1) {
              const next = indexOf(xx, y2 + 1, zz);
              if (visited[next] || blocks[next] !== paletteIndex) break yLoop;
            }
          }
          y2 += 1;
        }
        for (let yy = y; yy <= y2; yy += 1) {
          for (let zz = z; zz <= z2; zz += 1) {
            for (let xx = x; xx <= x2; xx += 1) visited[indexOf(xx, yy, zz)] = 1;
          }
        }

        const spec = specAt(world.palette, paletteIndex, specs);
        const command = cubeCommand(
          spec,
          [ox + x, oy + y + yOffset, oz + z],
          [ox + x2, oy + y2 + yOffset, oz + z2],
        );
        (paletteIndex === 0 ? removed : FRAGILE_BLOCK.test(spec.split('[')[0]) ? fragile : solid).push(command);
      }
    }
  }
  return { key, removed, solid, fragile };
}

function chunksEqual(leftWorld, rightWorld, key) {
  const left = leftWorld?.chunks.get(key);
  const right = rightWorld?.chunks.get(key);
  if (left === right) return true;
  if (!left || !right || left.count !== right.count) return false;
  if (left.persistedHash && left.persistedHash === right.persistedHash
    && leftWorld.palette === rightWorld.palette) return true;

  // Persisted chunk hashes cover numeric palette indexes, not their block
  // meanings. Compare actual blocks so independently loaded sibling branches
  // can skip equal chunks even when their palettes diverged after the fork.
  const leftSpecs = new Map();
  const rightSpecs = new Map();
  const leftView = left.view();
  const rightView = right.view();
  for (let index = 0; index < leftView.length; index += 1) {
    const paletteIndex = leftView[index];
    const rightPaletteIndex = rightView[index];
    if (paletteIndex === 0 && rightPaletteIndex === 0) continue;
    if (paletteIndex === 0 || rightPaletteIndex === 0) return false;
    if (!leftSpecs.has(paletteIndex)) {
      const entry = leftWorld.palette.get(paletteIndex);
      if (!entry) return false;
      leftSpecs.set(paletteIndex, blockSpec(entry));
    }
    if (!rightSpecs.has(rightPaletteIndex)) {
      const entry = rightWorld.palette.get(rightPaletteIndex);
      if (!entry) return false;
      rightSpecs.set(rightPaletteIndex, blockSpec(entry));
    }
    if (leftSpecs.get(paletteIndex) !== rightSpecs.get(rightPaletteIndex)) return false;
  }
  return true;
}

function changedKeys(appliedWorld, world) {
  const previous = appliedWorld?.chunks || new Map();
  const keys = new Set([...previous.keys(), ...world.chunks.keys()]);
  return [...keys].filter((key) => !chunksEqual(appliedWorld, world, key));
}

function groupPlansByColumn(plans) {
  const columns = new Map();
  for (const plan of plans) {
    const [cx, cy, cz] = World.coordsOf(plan.key);
    const columnKey = `${cx},${cz}`;
    const column = columns.get(columnKey) || { cx, cz, plans: [] };
    column.plans.push({ ...plan, cy });
    columns.set(columnKey, column);
  }
  for (const column of columns.values()) column.plans.sort((left, right) => left.cy - right.cy);
  return [...columns.values()];
}

// Every command that takes `appliedWorld` to `world`, in the order the live
// import runs them: removals, then solid blocks, then blocks that need
// support, each pass column by column from the bottom up.
function importCommands(world, appliedWorld, yOffset) {
  const keys = changedKeys(appliedWorld, world);
  const columns = groupPlansByColumn(keys.map((key) => compileChunk(world, key, yOffset, appliedWorld)));
  const commands = [];
  for (const kind of ['removed', 'solid', 'fragile']) {
    for (const column of columns) for (const plan of column.plans) commands.push(...plan[kind]);
  }
  return { keys, commands };
}

module.exports = { blockSpec, changedKeys, compileChunk, groupPlansByColumn, importCommands };
