'use strict';

const { spawn } = require('child_process');
const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const util = require('util');
const storage = require('./storage');
const setup = require('./setup');
const { step } = require('./progress');

// Per-conversation Linux sandbox, modelled on Codex's exec tooling: a Docker
// container with a bind-mounted /work directory, a PERSISTENT bash
// session per conversation (unified_exec-style: cwd, env vars, and background
// jobs survive across calls; a timeout resets the session), an opt-in
// networked one-shot mode in a second container, and codex-style output
// formatting (exit code, wall time, middle-truncated output). Both containers
// stay up between calls; a reset ends every process in one instead of
// recreating it.

const DOCKER = 'docker';
// The two ways Docker can be missing, as the model's bash result and the
// server log say them. A daemon that is not running is told apart by the
// CLI's own words (older CLIs say the first, newer ones the second).
const DOCKER_MISSING = 'The model\'s bash shell runs in a Docker container (on Linux, Codex CLI does too), and Docker is not installed: install Docker Desktop (https://www.docker.com/products/docker-desktop/) on a Mac or Docker Engine (https://docs.docker.com/engine/install/) on Linux, so that `docker version` works, then try again';
const DOCKER_NOT_RUNNING = 'The model\'s bash shell runs in a Docker container (on Linux, Codex CLI does too), and the Docker daemon is not running: open Docker Desktop on a Mac, or start Docker Engine on Linux (`sudo systemctl start docker`), so that `docker version` works, then try again';
const DAEMON_UNREACHABLE = /Cannot connect to the Docker daemon|failed to connect to the docker API/;
const IMAGE_VERSION = crypto.createHash('sha256').update(fs.readFileSync(path.join(__dirname, '..', 'sandbox', 'Dockerfile'))).digest('hex').slice(0, 12);
const IMAGE = `localhost/mcbench-sandbox:${IMAGE_VERSION}`;
const DOCKERFILE_DIR = path.join(__dirname, '..', 'sandbox');
const ROOT = path.join(storage.DATA_DIR, 'sandbox');

const DEFAULT_TIMEOUT_MS = 10_000;
const MAX_TIMEOUT_MS = 120_000;
const MAX_OUTPUT_BYTES = 20_000; // model-facing budget; middle-truncated beyond this
const MAX_COLLECT_BYTES = 8 * 1024 * 1024; // runaway guard while collecting
const IDLE_MS = 10 * 60 * 1000;

// Each chat's sandbox: its persistent shell (`session`: { child, collector },
// or null), when it was last used (`lastUsed`, epoch ms), and `state`, whose
// `name` is one of
//   ready       commands run, and the storage watchdog may end its processes
//   restricted  its command with file writes disabled is running, which is
//               how the chat gets back under its limits: the watchdog leaves
//               its processes alone
//   halted      the watchdog ended its processes (`ended` resolves once they
//               are gone); the next result of a command says why (`reason`)
//   removing    its containers are being removed (`removed` resolves once
//               they are gone and the sandbox is forgotten): a command that
//               arrives meanwhile waits, then starts a fresh one
// A chat with no sandbox here has no container this process knows of.
const sandboxes = new Map(); // conversationId -> sandbox

function track(conversationId) {
  const box = { id: conversationId, state: { name: 'ready' }, session: null, lastUsed: Date.now() };
  sandboxes.set(conversationId, box);
  return box;
}

// The chat's sandbox, used now. One being removed is waited for.
async function use(conversationId) {
  for (let box; (box = sandboxes.get(conversationId))?.state.name === 'removing';) await box.state.removed;
  const box = sandboxes.get(conversationId) ?? track(conversationId);
  box.lastUsed = Date.now();
  return box;
}

// Moves a sandbox to the state `next` from one of the states `from`.
function move(box, from, next) {
  if (!from.includes(box.state.name)) throw new Error(`The sandbox of ${box.id} is ${box.state.name} and cannot become ${next.name}`);
  box.state = next;
}

// The chat app offers one-shot networked commands; benchmark workers disable
// them for the whole process before any tool schema is built.
let networkEnabled = true;
const networkAccess = () => networkEnabled;
function disableNetwork() { networkEnabled = false; }

// A recovered/forked conversation can have a different /work root while
// retaining its ID. Never attach its shell to another root's container.
const SCOPE = crypto.createHash('sha256').update(ROOT).digest('hex').slice(0, 12);
const CONTAINER = new RegExp(`^mcb-(?:v3|net)-${SCOPE}-(.+)$`);

function containerName(conversationId, { network = false } = {}) {
  return `mcb-${network ? 'net' : 'v3'}-${SCOPE}-${conversationId}`;
}

