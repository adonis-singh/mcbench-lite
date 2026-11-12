'use strict';

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-claude-code-test-'));
process.env.MCBENCH_DATA_DIR = dataDir;
// A stand-in `claude` on PATH: it records its arguments and stdin, announces
// mcbench's tools, calls `build` over the MCP socket and streams the reply.
const binDir = path.join(dataDir, 'bin');
fs.mkdirSync(binDir);
const ARGS_FILE = path.join(dataDir, 'args.json');
const INPUT_FILE = path.join(dataDir, 'input.ndjson');
const SPAWNS_FILE = path.join(dataDir, 'spawns.txt');
const CWD_FILE = path.join(dataDir, 'cwd.txt');
// The CLI only gets an allowlisted environment, so the stand-in finds its
// files by fixed paths; a flag file makes it announce an extra built-in tool.
const EXTRA_TOOL_FLAG = path.join(dataDir, 'extra-tool');
// Another makes its result carry no usage.
const NO_USAGE_FLAG = path.join(dataDir, 'no-usage');
process.env.PATH = `${binDir}:${process.env.PATH}`;
fs.writeFileSync(path.join(binDir, 'claude'), `#!/usr/bin/env node
const net = require('net'); const fs = require('fs'); const readline = require('readline');
const args = process.argv.slice(2);
fs.writeFileSync(${JSON.stringify(ARGS_FILE)}, JSON.stringify(args));
fs.appendFileSync(${JSON.stringify(SPAWNS_FILE)}, 'spawn\\n');
fs.writeFileSync(${JSON.stringify(CWD_FILE)}, process.cwd());
const socketPath = JSON.parse(args[args.indexOf('--mcp-config') + 1]).mcpServers.mcbench.args[1];
const allowed = args[args.indexOf('--allowedTools') + 1].split(',');
const emit = e => console.log(JSON.stringify(e));
const socket = net.connect(socketPath);
const replies = new Map(); let id = 0;
readline.createInterface({ input: socket }).on('line', line => { const m = JSON.parse(line); replies.get(m.id)(m); });
const call = (method, params) => new Promise(resolve => { replies.set(++id, resolve); socket.write(JSON.stringify({ jsonrpc: '2.0', id, method, params }) + '\\n'); });
const ready = call('initialize', { protocolVersion: '2025-06-18' }).then(() => call('tools/list', {}));
// Like Claude Code with stream-json input: one turn per input line, and the
// process stays up until its input ends.
let turns = Promise.resolve();
readline.createInterface({ input: process.stdin }).on('line', line => { turns = turns.then(async () => {
  fs.writeFileSync(${JSON.stringify(INPUT_FILE)}, line);
  emit({ type: 'system', subtype: 'init', tools: fs.existsSync(${JSON.stringify(EXTRA_TOOL_FLAG)}) ? [...allowed, 'Bash'] : allowed, mcp_servers: [{ name: 'mcbench', status: 'connected' }], session_id: 'sess-1' });
  await ready;
  emit({ type: 'stream_event', event: { type: 'message_start', message: {} } });
  emit({ type: 'stream_event', event: { type: 'content_block_delta', index: 0, delta: { type: 'thinking_delta', thinking: 'plan' } } });
  // As Claude Code does: the arguments stream, the tool_use block lands on
  // stdout, then the MCP call names it.
  const toolUseId = 'toolu_' + id;
  const input = { summary: 's', program: 'p' };
  emit({ type: 'stream_event', event: { type: 'content_block_start', index: 1, content_block: { type: 'tool_use', id: toolUseId, name: 'mcp__mcbench__build', input: {} } } });
  emit({ type: 'stream_event', event: { type: 'content_block_delta', index: 1, delta: { type: 'input_json_delta', partial_json: '{"summary":"s",' } } });
  emit({ type: 'stream_event', event: { type: 'content_block_delta', index: 1, delta: { type: 'input_json_delta', partial_json: '"program":"p"}' } } });
  // Each request's assistant event carries that request's usage.
  emit({ type: 'assistant', message: { content: [{ type: 'tool_use', id: toolUseId, name: 'mcp__mcbench__build', input }], usage: { input_tokens: 4, cache_creation_input_tokens: 7264, cache_read_input_tokens: 7147, output_tokens: 60 } } });
  const result = await call('tools/call', { name: 'build', arguments: input, _meta: { 'claudecode/toolUseId': toolUseId } });
  const reply = 'Built: ' + result.result.content.map(c => c.text).join('|');
  emit({ type: 'stream_event', event: { type: 'content_block_delta', delta: { type: 'text_delta', text: reply } } });
  emit({ type: 'assistant', message: { content: [{ type: 'text', text: reply }], usage: { input_tokens: 2, cache_creation_input_tokens: 117, cache_read_input_tokens: 7264, output_tokens: 48 } } });
  // As Claude Code 2.1 reports it: the turn's usage summed over its requests, with no iterations.
  emit({ type: 'result', subtype: 'success', is_error: false, session_id: 'sess-1', result: 'Built', total_cost_usd: 0.01,
    ...(fs.existsSync(${JSON.stringify(NO_USAGE_FLAG)}) ? {} : { usage: { input_tokens: 6, cache_creation_input_tokens: 7381, cache_read_input_tokens: 14411, output_tokens: 108, output_tokens_details: { thinking_tokens: 1 }, iterations: [] } }) });
}); }).on('close', () => turns.then(() => process.exit(0)));
`, { mode: 0o755 });

