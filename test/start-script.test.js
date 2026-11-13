'use strict';
// ./start and install.sh, run on a copy of the app with a fake server and a
// fake Node.js served from this process. Nothing reaches the network, opens
// a browser or writes outside a temp folder.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { spawn, execFileSync } = require('node:child_process');

const ROOT = path.join(__dirname, '..');
const NODE_VERSION = '26.13.1';
const PLATFORM = `${process.platform}-${process.arch}`;
const NODE_NAME = `node-v${NODE_VERSION}-${PLATFORM}`;
const supported = ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64'].includes(PLATFORM);

function write(file, text, mode = 0o644) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text, { mode });
}

function stub(dir, name, script) {
  write(path.join(dir, name), `#!/bin/sh\n${script}\n`, 0o755);
}

// A copy of the app: the real start and install.sh, and a server that prints
// what it was started with.
function makeApp(dir) {
  write(path.join(dir, 'start'), fs.readFileSync(path.join(ROOT, 'start'), 'utf8'), 0o755);
  write(path.join(dir, 'install.sh'), fs.readFileSync(path.join(ROOT, 'install.sh'), 'utf8'), 0o755);
  write(path.join(dir, 'package-lock.json'), '{"lockfileVersion": 3}\n');
  write(path.join(dir, 'server', 'index.js'),
    "console.log(`fake server ${JSON.stringify(process.argv.slice(2))} PORT=${process.env.PORT} PATH0=${process.env.PATH.split(':')[0]}`);\n");
  return dir;
}

// A Node.js release as nodejs.org lays it out, whose node is this test's node
// and whose npm records each call in npm-calls.log in the folder it runs in
// and writes a native module for better-sqlite3.
const NATIVE = path.join('node_modules', 'better-sqlite3', 'build', 'Release', 'better_sqlite3.node');
function makeNodeTarball(dir) {
  const bin = path.join(dir, NODE_NAME, 'bin');
  stub(bin, 'node', `exec "${process.execPath}" "$@"`);
  stub(bin, 'npm', `echo "$*" >> npm-calls.log; mkdir -p "${path.dirname(NATIVE)}"; echo "built by the app's Node.js" > "${NATIVE}"`);
  const file = path.join(dir, `${NODE_NAME}.tar.xz`);
  execFileSync('tar', ['-cJf', file, '-C', dir, NODE_NAME]);
  return { bytes: fs.readFileSync(file), sha256: crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex') };
}

// Serves the tarball where nodejs.org has it; every request is recorded.
async function nodeDist(bytes) {
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    if (req.url !== `/v${NODE_VERSION}/${NODE_NAME}.tar.xz`) {
      res.writeHead(404);
      return res.end();
    }
    res.writeHead(200, { 'content-length': bytes.length });
    res.end(bytes);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { url: `http://127.0.0.1:${server.address().port}`, requests, close: () => new Promise((resolve) => server.close(resolve)) };
}

// What the prerequisite check looks for, all present: stubs on PATH and an
// empty /usr/bin/bwrap and /usr/bin/prlimit under the system root.
function prerequisites(dir) {
  const bin = path.join(dir, 'bin');
  for (const name of ['cc', 'python3', 'xcode-select']) stub(bin, name, 'exit 0');
  const systemRoot = path.join(dir, 'system');
  for (const name of ['bwrap', 'prlimit']) stub(path.join(systemRoot, 'usr', 'bin'), name, 'exit 0');
  return { bin, systemRoot };
}

function run(command, args, { cwd, env, input }) {
  return new Promise((resolve, reject) => {
    const child = spawn('/bin/bash', [command, ...args], { cwd, env, stdio: ['pipe', 'pipe', 'pipe'] });
    let stdout = '';
    let stderr = '';
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.on('error', reject);
    child.on('close', (code) => resolve({ code, stdout, stderr }));
    child.stdin.end(input || '');
  });
}

// A temp folder with the app, a fake Node.js served locally and every
// prerequisite present; `start` runs the app's start there.
async function fixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'start-script-'));
  const app = makeApp(path.join(dir, 'app'));
  const tarball = makeNodeTarball(path.join(dir, 'release'));
  const dist = await nodeDist(tarball.bytes);
  const { bin, systemRoot } = prerequisites(dir);
  const tools = path.join(dir, 'tools');
  const env = {
    HOME: path.join(dir, 'home'),
    PATH: `${bin}:/usr/bin:/bin`,
    MCBENCH_TOOLS_DIR: tools,
    MCBENCH_NODE_DIST: dist.url,
    MCBENCH_NODE_SHA256: tarball.sha256,
    MCBENCH_SYSTEM_ROOT: systemRoot,
    PORT: '4321',
  };
  t.after(async () => {
    await dist.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });
  const start = (args = ['--no-open'], extra = {}) => run(path.join(app, 'start'), args, { cwd: dir, env: { ...env, ...extra } });
  return { dir, app, tools, dist, env, start, npmCalls: () => fs.readFileSync(path.join(app, 'npm-calls.log'), 'utf8') };
}

