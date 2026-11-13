'use strict';

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFile } = require('child_process');
const { promisify } = require('util');
const os = require('os');
const { PaperServer } = require('./paper-server');
const { coreEngine } = require('./engine');
const { importCommands } = require('./live-placement');
const { World } = require('./world');
const { MC_VERSION } = require('./assets');
const { JAVAC } = require('./tools');
const { MIN_MC_Y, MAX_MC_Y, Y_OFFSET, validateWorldHeight } = require('./minecraft-dimension');

const execFileAsync = promisify(execFile);
const DATA_DIR = path.resolve(process.env.MCBENCH_DATA_DIR || path.join(__dirname, '..', 'data'));
const ROOT = path.join(DATA_DIR, 'minecraft-verify');
const CACHE_SOURCE = path.resolve(process.env.MCBENCH_MC_DATA_DIR || path.join(DATA_DIR, 'minecraft-live', 'paper'));
const { TEST_BUILD_TOOL, BUDGETS, BUDGET_ENV, DEFAULT_SAMPLES, DEFAULT_SETTLE_TICKS, MAX_INLINE_BYTES, entityCoordinates, validateEntities } = require('./verification-contract');

class VerificationBudgetError extends Error {
  constructor(resource, requested, allowed) {
    const setting = BUDGET_ENV[resource];
    super(`Minecraft test needs ${requested} ${resource}; configured budget is ${allowed} (${setting})`);
    this.code = 'VERIFICATION_BUDGET_EXCEEDED';
    this.budget = { resource, requested, allowed, setting };
  }
}

// A test that ran out of wall-clock time: its content was too dense to finish
// in time. `reached` is the last progress the engine reported ({phase, tick,
// totalTicks}: settling and then the timeline each count their own ticks).
class WallBudgetError extends VerificationBudgetError {
  constructor(elapsedMs, allowedMs, { reached }) {
    super('wallMs', elapsedMs, allowedMs);
    const seconds = (ms) => Math.round(ms / 1000);
    const where = !reached || reached.phase === 'activating' ? 'had not started ticking yet'
      : reached.phase === 'settling' ? `was still settling, at tick ${reached.tick} of ${reached.totalTicks} settling ticks`
        : `reached tick ${reached.tick} of ${reached.totalTicks} of its timeline, after its settling ticks`;
    this.message = `The test was too dense to finish within its ${seconds(allowedMs)}-second wall-clock budget (${BUDGET_ENV.wallMs}) and was stopped: it ${where}. Shorten it (fewer ticks, a smaller area, less active circuitry) or split it into several tests; this is a limit of the harness, not a result about the build.`;
    this.budget = { ...this.budget, phase: reached?.phase ?? null, ticksReached: reached?.tick ?? 0, totalTicks: reached?.totalTicks ?? null };
  }
}

function checkBudget(resource, requested, budgets) {
  if (requested > budgets[resource]) throw new VerificationBudgetError(resource, requested, budgets[resource]);
}

function integer(value, min, max, label) {
  if (!Number.isInteger(value) || value < min || value > max) throw new Error(`${label} must be an integer within ${min}..${max}`);
  return value;
}

// The experiments of a test_build call: {tests: [one experiment, ...]}.
function callTests(args, budgets = BUDGETS) {
  if (!args || typeof args !== 'object' || Array.isArray(args) || Object.keys(args).join() !== 'tests' || !Array.isArray(args.tests) || !args.tests.length) throw new Error('test_build requires {tests: [one experiment, ...]}');
  checkBudget('tests', args.tests.length, budgets);
  return args.tests;
}