const claudeCode = require('../server/claude-code');
const harness = require('../server/harness');
const { ClaudeCodeSession, streamInput, processes, CWD } = claudeCode._internal;

after(async () => {
  await harness.shutdownHarnesses();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

// The instructions 'You build.' these sessions run with, with no resolution section.
const digest = (text) => require('node:crypto').createHash('sha256').update(text).digest('hex');
const TERMS = { instructions: digest('You build.'), resolution: digest('') };
const TOOL = { type: 'function', function: { name: 'build', description: 'Build', parameters: { type: 'object' } } };
const recordedArgs = () => JSON.parse(fs.readFileSync(ARGS_FILE, 'utf8'));
const recordedInput = () => JSON.parse(fs.readFileSync(INPUT_FILE, 'utf8'));
const flag = (args, name) => args[args.indexOf(name) + 1];
const spawns = () => (fs.existsSync(SPAWNS_FILE) ? fs.readFileSync(SPAWNS_FILE, 'utf8').split('\n').filter(Boolean).length : 0);

// One whole turn on a session: its tool call answered, then its reply.
async function runTurn(options) {
  const session = new ClaudeCodeSession();
  await session.start({ tools: [TOOL], model: 'claude-opus-5-5', ...options });
  await session.next();
  session.deliver([{ type: 'text', text: 'ok' }]);
  const reply = await session.next();
  session.close();
  return reply;
}

test('Claude Code arguments remove built-in tools, pin settings off and carry the session mode', () => {
  const base = { model: 'claude-opus-5-5', effort: 'high', systemPrompt: 'S', allowedTools: ['mcp__mcbench__build'], socket: '/tmp/x.sock' };
  const fresh = claudeCode.claudeArgs({ ...base, session: { id: 'abc', resume: false } });
  assert.deepEqual(fresh.slice(0, 5), ['-p', '--output-format', 'stream-json', '--verbose', '--model']);
  assert.equal(flag(fresh, '--effort'), 'high');
  assert.equal(flag(fresh, '--thinking-display'), 'summarized');
  assert.equal(flag(fresh, '--tools'), '');
  assert.ok(fresh.includes('--strict-mcp-config') && fresh.includes('--disable-slash-commands'));
  assert.equal(flag(fresh, '--setting-sources'), '');
  assert.deepEqual(JSON.parse(flag(fresh, '--mcp-config')), { mcpServers: { mcbench: { type: 'stdio', command: 'python3', args: [path.join(__dirname, '..', 'server', 'mcp-bridge.py'), '/tmp/x.sock'] } } });
  assert.equal(flag(fresh, '--session-id'), 'abc');
  assert.equal(flag(fresh, '--input-format'), 'stream-json');
  assert.ok(fresh.includes('--include-partial-messages'));
  assert.equal(flag(claudeCode.claudeArgs({ ...base, session: { id: 'abc', resume: true } }), '--resume'), 'abc');
  const batch = claudeCode.claudeArgs({ ...base, effort: null, session: null });
  assert.ok(batch.includes('--no-session-persistence'));
  assert.ok(!batch.includes('--effort') && !batch.includes('--input-format') && !batch.includes('--session-id'));
  assert.equal(batch.at(-1), 'S');
  assert.equal(batch.at(-2), '--append-system-prompt');
  // A one-shot completion is streamed as a chat turn is, never saved, and its
  // system prompt replaces Claude Code's own.
  const oneShot = claudeCode.claudeArgs({ ...base, session: null, oneShot: true });
  assert.ok(oneShot.includes('--no-session-persistence') && oneShot.includes('--include-partial-messages'));
  assert.equal(flag(oneShot, '--input-format'), 'stream-json');
  assert.ok(!oneShot.includes('--session-id') && !oneShot.includes('--resume') && !oneShot.includes('--append-system-prompt'));
  assert.deepEqual(oneShot.slice(-2), ['--system-prompt', 'S']);
  const env = claudeCode.claudeEnv();
  assert.equal(env.ANTHROPIC_API_KEY, undefined);
  assert.equal(env.MCP_TOOL_TIMEOUT, '2147483647');
  assert.equal(env.CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING, '1');
  // No notes from earlier chats in Claude Code's memory folder.
  assert.equal(env.CLAUDE_CODE_DISABLE_AUTO_MEMORY, '1');
});

test('a completion with no conversation (a title, a prompt rewrite) runs with its system prompt in place of Claude Code\'s, and saves no session', async () => {
  const messages = [
    { role: 'system', content: 'Name this chat in 2 to 6 words.' },
    { role: 'user', content: 'Build a tiny stone bench, three blocks wide.' },
  ];
  await runTurn({ messages });
  const args = recordedArgs();
  assert.equal(flag(args, '--system-prompt'), 'Name this chat in 2 to 6 words.');
  assert.ok(!args.includes('--append-system-prompt'));
  assert.ok(args.includes('--no-session-persistence') && !args.includes('--session-id'));
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Build a tiny stone bench, three blocks wide.' }]);
});

test('the init event must list only mcbench tools served by a connected server', () => {
  const allowed = ['mcp__mcbench__build', 'mcp__mcbench__bash'];
  claudeCode.checkInit({ tools: ['mcp__mcbench__build'], mcp_servers: [{ name: 'mcbench', status: 'connected' }] }, allowed);
  assert.throws(() => claudeCode.checkInit({ tools: ['mcp__mcbench__build', 'Bash'], mcp_servers: [{ name: 'mcbench', status: 'connected' }] }, allowed), /outside mcbench: Bash/);
  assert.throws(() => claudeCode.checkInit({ tools: allowed, mcp_servers: [{ name: 'mcbench', status: 'failed' }] }, allowed), /status: failed/);
  assert.deepEqual(claudeCode.usageOf({ usage: { input_tokens: 1, output_tokens: 2, output_tokens_details: { thinking_tokens: 5 } } }), { input_tokens: 1, output_tokens: 2, output_tokens_details: { reasoning_tokens: 5 } });
  assert.deepEqual(JSON.parse(streamInput({ text: 'hi', images: [{ mimeType: 'image/png', data: 'AA==' }] })), {
    type: 'user', message: { role: 'user', content: [{ type: 'text', text: 'hi' }, { type: 'image', source: { type: 'base64', media_type: 'image/png', data: 'AA==' } }] },
  });
});

test('a first turn starts a saved session, relays its tool call through the completion loop and records the session', async () => {
  const conversation = { messages: [{ id: 'u1', role: 'user', content: 'Build a hut.' }] };
  const messages = [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Build a hut.' }];
  const session = new ClaudeCodeSession();
  await session.start({ conversation, conversationId: 'conv-1', messages, tools: [TOOL], model: 'claude-opus-5-5', effort: 'high', chat: true });
  const deltas = [];
  const first = await session.next((delta) => deltas.push(delta));
  assert.equal(first.reasoning, 'plan');
  assert.equal(first.finish_reason, 'tool_calls');
  assert.equal(first.tool_calls[0].function.name, 'build');
  assert.match(first.tool_calls[0].id, /^toolu_\d+$/);
  assert.deepEqual(JSON.parse(first.tool_calls[0].function.arguments), { summary: 's', program: 'p' });
  // The arguments stream under the call's id as they are written, and are
  // not sent again whole when the call runs.
  assert.deepEqual(deltas.map((d) => Object.keys(d)[0]), ['reasoning', 'toolCall', 'toolCall', 'toolCall']);
  const streamed = deltas.slice(1).map((d) => d.toolCall);
  assert.ok(streamed.every((call) => call.id === first.tool_calls[0].id && call.name === 'build' && call.index === 1));
  assert.equal(streamed.map((call) => call.args).join(''), '{"summary":"s","program":"p"}');
  session.deliver([{ type: 'text', text: '{"ok":true}' }]);
  const final = await session.next();
  assert.equal(final.content, 'Built: {"ok":true}');
  assert.equal(final.finish_reason, 'stop');
  assert.deepEqual(final.usage, { input_tokens: 6, cache_creation_input_tokens: 7381, cache_read_input_tokens: 14411, output_tokens: 108, iterations: [], output_tokens_details: { reasoning_tokens: 1 } });
  // The context reading is the latest request's input, not the turn's sum (21,798).
  assert.equal(first.contextTokens, 14415);
  assert.equal(final.contextTokens, 7383);
  const args = recordedArgs();
  assert.match(flag(args, '--session-id'), /^[0-9a-f-]{36}$/);
  assert.equal(flag(args, '--model'), 'claude-opus-5-5');
  assert.equal(flag(args, '--effort'), 'high');
  assert.equal(flag(args, '--append-system-prompt'), 'You build.');
  // Chats keep the task tools so Claude Code's task list reaches the UI.
  assert.equal(flag(args, '--allowedTools'), 'mcp__mcbench__build,TaskCreate,TaskUpdate,TaskList,TaskGet');
  assert.equal(flag(args, '--tools'), 'TaskCreate,TaskUpdate,TaskList,TaskGet');
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Build a hut.' }]);
  assert.deepEqual(harness.readSession('conv-1', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u1', tasks: [] });
});

test('a follow-up resumes the recorded session with only the new request', async () => {
  harness.writeSession('conv-2', 'claude-code', { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u1', tasks: [] });
  const conversation = { messages: [{ id: 'u1', role: 'user', content: 'Build a hut.' }, { id: 'a1', role: 'assistant', content: 'Built' }, { id: 'u2', role: 'user', content: 'Make it taller.' }] };
  const messages = [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Build a hut.' }, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Make it taller.' }];
  const session = new ClaudeCodeSession();
  await session.start({ conversation, conversationId: 'conv-2', messages, tools: [TOOL], model: 'claude-opus-5-5' });
  await session.next();
  session.deliver([{ type: 'text', text: 'ok' }]);
  await session.next();
  const args = recordedArgs();
  assert.equal(flag(args, '--resume'), 'sess-1');
  assert.ok(!args.includes('--session-id') && !args.includes('--effort'));
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Make it taller.' }]);
  assert.deepEqual(harness.readSession('conv-2', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u2', tasks: [] });
});

test('a follow-up turn is written to the running Claude Code process instead of starting another', async () => {
  const first = { messages: [{ id: 'u1', role: 'user', content: 'Build a hut.' }] };
  const before = spawns();
  await runTurn({ conversation: first, conversationId: 'conv-warm', messages: [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Build a hut.' }] });
  const second = { messages: [...first.messages, { id: 'a1', role: 'assistant', content: 'Built' }, { id: 'u2', role: 'user', content: 'Make it taller.' }] };
  const reply = await runTurn({ conversation: second, conversationId: 'conv-warm', messages: [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Build a hut.' }, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Make it taller.' }] });
  assert.equal(reply.content, 'Built: ok');
  assert.equal(spawns() - before, 1);
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Make it taller.' }]);
  assert.deepEqual(harness.readSession('conv-warm', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u2', tasks: [] });

  // Other settings (here the model) need a process started with them.
  const third = { messages: [...second.messages, { id: 'a2', role: 'assistant', content: 'Taller' }, { id: 'u3', role: 'user', content: 'Add a door.' }] };
  await runTurn({ conversation: third, conversationId: 'conv-warm', model: 'claude-sonnet-5-5', messages: [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Add a door.' }] });
  assert.equal(spawns() - before, 2);
  assert.equal(flag(recordedArgs(), '--resume'), 'sess-1');
});

test('after a revert the session starts over from the rendered transcript', async () => {
  harness.writeSession('conv-3', 'claude-code', { sessionId: 'sess-old', cwd: CWD, ...TERMS, lastSeen: 'gone', tasks: [] });
  const conversation = { messages: [{ id: 'u1', role: 'user', content: 'Build a hut.' }, { id: 'a1', role: 'assistant', content: 'Built' }, { id: 'u9', role: 'user', content: 'Add a door.' }] };
  const messages = [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Build a hut.' }, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Add a door.' }];
  const session = new ClaudeCodeSession();
  await session.start({ conversation, conversationId: 'conv-3', messages, tools: [TOOL], model: 'claude-opus-5-5' });
  await session.next();
  session.deliver([{ type: 'text', text: 'ok' }]);
  await session.next();
  const args = recordedArgs();
  assert.ok(args.includes('--session-id') && !args.includes('--resume'));
  assert.notEqual(flag(args, '--session-id'), 'sess-old');
  assert.match(recordedInput().message.content[0].text, /^<conversation>\nUser:\nBuild a hut\.\n\nAssistant:\nBuilt\n<\/conversation>\n\nAdd a door\.$/);
});

test('a built-in tool in the init event fails the turn and stops Claude Code', async () => {
  fs.writeFileSync(EXTRA_TOOL_FLAG, '');
  try {
    const session = new ClaudeCodeSession();
    await session.start({ messages: [{ role: 'user', content: 'go' }], tools: [TOOL], model: 'claude-opus-5-5' });
    await assert.rejects(session.next(), /exposed tools outside mcbench: Bash/);
    assert.equal(session.state, 'closed');
  } finally {
    fs.rmSync(EXTRA_TOOL_FLAG);
  }
});

test('Claude Code stream events become status lines, notices, a task list and usage-limit state', () => {
  const seen = { status: [], notices: [], plan: null, rateLimit: null, contextTokens: null };
  const session = {
    status: (text) => seen.status.push(text),
    notice: (level, text) => seen.notices.push([level, text]),
    setPlan: (items) => { seen.plan = items; },
    setRateLimit: (info) => { seen.rateLimit = info; },
    setContextTokens: (tokens) => { seen.contextTokens = tokens; },
  };
  const { relayEvent, contextTokensOf } = claudeCode;
  relayEvent(session, { type: 'system', subtype: 'status', status: 'compacting' });
  relayEvent(session, { type: 'system', subtype: 'api_retry', attempt: 2, max_retries: 10, error_status: 529 });
  relayEvent(session, { type: 'system', subtype: 'status', status: 'requesting' });
  relayEvent(session, { type: 'system', subtype: 'compact_boundary' });
  relayEvent(session, { type: 'system', subtype: 'model_fallback', level: 'warning', content: 'Switched to Claude Sonnet 5.5' });
  relayEvent(session, { type: 'rate_limit_event', rate_limit_info: { status: 'allowed_warning', rateLimitType: 'five_hour', resetsAt: 1791118200 } });
  // Tasks arrive as TaskCreate results and confirmed TaskUpdate calls, as Claude Code 2.1 emits them.
  const created = (id, subject, callId) => relayEvent(session, { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: callId }] }, tool_use_result: { task: { id, subject } } });
  created('1', 'Lay the floor', 'c1'); created('2', 'Raise the walls', 'c2'); created('3', 'Roof', 'c3');
  const update = (callId, input) => {
    relayEvent(session, { type: 'assistant', message: { content: [{ type: 'tool_use', id: callId, name: 'TaskUpdate', input }], usage: { input_tokens: 1, cache_creation_input_tokens: 0, cache_read_input_tokens: 0, output_tokens: 1 } } });
    relayEvent(session, { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: callId }] }, tool_use_result: { success: true, taskId: input.taskId } });
  };
  update('u1', { taskId: '1', status: 'completed' });
  update('u2', { taskId: '2', status: 'in_progress', activeForm: 'Raising the walls' });
  assert.deepEqual(seen.status, ['Compacting context', 'Retrying after HTTP 529 (attempt 2 of 10)', null]);
  assert.deepEqual(seen.notices, [['info', 'Claude Code compacted the conversation to fit its context window'], ['warning', 'Switched to Claude Sonnet 5.5']]);
  assert.deepEqual(seen.plan, [{ text: 'Lay the floor', status: 'done' }, { text: 'Raise the walls', status: 'active' }, { text: 'Roof', status: 'pending' }]);
  assert.equal(seen.rateLimit.status, 'allowed_warning');
  // An update for a task this session never saw leaves the list as it is.
  update('u3', { taskId: '9', status: 'completed' });
  assert.equal(seen.plan.length, 3);
  // Context use is one request's fresh, cache-written and cache-read input.
  assert.equal(contextTokensOf({ input_tokens: 2, cache_creation_input_tokens: 117, cache_read_input_tokens: 7264 }), 7383);
  // Each request's assistant event overwrites the reading: a turn ends on its last request's, not the sum (21,798).
  for (const [input, written, read] of [[3, 120, 7026], [2, 117, 7147], [2, 117, 7264]]) {
    relayEvent(session, { type: 'assistant', message: { content: [{ type: 'text', text: 'step' }], usage: { input_tokens: input, cache_creation_input_tokens: written, cache_read_input_tokens: read, output_tokens: 10 } } });
  }
  assert.equal(seen.contextTokens, 7383);
});

const hut = { messages: [{ id: 'u1', role: 'user', content: 'Build a hut.' }] };
const hutMessages = [{ role: 'system', content: 'You build.' }, { role: 'user', content: 'Build a hut.' }];

test('a turn stopped while its MCP server starts never spawns Claude Code and leaves no files', async () => {
  const before = spawns();
  const controller = new AbortController();
  const session = new ClaudeCodeSession(controller.signal);
  const starting = session.start({ conversation: hut, conversationId: 'conv-stop', messages: hutMessages, tools: [TOOL], model: 'claude-opus-5-5' });
  controller.abort(new Error('stopped'));
  await assert.rejects(starting, /stopped while starting/);
  const proc = session.process;
  await proc.exited.promise;
  assert.equal(proc.state, 'exited');
  assert.equal(proc.run, undefined);
  assert.equal(fs.existsSync(proc.runtimeDir), false);
  assert.equal(processes.has('conv-stop'), false);
  await new Promise((resolve) => setTimeout(resolve, 200));
  assert.equal(spawns(), before);
  assert.equal(harness.readSession('conv-stop', 'claude-code'), null);
});

test('a result that cannot be read fails the turn, and the next turn resumes the session its init recorded', async () => {
  fs.writeFileSync(NO_USAGE_FLAG, '');
  try {
    const session = new ClaudeCodeSession();
    await session.start({ conversation: hut, conversationId: 'conv-bad-result', messages: hutMessages, tools: [TOOL], model: 'claude-opus-5-5' });
    await session.next();
    session.deliver([{ type: 'text', text: 'ok' }]);
    await assert.rejects(session.next(), TypeError);
    assert.equal(session.state, 'closed');
    await session.process.exited.promise;
  } finally {
    fs.rmSync(NO_USAGE_FLAG);
  }
  const failed = { messages: [...hut.messages, { id: 'a1', role: 'assistant', content: '', tool_calls: [{ id: 'toolu_1' }] }, { id: 't1', role: 'tool', content: 'ok' }, { id: 'e1', role: 'error', content: 'Cannot read properties of undefined' }, { id: 'u2', role: 'user', content: 'Try again.' }] };
  await runTurn({ conversation: failed, conversationId: 'conv-bad-result', messages: [...hutMessages, { role: 'user', content: 'Try again.' }] });
  assert.equal(flag(recordedArgs(), '--resume'), 'sess-1');
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Try again.' }]);
});

test('a turn stopped mid-turn leaves its session recorded from init, and the next turn resumes it with only the new request', async () => {
  const controller = new AbortController();
  const session = new ClaudeCodeSession(controller.signal);
  await session.start({ conversation: hut, conversationId: 'conv-stopped', messages: hutMessages, tools: [TOOL], model: 'claude-opus-5-5', chat: true });
  // The tool call is pending: Claude Code has taken the request.
  await session.next();
  assert.deepEqual(harness.readSession('conv-stopped', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u1', tasks: [] });
  const proc = session.process;
  controller.abort(new Error('stopped'));
  await proc.exited.promise;
  // As runTurn and store.endTurn leave a stopped turn: the call, its
  // screenshots, a result for it and the stop.
  const stopped = { messages: [...hut.messages,
    { id: 'a1', role: 'assistant', content: '', tool_calls: [{ id: 'toolu_1' }] },
    { id: 's1', role: 'user', seeBuild: true, content: [] },
    { id: 't1', role: 'tool', content: '{"cancelled":true}' },
    { id: 'a2', role: 'assistant', content: '(stopped by user)', stopped: true },
    { id: 'u2', role: 'user', content: 'Carry on.' }] };
  await runTurn({ conversation: stopped, conversationId: 'conv-stopped', messages: [...hutMessages, { role: 'user', content: 'Carry on.' }] });
  const args = recordedArgs();
  assert.equal(flag(args, '--resume'), 'sess-1');
  assert.ok(!args.includes('--session-id'));
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Carry on.' }]);
  assert.deepEqual(harness.readSession('conv-stopped', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u2', tasks: [] });
});

test('a session recorded under a chat\'s own folder is not resumed: the chat starts a new session with its history, once', async () => {
  harness.writeSession('conv-old-folder', 'claude-code', { sessionId: 'sess-old', lastSeen: 'u1', tasks: [] });
  const second = { messages: [...hut.messages, { id: 'a1', role: 'assistant', content: 'Built' }, { id: 'u2', role: 'user', content: 'Make it taller.' }] };
  const reply = await runTurn({ conversation: second, conversationId: 'conv-old-folder', messages: [...hutMessages, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Make it taller.' }] });
  assert.equal(reply.content, 'Built: ok');
  const args = recordedArgs();
  assert.ok(args.includes('--session-id') && !args.includes('--resume'));
  assert.match(recordedInput().message.content[0].text, /^<conversation>\nUser:\nBuild a hut\./);
  assert.deepEqual(harness.readSession('conv-old-folder', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u2', tasks: [] });
});

test('a changed resolution keeps the session: the request carries the new resolution section, and other instructions start a new session', async () => {
  await runTurn({ conversation: hut, conversationId: 'conv-resolution', messages: hutMessages });
  const before = spawns();
  const large = 'You build.\n\n## Resolution\nLarger.';
  const second = { messages: [...hut.messages, { id: 'a1', role: 'assistant', content: 'Built' }, { id: 'u2', role: 'user', content: 'Make it taller.' }] };
  await runTurn({ conversation: second, conversationId: 'conv-resolution', messages: [{ role: 'system', content: large }, { role: 'user', content: 'Build a hut.' }, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Make it taller.' }] });
  // The warm process takes the turn: its session keeps its own instructions.
  assert.equal(spawns(), before);
  assert.match(recordedInput().message.content[0].text, /^<resolution-change>\nThe chat's resolution has changed\.[^\n]*\n\n## Resolution\nLarger\.\n<\/resolution-change>\n\nMake it taller\.$/);
  assert.deepEqual(harness.readSession('conv-resolution', 'claude-code'), { sessionId: 'sess-1', cwd: CWD, instructions: digest('You build.'), resolution: digest('## Resolution\nLarger.'), lastSeen: 'u2', tasks: [] });
  // Told once: the next turn at the same resolution carries only its request.
  const third = { messages: [...second.messages, { id: 'a2', role: 'assistant', content: 'Taller' }, { id: 'u3', role: 'user', content: 'Add a door.' }] };
  await runTurn({ conversation: third, conversationId: 'conv-resolution', messages: [{ role: 'system', content: large }, { role: 'user', content: 'Add a door.' }] });
  assert.deepEqual(recordedInput().message.content, [{ type: 'text', text: 'Add a door.' }]);
  // Other instructions: a new session with the history and those instructions.
  const fourth = { messages: [...third.messages, { id: 'a3', role: 'assistant', content: 'Door' }, { id: 'u4', role: 'user', content: 'Add a roof.' }] };
  await runTurn({ conversation: fourth, conversationId: 'conv-resolution', messages: [{ role: 'system', content: 'You build carefully.' }, { role: 'user', content: 'Build a hut.' }, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Add a roof.' }] });
  const args = recordedArgs();
  assert.ok(args.includes('--session-id') && !args.includes('--resume'));
  assert.equal(flag(args, '--append-system-prompt'), 'You build carefully.');
  assert.match(recordedInput().message.content[0].text, /^<conversation>\nUser:\nBuild a hut\./);
});

test('a resumed session is given the reply the user stopped while it streamed', async () => {
  harness.writeSession('conv-stopped-reply', 'claude-code', { sessionId: 'sess-1', cwd: CWD, ...TERMS, lastSeen: 'u1', tasks: [] });
  const stopped = { messages: [...hut.messages, { id: 'a1', role: 'assistant', content: '(stopped by user)', stopped: true, reply: 'First the floor, then the' }, { id: 'u2', role: 'user', content: 'continue' }] };
  await runTurn({ conversation: stopped, conversationId: 'conv-stopped-reply', messages: [...hutMessages, { role: 'assistant', content: '(stopped by user)' }, { role: 'user', content: 'continue' }] });
  assert.equal(flag(recordedArgs(), '--resume'), 'sess-1');
  assert.equal(recordedInput().message.content[0].text, '<stopped-reply>\nThe user stopped your previous reply while you were writing it, and this session does not hold it. This is the text you had written, as the user saw it:\n\nAssistant:\nFirst the floor, then the\n</stopped-reply>\n\ncontinue');
});

test('every Claude Code run starts with the app named as its entrypoint', () => {
  assert.equal(claudeCode.claudeEnv().CLAUDE_CODE_ENTRYPOINT, 'mcbench-lite');
});

test('every Claude Code run, a chat\'s or a one-shot\'s, works in the one folder outside any git work tree', async () => {
  assert.equal(CWD, path.join(os.homedir(), '.cache', 'mcbench', 'claude-cwd'));
  fs.mkdirSync(CWD, { recursive: true });
  assert.notEqual(require('node:child_process').spawnSync('git', ['-C', CWD, 'rev-parse', '--is-inside-work-tree']).status, 0);
  await runTurn({ conversation: hut, conversationId: 'conv-cwd-a', messages: hutMessages });
  assert.equal(fs.realpathSync(fs.readFileSync(CWD_FILE, 'utf8')), fs.realpathSync(CWD));
  await runTurn({ conversation: hut, conversationId: 'conv-cwd-b', messages: hutMessages });
  assert.equal(fs.realpathSync(fs.readFileSync(CWD_FILE, 'utf8')), fs.realpathSync(CWD));
  await runTurn({ messages: [{ role: 'system', content: 'Name this chat.' }, { role: 'user', content: 'A hut' }] });
  assert.equal(fs.realpathSync(fs.readFileSync(CWD_FILE, 'utf8')), fs.realpathSync(CWD));
  assert.equal(fs.existsSync(path.join(harness.conversationDir('conv-cwd-a'), 'claude-cwd')), false);
});

test('archiving a conversation kills its warm process, and shutdown kills every one', async () => {
  await runTurn({ conversation: hut, conversationId: 'conv-archive', messages: hutMessages });
  const archived = processes.get('conv-archive');
  assert.equal(archived.state, 'idle');
  harness.destroy('conv-archive');
  await archived.exited.promise;
  assert.equal(archived.state, 'exited');
  assert.equal(processes.has('conv-archive'), false);

  await runTurn({ conversation: hut, conversationId: 'conv-shutdown', messages: hutMessages });
  const warm = processes.get('conv-shutdown');
  await harness.shutdownHarnesses();
  assert.equal(warm.state, 'exited');
  assert.equal(processes.size, 0);
});

test('a Claude Code process whose cleanup fails once it has exited is logged, and has still exited', async (t) => {
  await runTurn({ conversation: hut, conversationId: 'conv-cleanup', messages: hutMessages });
  const proc = processes.get('conv-cleanup');
  const { server } = proc;
  // The MCP server is closed for real once the test is done.
  const close = server.shutdown;
  t.after(() => close());
  t.mock.method(server, 'shutdown', async () => { throw new Error('the socket is busy'); });
  const logged = t.mock.method(console, 'error', () => {});
  const unhandled = [];
  const record = (reason) => unhandled.push(reason);
  process.on('unhandledRejection', record);
  try {
    const exited = await Promise.race([proc.kill().then(() => true), new Promise((resolve) => setTimeout(resolve, 2000, false))]);
    assert.equal(exited, true, 'kill resolves');
    await new Promise(setImmediate);
  } finally {
    process.off('unhandledRejection', record);
  }
  assert.deepEqual(unhandled, []);
  assert.equal(proc.state, 'exited');
  assert.ok(logged.mock.calls.some((entry) => entry.arguments[0] === '[claude-code]' && /the socket is busy/.test(entry.arguments[1]?.message)));
});

test('a follow-up turn forgets tool_use blocks whose MCP call never came', async () => {
  await runTurn({ conversation: hut, conversationId: 'conv-blocks', messages: hutMessages });
  const proc = processes.get('conv-blocks');
  proc.toolUse('toolu_stale');
  const second = { messages: [...hut.messages, { id: 'a1', role: 'assistant', content: 'Built' }, { id: 'u2', role: 'user', content: 'Make it taller.' }] };
  await runTurn({ conversation: second, conversationId: 'conv-blocks', messages: [...hutMessages, { role: 'assistant', content: 'Built' }, { role: 'user', content: 'Make it taller.' }] });
  assert.equal(processes.get('conv-blocks'), proc);
  assert.equal(proc.toolUses.has('toolu_stale'), false);
});

test('a Claude Code process that cannot be spawned has exited: closing its turn and shutting down do not wait for it', async () => {
  const session = new ClaudeCodeSession();
  // A NUL byte in an argument makes child_process.spawn throw at once.
  await assert.rejects(session.start({ conversation: hut, conversationId: 'conv-no-spawn', messages: hutMessages, tools: [TOOL], model: 'claude\0opus' }), /null bytes/);
  const proc = session.process;
  assert.equal(proc.state, 'exited');
  assert.equal(proc.run, undefined);
  session.close();
  await proc.exited.promise;
  assert.equal(fs.existsSync(proc.runtimeDir), false);
  assert.equal(processes.has('conv-no-spawn'), false);
  await harness.shutdownHarnesses();
  // An exited process takes no turn: every other change of state throws.
  assert.throws(() => proc.begin(session, ''), /is exited cannot become busy/);
  assert.equal(proc.state, 'exited');
});