test('start downloads Node.js, installs the packages with it and runs the server with the other arguments', { skip: !supported }, async (t) => {
  const { app, tools, dist, start, npmCalls } = await fixture(t);
  const result = await start(['--no-open', '--flag']);
  assert.equal(result.code, 0, result.stderr);
  const nodeDir = path.join(tools, NODE_NAME);
  assert.equal(result.stdout,
    `Downloading Node.js ${NODE_VERSION} (about 30 MB) into ${tools}\n`
    + "Installing the app's packages (npm ci)\n"
    + `fake server ["--flag"] PORT=4321 PATH0=${nodeDir}/bin\n`);
  assert.equal(result.stderr, '');
  assert.deepEqual(dist.requests, [`/v${NODE_VERSION}/${NODE_NAME}.tar.xz`]);
  assert.deepEqual(fs.readdirSync(tools), [NODE_NAME]);
  assert.equal(npmCalls(), 'ci --no-fund --no-audit --no-update-notifier --loglevel=error\n');
  assert.ok(fs.existsSync(path.join(app, 'node_modules', '.mcbench-start')));
});

test('a second start downloads and installs nothing; a changed package-lock.json is installed again', { skip: !supported }, async (t) => {
  const { app, dist, start, npmCalls } = await fixture(t);
  assert.equal((await start()).code, 0);
  const second = await start();
  assert.equal(second.code, 0, second.stderr);
  assert.match(second.stdout, /^fake server \[\] PORT=4321 /);
  assert.equal(dist.requests.length, 1);
  assert.equal(npmCalls(), 'ci --no-fund --no-audit --no-update-notifier --loglevel=error\n');
  write(path.join(app, 'package-lock.json'), '{"lockfileVersion": 3, "changed": true}\n');
  const third = await start();
  assert.equal(third.code, 0, third.stderr);
  assert.match(third.stdout, /^Installing the app's packages \(npm ci\)\nfake server/);
  assert.equal(npmCalls(), 'ci --no-fund --no-audit --no-update-notifier --loglevel=error\nci --no-fund --no-audit --no-update-notifier --loglevel=error\n');
});

test('a native module built since by another Node.js, as `npm rebuild` with it does, is installed again', { skip: !supported }, async (t) => {
  const { app, start, npmCalls } = await fixture(t);
  assert.equal((await start()).code, 0);
  fs.writeFileSync(path.join(app, NATIVE), 'built by another Node.js\n');
  const second = await start();
  assert.equal(second.code, 0, second.stderr);
  assert.match(second.stdout, /^Installing the app's packages \(npm ci\)\nfake server/);
  assert.equal(npmCalls(), 'ci --no-fund --no-audit --no-update-notifier --loglevel=error\nci --no-fund --no-audit --no-update-notifier --loglevel=error\n');
  assert.equal(fs.readFileSync(path.join(app, NATIVE), 'utf8'), "built by the app's Node.js\n");
});

test('a download whose sha256 is not the pinned one fails and installs nothing', { skip: !supported }, async (t) => {
  const { app, tools, start } = await fixture(t);
  // Without the override the pinned checksum is the one checked.
  const result = await start(['--no-open'], { MCBENCH_NODE_SHA256: '' });
  assert.equal(result.code, 1);
  assert.match(result.stderr, new RegExp(`has the sha256 [0-9a-f]{64}, not [0-9a-f]{64}: it is not the Node\\.js this start expects, so nothing was installed\\.\\n$`));
  assert.doesNotMatch(result.stdout, /fake server/);
  assert.deepEqual(fs.readdirSync(tools), []);
  assert.equal(fs.existsSync(path.join(app, 'node_modules')), false);
});

// Runs start (or install.sh) with only the stubs in `bin` on PATH, which
// fake uname as `system` `machine`. `bwrap` is the script of the system
// root's /usr/bin/bwrap and `sysctls` the files of its /proc/sys.
async function check(t, { system, machine, commands = [], systemFiles = [], script = 'start', bwrap = 'exit 0', sysctls = {} }) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'start-check-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const app = makeApp(path.join(dir, 'app'));
  const bin = path.join(dir, 'bin');
  stub(bin, 'uname', `case "$1" in -s) echo ${system} ;; -m) echo ${machine} ;; esac`);
  for (const name of commands) stub(bin, name, 'exit 0');
  const systemRoot = path.join(dir, 'system');
  for (const name of systemFiles) stub(path.join(systemRoot, 'usr', 'bin'), name, name === 'bwrap' ? bwrap : 'exit 0');
  for (const [name, value] of Object.entries(sysctls)) write(path.join(systemRoot, 'proc', 'sys', name), `${value}\n`);
  const env = { HOME: path.join(dir, 'home'), PATH: bin, MCBENCH_TOOLS_DIR: path.join(dir, 'tools'), MCBENCH_SYSTEM_ROOT: systemRoot };
  const result = await run(path.join(app, script), ['--no-open'], { cwd: dir, env });
  assert.equal(result.code, 1);
  assert.equal(result.stdout, '');
  assert.equal(fs.existsSync(path.join(dir, 'tools')), false);
  return result.stderr;
}

test('an unsupported system or processor is refused with one line', async (t) => {
  assert.equal(await check(t, { system: 'FreeBSD', machine: 'amd64' }),
    'mcbench-lite runs on macOS and Linux, on arm64 or x64; this is FreeBSD amd64.\n');
  assert.equal(await check(t, { system: 'Linux', machine: 'riscv64' }),
    'mcbench-lite runs on macOS and Linux, on arm64 or x64; this is Linux riscv64.\n');
});

test('on macOS missing Xcode Command Line Tools are named with the command that installs them', async (t) => {
  const message = await check(t, { system: 'Darwin', machine: 'arm64' });
  assert.equal(message, 'mcbench-lite needs the Xcode Command Line Tools (they give cc, git and python3). Install them with:\n'
    + '  xcode-select --install\nThen run this again.\n');
});

const LINUX = [
  { item: 'cc', name: 'a C compiler (cc)', apt: 'build-essential', dnf: 'gcc' },
  { item: 'python3', name: 'python3', apt: 'python3', dnf: 'python3' },
  { item: 'curl', name: 'curl', apt: 'curl', dnf: 'curl' },
  { item: 'xz', name: 'xz', apt: 'xz-utils', dnf: 'xz' },
  { item: 'bwrap', file: true, name: 'bubblewrap (/usr/bin/bwrap)', apt: 'bubblewrap', dnf: 'bubblewrap' },
  { item: 'prlimit', file: true, name: 'prlimit (/usr/bin/prlimit, from util-linux)', apt: 'util-linux', dnf: 'util-linux' },
];
const uid = process.getuid();
const sudo = uid === 0 ? '' : 'sudo ';

// Each missing alone, with apt-get and with dnf.
for (const { item, file, name, apt, dnf } of LINUX) {
  test(`on Linux a missing ${item} is named with the package that installs it`, async (t) => {
    const commands = LINUX.filter((other) => !other.file && other.item !== item).map((other) => other.item);
    const systemFiles = LINUX.filter((other) => other.file && other.item !== item).map((other) => other.item);
    assert.equal(await check(t, { system: 'Linux', machine: 'x86_64', commands: [...commands, 'apt-get'], systemFiles }),
      `mcbench-lite needs these, which are not installed: ${name}.\nInstall them with:\n`
      + `  ${sudo}apt-get update && ${sudo}apt-get install -y ${apt}\nThen run this again.\n`);
    assert.equal(await check(t, { system: 'Linux', machine: 'aarch64', commands: [...commands, 'dnf'], systemFiles }),
      `mcbench-lite needs these, which are not installed: ${name}.\nInstall them with:\n`
      + `  ${sudo}dnf install -y ${dnf}\nThen run this again.\n`);
  });
}

test('on Linux everything missing is named at once, for pacman and for no known package manager', async (t) => {
  const all = LINUX.map(({ name }) => name).join(', ');
  assert.equal(await check(t, { system: 'Linux', machine: 'x86_64', commands: ['pacman'] }),
    `mcbench-lite needs these, which are not installed: ${all}.\nInstall them with:\n`
    + `  ${sudo}pacman -S --needed gcc python curl xz bubblewrap util-linux\nThen run this again.\n`);
  assert.equal(await check(t, { system: 'Linux', machine: 'x86_64' }),
    `mcbench-lite needs these, which are not installed: ${all}.\n`
    + "Install them with your system's package manager (the packages are usually called: gcc python3 curl xz bubblewrap util-linux).\n"
    + 'Then run this again.\n');
});

test('install.sh also requires git', async (t) => {
  const commands = LINUX.filter((other) => !other.file).map((other) => other.item);
  const systemFiles = LINUX.filter((other) => other.file).map((other) => other.item);
  assert.equal(await check(t, { system: 'Linux', machine: 'x86_64', commands: [...commands, 'apt-get'], systemFiles, script: 'install.sh' }),
    `mcbench-lite needs these, which are not installed: git.\nInstall them with:\n  ${sudo}apt-get update && ${sudo}apt-get install -y git\nThen run this again.\n`);
});

// Everything present on Linux, for the sandbox check.
const PRESENT = {
  system: 'Linux',
  machine: 'x86_64',
  commands: [...LINUX.filter((other) => !other.file).map((other) => other.item), 'apt-get'],
  systemFiles: LINUX.filter((other) => other.file).map((other) => other.item),
};
const SANDBOX_FAILS = 'echo "bwrap: No permissions to create a new namespace" >&2; exit 1';
const SANDBOX_HEAD = 'mcbench-lite runs build programs in a bubblewrap sandbox, which cannot be made here. bubblewrap says:\n'
  + '  bwrap: No permissions to create a new namespace\n';

test('on Linux a sandbox bubblewrap cannot make is refused with its message, and with the command for each setting that forbids it', async (t) => {
  assert.equal(await check(t, { ...PRESENT, bwrap: SANDBOX_FAILS, sysctls: { 'kernel/apparmor_restrict_unprivileged_userns': 0, 'user/max_user_namespaces': 15000 } }),
    `${SANDBOX_HEAD}Then run this again.\n`);
  // install.sh says it before it clones anything.
  assert.equal(await check(t, { ...PRESENT, commands: [...PRESENT.commands, 'git'], bwrap: SANDBOX_FAILS, script: 'install.sh' }),
    `${SANDBOX_HEAD}Then run this again.\n`);
  assert.equal(await check(t, { ...PRESENT, bwrap: SANDBOX_FAILS, sysctls: { 'kernel/apparmor_restrict_unprivileged_userns': 1, 'user/max_user_namespaces': 15000 } }),
    `${SANDBOX_HEAD}AppArmor restricts unprivileged user namespaces (kernel.apparmor_restrict_unprivileged_userns=1). To allow them from now on:\n`
    + `  echo kernel.apparmor_restrict_unprivileged_userns=0 | ${sudo}tee /etc/sysctl.d/60-apparmor-namespace.conf && ${sudo}sysctl -p /etc/sysctl.d/60-apparmor-namespace.conf\n`
    + 'Then run this again.\n');
  assert.equal(await check(t, { ...PRESENT, bwrap: SANDBOX_FAILS, sysctls: { 'kernel/unprivileged_userns_clone': 0, 'user/max_user_namespaces': 0 } }),
    `${SANDBOX_HEAD}The kernel forbids unprivileged user namespaces (kernel.unprivileged_userns_clone=0). To allow them from now on:\n`
    + `  echo kernel.unprivileged_userns_clone=1 | ${sudo}tee /etc/sysctl.d/60-userns.conf && ${sudo}sysctl -p /etc/sysctl.d/60-userns.conf\n`
    + 'The kernel allows no user namespaces (user.max_user_namespaces=0). To allow them from now on:\n'
    + `  echo user.max_user_namespaces=1000 | ${sudo}tee /etc/sysctl.d/60-max-userns.conf && ${sudo}sysctl -p /etc/sysctl.d/60-max-userns.conf\n`
    + 'Then run this again.\n');
});

test('on Linux start goes on to Node.js once bubblewrap makes the sandbox, asked for the namespaces and mounts the server uses', async (t) => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'start-sandbox-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));
  const app = makeApp(path.join(dir, 'app'));
  const bin = path.join(dir, 'bin');
  stub(bin, 'uname', 'case "$1" in -s) echo Linux ;; -m) echo x86_64 ;; esac');
  for (const name of PRESENT.commands) stub(bin, name, 'exit 0');
  // The download stops here: the check has passed.
  stub(bin, 'curl', 'exit 22');
  const systemRoot = path.join(dir, 'system');
  const asked = path.join(dir, 'bwrap-args');
  stub(path.join(systemRoot, 'usr', 'bin'), 'bwrap', `echo "$*" > "${asked}"`);
  stub(path.join(systemRoot, 'usr', 'bin'), 'prlimit', 'exit 0');
  const tools = path.join(dir, 'tools');
  const env = { HOME: path.join(dir, 'home'), PATH: `${bin}:/usr/bin:/bin`, MCBENCH_TOOLS_DIR: tools, MCBENCH_SYSTEM_ROOT: systemRoot };
  const result = await run(path.join(app, 'start'), ['--no-open'], { cwd: dir, env });
  assert.equal(result.code, 22, result.stderr);
  assert.equal(result.stdout, `Downloading Node.js ${NODE_VERSION} (about 30 MB) into ${tools}\n`);
  assert.equal(fs.readFileSync(asked, 'utf8'),
    '--unshare-user --unshare-ipc --unshare-pid --unshare-net --unshare-uts --unshare-cgroup --die-with-parent --ro-bind / / --proc /proc --dev /dev -- true\n');
});

