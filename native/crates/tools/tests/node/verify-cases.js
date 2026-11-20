'use strict';

// The inputs of mcbench-tools' verifier differential (tests/differential.rs):
// test_build calls and grader experiments, each with the world it runs on,
// written for both sides to run. Nothing here runs an experiment.
//
//   MCBENCH_CORPUS=<corpus> node verify-cases.js <checkout> <data folder clone> <out folder>
//
// Writes <out>/cases.jsonl (one case a line: {name, kind, world, placementMode,
// args | test, reportEncoding?, budgets?}) and the worlds it made under
// <out>/worlds/<n>/ (manifest.json and the chunk files, as the store writes
// them). The cases:
//
//   stored/      every stored test_build call of the data folder, on the world
//                of its assistant message (the world the call ran on)
//   corpus-short/ scripts/benchmark-verification.js's 20-tick experiment of each
//                community build, as a call and as a grader's experiment
//   ticker/      core-verify-parity's experiment of each build (60 ticks), both
//                placement modes
//   fixtures/    scripts/verifier-fixtures.js (core-parity --verify --fixtures)
//   errors/      every validation error today's tests name
//   budget/      each budget: operations, simulationTicks, columns, tests,
//                requestBytes as calls; wallMs and reportBytes as a grader's
//   generated/   seeded mutations of the fixtures: samples, settling, inputs,
//                censuses, probes; invalid variants; calls of 2 to 8 tests
//
// The data folder's database is opened read-only.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const [root, data, out] = process.argv.slice(2);
if (!out) throw new Error('usage: node verify-cases.js <checkout> <data folder> <out folder>');
process.env.MCBENCH_DATA_DIR = path.join(out, 'unused-data');
const server = (name) => require(path.join(root, 'server', name));
const { World } = server('world');
const facts = server('minecraft-facts').minecraftFacts();
const { corpusArtifacts } = require(path.join(root, 'scripts', 'corpus'));
const Database = require(path.join(root, 'node_modules', 'better-sqlite3'));

fs.mkdirSync(path.join(out, 'worlds'), { recursive: true });
const cases = [];
let worldCount = 0;
const sha = (text) => crypto.createHash('sha256').update(text).digest('hex');

// A world written as the store writes it, read back by both sides.
function saveWorld(world) {
  const dir = path.join(out, 'worlds', String(worldCount++));
  fs.mkdirSync(dir, { recursive: true });
  const manifest = world.serializeManifest((pairs) => {
    const text = JSON.stringify(pairs);
    const hash = sha(text).slice(0, 24);
    fs.writeFileSync(path.join(dir, `${hash}.json`), text);
    return hash;
  });
  fs.writeFileSync(path.join(dir, 'manifest.json'), JSON.stringify(manifest));
  return { manifest: path.join(dir, 'manifest.json'), chunks: dir };
}

const call = (name, world, tests, placementMode = 'updates') => cases.push({ name, kind: 'call', world, placementMode, args: { tests } });
const grade = (name, world, test, extra = {}) => cases.push({ name, kind: 'grade', world, placementMode: 'updates', reportEncoding: 'full', ...extra, test });

// --- stored calls
{
  const db = new Database(path.join(data, 'mcbench.sqlite3'), { readonly: true });
  const rows = db.prepare(`SELECT m.conversation_id AS chat, m.position, m.world_manifest_json AS world, json_extract(t.value, '$.function.arguments') AS args
    FROM messages m, json_each(json_extract(m.payload_json, '$.tool_calls')) t
    WHERE json_extract(t.value, '$.function.name') = 'test_build' ORDER BY m.conversation_id, m.position`).all();
  for (const [i, row] of rows.entries()) {
    const file = path.join(out, 'worlds', `stored-${i}.json`);
    fs.writeFileSync(file, row.world);
    const args = JSON.parse(row.args);
    cases.push({ name: `stored/${i}-${row.chat.slice(0, 8)}-${row.position}`, kind: 'call', world: { manifest: file, chunks: path.join(data, 'worlds', row.chat, 'chunks') }, placementMode: 'updates', args });
  }
  db.close();
}