// One experiment of a call (an entry of tests).
function validateRequest(world, args, yOffset = Y_OFFSET, budgets = BUDGETS) {
  if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('A test requires an object');
  for (const key of Object.keys(args)) if (!['probes', 'actions', 'samples', 'settle_ticks', 'block_entities', 'entities', 'scheduled_ticks', 'inventory_probes', 'inventory_missing', 'item_entities', 'world_inventories', 'living_entities', 'block_data_version'].includes(key)) throw new Error(`Unknown test argument: ${key}`);
  const requestBytes = Buffer.byteLength(JSON.stringify(args));
  checkBudget('requestBytes', requestBytes, budgets);
  validateWorldHeight(world, yOffset);
  if (!world.size()) throw new Error('The blueprint is empty');
  const list = (value, min, max, label) => {
    if (!Array.isArray(value) || value.length < min || value.length > max) throw new Error(`${label} requires ${min}..${max} entries`);
    return value;
  };
  const position = (value) => {
    list(value, 3, 3, 'position');
    return value.map((n, axis) => integer(n, axis === 1 ? MIN_MC_Y - yOffset : -4096, axis === 1 ? MAX_MC_Y - yOffset : 4096, 'coordinate'));
  };
  const probes = list(args.probes, 1, Number.MAX_SAFE_INTEGER, 'probes').map(position);
  const samples = list(args.samples ?? DEFAULT_SAMPLES, 1, Number.MAX_SAFE_INTEGER, 'samples').map(t => integer(t, 0, 2_147_483_647, 'sample tick'));
  // Dense long experiments can exceed V8's Set entry limit. Sort only when
  // needed, then deduplicate the validated copy without another collection.
  for (let i = 1; i < samples.length; i++) {
    if (samples[i] < samples[i - 1]) { samples.sort((a, b) => a - b); break; }
  }
  let unique = 1;
  for (let i = 1; i < samples.length; i++) if (samples[i] !== samples[unique - 1]) samples[unique++] = samples[i];
  samples.length = unique;
  const inventoryProbes = list(args.inventory_probes ?? [], 0, Number.MAX_SAFE_INTEGER, 'inventory_probes').map(position);
  const probeKeys = new Set(probes.map(p => p.join(',')));
  if (inventoryProbes.some(p => !probeKeys.has(p.join(','))) || new Set(inventoryProbes.map(p => p.join(','))).size !== inventoryProbes.length) throw new Error('inventory_probes must be distinct positions included in probes');
  if (args.inventory_missing !== undefined && !['error', 'report'].includes(args.inventory_missing)) throw new Error('inventory_missing must be error or report');
  if (args.world_inventories !== undefined && typeof args.world_inventories !== 'boolean') throw new Error('world_inventories must be a boolean');
  if (args.living_entities !== undefined && typeof args.living_entities !== 'boolean') throw new Error('living_entities must be a boolean');
  if (args.item_entities !== undefined && typeof args.item_entities !== 'boolean') throw new Error('item_entities must be a boolean');
  const blockEntities = list(args.block_entities ?? [], 0, Number.MAX_SAFE_INTEGER, 'block_entities').map(entry => {
    if (!entry || Object.keys(entry).sort().join(',') !== 'at,data_version,snbt' || typeof entry.snbt !== 'string' || !entry.snbt.trim()) throw new Error('block_entities require at, data_version and nonempty snbt');
    return { at: position(entry.at), data_version: integer(entry.data_version, 0, 2147483647, 'data_version'), snbt: entry.snbt };
  });
  if (new Set(blockEntities.map(e => e.at.join(','))).size !== blockEntities.length) throw new Error('Duplicate block entity position');
  const entityPosition = value => entityCoordinates(value, yOffset);
  const entities = validateEntities(args.entities ?? [], yOffset);
  let scheduledTicks;
  if (args.scheduled_ticks !== undefined) {
    const saved = args.scheduled_ticks;
    if (!saved || Object.keys(saved).sort().join(',') !== 'bounds,data_version,entries') throw new Error('scheduled_ticks require bounds, data_version and entries');
    const bounds = list(saved.bounds,2,2,'tick bounds').map(position);
    if (bounds[0].some((v,i)=>v>bounds[1][i])) throw new Error('Reversed scheduled tick bounds');
    const entries = list(saved.entries,0,Number.MAX_SAFE_INTEGER,'scheduled tick entries').map(e=>{
      if (!e || Object.keys(e).sort().join(',') !== 'at,delay,id,kind,priority,sub_tick' || !['block','fluid'].includes(e.kind) || typeof e.id !== 'string' || !/^minecraft:[a-z0-9_]+$/.test(e.id)) throw new Error('Invalid scheduled tick entry');
      const at=position(e.at);if(at.some((v,i)=>v<bounds[0][i]||v>bounds[1][i]))throw new Error('Scheduled tick outside saved region');
      if(typeof e.sub_tick!=='string'||!/^(-?[0-9]+)$/.test(e.sub_tick)||BigInt(e.sub_tick)<-(1n<<63n)||BigInt(e.sub_tick)>=(1n<<63n))throw new Error('sub_tick must be a signed 64-bit decimal string');
      return {kind:e.kind,id:e.id,at,delay:integer(e.delay,-2147483648,2147483647,'tick delay'),priority:integer(e.priority,-3,3,'tick priority'),sub_tick:e.sub_tick};
    });
    if(new Set(entries.map(e=>e.kind+':'+e.id+':'+e.at.join(','))).size!==entries.length)throw new Error('Duplicate scheduled tick');
    scheduledTicks={bounds,data_version:integer(saved.data_version,0,2147483647,'data_version'),entries};
  }
  const actions = list(args.actions ?? [], 0, Number.MAX_SAFE_INTEGER, 'actions').map(action => {
    const inventory = ['insert', 'extract'].includes(action?.type), drop = action?.type === 'drop';
    const fields = action?.type === 'book' ? ['tick','type','at','present'] : action?.type === 'page' ? ['tick','type','at','page'] : drop ? ['tick','type','at','item','count','velocity'] : inventory ? ['tick', 'type', 'at', 'slot', 'count', ...(action.type === 'insert' ? ['item'] : [])] : ['tick', 'type', 'at'];
    if (action?.type === 'use' && Object.hasOwn(action, 'yaw')) fields.push('yaw');
    const suppliedBook = action?.type === 'book' && Object.hasOwn(action, 'stack');
    if (suppliedBook) fields.push('item', 'count', 'stack');
    else if ((drop || action?.type === 'insert') && Object.hasOwn(action, 'stack')) fields.push('stack');
    if (!action || typeof action !== 'object' || Array.isArray(action)
      || Object.keys(action).length !== fields.length || fields.some(k => !Object.hasOwn(action,k))
      || !['press', 'toggle', 'insert', 'extract', 'drain', 'use', 'drop', 'page', 'book'].includes(action.type)) throw new Error('Malformed input action');
    const result = { tick: integer(action.tick, 0, samples.at(-1), 'action tick'), type: action.type, at: drop ? entityPosition(action.at) : position(action.at) };
    if (action.type === 'book') { if (typeof action.present !== 'boolean') throw new Error('book present must be boolean'); result.present = action.present; }
    if (suppliedBook) {
      if (!action.present || !['minecraft:written_book','minecraft:writable_book'].includes(action.item) || action.count !== 1) throw new Error('Book supply requires present:true and exactly one written or writable book');
      result.item = action.item; result.count = 1;
    }
    if (action.type === 'page') result.page = integer(action.page, 0, 2147483547, 'page');
    if (Object.hasOwn(action, 'yaw')) {
      if (typeof action.yaw !== 'number' || !Number.isFinite(action.yaw)) throw new Error('yaw must be a finite number of degrees');
      result.yaw = action.yaw;
    }
    if (drop) {
      result.count = integer(action.count, 1, 64, 'count');
      if (typeof action.item !== 'string' || !/^minecraft:[a-z0-9_]+$/.test(action.item)) throw new Error('item must be a Minecraft item ID');
      result.item = action.item;
      result.velocity = list(action.velocity,3,3,'velocity').map(n=>{if(typeof n!=='number'||!Number.isFinite(n)||Math.abs(n)>10)throw new Error('Velocity must be finite and within native entity load range -10..10');return n;});
    }
    if (Object.hasOwn(action, 'stack')) {
      const stack = action.stack;
      if (!stack || typeof stack !== 'object' || Array.isArray(stack) || Object.keys(stack).sort().join(',') !== 'data_version,snbt' || typeof stack.snbt !== 'string' || !stack.snbt.trim()) throw new Error('stack requires data_version and nonempty SNBT');
      result.stack = {data_version: integer(stack.data_version, 0, 2147483647, 'stack data_version'), snbt: stack.snbt};
    }
    if (inventory) {
      result.slot = integer(action.slot, 0, 2147483647, 'slot');
      result.count = integer(action.count, 1, 64, 'count');
      if (action.type === 'insert') {
        if (typeof action.item !== 'string' || !/^minecraft:[a-z0-9_]+$/.test(action.item)) throw new Error('item must be a Minecraft item ID');
        result.item = action.item;
      }
    }
    return result;
  }).sort((a, b) => a.tick - b.tick);
  const settleTicks = integer(args.settle_ticks ?? DEFAULT_SETTLE_TICKS, 1, 2_147_483_647, 'settle_ticks');
  const cost = { operations: (probes.length + inventoryProbes.length * 54) * (samples.length + 1) + actions.length + blockEntities.length + entities.length + (scheduledTicks?.entries.length || 0), simulationTicks: settleTicks + samples.at(-1), requestBytes };
  checkBudget('operations', cost.operations, budgets);
  checkBudget('simulationTicks', cost.simulationTicks, budgets);
  const columns = new Map();
  function include(cx, cz) {
    for (let dx = -1; dx <= 1; dx++) for (let dz = -1; dz <= 1; dz++) columns.set(`${cx + dx},${cz + dz}`, [cx + dx, cz + dz]);
    checkBudget('columns', columns.size, budgets);
  }
  if (scheduledTicks) for(let cx=Math.floor(scheduledTicks.bounds[0][0]/16);cx<=Math.floor(scheduledTicks.bounds[1][0]/16);cx++) for(let cz=Math.floor(scheduledTicks.bounds[0][2]/16);cz<=Math.floor(scheduledTicks.bounds[1][2]/16);cz++) include(cx,cz);
  for (const key of world.chunks.keys()) { const [cx, , cz] = World.coordsOf(key); include(cx, cz); }
  for (const p of [...probes, ...actions.map(a => a.at), ...blockEntities.map(e => e.at), ...entities.map(e => e.at), ...(scheduledTicks ? [...scheduledTicks.bounds,...scheduledTicks.entries.map(e=>e.at)] : [])]) include(Math.floor(p[0] / 16), Math.floor(p[2] / 16));
  return { ...(args.living_entities !== undefined ? {living_entities:args.living_entities} : {}), ...(args.block_data_version !== undefined ? {block_data_version:integer(args.block_data_version,0,2147483647,'block_data_version')} : {}), ...(args.world_inventories !== undefined ? {world_inventories:args.world_inventories} : {}), ...(scheduledTicks ? {scheduled_ticks:scheduledTicks} : {}), ...(args.inventory_missing !== undefined ? {inventory_missing:args.inventory_missing} : {}), ...(args.item_entities !== undefined ? {item_entities:args.item_entities} : {}), probes, samples, actions, entities, block_entities: blockEntities, inventory_probes: inventoryProbes, settle_ticks: settleTicks, duration: samples.at(-1), yOffset, columns: [...columns.values()], cost: { ...cost, columns: columns.size }, budgets: { ...budgets } };
}

