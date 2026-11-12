'use strict';

// Checks how the Rust engine (core/) reads and writes saved data against how
// the pinned server does (McbenchProbe.java), line by line: SNBT, item stacks
// (ItemStack.CODEC), block entities (BlockEntity.loadWithComponents and its
// saved data) and entities (EntityType.loadEntityRecursive and its saved
// data). Both answer each line "<command> <rest>" with one line, and the
// answers must be identical, except that the Rust engine may refuse
// ("REFUSE ...": what it does not model) where the server answers anything,
// and may not answer OK where the server fails. A refusal where the server
// answers OK is counted (coverage), not a mismatch.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/core-probe-parity.js
//     [--kind snbt,stack,be,entity,mpshape]   which questions (default snbt,stack)
//     [--fuzz N] [--seed S]           N random inputs of each kind
//     [--file lines.txt]              lines of "<command> <rest>" to ask instead
//     [--refusals]                    list the refusals with their reasons
//     [--keep file]                   write the lines asked
//     [--answers]                     print every line with both answers

const fs = require('fs');
const path = require('path');
const os = require('os');
const readline = require('readline');
const { execFileSync, spawn } = require('child_process');
const ROOT = path.join(__dirname, '..');
const { minecraftLive } = require('../server/minecraft-live');
const { graalvmHome, coreBuildArgs, rust } = require('../server/engine');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

const { random, compound, snbtValue, stack, blockEntity, entityItem, BLOCK_ENTITIES } = require('./saved-data-fuzz');

// The moving piston's collision shapes (mpshape): every state that has a
// collision shape of its own, as the block a piston moves (going out or in,
// at the progresses a tick sees and just before the block is set down, with
// the push's direction not in the way or not), and the pistons' own parts.
function movingPistonLines(version, every = false) {
  const blocks = [];
  let block = null;
  require('../server/minecraft-tables').checkTables(version);
  for (const line of fs.readFileSync(path.join(ROOT, 'core', 'minecraft', version, 'blocks.txt'), 'utf8').split('\n')) {
    const p = line.split('\t');
    if (p[0] === 'block') blocks.push(block = { key: p[1], properties: [], states: [] });
    else if (p[0] === 'property') block.properties.push([p[1], p[2].split(',')]);
    else if (p[0] === 'state') block.states.push({ values: p[2] === '-' ? [] : p[2].split(',').map(Number), shape: p[10] });
  }
  const text = (b, state) => `${b.key}${b.properties.length ? `[${b.properties.map(([name, values], i) => `${name}=${values[state.values[i]]}`).join(',')}]` : ''}`;
  const seen = new Map();
  for (const b of blocks) {
    if (b.key === 'minecraft:moving_piston') continue;
    for (const state of b.states) if (every || !seen.has(state.shape)) seen.set(every ? `${seen.size}` : state.shape, text(b, state));
  }
  const dirs = ['down', 'up', 'north', 'south', 'west', 'east'];
  const opposite = { down: 'up', up: 'down', north: 'south', south: 'north', west: 'east', east: 'west' };
  const lines = [];
  const add = (moved, direction, extending, source, progress) => {
    const movement = extending ? direction : opposite[direction];
    for (const noclip of ['-', movement, dirs[(dirs.indexOf(movement) + 2) % 6]]) lines.push(`mpshape ${moved} ${direction} ${extending ? 1 : 0} ${source ? 1 : 0} ${progress} ${noclip}`);
  };
  if (every) {
    // (The blocks whose shape is offset by position, at many positions.)
    for (const b of blocks) {
      if (b.key !== 'minecraft:bamboo' && b.key !== 'minecraft:pointed_dripstone') continue;
      for (const state of b.states) for (let x = -9; x < 9; x += 1) for (let z = -9; z < 9; z += 2) lines.push(`mpshape ${text(b, state)} up 1 0 0.5 - ${x * 7} ${z * 5 + x}`);
    }
    // (Every state, in one way of being moved each.)
    [...seen.values()].forEach((moved, i) => add(moved, dirs[i % 6], i % 2 === 0, false, i % 3 === 0 ? '0.0' : '0.5'));
    return lines;
  }
  for (const moved of seen.values()) {
    for (const direction of dirs) for (const extending of [true, false]) for (const progress of ['0.0', '0.5', '1.0']) add(moved, direction, extending, false, progress);
  }
  for (const direction of dirs) {
    for (const progress of ['0.0', '0.5', '1.0']) {
      for (const piston of ['minecraft:piston', 'minecraft:sticky_piston']) add(`${piston}[extended=false,facing=${direction}]`, direction, false, true, progress);
      for (const type of ['normal', 'sticky']) for (const short of ['true', 'false']) add(`minecraft:piston_head[facing=${direction},short=${short},type=${type}]`, direction, true, true, progress);
    }
  }
  return lines;
}

