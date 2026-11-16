'use strict';

// Builds the engine's release assets from core/'s sources: the native
// program for each platform the app runs on and the one WebAssembly module,
// each gzip-compressed and named by the engine's version (the hash of
// core/'s sources, server/engine.js sourcesHash) and its platform, and then
// pins them: SHA256SUMS beside them and server/engine-release.json, which
// server/tools.js PINS reads. scripts/publish-engine.js uploads that folder
// as the release engine-<version> of the public repository.
//
//   node scripts/release-engine.js <asset> --out <folder> [--work <folder>]
//                                                           builds one asset, cargo's
//                                                           files in --work (<out>.work)
//   node scripts/release-engine.js --pin <folder>             pins the folder's assets
//
// <asset> is a platform of ASSETS or wasm32. Each is built where it can be:
//   darwin-arm64, darwin-x64   on a Mac, with the app's rustup (tools.js):
//                              darwin-x64 is cross-compiled, its target added
//                              to the pinned toolchain
//   linux-x64, linux-arm64     on Linux with Docker, in Debian 10 (glibc 2.28,
//                              the oldest glibc Node.js's own Linux builds run
//                              on), so the program runs on that glibc and any
//                              later one: linux-arm64 is cross-compiled there
//                              with Debian's aarch64 gcc and its glibc 2.28
//   wasm32                     anywhere, with the app's rustup
// Every program is built with exactly the app's cargo arguments
// (engine.js coreBuildArgs: the release profile, and the target's
// profile-guided-optimisation profile where core/pgo has one), the module
// with build-wasm.js's. core/ must have no uncommitted change, so an asset is
// what its version's sources build. A Windows program is one more row of
// ASSETS (x86_64-pc-windows-msvc, aarch64-pc-windows-msvc, mcbench-core.exe)
// and one more build here.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { execFileSync } = require('child_process');

const { CORE, sourcesHash } = require('../server/engine');

const ROOT = path.join(__dirname, '..');
const RELEASE_FILE = path.join(ROOT, 'server', 'engine-release.json');
const RUSTUP_INIT = 'rustup-init';

// The assets: each one's Rust target, how it is built, and the platforms
// (tools.js PLATFORM) it is built on.
const MACS = ['darwin-arm64', 'darwin-x64'];
const ASSETS = {
  'darwin-arm64': { target: 'aarch64-apple-darwin', build: 'darwin', hosts: MACS },
  'darwin-x64': { target: 'x86_64-apple-darwin', build: 'darwin', hosts: MACS },
  'linux-arm64': { target: 'aarch64-unknown-linux-gnu', build: 'linux', hosts: ['linux-x64'], linker: 'aarch64-linux-gnu-gcc', packages: ['gcc-aarch64-linux-gnu', 'libc6-dev-arm64-cross'] },
  'linux-x64': { target: 'x86_64-unknown-linux-gnu', build: 'linux', hosts: ['linux-x64'], packages: [] },
  wasm32: { target: 'wasm32-unknown-unknown', build: 'wasm', hosts: [...MACS, 'linux-arm64', 'linux-x64'] },
};
// The image the Linux programs are built in: Debian 10 "buster", glibc 2.28,
// by digest, with its packages from Debian's archive.
const LINUX_IMAGE = 'debian:buster@sha256:58ce6f1271ae1c8a2006ff7d3e54e9874d839f573d8009c20154ad0f2fb0a225';

const assetName = (version, asset) => (asset === 'wasm32' ? `mcbench-core-${version}-wasm32.wasm.gz` : `mcbench-core-${version}-${asset}.gz`);
const tag = (version) => `engine-${version}`;
const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

function option(name) {
  const at = process.argv.indexOf(`--${name}`);
  if (at < 0) return null;
  if (!process.argv[at + 1]) throw new Error(`--${name} needs a value`);
  return path.resolve(process.argv[at + 1]);
}

// core/ as committed: an uncommitted change would build an asset that its
// version does not describe to anyone else.
function checkClean() {
  const changed = execFileSync('git', ['status', '--porcelain', '--', '.'], { cwd: CORE, encoding: 'utf8' }).split('\n').filter(Boolean);
  if (changed.length) throw new Error(`core/ has uncommitted changes, which no release can be built from:\n${changed.join('\n')}`);
}

// The program for a platform built on this Mac.
async function darwinProgram(asset, scratch) {
  const { installRust, rust } = require('../server/engine');
  const { buildProgram } = require('./build-engine');
  const tools = require('../server/tools');
  await installRust();
  const { target } = ASSETS[asset];
  const installed = execFileSync(tools.RUSTUP, ['target', 'list', '--installed'], { cwd: CORE, env: rust().env, encoding: 'utf8' }).split('\n');
  if (!installed.includes(target)) execFileSync(tools.RUSTUP, ['target', 'add', target], { cwd: CORE, env: rust().env, stdio: 'inherit' });
  return buildProgram({ target, targetDir: path.join(scratch, 'target') });
}