// The model can put a file where images/ or attachments/ should be.
function workDir(conversationId) {
  const dir = path.join(ROOT, conversationId);
  for (const folder of ['images', 'attachments']) {
    try { fs.mkdirSync(path.join(dir, folder), { recursive: true }); } catch (error) { throw workError(error, folder); }
  }
  return dir;
}

// Node's file errors name the host path, which holds the data folder and so
// the user's name, and these errors reach the model as tool results. A file
// error is told instead with the /work path the model gave and what its code
// means: "/work/f.txt/g: not a directory (ENOTDIR)". The bridge's own errors
// (no syscall) already name /work paths only.
const workPath = (relPath) => `/work/${String(relPath).replace(/^\/?work(?:\/|$)/, '')}`;
function workError(error, relPath) {
  if (!error.syscall) return error;
  return new Error(`${workPath(relPath)}: ${util.getSystemErrorMessage(error.errno)} (${error.code})`);
}

// A host-side file function of (conversationId, relPath, ...), with its file
// errors told as workError tells them.
const inWork = (run) => (conversationId, relPath, ...rest) => {
  try { return run(conversationId, relPath, ...rest); } catch (error) { throw workError(error, relPath); }
};

// Runs a one-shot Docker CLI invocation, capturing stdout+stderr interleaved
// in arrival order (the model sees one stream, like a terminal). A checked
// call fails with DOCKER_MISSING or DOCKER_NOT_RUNNING when Docker cannot be
// reached; an unchecked one runs a model's command, whose output is its own.
function docker(args, { timeoutMs = 60_000, check = true, binary = false } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(DOCKER, args, { timeout: timeoutMs, killSignal: 'SIGKILL' });
    const chunks = [];
    const stdout = [];
    // Text output is capped like the persistent shell's; past it the command is killed.
    let collected = 0;
    const collect = (fromStdout) => (d) => {
      collected += d.length;
      if (!binary && collected > MAX_COLLECT_BYTES) {
        if (child.exitCode === null) child.kill('SIGKILL');
        return;
      }
      chunks.push(d);
      if (binary && fromStdout) stdout.push(d);
    };
    child.stdout.on('data', collect(true));
    child.stderr.on('data', collect(false));
    child.on('error', (err) => reject(new Error(err.code === 'ENOENT' ? DOCKER_MISSING : `Docker sandbox failed to start: ${err.message}`)));
    child.on('close', (code, sig) => {
      if (check && code !== 0) {
        const text = Buffer.concat(chunks).toString('utf8');
        return reject(new Error(DAEMON_UNREACHABLE.test(text) ? DOCKER_NOT_RUNNING : `Docker sandbox ${args[0]} failed (${sig || code}):\n${text.slice(-2000)}`));
      }
      const output = binary ? Buffer.concat(stdout) : Buffer.concat(chunks).toString('utf8')
        + (collected > MAX_COLLECT_BYTES ? `\n[output exceeded ${MAX_COLLECT_BYTES} bytes — command killed]` : '');
      resolve({ code: code === null ? -1 : code, signal: sig, output });
    });
  });
}

// The shell's image, built once per process unless Docker has it: the
// server's start builds it in the background (index.js), so the first bash
// call does not wait for it. A build that failed, or found Docker not
// installed or not running, is tried again when it is next needed.
let image = null;
function ensureImage() {
  image ||= setup.track('shell', async () => {
    const probe = await docker(['image', 'ls', '--quiet', IMAGE]);
    if (probe.output.trim()) return;
    await step("[sandbox] building the model's shell image from sandbox/Dockerfile (once, a few minutes)", "[sandbox] shell image built",
      () => docker(['build', '-t', IMAGE, DOCKERFILE_DIR], { timeoutMs: 10 * 60 * 1000 }));
  }, { unavailable: (error) => [DOCKER_MISSING, DOCKER_NOT_RUNNING].includes(error.message) }).catch((error) => {
    image = null;
    throw error;
  });
  return image;
}

