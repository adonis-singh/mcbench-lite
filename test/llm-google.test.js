'use strict';

// Own file: the model catalog and model lists are cached per process.
const { test } = require('node:test');
const path = require('node:path');
const fs = require('node:fs');
const os = require('node:os');
process.env.MCBENCH_SETTINGS_FILE = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-settings-test-')), 'settings.json');
const assert = require('node:assert');

const { listModels, modelInfo } = require('../server/llm');

const CATALOG = {
  data: [
    { id: 'google/gemini-3.8-flash', created: 300, context_length: 1048576, architecture: { input_modalities: ['text', 'image'], output_modalities: ['text'] }, supported_parameters: ['tools', 'reasoning'], reasoning: { mandatory: true, supported_efforts: ['high', 'medium', 'low'], default_effort: 'medium' } },
    { id: 'google/gemini-2.5-flash', created: 100, context_length: 1048576, architecture: { input_modalities: ['text', 'image'], output_modalities: ['text'] }, supported_parameters: ['tools'] },
    { id: 'google/gemini-3.1-flash-image', created: 200, architecture: { input_modalities: ['text', 'image'], output_modalities: ['image', 'text'] }, supported_parameters: [] },
  ],
};

// Gemini's own model list, as models.list returns it: two pages, and models
// that cannot chat (embeddings, live audio) beside the ones that can.
const PAGES = {
  '': { models: [
    { name: 'models/gemini-2.5-flash', displayName: 'Gemini 2.5 Flash', inputTokenLimit: 1048576, supportedGenerationMethods: ['generateContent', 'countTokens'] },
    { name: 'models/gemini-embedding-2', displayName: 'Gemini Embedding 2', inputTokenLimit: 8192, supportedGenerationMethods: ['embedContent'] },
  ], nextPageToken: 'page-2' },
  'page-2': { models: [
    { name: 'models/gemini-3.8-flash', displayName: 'Gemini 3.8 Flash', inputTokenLimit: 1048576, supportedGenerationMethods: ['generateContent'] },
    { name: 'models/gemini-3.1-flash-image', displayName: 'Nano Banana 2', supportedGenerationMethods: ['generateContent'] },
    { name: 'models/gemini-3.8-live', displayName: 'Gemini 3.8 Live', supportedGenerationMethods: ['bidiGenerateContent'] },
  ] },
};

test('Google lists the models its key can chat with, described by the catalog, newest first', async () => {
  const previousFetch = global.fetch;
  const previousKey = process.env.GOOGLE_API_KEY;
  process.env.GOOGLE_API_KEY = 'test-key';
  const pages = [];
  global.fetch = async (url, options) => {
    if (url === 'https://openrouter.ai/api/v1/models') return Response.json(CATALOG);
    const parsed = new URL(url);
    assert.strictEqual(`${parsed.origin}${parsed.pathname}`, 'https://generativelanguage.googleapis.com/v1beta/models');
    // Gemini's own API takes its key in this header; a Bearer key is refused there.
    assert.strictEqual(options.headers['x-goog-api-key'], 'test-key');
    assert.strictEqual(options.headers.authorization, undefined);
    const token = parsed.searchParams.get('pageToken') || '';
    pages.push(token);
    return Response.json(PAGES[token]);
  };
  try {
    const models = await listModels('google');
    assert.deepStrictEqual(pages, ['', 'page-2']);
    assert.deepStrictEqual(models.map((m) => [m.id, m.name, m.created]), [
      ['gemini-3.8-flash', 'Gemini 3.8 Flash', 300],
      ['gemini-2.5-flash', 'Gemini 2.5 Flash', 100],
    ]);
    assert.deepStrictEqual(
      (({ imageInput, tools, efforts, defaultEffort, mandatoryReasoning, contextWindow }) => ({ imageInput, tools, efforts, defaultEffort, mandatoryReasoning, contextWindow }))(models[0]),
      { imageInput: true, tools: true, efforts: ['low', 'medium', 'high'], defaultEffort: 'medium', mandatoryReasoning: true, contextWindow: 1048576 },
    );
    assert.deepStrictEqual((await modelInfo('google', 'gemini-3.8-flash')).efforts, ['low', 'medium', 'high']);
  } finally {
    global.fetch = previousFetch;
    if (previousKey === undefined) delete process.env.GOOGLE_API_KEY;
    else process.env.GOOGLE_API_KEY = previousKey;
  }
});
