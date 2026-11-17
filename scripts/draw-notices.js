'use strict';

// The third-party notices of see_build's renderer (native/draw) for one
// target: the licence texts of the Rust crates the program links, as
// native/draw/Cargo.lock resolves them for that target, each with the crates
// that use it. Made by cargo-about 0.9.3 (installed into the tools folder the
// first time, with the Rust core/rust-toolchain.toml pins, as
// scripts/build-draw.js installs it), configured by scripts/draw-about.toml
// and laid out by scripts/draw-notices.hbs, offline over the crates cargo
// fetched for the lock file, so the same lock file gives the same text. A
// crate under a licence draw-about.toml does not accept fails it.
// scripts/release-draw.js puts the result in every asset.
//
//   node scripts/draw-notices.js <target triple> <out file>

const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { installRust, rust } = require('../server/engine');
const { DRAW } = require('../server/draw');
const { step } = require('../server/progress');
const { withLock } = require('../server/pid-lock');
const tools = require('../server/tools');

const execFileAsync = promisify(execFile);
const CARGO_ABOUT = '0.9.3';
const SCRIPTS = __dirname;

// cargo-about in the tools folder, built once from crates.io at its pinned
// version with its own lock file.
async function cargoAbout() {
  await installRust();
  const { cargo, env } = rust();
  const folder = path.join(tools.TOOLS_DIR, `cargo-about-${CARGO_ABOUT}`);
  const program = path.join(folder, 'bin', 'cargo-about');
  if (fs.existsSync(program)) return program;
  await withLock(`${folder}.lock`, async () => {
    if (fs.existsSync(program)) return;
    const staging = `${folder}.${process.pid}.partial`;
    fs.rmSync(staging, { recursive: true, force: true });
    try {
      await step(`[notices] building cargo-about ${CARGO_ABOUT} into ${tools.TOOLS_DIR}`, '[notices] cargo-about built',
        () => execFileAsync(cargo, ['install', 'cargo-about', '--version', CARGO_ABOUT, '--locked', '--features', 'cli', '--root', staging],
          { env: { ...env, CARGO_TARGET_DIR: path.join(staging, 'target') }, maxBuffer: 1 << 26 }));
      fs.rmSync(path.join(staging, 'target'), { recursive: true, force: true });
      fs.renameSync(staging, folder);
    } finally {
      fs.rmSync(staging, { recursive: true, force: true });
    }
  });
  return program;
}

async function notices(target, out) {
  const about = await cargoAbout();
  const { cargo, env } = rust();
  const manifest = path.join(DRAW, 'Cargo.toml');
  // The crates' sources, fetched for the lock file; cargo-about then reads
  // their licence files without the network.
  await execFileAsync(cargo, ['fetch', '--locked', '--manifest-path', manifest], { env, maxBuffer: 1 << 26 });
  await execFileAsync(about, ['generate', '--offline', '--locked', '-m', manifest, '-c', path.join(SCRIPTS, 'draw-about.toml'), '--target', target,
    '-o', out, path.join(SCRIPTS, 'draw-notices.hbs')], { env: { ...env, PATH: `${path.dirname(cargo)}${path.delimiter}${process.env.PATH}` }, maxBuffer: 1 << 26 });
  return out;
}

async function main() {
  const [target, out] = process.argv.slice(2);
  if (!target || !out) throw new Error('usage: node scripts/draw-notices.js <target triple> <out file>');
  await notices(target, path.resolve(out));
  console.log(`${out}: ${fs.statSync(out).size} bytes`);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stderr ? String(error.stderr) : error);
    process.exit(1);
  });
}

module.exports = { notices, CARGO_ABOUT };