// The networked container reaches the public internet and nothing else: not
// the host (on Docker Desktop host.docker.internal is the host's loopback,
// where this app's API, Paper's RCON and the user's other services listen),
// not the LAN, other containers or a cloud metadata service. The rules live
// in the container's network namespace, put there by a one-off container
// that shares it and holds NET_ADMIN; the sandbox itself never does. DNS goes
// to the resolvers Docker gave it. IPv6 is off in that namespace
// (IPV6_OFF): a global IPv6 address can be the host's own or a LAN device's,
// so no IPv6 range is public in the way the IPv4 rules need, and the public
// internet is reachable over IPv4.
const EGRESS_RULES = `set -e
for ns in $(awk '$1 == "nameserver" && $2 !~ /:/ { print $2 }' /etc/resolv.conf); do
  iptables -A OUTPUT -d "$ns" -p udp --dport 53 -j ACCEPT
  iptables -A OUTPUT -d "$ns" -p tcp --dport 53 -j ACCEPT
done
iptables -A OUTPUT -o lo -j ACCEPT
iptables -A OUTPUT -d 0.0.0.0/8,10.0.0.0/8,100.64.0.0/10,127.0.0.0/8,169.254.0.0/16,172.16.0.0/12,192.0.0.0/24,192.168.0.0/16,198.18.0.0/15,224.0.0.0/3 -j REJECT`;
// `all` turns IPv6 off on the interfaces the namespace has when it starts,
// `default` on the one it gets when it joins the bridge.
const IPV6_OFF = { 'net.ipv6.conf.all.disable_ipv6': '1', 'net.ipv6.conf.default.disable_ipv6': '1' };

async function firewall(name) {
  await docker(['run', '--rm', '--network', `container:${name}`, '--cap-drop', 'all', '--cap-add', 'NET_ADMIN',
    '--security-opt', 'no-new-privileges', IMAGE, 'sh', '-c', EGRESS_RULES]);
}

// One inspect answers whether the container exists, matches this image and
// /work, and is running; each Docker CLI call costs 10-25 ms, so nothing is
// asked twice.
// The networked container is the same sandbox on Docker's default network
// behind EGRESS_RULES: it starts with no network, gets its rules, and only
// then joins the bridge, so a container on the bridge has its rules. A
// restart would give it a fresh namespace without them, so a stopped one is
// made again.
// The root filesystem is read-only and /tmp a tmpfs, which counts against the
// container's memory: the only files a model can write on disk are in /work,
// which the chat's storage budget covers. (A writable layer lives in Docker's
// own storage, which no budget covers, and --storage-opt size is accepted
// and ignored on Docker Desktop.)
// Answers the chat's sandbox.
async function ensureContainer(conversationId, { network = false } = {}) {
  const box = await use(conversationId);
  const name = containerName(conversationId, { network });
  const inspected = await docker(['inspect', '-f', '{"image":{{json .Config.Image}},"mounts":{{json .Mounts}},"running":{{json .State.Running}},"networks":{{json .NetworkSettings.Networks}},"readOnly":{{json .HostConfig.ReadonlyRootfs}},"sysctls":{{json .HostConfig.Sysctls}}}', name], { check: false });
  if (inspected.code !== 0) {
    await ensureImage();
    await docker([
      'run', '-d', '--name', name,
      '--stop-timeout', '0', // PID 1 is `sleep infinity`; SIGTERM grace is pointless
      '--network', 'none',
      '--memory', '512m', '--memory-swap', '512m',
      '--pids-limit', '256', '--cpus', '2',
      '--read-only', '--tmpfs', '/tmp:rw,nosuid,nodev,size=256m',
      '--cap-drop', 'all', '--security-opt', 'no-new-privileges',
      '--user', `${process.getuid()}:${process.getgid()}`,
      '-v', `${workDir(conversationId)}:/work`,
      ...(network ? Object.entries(IPV6_OFF).flatMap(([key, value]) => ['--sysctl', `${key}=${value}`]) : []),
      IMAGE,
    ]);
    if (network) {
      await firewall(name);
      await docker(['network', 'disconnect', 'none', name]);
      await docker(['network', 'connect', 'bridge', name]);
    }
  } else {
    const metadata = JSON.parse(inspected.output);
    const mount = metadata.mounts.find(m => m.Destination === '/work');
    if (metadata.image !== IMAGE || mount?.Type !== 'bind' || mount.Source !== workDir(conversationId) || !metadata.readOnly) {
      await reset(conversationId);
      return ensureContainer(conversationId, { network });
    }
    // A networked container made before IPv6 was turned off is made again.
    const ipv6Off = Object.entries(IPV6_OFF).every(([key, value]) => metadata.sysctls?.[key] === value);
    if (network && !(metadata.running && Object.keys(metadata.networks).join() === 'bridge' && ipv6Off)) {
      await docker(['rm', '-f', name]);
      return ensureContainer(conversationId, { network });
    }
    // Container may exist but be stopped (server restart, idle reaper race).
    if (!metadata.running) await docker(['start', name]);
  }
  box.lastUsed = Date.now();
  return box;
}

