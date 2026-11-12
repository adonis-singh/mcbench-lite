'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { testBuildTool, TEST_BUILD_TOOL } = require('../server/verification-contract');
const { MinecraftVerifier } = require('../server/minecraft-verify');
const { World } = require('../server/world');

test('test_build advertises saved ticks only for strict placement without mutating other schemas', () => {
  const strict = testBuildTool('strict');
  assert.ok(strict.function.parameters.properties.tests.items.properties.scheduled_ticks);
  for (const tool of [TEST_BUILD_TOOL, testBuildTool(), testBuildTool('updates')]) {
    assert.equal(Object.hasOwn(tool.function.parameters.properties.tests.items.properties, 'scheduled_ticks'), false);
    assert.equal(tool.function.parameters.additionalProperties, false);
  }
  assert.ok(testBuildTool('strict').function.parameters.properties.tests.items.properties.scheduled_ticks);
  assert.throws(() => testBuildTool('approximate'), /Unsupported placement mode/);
});

test('unsupported saved queues fail before queuing or starting Minecraft, including empty queues', async () => {
  const verifier = new MinecraftVerifier();
  let executions = 0;
  verifier.execute = async () => { executions++; };
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  const progress = [];
  for (const entries of [[], [{ kind: 'block', id: 'minecraft:stone', at: [0, 0, 0], delay: 1, priority: 0, sub_tick: '1' }]]) {
    const request = { probes: [[0, 0, 0]], scheduled_ticks: { bounds: [[0, 0, 0], [1, 1, 1]], data_version: 4189, entries } };
    const before = structuredClone(request);
    await assert.rejects(verifier.run(world, request, { onUpdate: update => progress.push(update) }), /Omit scheduled_ticks.*not started/);
    assert.deepEqual(request, before, 'reject incompatible submissions without rewriting them');
  }
  assert.equal(executions, 0);
  assert.deepEqual(progress, []);
  await verifier.run(world, { probes: [[0, 0, 0]] });
  assert.equal(executions, 1, 'a corrected request still runs in the original mode');
  assert.equal(verifier.placementMode, 'updates');
});

test('strict placement still forwards authoritative empty and nonempty queues unchanged', async () => {
  const verifier = new MinecraftVerifier({ placementMode: 'strict' });
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone');
  verifier.execute = async (_, request) => request;
  for (const entries of [[], [{ kind: 'block', id: 'minecraft:stone', at: [0, 0, 0], delay: 7, priority: -1, sub_tick: '9007199254740993' }]]) {
    const saved = { bounds: [[0, 0, 0], [1, 1, 1]], data_version: 4189, entries };
    const result = await verifier.run(world, { probes: [[0, 0, 0]], scheduled_ticks: saved });
    assert.equal(result.placementMode, 'strict');
    assert.deepEqual(result.scheduled_ticks, saved);
  }
});

test('the tool description and guidance quote the budgets in force and the call shape', () => {
  const { TEST_BUILD_GUIDANCE, BUDGETS } = require('../server/verification-contract');
  for (const text of [`${BUDGETS.operations} operations`, `${BUDGETS.simulationTicks} game ticks`, `${BUDGETS.wallMs / 1000}-second`, `${BUDGETS.columns} chunk columns`, `${BUDGETS.requestBytes / 2 ** 20} MiB`, `${BUDGETS.reportBytes / 2 ** 20} MiB`, `one to ${BUDGETS.tests} experiments`, 'apply to each test on its own', 'too dense to finish in time and how many ticks it reached', 'unported: <what>']) {
    assert.ok(TEST_BUILD_GUIDANCE.includes(text), `the guidance says: ${text}`);
  }
  const { tests } = TEST_BUILD_TOOL.function.parameters.properties;
  assert.equal(tests.maxItems, BUDGETS.tests);
  assert.equal(tests.minItems, 1);
  assert.deepEqual(TEST_BUILD_TOOL.function.parameters.required, ['tests']);
  assert.match(TEST_BUILD_TOOL.function.description, /not shared across the call.*too dense to finish within its wall-clock budget.*how many ticks it reached/);
  assert.ok(BUDGETS.simulationTicks >= 72_000 && BUDGETS.columns >= 1024, 'the static limits stay well above the old ones; wallMs is the limiter');
});

test('the tool says what a use input takes and that a fence gate needs the yaw', () => {
  const { TEST_BUILD_GUIDANCE } = require('../server/verification-contract');
  const action = TEST_BUILD_TOOL.function.parameters.properties.tests.items.properties.actions;
  assert.deepEqual(Object.keys(action.items.properties).filter((key) => key === 'yaw'), ['yaw']);
  for (const text of [action.description, TEST_BUILD_GUIDANCE]) {
    for (const block of ['door', 'trapdoor', 'fence gate', 'repeater', 'comparator', 'note block', 'daylight detector', 'redstone dust', 'decorated pot']) assert.ok(text.includes(block), `it names the ${block}`);
  }
  assert.match(TEST_BUILD_GUIDANCE, /fence gate opens away from the player, so using one needs yaw/);
});
