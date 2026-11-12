'use strict';

// `.env` (server/dotenv.js) holds API keys: one the group or other accounts
// can read or write fails before anything in it is set, saying to chmod it.
const { after, test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const { loadDotEnv } = require('../server/dotenv');

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-dotenv-'));
after(() => fs.rmSync(scratch, { recursive: true, force: true }));

function dotenv(mode) {
  const file = path.join(fs.mkdtempSync(path.join(scratch, 'checkout-')), '.env');
  fs.writeFileSync(file, 'MCBENCH_DOTENV_TEST=from-file\n');
  fs.chmodSync(file, mode);
  return file;
}

test('a .env only its owner can read sets the variables the environment does not', () => {
  delete process.env.MCBENCH_DOTENV_TEST;
  loadDotEnv(dotenv(0o600));
  assert.equal(process.env.MCBENCH_DOTENV_TEST, 'from-file');
  delete process.env.MCBENCH_DOTENV_TEST;
});

test('a .env the group or other accounts can read or write fails, naming it and saying to run chmod 600 .env', () => {
  for (const mode of [0o644, 0o640, 0o604, 0o620, 0o602]) {
    delete process.env.MCBENCH_DOTENV_TEST;
    const file = dotenv(mode);
    assert.throws(() => loadDotEnv(file), {
      message: `${file} can be read or changed by other accounts on this machine (mode ${mode.toString(8)}). It holds API keys: run \`chmod 600 .env\` in ${path.dirname(file)}, then start the app again.`,
    });
    assert.equal(process.env.MCBENCH_DOTENV_TEST, undefined, 'nothing in it was set');
  }
});

test('no .env sets nothing', () => {
  loadDotEnv(path.join(scratch, 'missing', '.env'));
});
