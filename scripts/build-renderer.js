'use strict';

// Builds the browser's mesher (renderer/, Rust) into the WebAssembly module
// the app ships, renderer/mcbench-renderer.wasm, and records the hash of
// the sources it was built from beside it. The module is committed rather
// than built on each machine: it holds no Minecraft data (that is the baked
// pack, made on the machine), so the app needs no Rust toolchain, and
// test/mesher.test.js fails when the sources change without a rebuild.
//
//   node scripts/build-renderer.js     (cargo on PATH, with the wasm32-unknown-unknown target)

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const RENDERER = path.join(__dirname, '..', 'renderer');
const MODULE = path.join(RENDERER, 'mcbench-renderer.wasm');
const SOURCES_HASH = path.join(RENDERER, 'mcbench-renderer.sources');

// Everything cargo builds the module from.
function sourcesHash() {
  const files = ['Cargo.toml', 'Cargo.lock'];
  (function walk(relative) {
    for (const entry of fs.readdirSync(path.join(RENDERER, relative), { withFileTypes: true })) {
      const name = path.posix.join(relative, entry.name);
      if (entry.isDirectory()) walk(name);
      else files.push(name);
    }
  })('src');
  const hash = crypto.createHash('sha256');
  for (const name of files.sort()) hash.update(`${name}\0`).update(fs.readFileSync(path.join(RENDERER, name))).update('\0');
  return hash.digest('hex');
}

function build() {
  execFileSync('cargo', ['build', '--release', '--target', 'wasm32-unknown-unknown'], { cwd: RENDERER, stdio: 'inherit' });
  fs.copyFileSync(path.join(RENDERER, 'target', 'wasm32-unknown-unknown', 'release', 'mcbench_renderer.wasm'), MODULE);
  fs.writeFileSync(SOURCES_HASH, `${sourcesHash()}\n`);
}

if (require.main === module) {
  build();
  console.log(`${path.relative(process.cwd(), MODULE)}: ${fs.statSync(MODULE).size} bytes`);
}

module.exports = { MODULE, SOURCES_HASH, sourcesHash, build };
