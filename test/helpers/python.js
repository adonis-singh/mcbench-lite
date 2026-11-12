'use strict';
const path = require('node:path');
const { spawnSync } = require('node:child_process');
// The Python interpreter with nbtlib that reads NBT files independently of
// the app's writers: the repo-local virtualenv, or the one MCBENCH_PYTHON names.
//   uv venv .venv && uv pip install --python .venv/bin/python nbtlib
const PYTHON = process.env.MCBENCH_PYTHON || path.join(__dirname, '..', '..', '.venv', 'bin', 'python');
// Why a test that needs nbtlib is skipped, or false when PYTHON imports it.
function nbtlibMissing() {
  if (spawnSync(PYTHON, ['-c', 'import nbtlib'], { stdio: 'ignore' }).status === 0) return false;
  return `no Python with nbtlib at ${PYTHON} (uv venv .venv && uv pip install --python .venv/bin/python nbtlib, or name one with MCBENCH_PYTHON)`;
}
module.exports = { PYTHON, nbtlibMissing };
