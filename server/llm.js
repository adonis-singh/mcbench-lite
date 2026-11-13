'use strict';

const path = require('path');
const { createOpenAIOAuthFetchHandler } = require('openai-oauth');
const settings = require('./settings');
const { codexCompletion, codexSessionFetch } = require('./codex');
const qwencloud = require('./qwencloud');
const cursor = require('./cursor');
const claudeCode = require('./claude-code');
const codexCli = require('./codex-cli');

// Providers call a model API from the app's own turn loop. Harnesses run a
// vendor CLI's own loop under its login, with mcbench's tools attached over
// MCP; they appear in the same picker and go through the same runTurn.
// Labels, key variables and logins live in settings.PROVIDERS.
const PROVIDERS = {
  openrouter: { baseUrl: 'https://openrouter.ai/api/v1', protocol: 'openai' },
  openai: { baseUrl: 'https://api.openai.com/v1', protocol: 'openai' },
  codex: { protocol: 'responses', apiKeyOptional: true },
  anthropic: { baseUrl: 'https://api.anthropic.com/v1', protocol: 'anthropic' },
  groq: { baseUrl: 'https://api.groq.com/openai/v1', protocol: 'openai' },
  google: { baseUrl: 'https://generativelanguage.googleapis.com/v1beta/openai', protocol: 'openai' },
  qwencloud: { baseUrl: 'https://dashscope-intl.aliyuncs.com/compatible-mode/v1', protocol: 'openai' },
  cursor: { protocol: 'cursor' },
  'claude-code': { protocol: 'claude-code' },
  'codex-cli': { protocol: 'codex-cli' },
};

// Which vendor's catalog entries describe a provider's native model ids.
const CATALOG_VENDOR = { codex: 'openai', 'codex-cli': 'openai', 'claude-code': 'anthropic' };

function resolveProvider(providerId) {
  if (!Object.hasOwn(PROVIDERS, providerId)) throw new Error(`Unknown provider: ${providerId}`);
  const { label, envKey } = settings.PROVIDERS[providerId];
  const apiKey = settings.get().providers[providerId].apiKey || (process.env[envKey] || '').trim();
  const state = settings.publicSettings().providers[providerId];
  if (!state.enabled) throw new Error(`${label} is disabled in Settings`);
  if (!state.connected) throw new Error(state.auth === 'cli-login' ? `${label} is not signed in: sign in to it in Settings` : `Connect ${label} in Settings`);
  return { id: providerId, name: label, apiKey, ...PROVIDERS[providerId] };
}

// Live model capabilities, resolved from the openrouter public catalog (no
// key needed): which models exist, whether they take tools, and exactly which
// reasoning efforts they support. Native provider slugs are normalized to
// catalog ids so the same source answers for every provider.
const CATALOG_URL = 'https://openrouter.ai/api/v1/models';
const CATALOG_TTL_MS = 5 * 60 * 1000;
const EFFORT_ORDER = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
let catalogCache = { at: 0, byId: null };
let catalogLoading = null; // callers while it is fetched share the one fetch
const modelListCache = new Map();
// Gemini's own model list: unlike its OpenAI-compatible /models it says which
// models generate content. Its key goes in x-goog-api-key, not as a Bearer.
const GOOGLE_MODELS_URL = 'https://generativelanguage.googleapis.com/v1beta/models';

async function catalog() {
  if (catalogCache.byId && Date.now() - catalogCache.at <= CATALOG_TTL_MS) return catalogCache.byId;
  catalogLoading ||= (async () => {
    const res = await fetch(CATALOG_URL, { signal: AbortSignal.timeout(10000) });
    if (!res.ok) throw new Error(`Model catalog fetch failed with status ${res.status}`);
    const { data } = await res.json();
    catalogCache = { at: Date.now(), byId: new Map(data.map((m) => [m.id, m])) };
    return catalogCache.byId;
  })().finally(() => { catalogLoading = null; });
  return catalogLoading;
}

