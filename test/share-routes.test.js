'use strict';

// The share routes of server/index.js, driven through handleApi: the page's
// address, a publish streamed as NDJSON with the refusals before it starts,
// and a removal. The remote is a local bare repository; `gh`, the fetch of
// the public build.json and the export are fakes.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { EventEmitter } = require('node:events');
const { Readable } = require('node:stream');
const { execFile, execFileSync } = require('node:child_process');
const { promisify } = require('node:util');
const { after, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-share-routes-'));
process.env.MCBENCH_DATA_DIR = path.join(dataDir, 'data');
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'data', 'settings.json');
fs.writeFileSync(path.join(dataDir, 'gitconfig'), '');
Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: path.join(dataDir, 'gitconfig'),
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Test',
  GIT_AUTHOR_EMAIL: 'test@example.invalid',
  GIT_COMMITTER_NAME: 'Test',
  GIT_COMMITTER_EMAIL: 'test@example.invalid',
});
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const store = require('../server/store');
const settings = require('../server/settings');
const { Edit } = require('../server/turn-stream');
const { shareId } = require('../server/share');
const app = require('../server/index');

after(() => fs.rmSync(dataDir, { recursive: true, force: true }));

const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const bare = path.join(dataDir, 'remote.git');
git('init', '--quiet', '--bare', '-b', 'main', bare);
git('clone', '--quiet', bare, `${bare}-seed`);
fs.writeFileSync(path.join(`${bare}-seed`, 'README.md'), 'shared builds\n');
git('-C', `${bare}-seed`, 'add', '-A');
git('-C', `${bare}-seed`, 'commit', '--quiet', '-m', 'init');
git('-C', `${bare}-seed`, 'push', '--quiet', 'origin', 'HEAD:main');
const listed = () => git('--git-dir', bare, 'ls-tree', '-r', '--name-only', 'main').split('\n').filter(Boolean);

const { shares } = app;
shares.remote = () => bare;
shares.run = async (command, args, options) => {
  if (command !== 'gh') return promisify(execFile)(command, args, options);
  const pages = { html_url: 'https://owner.github.io/builds/', build_type: 'legacy', source: { branch: 'main', path: '/' } };
  return { stdout: args[1] === 'user' ? 'owner\n42+owner@users.noreply.github.com\n' : JSON.stringify(pages), stderr: '' };
};
shares.fetch = async (address) => {
  const file = new URL(address).pathname.split('/').slice(2).join('/');
  return { ok: true, json: async () => JSON.parse(execFileSync('git', ['--git-dir', bare, 'show', `main:${file}`])) };
};
shares.exportBuild = async () => ({
  blueprint: Buffer.from('blueprint'),
  mask: { minX: -64, maxX: 63, minZ: -64, maxZ: 63 },
  bounds: { min: [0, 64, 0], max: [0, 64, 0] },
  preview: Buffer.from('jpeg'),
  webRoot: '/baked/web',
  mesher: '/mesher.wasm',
});
shares.montage = async (tiles, cells) => Buffer.from(JSON.stringify(cells));
shares.viewer = { writeViewer: () => 'v1', sharePageHtml: () => '<html>' };

function request(method, pathname) {
  const req = Readable.from([]);
  Object.assign(req, { method, url: pathname, headers: {} });
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status, headers) => { res.status = status; res.headers = headers; res.headersSent = true; };
  res.write = (line) => { res.lines.push(JSON.parse(line)); return true; };
  res.end = (payload) => {
    if (payload) res.body = JSON.parse(payload);
    res.writableEnded = true;
  };
  // A refusal thrown in a route is answered as the server answers it.
  return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).catch((error) => {
    res.status = error.statusCode || 500;
    res.body = { error: error.message };
  }).then(() => res);
}

function setRepo(shareRepo) {
  const current = settings.get();
  settings.update({ providers: current.providers, titleModel: current.titleModel, shareRepo });
}

function newChat(blocks) {
  const conversation = store.createConversation('openrouter', 'model/x');
  if (blocks) conversation.world.set(0, 0, 0, 'minecraft:stone');
  store.save(conversation);
  return conversation.id;
}

