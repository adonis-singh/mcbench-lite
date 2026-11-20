#!/usr/bin/env node
'use strict';
// The Node side of the CLI harnesses' differential: runs one case of
// cases.py through today's server/claude-code.js or server/codex-cli.js and
// writes what happened to <out>/<case>/out.json (the Rust side, tests/cli.rs
// with MCBENCH_DIFF_HARNESS, writes the same shape).
//
//   node node-driver.js <repo> <cases.json> <case name> <out> <codex login>
//
// The fake CLI is tests/fakes/fake_cli.py, copied to <out>/<case>/bin/<cli>.
// MCBENCH_DATA_DIR is <out>/<case>/data. The caller gives the environment
// (HOME, TMPDIR, PATH, LANG), the same for both sides. On Linux the Codex host
// launcher (macOS's) is used, as the Rust side uses Launch::Host there: the
// container launcher is mcbench-sandbox's.

const fs = require('node:fs');
const path = require('node:path');

const [repo, casesFile, name, out, login] = process.argv.slice(2);
const testCase = JSON.parse(fs.readFileSync(casesFile, 'utf8')).find((c) => c.name === name);
const dir = path.join(out, name);
fs.rmSync(dir, { recursive: true, force: true });
// MCBENCH_DIFF_SANDBOXED=1: Codex in today's container (LAUNCHERS.linux),
// the fake in an install of Codex's layout, which the container mounts.
const sandboxed = process.env.MCBENCH_DIFF_SANDBOXED === '1' && testCase.cli === 'codex';
if (sandboxed && process.env.MCBENCH_I_OWN_THIS_DOCKER !== '1') {
  console.error('node-driver.js refused with MCBENCH_DIFF_SANDBOXED=1: it runs today\'s server/codex-cli.js container (named mcbench-codex-..., from the image localhost/mcbench-sandbox:<hash>), names that cannot be changed without editing server/; it is for a machine that is not the owner\'s (CI): set MCBENCH_I_OWN_THIS_DOCKER=1 there');
  process.exit(2);
}
const install = path.join(dir, 'install');
const bin = sandboxed ? path.join(install, 'bin') : path.join(dir, 'bin');
fs.mkdirSync(bin, { recursive: true });
if (sandboxed) fs.writeFileSync(path.join(install, 'codex-package.json'), JSON.stringify({ layoutVersion: 1, version: '0.169.0', variant: 'codex', entrypoint: 'bin/codex' }));
const program = path.join(bin, testCase.cli);
fs.copyFileSync(path.join(__dirname, '..', 'fakes', 'fake_cli.py'), program);
fs.chmodSync(program, 0o755);
fs.writeFileSync(path.join(bin, 'script.json'), JSON.stringify(testCase.script));
process.env.MCBENCH_DATA_DIR = path.join(dir, 'data');

const harness = require(path.join(repo, 'server', 'harness'));
const claudeCode = require(path.join(repo, 'server', 'claude-code'));
const codexCli = require(path.join(repo, 'server', 'codex-cli'));
const binary = testCase.missing_binary ? path.join(bin, 'missing') : program;
claudeCode.configure({ binary });
codexCli.configure({ binary, home: login });
const { LAUNCHERS } = codexCli._internal;
if (!sandboxed) LAUNCHERS.linux = LAUNCHERS.darwin;
const cli = testCase.cli === 'claude' ? claudeCode : codexCli;
const HARNESS = testCase.cli === 'claude' ? 'claude-code' : 'codex-cli';

const TOOLS = path.join(repo, 'native', 'crates', 'agent', 'contract', 'tools');
// The contract's files hold the budgets as holes (⟦key⟧); both sides fill
// them with the same number.
const tool = (file) => JSON.parse(fs.readFileSync(path.join(TOOLS, `${file}.json`), 'utf8').replace(/⟦[^⟧]*⟧/g, '12345'));
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