// Codex-style middle truncation: keep the head and tail, report what was cut.
function truncateMiddle(text, maxBytes) {
  if (Buffer.byteLength(text) <= maxBytes) return { text, truncated: false };
  const half = Math.floor(maxBytes / 2);
  const head = text.slice(0, half);
  const tail = text.slice(-half);
  const totalLines = text.split('\n').length;
  const keptLines = head.split('\n').length + tail.split('\n').length;
  return {
    text: `${head}\n[... omitted ${Math.max(totalLines - keptLines, 0)} of ${totalLines} lines ...]\n${tail}`,
    truncated: true,
    totalLines,
  };
}

// ---- persistent session -----------------------------------------------------

function killSession(box) {
  const { session } = box;
  if (!session) return;
  box.session = null;
  try { session.child.kill('SIGKILL'); } catch { /* already dead */ }
}

function getSession(box) {
  const existing = box.session;
  if (existing && existing.child.exitCode === null && !existing.child.killed) return existing;

  const child = spawn(DOCKER, ['exec', '-i', '-w', '/work', containerName(box.id), 'bash']);
  const session = { child, collector: null };
  const onData = (d) => {
    if (session.collector) session.collector(d);
  };
  child.stdout.on('data', onData);
  child.stderr.on('data', onData);
  // A container that dies under a command fails that command (through the
  // write callback and 'close'), never the app with an unhandled EPIPE.
  child.stdin.on('error', () => {});
  child.on('error', () => {});
  child.on('close', () => {
    if (box.session === session) box.session = null;
  });
  box.session = session;
  return session;
}

// Runs a command in the conversation's persistent shell. The command and a
// sentinel line are written to the shell's stdin; output is collected until
// the sentinel echoes back with the exit code. On timeout the session is
// killed and replaced (cwd/env reset, /work intact).
function runInSession(session, command, timeoutMs, signal) {
  return new Promise((resolve, reject) => {
    const token = `__MCB_${crypto.randomUUID().slice(0, 8)}`;
    const sentinel = new RegExp(`${token} (\\d+)`);
    let text = '';
    let done = false;

    const finish = (result, err) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      signal?.removeEventListener('abort', abort);
      session.child.removeListener('close', closed);
      session.child.removeListener('error', failed);
      session.collector = null;
      if (err) reject(err); else resolve(result);
    };

    session.collector = (chunk) => {
      text += chunk.toString('utf8');
      if (text.length > MAX_COLLECT_BYTES) {
        finish({ text: `${text.slice(0, 4000)}\n[output exceeded ${MAX_COLLECT_BYTES} bytes — command killed, shell session reset]`, exitCode: -1, timedOut: false, reset: true });
        return;
      }
      const match = text.match(sentinel);
      if (match) {
        // The sentinel printf adds its own leading newline; strip it plus the
        // command's trailing one.
        const body = text.slice(0, match.index).replace(/\n{1,2}$/, '');
        finish({ text: body, exitCode: Number(match[1]), timedOut: false, reset: false });
      }
    };

    const timer = setTimeout(() => {
      finish({ text, exitCode: 124, timedOut: true, reset: true });
    }, timeoutMs);

    const abort = () => finish(null, signal.reason || new Error('Cancelled by user'));
    const closed = () => finish(null, new Error('Sandbox shell exited before the command completed'));
    const failed = error => finish(null, error);
    session.child.once('close', closed);
    session.child.once('error', failed);
    signal?.addEventListener('abort', abort, { once: true });
    if (signal?.aborted) { abort(); return; }
    session.child.stdin.write(`${command}\nprintf '\\n${token} %s\\n' "$?"\n`, (err) => {
      if (err) finish(null, new Error(`sandbox shell write failed: ${err.message}`));
    });
  });
}

// ---- exec -------------------------------------------------------------------

function formatResult(body, exitCode, durationMs, timedOut, ms, note) {
  let text = body;
  if (timedOut) text = `command timed out after ${ms} milliseconds (pass timeout_ms to allow more, max ${MAX_TIMEOUT_MS})\n${text}`;
  const cut = truncateMiddle(text, MAX_OUTPUT_BYTES);
  const sections = [`Exit code: ${exitCode}`, `Wall time: ${Math.round(durationMs / 100) / 10} seconds`];
  if (cut.truncated) sections.push(`Total output lines: ${cut.totalLines} (truncated)`);
  if (note) sections.push(note);
  sections.push('Output:', cut.text);
  return sections.join('\n');
}

// A command in a fresh shell of container `name`: whatever it leaves running
// ends with it. `limits` are shell builtins run first (ulimit).
async function oneShot(name, command, ms, signal, limits = '') {
  const cancel = () => { endProcesses(name).catch(() => {}); };
  signal?.addEventListener('abort', cancel, { once: true });
  try {
    return await docker([
      'exec', '-w', '/work', name, 'bash', '-c',
      `${limits}timeout -k 2 "$1" bash -c "$2"; code=$?; kill -KILL -1 2>/dev/null; exit $code`, 'mcb', `${ms / 1000}`, command,
    ], { timeoutMs: ms + 30_000, check: false });
  } finally {
    signal?.removeEventListener('abort', cancel);
  }
}

