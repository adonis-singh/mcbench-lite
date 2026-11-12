'use strict';

// A Paper server on this machine, driven over RCON: the reference runtime of
// the Paper verifier (MinecraftVerifier, whose plugin McbenchVerify.java runs
// experiments in it; scripts and calibrations drive it, and
// scripts/ticker-parity.js checks the engine against it). The app's live
// world is not Paper's: it is the engine's, in the browser.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const net = require('net');
const { execFile, spawn } = require('child_process');
const { promisify } = require('util');

const { World, CHUNK_SIZE } = require('./world');
const { RconConnection } = require('./rcon');
const { Y_OFFSET, validateWorldHeight, writeDimensionPack } = require('./minecraft-dimension');
const { importCommands } = require('./live-placement');
const { graalvmHome } = require('./engine');
const { PaperFiles, PAPER } = require('./paper-files');

const execFileAsync = promisify(execFile);
const BIND_HOST = process.env.MCBENCH_MC_BIND || '127.0.0.1';
const RCON_HOST = '127.0.0.1';
const GAME_PORT = Number(process.env.MCBENCH_MC_PORT || 25599);
const MEMORY = process.env.MCBENCH_MC_MEMORY || '4G';
const DATA_DIR = process.env.MCBENCH_DATA_DIR
  ? path.resolve(process.env.MCBENCH_DATA_DIR)
  : path.join(__dirname, '..', 'data');
// AOT caches (Java 25, JEP 483/514), shared by every server with the same
// Java build, JVM options and Paper build: they load and link the server's
// classes from one mapped file, halving its boot.
const AOT_DIR = path.join(DATA_DIR, 'minecraft-aot');
const AOT_KEPT_MS = 14 * 24 * 60 * 60 * 1000;
// Each app process gives the servers it starts a password of its own: RCON's
// port is on loopback, where every local account and process can reach it.
const RCON_PASSWORD = crypto.randomBytes(24).toString('base64url');
// A void world: one layer of air.
const GENERATOR = JSON.stringify({
  layers: [{ block: 'minecraft:air', height: 1 }],
  biome: 'minecraft:plains',
  features: false,
  lakes: false,
});
// How long a server recording an AOT cache gets to exit after SIGTERM (Java
// writes the cache as it exits); any other server gets `stopMs`, a stray one
// STRAY_STOP_MS, before SIGKILL.
const RECORDING_STOP_MS = 120_000;
const STOP_MS = 5_000;
const STRAY_STOP_MS = 30_000;
// The last output of a starting server kept for its error messages.
const OUTPUT_KEPT = 8_000;
const LOGS_KEPT = 10;

class PaperServerCancelled extends Error {}

