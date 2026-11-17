'use strict';

// Builds see_build's renderer's release assets (native/draw, the program
// mcbench-draw) from their sources: the program for each platform the app
// runs on, gzip-compressed and named by the renderer's version (the hash of
// its sources, server/draw.js sourcesHash) and its platform, each a .tar.gz
// of the program and what ships beside it (FLOOR), and then pins
// them: SHA256SUMS beside them and server/draw-release.json, which
// server/tools.js PINS reads. scripts/publish-draw.js uploads that folder as
// the release draw-<version> of the public repository.
//
//   node scripts/release-draw.js <asset> --out <folder> [--work <folder>]
//                                                        builds one asset, cargo's
//                                                        files in --work (<out>.work)
//   node scripts/release-draw.js --pin <folder>          pins the folder's assets
//
// <asset> is a platform of ASSETS. Each is built where it can be, as the
// engine's are (scripts/release-engine.js):
//   darwin-arm64, darwin-x64   on a Mac, with the app's rustup (tools.js):
//                              darwin-x64 is cross-compiled
//   linux-x64, linux-arm64     on x64 Linux with Docker, in Debian 10 (glibc
//                              2.28), so the program runs on that glibc and
//                              any later one; linux-arm64 is cross-compiled
//                              there with Debian's aarch64 gcc
// The program links no graphics library when it is built: wgpu opens the
// system's Vulkan, GL or Metal driver when it runs. A Linux asset is checked
// for that here (its needed libraries are glibc's and libgcc's only), and
// for the newest glibc symbol version it asks for.
//
// Every asset holds, under licenses/, the bundled font's licence and the
// third-party notices of the crates the program links for its target
// (scripts/draw-notices.js).
//
// The software floor (native/draw-floor/README.md): on Linux x64,
// SwiftShader's Vulkan driver, which the helper opens itself when the machine
// has no GPU adapter, built from SwiftShader's sources at the commit
// Chromium 149.0.7827.55 pins, in AlmaLinux 8 (glibc 2.28) with its
// libstdc++ and libgcc linked in and a non-executable stack, stripped, and
// refused unless its sha256 is the one two builds from scratch gave; with the
// licences of everything compiled into it. Linux arm64 has none yet: its
// build needs SwiftShader's LLVM reactor on an arm64 machine (see the
// README). macOS (Metal) and Windows (WARP) need nothing.
const FLOOR = {
  'linux-x64': {
    file: 'libvk_swiftshader.so',
    commit: 'f9d5d49a3c599a315e3493dc1e9b5309cffb3305',
    source: 'https://swiftshader.googlesource.com/SwiftShader',
    image: 'almalinux:8@sha256:2d0f2eb2a83cc09dbc2eb92fade4dbe8f709628149e81aa00aec70630406603b',
    sha256: '1637fb0f1a642dd99b9c857e4355144239c8a3e7d7742c08da6498e7f71dbe8b',
    bytes: 4613672,
    licenses: ['LICENSE.txt', 'third_party/marl/LICENSE', 'third_party/SPIRV-Tools/LICENSE', 'third_party/SPIRV-Headers/LICENSE', 'third_party/subzero/LICENSE.TXT', 'third_party/llvm-subzero/LICENSE.TXT'],
  },
};

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const os = require('os');
const { execFileSync } = require('child_process');

const { sourcesHash, DRAW, RENDERER } = require('../server/draw');

const ROOT = path.join(__dirname, '..');
const RELEASE_FILE = path.join(ROOT, 'server', 'draw-release.json');
const RUSTUP_INIT = 'rustup-init';
// The CPUs a Linux build's containers may use: CARGO_BUILD_JOBS, or all.
const JOBS = Number(process.env.CARGO_BUILD_JOBS || os.cpus().length);

