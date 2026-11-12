'use strict';

const test = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const readline = require('node:readline');
const { buildCore } = require('../server/engine');

// The native engine's protocol (core/src/main.rs) on requests in the wrong
// order or malformed: each fails alone, with a reply, and the world goes on.
test('requests out of order or malformed fail with a reply and refuse nothing', async (t) => {
  const binary = await buildCore();
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-protocol-'));
  t.after(() => fs.rmSync(folder, { recursive: true, force: true }));
  const file = (name) => path.join(folder, name);
  fs.writeFileSync(file('commands.txt'), 'setblock 0 64 0 minecraft:stone\n');
  const child = spawn(binary, [], { cwd: folder, stdio: ['pipe', 'pipe', 'inherit'] });
  t.after(() => child.kill());
  const lines = readline.createInterface({ input: child.stdout })[Symbol.asyncIterator]();
  const next = async () => (await lines.next()).value;
  assert.strictEqual(await next(), 'MCBENCH_ENGINE_READY');
  const ask = async (line) => { child.stdin.write(`${line}\n`); return next(); };
  const state = async (id) => {
    assert.match(await ask(`STATE ${id} ${file('state.txt')}`), new RegExp(`^MCBENCH_DONE ${id} \\d+$`));
    return fs.readFileSync(file('state.txt'), 'utf8');
  };

  assert.match(await state(1), /^time 0$/m);
  // A tick before any base fails after its ticks ran, where McbenchEngine.tick
  // meets the null changes: the world's time went on.
  assert.strictEqual(await ask(`TICK 2 5 ${file('p')} ${file('e')}`), 'MCBENCH_TICK_FAILED 2 java.lang.NullPointerException: no base written');
  assert.match(await state(3), /^time 5$/m);
  assert.match(await ask(`PTICK 4 0 0 0 ${file('p')} ${file('e')} ${file('v')}`), /^MCBENCH_TICK_FAILED 4 .*PTICK before PSPAWN/);
  assert.strictEqual(await ask(`DUMP 5 ${file('d')}`), 'MCBENCH_FAILED 5 unknown request DUMP');
  assert.strictEqual(await ask('NOPE 6'), 'MCBENCH_FAILED 6 unknown request NOPE');
  assert.match(await ask('PLACE'), /^MCBENCH_PLACE_FAILED \? malformed request/);
  assert.match(await ask(`TICK 7 many ${file('p')} ${file('e')}`), /^MCBENCH_TICK_FAILED 7 malformed request/);
  assert.match(await ask(`PLACE 8 ${file('missing.txt')} ${file('b')} fresh`), /^MCBENCH_PLACE_FAILED 8 .*missing\.txt/);
  // None of these refused the world: a continuing placement and its ticks run.
  assert.match(await ask(`PLACE 9 ${file('commands.txt')} ${file('b')} continue`), /^MCBENCH_PLACED 9 \d+$/);
  assert.match(await ask(`TICK 10 2 ${file('p')} ${file('e')}`), /^MCBENCH_TICKED 10 \d+$/);
  assert.match(await ask(`PSPAWN 11 0.5 65 0.5 0 0 1`), /^MCBENCH_DONE 11 \d+$/);
  assert.match(await ask(`PTICK 12 0 0 0 ${file('p')} ${file('e')} ${file('v')}`), /^MCBENCH_TICKED 12 \d+$/);
  assert.strictEqual(fs.statSync(file('v')).size, 28 * 8);
  // A fresh placement replaces the world before it reads its commands: one
  // whose file is missing leaves a world without a base.
  assert.match(await ask(`PLACE 13 ${file('missing.txt')} ${file('b')} fresh`), /^MCBENCH_PLACE_FAILED 13 .*missing\.txt/);
  assert.strictEqual(await ask(`TICK 14 1 ${file('p')} ${file('e')}`), 'MCBENCH_TICK_FAILED 14 java.lang.NullPointerException: no base written');
});
