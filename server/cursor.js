'use strict';

// Cursor models run as local Cursor SDK agents whose only tools are mcbench's
// own (exposed as SDK custom tools). Each agent tool call is relayed as an
// ordinary completion tool call through the shared harness session, so
// runTurn executes and persists it like any other provider's; the next
// completion delivers its result to the waiting call.

const fs = require('fs');
const path = require('path');
// The SDK takes about 100 ms to load, so it loads when Cursor is first used.
let sdk = null;
const cursorSdk = () => (sdk ||= require('@cursor/sdk'));
const { RelaySession, sessionRelay, harnessPrompt, systemText, DATA_DIR } = require('./harness');

const AGENT_DIR = path.join(DATA_DIR, 'cursor-agent');
// The SDK serves custom tools to the agent as this MCP server.
const CUSTOM_TOOLS_SERVER = 'custom-user-tools';
const CATALOG_TTL_MS = 5 * 60 * 1000;
const EFFORT_ORDER = ['none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max', 'on'];
const EFFORT_PARAMS = new Set(['effort', 'reasoning_effort', 'reasoning']);

let catalog = { at: 0, apiKey: null, models: null };

function storedLogin() {
  return fs.existsSync(cursorSdk().getDefaultSdkAuthPath());
}

// The package's exports map hides package.json from require.
const version = () => JSON.parse(fs.readFileSync(path.resolve(path.dirname(require.resolve('@cursor/sdk')), '../../package.json'), 'utf8')).version;

// One picker entry per model, plus a "Fast" entry when the model has a fast
// mode. Context size and other settings keep the model's default variant.
function expandModels(list) {
  const models = new Map();
  for (const model of list) {
    const defaults = new Map((model.variants?.find((variant) => variant.isDefault)?.params || []).map((p) => [p.id, p.value]));
    const definitions = model.parameters || [];
    const effortParam = definitions.find((p) => EFFORT_PARAMS.has(p.id));
    const thinkingParam = definitions.find((p) => p.id === 'thinking');
    const efforts = new Map();
    for (const { value } of effortParam?.values || []) {
      efforts.set(value === 'extra-high' ? 'xhigh' : value, { [effortParam.id]: value, ...(thinkingParam ? { thinking: 'true' } : {}) });
    }
    if (thinkingParam) {
      efforts.set('none', { thinking: 'false' });
      if (!effortParam) efforts.set('on', { thinking: 'true' });
    }
    const effortNames = [...efforts.keys()].sort((a, b) => EFFORT_ORDER.indexOf(a) - EFFORT_ORDER.indexOf(b));
    const defaultEffort = thinkingParam && defaults.get('thinking') === 'false' ? 'none'
      : effortParam ? effortNames.find((name) => efforts.get(name)[effortParam.id] === defaults.get(effortParam.id)) || null
        : thinkingParam ? 'on' : null;
    const hasFast = definitions.some((p) => p.id === 'fast');
    const fastModes = hasFast ? ['false', 'true'] : [null];
    for (const fast of fastModes) {
      const id = fast === 'true' ? `${model.id}-fast` : model.id;
      models.set(id, {
        id,
        name: fast === 'true' ? `${model.displayName} Fast` : model.displayName,
        cursorId: model.id,
        defaults: fast ? new Map([...defaults, ['fast', fast]]) : defaults,
        efforts,
        info: {
          known: true,
          imageInput: null,
          imageOutput: false,
          tools: true,
          reasoning: effortNames.length > 0,
          efforts: effortNames,
          defaultEffort,
          mandatoryReasoning: effortNames.length > 0 && !efforts.has('none'),
          // The default variant's context size, e.g. "1m" or "300k".
          contextWindow: contextSize(defaults.get('context')),
          fast: fast === 'true',
          fastNote: null,
        },
      });
    }
  }
  return models;
}

function contextSize(value) {
  const match = /^(\d+(?:\.\d+)?)([km])$/i.exec(value || '');
  return match ? Math.round(Number(match[1]) * (match[2].toLowerCase() === 'm' ? 1e6 : 1e3)) : null;
}

// Callers while it is fetched share the one fetch for their key.
const loading = new Map(); // apiKey -> Promise
async function models(apiKey) {
  if (catalog.models && catalog.apiKey === apiKey && Date.now() - catalog.at <= CATALOG_TTL_MS) return catalog.models;
  if (!loading.has(apiKey)) {
    loading.set(apiKey, cursorSdk().Cursor.models.list(apiKey ? { apiKey } : undefined).then((list) => {
      catalog = { at: Date.now(), apiKey, models: expandModels(list) };
      return catalog.models;
    }).finally(() => loading.delete(apiKey)));
  }
  return loading.get(apiKey);
}

async function listModels(apiKey) {
  return [...(await models(apiKey)).values()].map(({ id, name, info }) => ({ id, name, created: 0, ...info }));
}

async function modelInfo(apiKey, id) {
  return (await models(apiKey)).get(id)?.info || null;
}

function modelSelection(model, effort) {
  const params = new Map(model.defaults);
  if (effort) {
    const override = model.efforts.get(effort);
    if (!override) throw new Error(`${model.name} does not support reasoning effort ${effort}`);
    for (const [key, value] of Object.entries(override)) params.set(key, value);
  }
  return { id: model.cursorId, params: [...params].map(([id, value]) => ({ id, value })) };
}

