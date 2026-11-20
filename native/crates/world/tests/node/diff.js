'use strict';

// The Node side of mcbench-world's differential harness (tests/differential.rs).
// Reads every stored world of a data folder clone through today's server
// modules and writes what each module makes of it, for the Rust side to
// make again and compare:
//
//   node diff.js <checkout with node_modules> <data folder clone> <blocks.txt> <out folder>
//
// The data folder is opened read-only (its database with better-sqlite3).
// Nothing here writes outside <out folder>. Hashes are sha256, hex.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const [root, data, blocksTxt, out] = process.argv.slice(2);
if (!out) throw new Error('usage: node diff.js <checkout> <data folder> <blocks.txt> <out folder>');
// Modules that open the data folder open the clone.
process.env.MCBENCH_DATA_DIR = data;
const server = (name) => require(path.join(root, 'server', name));

// The engine's table from the given file, not the checkout's core/minecraft.
const factsModule = server('minecraft-facts');
const facts = new factsModule.MinecraftFacts(fs.readFileSync(blocksTxt, 'utf8'));
factsModule.minecraftFacts = () => facts;

const Database = require(path.join(root, 'node_modules', 'better-sqlite3'));
const { World } = server('world');
const { worldBlueprint, blueprintWorld } = server('blueprint');
const { importCommands } = server('live-placement');
const { encodeWorld, decodeWorld, encodeChanges, applyChanges } = server('build-transfer');
const { LiveViews } = server('live-views');
const { BenchComposer } = server('bench-world');
const { describeSelection, readSelection } = server('selection');
const { BlockStates } = server('block-states');
const { readBlockStates } = server('region');
const { zipNames, readZipEntries } = server('zip-entry');
const { writeDimensionPack } = server('minecraft-dimension');

const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
fs.mkdirSync(out, { recursive: true });
const write = (name, value) => fs.writeFileSync(path.join(out, name), typeof value === 'string' || Buffer.isBuffer(value) ? value : JSON.stringify(value));
const timings = {};
const timed = (name, fn) => {
  const started = process.hrtime.bigint();
  const value = fn();
  (timings[name] ||= []).push(Number(process.hrtime.bigint() - started) / 1e6);
  return value;
};

// A chunk writer that keeps the files' names and contents' hashes, in the
// order written (store.js chunkWriter, without the disk).
function recorder() {
  const files = [];
  return { files, write: (pairs) => { const payload = JSON.stringify(pairs); const hash = sha(payload).slice(0, 24); files.push(`${hash} ${sha(payload)}`); return hash; } };
}
const manifestOf = (world) => { const r = recorder(); const text = JSON.stringify(world.serializeManifest(r.write)); return { manifest: sha(text), files: sha(r.files.join('\n')), written: r.files.length }; };
const chunkReader = (owner) => (hash) => JSON.parse(fs.readFileSync(path.join(data, 'worlds', owner, 'chunks', `${hash}.json`), 'utf8'));
const load = (owner, text) => World.fromManifest(JSON.parse(text), chunkReader(owner), new Map());
const attempt = (fn) => { try { return { value: fn() }; } catch (error) { return { error: String(error.message) }; } };

const db = new Database(path.join(data, 'mcbench.sqlite3'), { readonly: true });
const rows = [
  ...db.prepare("SELECT id AS owner, 'conversation' AS kind, world_manifest_json AS m, NULL AS message FROM conversations").all(),
  ...db.prepare("SELECT conversation_id AS owner, 'message world' AS kind, world_manifest_json AS m, id AS message FROM messages WHERE world_manifest_json IS NOT NULL ORDER BY rowid").all(),
  ...db.prepare("SELECT conversation_id AS owner, 'checkpoint' AS kind, checkpoint_manifest_json AS m, id AS message FROM messages WHERE checkpoint_manifest_json IS NOT NULL ORDER BY rowid").all(),
];