function catalogIds(providerId, model) {
  if (providerId === 'openrouter' || providerId === 'groq') return [model];
  const bare = model.replace(/-\d{8}$/, ''); // strip date snapshots
  const catalogProvider = CATALOG_VENDOR[providerId] || providerId;
  const ids = [`${catalogProvider}/${bare}`];
  // anthropic native ids use dashes where the catalog uses dots:
  // claude-sonnet-4-6 -> anthropic/claude-sonnet-4.6
  const dotted = bare.replace(/-(\d+)-(\d+)$/, '-$1.$2');
  if (dotted !== bare) ids.push(`${catalogProvider}/${dotted}`);
  return ids;
}

const findCatalogEntry = (byId, providerId, model) => catalogIds(providerId, model).map((id) => byId.get(id)).find(Boolean) || null;

// Claude Code takes Anthropic's native ids; the catalog knows them dotted
// (anthropic/claude-opus-5.5 -> claude-opus-5-5). Variants such as :batch
// are catalog routing, not models.
function claudeCodeModels(byId) {
  return [...byId.values()]
    .filter((entry) => entry.id.startsWith('anthropic/claude-') && !entry.id.includes(':'))
    .map((entry) => ({ id: entry.id.slice('anthropic/'.length).replace(/\./g, '-'), name: entry.name || entry.id, created: entry.created || 0, catalogEntry: entry }));
}

function infoFromCatalogEntry(entry) {
  if (!entry) {
    return {
      known: false,
      imageInput: null,
      imageOutput: false,
      tools: null,
      reasoning: false,
      efforts: [],
      defaultEffort: null,
      mandatoryReasoning: false,
      contextWindow: null,
    };
  }
  const params = entry.supported_parameters || [];
  const architecture = entry.architecture || {};
  const inputModalities = architecture.input_modalities;
  const inputSide = String(architecture.modality || '').split('->')[0];
  const reasoning = entry.reasoning || null;
  const efforts = [...(reasoning && reasoning.supported_efforts || [])]
    .sort((a, b) => EFFORT_ORDER.indexOf(a) - EFFORT_ORDER.indexOf(b));
  return {
    known: true,
    imageInput: Array.isArray(inputModalities)
      ? inputModalities.includes('image')
      : (inputSide ? inputSide.split('+').includes('image') : null),
    imageOutput: (architecture.output_modalities || []).includes('image'),
    tools: params.includes('tools'),
    reasoning: Boolean(reasoning),
    efforts,
    defaultEffort: (reasoning && reasoning.default_effort) || null,
    mandatoryReasoning: Boolean(reasoning && reasoning.mandatory),
    contextWindow: entry.context_length || null,
  };
}

async function modelInfo(providerId, model) {
  if (!Object.hasOwn(PROVIDERS, providerId)) throw new Error(`Unknown provider: ${providerId}`);
  if (providerId === 'qwencloud') return qwencloud.modelInfo(model) || infoFromCatalogEntry(null);
  if (providerId === 'cursor') return await cursor.modelInfo(resolveProvider('cursor').apiKey || null, model) || infoFromCatalogEntry(null);
  // ChatGPT sign-in and the Codex CLI serve the same account's catalogue.
  if (providerId === 'codex' || providerId === 'codex-cli') return await codexCli.modelInfo(model) || infoFromCatalogEntry(null);
  return infoFromCatalogEntry(findCatalogEntry(await catalog(), providerId, model));
}

function providerHeaders(provider) {
  if (provider.apiKeyOptional) return {};
  if (provider.protocol === 'anthropic') {
    return {
      'x-api-key': provider.apiKey,
      'anthropic-version': '2023-06-01',
      'anthropic-beta': 'thinking-binding-controls-2026-08-01',
    };
  }
  return { authorization: `Bearer ${provider.apiKey}` };
}

// ChatGPT sign-in: openai-oauth's request handler runs in this process and
// gets each request as a Request object, so nothing listens on a port. It
// reads the Codex login (codexHome()/auth.json) on every request, refreshes
// the tokens when they are near expiry and writes them back to that file.
// One handler per login file; it caches the account's model catalogue.
const codexHandlers = new Map();
function codexHandler() {
  const authFilePath = path.join(codexCli.codexHome(), 'auth.json');
  if (!codexHandlers.has(authFilePath)) codexHandlers.set(authFilePath, createOpenAIOAuthFetchHandler({ authFilePath, fetch: codexSessionFetch }));
  return codexHandlers.get(authFilePath);
}

