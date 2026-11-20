'use strict';

// node pin.js <node.jsonl> <linux-rust.jsonl> <macos-rust.jsonl>
//
// Writes allow-list.json: every case of the golden corpus whose Rust result
// differs from Node's, with its class (compare.js's), its cause (below; a
// difference without a cause here fails) and the Rust result pinned, once
// when Linux and macOS agree, per platform otherwise. tests/child requires
// exactly the pinned result.

const fs = require('fs');
const path = require('path');

const HOST_FRAMES = 'host frames: below build.js the stack shows the child\'s files (/shim/prelude.js, /server/mc-dsl.js, /server/build-child.js) where Node shows node:vm and the server\'s own paths; formatProgramError keeps only build.js frames, so a model sees them only when a program prints a stack it captured itself';
const ESCAPE = 'escape probe: the vector reaches the child\'s main realm, whose process is the prelude\'s { env, exit } (no pid, no permission model, no getBuiltinModule), where Node\'s reaches Node\'s real process; nothing on the host is reachable from either';
const CAUSES = {
  'adv-capture-stack-trace': HOST_FRAMES,
  'adv-prepare-stack-trace': HOST_FRAMES,
  'adv-lone-surrogate': 'a lone surrogate in a log line: JSON.stringify writes it as \\udc00 and a Rust String cannot hold it, so the parent reads it as U+FFFD; keeping it needs a JavaScript string type through BuildOutcome and mcbench-json (seam request in the report)',
  'adv-math-bits': 'Math bits: sin and cos are the embedded V8\'s own (the owner\'s decision, docs/native-app.md Math results); between macOS arm64 and Linux x64, sqrt and ** differ in the sign of NaN results (0x7ff8000000000000 on the Mac, 0xfff8000000000000 on Linux) and pow and ** in value, because V8 calls the platform C library\'s pow for exponents other than 0.5 and 2 (tests/child math-fixed)',
  'adv-recursion-depth-exact': 'recursion depth: the child\'s 8 MiB main-thread stack under V8\'s default stack limit holds a few more frames than Node\'s; the RangeError and its text are the same',
  'recbuildsandbox-0012': ESCAPE,
  'recbuildsandbox-0013': ESCAPE,
  'recbuildsandbox-0014': ESCAPE,
  'recbuildsandbox-0015': ESCAPE,
  'recbuildsandbox-0016': ESCAPE,
};

const [nodeFile, linuxFile, macFile] = process.argv.slice(2);
const read = (file) => new Map(fs.readFileSync(file, 'utf8').trim().split('\n').map((line) => { const o = JSON.parse(line); return [o.id, o]; }));
const strip = (o) => (o.ok
  ? { ok: true, changes: o.changes, logs: o.logs, files: o.files, touched: o.touched }
  : { ok: false, error: o.error, logs: o.logs });
const node = read(nodeFile);
const linux = read(linuxFile);
const mac = read(macFile);
const same = (a, b) => JSON.stringify(strip(a)) === JSON.stringify(strip(b));
// Cases whose Rust result depends on the build of the binary (the depth of
// recursion depends on the stack the host code used before the program
// started: 12,535 in the release app on Linux, 12,282 in the debug test
// binary): not pinned; tests/child requires one integer above 10,000.
const UNPINNED = new Set(['adv-recursion-depth-exact']);
const out = {};
for (const [id, n] of node) {
  const l = linux.get(id); const m = mac.get(id);
  if (same(n, l) && same(n, m)) continue;
  if (!CAUSES[id]) throw new Error(`${id} differs without a cause:\n node  ${JSON.stringify(strip(n)).slice(0, 400)}\n linux ${JSON.stringify(strip(l)).slice(0, 400)}\n macos ${JSON.stringify(strip(m)).slice(0, 400)}`);
  out[id] = {
    class: !l.ok && n.ok ? 'fails in rust only' : (l.ok && n.ok ? 'logs differ' : 'error message wording differs'),
    cause: CAUSES[id],
    ...(UNPINNED.has(id)
      ? { unpinned: true, measured: { linux: strip(l).logs, macos: strip(m).logs } }
      : { rust: same(l, m) ? strip(l) : { linux: strip(l), macos: strip(m) } }),
  };
}
fs.writeFileSync(path.join(__dirname, 'allow-list.json'), `${JSON.stringify(out, null, 1)}\n`);
console.log(`${Object.keys(out).length} cases on the allow-list`);
