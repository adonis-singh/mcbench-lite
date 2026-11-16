'use strict';

// The Rust engine as WebAssembly (core/) for the page's live worlds: the
// module released for this checkout's sources (engine.js installEngine), and
// the two tables of Minecraft it reads, blocks.txt and loot.json, from the
// tables folder this machine made (minecraft-tables.js). The page fetches
// the three from one address that holds the module's version and the
// tables' hash, /core/<version>-<tables hash>/, so a browser keeps them for
// good, and hands the tables to each instance before it starts
// (web/core-world.mjs).

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { installEngine, tablesFolder } = require('./engine');
const { minecraftLive } = require('./minecraft-live');

const PREFIX = '/core/';
// The files of an address, and where each is read from.
const FILES = {
  'engine.wasm': { type: 'application/wasm', file: (engine) => engine.module },
  'blocks.txt': { type: 'text/plain; charset=utf-8', file: () => path.join(tablesFolder(), 'blocks.txt') },
  'loot.json': { type: 'application/json', file: () => path.join(tablesFolder(), 'loot.json') },
};

// The engine module and its tables: `wasm.name` is one of
//   idle       nothing was asked for
//   preparing  the module is installed and the tables made (`done`, its promise)
//   ready      both are there (`engine`, `url`; `done`, resolved with them)
//   failed     the last try failed (`error`, why, and `at`, when): prepare()
//              tries again, and so does state() once RETRY_MS have passed
let wasm = { name: 'idle' };
// The page asks state() every second: a try that keeps failing is not run
// back to back.
const RETRY_MS = 30_000;

// Moves to the state `next` from one of the states `from`.
function move(from, next) {
  if (!from.includes(wasm.name)) throw new Error(`The engine module is ${wasm.name} and cannot become ${next.name}`);
  wasm = next;
}

// The first 16 hex digits of the sha256 of blocks.txt and loot.json, read
// without blocking.
async function tablesHash() {
  const hash = crypto.createHash('sha256');
  for (const name of ['blocks.txt', 'loot.json']) hash.update(`${name}\0`).update(await fs.promises.readFile(FILES[name].file())).update('\0');
  return hash.digest('hex').slice(0, 16);
}

// Installs the module and makes the tables (once while that runs or after it
// worked; again after it failed); resolves with { engine, url }.
function prepare() {
  if (['preparing', 'ready'].includes(wasm.name)) return wasm.done;
  const done = Promise.all([installEngine(), minecraftLive.minecraftTables()]).then(async ([engine]) => {
    const version = path.basename(path.dirname(engine.module)).replace(/^engine-wasm-/, '');
    return { engine, url: `${PREFIX}${version}-${await tablesHash()}/` };
  });
  move(['idle', 'failed'], { name: 'preparing', done });
  done.then(
    (module) => move(['preparing'], { name: 'ready', done, ...module }),
    (error) => move(['preparing'], { name: 'failed', error: String(error.stderr || error.message || error), at: Date.now() }),
  );
  return done;
}

// What the page is told: the engine's address once it is ready, nothing
// while it is prepared, and an error if the last try failed. Being asked
// RETRY_MS or more after a failure is what tries again, so no failure lasts
// until the server restarts.
function state() {
  if (wasm.name === 'ready') return { url: wasm.url };
  if (wasm.name !== 'failed') return {};
  const { error, at } = wasm;
  if (Date.now() - at >= RETRY_MS) prepare().catch(() => {});
  return { error: `The live world's engine could not be prepared: ${error.slice(-600)}` };
}

// Serves a file of the engine's address; false when the path is not under
// PREFIX.
async function serve(req, res, pathname) {
  if (!pathname.startsWith(PREFIX)) return false;
  const { engine, url } = await prepare();
  const name = pathname.slice(url.length);
  if (!pathname.startsWith(url) || !Object.hasOwn(FILES, name)) {
    res.writeHead(404, { 'content-type': 'text/plain' });
    res.end('Not found');
    return true;
  }
  const bytes = await fs.promises.readFile(FILES[name].file(engine));
  res.writeHead(200, { 'content-type': FILES[name].type, 'content-length': bytes.length, 'cache-control': 'private, max-age=31536000, immutable' });
  res.end(req.method === 'HEAD' ? undefined : bytes);
  return true;
}

module.exports = { prepare, serve, state, PREFIX };
