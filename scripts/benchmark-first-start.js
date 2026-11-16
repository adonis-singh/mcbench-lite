'use strict';

// Times a first start: the server of a `git archive` copy of this checkout
// (with this checkout's node_modules) on an empty tools folder and an empty
// data folder, with the engine's release assets served from a folder on
// loopback (MCBENCH_ENGINE_RELEASES, server/tools.js) instead of GitHub, as
// before a release is published. Everything else downloads from where the
// app downloads it. The copy has no Minecraft tables, so the start makes
// them as a person's first start does.
//
//   node scripts/benchmark-first-start.js --assets <folder> --work <empty folder> [--port 3150] [--keep]
//
// <folder> holds the assets scripts/release-engine.js built and pinned. It
// prints each terminal line with the seconds since the server was started,
// each setup step's state and seconds (GET /api/setup) once it says
// `[startup] ready`, and then checks the engine end to end: a build placed
// and ticked by the downloaded program (server/engine.js coreEngine in the
// copy), and the same build placed and ticked by the page's engine module in
// the headless shell the start installed, its tables fetched from the
// server; the two must give the same bytes. --keep leaves the server running
// (npm run test:ui can be pointed at it) until Ctrl-C.

const fs = require('fs');
const http = require('http');
const path = require('path');
const { execFileSync, execSync, spawn } = require('child_process');

const ROOT = path.join(__dirname, '..');
const RELEASE = require('../server/engine-release.json');

function option(name, fallback) {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
}

const COMMANDS = ['fill 0 63 0 6 63 2 minecraft:stone', 'setblock 1 64 1 minecraft:lever[face=floor,facing=north,powered=false]', 'setblock 2 64 1 minecraft:redstone_wire',
  'setblock 3 64 1 minecraft:repeater[facing=west,delay=2]', 'setblock 4 64 1 minecraft:redstone_lamp', 'setblock 5 64 1 minecraft:piston[facing=up]'];