function jars(directory) {
  if (!fs.existsSync(directory)) return [];
  return fs.readdirSync(directory, { withFileTypes: true }).flatMap(entry => {
    const file = path.join(directory, entry.name);
    return entry.isDirectory() ? jars(file) : entry.name.endsWith('.jar') ? [file] : [];
  });
}

// The compiler's version, asked once per process and compiler. An ask that
// failed is forgotten, so the next compile asks again.
const compilerVersions = {};

// The plugin's Java sources: its host and the experiment it shares with the
// engine.
const PLUGIN_SOURCES = ['McbenchVerify.java', 'McbenchExperiment.java'];
function pluginSources() {
  return Object.fromEntries(PLUGIN_SOURCES.map((name) => [name, fs.readFileSync(path.join(__dirname, name), 'utf8')]));
}

async function compilePlugin(paperDir, { signal, sources = pluginSources(), minecraftVersion = MC_VERSION, cacheDir = path.join(DATA_DIR, 'verification-plugin-cache') } = {}) {
  signal?.throwIfAborted();
  sources = require('./native-runtime-version').verifierSourcesForVersion(sources, minecraftVersion);
  const sourceDirectory = path.join(paperDir, 'verify-source');
  fs.rmSync(sourceDirectory, { recursive: true, force: true });
  fs.mkdirSync(sourceDirectory, { recursive: true });
  const files = Object.entries(sources).map(([name, text]) => {
    fs.writeFileSync(path.join(sourceDirectory, name), text);
    return path.join(sourceDirectory, name);
  });
  const serverJar = path.join(paperDir, 'versions', minecraftVersion, `paper-${minecraftVersion}.jar`);
  const dependencies = [serverJar, ...jars(path.join(paperDir, 'libraries'))];
  const compiler = process.env.MCBENCH_JAVAC || JAVAC;
  compilerVersions[compiler] ||= execFileAsync(compiler, ['-version'], { timeout: 30_000 }).catch((error) => {
    delete compilerVersions[compiler];
    throw error;
  });
  const compilerVersion = await compilerVersions[compiler];
  const digest = crypto.createHash('sha256');
  for (const [name, text] of Object.entries(sources)) digest.update(name).update(text);
  digest.update(minecraftVersion).update('release21-mojang-v1').update(compiler).update(compilerVersion.stdout).update(compilerVersion.stderr);
  // Paper's laid-out files are fixed by its pinned build; each one's path and
  // size name it.
  for (const dependency of dependencies) digest.update(path.relative(paperDir, dependency)).update(String(fs.statSync(dependency).size));
  const hash = digest.digest('hex');
  const pluginDir = path.join(paperDir, 'plugins');
  const jar = path.join(pluginDir, 'mcbench-verify.jar');
  const stamp = path.join(pluginDir, 'mcbench-verify.sha256');
  if (fs.existsSync(jar) && fs.existsSync(stamp) && fs.readFileSync(stamp, 'utf8') === hash) return;
  const cached = path.join(cacheDir, hash);
  fs.mkdirSync(pluginDir, { recursive: true });
  // The stamp goes before the jar changes and comes back after the new jar
  // is in place whole, so a crash between leaves no stamp naming a wrong jar.
  fs.rmSync(stamp, { force: true });
  const placeJar = (bytes) => {
    fs.writeFileSync(`${jar}.part`, bytes);
    fs.renameSync(`${jar}.part`, jar);
    fs.writeFileSync(stamp, hash);
  };
  try {
    const bytes = fs.readFileSync(path.join(cached, 'plugin.jar'));
    if (crypto.createHash('sha256').update(bytes).digest('hex') === fs.readFileSync(path.join(cached, 'jar.sha256'), 'utf8')) {
      placeJar(bytes);
      return;
    }
  } catch (error) { if (error.code !== 'ENOENT') throw error; }
  const build = path.join(paperDir, 'verify-classes');
  fs.rmSync(build, { recursive: true, force: true });
  fs.mkdirSync(build, { recursive: true });
  await execFileAsync(compiler, ['--release', '21', '-Xlint:-removal', '-proc:none', '-classpath', dependencies.join(path.delimiter), '-d', build, ...files], { timeout: 60_000, signal });
  fs.writeFileSync(path.join(build, 'plugin.yml'), `name: McbenchVerify\nversion: '1'\nmain: McbenchVerify\napi-version: '${minecraftVersion}'\ncommands:\n  mcbenchverify:\n    description: Internal verification control\n`);
  const manifest = path.join(paperDir, 'verify-manifest.mf');
  fs.writeFileSync(manifest, 'Manifest-Version: 1.0\npaperweight-mappings-namespace: mojang\n\n');
  const built = path.join(paperDir, 'verify-plugin.jar');
  await execFileAsync(process.env.MCBENCH_JAR || 'jar', ['cfm', built, manifest, '-C', build, '.'], { timeout: 30_000, signal });
  const bytes = fs.readFileSync(built);
  fs.rmSync(built);
  placeJar(bytes);
  // Publish a complete immutable entry; concurrent builders never see partial jars.
  fs.mkdirSync(cacheDir, { recursive: true });
  const temporary = fs.mkdtempSync(path.join(cacheDir, '.building-'));
  try {
    fs.writeFileSync(path.join(temporary, 'plugin.jar'), bytes);
    fs.writeFileSync(path.join(temporary, 'jar.sha256'), crypto.createHash('sha256').update(bytes).digest('hex'));
    try { fs.renameSync(temporary, cached); }
    catch (error) { if (!['EEXIST', 'ENOTEMPTY'].includes(error.code)) throw error; }
  } finally { fs.rmSync(temporary, { recursive: true, force: true }); }
}