// --- the community builds
const builds = [...new Map(corpusArtifacts())].sort(([a], [b]) => a.localeCompare(b));
const COMPONENTS = /redstone|repeater|comparator|lever|button|piston|observer|lamp|note_block|dropper|dispenser|hopper|door|trapdoor|rail|target|bulb|crafter|tnt|bell|lectern/;
const CONTAINERS = /chest$|barrel|hopper$|dropper|dispenser|furnace$|smoker|shulker_box/;
const UNLIMITED = { operations: 2_000_000_000, simulationTicks: 2_000_000_000, wallMs: 3_600_000, columns: 100_000, requestBytes: 2_000_000_000, reportBytes: Number.MAX_SAFE_INTEGER };

// scripts/benchmark-verification.js community(file, ticks), one copy.
function community(artifact, ticks) {
  const world = new World();
  const probes = [];
  const actions = [];
  const axis = (a) => artifact.blocks.reduce((range, block) => [Math.min(range[0], block.pos[a]), Math.max(range[1], block.pos[a])], [Infinity, -Infinity]);
  const [x0] = axis(0), [z0] = axis(2);
  for (const block of artifact.blocks) {
    const pos = [block.pos[0] - x0, block.pos[1], block.pos[2] - z0];
    world.set(pos[0], pos[1], pos[2], block.id, block.states || {});
    if (COMPONENTS.test(block.id) && probes.length < 8) probes.push(pos);
    if (facts.is(block.id, 'LeverBlock')) actions.push({ tick: 2, type: 'toggle', at: pos });
    else if (facts.is(block.id, 'ButtonBlock')) actions.push({ tick: 4, type: 'press', at: pos });
  }
  if (!probes.length) probes.push([artifact.blocks[0].pos[0] - x0, artifact.blocks[0].pos[1], artifact.blocks[0].pos[2] - z0]);
  world.finalize();
  return { world, args: { probes, actions, samples: [0, ticks] } };
}

// scripts/core-verify-parity.js experiment(artifact, ticks, every), one copy.
function ticker(artifact, ticks, every) {
  const probes = [];
  const actions = [];
  const containers = artifact.blocks.filter((block) => CONTAINERS.test(block.id)).slice(0, 16).map((block) => block.pos);
  probes.push(...containers);
  for (const block of artifact.blocks) {
    if (!COMPONENTS.test(block.id)) continue;
    if (probes.length < 256 && !containers.includes(block.pos)) probes.push(block.pos);
    if (block.id === 'minecraft:lever') actions.push({ tick: 2, type: 'toggle', at: block.pos }, { tick: Math.floor(ticks / 2), type: 'toggle', at: block.pos });
    else if (block.id.endsWith('_button')) actions.push({ tick: 4, type: 'press', at: block.pos });
  }
  containers.slice(0, 8).forEach((at, index) => actions.push({ tick: 3 + index, type: 'insert', at, slot: 0, count: 16, item: 'minecraft:cobblestone' }));
  const hopper = artifact.blocks.find((block) => block.id === 'minecraft:hopper');
  if (hopper) actions.push({ tick: 6, type: 'drop', at: [hopper.pos[0] + 0.5, hopper.pos[1] + 1.5, hopper.pos[2] + 0.5], item: 'minecraft:redstone', count: 5, velocity: [0, 0, 0] });
  if (containers.length) actions.push({ tick: ticks - 10, type: 'drain', at: containers[0] });
  if (!probes.length) probes.push(artifact.blocks[0].pos);
  return {
    probes, actions, samples: Array.from({ length: Math.floor(ticks / every) + 1 }, (_, index) => index * every),
    ...(containers.length ? { inventory_probes: containers, inventory_missing: 'report', world_inventories: true } : {}),
    item_entities: true, living_entities: true,
  };
}

for (const [name, file] of builds) {
  const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
  const short = community(artifact, 20);
  const saved = saveWorld(short.world);
  call(`corpus-short/${name}`, saved, [short.args]);
  grade(`corpus-short-grade/${name}`, saved, short.args, { budgets: UNLIMITED, reportEncoding: name.length % 2 ? 'rle-v1' : 'full' });
  const world = new World();
  for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
  world.finalize();
  const placed = saveWorld(world);
  for (const mode of ['updates', 'strict']) call(`ticker/${name}/${mode}`, placed, [ticker(artifact, 60, 1)], mode);
}

