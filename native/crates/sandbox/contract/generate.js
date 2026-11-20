'use strict';
// native/crates/sandbox/contract/generate.js [--check]
//
// Writes the files beside it from today's server/, never by hand:
//   egress-rules.sh  server/sandbox.js EGRESS_RULES, the networked box's rules
//   ipv6-off.json    server/sandbox.js IPV6_OFF, the sysctls set before them
//   uv-errors.json   libuv's message for every error name it has
//                    (util.getSystemErrorMap; sandbox.js workError tells a
//                    /work file error by this text and the code's name)
// With --check it writes nothing and exits 1 when a file differs.
//
// `node generate.js format` reads JSON lines of formatResult/truncateMiddle
// cases on stdin and prints Node's answers, one JSON line each (the Rust
// test tests/transcript_node.rs).
//
// sandbox.js does not export these, so it is loaded with one line appended
// that hands them out; nothing in server/ changes.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const util = require('node:util');
const Module = require('node:module');

const repo = path.resolve(__dirname, '../../../..');
const file = path.join(repo, 'server', 'sandbox.js');

function internals() {
  if (!process.env.MCBENCH_DATA_DIR) {
    const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-sandbox-contract-'));
    process.env.MCBENCH_DATA_DIR = scratch;
    process.on('exit', () => fs.rmSync(scratch, { recursive: true, force: true }));
  }
  const source = fs.readFileSync(file, 'utf8')
    + '\nmodule.exports.__contract = { EGRESS_RULES, IPV6_OFF, formatResult, truncateMiddle };\n';
  const loaded = new Module(file, module);
  loaded.filename = file;
  loaded.paths = Module._nodeModulePaths(path.dirname(file));
  loaded._compile(source, file);
  return loaded.exports.__contract;
}

if (process.argv[2] === 'format') {
  const { formatResult, truncateMiddle } = internals();
  const lines = fs.readFileSync(0, 'utf8').split('\n').filter(Boolean);
  const answers = lines.map((line) => {
    const c = JSON.parse(line);
    const out = c.kind === 'truncate'
      ? truncateMiddle(c.text, c.maxBytes)
      : formatResult(c.body, c.exitCode, c.durationMs, c.timedOut, c.ms, c.note);
    // A lone surrogate is written as U+FFFD, as Buffer writes it.
    const text = typeof out === 'string' ? out.toWellFormed() : { ...out, text: out.text.toWellFormed() };
    return JSON.stringify({ out: text }) + '\n';
  });
  // Written whole before exiting: a pipe's writes are asynchronous.
  fs.writeSync(1, answers.join(''));
  process.exit(0);
}

const { EGRESS_RULES, IPV6_OFF } = internals();
const uv = {};
for (const [name, message] of [...util.getSystemErrorMap().values()].sort(([a], [b]) => (a < b ? -1 : 1))) uv[name] = message;
const files = {
  'egress-rules.sh': EGRESS_RULES,
  'ipv6-off.json': JSON.stringify(IPV6_OFF, null, 2) + '\n',
  'uv-errors.json': JSON.stringify(uv, null, 2) + '\n',
};
const check = process.argv[2] === '--check';
let differs = false;
for (const [name, text] of Object.entries(files)) {
  const target = path.join(__dirname, name);
  if (check) {
    const now = fs.existsSync(target) ? fs.readFileSync(target, 'utf8') : null;
    if (now !== text) { differs = true; console.error(`${name} differs from server/`); }
  } else {
    fs.writeFileSync(target, text);
  }
}
process.exit(differs ? 1 : 0);