test('a publish is refused before it starts without a repository, with an empty build, or under a running turn', async () => {
  const id = newChat(true);
  setRepo(null);
  let res = await request('POST', `/api/conversations/${id}/share`);
  assert.deepEqual([res.status, res.body], [400, { error: 'Set the GitHub repository for shared builds in Settings' }]);
  assert.deepEqual((await request('GET', `/api/conversations/${id}/share`)).body, { url: null });
  setRepo('owner/builds');
  res = await request('POST', `/api/conversations/${newChat(false)}/share`);
  assert.equal(res.status, 400);
  assert.match(res.body.error, /empty/);
  const [hold] = app.reservations.take([id], () => new Edit(), 'busy');
  res = await request('POST', `/api/conversations/${id}/share`);
  app.reservations.release(id, hold);
  assert.equal(res.status, 409);
  res = await request('POST', '/api/conversations/missing/share');
  assert.equal(res.status, 404);
});

test('a publish streams its states, the chat then has its address, and a removal takes it off the site', async () => {
  setRepo('owner/builds');
  const id = newChat(true);
  const url = `https://owner.github.io/builds/b/${shareId(id)}/`;
  const res = await request('POST', `/api/conversations/${id}/share`);
  assert.equal(res.status, 200);
  assert.equal(res.headers['content-type'], 'application/x-ndjson');
  assert.deepEqual(res.lines, [{ state: 'exporting' }, { state: 'publishing' }, { state: 'waiting' }, { state: 'live', url }]);
  assert.equal(res.writableEnded, true);
  assert.deepEqual((await request('GET', `/api/conversations/${id}/share`)).body, { url });
  assert.ok(listed().includes(`b/${shareId(id)}/build.json`));

  const removed = await request('DELETE', `/api/conversations/${id}/share`);
  assert.deepEqual([removed.status, removed.body], [200, {}]);
  assert.ok(!listed().some((file) => file.startsWith(`b/${shareId(id)}/`)));
  assert.deepEqual((await request('GET', `/api/conversations/${id}/share`)).body, { url: null });
  assert.equal((await request('DELETE', `/api/conversations/${id}/share`)).status, 404);
});

test('a second publish of a chat that is publishing is refused 409 as JSON', async () => {
  setRepo('owner/builds');
  const id = newChat(true);
  let release;
  const exported = shares.exportBuild;
  shares.exportBuild = async (conversation) => { await new Promise((resolve) => { release = resolve; }); return exported(conversation); };
  const first = request('POST', `/api/conversations/${id}/share`);
  await new Promise(setImmediate);
  const second = await request('POST', `/api/conversations/${id}/share`);
  assert.deepEqual([second.status, second.body], [409, { error: 'This build is already being published' }]);
  release();
  shares.exportBuild = exported;
  assert.equal((await first).lines.at(-1).state, 'live');
});

test('the shared builds are listed with the chat each came from, and any of them is removed by its share id', async () => {
  setRepo(null);
  assert.deepEqual((await request('GET', '/api/shares')).body, { repo: null, builds: [] });
  assert.deepEqual((await request('DELETE', '/api/shares/abcdefabcdef')).body, { error: 'Set the GitHub repository for shared builds in Settings' });
  assert.equal((await request('DELETE', '/api/shares/abcdefabcdef')).status, 400);

  setRepo('owner/builds');
  const kept = newChat(true);
  const archived = newChat(true);
  const gone = newChat(true);
  for (const id of [gone, archived, kept]) assert.equal((await request('POST', `/api/conversations/${id}/share`)).lines.at(-1).state, 'live');
  store.archiveConversation(archived);
  store.archiveConversation(gone);
  store.deleteConversation(gone);

  const res = await request('GET', '/api/shares');
  assert.equal(res.status, 200);
  assert.equal(res.body.repo, 'owner/builds');
  const ours = res.body.builds.filter((build) => [kept, archived, gone].map(shareId).includes(build.shareId));
  assert.deepEqual(ours.map(({ publishedAt, ...build }) => build), [
    { shareId: shareId(kept), title: 'New build', kind: 'chat', labels: ['model/x'], url: `https://owner.github.io/builds/b/${shareId(kept)}/`, conversationId: kept, benchId: null, archived: false },
    { shareId: shareId(archived), title: 'New build', kind: 'chat', labels: ['model/x'], url: `https://owner.github.io/builds/b/${shareId(archived)}/`, conversationId: archived, benchId: null, archived: true },
    { shareId: shareId(gone), title: 'New build', kind: 'chat', labels: ['model/x'], url: `https://owner.github.io/builds/b/${shareId(gone)}/`, conversationId: null, benchId: null },
  ]);
  assert.ok(ours.every((build) => !Number.isNaN(Date.parse(build.publishedAt))));

  const removed = await request('DELETE', `/api/shares/${shareId(gone)}`);
  assert.deepEqual([removed.status, removed.body], [200, { ok: true }]);
  assert.ok(!listed().some((file) => file.startsWith(`b/${shareId(gone)}/`)));
  assert.ok(!(await request('GET', '/api/shares')).body.builds.some((build) => build.shareId === shareId(gone)));
  const again = await request('DELETE', `/api/shares/${shareId(gone)}`);
  assert.deepEqual([again.status, again.body], [404, { error: 'This build is not shared' }]);
  for (const malformed of ['ABCDEFABCDEF', 'abcdef', 'abcdefabcdef0', 'abcdef.bcdef', '..abcdefabcd', 'abcdefabcde%2F']) {
    const refused = await request('DELETE', `/api/shares/${malformed}`);
    assert.deepEqual([refused.status, refused.body], [404, { error: 'This build is not shared' }], malformed);
  }
});