// Runs a bash command for the model. Default: the conversation's persistent
// shell (no network). network=true: a fresh one-shot container WITH network
// (session state does not apply there, /work is shared). The chat is
// measured in the background after every command (storage.js); while it is
// over its storage budget or the disk under its reserve, as last measured, a
// command runs with file writes disabled. A command does not wait for the
// last one's measurement (du of a big chat takes 15 ms): the next one sees
// it, and the watchdog below stops a chat that grows far past its budget.
async function exec(conversationId, command, timeoutMs, { network = false, signal } = {}) {
  signal?.throwIfAborted();
  if (network && !networkEnabled) throw new Error('Network access is disabled in this workspace; the bash tool has no network mode here');
  const ms = Math.min(Math.max(Number(timeoutMs) || DEFAULT_TIMEOUT_MS, 1000), MAX_TIMEOUT_MS);
  const started = Date.now();
  try {
    const refusal = storage.refusal(conversationId);
    if (refusal) return await restricted(conversationId, command, ms, signal, refusal, started);
    return await (network ? networked : persistent)(conversationId, command, ms, signal, started);
  } catch (error) {
    const halt = takeHalt(conversationId);
    throw halt && !signal?.aborted ? new Error(halt) : error;
  } finally {
    storage.measure(conversationId);
  }
}

async function networked(conversationId, command, ms, signal, started) {
  await ensureContainer(conversationId, { network: true });
  signal?.throwIfAborted();
  const result = await oneShot(containerName(conversationId, { network: true }), command, ms, signal);
  signal?.throwIfAborted();
  return {
    exitCode: result.code,
    formatted: formatResult(result.output, result.code, Date.now() - started, result.code === 124, ms, takeHalt(conversationId) ?? '(one-shot networked shell — session state not kept)'),
  };
}

async function persistent(conversationId, command, ms, signal, started) {
  let box = await use(conversationId);
  // A live persistent shell proves its container is up: the exec client exits
  // with the container, and that close drops the session. Probing Docker
  // again on every call would cost three CLI round trips (about 50 ms on
  // Docker Desktop) against a 1 ms shell round trip.
  if (!box.session) box = await ensureContainer(conversationId);
  if (signal?.aborted) { await resetShell(box); signal.throwIfAborted(); }
  const session = getSession(box);
  let r;
  try {
    r = await runInSession(session, command, ms, signal);
  } catch (error) {
    await resetShell(box);
    throw error;
  }
  if (r.reset) await resetShell(box);
  const note = takeHalt(conversationId) ?? (r.timedOut ? '(shell session was reset: cwd and env cleared, /work files intact)' : null);
  return {
    exitCode: r.exitCode,
    formatted: formatResult(r.text, r.exitCode, Date.now() - started, r.timedOut, ms, note),
  };
}

// Over the limits, the command runs in a one-shot shell under `ulimit -f 0`,
// after the session and its background jobs are ended: it can read, delete
// and truncate files, and every write of a byte fails ("File too large";
// SIGXFSZ is ignored, so the write fails instead of ending the command).
// That is how a chat gets back under its budget, so while the command runs
// the sandbox is `restricted` and the watchdog does not end it. The result
// is a failure that says so, with the command's output (which is why a halt
// not yet reported is dropped).
async function restricted(conversationId, command, ms, signal, refusal, started) {
  const box = await ensureContainer(conversationId);
  // A halt under way ends first, so its kill cannot land on this command.
  if (box.state.name === 'halted') await box.state.ended;
  move(box, ['ready', 'halted'], { name: 'restricted' });
  try {
    await resetShell(box);
    signal?.throwIfAborted();
    const result = await oneShot(containerName(conversationId), command, ms, signal, 'ulimit -f 0; trap "" XFSZ; ');
    signal?.throwIfAborted();
    await storage.measure(conversationId);
    const still = storage.refusal(conversationId);
    return {
      exitCode: result.code,
      formatted: formatResult(result.output, result.code, Date.now() - started, result.code === 124, ms, '(one-shot shell with file writes disabled — session state not kept)'),
      storage: `${refusal} While that lasts, bash runs each command in a fresh shell with file writes disabled (ulimit -f 0): it can read, delete and truncate files in /work, and anything that writes fails with "File too large". ${still ? `After this command: ${still}` : 'After this command the chat is back within its limits; the next command runs normally.'}`,
    };
  } finally {
    // A reset meanwhile has the sandbox removing: that stands.
    if (box.state.name === 'restricted') box.state = { name: 'ready' };
  }
}

