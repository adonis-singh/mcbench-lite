'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-agent-vision-'));
process.env.MCBENCH_DATA_DIR = dataDir;

const store = require('../server/store');
const { systemPrompt } = require('../server/prompt');
const { _internal } = require('../server/agent');
const { apiMessages, toolsForImageInput } = _internal;

test('agent tool schemas follow the active verifier mode for vision and text models', () => {
  const { minecraftVerifier } = require('../server/minecraft-verify');
  const original = minecraftVerifier.placementMode;
  try {
    for (const mode of ['updates', 'strict', 'updates']) {
      minecraftVerifier.placementMode = mode;
      for (const vision of [false, true]) {
        const tool = toolsForImageInput(vision).find(tool => tool.function.name === 'test_build');
        assert.equal(Object.hasOwn(tool.function.parameters.properties.tests.items.properties, 'scheduled_ticks'), mode === 'strict');
      }
    }
  } finally {
    minecraftVerifier.placementMode = original;
  }
});

after(() => {
  store.close();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

test('text-only models do not receive vision tools or vision instructions', () => {
  const toolNames = toolsForImageInput(false).map((tool) => tool.function.name);
  assert.deepStrictEqual(toolNames, ['test_build', 'build', 'bash']);

  const prompt = systemPrompt({ imageInput: false });
  assert.doesNotMatch(prompt, /see_build|view_image/);
  assert.match(prompt, /verify the world structurally/i);
});

test('text-only model messages omit current and historical image payloads', () => {
  const conversation = {
    messages: [
      {
        role: 'user',
        content: [
          { type: 'text', text: 'Use this reference' },
          { type: 'image_url', image_url: { url: 'data:image/png;base64,YWJj' } },
        ],
      },
      {
        role: 'user',
        seeBuild: true,
        content: [
          { type: 'text', text: '[see_build] Screenshots of the current build:' },
          { type: 'image_url', image_url: { url: 'data:image/png;base64,ZGVm' } },
        ],
      },
    ],
  };

  const messages = apiMessages(conversation, { imageInput: false });
  assert.doesNotMatch(JSON.stringify(messages), /image_url|data:image/);
  assert.match(messages[0].content, /verify the world structurally/i);
  assert.match(JSON.stringify(messages), /accepts text input only/i);
});

test('vision models retain the visual tools and image payloads', () => {
  const toolNames = toolsForImageInput(true).map((tool) => tool.function.name);
  assert.ok(toolNames.includes('see_build'));
  assert.ok(toolNames.includes('view_image'));

  const messages = apiMessages({
    messages: [{
      role: 'user',
      content: [{ type: 'image_url', image_url: { url: 'data:image/png;base64,YWJj' } }],
    }],
  });
  assert.match(JSON.stringify(messages), /image_url|data:image/);
});

test('SVG attachments stay available to tools without sending unsupported image bytes to vision or titles', () => {
  const { supportsImageData } = require('../server/image-input');
  const svg = 'data:image/svg+xml;base64,' + Buffer.from('<svg/>').toString('base64');
  assert.equal(supportsImageData(svg), false);
  assert.equal(supportsImageData('data:image/png;base64,YWJj'), true);
  const messages = apiMessages({ messages: [{ role: 'user', content: [{
    type: 'attachment', attachment: { name: 'reference.svg', type: 'image/svg+xml', size: 6,
      path: '/work/attachments/reference.svg', url: '/api/attachments/fixture/reference.svg' },
  }] }] });
  assert.match(JSON.stringify(messages), /\/work\/attachments\/reference.svg/);
  assert.equal(messages[1].content.some(part => part.type === 'image_url'), false);
});

test('a selection reaches the model as exact block bounds', () => {
  const messages = apiMessages({ messages: [{ role: 'user', content: [
    { type: 'text', text: 'make A steeper' },
    { type: 'selection', selection: { letter: 'A', label: 'Dark oak stairs, top', min: [4, 9, -2], max: [17, 14, 6], blocks: 412 } },
  ] }] });
  assert.deepEqual(messages[1].content, [
    { type: 'text', text: 'make A steeper' },
    { type: 'text', text: '[Selection 1 (Dark oak stairs, top): x 4..17, y 9..14, z -2..6; 412 blocks]' },
  ]);
});

test('several selections reach the model numbered in the order they were picked', () => {
  const messages = apiMessages({ messages: [{ role: 'user', content: [
    { type: 'text', text: 'match the first to the second' },
    { type: 'selection', selection: { letter: 'B', label: 'Glass, top', min: [0, 5, 0], max: [3, 5, 3], blocks: 16 } },
    { type: 'selection', selection: { letter: 'A', label: 'Stone bricks, base', min: [0, 0, 0], max: [3, 0, 3], blocks: 16, block: 'minecraft:stone_bricks' } },
  ] }] });
  assert.deepEqual(messages[1].content.map((part) => part.text), [
    'match the first to the second',
    '[The user picked 2 regions in the viewer, numbered in the order picked]',
    '[Selection 1 of 2 (Glass, top): x 0..3, y 5..5, z 0..3; 16 blocks]',
    '[Selection 2 of 2 (Stone bricks, base): x 0..3, y 0..0, z 0..3; 16 blocks]',
  ]);
});