function tree(root) {
  const entries = {};
  if (!fs.existsSync(root)) return entries;
  for (const name of fs.readdirSync(root, { recursive: true }).sort()) {
    if (name.includes('fake-record')) continue;
    const file = path.join(root, name);
    const stat = fs.lstatSync(file);
    if (stat.isSymbolicLink()) entries[name] = { link: fs.readlinkSync(file) };
    else if (stat.isDirectory()) entries[name] = { dir: true };
    else entries[name] = { text: fs.readFileSync(file, 'utf8') };
  }
  return entries;
}

async function main() {
  const firstSystem = harness.sessionTerms(testCase.turns[0].messages);
  for (const preset of testCase.records || []) {
    const text = JSON.stringify(preset.record)
      .replaceAll('"$CWD"', JSON.stringify(claudeCode._internal.CWD))
      .replaceAll('"$INSTRUCTIONS"', JSON.stringify(firstSystem.instructions))
      .replaceAll('"$RESOLUTION"', JSON.stringify(firstSystem.resolution));
    harness.writeSession(preset.conv, preset.harness || HARNESS, JSON.parse(text));
  }
  const turns = [];
  for (const turn of testCase.turns) {
    const completions = [];
    const messages = JSON.parse(JSON.stringify(turn.messages));
    const controller = new AbortController();
    const conversationId = turn.conversation?.id;
    const conversation = turn.conversation?.transcript ? { messages: turn.conversation.transcript } : undefined;
    let answer = 0;
    for (;;) {
      const deltas = [];
      const onStarted = () => {
        deltas.push(JSON.stringify({ started: true }));
        if (turn.cancel?.on === 'started') controller.abort();
      };
      const onDelta = (delta) => {
        deltas.push(JSON.stringify(delta));
        if (turn.cancel?.on === 'content' && delta.content) controller.abort();
      };
      let message;
      try {
        message = await cli.completion({}, {
          conversationId, conversation, chat: turn.chat, model: turn.model, effort: turn.effort || undefined,
          onStarted, messages, tools: turn.tools.map(tool), signal: controller.signal, onDelta,
        });
      } catch (error) {
        completions.push({ deltas, error: error.message });
        break;
      }
      completions.push({ deltas, message: JSON.stringify(message) });
      if (!message.tool_calls) break;
      if (turn.cancel?.on === 'tool_call') {
        controller.abort();
        completions.push({ stopped: true });
        break;
      }
      const given = turn.answers[answer++];
      if (!given) {
        completions.push({ unanswered: true });
        break;
      }
      const call = message.tool_calls[0];
      messages.push({ role: 'assistant', content: message.content, tool_calls: message.tool_calls });
      messages.push({ role: 'tool', tool_call_id: call.id, content: given.content });
      if (given.screenshots) messages.push({ role: 'user', seeBuild: true, content: given.screenshots });
      if (given.delay) await sleep(given.delay);
    }
    // The turn is over, however it ended (llm.js closeHarnessSession).
    claudeCode.closeSession(conversationId);
    codexCli.closeSession(conversationId);
    turns.push(completions);
  }
  await harness.shutdownHarnesses();
  // A Codex run releases and cleans up after its completion settles.
  await sleep(300);
  // Inside the box the install is read-only: the fake records in CODEX_HOME.
  const records = sandboxed ? path.join(dir, 'data', 'harness', 'c1', 'codex-home', 'fake-record', 'record.jsonl') : path.join(bin, 'record.jsonl');
  const fake = fs.existsSync(records)
    ? fs.readFileSync(records, 'utf8').split('\n').filter(Boolean)
    : [];
  const result = { turns, harness: tree(path.join(dir, 'data', 'harness')), fake };
  fs.writeFileSync(path.join(dir, 'out.json'), JSON.stringify(result, null, 1));
}

main().then(() => process.exit(0), (error) => {
  console.error(error);
  process.exit(1);
});