function inputs(kinds, count, seed) {
  const lines = [];
  for (const kind of kinds) {
    const r = random(seed * 7919 + kind.length * 31 + kind.charCodeAt(0));
    for (let i = 0; i < count; i++) {
      if (kind === 'snbt') lines.push(`value ${snbtValue(r)}`, `snbt ${compound(Array.from({ length: r.int(4) }, () => [r.pick(['a', 'b', 'c']), snbtValue(r)]))}`);
      else if (kind === 'mpshape' || kind === 'mpstates') { if (i === 0) lines.push(...movingPistonLines(minecraftLive.minecraftVersion, kind === 'mpstates')); }
      else if (kind === 'stack') lines.push(`stack ${stack(r)}`);
      else if (kind === 'entity') lines.push(`entity ${entityItem(r, i, null, i % 3 === 0 ? null : i % 3 === 1 ? 'minecraft:item_frame' : 'minecraft:glow_item_frame')}`);
      else if (kind === 'be') {
        const block = r.pick(BLOCK_ENTITIES);
        lines.push(`be ${i & 15} ${1 + ((i >> 8) % 100)} ${(i >> 4) & 15} ${block} ${blockEntity(r, block)}`);
      }
    }
  }
  return lines;
}

// --- Running ---

function probe(command, args, lines, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd, stdio: ['pipe', 'pipe', 'pipe'] });
    const answers = [];
    let stderr = '';
    child.stderr.on('data', (chunk) => { stderr = `${stderr}${chunk}`.slice(-3000); });
    readline.createInterface({ input: child.stdout }).on('line', (line) => { if (line.startsWith('A ')) answers.push(line.slice(2)); });
    child.on('error', reject);
    child.on('close', (code) => (answers.length === lines.length ? resolve(answers) : reject(new Error(`${command} answered ${answers.length} of ${lines.length} lines (exit ${code}): ${stderr}`))));
    child.stdin.write(`${lines.join('\n')}\n`);
    child.stdin.end();
  });
}

async function main() {
  const kinds = option('kind', 'snbt,stack').split(',');
  const seed = Number(option('seed', 1));
  const lines = option('file', null) ? fs.readFileSync(option('file'), 'utf8').split('\n').filter(Boolean) : inputs(kinds, Number(option('fuzz', 200)), seed);
  if (option('keep', null)) fs.writeFileSync(option('keep'), `${lines.join('\n')}\n`);

  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-probe-parity-'));
  const classes = path.join(scratch, 'classes');
  fs.mkdirSync(classes);
  fs.mkdirSync(path.join(scratch, 'work'));
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const graalvm = graalvmHome();
  execFileSync(path.join(graalvm, 'bin', 'javac'), ['--release', '21', '-proc:none', '-XDignore.symbol.file', '-classpath', classpath.join(path.delimiter), '-d', classes,
    ...['McbenchEngine.java', 'McbenchLevel.java', 'McbenchExperiment.java'].map((name) => path.join(ROOT, 'server', name)), path.join(__dirname, 'McbenchProbe.java')], { stdio: ['ignore', 'ignore', 'inherit'] });
  execFileSync(rust().cargo, coreBuildArgs(), { cwd: path.join(ROOT, 'core'), env: rust().env, stdio: 'inherit' });
  const [java, rust] = await Promise.all([
    probe(path.join(graalvm, 'bin', 'java'), [...(process.arch === 'arm64' ? ['-XX:+UnlockDiagnosticVMOptions', '-XX:DisableIntrinsic=_dsin,_dcos'] : []), '-cp', [classes, ...classpath].join(path.delimiter), 'McbenchProbe'], lines, path.join(scratch, 'work')),
    probe(path.join(ROOT, 'core', 'target', 'release', 'mcbench-core'), ['--probe'], lines, scratch),
  ]);
  const totals = { same: 0, refused: 0, javaFailed: 0, mismatched: 0 };
  const reasons = new Map();
  lines.forEach((line, i) => {
    const [j, r] = [java[i], rust[i]];
    const javaOk = j.startsWith('OK ');
    if (process.argv.includes('--answers')) console.log(`${line.slice(0, 400)}\n  java: ${j.slice(0, 400)}\n  rust: ${r.slice(0, 400)}`);
    if (r.startsWith('REFUSE ')) {
      if (javaOk) { totals.refused++; const why = r.slice(7).replace(/[0-9]+/g, 'N').slice(0, 100); reasons.set(why, (reasons.get(why) || 0) + 1); } else totals.javaFailed++;
      if (process.argv.includes('--refusals') && javaOk) console.log(`refused   ${line.slice(0, 160)}\n          ${r.slice(0, 200)}`);
    } else if (j === r) totals.same++;
    else {
      totals.mismatched++;
      console.log(`MISMATCH  ${line}\n  java: ${j}\n  rust: ${r}`);
    }
  });
  console.log(JSON.stringify(totals));
  for (const [why, n] of [...reasons].sort((a, b) => b[1] - a[1]).slice(0, 25)) console.log(`  ${n}\t${why}`);
  fs.rmSync(scratch, { recursive: true, force: true });
  if (totals.mismatched) process.exitCode = 1;
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
