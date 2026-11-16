'use strict';

// Builds the engine from this checkout's core/ for this machine, the native
// program and the WebAssembly module, and puts them where the app runs them:
// the tools folder's engine-<version> and engine-wasm-<version>, the version
// being the hash of core/'s sources (server/engine.js sourcesHash,
// engineFiles). The app downloads the released engine for sources that were
// released (server/tools.js PINS); after a change to core/ it runs this
// build instead, and fails until it is made. Cargo builds in CARGO_TARGET_DIR,
// or core/target.
//
//   node scripts/build-engine.js
//
// The program is built as the releases build it (scripts/release-engine.js):
// with the profile core/pgo has for this machine's target, if any. Sources
// that are the released engine's are not built: the app installs the
// released assets for them, checked against their pins, and a build in their
// place would be taken for those assets.

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { installRust, rust, sourcesHash, engineFiles, coreBuildArgs, coreProgram, CORE } = require('../server/engine');
const { build: buildWasm } = require('./build-wasm');
const { step } = require('../server/progress');
const tools = require('../server/tools');

const execFileAsync = promisify(execFile);

// Builds the program for `target` (a Rust target triple) with cargo's
// release profile and the target's profile of core/pgo, if any, in
// `targetDir`; resolves with its path.
async function buildProgram({ target, targetDir }) {
  const { cargo, env } = rust();
  await step(`[engine] building the engine's program for ${target} from core/`, '[engine] program built',
    () => execFileAsync(cargo, coreBuildArgs({ target }), { cwd: CORE, env: { ...env, CARGO_TARGET_DIR: targetDir }, maxBuffer: 1 << 26 }));
  return coreProgram(targetDir, target);
}

// Copies `file` into a new folder `folder` as `name`, the folder appearing
// whole (a rename).
function place(file, folder, name) {
  const staging = `${folder}.${process.pid}.partial`;
  fs.rmSync(staging, { recursive: true, force: true });
  fs.mkdirSync(staging, { recursive: true });
  fs.copyFileSync(file, path.join(staging, name));
  fs.chmodSync(path.join(staging, name), 0o755);
  fs.renameSync(staging, folder);
}

async function main() {
  const version = await sourcesHash();
  if (version === tools.PINS.engine.version) {
    throw new Error(`core/ is the released engine ${version} (server/engine-release.json): the app downloads it, so it is not built here. Build after changing core/, or build release assets with scripts/release-engine.js`);
  }
  await installRust();
  const files = engineFiles(version);
  const targetDir = process.env.CARGO_TARGET_DIR || path.join(CORE, 'target');
  if (!fs.existsSync(files.program)) {
    const program = await buildProgram({ target: rust().host, targetDir });
    place(program, path.dirname(files.program), path.basename(files.program));
  }
  if (!fs.existsSync(files.module)) {
    const module = await buildWasm();
    place(module, path.dirname(files.module), path.basename(files.module));
  }
  console.log(`engine ${version}: ${files.program}, ${files.module}`);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stderr || error);
    process.exit(1);
  });
}

module.exports = { buildProgram };
