#!/usr/bin/env node
'use strict';
// Writes harness.jsonl beside this script: inputs of the harnesses' pure
// functions with what today's server/ answers for them, one JSON object per
// line ({ fn, input, output } or { fn, input, error }). The Rust test
// `golden_matches_node` (src/harness/tests.rs, always run) gives each input
// to the Rust function and requires the same output. Never edit the file by
// hand: run `node generate.js [<checkout>]` again (it loads server/ with a
// scratch MCBENCH_DATA_DIR, which it removes).

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const root = path.resolve(process.argv[2] || path.join(__dirname, '../../../../..'));
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-golden-'));
process.env.MCBENCH_DATA_DIR = scratch;
const harness = require(path.join(root, 'server/harness'));
const prompt = require(path.join(root, 'server/prompt'));
const claudeCode = require(path.join(root, 'server/claude-code'));
const codexCli = require(path.join(root, 'server/codex-cli'));

const lines = [];
function record(fn, input, run) {
  try {
    lines.push({ fn, input, output: run() });
  } catch (error) {
    lines.push({ fn, input, error: error.message });
  }
}

const PNG = 'data:image/png;base64,iVBORw0KGgo=';
const JPG = 'data:image/jpeg;base64,/9j/4AAQ';
const SYSTEM = { role: 'system', content: 'You build in Minecraft.' };
const transcripts = [
  [SYSTEM, { role: 'user', content: 'Build a hut.' }],
  [SYSTEM, { role: 'system', content: 'Second system part.' }, { role: 'user', content: 'Build a hut.' }],
  [SYSTEM,
    { role: 'user', content: 'Build a hut.' },
    { role: 'assistant', content: 'Starting.', tool_calls: [{ id: 'call-1', type: 'function', function: { name: 'build', arguments: '{"summary":"hut"}' } }] },
    { role: 'tool', tool_call_id: 'call-1', content: '{"ok":true}' },
    { role: 'user', content: [{ type: 'text', text: '[see_build] Screenshots of the current build:' }, { type: 'image_url', image_url: { url: PNG } }] },
    { role: 'assistant', content: 'Done.' },
    { role: 'user', content: [{ type: 'text', text: 'Make the roof dark oak.' }, { type: 'image_url', image_url: { url: JPG } }] }],
  [SYSTEM, { role: 'user', content: 'Go.' }, { role: 'assistant', content: '', tool_calls: [{ id: 'c', type: 'function', function: { name: 'bash', arguments: '' } }] },
    { role: 'tool', tool_call_id: 'c', content: 'out' }, { role: 'user', seeBuild: true, content: [{ type: 'image_url', image_url: { url: PNG } }] }],
  [SYSTEM, { role: 'user', content: 'A' }, { role: 'user', content: 'B' }],
  [{ role: 'user', content: 'No system.' }],
  [SYSTEM, { role: 'assistant', content: null }, { role: 'tool', tool_call_id: 'unknown', content: 'orphan' }, { role: 'user', content: '' }],
  [SYSTEM, { role: 'user', content: [{ type: 'text', text: '' }, { type: 'text', text: 'Only' }] }],
  [SYSTEM, { role: 'user', content: [{ type: 'audio', data: 'x' }] }],
  [SYSTEM, { role: 'user', content: [{ type: 'image_url', image_url: { url: 'https://example.com/a.png' } }] }],
  [SYSTEM, { role: 'user', content: [{ type: 'image_url', image_url: { url: 'data:image/png;base64,' } }] }],
  [SYSTEM, { role: 'user', content: 'Ünïcödé — "quotes" and \\ back\nslash 🧱' }],
];
const notes = [[], ['<note>\nA\n</note>'], ['<a>', '<b>']];
for (const messages of transcripts) {
  for (const history of [true, false]) {
    for (const note of notes) {
      record('harnessPrompt', { messages, history, notes: note }, () => harness.harnessPrompt(messages, { history, notes: note }));
    }
  }
  record('systemText', { messages }, () => harness.systemText(messages));
  record('sessionTerms', { messages }, () => harness.sessionTerms(messages));
}

const at = (resolution, networkAccess, imageInput) => [{ role: 'system', content: prompt.systemPrompt({ resolution, networkAccess, imageInput }) }, { role: 'user', content: 'Go on.' }];
const forms = [];
for (const resolution of [1, 2, 5, 10]) forms.push(at(resolution, true, true));
forms.push(at(1, false, false), at(5, false, false));
for (const messages of forms) {
  record('sessionTerms', { messages }, () => harness.sessionTerms(messages));
  for (const other of [forms[0], forms[2]]) {
    const recordTerms = harness.sessionTerms(other);
    record('resolutionNote', { record: recordTerms, messages }, () => harness.resolutionNote(recordTerms, messages));
  }
}
record('resolutionNote', { record: {}, messages: forms[0] }, () => harness.resolutionNote({}, forms[0]));
for (const text of ['plain', 'A\n\n## Resolution\nB', 'A\n\n## Resolution\n', 'A\n## Resolution\nB', '\n\n## Resolution\nX\n\n## Resolution\nY']) {
  record('splitResolution', { text }, () => prompt.splitResolution(text));
  record('resolutionUpdate', { section: text }, () => prompt.resolutionUpdate(text));
}
record('resolutionUpdate', { section: '' }, () => prompt.resolutionUpdate(''));