// One entry per distinct (chat, manifest): the files under worlds/<chat>.
const worlds = [];
const byKey = new Map();
const sequences = new Map(); // owner -> world numbers of its message worlds, consecutive repeats dropped
for (const row of rows) {
  const key = `${row.owner}\n${row.m}`;
  let entry = byKey.get(key);
  if (!entry) {
    entry = { n: worlds.length, owner: row.owner, kinds: {}, file: `manifest-${worlds.length}.json` };
    worlds.push(entry);
    byKey.set(key, entry);
    write(entry.file, row.m);
  }
  entry.kinds[row.kind] = (entry.kinds[row.kind] || 0) + 1;
  if (row.kind === 'message world') {
    const list = sequences.get(row.owner) || [];
    if (list.at(-1) !== entry.n) list.push(entry.n);
    sequences.set(row.owner, list);
  }
}

// The block table and render layers of the data folder's Minecraft, where
// today's server finds them (minecraft-live.js blockTableFile, bakedFile).
const { minecraftLive } = server('minecraft-live');
const tableFile = minecraftLive.blockTableFile();
const layersFile = minecraftLive.bakedFile('render-layers.tsv');
const states = new BlockStates(fs.readFileSync(tableFile, 'utf8'), fs.readFileSync(layersFile, 'utf8'));

// The edits a build program makes on the child's world (the same on the Rust side).
function edit(world) {
  const b = world.bounds() || { min: [0, 0, 0], max: [0, 0, 0] };
  const log = [];
  log.push(attempt(() => world.set(b.max[0] + 1, b.min[1], b.min[2], 'minecraft:gold_block', {})));
  log.push(attempt(() => world.set(b.min[0], b.min[1], b.min[2], 'minecraft:air', {})));
  log.push(attempt(() => world.fill([b.min[0], b.max[1] + 1, b.min[2]], [b.min[0] + 20, b.max[1] + 3, b.min[2] + 20], 'minecraft:oak_stairs', { half: 'top', facing: 'east' })));
  log.push(attempt(() => world.fill([b.min[0], b.min[1], b.min[2]], [b.min[0] + 2, b.min[1] + 2, b.min[2] + 2], 'minecraft:air', {})));
  return log;
}
const normalize = ({ id, states: blockStates }) => ({ id, states: blockStates });

for (const entry of worlds) {
  const text = fs.readFileSync(path.join(out, entry.file), 'utf8');
  const world = timed('load', () => load(entry.owner, text));
  entry.size = world.size();
  entry.chunkCount = world.chunks.size;
  entry.paletteLength = world.palette.entries.length;
  entry.palette = sha(JSON.stringify(world.palette.entries.slice(1)));
  const rewritten = attempt(() => JSON.stringify(world.serializeManifest(() => { throw new Error('a loaded chunk was written'); })));
  entry.restored = rewritten.value === text;
  entry.rewritten = rewritten.value ? { value: sha(rewritten.value) } : rewritten;
  entry.encodeCold = sha(timed('encodeWorld', () => encodeWorld(world)));
  entry.bounds = world.bounds();
  entry.encodeWarm = sha(encodeWorld(world));
  entry.lod = world.renderLod();
  const blueprint = timed('blueprint', () => worldBlueprint(world, 64));
  entry.blueprint = sha(blueprint);
  entry.blueprintWorld = attempt(() => manifestOf(timed('blueprintWorld', () => blueprintWorld(blueprint, 64))));
  entry.fresh = manifestOf(decodeWorld(encodeWorld(world)));
  entry.commands = attempt(() => sha(timed('importCommands', () => importCommands(world, null, 64)).commands.map((c) => `${c}\n`).join('')));
  entry.commands0 = attempt(() => sha(importCommands(world, null, 0).commands.map((c) => `${c}\n`).join('')));
  entry.paletteStates = attempt(() => sha(JSON.stringify([...states.ofPalette(world.palette).ids])));
  // The build transfer: the child's world, its edits, its change set, the parent's clone.
  const sent = encodeWorld(world);
  const child = timed('decodeWorld', () => decodeWorld(sent));
  const paletteStart = child.palette.entries.length;
  entry.edits = edit(child);
  const changes = timed('encodeChanges', () => encodeChanges(child, paletteStart));
  entry.changes = { file: `changes-${entry.n}.bin`, sha: sha(changes), paletteStart: world.palette.entries.length };
  write(entry.changes.file, changes);
  world.finalize();
  const clone = world.clone();
  const applied = attempt(() => timed('applyChanges', () => applyChanges(clone, changes, world.palette.entries.length, normalize)));
  entry.applied = applied.error ? { error: applied.error } : { ...manifestOf(clone), diff: clone.diff(world), size: clone.size(), bounds: clone.bounds() };
  process.stdout.write(`world ${entry.n} ${entry.owner} ${entry.size}\n`);
}

