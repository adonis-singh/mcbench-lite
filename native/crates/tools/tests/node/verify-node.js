'use strict';

// The Node side of mcbench-tools' verifier differential: runs every case of
// verify-cases.js through today's server, with the released engine program
// (the engine verifier, server/minecraft-verify.js, is today's default; the
// program comes from MCBENCH_TOOLS_DIR).
//
//   MCBENCH_TOOLS_DIR=<tools copy> node verify-node.js <checkout> <out folder> [first case] [last case]
//
// A call case runs as the agent runs a test_build call
// (agent.js executeTestBuild on minecraftVerifier, its tests on the pool's
// lanes); a grade case as a benchmark grader runs one (an EngineVerifier with
// the case's budgets, encoding and placement mode, run(world, test, {
// reportPath })). For case n it writes <out>/node/<n>.json (the result as
// JSON.stringify writes it; for a grade, {ok, result} or {ok, error, code,
// budget}) and the report files the result names, as <out>/node/<n>/<i>.json.

const fs = require('fs');
const os = require('os');
const path = require('path');

const [root, out, first = '0', last = 'Infinity'] = process.argv.slice(2);
if (!out) throw new Error('usage: node verify-node.js <checkout> <out folder> [first] [last]');
const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-verify-node-'));
process.env.MCBENCH_DATA_DIR = path.join(scratch, 'data');
const server = (name) => require(path.join(root, 'server', name));
const { World } = server('world');
const sandbox = server('sandbox');
const { minecraftVerifier, EngineVerifier } = server('minecraft-verify');
const { BUDGETS } = server('verification-contract');
const { _internal: { executeTestBuild } } = server('agent');

const load = ({ manifest, chunks }) => World.fromManifest(JSON.parse(fs.readFileSync(manifest, 'utf8')), (hash) => JSON.parse(fs.readFileSync(path.join(chunks, `${hash}.json`), 'utf8')), new Map());

async function main() {
  const cases = fs.readFileSync(path.join(out, 'cases.jsonl'), 'utf8').split('\n').filter(Boolean).map((line) => JSON.parse(line));
  const folder = path.join(out, 'node');
  fs.mkdirSync(folder, { recursive: true });
  const grader = new EngineVerifier({ root: path.join(scratch, 'grader') });
  for (let n = Number(first); n < Math.min(cases.length, Number(last) + 1); n++) {
    const c = cases[n];
    const files = path.join(folder, String(n));
    fs.rmSync(files, { recursive: true, force: true });
    fs.mkdirSync(files, { recursive: true });
    const world = load(c.world);
    const started = Date.now();
    let text;
    if (c.kind === 'call') {
      minecraftVerifier.placementMode = c.placementMode;
      const chat = `diff-${n}`;
      const result = await executeTestBuild({ id: chat, world }, c.args, undefined, minecraftVerifier, () => {});
      text = JSON.stringify(result);
      (result.tests || []).forEach((test, i) => {
        if (test.report) fs.copyFileSync(sandbox.resolveWorkPath(chat, test.report.slice('/work/'.length)), path.join(files, `${i}.json`));
      });
      fs.rmSync(path.join(scratch, 'data', 'sandbox'), { recursive: true, force: true });
    } else {
      grader.budgets = c.budgets ?? BUDGETS;
      grader.reportEncoding = c.reportEncoding;
      grader.placementMode = c.placementMode;
      try {
        const result = await grader.run(world, c.test, { reportPath: path.join(files, '0.json') });
        text = JSON.stringify({ ok: true, result });
      } catch (error) {
        text = JSON.stringify({ ok: false, error: error.message, ...(error.budget ? { code: error.code, budget: error.budget } : {}) });
      }
    }
    fs.writeFileSync(path.join(folder, `${n}.json`), text);
    console.log(`${n} ${c.name} ${Date.now() - started} ms`);
  }
  await grader.stop();
  await minecraftVerifier.stop();
  fs.rmSync(scratch, { recursive: true, force: true });
}

main().then(() => process.exit(0), (error) => { console.error(error); process.exit(1); });
