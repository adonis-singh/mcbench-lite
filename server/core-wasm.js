'use strict';

// The Rust engine as WebAssembly (core/, scripts/build-wasm.js) for the
// page's live worlds: built once per process from the sources (cached by their
// hash, so a build is only made after core/ changed) and served under a name
// that holds the hash, so a browser keeps it for good.

const fs = require('fs');
const { build } = require('../scripts/build-wasm');
const { minecraftLive } = require('./minecraft-live');
const setup = require('./setup');

const PREFIX = '/core/';

// The module's build: `wasm.name` is one of
//   idle      no build was asked for
//   building  a build runs (`done`, its promise)
//   built     the module is made (`file`, `url`; `done`, resolved with them)
//   failed    the last build failed (`error`, why, and `at`, when): prepare()
//             builds again, and so does state() once RETRY_MS have passed
let wasm = { name: 'idle' };
// The page asks state() every second: a build that keeps failing is not run
// back to back.
const RETRY_MS = 30_000;

// Moves the build to the state `next` from one of the states `from`.
function move(from, next) {
  if (!from.includes(wasm.name)) throw new Error(`The engine module is ${wasm.name} and cannot become ${next.name}`);
  wasm = next;
}

// Starts the build (once while it runs or after it worked; again after it
// failed); resolves with { file, url }.
function prepare() {
  if (['building', 'built'].includes(wasm.name)) return wasm.done;
  // The engine is built from Minecraft's tables, made first. The stamp is
  // read in the build's own promise: a module without one has failed.
  const done = minecraftLive.minecraftTables().then(() => setup.track('wasm', () => build())).then((file) => {
    const hash = fs.readFileSync(`${file}.sources`, 'utf8').slice(0, 16);
    return { file, url: `${PREFIX}${hash}.wasm` };
  });
  move(['idle', 'failed'], { name: 'building', done });
  done.then(
    (module) => move(['building'], { name: 'built', done, ...module }),
    (error) => move(['building'], { name: 'failed', error: String(error.stderr || error.message || error), at: Date.now() }),
  );
  return done;
}

// What the page is told: the module's url once it is built, nothing while it
// builds, and an error if the last build failed. Being asked RETRY_MS or more
// after a failure is what builds again, so no failure lasts until the server
// restarts.
function state() {
  if (wasm.name === 'built') return { url: wasm.url };
  if (wasm.name !== 'failed') return {};
  const { error, at } = wasm;
  if (Date.now() - at >= RETRY_MS) prepare().catch(() => {});
  return { error: `The live world's engine could not be built: ${error.slice(-600)}` };
}

// Serves the module at its url; false when the path is not the module's.
async function serve(req, res, pathname) {
  if (!pathname.startsWith(PREFIX) || !pathname.endsWith('.wasm')) return false;
  const module = await prepare();
  if (pathname !== module.url) {
    res.writeHead(404, { 'content-type': 'text/plain' });
    res.end('Not found');
    return true;
  }
  const bytes = fs.readFileSync(module.file);
  res.writeHead(200, { 'content-type': 'application/wasm', 'content-length': bytes.length, 'cache-control': 'public, max-age=31536000, immutable' });
  res.end(req.method === 'HEAD' ? undefined : bytes);
  return true;
}

module.exports = { prepare, serve, state, PREFIX };
