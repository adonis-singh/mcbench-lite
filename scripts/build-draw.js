'use strict';

// Builds see_build's renderer (native/draw, the program mcbench-draw) from
// this checkout's sources for this machine and puts it where the app runs it:
// the tools folder's draw-<version>, the version being the hash of its
// sources (server/draw.js sourcesHash). The app downloads the released
// renderer for sources that were released (server/tools.js PINS); after a
// change to native/draw or renderer/src it runs this build instead, and
// fails until it is made. Cargo builds in CARGO_TARGET_DIR, or
// native/draw/target, with the Rust core/rust-toolchain.toml pins (installed
// into the tools folder the first time, as for the engine).
//
//   node scripts/build-draw.js
//
// On Linux x64 it also builds the software Vulkan driver the release ships
// beside the program (scripts/release-draw.js FLOOR), with Docker.
//
// Sources that are the released renderer's are not built: the app installs
// the released asset for them, checked against its pin, and a build in its
// place would be taken for that asset.

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { installRust, rust } = require('../server/engine');
const { sourcesHash, drawProgram, DRAW } = require('../server/draw');
const { step } = require('../server/progress');
const tools = require('../server/tools');

const execFileAsync = promisify(execFile);

// cargo's arguments for the helper for `target` (a Rust target triple): the
// release profile, and --target only for a cross build.
function drawBuildArgs(target) {
  return ['build', '--release', '--quiet', '--locked', '--manifest-path', path.join(DRAW, 'Cargo.toml'), ...(target === rust().host ? [] : ['--target', target])];
}

// Where cargo puts the program it built for `target` in `targetDir`.
function builtProgram(targetDir, target) {
  return path.join(targetDir, ...(target === rust().host ? [] : [target]), 'release', 'mcbench-draw');
}

// Builds the program for `target` in `targetDir`; resolves with its path.
async function buildProgram({ target, targetDir }) {
  const { cargo, env } = rust();
  await step(`[see_build] building the screenshot renderer for ${target} from native/draw`, '[see_build] renderer built',
    () => execFileAsync(cargo, drawBuildArgs(target), { cwd: DRAW, env: { ...env, CARGO_TARGET_DIR: targetDir }, maxBuffer: 1 << 26 }));
  return builtProgram(targetDir, target);
}

async function main() {
  const version = await sourcesHash();
  if (version === tools.PINS.draw.version) {
    throw new Error(`native/draw and renderer/src are the released renderer ${version} (server/draw-release.json): the app downloads it, so it is not built here. Build after changing them, or build release assets with scripts/release-draw.js`);
  }
  await installRust();
  const program = drawProgram(version);
  if (!fs.existsSync(program)) {
    const built = await buildProgram({ target: rust().host, targetDir: process.env.CARGO_TARGET_DIR || path.join(DRAW, 'target') });
    const folder = path.dirname(program);
    const staging = `${folder}.${process.pid}.partial`;
    fs.rmSync(staging, { recursive: true, force: true });
    fs.mkdirSync(staging, { recursive: true });
    fs.copyFileSync(built, path.join(staging, path.basename(program)));
    fs.chmodSync(path.join(staging, path.basename(program)), 0o755);
    // The software floor the release ships beside it on this platform, built
    // and checked as the release builds it (Docker).
    const { FLOOR, buildFloor } = require('./release-draw');
    if (FLOOR[tools.PLATFORM]) fs.cpSync(buildFloor(tools.PLATFORM, path.join(process.env.CARGO_TARGET_DIR || path.join(DRAW, 'target'), 'floor')), staging, { recursive: true });
    fs.renameSync(staging, folder);
  }
  console.log(`renderer ${version}: ${program}`);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stderr || error);
    process.exit(1);
  });
}

module.exports = { buildProgram, drawBuildArgs, builtProgram };
