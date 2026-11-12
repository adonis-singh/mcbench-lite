// Which logo in /icons stands for a provider, and for a model.

// Providers drawn with another one's logo: Google's API serves Gemini.
const PROVIDER_LOGOS = {
  qwencloud: 'qwen', anthropic: 'claude', 'claude-code': 'claude', 'codex-cli': 'codex', google: 'gemini',
};
export const logoName = (name) => PROVIDER_LOGOS[name] || name;

const VENDOR_PREFIXES = [
  ['openai/', 'openai'], ['anthropic/', 'anthropic'], ['x-ai/', 'xai'],
  ['google/', 'gemini'], ['moonshotai/', 'moonshot'], ['deepseek/', 'deepseek'],
  ['qwen/', 'qwen'], ['mistralai/', 'mistral'], ['meta-llama/', 'meta'],
];
const VENDOR_HINTS = [
  ['llama', 'meta'], ['qwen', 'qwen'], ['kimi', 'moonshot'], ['moonshot', 'moonshot'],
  ['deepseek', 'deepseek'], ['mistral', 'mistral'], ['mixtral', 'mistral'],
  ['gemma', 'gemini'], ['gemini', 'gemini'], ['gpt', 'openai'], ['whisper', 'openai'],
  ['claude', 'anthropic'], ['grok', 'xai'],
];

// A model's logo is its maker's: a provider that serves only its own
// models says it outright, a reseller's model by its id.
export function vendorIcon(provider, modelId) {
  if (provider === 'openai') return 'openai';
  if (provider === 'codex' || provider === 'codex-cli') return 'codex';
  if (provider === 'anthropic' || provider === 'claude-code') return 'anthropic';
  if (provider === 'google') return 'gemini';
  const id = modelId.toLowerCase();
  for (const [prefix, icon] of VENDOR_PREFIXES) if (id.startsWith(prefix)) return icon;
  for (const [hint, icon] of VENDOR_HINTS) if (id.includes(hint)) return icon;
  return provider === 'groq' || provider === 'cursor' ? provider : 'openrouter';
}