// /work cannot be policed per write, so while containers are up the disk's
// free space is read every second (one statfs). When it is under the reserve,
// or has dropped WATCH_STEP since the chats were last measured, every chat
// with a container is measured, and the processes of one over its limits are
// ended: its command (and background jobs) stop, and the next command learns
// why. A chat can pass its budget by at most about WATCH_STEP. Only a `ready`
// sandbox is halted: a halted one was ended already, and a restricted one is
// running the command that frees space, and cannot write.
const WATCH_MS = 1000;
const WATCH_STEP = 64 * 1024 * 1024;
let watchedFree = null;
let watching = false;

// Why the watchdog ended the chat's processes, once: the sandbox is ready again.
function takeHalt(conversationId) {
  const box = sandboxes.get(conversationId);
  if (box?.state.name !== 'halted') return null;
  const { reason } = box.state;
  box.state = { name: 'ready' };
  return reason;
}

async function halt(box, refusal) {
  const state = { name: 'halted', reason: `The sandbox's processes were ended: ${refusal} Delete files in /work to continue.`, ended: null };
  move(box, ['ready'], state);
  killSession(box);
  state.ended = Promise.all([false, true].map((network) => endProcesses(containerName(box.id, { network }))));
  await state.ended;
}

async function watchStorage() {
  if (!sandboxes.size || watching) return;
  const free = storage.freeBytes();
  if (watchedFree === null || free > watchedFree) watchedFree = free;
  if (free >= storage.DISK_RESERVE_BYTES && watchedFree - free < WATCH_STEP) return;
  watchedFree = free;
  watching = true;
  try {
    for (const box of [...sandboxes.values()]) {
      await storage.measure(box.id);
      const refusal = storage.refusal(box.id);
      if (refusal && box.state.name === 'ready') await halt(box, refusal);
    }
  } finally {
    watching = false;
  }
}

const watchdog = setInterval(() => {
  watchStorage().catch((error) => console.error('[sandbox] storage watch failed:', error));
}, WATCH_MS).unref();

// ---- host-side file bridge ----------------------------------------------------

// Mirrors every stored conversation image (uploads + see_build renders) into
// /work/images so shell commands can operate on them. view_image outputs
// ('view-' prefix) are skipped — their source already lives in /work.
function syncImages(conversationId, imagesDir) {
  const src = path.join(imagesDir, conversationId);
  if (!fs.existsSync(src)) return;
  for (const name of fs.readdirSync(src)) {
    if (name.startsWith('view-')) continue;
    copyIntoWork(conversationId, `images/${name}`, path.join(src, name), { replace: false });
  }
}

function syncAttachments(conversationId, attachmentsDir) {
  const src = path.join(attachmentsDir, conversationId);
  if (!fs.existsSync(src)) return;
  for (const name of fs.readdirSync(src)) copyIntoWork(conversationId, `attachments/${name}`, path.join(src, name), { replace: false });
}

// /work is writable from inside the container, so the model controls every
// symlink under it. Host-side access resolves real paths, refuses anything
// outside the real work directory, and checks the opened inode against the
// resolved path so a link swapped after resolution cannot redirect access.
const inside = (root, file) => file === root || file.startsWith(root + path.sep);
const escapeError = relPath => new Error(`Path escapes /work: ${relPath}`);

function lexists(file) {
  try { fs.lstatSync(file); return true; } catch (error) { if (error.code === 'ENOENT') return false; throw error; }
}

const resolveWorkPath = inWork((conversationId, relPath, { write = false } = {}) => {
  const root = fs.realpathSync(workDir(conversationId));
  const file = path.resolve(root, String(relPath).replace(/^\/?work(?:\/|$)/, ''));
  if (!inside(root, file)) throw escapeError(relPath);
  let existing = file;
  while (!lexists(existing)) existing = path.dirname(existing);
  if (write && existing === file && fs.lstatSync(file).isSymbolicLink()) throw new Error(`Refusing to write through a symlink in /work: ${relPath}`);
  let real;
  try { real = fs.realpathSync(existing); } catch (error) {
    if (error.code === 'ENOENT' || error.code === 'ELOOP') throw escapeError(relPath);
    throw error;
  }
  if (!inside(root, real)) throw escapeError(relPath);
  return path.join(real, path.relative(existing, file));
});