// The program for a Linux platform, built in LINUX_IMAGE with the pinned
// rustup-init (its sha256 checked as the app checks it, tools.js) and the
// toolchain core/rust-toolchain.toml pins.
async function linuxProgram(asset, scratch) {
  const tools = require('../server/tools');
  const { coreBuildArgs, coreProgram, installRust } = require('../server/engine');
  // The host's target decides whether cargo is given --target (coreBuildArgs).
  await installRust();
  const { target, linker, packages } = ASSETS[asset];
  const init = await tools.install('rustup-init');
  const cache = path.join(scratch, 'cache');
  fs.mkdirSync(cache, { recursive: true });
  const profile = fs.existsSync(path.join(CORE, 'pgo', `${target}.profdata`)) ? `/src/core/pgo/${target}.profdata` : null;
  const cargo = ['cargo', ...coreBuildArgs({ target, profile })].map((word) => `'${word.replace(/'/g, "'\\''")}'`).join(' ');
  const script = [
    'set -eu',
    // The image's packages are buster's last security updates, so the
    // archive's security suite is a source too.
    "printf 'deb http://archive.debian.org/debian buster main\\ndeb http://archive.debian.org/debian-security buster/updates main\\n' > /etc/apt/sources.list",
    `apt-get -qq update && apt-get -qq install -y --no-install-recommends gcc libc6-dev ${packages.join(' ')} >/dev/null`,
    `export RUSTUP_HOME=/cache/rustup CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target PATH=/cache/cargo/bin:$PATH`,
    `[ -x /cache/cargo/bin/rustup ] || /${RUSTUP_INIT} -y --quiet --no-modify-path --profile minimal --default-toolchain none`,
    'cd /src/core && rustup install && rustup target add ' + target,
    ...(linker ? [`export CARGO_TARGET_${target.toUpperCase().replace(/-/g, '_')}_LINKER=${linker}`] : []),
    // The build runs at the lowest priority: the machine may be in use.
    `nice -n 19 ${cargo}`,
    `cp ${coreProgram('/cache/target', target)} /cache/${asset}`,
    `chown -R ${process.getuid()}:${process.getgid()} /cache`,
  ].join('\n');
  execFileSync('docker', ['run', '--rm', '--user', '0', '-v', `${CORE}:/src/core:ro`, '-v', `${cache}:/cache`, '-v', `${init}:/${RUSTUP_INIT}:ro`, LINUX_IMAGE, 'sh', '-c', script], { stdio: 'inherit' });
  return path.join(cache, asset);
}

async function buildAsset(asset, out) {
  if (!ASSETS[asset]) throw new Error(`No asset ${asset}: one of ${Object.keys(ASSETS).join(', ')}`);
  const { build, hosts } = ASSETS[asset];
  const { PLATFORM } = require('../server/tools');
  if (!hosts.includes(PLATFORM)) throw new Error(`${asset} is built on ${hosts.join(' or ')}, and this is ${PLATFORM}`);
  checkClean();
  const version = await sourcesHash();
  // Cargo's and the container's files.
  const scratch = path.join(option('work') || `${out}.work`, asset);
  fs.mkdirSync(scratch, { recursive: true });
  const file = { wasm: async () => require('./build-wasm').build(), darwin: darwinProgram, linux: linuxProgram }[build];
  const built = await file(asset, scratch);
  const name = assetName(version, asset);
  fs.writeFileSync(path.join(out, `${name}.part`), zlib.gzipSync(fs.readFileSync(built), { level: 9 }));
  fs.renameSync(path.join(out, `${name}.part`), path.join(out, name));
  console.log(`${path.join(out, name)}: ${fs.statSync(path.join(out, name)).size} bytes`);
}

// SHA256SUMS of the folder's assets and engine-release.json: every asset of
// this version must be there.
async function pin(folder) {
  const version = await sourcesHash();
  const assets = {};
  const sums = [];
  for (const asset of Object.keys(ASSETS)) {
    const name = assetName(version, asset);
    const file = path.join(folder, name);
    if (!fs.existsSync(file)) throw new Error(`${file} is missing: build it with node scripts/release-engine.js ${asset} --out ${folder}`);
    const bytes = fs.readFileSync(file);
    assets[asset] = { name, sha256: sha256(bytes), bytes: bytes.length };
    sums.push(`${assets[asset].sha256}  ${name}\n`);
  }
  fs.writeFileSync(path.join(folder, 'SHA256SUMS'), sums.join(''));
  fs.writeFileSync(RELEASE_FILE, `${JSON.stringify({ version, tag: tag(version), assets }, null, 2)}\n`);
  console.log(`${RELEASE_FILE}: engine ${version}, release ${tag(version)}`);
}

async function main() {
  const pinned = option('pin');
  if (pinned) return pin(pinned);
  const out = option('out');
  if (!out) throw new Error('usage: node scripts/release-engine.js <asset> --out <folder> | --pin <folder>');
  fs.mkdirSync(out, { recursive: true });
  return buildAsset(process.argv[2], out);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stderr ? String(error.stderr) : error);
    process.exit(1);
  });
}

module.exports = { ASSETS, assetName, tag };