function seedCache(paperDir, source) {
  fs.mkdirSync(paperDir, { recursive: true });
  if (!source || !fs.existsSync(source)) return;
  // Copy only Paper's laid-out files, never player worlds, plugins, or player data.
  const names = ['libraries', 'versions'];
  names.push(...fs.readdirSync(source).filter(name => /^paper-.*\.jar$/.test(name)));
  for (const name of names) {
    const from = path.join(source, name); const to = path.join(paperDir, name);
    if (fs.existsSync(from) && !fs.existsSync(to)) fs.cpSync(from, to, { recursive: true, mode: fs.constants.COPYFILE_FICLONE });
  }
}

// A test's experiment as both runtimes take it: validated against the
// world, with the verifier's settings, and the form written for Minecraft.
// `seed` fixes the test world's random draws, for comparing runtimes
// (scripts/ticker-parity.js); tests from the app use none.
class Experiments {
  constructor({ budgets = BUDGETS, sprint = true, reportEncoding = 'full', asyncPreparation = true, placementMode = 'updates', seed = null }) {
    this.budgets = budgets;
    this.sprint = sprint;
    if (!['updates', 'strict'].includes(placementMode)) throw new Error('Unsupported placement mode');
    this.placementMode = placementMode;
    if (!['full', 'rle-v1'].includes(reportEncoding)) throw new Error('Unsupported report encoding');
    this.reportEncoding = reportEncoding;
    this.asyncPreparation = asyncPreparation;
    this.seed = seed;
    this.queue = Promise.resolve();
    this.pending = 0; // tests queued or running
    this.closed = false;
    this.shutdown = new AbortController();
  }

  async run(world, args, { signal: externalSignal, onUpdate = () => {}, reportPath } = {}) {
    const signal = AbortSignal.any([this.shutdown.signal, ...(externalSignal ? [externalSignal] : [])]);
    signal.throwIfAborted();
    // Snapshot and validate before queuing: later edits cannot change this test.
    const request = this.request(world, args);
    const snapshot = new World({
      chunks: new Map([...world.chunks].map(([key, chunk]) => [key, chunk.clone()])),
      palette: world.palette, blockCount: world.size(), revision: world.revision,
    });
    onUpdate({ phase: 'queued', message: 'Waiting for the Minecraft verifier' });
    let started = false;
    const task = this.queue.then(() => {
      signal.throwIfAborted();
      started = true;
      return this.execute(snapshot, request, signal, onUpdate, reportPath);
    });
    // Keep the resource queue tied to execution and cleanup, even when a
    // waiting caller cancels and receives its result immediately.
    this.queue = task.catch(() => {});
    this.pending += 1;
    this.queue.then(() => { this.pending -= 1; });
    return new Promise((resolve, reject) => {
      const cancelQueued = () => { if (!started) reject(signal.reason); };
      signal.addEventListener('abort', cancelQueued, { once: true });
      if (signal.aborted) cancelQueued();
      task.then(resolve, reject).finally(() => signal.removeEventListener('abort', cancelQueued));
    });
  }

  // The validated experiment for a world, with this verifier's settings.
  request(world, args) {
    if (args?.scheduled_ticks !== undefined && this.placementMode !== 'strict') {
      throw new Error('scheduled_ticks is unavailable in updates placement mode. Omit scheduled_ticks when testing a newly authored blueprint. Restoring an authoritative saved tick queue requires a verifier configured for strict placement; this test was not started.');
    }
    const request = validateRequest(world, args, Y_OFFSET, this.budgets);
    request.worldRevision = world.revision;
    request.sprint = this.sprint;
    request.placementMode = this.placementMode;
    return request;
  }