const MACS = ['darwin-arm64', 'darwin-x64'];
const ASSETS = {
  'darwin-arm64': { target: 'aarch64-apple-darwin', build: 'darwin', hosts: MACS },
  'darwin-x64': { target: 'x86_64-apple-darwin', build: 'darwin', hosts: MACS },
  'linux-arm64': { target: 'aarch64-unknown-linux-gnu', build: 'linux', hosts: ['linux-x64'], linker: 'aarch64-linux-gnu-gcc', packages: ['gcc-aarch64-linux-gnu', 'libc6-dev-arm64-cross'], readelf: 'aarch64-linux-gnu-readelf' },
  'linux-x64': { target: 'x86_64-unknown-linux-gnu', build: 'linux', hosts: ['linux-x64'], packages: [], readelf: 'readelf' },
};
// The image the Linux programs are built in, as the engine's: Debian 10
// "buster", glibc 2.28, by digest, with its packages from Debian's archive.
const LINUX_IMAGE = 'debian:buster@sha256:58ce6f1271ae1c8a2006ff7d3e54e9874d839f573d8009c20154ad0f2fb0a225';
// The libraries a Linux program may need: glibc's (on glibc 2.28 libdl,
// libpthread and librt are libraries of their own) and libgcc's.
const LINUX_LIBRARIES = ['libc.so.6', 'libm.so.6', 'libdl.so.2', 'libpthread.so.0', 'librt.so.1', 'libgcc_s.so.1', 'ld-linux-x86-64.so.2', 'ld-linux-aarch64.so.1'];
const NEWEST_GLIBC = [2, 28];

const assetName = (version, asset) => `mcbench-draw-${version}-${asset}.tar.gz`;
const tag = (version) => `draw-${version}`;
const sha256 = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

function option(name) {
  const at = process.argv.indexOf(`--${name}`);
  if (at < 0) return null;
  if (!process.argv[at + 1]) throw new Error(`--${name} needs a value`);
  return path.resolve(process.argv[at + 1]);
}

// The sources as committed: an uncommitted change would build an asset its
// version does not describe to anyone else.
function checkClean() {
  const changed = execFileSync('git', ['status', '--porcelain', '--', DRAW, path.join(RENDERER, 'src'), path.join(RENDERER, 'Cargo.toml'), path.join(ROOT, 'core', 'rust-toolchain.toml')], { cwd: ROOT, encoding: 'utf8' }).split('\n').filter(Boolean);
  if (changed.length) throw new Error(`The renderer's sources have uncommitted changes, which no release can be built from:\n${changed.join('\n')}`);
}

async function darwinProgram(asset, scratch) {
  const { installRust, rust, CORE } = require('../server/engine');
  const { buildProgram } = require('./build-draw');
  const tools = require('../server/tools');
  await installRust();
  const { target } = ASSETS[asset];
  const installed = execFileSync(tools.RUSTUP, ['target', 'list', '--installed'], { cwd: CORE, env: rust().env, encoding: 'utf8' }).split('\n');
  if (!installed.includes(target)) execFileSync(tools.RUSTUP, ['target', 'add', target], { cwd: CORE, env: rust().env, stdio: 'inherit' });
  return buildProgram({ target, targetDir: path.join(scratch, 'target') });
}