// --- the verifier's fixtures
const fixtures = require(path.join(root, 'scripts', 'verifier-fixtures'));
const fixtureWorlds = [];
for (const fixture of fixtures) {
  const world = fixture.world();
  if (!world.size()) continue;
  const saved = saveWorld(world);
  const mode = fixture.placementMode ?? 'updates';
  fixtureWorlds.push({ fixture, saved, mode, world });
  call(`fixtures/${fixture.name}`, saved, [fixture.args], mode);
}

// --- every validation error today's tests name (test/minecraft-verify.test.js
// and its neighbours), on their world: stone at -1, 0, 0.
const stone = (() => { const w = new World(); w.set(-1, 0, 0, 'minecraft:stone'); w.finalize(); return saveWorld(w); })();
const p = [[0, 0, 0]];
const errors = [
  { probes: p, command: 'setblock' },
  { probes: [[0, '0\nstop', 0]] },
  { probes: [[0, 896, 0]] },
  { probes: p, samples: [72000] },
  { probes: p, samples: [0], actions: [{ tick: 1, type: 'press', at: [0, 0, 0] }] },
  { probes: p, actions: [{ tick: 0, type: 'setblock', at: [0, 0, 0] }] },
  { probes: p, settle_ticks: 0 },
  { probes: p, actions: [{ tick: 0, type: 'use', at: [0, 0, 0], yaw: '90' }] },
  { probes: p, actions: [{ tick: 0, type: 'toggle', at: [0, 0, 0], yaw: 90 }] },
  { probes: p, actions: [{ tick: 0, type: 'use', at: [0, 0, 0], yaw: -44.5 }, { tick: 0, type: 'use', at: [0, 0, 0] }] },
  { probes: p, samples: [72000 - 3999], settle_ticks: 4000 },
  { probes: p, inventory_probes: p, samples: [0], inventory_missing: 'empty' },
  { probes: p, inventory_probes: p, samples: [0], item_entities: 'yes' },
  { probes: p, inventory_probes: [[1, 0, 0]] },
  { probes: p, inventory_probes: [[0, 0, 0], [0, 0, 0]] },
  ...[[NaN, 0, 0], [0, 1e309, 0], [0, 896, 0], [4096, 0, 0], ['1', 0, 0], [0, 0]].map((at) => ({ probes: p, entities: [{ at, data_version: 4189, snbt: '{}' }] })),
  { probes: p, entities: [{ at: [96.125, 2.5, -96.75], data_version: 4189, snbt: '{}', command: 'summon' }] },
  { probes: p, entities: 'x' },
  { probes: p, entities: [{ at: [0, 0, 0], data_version: -1, snbt: '{}' }] },
  ...[{ velocity: [0, 0, null] }, { velocity: [11, 0, 0] }, { count: 0 }, { slot: 0 }, { at: [1, 2] }, { item: 'stone' }].map((bad) => ({ probes: p, actions: [{ tick: 0, type: 'drop', at: [6.5, 2, 2.12], velocity: [-1, 0, 0], item: 'minecraft:iron_ingot', count: 1, ...bad }] })),
  { probes: p, actions: [{ tick: 0, type: 'toggle', at: [0.5, 0, 0] }] },
  ...[{ page: -1 }, { page: 1.5 }, { slot: 1 }, { page: 2147483647 }, { at: [0.5, 0, 0] }].map((bad) => ({ probes: p, actions: [{ tick: 0, type: 'page', at: [0, 0, 0], page: 14, ...bad }] })),
  ...[{ present: 0 }, { present: 'false' }, { present: null }, { page: 1 }, { at: [0.5, 0, 0] }].map((bad) => ({ probes: p, actions: [{ tick: 0, type: 'book', at: [0, 0, 0], present: false, ...bad }] })),
  { probes: p, actions: [{ tick: 0, type: 'book', at: [0, 0, 0], present: false, item: 'minecraft:written_book', count: 1, stack: { data_version: 4556, snbt: '{}' } }] },
  { probes: p, actions: [{ tick: 0, type: 'book', at: [0, 0, 0], present: true, item: 'minecraft:stone', count: 1, stack: { data_version: 4556, snbt: '{}' } }] },
  { probes: p, actions: [{ tick: 0, type: 'book', at: [0, 0, 0], present: true, item: 'minecraft:written_book', count: 2, stack: { data_version: 4556, snbt: '{}' } }] },
  ...[null, [], {}, { data_version: -1, snbt: '{}' }, { data_version: 4556, snbt: '' }, { data_version: 4556, snbt: '{}', ignore: true }].map((stack) => ({ probes: p, samples: [0, 1], actions: [{ tick: 0, type: 'insert', at: [0, 0, 0], slot: 0, item: 'minecraft:enchanted_book', count: 1, stack }] })),
  { probes: p, samples: [0, 1], actions: [{ tick: 0, type: 'toggle', at: [0, 0, 0], stack: { data_version: 3465, snbt: '{}' } }] },
  { probes: p, samples: [0, 1], actions: [{ tick: 0, type: 'extract', at: [0, 0, 0], slot: 0, count: 1, stack: { data_version: 3465, snbt: '{}' } }] },
  ...[-1, 1.5, '3218', null].map((v) => ({ probes: p, samples: [0], block_data_version: v })),
  ...[0, 1, 'true', null, [], {}].map((v) => ({ probes: p, samples: [0], living_entities: v })),
  { probes: p, world_inventories: 1 },
  { probes: p, block_entities: [{ at: [0, 0, 0], data_version: 4556, snbt: '{}' }, { at: [0, 0, 0], data_version: 4556, snbt: '{}' }] },
  { probes: p, block_entities: [{ at: [0, 0, 0], data_version: 4556, snbt: '  ' }] },
  { probes: [] },
  { probes: 'x' },
  { samples: [0] },
  { probes: p, samples: [] },
  { probes: p, samples: [-1] },
  { probes: p, samples: [2147483648] },
  { probes: p, actions: 'x' },
  { probes: p, actions: [null] },
  { probes: p, actions: [[]] },
  { probes: p, actions: [{ tick: 0, type: 'insert', at: [0, 0, 0], slot: 0, count: 65, item: 'minecraft:stone' }] },
  { probes: p, actions: [{ tick: 0, type: 'insert', at: [0, 0, 0], slot: -1, count: 1, item: 'minecraft:stone' }] },
  { probes: p, actions: [{ tick: 0, type: 'insert', at: [0, 0, 0], slot: 0, count: 1, item: 'stone' }] },
  { probes: p, scheduled_ticks: { bounds: [[0, 0, 0], [1, 1, 1]], data_version: 4189, entries: [] } },
  { 0: 1, probes: p, zz: 2 },
  [],
  'x',
  null,
];
errors.forEach((args, i) => call(`errors/${i}`, stone, [args]));
const saved = { bounds: [[0, 0, 0], [2, 2, 2]], data_version: 4189, entries: [{ kind: 'fluid', id: 'minecraft:water', at: [0, 0, 0], delay: 5, priority: 0, sub_tick: '9007199254740993' }] };
const e = saved.entries[0];
for (const [i, bad] of [{ priority: 4 }, { sub_tick: 1 }, { sub_tick: '9223372036854775808' }, { sub_tick: '-9223372036854775808' }, { sub_tick: '00000000000000000000001' }, { sub_tick: '1\n' }, { delay: 1.5 }, { at: [3, 0, 0] }, { kind: 'entity' }, { id: 'fake' }, { extra: true }].entries()) {
  call(`errors/scheduled-${i}`, stone, [{ probes: p, scheduled_ticks: { ...saved, entries: [{ ...e, ...bad }] } }], 'strict');
}
call('errors/scheduled-duplicate', stone, [{ probes: p, scheduled_ticks: { ...saved, entries: [e, e] } }], 'strict');
call('errors/scheduled-reversed', stone, [{ probes: p, scheduled_ticks: { ...saved, bounds: [[2, 2, 2], [0, 0, 0]] } }], 'strict');
call('errors/scheduled-null', stone, [{ probes: p, scheduled_ticks: null }], 'strict');
call('errors/scheduled-saved', stone, [{ probes: p, scheduled_ticks: saved }], 'strict');
// The call's own shape.
for (const [i, args] of [{}, { probes: p }, { tests: [] }, { tests: {} }, { tests: [{}], probes: p }, { tests: 'x' }].entries()) cases.push({ name: `errors/call-${i}`, kind: 'call', world: stone, placementMode: 'updates', args });
// The empty blueprint and the build height.
call('errors/empty', saveWorld(new World()), [{ probes: p }]);
{ const w = new World(); w.set(0, 900, 0, 'minecraft:stone'); w.finalize(); call('errors/height', saveWorld(w), [{ probes: p }]); }

