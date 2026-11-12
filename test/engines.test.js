'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { buildCore } = require('../server/engine');

test('a verifier lane runs on the Rust engine alone', () => {
  const { EngineVerifier } = require('../server/minecraft-verify');
  const lane = new EngineVerifier({ root: fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-lane-')) });
  assert.strictEqual(lane.engine.build, buildCore);
});
