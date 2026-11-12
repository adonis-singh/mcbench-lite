'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');
const { START_REJECTIONS, verificationFailure } = require('../server/minecraft-verify');

// McbenchExperiment.start imports block entities, scheduled ticks and entities;
// what those three throw is what the Paper plugin reports as "could not start".
test('the rejections of a test\'s imported data are the ones McbenchExperiment.start throws', () => {
  const source = fs.readFileSync(path.join(__dirname, '../server/McbenchExperiment.java'), 'utf8').split('\n');
  const first = source.findIndex((line) => line.includes('private void importBlockEntities()'));
  const last = source.findIndex((line) => line.includes('private JsonArray readLivingEntities()'));
  const found = [];
  for (const line of source.slice(first, last)) for (const [, message] of line.matchAll(/new (?:IOException|IllegalStateException)\("([^"]*)"/g)) if (!found.includes(message)) found.push(message);
  assert.deepEqual(START_REJECTIONS, found);
});

test('engine rejections are worded as the Paper plugin words them', () => {
  const failed = (text) => verificationFailure(new Error(`The engine failed: ${text}`)).message;
  assert.equal(failed('java.io.IOException: No block entity at [0,1,3] <- java.lang.Exception: x @ McbenchExperiment.importBlockEntities(McbenchExperiment.java:387)'), 'Minecraft verification could not start: MCBENCH_ERROR No block entity at [0,1,3]\n');
  assert.equal(failed('java.io.IOException: Inventory probe is not a container: [1,2,3]'), 'Minecraft verification failed: java.io.IOException: Inventory probe is not a container: [1,2,3]');
  const stopped = new Error('The engine stopped (SIGKILL): ');
  assert.equal(verificationFailure(stopped), stopped);
});
