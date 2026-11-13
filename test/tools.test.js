'use strict';

// The app's own tools (server/tools.js): pinned downloads, checked against
// their sha256, unpacked into a folder named after their version that
// appears whole. Every download here comes from a server on loopback, and
// every file is written in a temporary folder.
const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const { execFileSync } = require('node:child_process');

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-tools-'));
process.env.MCBENCH_TOOLS_DIR = path.join(scratch, 'default-tools');
after(() => fs.rmSync(scratch, { recursive: true, force: true }));

const tools = require('../server/tools');
const { zipNames, extractZip } = require('../server/zip-entry');

const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

// A download server on loopback: `files` maps a path to its bytes, and
// `requests` lists the paths asked for.
async function serve(t, files) {
  const requests = [];
  const server = http.createServer((req, res) => {
    requests.push(req.url);
    const bytes = files[req.url];
    if (!bytes) return res.writeHead(404).end();
    res.writeHead(200, { 'content-length': bytes.length });
    // In two writes, so progress is told more than once.
    res.write(bytes.subarray(0, bytes.length >> 1));
    setImmediate(() => res.end(bytes.subarray(bytes.length >> 1)));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  return { url: `http://127.0.0.1:${server.address().port}`, requests };
}

// Pins of one program, the same download for every platform.
function pins(name, { url, sha, unpack, bin }) {
  const platform = { url, sha256: sha, bin };
  return { [name]: { title: name, version: '1.2.3', mb: 1, unpack, platforms: { [tools.PLATFORM]: platform } } };
}

// A zip of `entries` ({ name, data, mode }), stored or deflated, with each
// entry's Unix mode, as Chrome for Testing's zips carry them.
function zip(entries) {
  const locals = [];
  const central = [];
  let offset = 0;
  for (const { name, data = Buffer.alloc(0), mode, deflate = false } of entries) {
    const body = deflate ? zlib.deflateRawSync(data) : data;
    const nameBytes = Buffer.from(name);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0);
    local.writeUInt16LE(deflate ? 8 : 0, 8);
    local.writeUInt32LE(body.length, 18);
    local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBytes.length, 26);
    locals.push(local, nameBytes, body);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt8(3, 5); // made on Unix
    entry.writeUInt16LE(deflate ? 8 : 0, 10);
    entry.writeUInt32LE(body.length, 20);
    entry.writeUInt32LE(data.length, 24);
    entry.writeUInt16LE(nameBytes.length, 28);
    entry.writeUInt32LE((mode << 16) >>> 0, 38);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, nameBytes);
    offset += 30 + nameBytes.length + body.length;
  }
  const directory = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0);
  end.writeUInt16LE(entries.length, 8);
  end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(directory.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, directory, end]);
}

