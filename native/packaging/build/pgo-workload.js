'use strict';

// The corpus recording a pinned engine profile is recorded with (pgo.md,
// The engine's pinned profile): the requests below, written once as a
// recording that pgo-replay.js sends to an instrumented `mcbench-lite
// headless engine` on every platform.
//
//   MCBENCH_CORPUS=<corpus> node native/packaging/build/pgo-workload.js <repository> <bundle> <work> <archive>
//
// <bundle> is the Minecraft data bundle (MCBENCH_DATA_BUNDLE). <archive> is
// written: the recording as a ustar archive (every file 0644, time 0, in
// the order of their paths) compressed by zstd at level 19. It holds one
// folder, `01-worlds`, for its one engine process, with `requests.txt` (the
// request lines of the released program's protocol, as server/engine.js's
// Engine writes them, with every input file named `@<n>` and every output
// `@out`), `in/<n>` (the input files, named by the first 16 hex digits of
// their sha256) and `world/` (the data pack Engine writes in an engine's
// folder before it starts). The requests are recorded without an engine:
// the Engine here starts this script as its program, which keeps what it is
// sent and answers every request as done, so no request depends on
// another's answer. This script loads the Node server's modules, which load
// only on macOS and Linux; Windows builds replay a recording made on one of
// them. <work> is made fresh: <work>/tree holds a copy of the repository's
// server/, scripts/ and renderer/ with their node_modules (npm ci, no
// install scripts: nothing the workload loads is a native addon) and
// Minecraft's three engine tables from the bundle in
// core/minecraft/<version>/, where server/ reads them.
//
// The requests are core-bench.js's corpus run on the community builds of
// MCBENCH_CORPUS (scripts/corpus.js): each build six times in a row, as
// core-bench.js's default rounds (one warmup and five timed) send it, placed
// fresh, its levers toggled and buttons pressed, 1000 ticks with light in
// requests of 100 (the levers toggled again at tick 500), then a seeded
// VERIFY of core-bench.js's experiment (100 ticks, every component and
// container probed, levers, buttons, container inserts, a hopper drop and a
// drain). Those builds are the benchmark's: the recording stays in a scratch
// folder and is never tracked. core-bench.js is not run itself: before any
// run it lays out the Paper server (a download, and Paper's launcher on a
// JDK), which the Rust engine does not read, so its experiment() and the
// corpus run's sequence are repeated here (marked below). A refusal of the
// engine's ends nothing here: the engine refuses a spent world's requests
// until the next fresh placement, as cheaply.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { spawn, spawnSync } = require('child_process');

// The recording engine: started by Engine as `node pgo-workload.js --record
// <folder>` in the engine's folder.
function record(folder) {
  fs.mkdirSync(path.join(folder, 'in'), { recursive: true });
  fs.cpSync(path.join(process.cwd(), 'world'), path.join(folder, 'world'), { recursive: true });
  const requests = fs.openSync(path.join(folder, 'requests.txt'), 'w');
  // The words that name input and output files, by request.
  const INPUTS = { PLACE: [2], VERIFY: [2, 3] };
  const OUTPUTS = { PLACE: [3], VERIFY: [4], TICK: [3, 4] };
  process.stdout.write('MCBENCH_ENGINE_READY\n');
  let buffer = '';
  process.stdin.on('data', (chunk) => {
    buffer += chunk;
    const lines = buffer.split('\n');
    buffer = lines.pop();
    for (const line of lines) {
      const words = line.split(' ');
      for (const at of INPUTS[words[0]] || []) {
        const bytes = fs.readFileSync(words[at]);
        const name = crypto.createHash('sha256').update(bytes).digest('hex').slice(0, 16);
        if (!fs.existsSync(path.join(folder, 'in', name))) fs.writeFileSync(path.join(folder, 'in', name), bytes);
        words[at] = `@${name}`;
      }
      for (const at of OUTPUTS[words[0]] || []) words[at] = '@out';
      fs.writeSync(requests, `${words.join(' ')}\n`);
      process.stdout.write(`MCBENCH_DONE ${words[1]} 0\n`);
    }
  });
  process.stdin.on('end', () => {
    fs.closeSync(requests);
    process.exit(0);
  });
}
if (process.argv[2] === '--record') {
  record(process.argv[3]);
  return;
}

