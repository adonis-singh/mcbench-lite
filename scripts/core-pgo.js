'use strict';

// Records the profile the Rust engine is built with (profile-guided
// optimisation; coreBuildArgs in server/engine.js points rustc at
// core/pgo/<target>.profdata, for the app and every script that builds it):
// builds core/ instrumented, runs it over the community builds and saved
// worlds of the data folder (scripts/core-bench.js: placements, ticks and
// VERIFY requests) and over the fuzzer's worlds (blocks and entities the builds
// do not have), and merges what it recorded.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/core-pgo.js [--fuzz 200] [--llvm-profdata path]
//
// A profile only speeds the program up: the optimiser moves code around and
// changes no result. One recorded for older code serves the functions that
// have not changed since (rustc skips the others), so record again after
// larger changes. A profile serves the target it was recorded on (the
// functions' shapes differ with the target): record on each kind of machine
// the engine runs on (x86_64-unknown-linux-gnu, aarch64-apple-darwin).

const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync, spawnSync } = require('child_process');

const { coreProfile, rust } = require('../server/engine');

const ROOT = path.join(__dirname, '..');
const CORE = path.join(ROOT, 'core');
const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

// llvm-profdata of rustc's LLVM release (--llvm-profdata names one): rustup's
// llvm-tools component, llvm-profdata-<release> or Homebrew's llvm@<release> or
// the PATH's if it is of that release. (A profile is read by rustc of the same
// LLVM release or a later one: the profile that serves several machines is
// recorded on the one with the earliest.)
function llvmProfdata() {
  if (option('llvm-profdata', null)) return option('llvm-profdata');
  const sysroot = execFileSync(rust().rustc, ['--print', 'sysroot'], { encoding: 'utf8' }).trim();
  const verbose = execFileSync(rust().rustc, ['-vV'], { encoding: 'utf8' });
  const host = verbose.match(/^host: (.*)$/m)[1];
  const release = verbose.match(/^LLVM version: (\d+)/m)[1];
  const found = (command, args) => {
    const result = spawnSync(command, args, { encoding: 'utf8' });
    return result.status === 0 ? result.stdout.trim() : '';
  };
  const candidates = [
    path.join(sysroot, 'lib', 'rustlib', host, 'bin', 'llvm-profdata'),
    found('which', [`llvm-profdata-${release}`]),
    `/opt/homebrew/opt/llvm@${release}/bin/llvm-profdata`,
    `/usr/lib/llvm-${release}/bin/llvm-profdata`,
    found('which', ['llvm-profdata']),
  ];
  for (const candidate of candidates) {
    if (!candidate || !fs.existsSync(candidate)) continue;
    const version = found(candidate, ['--version']).match(/LLVM version (\d+)/);
    if (version && version[1] === release) return candidate;
  }
  throw new Error(`llvm-profdata of LLVM ${release} (rustc's) not found: add rustup's llvm-tools component, install that LLVM, or name one with --llvm-profdata`);
}

function run(command, args, options) {
  execFileSync(command, args, { stdio: 'inherit', ...options });
}

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-pgo-'));
try {
  const raw = path.join(scratch, 'raw');
  const target = path.join(scratch, 'target');
  // RUSTFLAGS replaces the config's flags (the profile to use), which an
  // instrumented build must not have.
  run(rust().cargo, ['build', '--release', '--quiet'], { cwd: CORE, env: { ...rust().env, RUSTFLAGS: `-Cprofile-generate=${raw}`, CARGO_TARGET_DIR: target } });
  const binary = path.join(target, 'release', 'mcbench-core');
  const bench = (...args) => run('node', [path.join(__dirname, 'core-bench.js'), '--engines', `pgo=${binary}`, '--rounds', '1', '--warmup', '0', '--graceful', ...args]);
  bench('--worlds', 'all');
  const fuzz = option('fuzz', '200');
  bench('--fuzz', fuzz, '--seed', '1', '--ticks', '100');
  bench('--fuzz', fuzz, '--seed', '100000', '--ticks', '100', '--drops', '--flow');
  bench('--fuzz', fuzz, '--seed', '200000', '--ticks', '60', '--shift');
  const profiles = fs.readdirSync(raw).filter((name) => name.endsWith('.profraw')).map((name) => path.join(raw, name));
  if (!profiles.length) throw new Error('the instrumented engine recorded no profile');
  const output = coreProfile();
  fs.mkdirSync(path.dirname(output), { recursive: true });
  run(llvmProfdata(), ['merge', '-o', output, ...profiles]);
  console.log(`${output}: ${fs.statSync(output).size} bytes from ${profiles.length} runs`);
} finally {
  fs.rmSync(scratch, { recursive: true, force: true });
}
