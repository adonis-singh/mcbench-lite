'use strict';

// Build programs run in a fresh, single-use OS process per call. node:vm alone
// is not a security boundary, so the process is. On Linux it runs under
// bubblewrap in new user, mount, PID, network, IPC, UTS and cgroup namespaces
// (no network stack, cannot see or signal host processes) whose root holds
// only read-only system files, the Node binary and its sources, with an
// RLIMIT_DATA cap and no core dumps; on macOS it runs under a seatbelt
// profile (sandbox-exec) that denies everything but reading those same files,
// executing the Node binary and writing /dev/null, so it has no network and
// cannot write or exec anything else. On both it runs under Node's
// permission model with read access to only its own five source files and no
// write, child-process, worker, addon, WASI, inspector, or network grants,
// with an empty environment and a heap cap. The only channel out is its stdio
// pipes: the parent serves mc.readFile through the symlink-safe /work reader
// and treats everything the child returns as untrusted input.

const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');

const { createBlockNormalizer } = require('./mc-dsl');
const { encodeWorld, applyChanges } = require('./build-transfer');

const READ = 1;
const DONE = 2;
const FAIL = 3;
const SETTLED = 4;
const SCRIPT_TIMEOUT_MS = 5000;
const WALL_TIMEOUT_MS = SCRIPT_TIMEOUT_MS + 5000;
// The V8 old-space cap for one build program, an operator budget like the
// verification limits. Linux additionally caps the whole data segment at the
// heap plus 2 GiB for typed-array backing stores and V8's own overhead.
const HEAP_LIMIT_MB = Number(process.env.MCBENCH_BUILD_HEAP_MB || 2048);
if (!Number.isInteger(HEAP_LIMIT_MB) || HEAP_LIMIT_MB < 256) throw new Error(`MCBENCH_BUILD_HEAP_MB must be an integer of at least 256, not ${process.env.MCBENCH_BUILD_HEAP_MB}`);
const DATA_LIMIT_BYTES = (HEAP_LIMIT_MB + 2048) * 1024 ** 2;
const MAX_RESULT_BYTES = 256 * 1024 * 1024;
const MAX_STDERR_BYTES = 16 * 1024;
const MAX_LOG_ENTRIES = 65; // 64 entries plus the limit notice
const MAX_LOG_ENTRY_CHARS = 300 + '...(truncated)'.length;
const MAX_WRITE_FILES = 16;
const MAX_WRITE_TOTAL = 32 * 1024 * 1024;

const SOURCES = ['build-child.js', 'mc-dsl.js', 'world.js', 'minecraft-dimension.js', 'build-transfer.js']
  .map((name) => path.join(__dirname, name));
const NODE_ARGS = [
  // node:sqlite opens files outside the permission model's read grant, so it is off.
  process.execPath, '--permission', '--no-experimental-sqlite', ...SOURCES.map((file) => `--allow-fs-read=${file}`),
  `--max-old-space-size=${HEAP_LIMIT_MB}`,
  SOURCES[0],
];
// The process reads the same files at the OS layer as Node's permission
// model grants it, plus what Node itself loads: its binary, the system's
// libraries and devices, and the listing of / (Node reads it at startup).
// Nothing in a home folder (keys, the app's settings, other chats) is
// readable even if the permission model fails; metadata (stat) stays
// readable, as Node's module loader walks the source's folders.
const SEATBELT_PROFILE = [
  '(version 1)',
  '(deny default)',
  `(allow process-exec (literal ${JSON.stringify(process.execPath)}))`,
  '(allow file-read-metadata)',
  `(allow file-read* (literal "/") ${[
    ...['/System', '/usr/lib', '/usr/share', '/private/var/db/timezone', '/dev'].map((dir) => `(subpath ${JSON.stringify(dir)})`),
    ...[...new Set([process.execPath, fs.realpathSync(process.execPath)])].map((file) => `(literal ${JSON.stringify(file)})`),
    ...SOURCES.map((file) => `(literal ${JSON.stringify(file)})`),
  ].join(' ')})`,
  '(allow file-write* (literal "/dev/null"))',
  '(allow sysctl-read)',
  '(deny network*)',
].join('\n');
// The Linux process's root is an empty tmpfs holding, read-only, what Node
// loads (/usr, the dynamic loader's cache, its binary), the build sources and
// the timezone (as on macOS, so Date formats as it does in the server), plus
// its own /proc and a minimal /dev. /lib, /lib64, /bin and /sbin are as the
// host has them: symlinks into /usr on merged-/usr systems, binds otherwise.
// Nothing else of /home, /root, /etc, /var or /tmp exists there, and nothing
// is writable but the devices (the root and /dev are remounted read-only).
const hostEntry = (file) => {
  const stat = fs.lstatSync(file, { throwIfNoEntry: false });
  if (!stat) return [];
  return stat.isSymbolicLink() ? ['--symlink', fs.readlinkSync(file), file] : ['--ro-bind', file, file];
};
const LAUNCHERS = {
  linux: () => ({
    command: '/usr/bin/prlimit',
    args: [`--data=${DATA_LIMIT_BYTES}`, '--core=0', '--', '/usr/bin/bwrap',
      '--unshare-user', '--unshare-ipc', '--unshare-pid', '--unshare-net', '--unshare-uts', '--unshare-cgroup',
      '--die-with-parent', '--new-session', '--as-pid-1',
      '--ro-bind', '/usr', '/usr',
      ...['/lib', '/lib64', '/bin', '/sbin', '/etc/ld.so.cache', '/etc/localtime'].flatMap(hostEntry),
      '--ro-bind', fs.realpathSync(process.execPath), process.execPath,
      ...SOURCES.flatMap((file) => ['--ro-bind', file, file]),
      '--proc', '/proc', '--dev', '/dev', '--remount-ro', '/dev', '--remount-ro', '/', '--chdir', '/',
      '--', ...NODE_ARGS],
    spareDuringBuild: true,
  }),
  darwin: () => ({ command: '/usr/bin/sandbox-exec', args: ['-p', SEATBELT_PROFILE, ...NODE_ARGS], spareDuringBuild: false }),
};
if (!LAUNCHERS[process.platform]) throw new Error(`Build programs run only on Linux (bwrap, prlimit) or macOS (sandbox-exec), not ${process.platform}`);
const { command: COMMAND, args: ARGS, spareDuringBuild: SPARE_DURING_BUILD } = LAUNCHERS[process.platform]();
// World-height validation runs in the child and must see the parent's offset.
const ENV = process.env.MCBENCH_MC_Y_OFFSET === undefined ? {} : { MCBENCH_MC_Y_OFFSET: process.env.MCBENCH_MC_Y_OFFSET };