test('every program is pinned for the four platforms, by address and sha256, and Chromium is the build playwright-core names', () => {
  for (const [name, pin] of Object.entries(tools.PINS)) {
    assert.deepEqual(Object.keys(pin.platforms).sort(), ['darwin-arm64', 'darwin-x64', 'linux-arm64', 'linux-x64'], name);
    for (const [platform, { url, sha256: hash, bin }] of Object.entries(pin.platforms)) {
      assert.match(url, /^https:\/\//, `${name} ${platform}`);
      assert.match(hash, /^[0-9a-f]{64}$/, `${name} ${platform}`);
      assert.ok(bin, `${name} ${platform}`);
    }
  }
  const browsers = JSON.parse(fs.readFileSync(path.join(path.dirname(require.resolve('playwright-core')), 'browsers.json'), 'utf8')).browsers;
  const shell = browsers.find((browser) => browser.name === 'chromium-headless-shell');
  assert.equal(tools.PINS.chromium.version, shell.browserVersion);
  assert.ok(tools.CHROMIUM_SHA256[shell.browserVersion], `Chromium ${shell.browserVersion} has no pinned checksums: pin them in server/tools.js`);
});

test('the JDK, rustup and the default tools folder are the tools folder\'s, never the system\'s', () => {
  assert.equal(tools.TOOLS_DIR, path.join(scratch, 'default-tools'));
  assert.ok(tools.JAVA.startsWith(path.join(tools.TOOLS_DIR, 'jdk-25.0.5+8') + path.sep), tools.JAVA);
  assert.ok(tools.JAVAC.endsWith(`${path.sep}bin${path.sep}javac`), tools.JAVAC);
  assert.equal(tools.RUSTUP, path.join(tools.TOOLS_DIR, 'rust', 'cargo', 'bin', 'rustup'));
  assert.deepEqual(tools.RUST_ENV, { RUSTUP_HOME: path.join(tools.TOOLS_DIR, 'rust', 'rustup'), CARGO_HOME: path.join(tools.TOOLS_DIR, 'rust', 'cargo') });
});

test('a download is kept once its sha256 matches, and its progress is told in bytes', async (t) => {
  const bytes = crypto.randomBytes(200_000);
  const { url } = await serve(t, { '/file': bytes });
  const target = path.join(scratch, 'download', 'file');
  const progress = [];
  await tools.download(`${url}/file`, target, { sha256: sha256(bytes), onProgress: (step) => progress.push(step) });
  assert.deepEqual(fs.readFileSync(target), bytes);
  assert.ok(progress.length >= 2);
  assert.deepEqual(progress.at(-1), { done: bytes.length, total: bytes.length });
  assert.deepEqual(fs.readdirSync(path.dirname(target)), ['file'], 'no temporary file is left');
});

test('a download whose sha256 differs fails, naming both, and nothing of it is kept', async (t) => {
  const { url } = await serve(t, { '/file': Buffer.from('tampered') });
  const folder = path.join(scratch, 'tampered');
  const expected = sha256(Buffer.from('the pinned bytes'));
  await assert.rejects(tools.download(`${url}/file`, path.join(folder, 'file'), { sha256: expected }),
    { message: `${url}/file downloaded with sha256 ${sha256(Buffer.from('tampered'))}, but mcbench-lite expects ${expected}. It was not kept.` });
  assert.deepEqual(fs.readdirSync(folder), []);
});

test('a download the server refuses fails with its status', async (t) => {
  const { url } = await serve(t, {});
  await assert.rejects(tools.download(`${url}/missing`, path.join(scratch, 'refused', 'file'), { sha256: '0'.repeat(64) }), /HTTP 404/);
});

// A loopback server that answers the first `failures.length` requests with
// failure `failures[i]` (drop: the connection closes at once; short: half of
// the body, then the connection closes; a number: that status), then `bytes`.
async function flaky(t, failures, bytes) {
  const requests = [];
  const server = http.createServer((req, res) => {
    const failure = failures[requests.length];
    requests.push(req.url);
    if (failure === 'drop') return req.socket.destroy();
    if (failure === 'short') {
      res.writeHead(200, { 'content-length': bytes.length });
      return res.write(bytes.subarray(0, bytes.length >> 1), () => req.socket.destroy());
    }
    if (typeof failure === 'number') return res.writeHead(failure).end();
    res.writeHead(200, { 'content-length': bytes.length }).end(bytes);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  return { url: `http://127.0.0.1:${server.address().port}/file`, requests };
}

test('a download that fails on the network is tried again, three times in all', async (t) => {
  const bytes = crypto.randomBytes(100_000);
  const { url, requests } = await flaky(t, ['drop', 'short'], bytes);
  const target = path.join(scratch, 'flaky', 'file');
  await tools.download(url, target, { sha256: sha256(bytes) });
  assert.deepEqual(fs.readFileSync(target), bytes);
  assert.equal(requests.length, 3);
  assert.deepEqual(fs.readdirSync(path.dirname(target)), ['file'], 'no temporary file is left');
});

test('a server error is tried again, and the third failure fails the download, saying so', async (t) => {
  const { url, requests } = await flaky(t, [503, 502, 500], Buffer.from('unused'));
  const folder = path.join(scratch, 'server-error');
  await assert.rejects(tools.download(url, path.join(folder, 'file'), { sha256: '0'.repeat(64) }),
    { message: `The download of ${url} failed: HTTP 500 (tried 3 times)` });
  assert.equal(requests.length, 3);
  assert.ok(!fs.existsSync(folder), 'nothing is written for an answer that is not the file');
});

test('a checksum that differs and a refused download are not tried again', async (t) => {
  const { url, requests } = await flaky(t, [], Buffer.from('tampered'));
  await assert.rejects(tools.download(url, path.join(scratch, 'once', 'file'), { sha256: '0'.repeat(64) }), /expects 0{64}\. It was not kept\./);
  assert.equal(requests.length, 1);
  const refused = await flaky(t, [404], Buffer.from('unused'));
  await assert.rejects(tools.download(refused.url, path.join(scratch, 'once', 'refused'), { sha256: '0'.repeat(64) }), /HTTP 404/);
  assert.equal(refused.requests.length, 1);
});

test('a single-file program is installed executable into its version\'s folder, and once', async (t) => {
  const bytes = Buffer.from('#!/bin/sh\necho installed\n');
  const { url, requests } = await serve(t, { '/program': bytes });
  const dir = path.join(scratch, 'file-tools');
  const pinned = pins('program', { url: `${url}/program`, sha: sha256(bytes), unpack: 'file', bin: 'program' });
  const program = await tools.install('program', { pins: pinned, dir });
  assert.equal(program, path.join(dir, 'program-1.2.3', 'program'));
  assert.equal(execFileSync(program, { encoding: 'utf8' }), 'installed\n');
  assert.equal(await tools.install('program', { pins: pinned, dir }), program);
  assert.deepEqual(requests, ['/program'], 'an installed program is not downloaded again');
  assert.deepEqual(fs.readdirSync(dir).filter((name) => !name.endsWith('.lock')), ['program-1.2.3']);
});

test('a .tar.gz is unpacked into the version\'s folder, and the program is found at its pinned path', async (t) => {
  const source = path.join(scratch, 'tar-source');
  fs.mkdirSync(path.join(source, 'package', 'bin'), { recursive: true });
  fs.writeFileSync(path.join(source, 'package', 'bin', 'tool'), '#!/bin/sh\necho from tar\n', { mode: 0o755 });
  const archive = path.join(scratch, 'tool.tar.gz');
  execFileSync('tar', ['-czf', archive, '-C', source, 'package']);
  const bytes = fs.readFileSync(archive);
  const { url } = await serve(t, { '/tool.tar.gz': bytes });
  const program = await tools.install('tool', { pins: pins('tool', { url: `${url}/tool.tar.gz`, sha: sha256(bytes), unpack: 'tar', bin: 'package/bin/tool' }), dir: path.join(scratch, 'tar-tools') });
  assert.equal(execFileSync(program, { encoding: 'utf8' }), 'from tar\n');
});

test('an install whose download fails its checksum leaves no folder, so the next start downloads again', async (t) => {
  const { url, requests } = await serve(t, { '/program': Buffer.from('tampered') });
  const dir = path.join(scratch, 'failed-tools');
  const pinned = pins('program', { url: `${url}/program`, sha: sha256(Buffer.from('pinned')), unpack: 'file', bin: 'program' });
  await assert.rejects(tools.install('program', { pins: pinned, dir }), /It was not kept/);
  await assert.rejects(tools.install('program', { pins: pinned, dir }), /It was not kept/);
  assert.equal(requests.length, 2);
  assert.deepEqual(fs.readdirSync(dir).filter((name) => !name.endsWith('.lock')), []);
});

test('a program with no pin for this platform fails, naming the platform', async () => {
  const pinned = { program: { title: 'A program', version: '1', mb: 1, unpack: 'file', platforms: {} } };
  await assert.rejects(tools.install('program', { pins: pinned, dir: path.join(scratch, 'no-platform') }), { message: `mcbench-lite has no pinned A program for ${tools.PLATFORM}` });
});

test('a zip is listed in its central directory\'s order and unpacked with its files\' modes, its folders and its symbolic links, stored or deflated', async () => {
  const archive = path.join(scratch, 'shell.zip');
  fs.writeFileSync(archive, zip([
    { name: 'chrome/', mode: 0o40755 },
    { name: 'chrome/headless_shell', data: Buffer.from('#!/bin/sh\necho shell\n'), mode: 0o100755, deflate: true },
    { name: 'chrome/resources.pak', data: Buffer.alloc(50_000, 7), mode: 0o100644, deflate: true },
    { name: 'chrome/empty', mode: 0o100644 },
    { name: 'chrome/lib/libGLESv2.so', data: Buffer.from('library'), mode: 0o100644 },
    { name: 'chrome/libGLESv2.so', data: Buffer.from('lib/libGLESv2.so'), mode: 0o120777 },
  ]));
  assert.deepEqual(zipNames(archive), ['chrome/', 'chrome/headless_shell', 'chrome/resources.pak', 'chrome/empty', 'chrome/lib/libGLESv2.so', 'chrome/libGLESv2.so']);
  const folder = path.join(scratch, 'unzipped');
  await extractZip(archive, folder);
  assert.equal(execFileSync(path.join(folder, 'chrome', 'headless_shell'), { encoding: 'utf8' }), 'shell\n');
  assert.equal(fs.statSync(path.join(folder, 'chrome', 'resources.pak')).mode & 0o777, 0o644);
  assert.deepEqual(fs.readFileSync(path.join(folder, 'chrome', 'resources.pak')), Buffer.alloc(50_000, 7));
  assert.equal(fs.readFileSync(path.join(folder, 'chrome', 'empty'), 'utf8'), '');
  assert.equal(fs.readlinkSync(path.join(folder, 'chrome', 'libGLESv2.so')), 'lib/libGLESv2.so');
  assert.equal(fs.readFileSync(path.join(folder, 'chrome', 'libGLESv2.so'), 'utf8'), 'library');
});

test('a CLI on PATH is the one the app runs; without one, the tools folder\'s', (t) => {
  const bin = path.join(scratch, 'path-bin');
  fs.mkdirSync(bin, { recursive: true });
  const previous = process.env.PATH;
  t.after(() => { process.env.PATH = previous; });
  process.env.PATH = bin;
  assert.equal(tools.cliProgram('claude'), tools.installed('claude').program);
  fs.writeFileSync(path.join(bin, 'claude'), '#!/bin/sh\n', { mode: 0o755 });
  assert.equal(tools.cliProgram('claude'), 'claude');
  assert.equal(tools.cliProgram('codex'), path.join(tools.TOOLS_DIR, 'codex-0.167.1', 'bin', 'codex'));
});

// The headless shell is checked to start (checkShell). Fakes stand in for
// it and for ldd: a shell that cannot load a library, as on a server Linux.
function fakeProgram(dir, name, script) {
  const file = path.join(dir, name);
  fs.writeFileSync(file, `#!/bin/sh\n${script}\n`, { mode: 0o755 });
  return file;
}

test('a headless shell that starts passes the check', async () => {
  const dir = fs.mkdtempSync(path.join(scratch, 'shell-ok-'));
  await tools.checkShell(fakeProgram(dir, 'headless_shell', 'echo "Chromium 149.0.7827.55"'), { platform: 'linux' });
});

test('a shell without its shared libraries fails naming them, with the command that installs them', async () => {
  const dir = fs.mkdtempSync(path.join(scratch, 'shell-libs-'));
  const shell = fakeProgram(dir, 'headless_shell', 'echo "headless_shell: error while loading shared libraries: libglib-2.0.so.0: cannot open shared object file: No such file or directory" >&2; exit 127');
  const ldd = fakeProgram(dir, 'ldd', [
    "echo '\tlinux-vdso.so.1 (0x0000ffff)'",
    "echo '\tlibglib-2.0.so.0 => not found'",
    "echo '\tlibnss3.so => not found'",
    "echo '\tlibglib-2.0.so.0 => not found'",
    "echo '\tlibc.so.6 => /lib/aarch64-linux-gnu/libc.so.6 (0x0000ffff)'",
  ].join('\n'));
  const cli = path.join(path.dirname(require.resolve('playwright-core')), 'cli.js');
  const sudo = process.getuid() === 0 ? '' : 'sudo ';
  await assert.rejects(tools.checkShell(shell, { platform: 'linux', ldd, aptGet: ldd }), {
    message: `The headless browser cannot start: this system does not have the shared libraries libglib-2.0.so.0, libnss3.so. Install them with this command, then try again:\n${sudo}${process.execPath} ${cli} install-deps chromium`,
  });
  // Without apt-get the libraries are still named.
  await assert.rejects(tools.checkShell(shell, { platform: 'linux', ldd, aptGet: path.join(dir, 'no-apt-get') }),
    { message: "The headless browser cannot start: this system does not have the shared libraries libglib-2.0.so.0, libnss3.so. Install them with this system's package manager, then try again." });
});

test('a shell that fails for another reason says what it printed', async () => {
  const dir = fs.mkdtempSync(path.join(scratch, 'shell-other-'));
  const shell = fakeProgram(dir, 'headless_shell', 'echo "Illegal instruction" >&2; exit 132');
  const ldd = fakeProgram(dir, 'ldd', "echo '\tlibc.so.6 => /lib/libc.so.6 (0x0)'");
  await assert.rejects(tools.checkShell(shell, { platform: 'linux', ldd }), { message: `The headless browser ${shell} does not start: Illegal instruction` });
});