test('a removal of a build that is publishing is refused 409, and a git failure is answered with its message', async () => {
  setRepo('owner/builds');
  const id = newChat(true);
  let release;
  const exported = shares.exportBuild;
  shares.exportBuild = async (conversation) => { await new Promise((resolve) => { release = resolve; }); return exported(conversation); };
  const publishing = request('POST', `/api/conversations/${id}/share`);
  await new Promise(setImmediate);
  const refused = await request('DELETE', `/api/shares/${shareId(id)}`);
  assert.deepEqual([refused.status, refused.body], [409, { error: 'Wait for the build to finish publishing' }]);
  release();
  shares.exportBuild = exported;
  assert.equal((await publishing).lines.at(-1).state, 'live');

  shares.remote = () => path.join(dataDir, 'missing.git');
  for (const [method, route] of [['GET', '/api/shares'], ['DELETE', `/api/shares/${shareId(id)}`]]) {
    const failed = await request(method, route);
    assert.equal(failed.status, 500);
    assert.match(failed.body.error, /^Could not get a copy of owner\/builds from GitHub: .*missing\.git/s);
  }
  shares.remote = () => bare;
  assert.deepEqual((await request('DELETE', `/api/shares/${shareId(id)}`)).body, { ok: true });
});

// A bench of three models, of which those in `built` placed a block.
function newBench(built, { blind = false } = {}) {
  const { bench, members } = store.createBench(crypto.randomUUID(), {
    title: 'A bridge',
    blind,
    resolution: 1,
    content: 'Build a bridge',
    members: ['a', 'b', 'c'].map((name) => ({ provider: 'openrouter', model: `model/${name}` })),
  });
  for (const member of members) {
    if (!built.includes(member.model.slice(-1))) continue;
    member.world.set(0, 0, 0, 'minecraft:stone');
    store.save(member);
  }
  return { id: bench.id, order: bench.order, members };
}

test('a bench publish is refused without a repository, with nothing built, or under a running turn', async () => {
  const { id, members } = newBench(['a']);
  setRepo(null);
  let res = await request('POST', `/api/benches/${id}/share`);
  assert.deepEqual([res.status, res.body], [400, { error: 'Set the GitHub repository for shared builds in Settings' }]);
  assert.deepEqual((await request('GET', `/api/benches/${id}/share`)).body, { url: null });
  assert.deepEqual((await request('DELETE', `/api/benches/${id}/share`)).status, 404);
  setRepo('owner/builds');
  res = await request('POST', `/api/benches/${newBench([]).id}/share`);
  assert.deepEqual([res.status, res.body], [400, { error: 'No model in this bench has built anything yet' }]);
  const [hold] = app.reservations.take([members[1].id], () => new Edit(), 'busy');
  res = await request('POST', `/api/benches/${id}/share`);
  app.reservations.release(members[1].id, hold);
  assert.deepEqual([res.status, res.body], [409, { error: 'Wait for the bench\'s running turns before sharing' }]);
  assert.equal((await request('POST', '/api/benches/missing/share')).status, 404);
});