const args = process.argv.slice(2);
if (args.length !== 4) {
  console.error('usage: MCBENCH_CORPUS=<corpus> node native/packaging/build/pgo-workload.js <repository> <bundle> <work> <archive>');
  process.exit(2);
}
const [repository, bundle, work, archive] = args.map((arg) => path.resolve(arg));
const recording = path.join(work, 'recording');
const tree = path.join(work, 'tree');

const TABLES = ['blocks.txt', 'loot.json', 'datafix.txt'];

// The tree the workload's modules load from.
function makeTree() {
  fs.rmSync(work, { recursive: true, force: true });
  fs.mkdirSync(path.join(tree, 'core'), { recursive: true });
  for (const name of ['server', 'scripts', 'renderer', 'package.json', 'package-lock.json']) fs.cpSync(path.join(repository, name), path.join(tree, name), { recursive: true });
  // server/engine.js reads the engine's toolchain file when it loads.
  fs.copyFileSync(path.join(repository, 'core', 'rust-toolchain.toml'), path.join(tree, 'core', 'rust-toolchain.toml'));
  // npm is the one beside this node (bin/../lib/node_modules on macOS and
  // Linux, node_modules beside node.exe on Windows).
  const prefix = path.dirname(process.execPath);
  const npm = process.platform === 'win32'
    ? path.join(prefix, 'node_modules', 'npm', 'bin', 'npm-cli.js')
    : path.join(prefix, '..', 'lib', 'node_modules', 'npm', 'bin', 'npm-cli.js');
  const installed = spawnSync(process.execPath, [npm, 'ci', '--ignore-scripts', '--omit=dev', '--no-audit', '--no-fund', '--loglevel=error'], { cwd: tree, stdio: 'inherit' });
  if (installed.status !== 0) throw new Error(`npm ci in ${tree} failed (exit code ${installed.status})`);
}

// The engine's tables from the bundle (a ustar archive compressed by zstd,
// native/tools/extract/README.md) into `folder`.
function unpackTables(folder) {
  const archive = zlib.zstdDecompressSync(fs.readFileSync(bundle));
  fs.mkdirSync(folder, { recursive: true });
  const found = [];
  for (let at = 0; at + 512 <= archive.length && archive[at] !== 0;) {
    const name = archive.subarray(at, at + 100).toString('utf8').replace(/\0.*$/s, '');
    const size = parseInt(archive.subarray(at + 124, at + 136).toString('utf8').replace(/\0.*$/s, '').trim(), 8);
    if (TABLES.includes(name)) {
      fs.writeFileSync(path.join(folder, name), archive.subarray(at + 512, at + 512 + size));
      found.push(name);
    }
    at += 512 + Math.ceil(size / 512) * 512;
  }
  const missing = TABLES.filter((name) => !found.includes(name));
  if (missing.length) throw new Error(`${bundle} holds no ${missing.join(', ')}`);
}

// The engines, numbered in the order they start (the recording's folders).
let engines = 0;
async function startEngine(modules, tables, name) {
  const folder = path.join(recording, `${String(++engines).padStart(2, '0')}-${name}`);
  const engine = new modules.Engine({ build: async () => process.execPath, args: [__filename, '--record', folder], spawnEngine: spawn });
  await engine.warm({ classpath: [], binaries: work, folder: path.join(work, `engine-${name}`), minecraftVersion: modules.MC_VERSION });
  return engine;
}

async function endEngine(engine, name) {
  const code = await new Promise((resolve) => { engine.child.once('close', resolve); engine.child.stdin.end(); });
  if (code !== 0) throw new Error(`the recording of run ${name} exited with code ${code}`);
}