  // The experiment as the plugin reads it from its request file.
  transport(request) {
    const transport = { ...request, reportEncoding: this.reportEncoding, asyncPreparation: this.asyncPreparation, ...(this.seed === null ? {} : { seed: this.seed }) };
    if (request.samples.length > 1) {
      const step = request.samples[1] - request.samples[0];
      if (request.samples.every((tick, i) => tick === request.samples[0] + i * step)) {
        transport.sampleSequence = { start: request.samples[0], step, count: request.samples.length };
        delete transport.samples;
      }
    }
    return transport;
  }

}

// Whether a finished experiment observed everything its request asked for.
function checkComplete(request, result, reportFile) {
  if (result.elapsedTicks !== request.duration || result.sampleCount !== request.samples.length
    || result.probeCount !== request.probes.length || result.inputCount !== request.actions.length
    || result.observationCount !== request.probes.length * (request.samples.length + 1)
    || !fs.existsSync(reportFile) || fs.statSync(reportFile).size !== result.reportBytes) {
    throw new Error('Minecraft verification returned incomplete observations');
  }
}

// A complete experiment's result within its budget, archived: what
// test_build returns.
function finishedResult({ world, request, result, reportFile, archive, startedAt, note }) {
  checkBudget('reportBytes', result.reportBytes, request.budgets);
  fs.mkdirSync(path.dirname(archive), { recursive: true });
  fs.copyFileSync(reportFile, archive, fs.constants.COPYFILE_FICLONE);
  return {
    ...(result.reportBytes < MAX_INLINE_BYTES / 2 ? JSON.parse(fs.readFileSync(reportFile, 'utf8')) : { observationsInFile: true, probesInFile: true, inputsInFile: true }),
    ...result, worldRevision: world.revision, reportFile: archive,
    cost: request.cost, budgets: request.budgets, wallMs: Date.now() - startedAt, note,
  };
}

// The experiment's progress, as test_build reports it.
function progressUpdate(progress) {
  return { ...progress, message: progress.phase === 'activating'
    ? `Waiting for Minecraft chunks to tick: ${progress.tick}/${progress.totalTicks}`
    : progress.phase === 'settling'
    ? `Settling Minecraft: ${progress.tick}/${progress.totalTicks} ticks`
    : `Observing Minecraft: ${progress.tick}/${progress.totalTicks} ticks` };
}

// The reference runtime: a Paper server per verifier, which the plugin
// (McbenchVerify.java) drives. Scripts and calibrations run it; the app
// verifies with the engine (EngineVerifier), checked against it by
// scripts/ticker-parity.js.
class MinecraftVerifier extends Experiments {
  constructor({ root = ROOT, cacheSource, minecraftVersion = MC_VERSION, makeLive, compile, budgets = BUDGETS, sprint = true, reportEncoding = 'full', asyncPreparation = true, placementMode = 'updates', physicsGuard = process.env.MCBENCH_VERIFY_PHYSICS_GUARD === '1', seed = null } = {}) {
    super({ budgets, sprint, reportEncoding, asyncPreparation, placementMode, seed });
    this.root = root;
    if (!['1.21.4', '1.21.8', '1.21.10', '1.21.11'].includes(minecraftVersion)) throw new Error('Unsupported Minecraft runtime version');
    this.minecraftVersion = minecraftVersion;
    this.cacheSource = cacheSource ?? (minecraftVersion === MC_VERSION ? CACHE_SOURCE : null);
    // Pin native protocol code to this verifier instance. Editing tooling must
    // not hot-swap a running model's plugin underneath its cached JS protocol.
    const sources = pluginSources();
    this.compile = compile || ((directory, options) => compilePlugin(directory, { ...options, sources }));
    if (minecraftVersion === '1.21.4' && placementMode === 'strict') throw new Error('Minecraft 1.21.4 has no native strict setblock mode; select updates explicitly or use a newer runtime');
    this.preparePhysicsGuard = physicsGuard ? require('./native-speed').physicsGuardBuilder() : null;
    this.makeLive = makeLive || (() => new PaperServer({
      paperDir: path.join(root, 'paper'),
      minecraftVersion,
      gamePort: 0, bindHost: '127.0.0.1', memory: process.env.MCBENCH_VERIFY_MEMORY || '2G', idleMs: 0,
    }));
    this.active = null;
    this.runtime = null;
    this.idleTimer = null;
  }

