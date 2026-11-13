'use strict';

// The engines. The app runs the Rust engine (core/, coreEngine) and nothing
// else: it places builds, ticks them and runs the verifier's experiments,
// and a request it does not model fails with "The engine failed: unported:
// ...". The Java engine (McbenchEngine.java: Mojang's and Paper's own block
// code from the pinned server jar) is the Rust engine's reference, run only
// by the parity scripts, on the JVM (jvmEngine) or compiled ahead of time
// into a native program with GraalVM's native-image (buildEngine), built on
// this machine from the server jar already here, once per jar, engine
// source and GraalVM release.
//
// native-image must know ahead of time what the code reaches through
// reflection and which resources it reads: server/native/<version> holds
// that for each Minecraft version (recorded by scripts/engine-metadata.js),
// and server/native/common what every version shares, chiefly the server
// jar's vanilla data pack (tags, recipes), which Minecraft reads as a file
// system on its own jar and the program carries as resources.

const crypto = require('crypto');
const zlib = require('zlib');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFile, execFileSync, spawn } = require('child_process');
const { promisify } = require('util');
const { writeDimensionPack } = require('./minecraft-dimension');
const { step } = require('./progress');
const setup = require('./setup');
const tools = require('./tools');
const { withLock } = require('./pid-lock');

const execFileAsync = promisify(execFile);
const SOURCES = ['McbenchEngine.java', 'McbenchLevel.java', 'McbenchExperiment.java'].map((name) => path.join(__dirname, name));
const NATIVE = path.join(__dirname, 'native');
// The build's profile (profile-guided optimisation), per Minecraft version.
const PROFILE = 'engine.iprof.gz';
// What the program runs with unless told otherwise (native-image's -R: sets
// a run-time option's default). The garbage collector starts with a heap of
// 512 MB, which makes its young generation (the memory new objects are made
// in) large enough that a collection comes rarely: with the default, a
// placement or a few hundred ticks of a busy build collected every 15 ms or
// so, and the collections cost about 8% of the time. It changes no result,
// only how much memory the program holds on to once it has worked (about
// 450 MB more).
const RUNTIME_DEFAULTS = ['-R:MinHeapSize=536870912'];
// Where built programs are published (scripts/publish-engine.js): assets of
// one release of the project's private repository, named by program.
const RELEASES = { repo: 'adonis-singh/mcbench-lite', tag: 'engines' };

// GraalVM 25 with native-image: MCBENCH_GRAALVM_HOME, or the newest release
// unpacked under ~/.local/graalvm.
function graalvmHome() {
  let home = process.env.MCBENCH_GRAALVM_HOME;
  const unpacked = path.join(os.homedir(), '.local', 'graalvm');
  if (!home && fs.existsSync(unpacked)) {
    const release = fs.readdirSync(unpacked).filter((name) => name.startsWith('graalvm')).sort().at(-1);
    if (release) home = path.join(unpacked, release, process.platform === 'darwin' ? 'Contents/Home' : '');
  }
  if (!home || !fs.existsSync(path.join(home, 'bin', 'native-image'))) {
    throw new Error('The engine is built with GraalVM 25 native-image: unpack it under ~/.local/graalvm or set MCBENCH_GRAALVM_HOME');
  }
  return home;
}

function files(directory) {
  return fs.readdirSync(directory).sort().map((name) => path.join(directory, name));
}

// The engine's classes, compiled by `javac` against the server's jars into
// `classes`.
async function compileEngine(javac, classpath, classes) {
  await execFileAsync(javac, ['--release', '21', '-proc:none', '-XDignore.symbol.file', '-classpath', classpath.join(path.delimiter), '-d', classes, ...SOURCES], { timeout: 120_000 });
}