// core-bench.js's COMPONENTS, CONTAINERS and experiment(), as it has them.
const COMPONENTS = /redstone|repeater|comparator|lever|button|piston|observer|lamp|note_block|dropper|dispenser|hopper|door|trapdoor|rail|target|bulb|crafter|tnt|bell|lectern/;
const CONTAINERS = /chest$|barrel|hopper$|dropper|dispenser|furnace$|smoker|shulker_box/;
function experiment(facts, blocks, first, ticks) {
  const probes = [];
  const actions = [];
  const containers = blocks.filter((block) => CONTAINERS.test(block.id)).slice(0, 16).map((block) => block.pos);
  probes.push(...containers);
  for (const block of blocks) {
    if (!COMPONENTS.test(block.id)) continue;
    if (probes.length < 256 && !containers.includes(block.pos)) probes.push(block.pos);
    if (facts.is(block.id, 'LeverBlock')) actions.push({ tick: 2, type: 'toggle', at: block.pos }, { tick: Math.floor(ticks / 2), type: 'toggle', at: block.pos });
    else if (facts.is(block.id, 'ButtonBlock')) actions.push({ tick: 4, type: 'press', at: block.pos });
  }
  containers.slice(0, 8).forEach((at, index) => actions.push({ tick: 3 + index, type: 'insert', at, slot: 0, count: 16, item: 'minecraft:cobblestone' }));
  const hopper = blocks.find((block) => block.id === 'minecraft:hopper');
  if (hopper) actions.push({ tick: 6, type: 'drop', at: [hopper.pos[0] + 0.5, hopper.pos[1] + 1.5, hopper.pos[2] + 0.5], item: 'minecraft:redstone', count: 5, velocity: [0, 0, 0] });
  if (containers.length) actions.push({ tick: ticks - 10, type: 'drain', at: containers[0] });
  if (!probes.length) probes.push(first.pos);
  return {
    probes, actions, samples: Array.from({ length: Math.floor(ticks / 1) + 1 }, (_, index) => index),
    ...(containers.length ? { inventory_probes: containers, inventory_missing: 'report', world_inventories: true } : {}),
    item_entities: true, living_entities: true,
  };
}

// core-bench.js's corpus run (its runItem with the place, tick and verify
// phases, --seeded) on each world.
async function corpusRun(modules, tables, worlds) {
  const { facts, importCommands, Y_OFFSET, MinecraftVerifier } = modules;
  const engine = await startEngine(modules, tables, 'worlds');
  const verifier = new MinecraftVerifier({ root: path.join(work, 'verifier'), placementMode: 'updates', seed: 1 });
  const folder = path.join(work, 'worlds');
  const file = (name) => path.join(folder, name);
  let refused = 0;
  for (const { world, blocks, first } of worlds) {
    fs.rmSync(folder, { recursive: true, force: true });
    fs.mkdirSync(path.join(folder, 'verify'), { recursive: true });
    fs.writeFileSync(file('commands.txt'), `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`);
    const inputs = (kind) => blocks.flatMap((block) => {
      const at = [block.pos[0], block.pos[1] + Y_OFFSET, block.pos[2]];
      if (facts.is(block.id, 'LeverBlock')) return [['toggle', ...at, 0]];
      if (kind === 'start' && facts.is(block.id, 'ButtonBlock')) return [['press', ...at, 0]];
      return [];
    });
    // An input on a lever or button that broke when the world settled is
    // refused by the engine and the ticks go on (core-bench.js ends the item
    // there).
    await engine.place(file('commands.txt'), file('blueprint.bin'), { fresh: true });
    for (let done = 0; done < 1000; done += 100) {
      if (done === 0) for (const input of inputs('start')) await engine.input(...input);
      if (done === 500) for (const input of inputs('middle')) await engine.input(...input);
      await engine.tick(100, file('patch.bin'), file('entities.json'), { light: true });
    }
    // The verifier refuses a world outside Minecraft's heights, as it
    // refuses it in the app.
    let request;
    try {
      request = verifier.request(world, experiment(facts, blocks, first, 100));
    } catch (error) {
      refused++;
      continue;
    }
    fs.writeFileSync(file('request.json'), JSON.stringify(verifier.transport(request)));
    await engine.verify(file('request.json'), file('commands.txt'), file('verify'));
  }
  await endEngine(engine, 'worlds');
  console.log(`pgo-workload: run worlds: ${worlds.length} worlds placed and ticked 1000 times, ${worlds.length - refused} verified (${refused} outside Minecraft's heights)`);
}

