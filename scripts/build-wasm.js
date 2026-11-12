'use strict';

// Builds the Rust engine (core/) into the WebAssembly module the browser
// runs, core/target/wasm32-unknown-unknown/wasm/mcbench_core.wasm, unless it
// is built already from these sources. The module is not committed: it is
// made from core/'s sources, which change with every port, and cached by
// their hash like the native program.
//
// It is built with the toolchain core/rust-toolchain.toml pins, which holds
// the wasm32-unknown-unknown target (server/engine.js: installRust, rust).
//
//   node scripts/build-wasm.js [--jmath intel|fdlibm]
//
// --jmath forces Java's Math.sin and Math.cos flavour (core/src/jmath.rs);
// the module defaults to Intel's.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { installRust, rust, withLock, CORE } = require('../server/engine');
const { step } = require('../server/progress');
const { checkTables } = require('../server/minecraft-tables');
const { MC_VERSION } = require('../server/assets');

const execFileAsync = promisify(execFile);

const TARGET = 'wasm32-unknown-unknown';

function sourcesHash(features) {
  const files = ['Cargo.toml', 'Cargo.lock', '.cargo/config.toml', 'rust-toolchain.toml'];
  for (const root of ['src', 'minecraft']) {
    (function walk(relative) {
      for (const entry of fs.readdirSync(path.join(CORE, relative), { withFileTypes: true })) {
        const name = path.posix.join(relative, entry.name);
        if (entry.isDirectory()) walk(name);
        else files.push(name);
      }
    })(root);
  }
  const hash = crypto.createHash('sha256').update(`${features}\0`);
  for (const name of files.sort()) hash.update(`${name}\0`).update(fs.readFileSync(path.join(CORE, name))).update('\0');
  return hash.digest('hex');
}

// Builds the module (if its sources changed) and resolves with its path.
// `jmath` is 'intel' or 'fdlibm'; unset, the module's own default (Intel's).
async function build({ jmath = null } = {}) {
  checkTables(MC_VERSION);
  const features = jmath ? `jmath-${jmath}` : '';
  const folder = path.join(CORE, 'target', jmath ? `wasm-${jmath}` : 'wasm-default');
  const module = path.join(folder, 'mcbench-core.wasm');
  const stamp = `${module}.sources`;
  const hash = sourcesHash(features);
  const fresh = () => fs.existsSync(module) && fs.existsSync(stamp) && fs.readFileSync(stamp, 'utf8') === hash;
  if (fresh()) return module;
  fs.mkdirSync(folder, { recursive: true });
  await installRust();
  // Test files build at once: one process builds while the others wait for
  // it, then find the module fresh.
  await withLock(`${folder}.lock`, async () => {
    if (fresh()) return;
    // cargo's warnings are the port's own (shown by the native build); errors fail the build.
    const { cargo, env } = rust();
    await step('[engine] building the live world\'s WebAssembly engine (the first time takes a few minutes)', '[engine] WebAssembly engine built',
      () => execFileAsync(cargo, ['rustc', '--lib', '--crate-type', 'cdylib', '--profile', 'wasm', '--target', TARGET, '--target-dir', folder, '--quiet', ...(features ? ['--features', features] : [])], { cwd: CORE, maxBuffer: 1 << 26, env }));
    // The module appears whole (a rename), never half copied.
    const copy = `${module}.${process.pid}`;
    fs.copyFileSync(path.join(folder, TARGET, 'wasm', 'mcbench_core.wasm'), copy);
    fs.renameSync(copy, module);
    fs.writeFileSync(stamp, hash);
  });
  return module;
}

if (require.main === module) {
  const at = process.argv.indexOf('--jmath');
  build({ jmath: at < 0 ? null : process.argv[at + 1] }).then((module) => {
    console.log(`${path.relative(process.cwd(), module)}: ${fs.statSync(module).size} bytes`);
  }, (error) => {
    console.error(error.stderr || error);
    process.exit(1);
  });
}

module.exports = { build, sourcesHash };