// Runs native-image in a process group of its own (it starts a builder JVM
// of its own), so `signal` stops the whole build, and logs its output.
function nativeImage(graalvm, args, { cwd, log, signal }) {
  return new Promise((resolve, reject) => {
    signal.throwIfAborted();
    const output = fs.openSync(log, 'w');
    const child = spawn(path.join(graalvm, 'bin', 'native-image'), args, { cwd, detached: true, stdio: ['ignore', output, 'pipe'] });
    fs.closeSync(output);
    let stderr = '';
    child.stderr.on('data', (chunk) => { stderr = `${stderr}${chunk}`.slice(-4000); });
    const stop = () => { try { process.kill(-child.pid, 'SIGKILL'); } catch {} };
    signal.addEventListener('abort', stop, { once: true });
    child.once('error', reject);
    child.once('close', (code, killedBy) => {
      signal.removeEventListener('abort', stop);
      if (signal.aborted) reject(signal.reason);
      else if (code === 0) resolve();
      else reject(new Error(`The engine build failed (${killedBy || `code ${code}`}, see ${log}): ${fs.readFileSync(log, 'utf8').slice(-1500)}${stderr}`));
    });
  });
}

// Compiles the engine for `classpath` (the server's jars, absolute paths)
// into `folder`, unless this exact program is already there or published,
// and returns its path. A native-image build takes minutes and a 12 GB
// heap; `signal` stops it. It is optimised with the version's recorded
// profile; `instrument` builds the program that records a profile instead
// (scripts/engine-metadata.js).
// The program's name: a digest of everything it is built from (the engine's
// sources, the version's metadata and profile, the GraalVM release, the
// server's jars by name and size, which Paper's pinned build fixes, and the
// platform), so one name is one program on any machine.
function engineName({ classpath, minecraftVersion, instrument }) {
  const metadata = path.join(NATIVE, minecraftVersion);
  const digest = crypto.createHash('sha256');
  for (const file of [...SOURCES, ...files(metadata), ...files(path.join(NATIVE, 'common')), path.join(graalvmHome(), 'release')]) digest.update(fs.readFileSync(file));
  for (const jar of classpath) digest.update(`${path.basename(jar)} ${fs.statSync(jar).size}\n`);
  digest.update(`${process.platform} ${process.arch} ${RUNTIME_DEFAULTS.join(' ')}`);
  return `mcbench-engine-${instrument ? 'instrumented-' : ''}${digest.digest('hex').slice(0, 12)}`;
}

// Downloads the program `name` from the project's releases, if it was
// published there, to `binary`, with the GitHub CLI. False when it was not
// published or the CLI is not installed.
async function fetchPublished(name, binary) {
  const download = fs.mkdtempSync(`${binary}-download-`);
  try {
    await execFileAsync('gh', ['release', 'download', RELEASES.tag, '--repo', RELEASES.repo, '--pattern', `${name}.gz`, '--dir', download], { timeout: 600_000 });
    fs.writeFileSync(`${binary}.part`, zlib.gunzipSync(fs.readFileSync(path.join(download, `${name}.gz`))), { mode: 0o755 });
    fs.renameSync(`${binary}.part`, binary);
    return true;
  } catch (error) {
    if (error.code === 'ENOENT' || /no assets match|release not found/i.test(error.stderr || '')) return false;
    throw error;
  } finally {
    fs.rmSync(download, { recursive: true, force: true });
  }
}

// Entries of a program folder that another build may still be writing: a
// download, a compile's classes and native-image's output. They are left
// alone until they are a day old.
const IN_PROGRESS = /^mcbench-engine-.*(?:-download-|\.part$)|^mcbench-engine-classes-/;
const STALE_MS = 24 * 60 * 60 * 1000;

