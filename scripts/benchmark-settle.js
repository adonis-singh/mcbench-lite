#!/usr/bin/env node
'use strict';

// What a build call costs on a stored conversation's world, with Minecraft's
// settle in it (agent.js executeBuild): the first call (the world is settled
// before the program, because nothing has settled it yet), then one-block
// edits (the settle is the world's own, kept from the call before), and the
// same edit by a program that reads mc.settled. Medians over --runs.
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/benchmark-settle.js [--runs 5] [--java] [--both] [--ticks 100] <conversation id>...
// --ticks sets the build's settle_ticks (default: the build tool's).
// --java adds a block the Rust engine refuses (resin_clump), so every settle
// is the Java engine's. --both alternates the settle's ticks without light
// (what shot-world.js asks for) and with it (a stock TICK), call by call.
// BENCH_ROOT runs another checkout's server/ (the build calls before the
// settle existed): it times the build calls alone.
// Wrap in the native-engine lock: until mkdir /tmp/mcbench-native.lock; do sleep 30; done

const path = require('node:path');
const { performance } = require('node:perf_hooks');

const root = path.resolve(process.env.BENCH_ROOT || path.join(__dirname, '..'), 'server');
const shotWorld = require(path.join(root, 'shot-world'));
const settles = [];
let stockLight = false;
let settleTicks;
if (shotWorld.settle) {
  const settle = shotWorld.settle;
  shotWorld.settle = async (...args) => {
    const started = performance.now();
    const results = await settle(...args);
    const result = [...results.values()].at(-1);
    settles.push({ ms: performance.now() - started, cached: result.cached === true, timings: result.timings });
    return results;
  };
  const { Engine } = require(path.join(root, 'engine'));
  const tick = Engine.prototype.tick;
  Engine.prototype.tick = function benchmarkTick(ticks, patch, base, entities, options) {
    return tick.call(this, ticks, patch, base, entities, stockLight ? undefined : options);
  };
}
const store = require(path.join(root, 'store'));
const { getBlockRegistry } = require(path.join(root, 'assets'));
const { executeBuild } = require(path.join(root, 'agent'))._internal;

const median = (values) => [...values].sort((a, b) => a - b)[Math.floor(values.length / 2)];
const round = (value) => Math.round(value * 10) / 10;

async function measure(conversation, registry, program) {
  settles.length = 0;
  const started = performance.now();
  const result = await executeBuild(conversation, { program, ...(settleTicks === undefined ? {} : { settle_ticks: settleTicks }) }, registry);
  const ms = performance.now() - started;
  if (!result.ok) throw new Error(result.error);
  return { ms, settles: settles.map((s) => ({ ...s })) };
}

// Medians of a variant's calls: the whole call, the settle in it, and each part of the settle.
function summary(calls) {
  const posts = calls.map((call) => call.settles.at(-1));
  return {
    callMs: round(median(calls.map((call) => call.ms))),
    fastestCallMs: round(Math.min(...calls.map((call) => call.ms))),
    ...(posts[0] ? {
      settleMs: round(median(posts.map((post) => post.ms))),
      fastestSettleMs: round(Math.min(...posts.map((post) => post.ms))),
      parts: Object.fromEntries(Object.keys(posts[0].timings).filter((name) => typeof posts[0].timings[name] === 'number').map((name) => [name, round(median(posts.map((post) => post.timings[name])))])),
      engineRuns: calls.map((call) => call.settles.filter((s) => !s.cached).length).join(''),
    } : {}),
  };
}

async function main() {
  const args = process.argv.slice(2);
  const runsAt = args.indexOf('--runs');
  const runs = runsAt === -1 ? 5 : Number(args.splice(runsAt, 2)[1]);
  const ticksAt = args.indexOf('--ticks');
  if (ticksAt !== -1) settleTicks = Number(args.splice(ticksAt, 2)[1]);
  const java = args.includes('--java');
  const variants = args.includes('--both') ? ['noLight', 'stockLight'] : ['noLight'];
  const ids = args.filter((arg) => !arg.startsWith('--'));
  const registry = await getBlockRegistry();
  for (const id of ids) {
    const conversation = store.getConversation(id);
    const bounds = conversation.world.bounds();
    const at = [bounds.min[0], bounds.max[1] + 3, bounds.min[2]];
    const edit = (on) => `mc.set(${JSON.stringify(at)}, ${on ? "'stone'" : "'air'"});`;
    const toggle = (on) => `module.exports = (mc) => { ${edit(on)} };`;
    const read = (on) => `module.exports = (mc) => { mc.log(mc.settled(${at.join(', ')}), mc.settled(${bounds.min.join(', ')})); ${edit(on)} };`;
    const row = { conversation: id.slice(0, 8), title: conversation.title, blocks: conversation.world.size(), engine: java ? 'java' : 'rust' };
    if (java) {
      const placed = await executeBuild(conversation, { program: `module.exports = (mc) => mc.set([${bounds.min[0]}, ${bounds.max[1] + 1}, ${bounds.min[2]}], 'resin_clump');` }, registry);
      if (!placed.ok) throw new Error(placed.error);
    }
    const cold = await measure(conversation, registry, toggle(true));
    const calls = Object.fromEntries(variants.map((variant) => [variant, []]));
    const reads = [];
    let on = true;
    for (let i = 0; i < runs; i += 1) {
      for (const variant of variants) {
        stockLight = variant === 'stockLight';
        on = !on;
        calls[variant].push(await measure(conversation, registry, toggle(on)));
      }
      if (shotWorld.settle) {
        stockLight = false;
        on = !on;
        reads.push(await measure(conversation, registry, read(on)));
      }
    }
    row.cold = { callMs: round(cold.ms), settleMs: cold.settles.map((s) => round(s.ms)) };
    for (const variant of variants) row[variant] = summary(calls[variant]);
    if (reads.length) row.withSettledRead = { callMs: round(median(reads.map((call) => call.ms))) };
    console.log(JSON.stringify(row));
  }
  await shotWorld.stopShotEngines?.();
  store.close();
}

main().catch((error) => { console.error(error); process.exit(1); });