function providerFetch(provider, pathname, options) {
  // The handler routes on the path alone; the host is never contacted.
  if (provider.id === 'codex') return codexHandler()(new Request(`http://openai-oauth.invalid/v1${pathname}`, options));
  return fetch(`${provider.baseUrl}${pathname}`, options);
}

async function providerJson(provider, pathname) {
  const res = await providerFetch(provider, pathname, {
    headers: providerHeaders(provider),
    signal: AbortSignal.timeout(30000),
  });
  if (!res.ok) throw new Error(providerErrorMessage(provider, res.status, await res.text()));
  return res.json();
}

async function listModels(providerId) {
  if (providerId === 'codex' || providerId === 'codex-cli') {
    resolveProvider(providerId);
    return codexCli.listModels();
  }
  const cached = modelListCache.get(providerId);
  if (cached && Date.now() - cached.at < CATALOG_TTL_MS) return cached.models;
  const provider = resolveProvider(providerId);
  if (providerId === 'qwencloud') {
    const payload = await providerJson(provider, '/models');
    const models = qwencloud.recentModels(payload.data || []);
    modelListCache.set(providerId, { at: Date.now(), models });
    return models;
  }
  if (providerId === 'cursor') return cursor.listModels(provider.apiKey || null);
  const byId = await catalog();
  let nativeModels;

  if (providerId === 'claude-code') {
    nativeModels = claudeCodeModels(byId);
  } else if (providerId === 'openrouter') {
    nativeModels = [...byId.values()].map((entry) => ({
      id: entry.id,
      name: entry.name || entry.id,
      created: entry.created || 0,
      catalogEntry: entry,
    }));
  } else if (providerId === 'anthropic') {
    nativeModels = [];
    let pathname = '/models?limit=100';
    for (;;) {
      const payload = await providerJson(provider, pathname);
      nativeModels.push(...(payload.data || []).map((entry) => ({
        id: entry.id,
        name: entry.display_name || entry.id,
        created: Math.floor(Date.parse(entry.created_at || 0) / 1000) || 0,
      })));
      if (!payload.has_more) break;
      pathname = `/models?limit=100&after_id=${encodeURIComponent(payload.last_id)}`;
    }
  } else if (providerId === 'google') {
    // Only the model's name and methods come from Google: it publishes no
    // vision, tool or effort facts, so those are the catalog's, as is the
    // release date that orders the list.
    nativeModels = [];
    let token = '';
    do {
      const res = await fetch(`${GOOGLE_MODELS_URL}?pageSize=1000${token && `&pageToken=${encodeURIComponent(token)}`}`, {
        headers: { 'x-goog-api-key': provider.apiKey },
        signal: AbortSignal.timeout(30000),
      });
      if (!res.ok) throw new Error(providerErrorMessage(provider, res.status, await res.text()));
      const payload = await res.json();
      for (const entry of payload.models) {
        if (!entry.supportedGenerationMethods.includes('generateContent')) continue;
        const id = entry.name.slice('models/'.length);
        nativeModels.push({ id, name: entry.displayName, created: findCatalogEntry(byId, providerId, id)?.created || 0 });
      }
      token = payload.nextPageToken;
    } while (token);
  } else {
    const payload = await providerJson(provider, '/models');
    nativeModels = (payload.data || []).map((entry) => ({
      id: entry.id,
      name: entry.name || entry.id,
      created: entry.created || 0,
    }));
  }

  // Newest first: pickers lead with recent releases, not the alphabet.
  // Image-generation models are dead weight in a chat builder; drop them.
  const models = nativeModels.map(({ id, name, created, catalogEntry }) => (
    { id, name, created, ...infoFromCatalogEntry(catalogEntry || findCatalogEntry(byId, providerId, id)) }
  )).filter((model) => !model.imageOutput && !model.id.startsWith('gpt-image-'))
    .sort((a, b) => (b.created - a.created) || a.name.localeCompare(b.name));
  modelListCache.set(providerId, { at: Date.now(), models });
  return models;
}