  async execute(world, request, externalSignal, onUpdate, reportPath) {
    const startedAt = Date.now();
    const timeout = AbortSignal.timeout(request.budgets.wallMs);
    const signal = AbortSignal.any([externalSignal, timeout]);
    signal.throwIfAborted();
    if (this.closed) throw new Error('Minecraft verification is shutting down');
    clearTimeout(this.idleTimer);
    // A runtime whose server died since (a crash, a closed RCON connection) is not reused.
    if (this.runtime && !this.runtime.alive) {
      const dead = this.runtime;
      this.runtime = null;
      await dead.stop();
    }
    const reused = Boolean(this.runtime);
    const live = this.runtime || this.makeLive();
    this.active = live;
    // The runtime is kept for the next test when it ends in a state the
    // plugin reported: a pass, or a failure of the model's own request.
    let healthy = false;
    const timings = {};
    let phaseStart = Date.now();
    const phase = name => { timings[name] = Date.now() - phaseStart; phaseStart = Date.now(); };
    const cancel = () => { live.stop().catch(() => {}); };
    signal.addEventListener('abort', cancel, { once: true });
    try {
      onUpdate({ phase: 'preparing', message: 'Preparing the isolated Minecraft runtime' });
      // Placement settings come from this test's preparation alone.
      live.placementBatch = null;
      live.preloadedColumns = null;
      // A reused JVM already holds this instance's pinned plugin and agent.
      if (!reused) {
        seedCache(live.paperDir, this.cacheSource);
        signal.throwIfAborted();
        const serverJar = path.join(live.paperDir, 'versions', this.minecraftVersion, `paper-${this.minecraftVersion}.jar`);
        // First use on a clean install downloads Paper and its dependencies.
        if (!fs.existsSync(serverJar)) onUpdate({ phase: 'downloading', message: 'Downloading Paper and its dependencies' });
        await live.ensurePaper();
        signal.throwIfAborted();
        if (this.preparePhysicsGuard) {
          const supported = await this.preparePhysicsGuard(live.paperDir, serverJar, [serverJar, ...jars(path.join(live.paperDir, 'libraries'))], { signal });
          live.jvmOptions = supported ? `-javaagent:${path.join(live.paperDir, 'mcbench-speed', 'physics-guard.jar')}=${path.join(live.paperDir, 'mcbench-speed')}` : '';
          if (!supported) onUpdate({ phase: 'preparing', message: 'Using stock Minecraft execution for this server version' });
        }
        onUpdate({ phase: 'compiling', message: 'Preparing the verification plugin' });
        await this.compile(live.paperDir, { signal, minecraftVersion: this.minecraftVersion });
        signal.throwIfAborted();
      }
      const pluginData = path.join(live.paperDir, 'plugins', 'McbenchVerify');
      fs.mkdirSync(pluginData, { recursive: true });
      const resultFile = path.join(pluginData, 'result.json');
      const reportFile = path.join(pluginData, 'report.json');
      const progressFile = path.join(pluginData, 'progress.json');
      const preparedFile = path.join(pluginData, 'prepared.json');
      fs.rmSync(preparedFile, { force: true });
      fs.rmSync(resultFile, { force: true });
      fs.rmSync(reportFile, { force: true });
      fs.rmSync(progressFile, { force: true });
      const transport = this.transport(request);
      // A fresh file per experiment.
      const requestName = `request-${crypto.randomUUID()}.json`;
      fs.writeFileSync(path.join(pluginData, requestName), JSON.stringify(transport));
      onUpdate({ phase: reused ? 'resetting' : 'starting', message: reused ? 'Creating a fresh world in the ready Paper runtime' : 'Starting the isolated Paper server' });
      if (!reused) await live.startServer();
      phase('startupMs');
      signal.throwIfAborted();
      // Loading all requested chunks may exceed the ordinary RCON timeout.
      // The experiment's abort signal still enforces its configured wall budget.
      const checkRuntimeAlive = require('./verification-liveness').runtimeLivenessCheck(live);
      let ready = await live.command(`mcbenchverify prepare ${requestName}`, true, request.budgets.wallMs)
        .finally(() => fs.rmSync(path.join(pluginData, requestName), { force: true }));
      if (ready.includes('MCBENCH_PREPARING')) {
        let last = '';
        while (!fs.existsSync(preparedFile)) {
          signal.throwIfAborted();
          checkRuntimeAlive();
          if (fs.existsSync(progressFile)) {
            const text = fs.readFileSync(progressFile, 'utf8');
            if (text !== last) {
              const p = JSON.parse(text);
              onUpdate({ phase: 'loading', loadedColumns: p.tick, totalColumns: p.totalTicks, message: `Loading Minecraft chunks: ${p.tick}/${p.totalTicks}` });
              last = text;
            }
          }
          await new Promise(resolve => setTimeout(resolve, 100));
        }
        const prepared = JSON.parse(fs.readFileSync(preparedFile));
        if (!prepared.ok) {
          healthy = true;
          throw new Error(`Minecraft preparation failed: ${prepared.error}`);
        }
        if (prepared.batchPlacement) {
          live.placementBatch = { directory: 'McbenchVerify', command: 'mcbenchverify apply' };
          live.preloadedColumns = new Set(request.columns.map(([x, z]) => `${x},${z}`));
        }
        ready = `MCBENCH_READY ${prepared.dimension}`;
      }
      if (!ready.includes('MCBENCH_READY')) throw new Error(`Minecraft verification plugin unavailable: ${ready}`);
      if (request.placementMode === 'strict') live.placementBatch = { directory: 'McbenchVerify', command: 'mcbenchverify apply' };
      live.freshWorld(ready.match(/MCBENCH_READY ([a-z0-9_.-]+:[a-z0-9_./-]+)/)[1]);
      phase('resetMs');
      signal.throwIfAborted();
      onUpdate({ phase: 'placing', message: `Importing blueprint revision ${world.revision}` });
      await live.applyWorld(world, { resumeTicks: false });
      phase('placementMs');
      signal.throwIfAborted();
      onUpdate({ phase: 'running', message: 'Running Minecraft inputs and observations' });
      const started = await live.command('mcbenchverify run', true, request.budgets.wallMs);
      if (!started.includes('MCBENCH_RUNNING') && !fs.existsSync(resultFile)) throw new Error(`Minecraft verification could not start: ${started}`);
      let lastProgress = '';
      while (!fs.existsSync(resultFile)) {
        signal.throwIfAborted();
        checkRuntimeAlive();
        if (fs.existsSync(progressFile)) {
          const text = fs.readFileSync(progressFile, 'utf8');
          if (text !== lastProgress) {
            onUpdate(progressUpdate(JSON.parse(text)));
            lastProgress = text;
          }
        }
        await new Promise(resolve => setTimeout(resolve, 100));
      }
      signal.throwIfAborted();
      const result = JSON.parse(fs.readFileSync(resultFile, 'utf8'));
      if (!result.ok) {
        healthy = true;
        if (result.budgetResource) throw new VerificationBudgetError(result.budgetResource, result.requested, request.budgets[result.budgetResource]);
        throw new Error(`Minecraft verification failed: ${result.error}`);
      }
      checkComplete(request, result, reportFile);
      healthy = true;
      phase('simulationMs');
      const finished = finishedResult({ world, request, result, reportFile, archive: reportPath || path.join(this.root, 'reports', `${crypto.randomUUID()}.json`), startedAt,
        note: 'Observed in an isolated Paper copy. Experiment completion is not a design pass. Initial is before inputs; observations are after inputs at each sample tick.' });
      phase('archiveMs');
      onUpdate({ phase: 'running', tick: result.elapsedTicks, totalTicks: request.duration, message: 'Minecraft observations complete' });
      return { ...finished, timings: { ...timings, reusedRuntime: reused } };
    } catch (error) {
      if (timeout.aborted && !externalSignal.aborted) throw new VerificationBudgetError('wallMs', Date.now() - startedAt, request.budgets.wallMs);
      throw error;
    } finally {
      signal.removeEventListener('abort', cancel);
      try {
        onUpdate({ phase: 'cleanup', message: healthy ? 'Keeping the isolated runtime ready for the next fresh world' : 'Stopping the isolated test server' });
      } finally {
        try {
          if (healthy && !signal.aborted && !this.closed) {
            this.runtime = live;
            this.idleTimer = setTimeout(() => {
              // Serialize retirement with queued tests so a timer cannot stop their runtime.
              this.queue = this.queue.then(async () => {
                if (this.active || this.runtime !== live) return;
                this.runtime = null;
                await live.stop();
              }).catch(() => {});
            }, 5 * 60_000);
            this.idleTimer.unref();
          } else {
            this.runtime = null;
            await live.stop();
          }
        } finally { this.active = null; }
      }
    }
  }