const m = (id, role, extra = {}) => ({ id, role, ...extra });
const stored = [
  [m('u0', 'user'), m('a0', 'assistant', { stopped: true, reply: 'Old turn' }), m('u1', 'user'), m('a1', 'assistant', { stopped: true, reply: 'First the floor, then' }), m('u2', 'user')],
  [m('u1', 'user'), m('a1', 'assistant', { stopped: true }), m('u2', 'user')],
  [m('u1', 'user'), m('a1', 'assistant', { stopped: true, reply: '' }), m('u2', 'user')],
  [m('u1', 'user'), m('a1', 'assistant'), m('t1', 'tool'), m('s1', 'user', { seeBuild: true }), m('a2', 'assistant'), m('u2', 'user')],
  [m('u1', 'user'), m('a1', 'assistant'), m('e1', 'error'), m('u2', 'user')],
  [m('u0', 'user'), m('u2', 'user')],
  [m('u1', 'user'), m('a1', 'assistant'), m('u9', 'user'), m('a9', 'assistant'), m('u2', 'user')],
  [m('u1', 'user'), m('t1', 'tool'), m('s1', 'user', { seeBuild: true })],
  [m('u1', 'user')],
  [m('x', 'assistant', { stopped: true, reply: 'Before' }), m('u1', 'user'), m('u2', 'user')],
];
const records = [{ sessionId: 's', lastSeen: 'u1' }, { sessionId: 's', lastSeen: 'gone' }, { sessionId: 's' }, { lastSeen: 'u2' }];
for (const messages of stored) {
  for (const rec of records) {
    record('stoppedReplyNote', { record: rec, transcript: messages }, () => harness.stoppedReplyNote(rec, messages));
    record('sessionContinues', { record: rec, transcript: messages }, () => harness.sessionContinues(rec, messages));
  }
}

const pending = [
  [{ role: 'assistant', content: '' }, { role: 'tool', tool_call_id: 'c1', content: '{"ok":true}' }],
  [{ role: 'tool', tool_call_id: 'c1', content: 'r' }, { role: 'user', seeBuild: true, content: [{ type: 'text', text: 'shots' }, { type: 'image_url', image_url: { url: PNG } }] }],
  [{ role: 'tool', tool_call_id: 'c1', content: [{ type: 'text', text: 'a' }, { type: 'image_url', image_url: { url: JPG } }] }, { role: 'user', content: 'more' }],
  [{ role: 'tool', tool_call_id: 'c1', content: 'r' }, { role: 'assistant', content: 'x' }],
  [{ role: 'tool', tool_call_id: 'c2', content: 'r' }],
  [{ role: 'tool', tool_call_id: 'c1', content: [{ type: 'file', name: 'x' }] }],
  [{ role: 'tool', tool_call_id: 'c1', content: [{ type: 'text' }] }],
];
for (const messages of pending) record('pendingResult', { messages, callId: 'c1' }, () => harness.pendingResult(messages, 'c1'));