// The assets' folder at /<tag>/<name>, as GitHub serves a release's assets.
async function serveAssets(folder) {
  const server = http.createServer((req, res) => {
    const name = Object.values(RELEASE.assets).map((asset) => asset.name).find((n) => req.url === `/${RELEASE.tag}/${n}`);
    if (!name) return res.writeHead(404).end();
    const bytes = fs.readFileSync(path.join(folder, name));
    res.writeHead(200, { 'content-length': bytes.length }).end(bytes);
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  return { server, url: `http://127.0.0.1:${server.address().port}` };
}

async function main() {
  const assets = path.resolve(option('assets', ''));
  const work = path.resolve(option('work', ''));
  const port = Number(option('port', 3150));
  if (!option('assets', null) || !option('work', null)) throw new Error('usage: node scripts/benchmark-first-start.js --assets <folder> --work <empty folder> [--port 3150] [--keep]');
  if (port < 3140 || port > 3199) throw new Error('--port is one of 3140 to 3199');
  if (fs.existsSync(work) && fs.readdirSync(work).length) throw new Error(`${work} is not empty`);
  const [app, tools, data] = ['app', 'tools', 'data'].map((name) => path.join(work, name));
  for (const folder of [app, tools, data]) fs.mkdirSync(folder, { recursive: true });
  execSync(`git archive HEAD | tar -x -C ${JSON.stringify(app)}`, { cwd: ROOT });
  execFileSync('cp', [...(process.platform === 'darwin' ? ['-c'] : []), '-R', path.join(ROOT, 'node_modules'), app]);
  const { server: assetServer, url } = await serveAssets(assets);

  const env = {
    ...process.env, PORT: String(port), MCBENCH_TOOLS_DIR: tools, MCBENCH_DATA_DIR: data, MCBENCH_SETTINGS_FILE: path.join(data, 'settings.json'), MCBENCH_ENGINE_RELEASES: url,
  };
  const started = Date.now();
  const seconds = () => ((Date.now() - started) / 1000).toFixed(1).padStart(6);
  const child = spawn(process.execPath, ['server/index.js'], { cwd: app, env, stdio: ['ignore', 'pipe', 'pipe'] });
  const ended = new Promise((resolve) => child.once('close', resolve));
  const outcome = await new Promise((resolve, reject) => {
    let buffer = '';
    const read = (chunk) => {
      buffer += chunk;
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const line of lines) {
        console.log(`${seconds()} s  ${line.replace(/token=[0-9a-f]+/, 'token=…')}`);
        if (line.startsWith('[startup] ready')) resolve('ready');
        if (line.startsWith('[startup] not everything')) resolve('not ready');
      }
    };
    child.stdout.on('data', read);
    child.stderr.on('data', read);
    child.once('close', (code) => reject(new Error(`The server exited (code ${code}) before it was ready`)));
  });
  const readyAt = (Date.now() - started) / 1000;
  const token = fs.readFileSync(path.join(data, 'auth-token'), 'utf8').trim();
  const api = async (pathname) => (await fetch(`http://127.0.0.1:${port}${pathname}`, { headers: { authorization: `Bearer ${token}` } })).json();
  console.log(`\n${outcome} after ${readyAt.toFixed(1)} s`);
  for (const step of (await api('/api/setup')).steps) console.log(`  ${step.id.padEnd(10)} ${step.state.padEnd(11)} ${step.seconds ?? ''}${step.error ? `  ${step.error.slice(0, 300)}` : ''}`);

  try {
    // The downloaded program, through the copy's own server code.
    const native = JSON.parse(execFileSync(process.execPath, ['-e', `
      const fs = require('fs'); const path = require('path'); const os = require('os');
      const { coreEngine, installEngine } = require('./server/engine');
      (async () => {
        const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-first-start-'));
        const engine = coreEngine();
        await engine.warm({ folder });
        fs.writeFileSync(path.join(folder, 'c.txt'), ${JSON.stringify(COMMANDS.map((line) => `${line}\n`).join(''))});
        await engine.place(path.join(folder, 'c.txt'), path.join(folder, 'b.bin'));
        await engine.input('toggle', 1, 64, 1, 0);
        await engine.tick(8, path.join(folder, 'p.bin'), path.join(folder, 'e.json'));
        const hex = (f) => require('crypto').createHash('sha256').update(fs.readFileSync(path.join(folder, f))).digest('hex').slice(0, 16);
        console.log(JSON.stringify({ program: (await installEngine()).program, blueprint: hex('b.bin'), patch: hex('p.bin') }));
        await engine.stop();
      })();`], { cwd: app, env, encoding: 'utf8' }));
    console.log(`\nthe downloaded program ${native.program}: blueprint ${native.blueprint}, patch after 8 ticks ${native.patch}`);

    // The page's engine in the headless shell the start installed.
    const { chromium } = require(path.join(app, 'node_modules', 'playwright-core'));
    const shell = execFileSync(process.execPath, ['-e', "require('./server/tools').headlessShell().then((p) => console.log(p))"], { cwd: app, env, encoding: 'utf8' }).trim().split('\n').at(-1);
    const browser = await chromium.launch({ executablePath: shell });
    try {
      const page = await browser.newPage();
      const host = execFileSync(process.execPath, ['-e', `console.log(require('./server/auth').link(${port}))`], { cwd: app, env, encoding: 'utf8' }).trim();
      await page.goto(host, { waitUntil: 'domcontentloaded' });
      const fetched = [];
      page.on('response', (response) => { if (new URL(response.url()).pathname.startsWith('/core/')) fetched.push(`${new URL(response.url()).pathname} ${response.status()}`); });
      const inPage = await page.evaluate(async (commands) => {
        const { CoreEngine } = await import('/core-engine.mjs');
        const status = await (await fetch('/api/minecraft/status')).json();
        const engine = await CoreEngine.start(status.core);
        const hex = async (bytes) => [...new Uint8Array(await crypto.subtle.digest('SHA-256', bytes))].map((b) => b.toString(16).padStart(2, '0')).join('').slice(0, 16);
        const placed = await engine.place(commands);
        await engine.input('toggle', 1, 64, 1, 0);
        const ticked = await engine.tick(8);
        engine.terminate();
        return { core: status.core, blueprint: await hex(placed.bytes), patch: await hex(ticked.bytes) };
      }, COMMANDS.map((line) => `${line}\n`).join(''));
      console.log(`the page's engine ${inPage.core}: blueprint ${inPage.blueprint}, patch after 8 ticks ${inPage.patch}; fetched ${fetched.join(', ')}`);
      if (inPage.blueprint !== native.blueprint || inPage.patch !== native.patch) throw new Error('The page\'s engine and the program differ');
      console.log('the page\'s engine and the downloaded program give the same bytes');
    } finally {
      await browser.close();
    }
  } finally {
    assetServer.close();
    if (process.argv.includes('--keep')) {
      console.log(`\nThe server stays on port ${port} (data folder ${data}); Ctrl-C ends it.`);
      process.once('SIGINT', () => child.kill('SIGINT'));
    } else {
      child.kill('SIGINT');
    }
    await ended;
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