async function compileNative({ classpath, folder, minecraftVersion, signal, instrument = false }) {
  const graalvm = graalvmHome();
  const metadata = path.join(NATIVE, minecraftVersion);
  if (!fs.existsSync(path.join(metadata, instrument ? 'reachability-metadata.json' : PROFILE))) throw new Error(`No engine metadata for Minecraft ${minecraftVersion} (run scripts/engine-metadata.js)`);
  const name = engineName({ classpath, minecraftVersion, instrument });
  const binary = path.join(folder, name);
  if (fs.existsSync(binary)) return binary;
  fs.mkdirSync(folder, { recursive: true });
  // Programs built from other inputs go; another process's build in progress stays.
  for (const entry of fs.readdirSync(folder).filter((entry) => entry.startsWith('mcbench-engine-'))) {
    const file = path.join(folder, entry);
    if (IN_PROGRESS.test(entry) && Date.now() - fs.statSync(file).mtimeMs < STALE_MS) continue;
    fs.rmSync(file, { recursive: true, force: true });
  }
  // A program published for these exact inputs needs no build here.
  if (!instrument && await fetchPublished(name, binary)) return binary;
  const classes = fs.mkdtempSync(path.join(folder, 'mcbench-engine-classes-'));
  await compileEngine(path.join(graalvm, 'bin', 'javac'), classpath, classes);
  const profile = path.join(classes, 'engine.iprof');
  if (!instrument) fs.writeFileSync(profile, zlib.gunzipSync(fs.readFileSync(path.join(metadata, PROFILE))));
  await nativeImage(graalvm, [
    '-cp', [...classpath, classes].join(path.delimiter),
    `-H:ConfigurationFileDirectories=${metadata},${path.join(NATIVE, 'common')}`,
    // Netty's own native-image settings build some of its classes into the
    // image, which pulls in a logger made at run time.
    '--initialize-at-run-time=io.netty',
    // Analysis peaks near 11 GB: a fixed heap, not one sized from whatever
    // memory is free when the build starts.
    '-J-Xmx12g',
    instrument ? '--pgo-instrument' : `--pgo=${profile}`,
    ...RUNTIME_DEFAULTS,
    '--no-fallback', '-o', `${binary}.part`, 'McbenchEngine',
  ], { cwd: folder, log: path.join(folder, 'engine-build.log'), signal });
  fs.renameSync(`${binary}.part`, binary);
  fs.rmSync(classes, { recursive: true, force: true });
  return binary;
}

// One build per program folder, shared by every engine process that runs
// the program: it stops once all of them have stopped waiting for it. A
// caller after that stop starts a new build, once the stopped one ended.
const builds = new Map();
function buildEngine({ classpath, folder, minecraftVersion, signal }) {
  signal.throwIfAborted();
  let build = builds.get(folder);
  if (!build || build.controller.signal.aborted) {
    const controller = new AbortController();
    const previous = build ? build.done.catch(() => {}) : Promise.resolve();
    const next = { controller, waiting: 0 };
    next.done = previous.then(() => compileNative({ classpath, folder, minecraftVersion, signal: controller.signal }))
      .finally(() => { if (builds.get(folder) === next) builds.delete(folder); });
    builds.set(folder, next);
    build = next;
  }
  build.waiting += 1;
  const stopped = () => {
    build.waiting -= 1;
    if (!build.waiting) build.controller.abort(signal.reason);
  };
  signal.addEventListener('abort', stopped, { once: true });
  return new Promise((resolve, reject) => {
    signal.addEventListener('abort', () => reject(signal.reason), { once: true });
    build.done.then(resolve, reject).finally(() => signal.removeEventListener('abort', stopped));
  });
}

// `promise`, or the signal's reason as soon as it aborts.
function untilAborted(promise, signal) {
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason);
    if (signal.aborted) return abort();
    signal.addEventListener('abort', abort, { once: true });
    promise.then(resolve, reject).finally(() => signal.removeEventListener('abort', abort));
  });
}

// An engine process and the requests it answers. `status` is one of:
//   idle      never warmed
//   building  the program is being built (`abort` stops the wait for it)
//   starting  the process runs and has not said it is ready
//   ready     the process answers requests
//   dead      the process exited, or could not be built or started (`error`)
//   stopped   stop() ended it (`closed` resolves once the process exited)
// building, starting and ready are one run: the same object, its `name`
// moving on, holding its process, requests and output. warm() from idle,
// dead or stopped begins a new run; a run that is no longer `status` never
// spawns and never changes `status`.
class Engine {
  constructor({ build = buildEngine, spawnEngine = spawn } = {}) {
    this.build = build;
    this.spawnEngine = spawnEngine;
    this.status = { name: 'idle' };
    this.requestId = 0;
  }