// A program failure rebuilt from the child. formatProgramError reads the stack
// when there is one and otherwise String(err), which is the thrown value's own
// string form.
class ProgramError extends Error {
  constructor({ stack, message, logs }) {
    super(message);
    this.stack = stack;
    if (logs) this.logs = logs;
  }

  toString() {
    return this.stack ? super.toString() : this.message;
  }
}

function frame(payload) {
  const head = Buffer.allocUnsafe(4);
  head.writeUInt32LE(payload.length, 0);
  return Buffer.concat([head, payload]);
}

const isStringArray = (value) => Array.isArray(value) && value.every((item) => typeof item === 'string');

function checkLogs(logs) {
  if (!isStringArray(logs) || logs.length > MAX_LOG_ENTRIES || logs.some((entry) => entry.length > MAX_LOG_ENTRY_CHARS)) {
    throw new RangeError('invalid logs');
  }
  return logs;
}

// The child's result is untrusted: accept only what the mc API could produce.
function checkResult({ logs, files, touched }) {
  if (!Array.isArray(files) || files.length > MAX_WRITE_FILES
    || files.some((file) => typeof file?.path !== 'string' || !file.path.trim() || typeof file.text !== 'string')
    || files.reduce((total, file) => total + file.text.length, 0) > MAX_WRITE_TOTAL
    || !Number.isSafeInteger(touched)) {
    throw new RangeError('invalid result');
  }
  return { logs: checkLogs(logs), files: files.map(({ path: relPath, text }) => ({ path: relPath, text })), touched };
}

// Each process runs exactly one program. The first is started when this
// module loads and each next one, on Linux, once a build's whole request is
// written to its process, and on macOS as soon as the build settles, so its
// ~12 ms of startup overlaps the server's start, the build's run or the
// model's next turn. On Linux the spawn then leaves the build's path and a
// build that follows straight on finds the process ready, which hides
// bwrap's ~0.7 ms of extra startup; on macOS a process starting during a
// build slows that build (about 2 ms on a two-million-block world).
// Unreferenced, a spare never keeps the parent alive, and it exits on its
// own when the parent's end of stdin closes.
function launch() {
  // Started in /, whose listing is all it may read there; the server's own
  // folder would be read (and refused) at startup.
  const child = spawn(COMMAND, ARGS, { stdio: ['pipe', 'pipe', 'pipe'], env: ENV, cwd: '/' });
  child.on('error', () => {});
  child.stdin.on('error', () => {});
  child.unref();
  for (const stream of child.stdio) stream.unref();
  return child;
}
let spare = launch();

const alive = (child) => child.pid !== undefined && child.exitCode === null && child.signalCode === null;

function takeProcess() {
  const child = spare && alive(spare) ? spare : launch();
  spare = null;
  child.ref();
  for (const stream of child.stdio) stream.ref();
  return child;
}

