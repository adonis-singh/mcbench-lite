'use strict';

// Curated September 2026 releases, intersected with the authenticated /models
// response. Native creation timestamps also change when old aliases are
// redeployed, so a date cutoff alone would bring legacy models back.
// Capabilities and thinking parameters follow QwenCloud, not OpenRouter:
// https://docs.qwencloud.com/api-reference/chat/openai-chat
// https://docs.qwencloud.com/developer-guides/getting-started/text-generation-models
const MODELS = new Map([
  ['qwen3.8-max-0902', { name: 'Qwen3.8 Max (0902)', imageInput: true, nativeEffort: true, preserveThinking: true }],
  ['qwen3.8-max', { name: 'Qwen3.8 Max', imageInput: true, nativeEffort: true, preserveThinking: true }],
  ['qwen3.8-flash', { name: 'Qwen3.8 Flash', imageInput: true, nativeEffort: true, preserveThinking: true }],
  ['qwen3.8-2.4t-a95b', { name: 'Qwen3.8 2.4T A95B', imageInput: false, nativeEffort: true, mandatoryReasoning: true }],
  ['qwen3.8-27b', { name: 'Qwen3.8 27B', imageInput: true, nativeEffort: true }],
  ['qwen3.7-max', { name: 'Qwen3.7 Max', imageInput: false, preserveThinking: true }],
  ['qwen3.7-plus', { name: 'Qwen3.7 Plus', imageInput: true, preserveThinking: true }],
  ['qwen3.7-flash', { name: 'Qwen3.7 Flash', imageInput: true, preserveThinking: true }],
]);

// UI effort levels for models that expose a token budget instead of an enum.
const THINKING_BUDGETS = { low: 4096, medium: 16384, high: 32768, xhigh: 65536, max: 262144 };

function modelInfo(id) {
  const model = MODELS.get(id);
  if (!model) return null;
  return {
    known: true,
    imageInput: model.imageInput,
    imageOutput: false,
    tools: true,
    reasoning: true,
    efforts: [
      ...(model.mandatoryReasoning ? [] : ['none']),
      ...(model.nativeEffort ? ['low', 'medium', 'xhigh'] : Object.keys(THINKING_BUDGETS)),
    ],
    defaultEffort: model.nativeEffort ? 'xhigh' : 'max',
    mandatoryReasoning: Boolean(model.mandatoryReasoning),
  };
}

function recentModels(entries) {
  const available = new Map(entries.map((entry) => [entry.id, entry]));
  return [...MODELS].filter(([id]) => available.has(id)).map(([id, model]) => ({
    id,
    name: model.name,
    created: available.get(id).created || 0,
    ...modelInfo(id),
  }));
}

function requestOptions(id, effort) {
  const model = MODELS.get(id);
  if (!model) return {};
  if (model.mandatoryReasoning && effort === 'none') {
    throw new Error(`${id} requires thinking; choose low, medium, or xhigh`);
  }
  const options = { enable_thinking: effort !== 'none' };
  if (model.preserveThinking) options.preserve_thinking = true;
  if (!effort || effort === 'none') return options;
  if (model.nativeEffort) {
    const normalized = { minimal: 'low', high: 'xhigh', max: 'xhigh' }[effort] || effort;
    if (!['low', 'medium', 'xhigh'].includes(normalized)) throw new Error(`Unsupported QwenCloud reasoning effort: ${effort}`);
    options.reasoning_effort = normalized;
  } else {
    const budget = THINKING_BUDGETS[effort === 'minimal' ? 'low' : effort];
    if (!budget) throw new Error(`Unsupported QwenCloud reasoning effort: ${effort}`);
    options.thinking_budget = budget;
  }
  return options;
}

module.exports = { modelInfo, recentModels, requestOptions };