// The program for a Linux platform, built in LINUX_IMAGE with the pinned
// rustup-init and the toolchain core/rust-toolchain.toml pins, the
// repository's native/draw, renderer and core/rust-toolchain.toml mounted
// read-only; then its needed libraries and glibc versions are checked.
async function linuxProgram(asset, scratch) {
  const tools = require('../server/tools');
  const { target, linker, packages, readelf } = ASSETS[asset];
  const init = await tools.install('rustup-init');
  const cache = path.join(scratch, 'cache');
  fs.mkdirSync(cache, { recursive: true });
  const script = [
    'set -eu',
    "printf 'deb http://archive.debian.org/debian buster main\\ndeb http://archive.debian.org/debian-security buster/updates main\\n' > /etc/apt/sources.list",
    `apt-get -qq update && apt-get -qq install -y --no-install-recommends ca-certificates gcc libc6-dev binutils ${packages.join(' ')} >/dev/null`,
    'export RUSTUP_HOME=/cache/rustup CARGO_HOME=/cache/cargo CARGO_TARGET_DIR=/cache/target PATH=/cache/cargo/bin:$PATH',
    `[ -x /cache/cargo/bin/rustup ] || /${RUSTUP_INIT} -y --quiet --no-modify-path --profile minimal --default-toolchain none`,
    `cd /src/core && rustup install && rustup target add ${target}`,
    ...(linker ? [`export CARGO_TARGET_${target.toUpperCase().replace(/-/g, '_')}_LINKER=${linker}`] : []),
    `cd /src/native/draw && RUSTUP_TOOLCHAIN=$(sed -n 's/^channel = "\\(.*\\)"$/\\1/p' /src/core/rust-toolchain.toml) nice -n 19 cargo build --release --quiet --locked -j ${JOBS} --target ${target}`,
    `cp /cache/target/${target}/release/mcbench-draw /cache/${asset}`,
    `${readelf} -d /cache/${asset} | sed -n 's/.*(NEEDED).*\\[\\(.*\\)\\]/\\1/p' > /cache/${asset}.needed`,
    `${readelf} -V /cache/${asset} | grep -o 'GLIBC_[0-9.]*' | sort -u > /cache/${asset}.glibc`,
    `chown -R ${process.getuid()}:${process.getgid()} /cache`,
  ].join('\n');
  execFileSync('docker', ['run', '--rm', '--user', '0', '--cpus', String(JOBS),
    '-v', `${DRAW}:/src/native/draw:ro`, '-v', `${RENDERER}:/src/renderer:ro`, '-v', `${path.join(ROOT, 'core', 'rust-toolchain.toml')}:/src/core/rust-toolchain.toml:ro`,
    '-v', `${cache}:/cache`, '-v', `${init}:/${RUSTUP_INIT}:ro`, LINUX_IMAGE, 'sh', '-c', script], { stdio: 'inherit' });
  const needed = fs.readFileSync(path.join(cache, `${asset}.needed`), 'utf8').split('\n').filter(Boolean);
  const foreign = needed.filter((name) => !LINUX_LIBRARIES.includes(name));
  if (foreign.length) throw new Error(`${asset} needs ${foreign.join(', ')} when it starts, beyond glibc's and libgcc's libraries`);
  const newest = fs.readFileSync(path.join(cache, `${asset}.glibc`), 'utf8').split('\n').filter(Boolean)
    .map((name) => name.slice('GLIBC_'.length).split('.').map(Number)).sort((a, b) => a[0] - b[0] || (a[1] || 0) - (b[1] || 0)).at(-1);
  if (newest[0] > NEWEST_GLIBC[0] || (newest[0] === NEWEST_GLIBC[0] && (newest[1] || 0) > NEWEST_GLIBC[1])) throw new Error(`${asset} asks for glibc ${newest.join('.')}, newer than ${NEWEST_GLIBC.join('.')}`);
  console.log(`${asset}: needs ${needed.join(', ')}; newest glibc ${newest.join('.')}`);
  return path.join(cache, asset);
}