  // The running process, or null.
  get child() {
    return ['starting', 'ready'].includes(this.status.name) ? this.status.child : null;
  }

  // False once the engine died or was stopped: it needs warm() again.
  get alive() {
    return !['dead', 'stopped'].includes(this.status.name);
  }

  // Builds the engine if needed and starts it once: `classpath` is the
  // server's (absolute paths), `binaries` holds the program, shared by every
  // engine process, and `folder` this process's own files (its world).
  warm({ classpath, binaries, folder, minecraftVersion }) {
    if (['building', 'starting', 'ready'].includes(this.status.name)) return this.status.ready;
    const run = { name: 'building', abort: new AbortController(), child: null, requests: new Map(), output: '' };
    run.ready = this.start(run, { classpath, binaries, folder, minecraftVersion }).catch((error) => {
      if (this.status === run) this.status = { name: 'dead', error };
      throw error;
    });
    this.status = run;
    return run.ready;
  }

  async start(run, { classpath, binaries, folder, minecraftVersion }) {
    const binary = await untilAborted(this.build({ classpath, folder: binaries, minecraftVersion, signal: run.abort.signal }), run.abort.signal);
    if (this.status !== run) throw new Error('The engine stopped while it was being built');
    fs.mkdirSync(folder, { recursive: true });
    // The engine's world folder holds the data packs a new world of the
    // live server enables (see McbenchEngine.boot).
    writeDimensionPack(folder);
    const child = this.spawnEngine(binary, [], { cwd: folder, stdio: ['pipe', 'pipe', 'pipe'] });
    run.child = child;
    run.name = 'starting';
    let buffer = '';
    let started;
    const startedPromise = new Promise((resolve, reject) => { started = { resolve, reject }; });
    child.stdout.on('data', (chunk) => {
      buffer += chunk;
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const line of lines) {
        // A run stopped meanwhile fails with its process's exit instead.
        if (line === 'MCBENCH_ENGINE_READY' && this.status === run) {
          run.name = 'ready';
          started.resolve();
        }
        if (line.startsWith('MCBENCH_ENGINE_FAILED ')) run.output = line.slice('MCBENCH_ENGINE_FAILED '.length);
        const done = line.match(/^MCBENCH_(?:PLACED|VERIFIED|TICKED|DONE) (\d+) \d+$/);
        const failed = line.match(/^MCBENCH_(?:PLACE_FAILED|VERIFY_FAILED|TICK_FAILED|FAILED) (\d+) (.*)$/);
        const id = Number((done || failed)?.[1]);
        const request = run.requests.get(id);
        if (!request) continue;
        run.requests.delete(id);
        if (done) request.resolve();
        else request.reject(new Error(`The engine failed: ${failed[2]}`));
      }
    });
    child.stderr.on('data', (chunk) => { run.output = `${run.output}${chunk}`.slice(-8000); });
    child.stdin.on('error', () => {});
    // A process that could not start closes next, which fails its requests.
    child.on('error', (error) => { run.output = `${run.output}${error.message}`; });
    child.once('close', (code, signal) => {
      run.closed = true;
      const error = new Error(`The engine stopped (${signal || `code ${code}`}): ${run.output.slice(-2000)}`);
      if (this.status === run) this.status = { name: 'dead', error };
      started.reject(error);
      for (const request of run.requests.values()) request.reject(error);
      run.requests.clear();
    });
    await startedPromise;
  }

  // Sends `verb` with `args` once the engine is ready. Arguments are words
  // of the protocol, which splits a line on spaces.
  async request(verb, ...args) {
    for (const arg of args) {
      if (!/^\S+$/.test(String(arg))) throw new Error(`An engine request's arguments cannot be empty or hold whitespace: ${JSON.stringify(String(arg))}`);
    }
    if (['building', 'starting'].includes(this.status.name)) await this.status.ready;
    const run = this.status;
    if (run.name !== 'ready') {
      throw new Error({ idle: 'The engine was not started', dead: run.error?.message, stopped: 'The engine was stopped' }[run.name] ?? 'The engine restarted while the request waited');
    }
    const id = ++this.requestId;
    const answered = new Promise((resolve, reject) => run.requests.set(id, { resolve, reject }));
    run.child.stdin.write(`${[verb, id, ...args].join(' ')}\n`);
    return answered;
  }

  // Places the commands in `commandsFile`, on an empty world or (with
  // `fresh` false) on the world the last placement left, and writes the
  // blueprint to `blueprintFile`. The engine must have been warmed.
  place(commandsFile, blueprintFile, { fresh = true } = {}) {
    return this.request('PLACE', commandsFile, blueprintFile, fresh ? 'fresh' : 'continue');
  }

  // Runs a verifier experiment (McbenchExperiment): the plugin's request file,
  // the import's commands, and the folder that gets report.json and
  // result.json.
  verify(requestFile, commandsFile, folder) {
    return this.request('VERIFY', requestFile, commandsFile, folder);
  }

  // Ticks the live world (the one placements build) `ticks` times and
  // writes what changed as a patch to `patchFile`, the bounds and built
  // columns grown or shrunk with it. The entities go to `entitiesFile` (see
  // McbenchEngine.tick). The ticks end by lighting what they changed;
  // `light: false` asks the Rust engine not to, for a caller that reads
  // nothing but blocks afterwards (the light is lit when anything reads it,
  // so the blocks are the same). The Java engine ignores the request's
  // extra word and lights, as the stock request does.
  tick(ticks, patchFile, entitiesFile, { light = true } = {}) {
    return this.request('TICK', ticks, patchFile, entitiesFile, ...(light ? [] : ['nolight']));
  }

  // The creative player of the Rust engine (core/src/player; the Java engine
  // has none): spawned at x, y, z looking as yaw and pitch say, then ticked
  // with the keys held (the protocol's flags: 1 forward, 2 backward, 4 left, 8
  // right, 16 jump, 32 shift, 64 sprint) and the look: the world's tick as
  // `tick` writes it, and the player's view (28 little endian doubles) to
  // `viewFile`.
  spawnPlayer(x, y, z, yaw, pitch, flying) {
    return this.request('PSPAWN', x, y, z, yaw, pitch, flying ? 1 : 0);
  }

  playerTick(keys, yaw, pitch, patchFile, entitiesFile, viewFile) {
    return this.request('PTICK', keys, yaw, pitch, patchFile, entitiesFile, viewFile);
  }

  removePlayer() {
    return this.request('PREMOVE');
  }

  // A player's input to the live world at x, y, z, by a player looking
  // with `yaw` (degrees, Minecraft's: 0 south, 90 west): 'toggle' a lever,
  // 'press' a button, or 'use' a door, trapdoor, fence gate, repeater,
  // comparator, note block, daylight detector, redstone dust or decorated pot
  // with an empty hand.
  input(type, x, y, z, yaw) {
    return this.request('INPUT', type, x, y, z, yaw);
  }

  // Writes what the world holds besides its blocks to `file`
  // (McbenchEngine.state).
  state(file) {
    return this.request('STATE', file);
  }

  // Writes what the Rust engine reads of Minecraft to `file`
  // (McbenchEngine.dump; the Java engine's, for scripts/core-registry.js).
  dump(file) {
    return this.request('DUMP', file);
  }

  // Ends the engine: a build it waits for stops being waited for (it never
  // spawns), a process gets SIGTERM, then SIGKILL after 10 s. Resolves once
  // the process exited; a second stop meanwhile waits for the same exit.
  stop() {
    const run = this.status;
    if (run.name === 'stopped') return run.closed;
    let closed = Promise.resolve();
    if (run.name === 'building') run.abort.abort(new Error('The engine stopped while it was being built'));
    if (['starting', 'ready'].includes(run.name)) closed = this.end(run);
    this.status = { name: 'stopped', closed };
    return closed;
  }

  async end(run) {
    const { child } = run;
    const closed = new Promise((resolve) => child.once('close', resolve));
    child.kill('SIGTERM');
    let timer;
    await Promise.race([closed, new Promise((resolve) => { timer = setTimeout(resolve, 10_000); })]);
    clearTimeout(timer);
    if (!run.closed) {
      child.kill('SIGKILL');
      await closed;
    }
  }
}