test('start and install.sh check the prerequisites with the same code', () => {
  const block = (file) => /# --- prerequisites[^\n]*\n[\s\S]*?# --- end of prerequisites ---\n/.exec(fs.readFileSync(path.join(ROOT, file), 'utf8'))[0];
  assert.equal(block('install.sh'), block('start'));
});

test('install.sh, piped to bash, clones the repository and starts it, updates it the next time, and stops at a folder that is not it', { skip: !supported }, async (t) => {
  const { dir, env } = await fixture(t);
  // The repository: the fixture's app, committed.
  const repo = path.join(dir, 'repo');
  makeApp(repo);
  const git = (...args) => execFileSync('git', ['-c', 'user.name=test', '-c', 'user.email=test@example.com', ...args], { cwd: repo, env });
  git('init', '--quiet');
  git('add', 'start', 'install.sh', 'package-lock.json', 'server/index.js');
  git('commit', '--quiet', '-m', 'app');
  const script = fs.readFileSync(path.join(ROOT, 'install.sh'), 'utf8');
  const target = path.join(env.HOME, 'mcbench-lite');
  const install = (extra = {}) => run('-s', ['--', '--no-open'], { cwd: dir, env: { ...env, MCBENCH_REPO: repo, ...extra }, input: script });

  const first = await install();
  assert.equal(first.code, 0, first.stderr);
  assert.equal(first.stdout.split('\n').slice(0, 2).join('\n'),
    'Cloning mcbench-lite into ~/mcbench-lite\nTo start mcbench-lite again later, run: ~/mcbench-lite/start');
  assert.match(first.stdout, /\nfake server \[\] PORT=4321 /);
  assert.ok(fs.existsSync(path.join(target, 'server', 'index.js')));

  write(path.join(repo, 'server', 'index.js'), "console.log('updated server');\n");
  git('commit', '--quiet', '-am', 'update');
  const second = await install();
  assert.equal(second.code, 0, second.stderr);
  assert.equal(second.stdout, 'Updating mcbench-lite in ~/mcbench-lite\nTo start mcbench-lite again later, run: ~/mcbench-lite/start\nupdated server\n');

  const other = path.join(dir, 'other');
  fs.mkdirSync(other);
  const refused = await install({ MCBENCH_DIR: other });
  assert.equal(refused.code, 1);
  assert.equal(refused.stderr, `${other} already exists and is not a clone of ${repo}. Move it away, or set MCBENCH_DIR to another folder, and run this again.\n`);
  assert.deepEqual(fs.readdirSync(other), []);
});