function crashError(stderr) {
  const text = Buffer.concat(stderr).toString('utf8');
  if (/heap out of memory|Allocation failed/.test(text)) {
    return new Error(`Program ran out of memory (limit ${HEAP_LIMIT_MB} MB) and was terminated`);
  }
  const detail = text.trim().split('\n').at(-1);
  return new Error(`Program process exited unexpectedly${detail ? `: ${detail}` : ''}`);
}

// Runs a build program in an isolated process against `world` (a fresh clone
// of the pre-build world). On success the clone is left exactly as an
// in-process run would leave it and { logs, files, touched } is returned; on
// failure the clone is untouched and the error carries the program's logs.
// `minecraft`, when given, is { world: the World Minecraft made of the pre-build
// world, which the program reads with mc.settled; it crosses to the process
// only if the program asks for it, ticks: the ticks it ran }.
function runProgram(source, world, blockRegistry, readFile, minecraft) {
  const paletteStart = world.palette.entries.length;
  const request = Buffer.from(JSON.stringify({ source, registry: blockRegistry, readFile: typeof readFile === 'function', settledTicks: minecraft ? minecraft.ticks : null }), 'utf8');
  const worldBuffer = encodeWorld(world);
  return new Promise((resolve, reject) => {
    const child = takeProcess();
    const chunks = [];
    const stderr = [];
    let buffered = 0;
    let stderrBytes = 0;
    let settled = false;
    const timer = setTimeout(() => {
      finish(new Error(`Program process exceeded ${WALL_TIMEOUT_MS} ms and was terminated`));
    }, WALL_TIMEOUT_MS);

    function finish(error, value) {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      child.kill('SIGKILL');
      spare ||= launch();
      if (error) reject(error);
      else resolve(value);
    }

    function handle(type, payload) {
      if (type === READ && typeof readFile === 'function') {
        let ok = 1;
        let text;
        try { text = readFile(payload.toString('utf8')); } catch (error) { ok = 0; text = error.message; }
        child.stdin.write(Buffer.concat([Buffer.from([ok]), frame(Buffer.from(text, 'utf8'))]));
        return;
      }
      if (type === SETTLED && minecraft) {
        child.stdin.write(frame(encodeWorld(minecraft.world)));
        return;
      }
      if (type === FAIL) {
        const failure = JSON.parse(payload.toString('utf8'));
        if (typeof failure.stack !== 'string' || typeof failure.message !== 'string') throw new RangeError('invalid failure');
        finish(new ProgramError({ stack: failure.stack, message: failure.message, logs: failure.logs === null ? null : checkLogs(failure.logs) }));
        return;
      }
      if (type !== DONE) throw new RangeError(`unexpected frame ${type}`);
      const metaLength = payload.readUInt32LE(0);
      const result = checkResult(JSON.parse(payload.toString('utf8', 4, 4 + metaLength)));
      applyChanges(world, payload.subarray(4 + metaLength), paletteStart, createBlockNormalizer(blockRegistry));
      finish(null, result);
    }

    // Frames are [type u8][length u32][payload]; the buffered chunks are joined
    // only once a whole frame has arrived.
    child.stdout.on('data', (data) => {
      if (settled) return;
      chunks.push(data);
      buffered += data.length;
      try {
        if (buffered > MAX_RESULT_BYTES) throw new RangeError('result too large');
        while (buffered >= 5 && !settled) {
          if (chunks[0].length < 5) chunks.splice(0, chunks.length, Buffer.concat(chunks));
          const length = chunks[0].readUInt32LE(1);
          if (buffered < 5 + length) break;
          const all = chunks.length === 1 ? chunks[0] : Buffer.concat(chunks);
          chunks.splice(0, chunks.length, all.subarray(5 + length));
          buffered -= 5 + length;
          handle(all.readUInt8(0), all.subarray(5, 5 + length));
        }
      } catch (error) {
        finish(new Error(`Program process returned a malformed result: ${error.message}`));
      }
    });
    child.stderr.on('data', (data) => {
      if (stderrBytes >= MAX_STDERR_BYTES) return;
      stderr.push(data);
      stderrBytes += data.length;
    });
    child.on('error', (error) => finish(error));
    child.on('close', () => finish(crashError(stderr)));
    const worldLength = Buffer.allocUnsafe(4);
    worldLength.writeUInt32LE(worldBuffer.length, 0);
    child.stdin.write(frame(request));
    child.stdin.write(worldLength);
    child.stdin.write(worldBuffer, () => { if (SPARE_DURING_BUILD) spare ||= launch(); });
  });
}

module.exports = { runProgram, WALL_TIMEOUT_MS, _internal: { COMMAND, ARGS, ENV, NODE_ARGS } };
