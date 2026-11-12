'use strict';

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-codex-cli-test-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const codexCli = require('../server/codex-cli');
const harness = require('../server/harness');

after(() => fs.rmSync(dataDir, { recursive: true, force: true }));

test('the Codex model catalogue lists visible models in priority order with their native efforts', () => {
  const level = (effort) => ({ effort, description: '' });
  const models = codexCli.modelsFromCache({ models: [
    { slug: 'gpt-reserve', display_name: 'Reserve', visibility: 'hide', supported_reasoning_levels: [level('low')], default_reasoning_level: 'low', input_modalities: ['text', 'image'], priority: 1 },
    { slug: 'gpt-5.6-sol', display_name: 'GPT-5.6-Sol', visibility: 'list', additional_speed_tiers: ['fast'], service_tiers: [{ id: 'priority', name: 'Fast', description: '2x speed' }], supported_reasoning_levels: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'].map(level), default_reasoning_level: 'low', input_modalities: ['text', 'image'], priority: 5 },
    { slug: 'gpt-6-astra', display_name: 'GPT-6-Astra', visibility: 'list', supported_reasoning_levels: ['low', 'medium', 'high'].map(level), default_reasoning_level: 'medium', input_modalities: ['text'], priority: 2 },
  ] });
  assert.deepEqual(models.map((m) => m.id), ['gpt-6-astra', 'gpt-5.6-sol']);
  assert.deepEqual(models[1], {
    id: 'gpt-5.6-sol', name: 'GPT-5.6-Sol', created: 0, known: true, imageInput: true, imageOutput: false, tools: true,
    reasoning: true, efforts: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'], defaultEffort: 'low', mandatoryReasoning: true,
    contextWindow: null, fast: true, fastNote: '2x speed',
  });
  assert.equal(models[0].imageInput, false);
});

test('the container configuration pins the model, effort, instructions, disabled features and the mcbench MCP server', () => {
  const toml = codexCli.configToml({ model: 'gpt-6-astra', effort: 'high', systemPrompt: 'Build "well".\nLine two' });
  assert.match(toml, /^model = "gpt-6-astra"\nmodel_reasoning_effort = "high"\napproval_policy = "never"\nsandbox_mode = "danger-full-access"\n/);
  assert.match(toml, /developer_instructions = "Build \\"well\\"\.\\nLine two"\n/);
  assert.match(toml, /\nfeatures\.shell_tool = false\n/);
  assert.match(toml, /\nmcp_servers\.mcbench\.command = "python3"\nmcp_servers\.mcbench\.args = \["\/run\/mcbench\/mcp-bridge\.py", "\/run\/mcbench\/mcp\.sock"\]\n/);
  assert.doesNotMatch(codexCli.configToml({ model: 'm', systemPrompt: 's' }), /model_reasoning_effort/);
});

test('the host run ignores user config and rules, passes the same settings as overrides, and denies writes', () => {
  const bridge = path.join(__dirname, '..', 'server', 'mcp-bridge.py');
  const args = codexCli.hostArgs({ model: 'gpt-6-astra', effort: 'low', systemPrompt: 'S', socket: '/tmp/m.sock', resume: false, threadId: null, images: ['/tmp/image-1.png'] });
  assert.deepEqual(args.slice(0, 6), ['exec', '--json', '--skip-git-repo-check', '--ignore-user-config', '--ignore-rules', '-c']);
  const overrides = args.filter((arg, index) => args[index - 1] === '-c');
  assert.ok(overrides.includes('model="gpt-6-astra"') && overrides.includes('model_reasoning_effort="low"'));
  assert.ok(overrides.includes('sandbox_mode="read-only"') && overrides.includes('approval_policy="never"'));
  assert.ok(overrides.includes('features.shell_tool=false') && overrides.includes('developer_instructions="S"'));
  // Codex 0.167.1 turns these on by default, and each gives the model a tool:
  // subagents (multi_agent_v2) and a code-running host (code_mode_host).
  for (const feature of ['multi_agent', 'multi_agent_v2', 'code_mode_host']) assert.ok(overrides.includes(`features.${feature}=false`), feature);
  assert.ok(overrides.includes(`mcp_servers.mcbench.args=[${JSON.stringify(bridge)}, "/tmp/m.sock"]`));
  assert.ok(overrides.includes('mcp_servers.mcbench.default_tools_approval_mode="approve"'));
  assert.deepEqual(args.slice(-3), ['-i', '/tmp/image-1.png', '-']);
  const resumed = codexCli.hostArgs({ model: 'm', systemPrompt: 'S', socket: '/tmp/m.sock', resume: true, threadId: 'thread-1', images: [] });
  assert.deepEqual(resumed.slice(0, 3), ['exec', 'resume', 'thread-1']);
  assert.ok(!resumed.some((arg) => arg.startsWith('model_reasoning_effort')));
  assert.equal(codexCli.hostEnv().OPENAI_API_KEY, undefined);
});

// The check is an allow list: a kind of item the app does not expect, one a
// later Codex might add among them, ends the run as a shell command does.
test('only text, reasoning, plan, error and mcbench tool items pass; anything else ends a Codex run', () => {
  for (const type of ['command_execution', 'file_change', 'web_search', 'collab_tool_call', 'image_generation', 'some_future_tool']) {
    assert.throws(() => codexCli.checkItem({ type }), new RegExp(`outside mcbench: ${type}$`));
  }
  assert.throws(() => codexCli.checkItem({ type: 'mcp_tool_call', server: 'other', tool: 'x' }), /mcp_tool_call other/);
  codexCli.checkItem({ type: 'mcp_tool_call', server: 'mcbench', tool: 'build' });
  codexCli.checkItem({ type: 'agent_message', text: 'hi' });
  codexCli.checkItem({ type: 'reasoning', text: 'thinking' });
  codexCli.checkItem({ type: 'todo_list', items: [] });
  codexCli.checkItem({ type: 'error', message: 'a warning' });
});

test('Codex turn usage maps onto the OpenAI token accounting the app normalizes', () => {
  assert.deepEqual(codexCli.usageOf({ input_tokens: 10, cached_input_tokens: 4, output_tokens: 6, reasoning_output_tokens: 2 }), {
    input_tokens: 10, output_tokens: 6, input_tokens_details: { cached_tokens: 4 }, output_tokens_details: { reasoning_tokens: 2 },
  });
});

test('the Codex harness has a launcher for Linux (container) and macOS (host) and refuses other platforms', () => {
  assert.deepEqual(Object.keys(codexCli._internal.LAUNCHERS).sort(), ['darwin', 'linux']);
});

// A stand-in launcher: `codex exec` is a Node script that starts a thread,
// replies, and exits 0 on SIGTERM as well as at the end of its input. Its
// CODEX_HOME is the run's `<dir>/codex-home`, as both real launchers give it.
function fakeLauncher(script, seen) {
  return async ({ dir }) => {
    fs.mkdirSync(path.join(dir, 'workspace'), { recursive: true });
    return {
      command: process.execPath, args: [script], cwd: path.join(dir, 'workspace'), env: { ...process.env, CODEX_HOME: path.join(dir, 'codex-home') },
      onStop: () => { seen.stops++; }, release: async () => { seen.released++; },
    };
  };
}

test('a Codex thread is recorded when it starts, a stopped turn\'s included, and closing after exit stops nothing', async () => {
  const script = path.join(dataDir, 'codex.js');
  fs.writeFileSync(script, `
    process.on('SIGTERM', () => process.exit(0));
    console.log(JSON.stringify({ type: 'thread.started', thread_id: 'th-1' }));
    process.stdin.resume();
    process.stdin.on('end', () => {
      console.log(JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: 'Done' } }));
      process.exit(0);
    });`);
  const { LAUNCHERS } = codexCli._internal;
  const original = LAUNCHERS[process.platform];
  const seen = { stops: 0, released: 0 };
  LAUNCHERS[process.platform] = fakeLauncher(script, seen);
  const conversation = { messages: [{ id: 'u1', role: 'user', content: 'Build a hut.' }] };
  const options = { conversation, conversationId: 'conv-codex', messages: [{ role: 'user', content: 'Build a hut.' }], tools: [], model: 'gpt-6-astra' };
  try {
    // A normal turn records its thread; closing it afterwards stops nothing.
    const done = await codexCli.completion({}, options);
    assert.equal(done.content, 'Done');
    assert.equal(harness.readSession('conv-codex', 'codex-cli').threadId, 'th-1');
    codexCli.closeSession('conv-codex');
    assert.equal(seen.stops, 0);

    // Stopped: the process exits 0 on SIGTERM. The thread was recorded when
    // Codex named it, so the next turn resumes it.
    const controller = new AbortController();
    fs.writeFileSync(script, `
      process.on('SIGTERM', () => process.exit(0));
      console.log(JSON.stringify({ type: 'thread.started', thread_id: 'th-2' }));
      setInterval(() => {}, 1000);`);
    const stopped = codexCli.completion({}, { ...options, signal: controller.signal });
    await new Promise((resolve) => setTimeout(resolve, 200));
    controller.abort(new Error('stopped'));
    await assert.rejects(stopped, /stopped/);
    while (seen.released < 2) await new Promise((resolve) => setTimeout(resolve, 20));
    assert.equal(seen.stops, 1);
    const empty = require('node:crypto').createHash('sha256').update('').digest('hex');
    assert.deepEqual(harness.readSession('conv-codex', 'codex-cli'), { threadId: 'th-2', home: 'chat', instructions: empty, resolution: empty, lastSeen: 'u1' });
  } finally {
    LAUNCHERS[process.platform] = original;
  }
});

test('a Codex run whose release fails after it exits is logged, not left as an unhandled rejection', async (t) => {
  const script = path.join(dataDir, 'codex-release.js');
  fs.writeFileSync(script, `
    console.log(JSON.stringify({ type: 'thread.started', thread_id: 'th-r' }));
    process.stdin.resume();
    process.stdin.on('end', () => {
      console.log(JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: 'Done' } }));
      process.exit(0);
    });`);
  const { LAUNCHERS } = codexCli._internal;
  const original = LAUNCHERS[process.platform];
  const seen = { stops: 0, released: 0 };
  const fake = fakeLauncher(script, seen);
  LAUNCHERS[process.platform] = async (options) => ({ ...await fake(options), release: async () => { throw new Error('the proxy would not close'); } });
  const logged = t.mock.method(console, 'error', () => {});
  const unhandled = [];
  const record = (reason) => unhandled.push(reason);
  process.on('unhandledRejection', record);
  try {
    const done = await codexCli.completion({}, { conversationId: 'conv-release', messages: [{ role: 'user', content: 'Build a hut.' }], tools: [], model: 'gpt-6-astra' });
    assert.equal(done.content, 'Done');
    for (let i = 0; i < 50 && !logged.mock.calls.length && !unhandled.length; i++) await new Promise((resolve) => setTimeout(resolve, 10));
  } finally {
    process.off('unhandledRejection', record);
    LAUNCHERS[process.platform] = original;
  }
  assert.deepEqual(unhandled, []);
  assert.ok(logged.mock.calls.some((entry) => entry.arguments[0] === '[codex-cli]' && /the proxy would not close/.test(entry.arguments[1]?.message)));
});

const tokenCount = (input, cached) => ({ type: 'event_msg', timestamp: '2026-11-11T12:00:01.000Z', payload: { type: 'token_count', rate_limits: {}, info: input === null ? null : {
  total_token_usage: { input_tokens: 999999, cached_input_tokens: 0, output_tokens: 1, reasoning_output_tokens: 0, total_tokens: 1000000 },
  last_token_usage: { input_tokens: input, cached_input_tokens: cached, cache_write_input_tokens: 0, output_tokens: 283, reasoning_output_tokens: 65, total_tokens: input + 283 },
  last_usage_includes_reasoning: true, model_context_window: 258400 } } });

function writeRollout(home, day, threadId, events) {
  const folder = path.join(home, 'sessions', ...day.split('-'));
  fs.mkdirSync(folder, { recursive: true });
  fs.writeFileSync(path.join(folder, `rollout-${day}T12-00-00-${threadId}.jsonl`), events.map((event) => JSON.stringify(event) + '\n').join(''));
}

test('the context reading is the last request\'s input from the thread\'s own rollout file', () => {
  const home = fs.mkdtempSync(path.join(dataDir, 'home-'));
  writeRollout(home, '2026-11-10', 'th-a', [tokenCount(5000, 0)]);
  writeRollout(home, '2026-11-11', 'th-b', [{ type: 'session_meta', payload: { id: 'th-b' } }, tokenCount(40000, 0), tokenCount(98982, 81408), tokenCount(null)]);
  assert.equal(codexCli.contextTokensOf(home, 'th-a'), 5000);
  assert.equal(codexCli.contextTokensOf(home, 'th-b'), 98982);
  assert.throws(() => codexCli.contextTokensOf(home, 'th-none'), new RegExp(`No Codex rollout file for thread th-none under ${path.join(home, 'sessions')}`));
  writeRollout(home, '2026-11-11', 'th-c', [{ type: 'session_meta', payload: { id: 'th-c' } }, tokenCount(null)]);
  assert.throws(() => codexCli.contextTokensOf(home, 'th-c'), /No token_count event with usage in .*rollout-2026-11-11T12-00-00-th-c\.jsonl/);
});

// A fake `codex exec` whose completed turn writes its rollout file like Codex:
// session_meta on a new thread, then token_count events, and a turn.completed
// whose usage is the sum over the turn's requests.
function rolloutScript({ threadId, append, requests, sum }) {
  return `
    const fs = require('fs');
    const path = require('path');
    const folder = path.join(process.env.CODEX_HOME, 'sessions', '2026', '11', '11');
    const file = path.join(folder, 'rollout-2026-11-11T12-00-00-${threadId}.jsonl');
    const events = ${JSON.stringify([...(append ? [] : [{ type: 'session_meta', payload: { id: threadId } }, tokenCount(null)]), ...requests.map(([input, cached]) => tokenCount(input, cached)), tokenCount(null)])};
    console.log(JSON.stringify({ type: 'thread.started', thread_id: '${threadId}' }));
    process.stdin.resume();
    process.stdin.on('end', () => {
      fs.mkdirSync(folder, { recursive: true });
      fs.appendFileSync(file, events.map((event) => JSON.stringify(event) + '\\n').join(''));
      console.log(JSON.stringify({ type: 'item.completed', item: { type: 'agent_message', text: 'Done' } }));
      console.log(JSON.stringify({ type: 'turn.completed', usage: { input_tokens: ${sum}, cached_input_tokens: 81408, output_tokens: 566, reasoning_output_tokens: 130 } }));
      process.exit(0);
    });`;
}

test('a completed Codex turn reports its last request\'s input as the context reading, and a resumed turn the new last one', async () => {
  const script = path.join(dataDir, 'codex-context.js');
  fs.writeFileSync(script, rolloutScript({ threadId: 'th-ctx', requests: [[40000, 0], [98982, 81408]], sum: 138982 }));
  const { LAUNCHERS } = codexCli._internal;
  const original = LAUNCHERS[process.platform];
  const seen = { stops: 0, released: 0 };
  LAUNCHERS[process.platform] = fakeLauncher(script, seen);
  const first = { id: 'u1', role: 'user', content: 'Build a hut.' };
  try {
    const done = await codexCli.completion({}, { conversation: { messages: [first] }, conversationId: 'conv-context', messages: [first], tools: [], model: 'gpt-6-astra' });
    assert.equal(done.content, 'Done');
    assert.equal(done.contextTokens, 98982);
    assert.equal(done.usage.input_tokens, 138982);

    // The resumed thread appends to the same rollout file.
    fs.writeFileSync(script, rolloutScript({ threadId: 'th-ctx', append: true, requests: [[120500, 99000]], sum: 120500 }));
    const reply = { id: 'a1', role: 'assistant', content: 'Done' };
    const second = { id: 'u2', role: 'user', content: 'Add a door.' };
    const messages = [first, reply, second];
    const resumed = await codexCli.completion({}, { conversation: { messages }, conversationId: 'conv-context', messages, tools: [], model: 'gpt-6-astra' });
    assert.equal(resumed.contextTokens, 120500);
    const files = fs.readdirSync(path.join(harness.conversationDir('conv-context'), 'codex-home', 'sessions'), { recursive: true }).filter((name) => name.endsWith('.jsonl'));
    assert.equal(files.length, 1);
  } finally {
    LAUNCHERS[process.platform] = original;
  }
});

test('a failed Codex turn reports Codex\'s failure and reads no rollout file', async () => {
  const script = path.join(dataDir, 'codex-failed.js');
  fs.writeFileSync(script, `
    console.log(JSON.stringify({ type: 'thread.started', thread_id: 'th-fail' }));
    process.stdin.resume();
    process.stdin.on('end', () => {
      console.log(JSON.stringify({ type: 'turn.failed', error: { message: 'usage limit reached' } }));
      process.exit(0);
    });`);
  const { LAUNCHERS } = codexCli._internal;
  const original = LAUNCHERS[process.platform];
  LAUNCHERS[process.platform] = fakeLauncher(script, { stops: 0, released: 0 });
  try {
    await assert.rejects(codexCli.completion({}, { conversationId: 'conv-failed', messages: [{ role: 'user', content: 'Build a hut.' }], tools: [], model: 'gpt-6-astra' }),
      /^Error: Codex turn failed: usage limit reached$/);
  } finally {
    LAUNCHERS[process.platform] = original;
  }
});

test('concurrent Codex catalogue lookups share one `codex debug models` run', async () => {
  const counter = path.join(dataDir, 'debug-models.txt');
  const binary = path.join(dataDir, 'codex');
  fs.writeFileSync(binary, `#!/usr/bin/env node
require('fs').appendFileSync(${JSON.stringify(counter)}, 'run\\n');
setTimeout(() => console.log(JSON.stringify({ models: [{ slug: 'gpt-6-astra', display_name: 'Astra', visibility: 'list', priority: 1 }] })), 100);
`, { mode: 0o755 });
  codexCli.configure({ binary });
  codexCli.forgetModels();
  try {
    const [a, b] = await Promise.all([codexCli.listModels(), codexCli.listModels()]);
    assert.equal(a, b);
    assert.equal(a[0].id, 'gpt-6-astra');
    assert.equal(fs.readFileSync(counter, 'utf8').split('\n').filter(Boolean).length, 1);
  } finally {
    codexCli.configure({});
    codexCli.forgetModels();
  }
});