// scripts/open-browser.js with `open` and `xdg-open` stubs that record the
// address they are given, against a server in this process that serves its
// page only to the data folder's token.
async function opener(t, serverPid, answer) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'open-browser-'));
  const bin = path.join(dir, 'bin');
  const opened = path.join(dir, 'opened');
  for (const name of ['open', 'xdg-open']) stub(bin, name, `echo "$1" > "${opened}"`);
  const data = path.join(dir, 'data');
  const seen = [];
  const server = http.createServer((req, res) => {
    seen.push(req.headers.authorization);
    const token = fs.readFileSync(path.join(data, 'auth-token'), 'utf8').trim();
    res.writeHead(answer && req.headers.authorization === `Bearer ${token}` ? 200 : 401);
    res.end();
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => {
    server.close();
    fs.rmSync(dir, { recursive: true, force: true });
  });
  const env = { HOME: path.join(dir, 'home'), PATH: `${bin}:/usr/bin:/bin`, MCBENCH_DATA_DIR: data, PORT: String(server.address().port) };
  const result = await new Promise((resolve) => {
    const child = spawn(process.execPath, [path.join(ROOT, 'scripts', 'open-browser.js'), String(serverPid)], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    child.stdout.on('data', (chunk) => { output += chunk; });
    child.stderr.on('data', (chunk) => { output += chunk; });
    child.on('close', (code) => resolve({ code, output }));
    if (!answer) setTimeout(() => process.kill(serverPid, 'SIGKILL'), 600);
  });
  // The stub runs detached: give it a moment to write.
  for (let i = 0; i < 50 && answer && !fs.existsSync(opened); i++) await new Promise((resolve) => setTimeout(resolve, 20));
  return { ...result, seen, opened: fs.existsSync(opened) ? fs.readFileSync(opened, 'utf8') : null, token: fs.readFileSync(path.join(data, 'auth-token'), 'utf8').trim(), port: server.address().port };
}

test('the browser is opened on the access link once the server serves its page to the token', async (t) => {
  const { code, output, opened, token, port } = await opener(t, process.pid, true);
  assert.equal(code, 0);
  assert.equal(output, '');
  assert.match(opened, new RegExp(`^http://mcbench-[0-9a-f]{10}\\.localhost:${port}/\\?token=${token}\\n$`));
});

test('no browser is opened when the server exits before it serves the page', async (t) => {
  const sleeper = spawn('/bin/sleep', ['30']);
  const { code, opened, seen } = await opener(t, sleeper.pid, false);
  assert.equal(code, 0);
  assert.equal(opened, null);
  assert.ok(seen.length > 0);
});