test('a bench publish streams its states and puts the models that built something on one page, which a removal takes off', async () => {
  setRepo('owner/builds');
  const { id, order, members } = newBench(['a', 'c'], { blind: true });
  const url = `https://owner.github.io/builds/b/${shareId(id)}/`;
  const res = await request('POST', `/api/benches/${id}/share`);
  assert.equal(res.headers['content-type'], 'application/x-ndjson');
  assert.deepEqual(res.lines, [{ state: 'exporting' }, { state: 'publishing' }, { state: 'waiting' }, { state: 'live', url }]);
  assert.deepEqual((await request('GET', `/api/benches/${id}/share`)).body, { url });
  const build = JSON.parse(execFileSync('git', ['--git-dir', bare, 'show', `main:b/${shareId(id)}/build.json`]));
  const letter = (model) => String.fromCharCode(65 + order.indexOf(members.find((member) => member.model === model).id));
  const shownModels = order.map((memberId) => members.find((member) => member.id === memberId).model).filter((model) => model !== 'model/b');
  assert.deepEqual([build.kind, build.builds.map((entry) => entry.label)], ['bench', shownModels.map(letter)]);
  assert.deepEqual(build.builds.map((entry) => entry.blueprint), ['0.bin.gz', '1.bin.gz']);

  // A second publish while one runs is refused.
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const exported = shares.exportBuild;
  shares.exportBuild = async (...args) => { await gate; return exported(...args); };
  const first = request('POST', `/api/benches/${id}/share`);
  await new Promise(setImmediate);
  const second = await request('POST', `/api/benches/${id}/share`);
  assert.deepEqual([second.status, second.body], [409, { error: 'This build is already being published' }]);
  release();
  shares.exportBuild = exported;
  assert.equal((await first).lines.at(-1).state, 'live');

  const removed = await request('DELETE', `/api/benches/${id}/share`);
  assert.deepEqual([removed.status, removed.body], [200, {}]);
  assert.ok(!listed().some((file) => file.startsWith(`b/${shareId(id)}/`)));
  assert.deepEqual((await request('GET', `/api/benches/${id}/share`)).body, { url: null });
  assert.equal((await request('DELETE', `/api/benches/${id}/share`)).status, 404);
});

test('the shares list a bench with its id, an archived bench as archived, and a bench deleted for good with none, which is still removable', async () => {
  setRepo('owner/builds');
  const kept = newBench(['a', 'b', 'c']);
  const archived = newBench(['a']);
  const gone = newBench(['b']);
  const chat = newChat(true);
  for (const id of [gone.id, archived.id, kept.id]) assert.equal((await request('POST', `/api/benches/${id}/share`)).lines.at(-1).state, 'live');
  assert.equal((await request('POST', `/api/conversations/${chat}/share`)).lines.at(-1).state, 'live');
  store.archiveBench(archived.id);
  store.archiveBench(gone.id);
  store.deleteBench(gone.id);

  const { body } = await request('GET', '/api/shares');
  const ids = [chat, kept.id, archived.id, gone.id].map(shareId);
  const rows = body.builds.filter((build) => ids.includes(build.shareId)).map(({ publishedAt, ...build }) => build);
  const page = (id) => `https://owner.github.io/builds/b/${shareId(id)}/`;
  assert.deepEqual(rows, [
    { shareId: shareId(chat), title: 'New build', kind: 'chat', labels: ['model/x'], url: page(chat), conversationId: chat, benchId: null, archived: false },
    { shareId: shareId(kept.id), title: 'A bridge', kind: 'bench', labels: ['model/a', 'model/b', 'model/c'], url: page(kept.id), conversationId: null, benchId: kept.id, archived: false },
    { shareId: shareId(archived.id), title: 'A bridge', kind: 'bench', labels: ['model/a'], url: page(archived.id), conversationId: null, benchId: archived.id, archived: true },
    { shareId: shareId(gone.id), title: 'A bridge', kind: 'bench', labels: ['model/b'], url: page(gone.id), conversationId: null, benchId: null },
  ]);
  assert.deepEqual((await request('DELETE', `/api/shares/${shareId(gone.id)}`)).body, { ok: true });
  assert.ok(!listed().some((file) => file.startsWith(`b/${shareId(gone.id)}/`)));
});