// The engine's classes on GraalVM's JVM instead of the native program, for
// scripts: recording native-image's metadata (`jvmArgs` adds the tracing
// agent) and comparing changes without a native build. `classes` is a
// folder to compile into. HotSpot's intrinsics for Math.sin and Math.cos
// differ in the last place from StrictMath's, which the native program
// computes them with (primed TNT's first motion draws on them), so they are
// turned off on aarch64. (On x86-64 HotSpot and the native program both use
// Intel's libm algorithm.) `java` and `javac` are GraalVM's unless named
// (the app's tables are dumped on the plain JDK, minecraft-tables.js).
function jvmEngine({ classpath, classes, jvmArgs = [], env = process.env, java = path.join(graalvmHome(), 'bin', 'java'), javac = path.join(graalvmHome(), 'bin', 'javac') }) {
  return new Engine({
    build: async () => compileEngine(javac, classpath, classes),
    spawnEngine: (binary, args, options) => spawn(java, [...(process.arch === 'arm64' ? ['-XX:+UnlockDiagnosticVMOptions', '-XX:DisableIntrinsic=_dsin,_dcos'] : []), ...jvmArgs, '-cp', [...classpath, classes].join(path.delimiter), 'McbenchEngine'], { ...options, env }),
  });
}

// The engine's environment for MCBENCH_ENGINE_STOCK (see McbenchLevel): "0"
// for the engine's fast paths, "1" for Paper's own code paths only, "check"
// for the fast paths with every fast read compared against Paper's.
function engineEnv(mode) {
  return { ...process.env, MCBENCH_ENGINE_STOCK: mode };
}

