#!/usr/bin/env node
'use strict';

// A stand-in for `codex` (codex-cli 0.167.1) in tests. `login --device-auth`
// prints what the real CLI prints on a pipe, colours included (copied from
// it, with a made-up one-time code), and then waits as the real one does
// while it asks OpenAI whether the code was entered: here, until the test
// writes fake-approve (the login file is written and it exits 0, as the real
// one does) or fake-deny (it fails) in CODEX_HOME. The BROWSER it was given
// is written beside it for the test to read. Like the real one, every start
// links apply_patch, in a folder under CODEX_HOME/tmp, to its own executable
// (here the script it was started as, which may be a wrapper that requires
// this file), except, as the real one does on Linux, when CODEX_HOME is under
// the temporary folder. `debug models` prints a one-model catalogue, and
// `exec` prints the executable that ran.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const home = process.env.CODEX_HOME;
const args = process.argv.slice(2).join(' ');
const auth = path.join(home, 'auth.json');
const executable = fs.realpathSync(process.argv[1]);
fs.mkdirSync(home, { recursive: true });
const temporary = fs.realpathSync(os.tmpdir());
if (`${fs.realpathSync(home)}/`.startsWith(`${temporary}/`)) {
  process.stderr.write(`WARNING: proceeding, even though we could not create PATH aliases: Refusing to create helper binaries under temporary dir "${temporary}"\n`);
} else {
  fs.mkdirSync(path.join(home, 'tmp', 'arg0'), { recursive: true });
  const arg0 = fs.mkdtempSync(path.join(home, 'tmp', 'arg0', 'codex-arg0'));
  fs.symlinkSync(executable, path.join(arg0, 'apply_patch'));
}

if (args === '--version') {
  process.stdout.write('codex-cli 0.167.1\n');
} else if (args === 'logout') {
  if (fs.existsSync(auth)) {
    fs.rmSync(auth);
    process.stdout.write('Successfully logged out\n');
  } else {
    process.stderr.write('Not logged in\n');
  }
} else if (args === 'login --device-auth') {
  fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(path.join(home, 'fake-browser'), process.env.BROWSER || '');
  process.stdout.write('\nWelcome to Codex [v\u001b[90m0.167.1\u001b[0m]\n\u001b[90mOpenAI\'s command-line coding agent\u001b[0m\n\n'
    + 'Follow these steps to sign in with ChatGPT using device code authorization:\n\n'
    + '1. Open this link in your browser and sign in to your account\n   \u001b[94mhttps://auth.openai.com/codex/device\u001b[0m\n\n'
    + '2. Enter this one-time code \u001b[90m(expires in 15 minutes)\u001b[0m\n   \u001b[94mFAKE-C0DE1\u001b[0m\n\n'
    + '\u001b[90mContinue only if you started this login in Codex. If a website or another person gave you this code, cancel.\u001b[0m\n\n');
  const timer = setInterval(() => {
    if (fs.existsSync(path.join(home, 'fake-approve'))) {
      const claims = { email: 'person@example.com', 'https://api.openai.com/auth': { chatgpt_plan_type: 'pro' } };
      const idToken = ['e30', Buffer.from(JSON.stringify(claims)).toString('base64url'), 'sig'].join('.');
      fs.writeFileSync(auth, JSON.stringify({ tokens: { id_token: idToken } }));
      process.stdout.write('Successfully logged in\n');
      clearInterval(timer);
    } else if (fs.existsSync(path.join(home, 'fake-deny'))) {
      process.stderr.write('Error logging in: device code exchange failed: the code was denied\n');
      process.exit(1);
    }
  }, 20);
} else if (args === 'debug models') {
  process.stdout.write(JSON.stringify({ models: [{ slug: 'fake-model', display_name: 'Fake Model', visibility: 'list', priority: 1 }] }));
} else if (process.argv[2] === 'exec') {
  process.stdout.write(`${JSON.stringify({ type: 'fake.ran', executable })}\n`);
  codeModeTurn();
} else {
  process.stderr.write(`fake codex: unexpected arguments ${args}\n`);
  process.exit(2);
}

// A turn of a code_mode_only model, whose one tool is Code Mode's `exec`, as
// Codex 0.167.1 and 0.169.0 run it, with the settings of CODEX_HOME's
// config.toml (one dotted key per line, as the harness writes it) and the
// `-c` overrides. Like 0.169.0, it warns in an error item about
// features.multi_agent set in the config file. With features.code_mode_host
// off, `exec` fails and Codex reports the error item it reports then;
// otherwise the script calls each tool the mcbench MCP server (started from
// the mcp_servers.mcbench settings, over stdio) lists, with no arguments,
// and each call is an mcp_tool_call item.
function codeModeTurn() {
  const configFile = path.join(home, 'config.toml');
  const file = fs.existsSync(configFile)
    ? Object.fromEntries(fs.readFileSync(configFile, 'utf8').split('\n').filter(Boolean).map((line) => line.split(/ = (.*)/s).slice(0, 2)))
    : {};
  const overrides = { ...file, ...Object.fromEntries(process.argv.flatMap((arg, i) => (process.argv[i - 1] === '-c' ? [arg.split(/=(.*)/s).slice(0, 2)] : []))) };
  const emit = (event) => process.stdout.write(`${JSON.stringify(event)}\n`);
  emit({ type: 'thread.started', thread_id: 'fake-thread' });
  if ('features.multi_agent' in file) {
    emit({ type: 'item.completed', item: { id: 'item_warning', type: 'error', message: '`[features].multi_agent` is deprecated. It still controls the V1 fallback. `[agents].enabled = false` disables subagent tools even when the model selects a subagent version, unless `[features].multi_agent_v2` is enabled.' } });
  }
  const end = () => emit({ type: 'turn.completed', usage: { input_tokens: 1, cached_input_tokens: 0, output_tokens: 1, reasoning_output_tokens: 0 } });
  if (overrides['features.code_mode_host'] === 'false') {
    emit({ type: 'item.completed', item: { id: 'item_0', type: 'error', message: 'Code Mode is unavailable because code-mode host is disabled. Code mode will fail closed; enable `features.code_mode_host` and install `codex-code-mode-host`.' } });
    return end();
  }
  if (!overrides['mcp_servers.mcbench.command']) return end();
  const { spawn } = require('node:child_process');
  const readline = require('node:readline');
  const server = spawn(JSON.parse(overrides['mcp_servers.mcbench.command']), JSON.parse(overrides['mcp_servers.mcbench.args']), { stdio: ['pipe', 'pipe', 'inherit'] });
  const waiting = new Map();
  let id = 0;
  readline.createInterface({ input: server.stdout }).on('line', (line) => {
    const message = JSON.parse(line);
    waiting.get(message.id)?.(message);
  });
  const request = (method, params) => new Promise((resolve) => {
    waiting.set(++id, resolve);
    server.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
  });
  (async () => {
    await request('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'fake-codex', version: '0.167.1' } });
    server.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
    const { result } = await request('tools/list', {});
    for (const [index, tool] of result.tools.entries()) {
      const item = { id: `item_${index + 1}`, type: 'mcp_tool_call', server: 'mcbench', tool: tool.name, arguments: {} };
      emit({ type: 'item.started', item: { ...item, status: 'in_progress' } });
      const reply = await request('tools/call', { name: tool.name, arguments: {} });
      emit({ type: 'item.completed', item: { ...item, status: 'completed', result: reply.result } });
    }
    server.stdin.end();
    end();
  })();
}