// `text` is what `provider` answered. A provider can quote the key it was
// sent (OpenAI's 401 does), and the message reaches the page and the server
// log, so every copy of the key becomes [key] first.
function providerErrorMessage(provider, status, answer) {
  const text = provider.apiKey ? answer.replaceAll(provider.apiKey, '[key]') : answer;
  let message = text.slice(0, 400);
  try {
    // Gemini wraps its error object in a one-element array.
    const parsed = JSON.parse(text);
    const payload = Array.isArray(parsed) ? parsed[0] : parsed;
    message = payload.error?.message || payload.error?.error?.message || payload.message || message;
  } catch { /* non-JSON response */ }
  return `LLM request failed (HTTP ${status}): ${message}`;
}

// Gemini's thought signatures ride on its tool calls (extra_content) and
// must go back with them, or Gemini refuses the next request; other
// providers get the bare call.
// `seeBuild` marks a tool result's screenshots for the ChatGPT sign-in
// request (codex.js); chat completion APIs get the message without it.
function remoteMessages(messages, { preserveThinking = false, signatures = false } = {}) {
  return messages.map(({ seeBuild, ...message }) => {
    if (message.role !== 'assistant') return message;
    const { reasoning, reasoning_content, reasoning_details, finish_reason, ...entry } = message;
    if (preserveThinking && (reasoning_content || reasoning)) entry.reasoning_content = reasoning_content || reasoning;
    if (!signatures && entry.tool_calls) entry.tool_calls = entry.tool_calls.map(({ extra_content, ...call }) => call);
    return entry;
  });
}

function dataImage(url) {
  const match = /^data:([^;,]+);base64,(.+)$/.exec(url);
  if (!match) throw new Error('Anthropic images must be base64 data URLs');
  return { media_type: match[1], data: match[2] };
}

function anthropicContent(content) {
  if (typeof content === 'string') return [{ type: 'text', text: content }];
  return content.map((part) => {
    if (part.type === 'text') return { type: 'text', text: part.text };
    if (part.type === 'image_url') {
      return { type: 'image', source: { type: 'base64', ...dataImage(part.image_url.url) } };
    }
    throw new Error(`Unsupported Anthropic content type: ${part.type}`);
  });
}

// Messages needs tool input as an object. A call whose arguments were not
// valid JSON was answered with an error; it is replayed with the raw text so
// the model sees what it sent.
function toolInput(text) {
  try {
    const input = JSON.parse(text || '{}');
    if (input && typeof input === 'object' && !Array.isArray(input)) return input;
  } catch { /* falls through */ }
  return { invalid_arguments: String(text) };
}

function anthropicRequest(messages, tools) {
  const system = messages
    .filter((message) => message.role === 'system')
    .map((message) => message.content)
    .join('\n\n');
  const converted = [];

  for (const message of messages) {
    if (message.role === 'system') continue;
    if (message.role === 'tool') {
      converted.push({
        role: 'user',
        content: [{
          type: 'tool_result',
          tool_use_id: message.tool_call_id,
          content: message.content,
        }],
      });
      continue;
    }

    // Anthropic rejects empty text blocks (e.g. an assistant turn that was
    // pure tool calls, or an image-only user message).
    const content = anthropicContent(message.content || '').filter((part) => part.type !== 'text' || part.text);
    if (message.role === 'assistant') {
      // Thinking blocks must be replayed verbatim (signatures included) or
      // the API rejects the next tool_result turn when thinking is enabled.
      const thinking = (message.reasoning_details || [])
        .filter((block) => block.type === 'thinking' || block.type === 'redacted_thinking');
      content.unshift(...thinking);
      for (const call of message.tool_calls || []) {
        content.push({
          type: 'tool_use',
          id: call.id,
          name: call.function.name,
          input: toolInput(call.function.arguments),
        });
      }
    }
    if (content.length) converted.push({ role: message.role, content });
  }

  return {
    system,
    messages: converted,
    // Arguments stream as the model writes them, not in one burst at the end.
    // They can arrive cut off; runTurn turns unparseable ones into an error.
    tools: (tools || []).map((tool) => ({
      name: tool.function.name,
      description: tool.function.description,
      input_schema: tool.function.parameters,
      eager_input_streaming: true,
    })),
  };
}