  async stop() {
    this.closed = true;
    this.shutdown.abort(new Error('Minecraft verification is shutting down'));
    clearTimeout(this.idleTimer);
    await this.queue;
    const live = this.runtime; this.runtime = null;
    if (live) await live.stop();
  }
}

// What McbenchExperiment.start throws while it imports a test's block
// entities, scheduled ticks and entities (test/start-rejections.test.js keeps
// this list equal to the Java source's): the Paper plugin reports these
// before its experiment has started.
const START_REJECTIONS = [
  'No block entity at ',
  'Cannot import future block-entity data version ',
  'Block entity ID mismatch at ',
  'Invalid block entity at ',
  'Saved tick restoration requires frozen strict placement',
  'Future scheduled ticks cannot be downgraded',
  'Scheduled tick outside saved bounds',
  'Saved tick order leaves no room for subsequent native updates',
  'Unknown saved block tick: ',
  'Unknown saved fluid tick: ',
  'Unknown saved tick kind',
  'Missing restored block tick',
  'Native queue changed saved block tick timing or order',
  'Missing restored fluid tick',
  'Native queue changed saved fluid tick timing or order',
  'Cannot import future entity data version ',
  'Native entity import currently supports boats, rafts, dropped items, experience orbs, item frames, paintings, armor stands and ordinary container/furnace/TNT minecarts only: ',
  'Entity leash relationships require a faithful relationship importer',
  'Invalid passenger list',
  'Unsupported passengers: only a single original dolphin riding an ordinary minecart is supported',
  'Invalid dolphin passenger',
  'Unsupported passenger family or relationship',
  'Passenger requires original UUID',
  'Invalid passenger UUID',
  'Passenger requires original Pos',
  'Invalid passenger Pos',
  'Invalid passenger coordinate',
  'Passenger is outside the mounted-entity import margin',
  'Entity needs Pos',
  'Entity needs three Pos coordinates and its source UUID',
  'Entity needs a native UUID int array',
  'Entity UUID must contain four ints',
  'Invalid entity Pos',
  'Entity Pos does not match declared at',
  'Item frame needs native attachment block_pos',
  'Invalid frame attachment coordinates',
  'Invalid native entity ',
  'Native frame loader rejected the source attachment position',
  'Frame attachment/facing does not match source Pos',
  'Native import changed source entity UUID',
  'Native import lost passenger relationship',
  'Native import changed dolphin passenger identity',
  'Native import changed saved passenger position',
  'Failed native entity insertion (possibly duplicate UUID): ',
  'Could not audit imported entity ',
];

// An experiment the engine rejected, worded as the Paper plugin words it,
// which is what graders and models read: a rejection while the test's data
// is imported is "could not start: MCBENCH_ERROR <message>"; any other
// failure is "failed: <exception>". A fault of the engine process itself
// ("The engine stopped") keeps its own words.
function verificationFailure(error) {
  const rejected = /^The engine failed: (.*)$/s.exec(error.message);
  if (!rejected) return error;
  // The engine appends the exception's causes (" <- ...") and where it arose (" @ frame"); the plugin reports the exception alone.
  const exception = rejected[1].split(/ <- | @ /)[0];
  const message = exception.replace(/^(?:[\w$]+\.)+[\w$]*(?:Exception|Error): /, '');
  return START_REJECTIONS.some((prefix) => message.startsWith(prefix))
    ? new Error(`Minecraft verification could not start: MCBENCH_ERROR ${message}\n`)
    : new Error(`Minecraft verification failed: ${exception}`);
}

// The app's runtime: the Rust engine (engine.js) runs each experiment in a
// level of its own, with reports identical to the Paper reference's
// (scripts/ticker-parity.js, scripts/core-verify-parity.js). An experiment
// that reaches what it does not model fails with the reason. Each verifier
// has its own engine process.
class EngineVerifier extends Experiments {
  constructor({ root = ROOT, engine = coreEngine(), budgets = BUDGETS, placementMode = 'updates' } = {}) {
    super({ budgets, placementMode });
    this.root = root;
    this.engine = engine;
  }