// Incremental commands and live views along each chat's message worlds.
const views = [];
for (const [owner, list] of sequences) {
  let current = null;
  const live = new LiveViews({ resolve: () => ({ world: current, layout: null }), yOffset: 64, now: (() => { let t = 0; return () => (t += 1000); })() });
  const steps = [];
  let since = null;
  let previous = null;
  for (const n of list) {
    current = load(owner, fs.readFileSync(path.join(out, worlds[n].file), 'utf8')).finalize();
    const step = { n };
    step.describe = attempt(() => live.describe(`chat-${owner}`));
    const imports = attempt(() => live.imports(`chat-${owner}`, since));
    if (imports.value) {
      since = imports.value.version;
      imports.value.commands = sha(imports.value.commands);
    }
    step.imports = imports;
    if (previous) step.incremental = attempt(() => sha(importCommands(current, previous, 64).commands.map((c) => `${c}\n`).join('')));
    steps.push(step);
    previous = current;
  }
  views.push({ owner, steps });
}

// Bench compositions: every bench's members in slot order, their current
// worlds, then the first member at its first message world, then current again.
const benches = [];
for (const bench of db.prepare('SELECT id FROM benches ORDER BY rowid').all()) {
  const members = db.prepare('SELECT id, world_manifest_json AS m FROM conversations WHERE bench_id = ? ORDER BY bench_slot').all(bench.id);
  const composer = new BenchComposer();
  const current = members.map((member) => ({ id: member.id, world: load(member.id, member.m).finalize() }));
  const first = members.length && sequences.get(members[0].id)?.[0] !== undefined ? worlds[sequences.get(members[0].id)[0]] : null;
  const variants = [current, first ? [{ id: members[0].id, world: load(first.owner, fs.readFileSync(path.join(out, first.file), 'utf8')).finalize() }, ...current.slice(1)] : current, current];
  const results = variants.map((entries) => attempt(() => {
    const { world, layout } = composer.compose(entries);
    return { layout, size: world.size(), revision: world.revision, encode: sha(encodeWorld(world)), ...manifestOf(world) };
  }));
  benches.push({ id: bench.id, members: members.map((m) => m.id), first: first ? first.n : null, results });
}

// Selections: every selection part of every message, read, and described on its chat's world.
const selections = [];
for (const row of db.prepare("SELECT conversation_id AS owner, payload_json AS p FROM messages WHERE payload_json LIKE '%\"type\":\"selection\"%' ORDER BY rowid").all()) {
  const payload = JSON.parse(row.p);
  for (const part of payload.content || []) {
    if (part.type !== 'selection') continue;
    const conversation = db.prepare('SELECT world_manifest_json AS m FROM conversations WHERE id = ?').get(row.owner);
    const world = load(row.owner, conversation.m);
    selections.push({ owner: row.owner, part: part.selection, read: attempt(() => readSelection(part.selection)), describe: attempt(() => describeSelection(world, part.selection.min, part.selection.max, states.names)) });
  }
}
// And boxes around each chat's build, its corners and past the limits.
for (const entry of worlds.filter((w) => w.kinds.conversation)) {
  const world = load(entry.owner, fs.readFileSync(path.join(out, entry.file), 'utf8'));
  const b = world.bounds() || { min: [0, 0, 0], max: [0, 0, 0] };
  const mid = b.min.map((v, a) => Math.floor((v + b.max[a]) / 2));
  for (const [min, max] of [[b.min, b.max], [b.min, mid], [mid, b.max], [[b.min[0], b.max[1], b.min[2]], b.max], [[-4096, -4096, -4096], [4096, 4096, 4096]], [[0, 0], [1, 1, 1]], [[0, 0, 0], [4097, 0, 0]], [[0.5, 0, 0], [1, 1, 1]], [[3, 0, 0], [1, 1, 1]]]) {
    selections.push({ owner: entry.owner, min, max, describe: attempt(() => describeSelection(world, min, max, states.names)) });
  }
}

