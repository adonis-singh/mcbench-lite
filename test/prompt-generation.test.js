'use strict';

// Its own settings file: never the checkout's, which holds the owner's keys.
process.env.MCBENCH_SETTINGS_FILE = require('node:path').join(require('node:fs').mkdtempSync(require('node:path').join(require('node:os').tmpdir(), 'mcbench-settings-test-')), 'settings.json');

const test = require('node:test');
const assert = require('node:assert');

const { generatePrompt, INSTRUCTIONS } = require('../server/prompt-generation');

test('generatePrompt sends the text with short instructions that mention fill-ins', async () => {
  let request;
  const result = await generatePrompt({
    provider: 'openrouter',
    model: 'openai/gpt-5.6-luna',
    effort: 'low',
    text: '  make a telescope in a desert  ',
  }, async (options) => {
    request = options;
    return { content: '  Build a desert observatory around a large {{style}} telescope.  ' };
  });

  assert.strictEqual(result, 'Build a desert observatory around a large {{style}} telescope.');
  assert.strictEqual(request.provider, 'openrouter');
  assert.strictEqual(request.model, 'openai/gpt-5.6-luna');
  assert.strictEqual(request.effort, 'low');
  assert.deepStrictEqual(request.tools, []);
  assert.deepStrictEqual(request.messages, [
    { role: 'system', content: INSTRUCTIONS },
    { role: 'user', content: 'make a telescope in a desert' },
  ]);
  assert.match(INSTRUCTIONS, /\{\{style\}\}/);
  assert.match(INSTRUCTIONS, /Do not add specifics/);
});

test('generatePrompt needs a model and some text, and rejects an empty response', async () => {
  await assert.rejects(generatePrompt({ model: 'm', text: 'x' }), /Choose a provider/);
  await assert.rejects(generatePrompt({ provider: 'openai', model: 'm', text: '  ' }), /Write something to improve/);
  await assert.rejects(
    generatePrompt({ provider: 'openai', model: 'm', text: 'x' }, async () => ({ content: '  ' })),
    /empty prompt/,
  );
});