  async execute(world, request, externalSignal, onUpdate, reportPath) {
    const startedAt = Date.now();
    const timeout = AbortSignal.timeout(request.budgets.wallMs);
    const signal = AbortSignal.any([externalSignal, timeout]);
    signal.throwIfAborted();
    if (this.closed) throw new Error('Minecraft verification is shutting down');
    const timings = {};
    let phaseStart = Date.now();
    const phase = (name) => { timings[name] = Date.now() - phaseStart; phaseStart = Date.now(); };
    // The folders of runs a crash cut short; a run lasts at most its wall budget.
    this.swept ||= sweepRuns(path.join(this.root, 'runs'), 2 * this.budgets.wallMs);
    const folder = path.join(this.root, 'runs', crypto.randomUUID());
    fs.mkdirSync(folder, { recursive: true });
    // A cancelled or overlong test stops the engine mid-run; the next test
    // starts it again.
    const cancel = () => { this.engine.stop().catch(() => {}); };
    signal.addEventListener('abort', cancel, { once: true });
    let reached = null;
    try {
      onUpdate({ phase: 'preparing', message: 'Preparing Minecraft' });
      await this.warm();
      phase('startupMs');
      signal.throwIfAborted();
      const requestFile = path.join(folder, 'request.json');
      const commandsFile = path.join(folder, 'commands.txt');
      fs.writeFileSync(requestFile, JSON.stringify(this.transport(request)));
      fs.writeFileSync(commandsFile, `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`);
      onUpdate({ phase: 'running', message: 'Running Minecraft inputs and observations' });
      const progressFile = path.join(folder, 'progress.json');
      let lastProgress = '';
      const progress = setInterval(() => {
        if (!fs.existsSync(progressFile)) return;
        const text = fs.readFileSync(progressFile, 'utf8');
        if (text === lastProgress) return;
        lastProgress = text;
        reached = JSON.parse(text);
        onUpdate(progressUpdate(reached));
      }, 100);
      try {
        await this.engine.verify(requestFile, commandsFile, folder);
      } catch (error) {
        throw verificationFailure(error);
      } finally {
        clearInterval(progress);
      }
      signal.throwIfAborted();
      const result = JSON.parse(fs.readFileSync(path.join(folder, 'result.json'), 'utf8'));
      if (!result.ok) {
        if (result.budgetResource) throw new VerificationBudgetError(result.budgetResource, result.requested, request.budgets[result.budgetResource]);
        throw new Error(`Minecraft verification failed: ${result.error}`);
      }
      const reportFile = path.join(folder, 'report.json');
      checkComplete(request, result, reportFile);
      phase('simulationMs');
      const finished = finishedResult({ world, request, result, reportFile, archive: reportPath || path.join(this.root, 'reports', `${crypto.randomUUID()}.json`), startedAt,
        note: 'Observed in an isolated Minecraft world, run by an exact port of the pinned Paper server\'s code. Experiment completion is not a design pass. Initial is before inputs; observations are after inputs at each sample tick.' });
      phase('archiveMs');
      onUpdate({ phase: 'running', tick: result.elapsedTicks, totalTicks: request.duration, message: 'Minecraft observations complete' });
      return { ...finished, timings: { ...timings, ...(this.engine.last ? { broker: this.engine.last } : {}) } };
    } catch (error) {
      if (timeout.aborted && !externalSignal.aborted) throw new WallBudgetError(Date.now() - startedAt, request.budgets.wallMs, { reached });
      throw error;
    } finally {
      signal.removeEventListener('abort', cancel);
      fs.rmSync(folder, { recursive: true, force: true });
    }
  }

  // Starts this verifier's engine process (and builds the program first if
  // needed), once.
  async warm() {
    return this.engine.warm({ folder: path.join(this.root, 'engine') });
  }

  async stop() {
    this.closed = true;
    this.shutdown.abort(new Error('Minecraft verification is shutting down'));
    await this.queue;
    await this.engine.stop();
  }
}

// Removes the run folders in `runs` last changed over `ageMs` ago.
function sweepRuns(runs, ageMs) {
  if (!fs.existsSync(runs)) return true;
  for (const name of fs.readdirSync(runs)) {
    const folder = path.join(runs, name);
    if (Date.now() - fs.statSync(folder).mtimeMs > ageMs) fs.rmSync(folder, { recursive: true, force: true });
  }
  return true;
}

// Tests from chats running at once (a bench's models) go to as many engine
// verifiers as the machine's memory holds beside what else it runs, each with
// its own folder and engine process; one chat's tests still queue in order
// on whichever verifier is least busy when each arrives.
const RESERVED_BYTES = 5 * 2 ** 30; // kept for the rest of the machine
const LANE_BYTES = 2.5 * 2 ** 30; // an engine process at its largest
const MAX_LANES = 3;


class VerifierPool {
  constructor({ root = ROOT, memory = async () => os.totalmem(), makeLane = (laneRoot, options) => new EngineVerifier({ root: laneRoot, ...options }) } = {}) {
    this.root = root;
    this.memory = memory;
    this.makeLane = makeLane;
    this.lanes = [makeLane(root, {})];
    this.sized = null;
  }

  get placementMode() { return this.lanes[0].placementMode; }

  // A batch worker's own verifier, on engines it does not own (the sweep's
  // engine pool), replaces the memory-sized lanes.
  useLanes(lanes) {
    this.lanes = lanes;
    this.sized = Promise.resolve();
  }

  // Batch workers choose the placement mode before any test runs.
  set placementMode(mode) { for (const lane of this.lanes) lane.placementMode = mode; }

  size() {
    this.sized ||= this.memory().then((bytes) => {
      const count = Math.min(MAX_LANES, Math.max(1, Math.floor((bytes - RESERVED_BYTES) / LANE_BYTES)));
      for (let index = this.lanes.length; index < count; index += 1) {
        this.lanes.push(this.makeLane(path.join(this.root, 'lanes', String(index)), { placementMode: this.placementMode }));
      }
    }, (error) => {
      this.sized = null;
      throw error;
    });
    return this.sized;
  }

  // Starts every verifier's engine, so the first tests do not wait for one
  // to boot.
  async warm() {
    await this.size();
    await Promise.all(this.lanes.map((lane) => lane.warm()));
  }

  async run(world, args, options) {
    await this.size();
    const lane = this.lanes.reduce((best, candidate) => (candidate.pending < best.pending ? candidate : best));
    return lane.run(world, args, options);
  }

  async stop() {
    await Promise.all(this.lanes.map((lane) => lane.stop()));
  }
}

const minecraftVerifier = new VerifierPool();
module.exports = { MAX_LANES, minecraftVerifier, MinecraftVerifier, EngineVerifier, VerifierPool, START_REJECTIONS, verificationFailure, TEST_BUILD_TOOL, callTests, validateRequest, compilePlugin, VerificationBudgetError, WallBudgetError };