// Selection parts as the composer sends them (the data folder holds none):
// one per chat's build as describeSelection names it, and malformed ones.
for (const entry of worlds.filter((w) => w.kinds.conversation)) {
  const world = load(entry.owner, fs.readFileSync(path.join(out, entry.file), 'utf8'));
  const b = world.bounds() || { min: [0, 0, 0], max: [0, 0, 0] };
  const named = attempt(() => describeSelection(world, b.min, b.max, states.names)).value || { blocks: 0, block: null, label: 'Empty space' };
  const good = { letter: 'A', label: named.label, min: b.min, max: b.max, blocks: named.blocks, block: named.block };
  const parts = [good, { ...good, letter: 'AA' }, { ...good, letter: 'a' }, { ...good, label: '' }, { ...good, label: `${'x'.repeat(130)}é` }, { ...good, label: `${'é'.repeat(118)}\u{1F600}` },
    { ...good, block: '../x' }, { ...good, block: undefined }, { ...good, blocks: 1.5 }, { ...good, min: [0, 0] }, { ...good, min: [0.5, 0, 0] }, { ...good, max: [5000, 0, 0] }, null];
  for (const part of parts) selections.push({ owner: entry.owner, part, read: attempt(() => readSelection(part)), describe: attempt(() => describeSelection(world, part?.min, part?.max, states.names)) });
}

// Region folders and archives of the data folder.
const regions = [];
const archives = [];
(function walk(folder) {
  for (const name of fs.readdirSync(folder).sort()) {
    const file = path.join(folder, name);
    const stat = fs.lstatSync(file);
    if (stat.isDirectory()) {
      if (name === 'region') {
        const read = attempt(() => [...readBlockStates(file)]);
        regions.push({ folder: path.relative(data, file), count: read.value?.length, states: read.value ? sha(read.value.map(([k, v]) => `${k} ${v}`).join('\n')) : null, error: read.error });
      } else if (name !== 'worlds') walk(file);
    } else if (stat.isFile() && /\.(jar|zip)$/.test(name)) {
      const names = attempt(() => zipNames(file));
      const small = (entryName) => /\.(json|txt|mcmeta|properties|MF|yml)$/.test(entryName);
      const read = attempt(() => [...readZipEntries(file, small)].map(([entryName, bytes]) => `${entryName} ${sha(bytes)}`));
      archives.push({ file: path.relative(data, file), names: names.value ? sha(names.value.join('\n')) : null, count: names.value?.length, entries: read.value ? sha(read.value.join('\n')) : null, read: read.value?.length, error: names.error || read.error });
    }
  }
})(data);

// The data pack, the block states' bytes and names, and localeCompare.
const packFolder = path.join(out, 'pack');
writeDimensionPack(packFolder);
const pack = ['world/datapacks/mcbench/pack.mcmeta', 'world/datapacks/mcbench/data/minecraft/dimension_type/overworld.json'].map((file) => [file, fs.readFileSync(path.join(packFolder, file), 'utf8')]);
let seed = 7;
const random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 2 ** 32; };
const printable = Array.from({ length: 95 }, (_, i) => String.fromCharCode(32 + i));
const words = Array.from({ length: 3000 }, () => Array.from({ length: 1 + Math.floor(random() * 6) }, () => printable[Math.floor(random() * (random() < 0.5 ? 95 : 10)) + (random() < 0.5 ? 0 : 0)]).join(''));
const sorted = [...words].sort((a, b) => a.localeCompare(b));

write('node.json', {
  tables: { table: path.relative(data, tableFile), layers: path.relative(data, layersFile), bytes: sha(states.bytes()), names: sha(JSON.stringify([...states.names])) },
  worlds, views, benches, selections, regions, archives, pack, collation: { words, sorted }, timings,
});
process.stdout.write(`worlds ${worlds.length}, chats with message worlds ${views.length}, benches ${benches.length}, selections ${selections.length}, regions ${regions.length}, archives ${archives.length}\n`);
