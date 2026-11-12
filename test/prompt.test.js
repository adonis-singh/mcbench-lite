'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { after, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-prompt-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const { systemPrompt } = require('../server/prompt');
const { TEST_BUILD_TOOL } = require('../server/verification-contract');

after(() => fs.rmSync(dataDir, { recursive: true, force: true }));

// The app has no Paper server: the user watches the build in the page, where
// the same engine runs it live.
test('the prompt describes the live world in the browser, not a Paper server', () => {
  for (const imageInput of [true, false]) {
    const prompt = systemPrompt({ imageInput });
    assert.doesNotMatch(prompt, /Paper|playable|In-game|blueprint renderer|browser map/);
    assert.match(prompt, /runs it live at 20 ticks a second/);
    assert.match(prompt, /pull levers, press buttons/);
    assert.match(prompt, /not copied back into the blueprint/);
  }
  assert.doesNotMatch(TEST_BUILD_TOOL.function.description, /playable/);
});

test('the prompt says what mc.find does past 500 matches and that nothing ticks randomly', () => {
  const prompt = systemPrompt();
  assert.doesNotMatch(prompt, /limited to 500/);
  assert.match(prompt, /more than 500 matches is an error/);
  assert.match(prompt, /no random ticks, weather or change of daylight/);
});