// The Rust engine (core/): Minecraft's block-update code ported from the
// pinned server's classes and checked byte for byte against the Java engine
// (scripts/core-parity.js). It speaks the Java engine's protocol for
// placements, ticks and inputs, and refuses ("unported: ...") any request
// that reaches what it does not model. Built with cargo once per process.
const CORE = path.join(__dirname, '..', 'core');

// The Rust that builds core/, natively and as WebAssembly, is the release
// core/rust-toolchain.toml names, with its WebAssembly target, installed by
// the app's own rustup in the tools folder (tools.js), which the first start
// installs.
const RUST_CHANNEL = fs.readFileSync(path.join(CORE, 'rust-toolchain.toml'), 'utf8').match(/^channel = "(.*)"$/m)[1];
// rustup installs a toolchain only when told to (installRust), never as a
// side effect of another command.
const RUSTUP_ENV = { ...process.env, ...tools.RUST_ENV, RUSTUP_AUTO_INSTALL: '0' };

// Whether the pinned toolchain is installed.
async function rustInstalled() {
  try {
    await execFileAsync(tools.RUSTUP, ['which', 'cargo'], { cwd: CORE, env: RUSTUP_ENV });
    return true;
  } catch (error) {
    if (!/is not installed/.test(error.stderr)) throw error;
    return false;
  }
}