// The floor's driver for `asset` in `scratch`/floor: SwiftShader fetched by
// its commit, built in its container, stripped, checked against its pin;
// its licences under licenses/swiftshader. Resolves with that folder.
function buildFloor(asset, scratch) {
  const floor = FLOOR[asset];
  const work = path.join(scratch, 'swiftshader');
  const src = path.join(work, 'src');
  const out = path.join(scratch, 'floor');
  const git = (...args) => execFileSync('git', ['-C', src, ...args], { encoding: 'utf8' }).trim();
  if (!fs.existsSync(path.join(src, '.git'))) {
    fs.mkdirSync(src, { recursive: true });
    git('init', '-q');
    git('remote', 'add', 'origin', floor.source);
    git('fetch', '-q', '--depth', '1', 'origin', floor.commit);
    git('checkout', '-q', 'FETCH_HEAD');
  }
  if (git('rev-parse', 'HEAD') !== floor.commit) throw new Error(`${src} is not SwiftShader ${floor.commit}`);
  fs.mkdirSync(path.join(work, 'build'), { recursive: true });
  const script = [
    'set -eu',
    `trap 'chown -R ${process.getuid()}:${process.getgid()} /w' EXIT`,
    'dnf -q -y install gcc-toolset-13-gcc-c++ cmake make python3 git >/dev/null',
    'source /opt/rh/gcc-toolset-13/enable',
    'cmake -S src -B build -DCMAKE_BUILD_TYPE=Release -DREACTOR_BACKEND=Subzero -DSWIFTSHADER_BUILD_TESTS=OFF -DSWIFTSHADER_BUILD_BENCHMARKS=OFF -DSWIFTSHADER_BUILD_PVR=OFF -DSWIFTSHADER_WARNINGS_AS_ERRORS=OFF -DSWIFTSHADER_ENABLE_ASTC=OFF "-DCMAKE_SHARED_LINKER_FLAGS=-static-libstdc++ -static-libgcc -Wl,-z,noexecstack" >/dev/null',
    `cmake --build build --target vk_swiftshader -j ${JOBS} >build.log 2>&1 || { tail -40 build.log; exit 1; }`,
  ].join('\n');
  execFileSync('docker', ['run', '--rm', '--cpus', String(JOBS), '-v', `${work}:/w`, '-w', '/w', floor.image, 'bash', '-c', script], { stdio: 'inherit' });
  fs.rmSync(out, { recursive: true, force: true });
  fs.mkdirSync(path.join(out, 'licenses', 'swiftshader'), { recursive: true });
  const library = path.join(out, floor.file);
  fs.copyFileSync(path.join(work, 'build', 'Linux', floor.file), library);
  execFileSync('strip', [library]);
  const bytes = fs.readFileSync(library);
  if (sha256(bytes) !== floor.sha256 || bytes.length !== floor.bytes) throw new Error(`${library} built with sha256 ${sha256(bytes)} and ${bytes.length} bytes; the floor pins ${floor.sha256} and ${floor.bytes}`);
  for (const license of floor.licenses) fs.copyFileSync(path.join(src, license), path.join(out, 'licenses', 'swiftshader', license.replace(/\//g, '-')));
  return out;
}

async function buildAsset(asset, out) {
  if (!ASSETS[asset]) throw new Error(`No asset ${asset}: one of ${Object.keys(ASSETS).join(', ')}`);
  const { build, hosts } = ASSETS[asset];
  const { PLATFORM } = require('../server/tools');
  if (!hosts.includes(PLATFORM)) throw new Error(`${asset} is built on ${hosts.join(' or ')}, and this is ${PLATFORM}`);
  checkClean();
  const version = await sourcesHash();
  const scratch = path.join(option('work') || `${out}.work`, asset);
  fs.mkdirSync(scratch, { recursive: true });
  const built = await { darwin: darwinProgram, linux: linuxProgram }[build](asset, scratch);
  // The archive's files: the program, the floor's beside it, and under
  // licenses/ the bundled font's licence (OFL 1.1) and the notices of the
  // Rust crates the program links (scripts/draw-notices.js).
  const files = path.join(scratch, 'archive');
  fs.rmSync(files, { recursive: true, force: true });
  if (FLOOR[asset]) fs.cpSync(buildFloor(asset, scratch), files, { recursive: true });
  fs.mkdirSync(path.join(files, 'licenses'), { recursive: true });
  fs.copyFileSync(path.join(DRAW, 'fonts', 'LICENSE'), path.join(files, 'licenses', 'LiberationSans-OFL-1.1.txt'));
  await require('./draw-notices').notices(ASSETS[asset].target, path.join(files, 'licenses', 'THIRD-PARTY-NOTICES.txt'));
  fs.copyFileSync(built, path.join(files, 'mcbench-draw'));
  fs.chmodSync(path.join(files, 'mcbench-draw'), 0o755);
  const name = assetName(version, asset);
  const entries = fs.readdirSync(files).sort();
  execFileSync('tar', ['-czf', path.join(out, `${name}.part`), '-C', files, ...entries]);
  fs.renameSync(path.join(out, `${name}.part`), path.join(out, name));
  console.log(`${path.join(out, name)}: ${fs.statSync(path.join(out, name)).size} bytes`);
}

// SHA256SUMS of the folder's assets and draw-release.json: every asset of
// this version must be there.
async function pin(folder) {
  const version = await sourcesHash();
  const assets = {};
  const sums = [];
  for (const asset of Object.keys(ASSETS)) {
    const name = assetName(version, asset);
    const file = path.join(folder, name);
    if (!fs.existsSync(file)) throw new Error(`${file} is missing: build it with node scripts/release-draw.js ${asset} --out ${folder}`);
    const bytes = fs.readFileSync(file);
    assets[asset] = { name, sha256: sha256(bytes), bytes: bytes.length };
    sums.push(`${assets[asset].sha256}  ${name}\n`);
  }
  fs.writeFileSync(path.join(folder, 'SHA256SUMS'), sums.join(''));
  fs.writeFileSync(RELEASE_FILE, `${JSON.stringify({ version, tag: tag(version), assets }, null, 2)}\n`);
  console.log(`${RELEASE_FILE}: renderer ${version}, release ${tag(version)}`);
}

async function main() {
  const pinned = option('pin');
  if (pinned) return pin(pinned);
  const out = option('out');
  if (!out) throw new Error('usage: node scripts/release-draw.js <asset> --out <folder> | --pin <folder>');
  fs.mkdirSync(out, { recursive: true });
  return buildAsset(process.argv[2], out);
}

if (require.main === module) {
  main().catch((error) => {
    console.error(error.stderr ? String(error.stderr) : error);
    process.exit(1);
  });
}

module.exports = { ASSETS, assetName, tag, FLOOR, buildFloor };
