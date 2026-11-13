'use strict';

// What the first start prepares (server/setup.js), as GET /api/setup tells
// the page: one step per thing, each marked by the work that does it.
const { test, beforeEach } = require('node:test');
const assert = require('node:assert/strict');

const setup = require('../server/setup');

beforeEach(() => setup.reset());

const step = (id) => setup.view().steps.find((entry) => entry.id === id);

test('every step waits until its work starts, and only the shell image is optional', () => {
  const { ready, steps } = setup.view();
  assert.equal(ready, false);
  assert.deepEqual(steps.map((entry) => entry.id), ['jdk', 'rust', 'minecraft', 'tables', 'blocks', 'viewer', 'engine', 'wasm', 'browser', 'shell']);
  assert.ok(steps.every((entry) => entry.state === 'waiting' && typeof entry.title === 'string' && typeof entry.doing === 'string'));
  assert.deepEqual(steps.filter((entry) => entry.optional).map((entry) => entry.id), ['shell']);
});

test('a step runs with its progress, then is done with how long it took; its work is called at once', async () => {
  let release;
  let called = false;
  const done = setup.track('jdk', () => {
    called = true;
    return new Promise((resolve) => { release = resolve; });
  });
  assert.ok(called, 'called before track returned');
  setup.progress('jdk')({ done: 10, total: 100 });
  setup.report('jdk', { detail: 'Downloading' });
  assert.deepEqual(step('jdk'), { id: 'jdk', title: 'Java', doing: 'Downloading Java', optional: false, state: 'running', detail: 'Downloading', progress: { done: 10, total: 100 } });
  release('the result');
  assert.equal(await done, 'the result');
  const finished = step('jdk');
  assert.equal(finished.state, 'done');
  assert.equal(typeof finished.seconds, 'number');
  assert.equal(finished.progress, undefined);
  assert.equal(finished.detail, undefined);
});

test('a failed step says why and runs again when it is next needed; a done step stays done', async () => {
  await assert.rejects(setup.track('rust', async () => { throw Object.assign(new Error('rustup failed'), { stderr: 'error: no network\n' }); }), /rustup failed/);
  assert.deepEqual([step('rust').state, step('rust').error], ['failed', 'error: no network']);
  await setup.track('rust', async () => {});
  assert.equal(step('rust').state, 'done');
  const { seconds } = step('rust');
  await assert.rejects(setup.track('rust', async () => { throw new Error('later'); }), /later/);
  assert.deepEqual([step('rust').state, step('rust').seconds], ['done', seconds]);
});

test('a failure about the machine makes a step unavailable, and the optional shell image does not keep the app from ready', async () => {
  for (const { id } of setup.STEPS.filter((entry) => !entry.optional)) await setup.track(id, () => {});
  await assert.rejects(setup.track('shell', async () => { throw new Error('Docker is not installed'); }, { unavailable: () => true }));
  assert.deepEqual([step('shell').state, step('shell').error], ['unavailable', 'Docker is not installed']);
  assert.equal(setup.view().ready, true);
});

test('ready waits for every step that is not optional', async () => {
  for (const { id } of setup.STEPS.filter((entry) => !entry.optional && entry.id !== 'browser')) await setup.track(id, () => {});
  assert.equal(setup.view().ready, false);
  await setup.track('browser', () => {});
  assert.equal(setup.view().ready, true);
});

test('an unknown step is a mistake in the code', () => {
  assert.throws(() => setup.track('nothing', () => {}), /No setup step nothing/);
});

// A failed step tried again (POST /api/setup/retry): the work the server
// started with runs again, so the failed download runs, and the step that
// waited on it after it. The download is from a server on loopback.
test('retry runs the failed step again, and the step that waited on it; with nothing failed it is refused', async (t) => {
  const http = require('node:http');
  const crypto = require('node:crypto');
  const fs = require('node:fs');
  const os = require('node:os');
  const path = require('node:path');
  const tools = require('../server/tools');
  const bytes = Buffer.from('the headless shell');
  let serving = false;
  const server = http.createServer((req, res) => (serving ? res.writeHead(200).end(bytes) : res.writeHead(404).end()));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const folder = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-setup-retry-'));
  t.after(() => { server.close(); fs.rmSync(folder, { recursive: true, force: true }); });
  const target = path.join(folder, 'shell');
  let runs = 0;
  let finished;
  setup.start(() => {
    runs += 1;
    finished = setup.track('browser', () => tools.download(`http://127.0.0.1:${server.address().port}/shell`, target,
      { sha256: crypto.createHash('sha256').update(bytes).digest('hex') }))
      .then(() => setup.track('engine', () => {}))
      .catch(() => {});
  });
  await finished;
  assert.deepEqual([step('browser').state, step('engine').state], ['failed', 'waiting']);
  assert.match(step('browser').error, /HTTP 404/);

  serving = true;
  assert.equal(setup.retry().steps.find((entry) => entry.id === 'browser').state, 'running');
  await finished;
  assert.deepEqual([runs, step('browser').state, step('engine').state], [2, 'done', 'done']);
  assert.ok(fs.readFileSync(target).equals(bytes));
  assert.throws(() => setup.retry(), { statusCode: 409, message: 'No setup step failed' });
  assert.throws(() => setup.start(() => {}), /started already/);
});