async function request(provider, body, signal) {
  const timeout = AbortSignal.timeout(4 * 60 * 60 * 1000);
  const headers = { 'content-type': 'application/json', ...providerHeaders(provider) };
  const endpoint = { anthropic: '/messages', responses: '/responses', openai: '/chat/completions' }[provider.protocol];
  const response = await providerFetch(provider, endpoint, {
    method: 'POST',
    headers,
    body: JSON.stringify(body),
    signal: signal ? AbortSignal.any([signal, timeout]) : timeout,
  });
  if (!response.ok) throw new Error(providerErrorMessage(provider, response.status, await response.text()));
  return response;
}

async function openAiCompletion(provider, { model, messages, tools, effort, signal, onDelta }) {
  const body = {
    model,
    messages: remoteMessages(messages, { preserveThinking: provider.id === 'qwencloud', signatures: provider.id === 'google' }),
    stream: true,
    stream_options: { include_usage: true },
  };
  if (tools?.length) body.tools = tools;
  if (provider.id === 'qwencloud') Object.assign(body, qwencloud.requestOptions(model, effort));
  else if (effort) body.reasoning_effort = effort;
  const response = await request(provider, body, signal);
  const message = { role: 'assistant', content: '' };
  const toolCalls = [];
  let reasoning = '';
  let finishReason = '';

  await readSse(response, (payload) => {
    if (payload.error) throw new Error(providerErrorMessage(provider, response.status, JSON.stringify(payload)));
    if (payload.usage) message.usage = payload.usage;
    const choice = payload.choices?.[0];
    if (!choice) return;
    if (choice.finish_reason) finishReason = choice.finish_reason;
    const delta = choice.delta;
    if (!delta) return;
    if (delta.content) {
      message.content += delta.content;
      onDelta?.({ content: delta.content });
    }
    const reasoningDelta = delta.reasoning || delta.reasoning_content || '';
    if (reasoningDelta) {
      reasoning += reasoningDelta;
      onDelta?.({ reasoning: reasoningDelta });
    }
    for (const part of delta.tool_calls || []) {
      // Gemini sends each call whole, without an index: a part that names a
      // function begins a call, and one that does not goes on with the last.
      const index = part.index ?? (part.function?.name ? toolCalls.length : Math.max(0, toolCalls.length - 1));
      if (!toolCalls[index]) {
        toolCalls[index] = { id: '', type: 'function', function: { name: '', arguments: '' } };
      }
      if (part.id) toolCalls[index].id = part.id;
      if (part.extra_content) toolCalls[index].extra_content = part.extra_content;
      if (part.function?.name) toolCalls[index].function.name += part.function.name;
      if (part.function?.arguments) toolCalls[index].function.arguments += part.function.arguments;
      if (part.function?.name || part.function?.arguments) {
        onDelta?.({
          toolCall: {
            index,
            id: toolCalls[index].id,
            name: toolCalls[index].function.name,
            args: part.function?.arguments || '',
          },
        });
      }
    }
  });

  if (reasoning) message.reasoning = reasoning;
  if (!finishReason) throw new Error('Model stream ended without a finish reason');
  if (['length', 'content_filter'].includes(finishReason)) throw new Error(`Model response incomplete: ${finishReason}`);
  message.finish_reason = finishReason;
  if (toolCalls.length) message.tool_calls = toolCalls;
  return message;
}

// Anthropic's native API: a model with effort levels thinks adaptively and
// returns a readable summary of it (the API default leaves the text empty);
// a chosen effort sets output_config.effort (the shared effort scale is
// Anthropic's own), and without one the API default effort applies. A model
// without effort levels runs with its API defaults.
// The history only grows between requests (agent-context.js), so the prompt
// cache follows the conversation, and a thinking block replayed after an
// edited history is an error, never quietly dropped.
function anthropicBody({ model, messages, tools, effort, adaptive }) {
  const body = { model, ...anthropicRequest(messages, tools), max_tokens: 32768, stream: true, cache_control: { type: 'ephemeral' } };
  if (adaptive) body.thinking = { type: 'adaptive', display: 'summarized', block_binding: { prefix_mismatch_behavior: 'error' } };
  if (effort) body.output_config = { effort };
  return body;
}