// Installs rustup and the pinned toolchain the first time they are needed,
// saying so, in one process at a time (rustup's own installs collide).
let rustInstall = null;
function installRust() {
  rustInstall ||= setup.track('rust', async () => {
    await step(`[rust] downloading rustup ${tools.PINS['rustup-init'].version} (about ${tools.PINS['rustup-init'].mb} MB) into ${tools.TOOLS_DIR}`, '[rust] rustup installed',
      () => tools.installRustup({ onProgress: setup.progress('rust') }));
    if (await rustInstalled()) return;
    await withLock(path.join(tools.TOOLS_DIR, 'rust', 'toolchain-install.lock'), async () => {
      if (await rustInstalled()) return;
      setup.report('rust', { detail: `Installing Rust ${RUST_CHANNEL} and its WebAssembly target with rustup`, progress: undefined });
      await step(`[rust] installing Rust ${RUST_CHANNEL} and its WebAssembly target with rustup (once, about 550 MB on disk)`, `[rust] Rust ${RUST_CHANNEL} installed`,
        () => execFileAsync(tools.RUSTUP, ['install'], { cwd: CORE, env: RUSTUP_ENV, maxBuffer: 1 << 24 }));
    });
  }).catch((error) => {
    rustInstall = null;
    throw error;
  });
  return rustInstall;
}

// The pinned toolchain's cargo and the environment that has it compile with
// the same toolchain's rustc (cargo run by its path would take the first
// rustc on PATH), and its host target. The toolchain must be installed, and
// the tables of Minecraft the engine compiles in made (minecraft-tables.js,
// required here as it requires this module).
let rustTools = null;
function rust() {
  require('./minecraft-tables').checkTables(require('./assets').MC_VERSION);
  if (rustTools) return rustTools;
  const which = (name) => {
    try {
      return execFileSync(tools.RUSTUP, ['which', name], { cwd: CORE, env: RUSTUP_ENV, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim();
    } catch (error) {
      throw new Error(String(error.stderr || error.message).trim());
    }
  };
  const rustc = which('rustc');
  const host = execFileSync(rustc, ['-vV'], { encoding: 'utf8' }).match(/^host: (.*)$/m)[1];
  rustTools = { cargo: which('cargo'), rustc, host, env: { ...RUSTUP_ENV, RUSTC: rustc } };
  return rustTools;
}

// The profile scripts/core-pgo.js records for this machine's target
// (core/pgo/<target>.profdata). A profile describes the code of one target
// only, and core/pgo holds one for aarch64-apple-darwin and one for
// x86_64-unknown-linux-gnu.
function coreProfile() {
  return path.join(CORE, 'pgo', `${rust().host}.profdata`);
}

// cargo's arguments for building the engine: on a target core/pgo has a
// profile for, optimised with it; on any other target (an Intel Mac, ARM
// Linux), built without one. A profile changes how fast the program is,
// never what it does. rustc finds the profile from the directory it runs in,
// which cargo does not keep the same for every run it makes (its probe of the
// compiler runs elsewhere), so the path is given in full.
function coreBuildArgs() {
  const profile = coreProfile();
  return ['build', '--release', '--quiet', ...(fs.existsSync(profile) ? ['--config', `build.rustflags=['-Cprofile-use=${profile}']`] : [])];
}

let coreBuild = null;
function buildCore() {
  coreBuild ||= (async () => {
    await installRust();
    const { cargo, env } = rust();
    await setup.track('engine', () => step('[engine] building the Rust engine (the first time takes a few minutes)', '[engine] Rust engine built',
      () => execFileAsync(cargo, coreBuildArgs(), { cwd: CORE, env, maxBuffer: 1 << 24 })));
    return path.join(CORE, 'target', 'release', 'mcbench-core');
  })().catch((error) => {
    coreBuild = null;
    throw error;
  });
  return coreBuild;
}

// The Rust engine: speaks the Java engine's protocol (Engine) and writes the
// same files. It reads nothing of the server's jars, so warm() needs only
// `folder`.
function coreEngine() {
  return new Engine({ build: buildCore });
}

module.exports = { Engine, buildCore, coreBuildArgs, coreProfile, coreEngine, installRust, rust, CORE, buildEngine, compileEngine, compileNative, engineName, engineEnv, jvmEngine, graalvmHome, NATIVE, PROFILE, RELEASES };