// --- the budgets
const floor = (chunks, layers = 1) => { const w = new World(); w.fill([0, 0, 0], [chunks * 16 - 1, layers - 1, chunks * 16 - 1], 'minecraft:stone'); w.finalize(); return w; };
const small = saveWorld(floor(1));
call('budget/operations', small, [{ probes: Array.from({ length: 1000 }, (_, i) => [i % 16, 0, Math.floor(i / 16) % 16]), samples: Array.from({ length: 1000 }, (_, i) => i) }]);
call('budget/operations-inventory', small, [{ probes: p, inventory_probes: p, samples: Array.from({ length: 20000 }, (_, i) => i) }]);
call('budget/simulationTicks', small, [{ probes: p, samples: [71961] }]);
call('budget/simulationTicks-at', small, [{ probes: p, samples: [71960], settle_ticks: 40 }]);
call('budget/columns', small, [{ probes: Array.from({ length: 2000 }, (_, i) => [(i % 45) * 16 * 3 - 2048, 0, Math.floor(i / 45) * 16 * 3 - 2048]) }]);
{ const w = new World(); for (let x = -4096; x <= 4096; x += 48) for (let z = -4096; z <= 4096; z += 48) w.set(x, 0, z, 'minecraft:stone'); w.finalize(); call('budget/columns-world', saveWorld(w), [{ probes: p }]); }
call('budget/tests', small, Array.from({ length: 9 }, () => ({ probes: p })));
call('budget/requestBytes', small, [{ probes: p, block_entities: [{ at: [0, 0, 0], data_version: 4556, snbt: `{id:"minecraft:chest",CustomName:"${'x'.repeat(8 * 1024 * 1024)}"}` }] }]);
// A grader's lowered budgets: the report's bytes, and the wall clock on a
// busy build (observer rings that fire for good).
const busy = (() => {
  const world = new World();
  world.fill([0, 0, 0], [15, 0, 15], 'minecraft:stone');
  for (let y = 1; y <= 8; y++) for (let x = 0; x < 16; x += 2) for (let z = 0; z < 16; z += 2) {
    world.set(x, y, z, 'minecraft:observer', { facing: 'east', powered: 'false' });
    world.set(x + 1, y, z, 'minecraft:observer', { facing: 'south', powered: 'false' });
    world.set(x + 1, y, z + 1, 'minecraft:observer', { facing: 'west', powered: 'false' });
    world.set(x, y, z + 1, 'minecraft:observer', { facing: 'north', powered: 'false' });
  }
  world.finalize();
  return saveWorld(world);
})();
const BUDGETS = server('verification-contract').BUDGETS;
grade('budget/reportBytes', small, { probes: p, samples: [0, 20] }, { budgets: { ...BUDGETS, reportBytes: 600 } });
grade('budget/reportBytes-engine', small, { probes: Array.from({ length: 256 }, (_, i) => [i % 16, 0, Math.floor(i / 16)]), samples: Array.from({ length: 50 }, (_, i) => i) }, { budgets: { ...BUDGETS, reportBytes: 4000 } });
// 30,000 ticks: far more than either engine runs in 400 ms on any load, so
// both sides stop it (3,000 settling ticks finished in time once on a quiet
// machine and not on a loaded one).
grade('budget/wallMs', busy, { probes: [[1, 1, 0]], samples: [0, 30000] }, { budgets: { ...BUDGETS, wallMs: 400 } });
grade('budget/wallMs-settling', busy, { probes: [[1, 1, 0]], samples: [0], settle_ticks: 30000 }, { budgets: { ...BUDGETS, wallMs: 400 } });
grade('budget/operations-grade', small, { probes: p, samples: Array.from({ length: 11 }, (_, i) => i) }, { budgets: { ...BUDGETS, operations: 11 } });
grade('budget/no-tests-key', small, { probes: p }, { budgets: UNLIMITED, reportEncoding: 'rle-v1' });

