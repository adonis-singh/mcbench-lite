'use strict';

// The native Rust program the WebAssembly module is compared with and timed
// against: built the way the app builds it (coreBuildArgs, with the profile
// recorded for this machine), with Java's Math.sin and Math.cos chosen as the
// module's are (core/src/jmath.rs), into a target folder of its own.

const path = require('path');
const { execFileSync } = require('child_process');
const { Engine, coreBuildArgs, rust } = require('../server/engine');

const CORE = path.join(__dirname, '..', 'core');

function nativeEngine(jmath) {
  const folder = path.join(CORE, 'target', `native-${jmath}`);
  execFileSync(rust().cargo, [...coreBuildArgs(), '--features', `jmath-${jmath}`, '--target-dir', folder], { cwd: CORE, env: rust().env, stdio: 'inherit' });
  return new Engine({ build: async () => path.join(folder, 'release', 'mcbench-core') });
}

module.exports = { nativeEngine };