function checkedDescriptor(conversationId, relPath, fd, file) {
  try {
    const opened = fs.realpathSync(file);
    if (!inside(fs.realpathSync(workDir(conversationId)), opened)) throw escapeError(relPath);
    const expected = fs.statSync(opened), actual = fs.fstatSync(fd);
    if (expected.dev !== actual.dev || expected.ino !== actual.ino) throw escapeError(relPath);
    return opened;
  } catch (error) { fs.closeSync(fd); throw error; }
}

const workFileExists = inWork((conversationId, relPath) => fs.existsSync(resolveWorkPath(conversationId, relPath)));

const readWorkFile = inWork((conversationId, relPath, { maxBytes = Infinity, encoding } = {}) => {
  const file = resolveWorkPath(conversationId, relPath);
  let fd;
  try { fd = fs.openSync(file, fs.constants.O_RDONLY | fs.constants.O_NOFOLLOW); } catch (error) {
    if (error.code === 'ENOENT') throw new Error(`No such file in /work: ${relPath}`);
    if (error.code === 'ELOOP') throw escapeError(relPath);
    throw error;
  }
  checkedDescriptor(conversationId, relPath, fd, file);
  try {
    const { size } = fs.fstatSync(fd);
    if (size > maxBytes) throw new Error(`${relPath} is ${size} bytes (max ${maxBytes})`);
    const data = fs.readFileSync(fd);
    return encoding ? data.toString(encoding) : data;
  } finally { fs.closeSync(fd); }
});

// Returns an open descriptor for a regular file in /work, or null when
// replace is false and the file already exists.
const openWorkFileForWrite = inWork((conversationId, relPath, { replace = true } = {}) => {
  if (!replace && lexists(path.join(workDir(conversationId), String(relPath).replace(/^\/?work(?:\/|$)/, '')))) return null;
  const file = resolveWorkPath(conversationId, relPath, { write: true });
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const { O_WRONLY, O_CREAT, O_EXCL, O_NOFOLLOW } = fs.constants;
  let fd;
  try { fd = fs.openSync(file, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0o644); } catch (error) {
    if (error.code !== 'EEXIST') throw error;
    if (!replace) return null;
    try { fd = fs.openSync(file, O_WRONLY | O_NOFOLLOW); } catch (retry) {
      if (retry.code === 'ELOOP') throw new Error(`Refusing to write through a symlink in /work: ${relPath}`);
      throw retry;
    }
  }
  checkedDescriptor(conversationId, relPath, fd, file);
  if (!fs.fstatSync(fd).isFile()) { fs.closeSync(fd); throw new Error(`Not a regular file in /work: ${relPath}`); }
  fs.ftruncateSync(fd, 0);
  return fd;
});

// Synchronous /work reader for build programs (mc.readFile): lets bash
// precompute data (voxel grids, palettes) far larger than the model could
// ever echo through its own context.
function makeReadFile(conversationId) {
  return (relPath) => {
    try { return readWorkFile(conversationId, relPath, { maxBytes: 8 * 1024 * 1024, encoding: 'utf8' }); }
    catch (error) { throw new Error(`mc.readFile: ${error.message}`); }
  };
}

// The reverse bridge: build programs export data (mc.writeFile / mc.dump) for
// bash to analyze. Called by the agent after a build succeeds.
const writeWorkFile = inWork((conversationId, relPath, text) => {
  const fd = openWorkFileForWrite(conversationId, relPath);
  try { fs.writeFileSync(fd, text); } finally { fs.closeSync(fd); }
});

// Copies a host file into /work without following model-created links.
const copyIntoWork = inWork((conversationId, relPath, source, { replace = true } = {}) => {
  const fd = openWorkFileForWrite(conversationId, relPath, { replace });
  if (fd === null) return;
  const input = fs.openSync(source, 'r');
  try {
    const buffer = Buffer.allocUnsafe(1 << 20);
    for (let n; (n = fs.readSync(input, buffer, 0, buffer.length, null)) > 0;) fs.writeSync(fd, buffer, 0, n);
  } finally { fs.closeSync(input); fs.closeSync(fd); }
});

// Deletes files in /work from inside the container, where /work is all there
// is: a link the model put in their path cannot reach a host file.
async function removeWorkFiles(conversationId, relPaths) {
  await ensureContainer(conversationId);
  await docker(['exec', '-w', '/work', containerName(conversationId), 'rm', '-f', '--', ...relPaths]);
}