// --- generated: seeded mutations of the fixtures
let seed = 20261117;
const random = () => { seed = (seed + 0x6D2B79F5) | 0; let t = Math.imul(seed ^ (seed >>> 15), 1 | seed); t ^= t + Math.imul(t ^ (t >>> 7), 61 | t); return ((t ^ (t >>> 14)) >>> 0) / 4294967296; };
const pick = (list) => list[Math.floor(random() * list.length)];
const int = (lo, hi) => lo + Math.floor(random() * (hi - lo + 1));
const usable = fixtureWorlds.filter(({ fixture }) => !fixture.args.scheduled_ticks || true);
const INVALID = [
  (a) => ({ ...a, unknown: 1 }),
  (a) => ({ ...a, probes: [[0, 'x', 0]] }),
  (a) => ({ ...a, settle_ticks: -5 }),
  (a) => ({ ...a, samples: [1.5] }),
  (a) => ({ ...a, actions: [...(a.actions || []), { tick: 0, type: 'jump', at: [0, 0, 0] }] }),
  (a) => ({ ...a, actions: [...(a.actions || []), { tick: 'now', type: 'toggle', at: [0, 0, 0] }] }),
  (a) => ({ ...a, inventory_missing: 'maybe' }),
  (a) => ({ ...a, item_entities: 1 }),
  (a) => ({ ...a, block_data_version: 'old' }),
  (a) => ({ ...a, entities: [{ at: [0, -500, 0], data_version: 4556, snbt: '{}' }] }),
  (a) => ({ ...a, block_entities: [{ at: [0, 0, 0], data_version: 4556 }] }),
  (a) => ({ ...a, actions: [...(a.actions || []), { tick: 0, type: 'drop', at: [0, 0, 0], item: 'minecraft:stone', count: 1, velocity: [0, 99, 0] }] }),
];
const mutate = (args) => {
  const a = structuredClone(args);
  const last = Math.max(0, ...(a.actions || []).map((x) => x.tick));
  const choice = int(0, 9);
  if (choice <= 2) {
    const step = int(1, 7), start = int(0, 3);
    a.samples = Array.from({ length: int(1, 40) }, (_, i) => start + i * step);
    if (a.samples.at(-1) < last) a.samples.push(last + int(0, 5));
  } else if (choice === 3) a.samples = [...Array.from({ length: int(1, 12) }, () => int(0, last + 60)), last];
  else if (choice === 4) a.settle_ticks = int(1, 120);
  else if (choice === 5) { a.item_entities = random() < 0.5; a.living_entities = random() < 0.5; a.world_inventories = random() < 0.5; }
  else if (choice === 6 && a.actions?.length) a.actions = a.actions.filter(() => random() < 0.6);
  else if (choice === 7) a.probes = [...a.probes, ...Array.from({ length: int(1, 20) }, () => [int(-3, 12), int(0, 6), int(-3, 12)])];
  else if (choice === 8) a.actions = [...(a.actions || [])].reverse();
  else a.samples = [...(a.samples || [0, 20, 100])].reverse();
  return a;
};
for (let i = 0; i < 260; i++) {
  const { fixture, saved: world, mode } = pick(usable);
  let args = mutate(fixture.args);
  if (random() < 0.2) args = pick(INVALID)(args);
  call(`generated/${i}-${fixture.name}`, world, [args], mode);
}
for (let i = 0; i < 40; i++) {
  const { fixture, saved: world, mode } = pick(usable);
  call(`generated/multi-${i}-${fixture.name}`, world, Array.from({ length: int(2, 8) }, () => (random() < 0.15 ? pick(INVALID)(mutate(fixture.args)) : mutate(fixture.args))), mode);
}

fs.writeFileSync(path.join(out, 'cases.jsonl'), cases.map((c) => JSON.stringify(c)).join('\n') + '\n');
const kinds = {};
for (const c of cases) kinds[c.name.split('/')[0]] = (kinds[c.name.split('/')[0]] || 0) + 1;
console.log(`${cases.length} cases`, kinds);
