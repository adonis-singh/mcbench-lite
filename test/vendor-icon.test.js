'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const { test } = require('node:test');

test('the google provider, and every model it serves, draw with the Gemini logo', async () => {
  const { logoName, vendorIcon } = await import('../web/vendor-icon.mjs');
  assert.equal(logoName('google'), 'gemini');
  assert.equal(vendorIcon('google', 'gemini-2.5-pro'), 'gemini');
  assert.equal(vendorIcon('google', 'models/imagen-4'), 'gemini');
  assert.equal(vendorIcon('openrouter', 'google/gemma-3-27b'), 'gemini');
});

test('the other providers keep their logos', async () => {
  const { logoName, vendorIcon } = await import('../web/vendor-icon.mjs');
  assert.deepStrictEqual(['qwencloud', 'anthropic', 'claude-code', 'codex-cli', 'openai', 'groq'].map(logoName), ['qwen', 'claude', 'claude', 'codex', 'openai', 'groq']);
  assert.equal(vendorIcon('claude-code', 'claude-haiku-5-5'), 'anthropic');
  assert.equal(vendorIcon('openrouter', 'x-ai/grok-4'), 'xai');
  assert.equal(vendorIcon('groq', 'some-model'), 'groq');
  assert.equal(vendorIcon('openrouter', 'unknown/model'), 'openrouter');
});

test('every logo a provider or model can name is in web/icons', async () => {
  const { logoName, vendorIcon } = await import('../web/vendor-icon.mjs');
  const icons = new Set(fs.readdirSync(path.join(__dirname, '..', 'web', 'icons')).map((file) => file.replace(/\.svg$/, '')));
  for (const provider of ['google', 'qwencloud', 'anthropic', 'claude-code', 'codex-cli', 'openai', 'groq', 'cursor', 'openrouter', 'codex']) {
    assert.ok(icons.has(logoName(provider)), provider);
  }
  for (const id of ['gpt-5', 'gemini-3', 'kimi-k2', 'llama-4', 'deepseek-v4', 'mixtral', 'qwen3', 'grok-4', 'claude-x']) {
    assert.ok(icons.has(vendorIcon('openrouter', id)), id);
  }
});