// Whether an Anthropic model has effort levels, from the catalog. Looked up
// once per model, so later requests do not depend on OpenRouter. A failed
// lookup reads as a catalog error, not as an HTTP status or network failure
// of the Anthropic request, so completionWithRetry does not retry it.
const adaptiveModels = new Map(); // model -> Promise<boolean>
function anthropicAdaptive(model) {
  if (!adaptiveModels.has(model)) {
    adaptiveModels.set(model, modelInfo('anthropic', model).then((info) => info.efforts.length > 0, (error) => {
      adaptiveModels.delete(model);
      throw new Error(`The OpenRouter model catalog could not describe ${model}: ${error.message}`);
    }));
  }
  return adaptiveModels.get(model);
}

async function anthropicCompletion(provider, { model, messages, tools, effort, signal, onDelta }) {
  const adaptive = await anthropicAdaptive(model);
  const response = await request(provider, anthropicBody({ model, messages, tools, effort, adaptive }), signal);
  const message = { role: 'assistant', content: '' };
  const toolCalls = [];
  // Signed thinking blocks, kept verbatim for replay on the next turn.
  const thinkingBlocks = {};
  let complete = false;
  let reasoning = '';

  await readSse(response, (payload) => {
    if (payload.type === 'error') throw new Error(providerErrorMessage(provider, response.status, JSON.stringify(payload)));
    if (payload.type === 'message_start' && payload.message?.usage) message.usage = payload.message.usage;
    if (payload.type === 'message_delta' && payload.usage) message.usage = { ...message.usage, ...payload.usage };
    if (payload.type === 'message_stop') complete = true;
    if (payload.type === 'content_block_start') {
      const block = payload.content_block;
      if (block?.type === 'tool_use') {
        const call = {
          id: block.id,
          type: 'function',
          function: { name: block.name, arguments: '' },
        };
        toolCalls[payload.index] = call;
        onDelta?.({ toolCall: { index: payload.index, id: call.id, name: call.function.name, args: '' } });
      } else if (block?.type === 'thinking') {
        thinkingBlocks[payload.index] = { type: 'thinking', thinking: '', signature: '' };
      } else if (block?.type === 'redacted_thinking') {
        thinkingBlocks[payload.index] = { type: 'redacted_thinking', data: block.data };
      }
      return;
    }
    if (payload.type === 'content_block_delta') {
      const delta = payload.delta;
      if (delta.type === 'text_delta') {
        message.content += delta.text;
        onDelta?.({ content: delta.text });
      } else if (delta.type === 'thinking_delta') {
        reasoning += delta.thinking;
        if (thinkingBlocks[payload.index]) thinkingBlocks[payload.index].thinking += delta.thinking;
        onDelta?.({ reasoning: delta.thinking });
      } else if (delta.type === 'signature_delta') {
        if (thinkingBlocks[payload.index]) thinkingBlocks[payload.index].signature += delta.signature;
      } else if (delta.type === 'input_json_delta') {
        const call = toolCalls[payload.index];
        call.function.arguments += delta.partial_json;
        onDelta?.({
          toolCall: {
            index: payload.index,
            id: call.id,
            name: call.function.name,
            args: delta.partial_json,
          },
        });
      }
      return;
    }
    if (payload.type === 'message_delta' && payload.delta?.stop_reason) {
      message.finish_reason = payload.delta.stop_reason;
    }
  });

  if (reasoning) message.reasoning = reasoning;
  if (!complete) throw new Error('Anthropic stream ended before message_stop');
  if (message.finish_reason === 'max_tokens') throw new Error('Model response incomplete: max_tokens');
  const details = Object.keys(thinkingBlocks).sort((a, b) => a - b).map((k) => thinkingBlocks[k]);
  if (details.length) message.reasoning_details = details;
  if (toolCalls.length) message.tool_calls = toolCalls.filter(Boolean);
  return message;
}