// Loads an image from /work for the model to look at. Pillow (inside the
// container) normalizes any format and downscales to vision-API resolution;
// node stays dependency-free.
async function viewImage(conversationId, relPath) {
  const file = resolveWorkPath(conversationId, relPath);
  if (!fs.existsSync(file)) throw new Error(`No such file in /work: ${relPath}`);
  const rel = path.relative(fs.realpathSync(workDir(conversationId)), file);
  const script = [
    'import sys',
    'from PIL import Image',
    'img = Image.open(sys.argv[1])',
    'img = img.convert("RGBA")',
    'scale = min(1.0, 1568 / max(img.size))',
    'if scale < 1.0: img = img.resize((round(img.width * scale), round(img.height * scale)), Image.LANCZOS)',
    'img.save(sys.stdout.buffer, "PNG")',
  ].join('\n');
  await ensureContainer(conversationId);
  // Stream the PNG back directly. Host-deleted files on Docker Desktop bind
  // mounts can remain cached inside the container and break the next save.
  const { output } = await docker(
    ['exec', '-w', '/work', containerName(conversationId), 'python3', '-c', script, rel],
    { timeoutMs: 30_000, binary: true },
  );
  return output;
}

// Ends every process in a container but its init (`sleep infinity`), which
// keeps it up: `kill -1` signals all processes except the caller and PID 1.
function endProcesses(name) {
  return docker(['exec', name, 'bash', '-c', 'kill -KILL -1'], { check: false });
}

// A new shell for the conversation, with the command that ran away and every
// background job ended. Killing the host docker exec client alone leaves its
// children running.
async function resetShell(box) {
  killSession(box);
  await endProcesses(containerName(box.id));
}

// Removes the chat's containers (not its /work). From any state the sandbox
// is `removing` until they are gone, then forgotten; a reset that arrives
// meanwhile waits for the same removal.
async function reset(conversationId) {
  const box = sandboxes.get(conversationId) ?? track(conversationId);
  if (box.state.name === 'removing') return box.state.removed;
  killSession(box);
  const removed = Promise.all([false, true].map((network) => docker(['rm', '-f', containerName(conversationId, { network })]).catch(() => {})))
    .then(() => { sandboxes.delete(conversationId); });
  box.state = { name: 'removing', removed };
  return removed;
}

async function destroy(conversationId) {
  await reset(conversationId);
  fs.rmSync(path.join(ROOT, conversationId), { recursive: true, force: true });
  storage.forget(conversationId);
}

// The containers of this /work root that an earlier run of the app left:
// they count as used now, so the idle reaper and the storage watchdog see
// them. Asked once per process (again after a failure).
let adopted = null;
function adoptContainers() {
  adopted ||= docker(['ps', '-a', '--filter', `name=-${SCOPE}-`, '--format', '{{.Names}}']).then(({ output }) => {
    for (const name of output.split('\n')) {
      const id = CONTAINER.exec(name)?.[1];
      if (id && !sandboxes.has(id)) track(id);
    }
  }, (error) => {
    adopted = null;
    throw error;
  });
  return adopted;
}

// Idle containers are removed (not their /work — that persists on disk and the
// container recreates on the next exec). The removals are not waited for: a
// command that arrives during one waits for it (use).
async function reapIdle() {
  await adoptContainers();
  const now = Date.now();
  for (const box of sandboxes.values()) {
    if (now - box.lastUsed > IDLE_MS) reset(box.id);
  }
}

// With Docker not installed or not running there is nothing to reap, so that
// is not logged. Any other failure is logged when it starts or changes, not
// every minute.
let reaperFailure = null;
function reaperFailed(error) {
  if (error.message === DOCKER_MISSING || error.message === DOCKER_NOT_RUNNING) return;
  if (error.message !== reaperFailure) console.error('[sandbox] idle reaper failed:', error.message);
  reaperFailure = error.message;
}
const reaper = setInterval(() => {
  reapIdle().then(() => { reaperFailure = null; }, reaperFailed);
}, 60_000).unref();

// The server is stopping: every container this process tracks is removed,
// networked ones and background jobs with them, so nothing writes to /work
// once the watchdog is gone. A removal that fails (Docker not running) is
// left to the next start's reaper (reset never throws).
async function shutdown() {
  clearInterval(watchdog);
  clearInterval(reaper);
  await Promise.all([...sandboxes.keys()].map(reset));
}

module.exports = {
  exec, viewImage, syncImages, syncAttachments, reset, destroy, shutdown, workDir, resolveWorkPath, makeReadFile, writeWorkFile,
  readWorkFile, workFileExists, copyIntoWork, removeWorkFiles, workError, networkAccess, disableNetwork, ensureImage,
  IMAGE, DEFAULT_TIMEOUT_MS, MAX_TIMEOUT_MS,
  DOCKER_MISSING, DOCKER_NOT_RUNNING,
  _internal: { containerName, watchStorage, reapIdle, reaperFailed, sandboxes },
};