// A new agent sees the mcbench system prompt and the whole transcript as its
// first message; its own conversation then continues within the turn.
function renderPrompt(messages) {
  const { text, images } = harnessPrompt(messages, { history: true, notes: [] });
  return { text: `<instructions>\n${systemText(messages)}\n</instructions>\n\n${text}`, images };
}

class CursorSession extends RelaySession {
  constructor(signal) {
    super('Cursor', signal);
    this.agent = null;
    this.run = null;
  }

  tools(definitions) {
    return Object.fromEntries(definitions.map((tool) => [tool.function.name, {
      description: tool.function.description,
      inputSchema: tool.function.parameters,
      execute: (args, context) => this.call(context.toolCallId, tool.function.name, args)
        .then((content) => ({ content }), (error) => ({ content: [{ type: 'text', text: error.message }], isError: true })),
    }]));
  }

  update(update) {
    if (update.type === 'tool-call-started') this.checkToolCall(update.toolCall);
    else if (update.type === 'text-delta') super.update({ text: update.text });
    else if (update.type === 'thinking-delta') super.update({ reasoning: update.text });
    else if (update.type === 'summary-started') this.status('Summarizing context');
    else if (update.type === 'summary-completed') this.notice('info', 'Cursor summarized the conversation to fit its context window');
    else if (update.type === 'turn-ended' && update.usage) {
      // A run is many model turns; its usage is their sum. The last turn's
      // input is how full the context was.
      const u = update.usage;
      const sum = (a, b) => (b === undefined ? a : (a || 0) + b);
      const total = this.usage || {};
      super.update({ usage: {
        input_tokens: sum(total.input_tokens, u.inputTokens), output_tokens: sum(total.output_tokens, u.outputTokens),
        cache_read_input_tokens: sum(total.cache_read_input_tokens, u.cacheReadTokens),
        cache_creation_input_tokens: sum(total.cache_creation_input_tokens, u.cacheWriteTokens),
        ...(u.reasoningTokens !== undefined ? { output_tokens_details: { reasoning_tokens: sum(total.output_tokens_details?.reasoning_tokens, u.reasoningTokens) } } : {}),
      } });
      this.setContextTokens((u.inputTokens || 0) + (u.cacheReadTokens || 0) + (u.cacheWriteTokens || 0));
    }
  }

  // Cursor is told to offer only mcbench's tools (`tools: ['mcp']` with the
  // custom tools), as Claude Code and Codex are, and as with them the run
  // ends if the agent starts any other tool: a built-in, another MCP server,
  // or a tool the SDK has no type for (it passes those on as the raw
  // protocol value, whose kind is in `case`). Checked against @cursor/sdk
  // 1.0.32 and 1.3.1, where a custom tool starts as type "mcp" with this
  // server's name and the tool's name.
  checkToolCall(toolCall) {
    const { type, args } = toolCall;
    if (type === 'mcp' && args.providerIdentifier === CUSTOM_TOOLS_SERVER && this.allowed.has(args.toolName)) return;
    const name = type === 'mcp' ? `mcp ${args.providerIdentifier} ${args.toolName}` : type ?? toolCall.case;
    const error = new Error(`Cursor used a tool outside mcbench: ${name}`);
    this.stop();
    this.finish(error);
  }

  // Creates the agent and sends the prompt. A stop during either await is
  // honored once it returns: what it opened is closed and start throws.
  async start({ model: id, effort, tools = [], messages }, provider) {
    const { text, images } = renderPrompt(messages);
    const model = (await models(provider.apiKey || null)).get(id);
    if (!model) throw new Error(`Unknown Cursor model: ${id}`);
    this.throwIfClosed();
    this.allowed = new Set(tools.map((tool) => tool.function.name));
    fs.mkdirSync(AGENT_DIR, { recursive: true });
    const agent = await cursorSdk().Agent.create({
      ...(provider.apiKey ? { apiKey: provider.apiKey } : {}),
      model: modelSelection(model, effort),
      tools: ['mcp'],
      local: { cwd: AGENT_DIR, settingSources: [], customTools: this.tools(tools) },
    });
    if (this.state === 'closed') {
      agent.close();
      throw this.closeReason();
    }
    this.agent = agent;
    const run = await agent.send(images.length ? { text, images } : text, { onDelta: ({ update }) => this.update(update) });
    // Closing meanwhile closed the agent, which ends the run.
    this.throwIfClosed();
    this.follow(run);
  }

  follow(run) {
    this.run = run;
    run.wait().then(
      (result) => this.finish(result.status === 'finished' ? null : new Error(`Cursor run ${result.status}${result.error ? `: ${result.error.message}` : ''}`)),
      (error) => this.finish(error),
    );
  }

  stop() {
    if (this.run?.supports('cancel')) this.run.cancel().catch(() => {});
  }

  release() {
    this.agent?.close();
  }
}

// One run per conversation, as the CLI harnesses keep theirs.
const { completion: cursorCompletion, closeSession } = sessionRelay(CursorSession);

module.exports = { cursorCompletion, closeSession, listModels, modelInfo, storedLogin, version };
module.exports._internal = { models, expandModels, modelSelection, renderPrompt, CursorSession };