const bridge = { command: '<bridge>', args: ['tool-bridge', '/run/x/mcp.sock'] };
const claudeArgs = (input) => {
  // Today's command line with the bridge as the native app names it: the
  // mcp-config is rebuilt around `bridge`, the only part that differs.
  const args = claudeCode.claudeArgs({ ...input, socket: '/run/x/mcp.sock' });
  const at = args.indexOf('--mcp-config');
  args[at + 1] = JSON.stringify({ mcpServers: { mcbench: { type: 'stdio', ...bridge } } });
  return args;
};
const allowed = ['mcp__mcbench__build', 'mcp__mcbench__see_build'];
for (const session of [null, { id: 'abc', resume: false }, { id: 'abc', resume: true }]) {
  for (const oneShot of [false, true]) {
    for (const effort of [undefined, '', 'high']) {
      for (const builtins of [[], ['TaskCreate', 'TaskUpdate', 'TaskList', 'TaskGet']]) {
        const input = { model: 'claude-opus-5-5', effort, systemPrompt: 'S "q"\n2', allowedTools: allowed, session, oneShot, builtins };
        record('claudeArgs', input, () => claudeArgs(input));
      }
    }
  }
}
const inits = [
  { tools: allowed, mcp_servers: [{ name: 'mcbench', status: 'connected' }] },
  { tools: [...allowed, 'Bash', 'Read'], mcp_servers: [{ name: 'mcbench', status: 'connected' }] },
  { tools: [allowed[0], 'TaskCreate'], mcp_servers: [{ name: 'mcbench', status: 'connected' }] },
  { tools: allowed, mcp_servers: [{ name: 'mcbench', status: 'failed' }] },
  { tools: allowed, mcp_servers: [{ name: 'other', status: 'connected' }] },
  { tools: allowed, mcp_servers: [] },
];
for (const event of inits) record('checkInit', { event, allowed, builtins: ['TaskCreate'] }, () => { claudeCode.checkInit(event, allowed, ['TaskCreate']); return null; });
for (const result of [
  { usage: { input_tokens: 1, output_tokens: 2, output_tokens_details: { thinking_tokens: 5 } } },
  { usage: { input_tokens: 1, output_tokens_details: {}, iterations: [] } },
  { usage: { output_tokens_details: { thinking_tokens: 0 }, z: 1, a: 2 } },
  { usage: {} },
  {},
  { usage: null },
]) record('claudeUsageOf', { result }, () => claudeCode.usageOf(result));
for (const usage of [{ input_tokens: 2, cache_creation_input_tokens: 117, cache_read_input_tokens: 7264 }, { input_tokens: 2 }, { input_tokens: 1.5, cache_creation_input_tokens: 0.25, cache_read_input_tokens: 0 }]) {
  record('claudeContextTokensOf', { usage }, () => claudeCode.contextTokensOf(usage));
}
for (const input of [{ text: 'hi', images: [] }, { text: 'hi "there"\n', images: [{ mimeType: 'image/png', data: 'AA==' }, { mimeType: 'image/jpeg', data: '/9j/' }] }]) {
  record('streamInput', input, () => claudeCode._internal.streamInput(input));
}
for (const input of [{ model: 'gpt-6-astra', effort: 'high', systemPrompt: 'Build "well".\nLine two\ttab  ' }, { model: 'm', systemPrompt: 's' }, { model: 'm', effort: '', systemPrompt: '' }]) {
  record('configToml', input, () => codexCli.configToml(input).replace('python3', '/run/mcbench-relay').replace('"/run/mcbench/mcp-bridge.py", "/run/mcbench/mcp.sock"', '"tool-bridge", "/run/mcbench/mcp.sock"'));
}
for (const input of [
  { model: 'gpt-6-astra', effort: 'low', systemPrompt: 'S', resume: false, threadId: null, images: ['/tmp/image-1.png'] },
  { model: 'm', systemPrompt: 'S\n"x"', resume: true, threadId: 'thread-1', images: [] },
  { model: 'm', effort: 'xhigh', systemPrompt: '', resume: false, threadId: null, images: ['/a', '/b'] },
]) {
  record('hostArgs', input, () => codexCli.hostArgs({ ...input, socket: '/run/x/mcp.sock' })
    .map((arg) => arg.replace(`"python3"`, '"<bridge>"').replace(`[${JSON.stringify(path.join(root, 'server', 'mcp-bridge.py'))}, "/run/x/mcp.sock"]`, '["tool-bridge", "/run/x/mcp.sock"]')));
}
for (const item of [{ type: 'command_execution' }, { type: 'file_change' }, { type: 'web_search' }, { type: 'some_future_tool' }, { type: 'mcp_tool_call', server: 'other', tool: 'x' },
  { type: 'mcp_tool_call', server: 'mcbench', tool: 'build' }, { type: 'mcp_tool_call' }, { type: 'agent_message', text: 'hi' }, { type: 'reasoning' }, { type: 'todo_list' }, { type: 'error', server: 'x' }, {}, { type: 'web_search', server: '' }]) {
  record('checkItem', { item }, () => { codexCli.checkItem(item); return null; });
}
for (const usage of [{ input_tokens: 10, cached_input_tokens: 4, output_tokens: 6, reasoning_output_tokens: 2 }, { input_tokens: 1 }, {}]) {
  record('codexUsageOf', { usage }, () => codexCli.usageOf(usage));
}
const level = (effort) => ({ effort, description: '' });
for (const cache of [
  { models: [
    { slug: 'gpt-reserve', display_name: 'Reserve', visibility: 'hide', supported_reasoning_levels: [level('low')], default_reasoning_level: 'low', input_modalities: ['text', 'image'], priority: 1 },
    { slug: 'gpt-5.6-sol', display_name: 'GPT-5.6-Sol', visibility: 'list', additional_speed_tiers: ['fast'], service_tiers: [{ id: 'priority', name: 'Fast', description: '2x speed' }], supported_reasoning_levels: ['low', 'medium', 'high', 'xhigh', 'max', 'ultra'].map(level), default_reasoning_level: 'low', input_modalities: ['text', 'image'], priority: 5, context_window: 400000 },
    { slug: 'gpt-6-astra', display_name: 'GPT-6-Astra', visibility: 'list', supported_reasoning_levels: ['low', 'medium', 'high'].map(level), default_reasoning_level: 'medium', input_modalities: ['text'], priority: 2 },
    { slug: 'fast-no-note', display_name: 'F', visibility: 'list', additional_speed_tiers: ['fast'], priority: 3 },
  ] },
  { models: [{ slug: 'fake-model', display_name: 'Fake Model', visibility: 'list', priority: 1 }] },
]) record('modelsFromCache', { cache }, () => codexCli.modelsFromCache(cache));

fs.rmSync(scratch, { recursive: true, force: true });
fs.writeFileSync(path.join(__dirname, 'harness.jsonl'), lines.map((line) => JSON.stringify(line)).join('\n') + '\n');
console.log(`golden: ${lines.length} cases`);