async function readSse(response, onEvent) {
  const emit = (record) => {
    const data = record.split(/\r?\n/)
      .filter((line) => line.startsWith('data:'))
      .map((line) => line.slice(5).trim())
      .join('\n');
    if (data && data !== '[DONE]') onEvent(JSON.parse(data));
  };
  const decoder = new TextDecoder();
  let buffer = '';
  for await (const chunk of response.body) {
    buffer += decoder.decode(chunk, { stream: true });
    const records = buffer.split(/\r?\n\r?\n/);
    buffer = records.pop();
    records.forEach(emit);
  }
  if (buffer.trim()) emit(buffer);
}

function normalizeUsage(usage) {
  if (!usage) return null;
  const number = value => Number.isFinite(value) ? value : null;
  const anthropic = usage.cache_creation_input_tokens !== undefined || usage.cache_read_input_tokens !== undefined;
  return {
    inputTokens: anthropic
      ? number((usage.input_tokens || 0) + (usage.cache_creation_input_tokens || 0) + (usage.cache_read_input_tokens || 0))
      : number(usage.input_tokens ?? usage.prompt_tokens),
    outputTokens: number(usage.output_tokens ?? usage.completion_tokens),
    cachedInputTokens: number(usage.input_tokens_details?.cached_tokens ?? usage.prompt_tokens_details?.cached_tokens ?? usage.cache_read_input_tokens),
    cacheWriteTokens: number(usage.cache_creation_input_tokens),
    reasoningTokens: number(usage.output_tokens_details?.reasoning_tokens ?? usage.completion_tokens_details?.reasoning_tokens),
  };
}

const COMPLETIONS = {
  openai: openAiCompletion,
  anthropic: anthropicCompletion,
  responses: (provider, options) => codexCompletion(provider, options, request, readSse),
  cursor: cursor.cursorCompletion,
  'claude-code': claudeCode.completion,
  'codex-cli': codexCli.completion,
};

async function chatCompletion(options) {
  const provider = resolveProvider(options.provider);
  const start = Date.now();
  let firstDeltaMs = null;
  // The reply text streamed so far, which a completion that fails keeps as
  // `reply` on its error: a stopped turn stores what the user saw.
  let reply = '';
  const tracked = { ...options, onDelta: delta => {
    firstDeltaMs ??= Date.now() - start;
    if (delta.content) reply += delta.content;
    options.onDelta?.(delta);
  } };
  const base = () => ({ provider: options.provider, model: options.model, effort: options.effort || null,
    startedAt: new Date(start).toISOString(), wallMs: Date.now() - start, firstDeltaMs,
    contextBytes: Buffer.byteLength(JSON.stringify(options.messages)), messageCount: options.messages.length });
  try {
    const message = await COMPLETIONS[provider.protocol](provider, tracked);
    const usage = normalizeUsage(message.usage);
    // How much of the model's context the latest request filled: one request
    // per completion for API providers; harnesses report it when they can.
    const contextTokens = provider.protocol === 'openai' || provider.protocol === 'anthropic' || provider.protocol === 'responses'
      ? usage?.inputTokens ?? null : message.contextTokens ?? null;
    message.telemetry = { ...base(), usage, contextTokens };
    delete message.usage;
    delete message.contextTokens;
    return message;
  } catch (error) {
    error.telemetry = { ...base(), usage: null, failed: true, cancelled: Boolean(options.signal?.aborted) };
    error.reply = reply;
    throw error;
  }
}

// A harness keeps its CLI or agent running only within a turn. Called when a
// turn ends for any reason, so an error mid-turn cannot leave one waiting.
function closeHarnessSession(conversationId) {
  claudeCode.closeSession(conversationId);
  codexCli.closeSession(conversationId);
  cursor.closeSession(conversationId);
}

module.exports = { chatCompletion, closeHarnessSession, resolveProvider, modelInfo, listModels, PROVIDERS };
module.exports._internal = {
  anthropicRequest, anthropicBody, dataImage, remoteMessages, readSse, providerErrorMessage, catalogIds, infoFromCatalogEntry, normalizeUsage, claudeCodeModels, codexHandlers,
};