// The recording as the archive: a ustar archive of its files, each 0644,
// owner 0, time 0, in the order of their paths, compressed by zstd at level 19.
function pack() {
  const files = [];
  const walk = (folder, prefix) => {
    for (const entry of fs.readdirSync(folder, { withFileTypes: true })) {
      if (entry.isDirectory()) walk(path.join(folder, entry.name), `${prefix}${entry.name}/`);
      else files.push(`${prefix}${entry.name}`);
    }
  };
  walk(recording, '');
  files.sort();
  const blocks = [];
  for (const name of files) {
    if (Buffer.byteLength(name) > 100) throw new Error(`${name} is longer than a ustar name`);
    const bytes = fs.readFileSync(path.join(recording, name));
    const header = Buffer.alloc(512);
    header.write(name, 0);
    header.write('0000644\0', 100);
    header.write('0000000\0', 108);
    header.write('0000000\0', 116);
    header.write(`${bytes.length.toString(8).padStart(11, '0')}\0`, 124);
    header.write('00000000000\0', 136);
    header.write('        ', 148);
    header.write('0', 156);
    header.write('ustar\0', 257);
    header.write('00', 263);
    const sum = header.reduce((total, byte) => total + byte, 0);
    header.write(`${sum.toString(8).padStart(6, '0')}\0 `, 148);
    blocks.push(header, bytes, Buffer.alloc((512 - (bytes.length % 512)) % 512));
  }
  blocks.push(Buffer.alloc(1024));
  const compressed = zlib.zstdCompressSync(Buffer.concat(blocks), { params: { [zlib.constants.ZSTD_c_compressionLevel]: 19, [zlib.constants.ZSTD_c_checksumFlag]: 1 } });
  fs.writeFileSync(archive, compressed);
  const sha = crypto.createHash('sha256').update(compressed).digest('hex');
  console.log(`pgo-workload: ${archive}: ${files.length} files, ${compressed.length} bytes, sha256 ${sha}`);
}

async function main() {
  makeTree();
  const { MC_VERSION } = require(path.join(tree, 'server', 'assets'));
  const tables = path.join(tree, 'core', 'minecraft', MC_VERSION);
  unpackTables(tables);
  const modules = {
    MC_VERSION,
    Engine: require(path.join(tree, 'server', 'engine')).Engine,
    World: require(path.join(tree, 'server', 'world')).World,
    importCommands: require(path.join(tree, 'server', 'live-placement')).importCommands,
    Y_OFFSET: require(path.join(tree, 'server', 'minecraft-dimension')).Y_OFFSET,
    MinecraftVerifier: require(path.join(tree, 'server', 'minecraft-verify')).MinecraftVerifier,
    facts: require(path.join(tree, 'server', 'minecraft-facts')).minecraftFacts(),
  };
  // core-bench.js's items of the corpus: each build's world, and its blocks
  // and first block as the artifact lists them.
  const builds = require(path.join(tree, 'scripts', 'corpus')).corpusArtifacts().sort(([a], [b]) => a.localeCompare(b));
  await corpusRun(modules, tables, builds.flatMap(([name, file]) => {
    const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
    const world = new modules.World();
    for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
    world.finalize();
    return Array(6).fill({ name, world, blocks: artifact.blocks, first: artifact.blocks[0] });
  }));
  pack();
}

main().catch((error) => {
  console.error(`pgo-workload: ${error.message}`);
  process.exit(1);
});
