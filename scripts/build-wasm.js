'use strict';

// Builds the Rust engine (core/) from its sources into a WebAssembly module,
// <target>/wasm-default/mcbench-core.wasm (<target> is CARGO_TARGET_DIR, or
// core/target), unless it is built already from these sources. The app runs
// the module released for its sources (server/engine.js installEngine);
// this builds it for a developer (scripts/build-engine.js), for the releases
// (scripts/release-engine.js), and with Java's Math.sin and Math.cos forced
// for the parity scripts.
//
// It is built with the toolchain core/rust-toolchain.toml pins, which holds
// the wasm32-unknown-unknown target (server/engine.js: installRust, rust).
//
//   node scripts/build-wasm.js [--jmath intel|fdlibm]
//
// --jmath forces Java's Math.sin and Math.cos flavour (core/src/jmath.rs);
// the module defaults to Intel's.

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { installRust, rust, sourcesHash, CORE } = require('../server/engine');
const { withLock } = require('../server/pid-lock');
const { step } = require('../server/progress');

const execFileAsync = promisify(execFile);

const TARGET = 'wasm32-unknown-unknown';

// cargo's arguments for the module, with `features`.
const wasmBuildArgs = (features) => ['rustc', '--lib', '--crate-type', 'cdylib', '--profile', 'wasm', '--target', TARGET, '--quiet', ...(features ? ['--features', features] : [])];

// Builds the module (if its sources changed) and resolves with its path.
// `jmath` is 'intel' or 'fdlibm'; unset, the module's own default (Intel's).
async function build({ jmath = null } = {}) {
  const features = jmath ? `jmath-${jmath}` : '';
  const folder = path.join(process.env.CARGO_TARGET_DIR || path.join(CORE, 'target'), jmath ? `wasm-${jmath}` : 'wasm-default');
  const module = path.join(folder, 'mcbench-core.wasm');
  const stamp = `${module}.sources`;
  const hash = `${await sourcesHash()} ${features}`;
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
    await step('[engine] building the WebAssembly engine from core/', '[engine] WebAssembly engine built',
      () => execFileAsync(cargo, [...wasmBuildArgs(features), '--target-dir', folder], { cwd: CORE, maxBuffer: 1 << 26, env }));
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

module.exports = { build, wasmBuildArgs, TARGET };