// Where a run records an AOT cache before it is complete.
function recordingOf(cache) {
  return cache.replace(/\.aot$/, '.recording.aot');
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

// Paper's server: Java from the same GraalVM JDK, in Paper's folder.
function spawnJava(args, { cwd }) {
  return spawn(path.join(graalvmHome(), 'bin', 'java'), args, { cwd, stdio: ['ignore', 'pipe', 'pipe'] });
}

// What identifies the Java build that records and loads an AOT cache.
function javaBuild() {
  return fs.readFileSync(path.join(graalvmHome(), 'release'), 'utf8');
}

// The command line of process `pid`, or null when there is none.
async function commandOf(pid) {
  try {
    return (await execFileAsync('ps', ['-ww', '-p', String(pid), '-o', 'command='])).stdout.trim() || null;
  } catch {
    return null;
  }
}

// A port that is free on `host`.
function freePort(host) {
  return new Promise((resolve, reject) => {
    const probe = net.createServer();
    probe.once('error', reject);
    probe.listen(0, host, () => {
      const { port } = probe.address();
      probe.close(() => resolve(port));
    });
  });
}

// Whether `server` (a child process) has exited.
function hasExited(server) {
  return server.exitCode !== null || server.signalCode !== null;
}

// Resolves true once `server` has exited, or false after `ms` without it.
function exitedWithin(server, ms) {
  if (hasExited(server)) return Promise.resolve(true);
  return new Promise((resolve) => {
    const done = () => { clearTimeout(timer); resolve(true); };
    const timer = setTimeout(() => { server.off('exit', done); resolve(false); }, ms);
    server.once('exit', done);
  });
}

// Moves a run to the state `name` from one of the states `from`.
function move(run, from, name) {
  if (!from.includes(run.name)) throw new Error(`The Paper server is ${run.name} and cannot become ${name}`);
  run.name = name;
}

// A Paper process and the commands it answers. `state.name` is one of
//   stopped   no process: never started, or stopped
//   starting  startServer gets its files and ports ready: nothing is spawned
//   booting   the process (`server`) runs and RCON is not connected yet
//   ready     RCON (`rcon`) is connected: commands reach the server, until
//             that connection closes (a timeout closes it)
//   died      the process exited when nothing here stopped it (`error`): no
//             command reaches it until it is stopped or started again
//   stopping  stop() is ending it (`done` resolves once it is stopped)
// A start is one run: the same object from starting to stopped, its `name`
// moving on, holding its process and the AOT cache it records (`training`,
// or null). Only one run starts at a time, and the next is a new object, so
// a start or command that stop() overtook finds its run stopping or stopped
// and goes no further.
class PaperServer extends PaperFiles {
  constructor({
    spawnServer = spawnJava,
    inspectProcess = commandOf,
    killProcess = (pid, signal) => process.kill(pid, signal),
    findJavaBuild = javaBuild,
    connectRcon = RconConnection.connect,
    wait = sleep,
    bindHost = BIND_HOST,
    gamePort = GAME_PORT,
    memory = MEMORY,
    jvmOptions = '',
    yOffset = Y_OFFSET,
    aotDir = AOT_DIR,
    stopMs = STOP_MS,
    ...files
  } = {}) {
    super(files);
    this.spawnServer = spawnServer;
    this.inspectProcess = inspectProcess;
    this.killProcess = killProcess;
    this.findJavaBuild = findJavaBuild;
    this.connectRcon = connectRcon;
    this.wait = wait;
    this.stopMs = stopMs;
    this.aotDir = path.resolve(aotDir);
    this.bindHost = bindHost;
    this.gamePort = gamePort;
    this.memory = memory;
    this.jvmOptions = jvmOptions;
    this.yOffset = yOffset;
    this.state = { name: 'stopped' };
    this.dimension = null;
    this.loadedColumns = new Map();
    this.appliedWorld = null;
    // Why the last placement failed part-way: what Paper's world holds is
    // unknown until freshWorld.
    this.placementFailed = null;
  }

  // The Paper process, from its spawn until a stop has ended it.
  get server() {
    return ['booting', 'ready', 'died', 'stopping'].includes(this.state.name) ? this.state.server : null;
  }

  // A server that started, has not exited, and holds an open RCON connection.
  get alive() {
    return this.state.name === 'ready' && !this.state.rcon.closed;
  }

  // Work begun in `run` (a start, a command) is cancelled once stop() has
  // overtaken it.
  assertActive(run) {
    if (['stopping', 'stopped'].includes(run.name)) throw new PaperServerCancelled('The Paper server was stopped');
  }

  // Starts a stopped server. One that died is stopped first: its process is
  // gone, the recording and pid file it left are not. A server that is
  // starting, running or being stopped is not started again.
  async startServer() {
    if (this.state.name === 'died') await this.stop();
    if (this.state.name !== 'stopped') throw new Error(`The Paper server is ${this.state.name} and cannot be started`);
    const run = { name: 'starting', server: null, rcon: null, training: null, error: null, done: null };
    this.state = run;
    // A start that fails ends its process; one cancelled by stop() leaves that to stop().
    try {
      await this.launch(run);
    } catch (error) {
      if (!['stopping', 'stopped'].includes(run.name)) await this.stop();
      throw error;
    }
  }

  async launch(run) {
    // A server an earlier run of the app left behind.
    await this.reapStray();
    this.assertActive(run);
    fs.mkdirSync(this.paperDir, { recursive: true });
    // The world goes with its folders: nothing of the last one is placed or
    // force-loaded in the new one.
    for (const name of ['world', 'world_nether', 'world_the_end']) fs.rmSync(path.join(this.paperDir, name), { recursive: true, force: true });
    this.freshWorld(null);
    // Paper archives each run's log; the last few are kept.
    const logs = path.join(this.paperDir, 'logs');
    if (fs.existsSync(logs)) {
      const archived = fs.readdirSync(logs).filter((name) => name.endsWith('.log.gz'))
        .map((name) => ({ file: path.join(logs, name), time: fs.statSync(path.join(logs, name)).mtimeMs }))
        .sort((a, b) => b.time - a.time);
      for (const { file } of archived.slice(LOGS_KEPT)) fs.rmSync(file);
    }
    writeDimensionPack(this.paperDir);
    const classpath = await this.ensurePaper();
    this.assertActive(run);
    const gamePort = this.gamePort || await freePort(this.bindHost);
    const rconPort = await freePort(RCON_HOST);
    this.assertActive(run);
    this.writeServerFiles({ gamePort, rconPort });
    const jvm = [`-Xms${this.memory}`, `-Xmx${this.memory}`, ...this.jvmOptions.split(/\s+/).filter(Boolean)];
    const cache = this.aotCache(jvm);
    // Without a cache for this exact JVM and classpath, this run records
    // one, written when the server stops gracefully (see end).
    const aot = fs.existsSync(cache)
      ? ['-XX:AOTMode=on', `-XX:AOTCache=${cache}`]
      : [`-XX:AOTCacheOutput=${recordingOf(cache)}`];
    run.training = fs.existsSync(cache) ? null : cache;
    // A run killed before it finished writing leaves a partial recording.
    if (run.training) fs.rmSync(recordingOf(cache), { force: true });
    else fs.utimesSync(cache, new Date(), new Date()); // kept while in use
    // The folder on the command line names the process to reapStray.
    const server = this.spawnServer([
      ...jvm, ...aot, `-Dmcbench.server=${this.paperDir}`, '-cp', classpath.join(path.delimiter), 'org.bukkit.craftbukkit.Main', '--nogui',
    ], { cwd: this.paperDir });
    run.server = server;
    move(run, ['starting'], 'booting');
    fs.writeFileSync(this.pidFile, String(server.pid));
    let output = '';
    let failure = null;
    const collect = (chunk) => { output = `${output}${chunk}`.slice(-OUTPUT_KEPT); };
    server.stdout.on('data', collect);
    server.stderr.on('data', collect);
    server.once('error', (error) => { failure = error; });
    // An exit no stop asked for (a crash) leaves the server died.
    server.once('exit', (code, signal) => {
      if (!['booting', 'ready'].includes(run.name)) return;
      run.error = new Error(`The Minecraft server exited (${signal || `code ${code}`}): ${output.slice(-2000)}`);
      run.rcon?.close();
      move(run, ['booting', 'ready'], 'died');
    });
    await this.boot(run, rconPort, () => output, () => failure);
  }

  async boot(run, rconPort, output, failure) {
    const { server } = run;
    const deadline = Date.now() + 300_000;
    while (Date.now() < deadline) {
      this.assertActive(run);
      if (failure()) throw new Error(`Minecraft server could not start: ${failure().message}`);
      if (/Done \([0-9.]+s\)!/.test(output())) break;
      if (/You need to agree to the EULA/i.test(output())) throw new Error('Minecraft server rejected the EULA setting');
      if (hasExited(server)) throw new Error(`Minecraft server exited during startup (${server.signalCode || `code ${server.exitCode}`}): ${output()}`);
      await this.wait(100);
    }
    if (Date.now() >= deadline) throw new Error('Minecraft server did not finish starting within five minutes');

    const rcon = await this.connectRcon(RCON_HOST, rconPort, RCON_PASSWORD, 10_000);
    if (run.name !== 'booting') {
      rcon.close();
      this.assertActive(run);
      throw run.error;
    }
    run.rcon = rcon;
    move(run, ['booting'], 'ready');
    for (const command of [
      'defaultgamemode spectator',
      'gamemode spectator @a',
      'difficulty peaceful',
      'gamerule spawnRadius 0',
      'gamerule spawnChunkRadius 0',
      'gamerule doMobSpawning false',
      'gamerule doDaylightCycle false',
      'gamerule doWeatherCycle false',
      'gamerule doFireTick false',
      'gamerule randomTickSpeed 0',
      'gamerule keepInventory true',
      'time set noon',
      'weather clear',
      'save-off',
    ]) await this.command(command);
  }

  // The file holding the Paper process's id, for reapStray.
  get pidFile() {
    return path.join(this.paperDir, 'mcbench-server.pid');
  }

  // One cache per Java build, JVM options and Paper build. Caches unused for two
  // weeks, and recordings a killed run left, are removed.
  aotCache(jvm) {
    const hash = crypto.createHash('sha256').update(JSON.stringify([this.findJavaBuild(), jvm, PAPER[this.minecraftVersion].sha256])).digest('hex').slice(0, 16);
    fs.mkdirSync(this.aotDir, { recursive: true });
    for (const name of fs.readdirSync(this.aotDir)) {
      const file = path.join(this.aotDir, name);
      const age = Date.now() - fs.statSync(file).mtimeMs;
      if (name.endsWith('.recording.aot') ? age > 24 * 60 * 60 * 1000 : age > AOT_KEPT_MS) fs.rmSync(file, { force: true });
    }
    return path.join(this.aotDir, `${hash}.aot`);
  }

  // Ends a run's Paper process: SIGTERM, then SIGKILL if it has not exited in
  // time. A run recording the AOT cache writes it as Java exits, so it gets a
  // long graceful stop; the cache is used once the write finished, which a
  // clean exit (0, or 143 for Java's SIGTERM) says, and no crash or kill does.
  async end(run) {
    const { server, training } = run;
    if (!server) return;
    server.kill('SIGTERM');
    if (!await exitedWithin(server, training ? RECORDING_STOP_MS : this.stopMs)) {
      server.kill('SIGKILL');
      await exitedWithin(server, 60_000);
    }
    const clean = server.signalCode === null && [0, 143].includes(server.exitCode);
    if (training && clean && fs.existsSync(recordingOf(training))) fs.renameSync(recordingOf(training), training);
    fs.rmSync(this.pidFile, { force: true });
  }

  // The settings the server boots with. Paper adds its defaults for the rest.
  writeServerFiles({ gamePort, rconPort }) {
    const properties = {
      'allow-flight': 'true',
      difficulty: 'peaceful',
      'enable-rcon': 'true',
      'force-gamemode': 'true',
      gamemode: 'spectator',
      'generate-structures': 'false',
      'generator-settings': GENERATOR,
      'level-type': 'flat',
      motd: 'mcbench-lite reference server',
      'online-mode': 'false',
      'rcon.password': RCON_PASSWORD,
      'rcon.port': String(rconPort),
      'server-ip': this.bindHost,
      'server-port': String(gamePort),
      'simulation-distance': '10',
      'spawn-protection': '0',
      'view-distance': '32',
    };
    fs.writeFileSync(path.join(this.paperDir, 'server.properties'),
      `${Object.entries(properties).map(([key, value]) => `${key}=${value}`).join('\n')}\n`);
    fs.writeFileSync(path.join(this.paperDir, 'eula.txt'), 'eula=true\n');
    fs.writeFileSync(path.join(this.paperDir, 'bukkit.yml'), 'settings:\n  allow-end: false\n');
  }

  // A new world for applyWorld to place into (the verifier's fresh dimension):
  // nothing of it is placed or force-loaded yet.
  freshWorld(dimension) {
    this.dimension = dimension;
    this.appliedWorld = null;
    this.loadedColumns.clear();
    this.placementFailed = null;
  }

  async applyWorld(world, { resumeTicks = true } = {}) {
    const run = this.state;
    if (run.name === 'stopping') throw new PaperServerCancelled('The Paper server was stopped');
    if (this.placementFailed) throw new Error(`An earlier placement failed part-way, so what Paper's world holds is unknown: ${this.placementFailed.message}`);
    validateWorldHeight(world, this.yOffset);
    const { keys, commands } = importCommands(world, this.appliedWorld, this.yOffset);
    const total = commands.length;
    let applied = 0;
    this.progress = { applied, total };

    // Keep the complete circuit and a one-chunk margin ticking, including
    // unchanged columns. The verifier adds extra probe/input tickets itself.
    const wanted = new Map();
    for (const key of world.chunks.keys()) {
      const [cx, , cz] = World.coordsOf(key);
      for (let dx = -1; dx <= 1; dx++) for (let dz = -1; dz <= 1; dz++) {
        wanted.set(`${cx + dx},${cz + dz}`, [(cx + dx) * CHUNK_SIZE, (cz + dz) * CHUNK_SIZE]);
      }
    }
    // Include removed columns until placement finishes.
    const during = new Map([...this.loadedColumns, ...wanted]);
    await this.command('tick freeze');
    // Commands travel in batch files the plugin runs in order, one RCON
    // round trip per batch.
    const runBatch = async (lines) => {
      const directory = path.join(this.paperDir, 'plugins', this.placementBatch.directory);
      fs.mkdirSync(directory, { recursive: true });
      // A fresh file per batch.
      const name = `batch-${crypto.randomUUID()}.txt`;
      fs.writeFileSync(path.join(directory, name), lines.join('\n'));
      const output = await this.command(`${this.placementBatch.command} ${name}`);
      if (!output.includes(`MCBENCH_APPLIED ${lines.length}`)) throw new Error(`Minecraft placement failed: ${output}`);
    };
    try {
      for (const [key, [x, z]] of during) {
        if (!this.loadedColumns.has(key) && !this.preloadedColumns?.has(key)) {
          await this.command(`forceload add ${x} ${z}`);
          this.loadedColumns.set(key, [x, z]);
        }
      }
      const batch = [];
      const flush = async () => {
        if (!batch.length) return;
        await runBatch(batch);
        applied += batch.length;
        this.progress = { applied, total };
        batch.length = 0;
      };
      for (const command of commands) {
        if (this.placementBatch) {
          batch.push(command);
          if (batch.length === 512) await flush();
        } else {
          await this.command(command);
          applied++;
          this.progress = { applied, total };
        }
      }
      await flush();
      for (const [key, [x, z]] of this.loadedColumns) {
        if (wanted.has(key)) continue;
        await this.command(`forceload remove ${x} ${z}`);
        this.loadedColumns.delete(key);
      }
    } catch (error) {
      this.appliedWorld = null;
      this.placementFailed = error;
      throw error;
    } finally {
      if (resumeTicks && this.state === run && this.alive) await this.command('tick unfreeze');
    }
    this.appliedWorld = world;
    return keys;
  }

  async command(command, check = true, timeoutMs = 60_000) {
    const run = this.state;
    if (run.name === 'stopping') throw new PaperServerCancelled('The Paper server was stopped');
    if (run.name === 'died') throw run.error;
    // A connection that failed (a timeout closes it) is not used again.
    if (run.name !== 'ready' || run.rcon.closed) throw new Error('Minecraft RCON is not connected');
    let output;
    try {
      const scoped = this.dimension && !command.startsWith('mcbenchverify ')
        ? `execute in ${this.dimension} run ${command}` : command;
      output = await run.rcon.exec(scoped, timeoutMs);
    } catch (error) {
      this.assertActive(run);
      throw error;
    }
    this.assertActive(run);
    if (check && /(?:Unknown or incomplete command|Cannot place blocks outside of the world|That position is not loaded)/i.test(output)) {
      throw new Error(`Minecraft rejected "${command}": ${output}`);
    }
    return output;
  }

  // Stops the server, and whatever is starting it: a start begun before
  // never spawns or continues once this resolves. A stop during another
  // waits for the same end.
  async stop() {
    const run = this.state;
    if (run.name === 'stopped') return;
    if (run.name !== 'stopping') {
      move(run, ['starting', 'booting', 'ready', 'died'], 'stopping');
      run.rcon?.close();
      run.done = this.end(run).finally(() => {
        move(run, ['stopping'], 'stopped');
        this.freshWorld(null);
      });
    }
    await run.done;
  }

  // What an earlier run of the app left: its Paper process, found by the pid
  // it recorded and ended only if that pid is still a java process of this
  // folder.
  async reapStray() {
    if (!fs.existsSync(this.pidFile)) return;
    const pid = Number(fs.readFileSync(this.pidFile, 'utf8'));
    const isPaper = async () => {
      const command = await this.inspectProcess(pid);
      return /(?:^|\/)java\s/.test(command || '') && command.includes(` -Dmcbench.server=${this.paperDir} `);
    };
    if (await isPaper()) {
      this.killProcess(pid, 'SIGTERM');
      for (const deadline = Date.now() + STRAY_STOP_MS; await isPaper();) {
        if (Date.now() >= deadline) {
          this.killProcess(pid, 'SIGKILL');
          break;
        }
        await this.wait(100);
      }
    }
    fs.rmSync(this.pidFile, { force: true });
  }
}

module.exports = { PaperServer, PaperServerCancelled, freePort };
