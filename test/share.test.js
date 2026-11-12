'use strict';

// Shared builds (server/share.js): a chat's build published to a GitHub
// Pages repository's working tree and pushed. The remote is a local bare
// repository, `gh` and the fetch of the public build.json are fakes, and the
// export (the settle and the preview shot) and the viewer's files are given.

const assert = require('node:assert/strict');
const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const zlib = require('node:zlib');
const { execFile, execFileSync } = require('node:child_process');
const { promisify } = require('node:util');
const { after, test } = require('node:test');

const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-share-test-'));
process.env.MCBENCH_DATA_DIR = path.join(scratch, 'data');
process.env.MCBENCH_SETTINGS_FILE = path.join(scratch, 'data', 'settings.json');
// git as it is on a machine with no configuration of its own: no signing,
// no hooks of the developer's.
fs.writeFileSync(path.join(scratch, 'gitconfig'), '');
Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: path.join(scratch, 'gitconfig'),
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Test',
  GIT_AUTHOR_EMAIL: 'test@example.invalid',
  GIT_COMMITTER_NAME: 'Test',
  GIT_COMMITTER_EMAIL: 'test@example.invalid',
});
after(() => fs.rmSync(scratch, { recursive: true, force: true }));

const { Shares, Publish, shareId, benchLabel } = require('../server/share');
const { montageCells } = require('../server/shot');

const execFileAsync = promisify(execFile);
const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

// A bare repository with one commit on main, as a repository GitHub Pages
// serves from has.
let remotes = 0;
function remote() {
  const bare = path.join(scratch, `remote-${remotes += 1}.git`);
  git('init', '--quiet', '--bare', '-b', 'main', bare);
  const seed = `${bare}-seed`;
  git('clone', '--quiet', bare, seed);
  fs.writeFileSync(path.join(seed, 'README.md'), 'shared builds\n');
  git('-C', seed, 'add', '-A');
  git('-C', seed, 'commit', '--quiet', '-m', 'init');
  git('-C', seed, 'push', '--quiet', 'origin', 'HEAD:main');
  return bare;
}
const shown = (bare, file) => execFileSync('git', ['--git-dir', bare, 'show', `main:${file}`]);
const listed = (bare) => git('--git-dir', bare, 'ls-tree', '-r', '--name-only', 'main').split('\n').filter(Boolean);

const BUILD = {
  blueprint: Buffer.from('blueprint bytes'),
  mask: { minX: -64, maxX: 63, minZ: -64, maxZ: 63 },
  bounds: { min: [-3, 64, 2], max: [4, 90, 9] },
  preview: Buffer.from('jpeg'),
  webRoot: '/baked/web',
  mesher: '/mesher.wasm',
};

// Shares on a fresh data folder: repositories named `owner/<n>` are the bare
// remotes in `repos`; Pages serves what a remote holds after `deployAt`
// fetches of build.json, from `pagesSource` of a site built by `buildType`.
// `step(command, args, options)`, when it gives anything, runs in place of
// the command (gh or git) it is called for.
function harness({ exportBuild = async () => BUILD, deployAt = 1, pagesError = null, pagesSource = { branch: 'main', path: '/' }, buildType = 'legacy', waitMs = 2000, step = () => undefined, networkMs, localMs } = {}) {
  const root = fs.mkdtempSync(path.join(scratch, 'share-'));
  const repos = new Map();
  const commands = [];
  const viewerCalls = [];
  let fetches = 0;
  const shares = new Shares({
    root,
    remote: (repo) => repos.get(repo),
    run: async (command, args, options) => {
      commands.push([command, ...args].join(' '));
      // Every command can be stopped at its time limit; git never prompts.
      assert.ok(options.signal instanceof AbortSignal);
      if (command === 'git') assert.deepEqual([options.env.GIT_TERMINAL_PROMPT, options.env.GCM_INTERACTIVE, options.env.GIT_ASKPASS], ['0', 'never', '']);
      const instead = step(command, args, options);
      if (instead) return instead;
      if (command === 'gh' && args[1] === 'user') {
        assert.deepEqual(args, ['api', 'user', '--jq', '.login, "\\(.id)+\\(.login)@users.noreply.github.com"']);
        return { stdout: 'owner\n42+owner@users.noreply.github.com\n', stderr: '' };
      }
      if (command === 'gh') {
        assert.match(args[1], /^repos\/owner\/[^/]+\/pages$/);
        assert.equal(args.length, 2);
        if (pagesError) throw Object.assign(new Error('gh failed'), { stderr: pagesError });
        const html_url = `https://owner.github.io/${args[1].split('/').at(-2)}/`;
        return { stdout: JSON.stringify({ html_url, build_type: buildType, source: pagesSource }), stderr: '' };
      }
      assert.equal(command, 'git');
      return execFileAsync(command, args, options);
    },
    fetch: async (address, options) => {
      assert.equal(options.cache, 'no-store');
      fetches += 1;
      const { pathname } = new URL(address);
      const [, name, ...rest] = pathname.split('/');
      if (fetches <= deployAt) return { ok: false, status: 404 };
      const bare = repos.get(`owner/${name}`);
      return { ok: true, status: 200, json: async () => JSON.parse(shown(bare, rest.join('/'))) };
    },
    exportBuild,
    // The card of a bench: the cells and the shots, as text.
    montage: async (tiles, cells, size) => Buffer.from(JSON.stringify({ tiles: tiles.map(String), cells, size })),
    viewer: {
      writeViewer(siteDir, options) {
        viewerCalls.push(options);
        fs.mkdirSync(path.join(siteDir, 'viewer', 'v1'), { recursive: true });
        fs.writeFileSync(path.join(siteDir, 'viewer', 'v1', 'viewer.js'), 'viewer');
        return 'v1';
      },
      sharePageHtml: (page) => `<html>${JSON.stringify(page)}</html>`,
    },
    waitMs,
    pollMs: 5,
    networkMs,
    localMs,
  });
  const repo = (name) => {
    repos.set(`owner/${name}`, remote());
    return `owner/${name}`;
  };
  return { shares, root, repos, repo, commands, viewerCalls, fetches: () => fetches };
}

const chat = (id = crypto.randomUUID()) => ({ id, title: 'A tower', model: 'model/x' });

// Publishes the chat share of `conversation`, or the bench share of `bench`
// and its `members`, and gives the states it moved through.
async function publish(shares, { repo, conversation, bench, members }) {
  const states = [];
  await shares.publish({
    repo,
    sourceId: (conversation ?? bench).id,
    build: conversation ? () => shares.chatShare(conversation) : () => shares.benchShare(bench, members),
    onState: (event) => states.push(event),
  });
  return states;
}

test('a share id is the first 12 hex digits of the sha256 of the chat id', () => {
  assert.equal(shareId('abc'), crypto.createHash('sha256').update('abc').digest('hex').slice(0, 12));
});

test('a publish moves only along exporting, publishing, waiting, live, or to failed from the first three', () => {
  const events = [];
  const move = new Publish((event) => events.push(event));
  assert.equal(move.state, 'exporting');
  assert.throws(() => move.to('waiting'), /exporting.*cannot become waiting/);
  move.to('publishing');
  move.to('waiting');
  move.to('live', { url: 'u' });
  assert.throws(() => move.to('failed', { error: 'x' }), /live.*cannot become failed/);
  assert.deepEqual(events, [{ state: 'exporting' }, { state: 'publishing' }, { state: 'waiting' }, { state: 'live', url: 'u' }]);
  for (const from of [[], ['publishing'], ['publishing', 'waiting']]) {
    const other = new Publish(() => {});
    for (const state of from) other.to(state);
    other.to('failed', { error: 'x' });
    assert.throws(() => other.to('live', { url: 'u' }), /failed.*cannot become live/);
  }
});

test('a publish pushes the page, its build and the viewer, and is live once Pages serves its version', async () => {
  const { shares, repos, repo, root, viewerCalls, fetches } = harness();
  const name = repo('builds');
  const conversation = chat();
  const id = shareId(conversation.id);
  assert.equal(await shares.url(name, shareId(conversation.id)), null);
  const states = await publish(shares, { repo: name, conversation });
  const url = `https://owner.github.io/builds/b/${id}/`;
  assert.deepEqual(states, [{ state: 'exporting' }, { state: 'publishing' }, { state: 'waiting' }, { state: 'live', url }]);
  assert.equal(fetches(), 2, 'polled until the pushed version was served');

  const bare = repos.get(name);
  assert.deepEqual(listed(bare).sort(), ['.nojekyll', 'README.md', `b/${id}/0.bin.gz`, `b/${id}/build.json`, `b/${id}/index.html`, `b/${id}/preview.jpg`, 'viewer/v1/viewer.js']);
  const build = JSON.parse(shown(bare, `b/${id}/build.json`));
  assert.match(build.version, /^[0-9a-f-]{36}$/);
  assert.deepEqual(build, { title: 'A tower', kind: 'chat', version: build.version, url, builds: [{ label: 'model/x', blueprint: '0.bin.gz', mask: BUILD.mask, bounds: BUILD.bounds }] });
  assert.deepEqual(zlib.gunzipSync(shown(bare, `b/${id}/0.bin.gz`)), BUILD.blueprint);
  assert.deepEqual(shown(bare, `b/${id}/preview.jpg`), BUILD.preview);
  assert.deepEqual(viewerCalls, [{ webRoot: '/baked/web', mesher: '/mesher.wasm' }]);
  const page = JSON.parse(shown(bare, `b/${id}/index.html`).toString().slice(6, -7));
  assert.deepEqual(page, { viewerId: 'v1', shareId: id, title: 'A tower', description: 'Built by model/x in mcbench-lite. Drag to look around.', url, image: `${url}preview.jpg`, labels: ['model/x'] });
  assert.equal(await shares.url(name, shareId(conversation.id)), url);
  assert.ok(fs.existsSync(path.join(root, 'site', '.git')));
  // The commit is the signed-in GitHub account's, at its noreply address.
  assert.equal(git('--git-dir', bare, 'log', '-1', '--format=%an <%ae> %cn <%ce> %s', 'main').trim(),
    'owner <42+owner@users.noreply.github.com> owner <42+owner@users.noreply.github.com> Share ' + `${id}: A tower`);

  // Published again, it has a new version.
  await publish(shares, { repo: name, conversation });
  assert.notEqual(JSON.parse(shown(bare, `b/${id}/build.json`)).version, build.version);
});

test('a second publish of a chat that is publishing is refused 409, and publishes of two chats change the tree one at a time', async () => {
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  const { shares, repo, commands } = harness({ exportBuild: async () => { await held; return BUILD; }, deployAt: 0 });
  const name = repo('builds');
  const first = chat();
  const second = chat();
  const running = publish(shares, { repo: name, conversation: first });
  assert.equal(shares.publishing(shareId(first.id)), true);
  assert.throws(() => shares.publish({ repo: name, sourceId: first.id, build: () => shares.chatShare(first), onState: () => {} }), (error) => error.statusCode === 409);
  await assert.rejects(shares.remove(name, shareId(first.id)), (error) => error.statusCode === 409);
  const other = publish(shares, { repo: name, conversation: second });
  release();
  const [a, b] = await Promise.all([running, other]);
  assert.equal(a.at(-1).state, 'live');
  assert.equal(b.at(-1).state, 'live');
  assert.equal(shares.publishing(shareId(first.id)), false);
  // Each publish's git commands, clone or fetch to push, run with no other's between.
  const tree = commands.filter((line) => /^git (clone|fetch|reset|add|commit|push)/.test(line)).map((line) => line.split(' ')[1]);
  assert.deepEqual(tree, ['clone', 'add', 'commit', 'push', 'fetch', 'reset', 'add', 'commit', 'push']);
});

test('a repository without GitHub Pages fails the publish and says so, pushing nothing', async () => {
  const { shares, repos, repo } = harness({ pagesError: 'gh: Not Found (HTTP 404)' });
  const name = repo('builds');
  const states = await publish(shares, { repo: name, conversation: chat() });
  assert.deepEqual(states.map((event) => event.state), ['exporting', 'publishing', 'failed']);
  assert.match(states.at(-1).error, /GitHub Pages is not set up for owner\/builds.*Not Found.*Settings → Pages, set Source to "Deploy from a branch", branch main, folder \/ \(root\)/);
  assert.deepEqual(listed(repos.get(name)), ['README.md']);
});

test('a site Pages serves from another branch or folder fails before pushing, saying what to set; one a workflow deploys is published', async () => {
  for (const source of [{ branch: 'gh-pages', path: '/' }, { branch: 'main', path: '/docs' }]) {
    const { shares, repos, repo, root } = harness({ pagesSource: source });
    const name = repo('builds');
    const states = await publish(shares, { repo: name, conversation: chat() });
    assert.deepEqual(states.map((event) => event.state), ['exporting', 'publishing', 'failed']);
    assert.equal(states.at(-1).error, `GitHub Pages for owner/builds serves the ${source.branch} branch's ${source.path} folder, but builds are pushed to the root of main: in the repository's Settings → Pages, set Source to "Deploy from a branch", branch main, folder / (root)`);
    assert.deepEqual(listed(repos.get(name)), ['README.md']);
    assert.ok(fs.existsSync(path.join(root, 'site', '.git')), 'nothing was written, so the copy is kept');
  }
  const { shares, repo } = harness({ deployAt: 0, buildType: 'workflow', pagesSource: { branch: 'gh-pages', path: '/' } });
  assert.equal((await publish(shares, { repo: repo('builds'), conversation: chat() })).at(-1).state, 'live');
});

test('gh not installed, or not signed in, fails the publish saying which', async () => {
  const cases = [
    [Object.assign(new Error('spawn gh ENOENT'), { code: 'ENOENT' }), 'The GitHub CLI (gh) is not installed: install it from https://cli.github.com and run `gh auth login`'],
    [Object.assign(new Error('gh failed'), { code: 4, stderr: 'To get started with GitHub CLI, please run:  gh auth login\n' }), /^The GitHub CLI is not signed in to GitHub: run `gh auth login`/],
    [Object.assign(new Error('gh failed'), { code: 1, stderr: 'gh: Bad credentials (HTTP 401)\n' }), /^The GitHub CLI is not signed in to GitHub: run `gh auth login`.*HTTP 401/],
  ];
  for (const [error, message] of cases) {
    const { shares, repo } = harness({ step: (command) => (command === 'gh' ? Promise.reject(error) : undefined) });
    const states = await publish(shares, { repo: repo('builds'), conversation: chat() });
    assert.deepEqual(states.map((event) => event.state), ['exporting', 'publishing', 'failed']);
    if (typeof message === 'string') assert.equal(states.at(-1).error, message);
    else assert.match(states.at(-1).error, message);
  }
});

// A command that runs until its signal stops it, as execFile's does.
const hang = (options) => new Promise((resolve, reject) => {
  options.signal.addEventListener('abort', () => reject(Object.assign(new Error('The operation was aborted'), { code: 'ABORT_ERR' })));
});

test('a git step that does not finish within its time limit fails, naming the step, and the jobs after it run', async () => {
  let stuck = true;
  const { shares, repo, root } = harness({
    deployAt: 0,
    networkMs: 1500,
    step: (command, args, options) => (stuck && command === 'git' && args[0] === 'push' ? hang(options) : undefined),
  });
  const name = repo('builds');
  const conversation = chat();
  const states = await publish(shares, { repo: name, conversation });
  assert.deepEqual(states.map((event) => event.state), ['exporting', 'publishing', 'failed']);
  assert.equal(states.at(-1).error, '`git push` did not finish within 1.5 seconds');
  assert.ok(!fs.existsSync(path.join(root, 'site')), 'a change stopped part-way drops the copy');
  stuck = false;
  assert.equal((await publish(shares, { repo: name, conversation })).at(-1).state, 'live');
  assert.equal((await shares.list(name)).length, 1);
});

test('a git sign-in git cannot make fails saying to run gh auth setup-git', async () => {
  const { shares, repo } = harness({
    step: (command, args) => (command === 'git' && args[0] === 'clone'
      ? Promise.reject(Object.assign(new Error('Command failed'), { code: 128, stderr: "fatal: could not read Username for 'https://github.com': terminal prompts disabled\n" }))
      : undefined),
  });
  const states = await publish(shares, { repo: repo('builds'), conversation: chat() });
  assert.equal(states.at(-1).error, 'Could not get a copy of owner/builds from GitHub: git could not sign in to GitHub over HTTPS (git clone). Run `gh auth setup-git` in a terminal so git signs in with the GitHub CLI\'s account, then try again.');
});

test('a copy that cannot be brought up to date is kept: url() answers from it, and listing fails saying why', async () => {
  let offline = false;
  const { shares, repo, root } = harness({
    deployAt: 0,
    step: (command, args) => (offline && command === 'git' && args[0] === 'fetch'
      ? Promise.reject(Object.assign(new Error('Command failed'), { code: 128, stderr: "fatal: unable to access 'https://github.com/owner/builds.git/': Could not resolve host: github.com\n" }))
      : undefined),
  });
  const name = repo('builds');
  const conversation = chat();
  const id = shareId(conversation.id);
  await publish(shares, { repo: name, conversation });
  offline = true;
  await assert.rejects(shares.list(name), { message: "Could not bring the site's copy up to date with owner/builds on GitHub: fatal: unable to access 'https://github.com/owner/builds.git/': Could not resolve host: github.com" });
  assert.ok(fs.existsSync(path.join(root, 'site', 'b', id, 'build.json')));
  assert.equal(await shares.url(name, id), `https://owner.github.io/builds/b/${id}/`);
  offline = false;
  assert.equal((await shares.list(name)).length, 1);
});

test('an export that fails fails the publish', async () => {
  const { shares, repo } = harness({ exportBuild: async () => { throw new Error('Minecraft keeps none of the blocks'); } });
  const states = await publish(shares, { repo: repo('builds'), conversation: chat() });
  assert.deepEqual(states, [{ state: 'exporting' }, { state: 'failed', error: 'Minecraft keeps none of the blocks' }]);
});

test('a push Pages does not serve in time fails, saying the files are pushed', async () => {
  const { shares, repos, repo } = harness({ deployAt: Infinity, waitMs: 40 });
  const name = repo('builds');
  const conversation = chat();
  const states = await publish(shares, { repo: name, conversation });
  assert.deepEqual(states.map((event) => event.state), ['exporting', 'publishing', 'waiting', 'failed']);
  assert.match(states.at(-1).error, /files are pushed.*GitHub Pages has not served them yet/);
  assert.ok(listed(repos.get(name)).includes(`b/${shareId(conversation.id)}/build.json`));
});

test('a removal deletes the build\'s folder and pushes; a chat not shared is a 404', async () => {
  const { shares, repos, repo } = harness({ deployAt: 0 });
  const name = repo('builds');
  const conversation = chat();
  await publish(shares, { repo: name, conversation });
  assert.equal(await shares.remove(name, shareId(conversation.id)), true);
  assert.deepEqual(listed(repos.get(name)).filter((file) => file.startsWith('b/')), []);
  assert.equal(await shares.url(name, shareId(conversation.id)), null);
  assert.equal(await shares.remove(name, shareId(conversation.id)), false);
});

test('a changed repository gets a fresh clone, and a chat shared in the old one is not shared in it', async () => {
  const { shares, repos, repo } = harness({ deployAt: 0 });
  const first = repo('one');
  const conversation = chat();
  await publish(shares, { repo: first, conversation });
  const second = repo('two');
  assert.equal(await shares.url(second, shareId(conversation.id)), null);
  const other = chat();
  await publish(shares, { repo: second, conversation: other });
  assert.deepEqual(listed(repos.get(second)).filter((file) => file.startsWith('b/')).map((file) => file.split('/')[1]), Array(4).fill(shareId(other.id)));
  assert.equal(await shares.url(first, shareId(conversation.id)), null, 'the tree is the second repository now');
});

test('the site\'s builds are listed newest first with their last publish time, and a removed one is gone', async () => {
  const { shares, repo } = harness({ deployAt: 0 });
  const name = repo('builds');
  assert.deepEqual(await shares.list(name), [], 'a site with no b/ folder has no builds');
  const older = { ...chat(), title: 'Older' };
  const newer = { ...chat(), title: 'Newer' };
  await publish(shares, { repo: name, conversation: older });
  await publish(shares, { repo: name, conversation: newer });
  const builds = await shares.list(name);
  assert.deepEqual(builds.map(({ publishedAt, ...build }) => build), [newer, older].map((conversation) => ({
    shareId: shareId(conversation.id),
    title: conversation.title,
    kind: 'chat',
    labels: ['model/x'],
    url: `https://owner.github.io/builds/b/${shareId(conversation.id)}/`,
  })));
  for (const build of builds) {
    assert.match(build.publishedAt, /^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d([+-]\d\d:\d\d|Z)$/);
    assert.ok(Math.abs(Date.parse(build.publishedAt) - Date.now()) < 60_000);
  }
  // Published again, the older is the newest.
  await publish(shares, { repo: name, conversation: older });
  assert.deepEqual((await shares.list(name)).map((build) => build.title), ['Older', 'Newer']);
  assert.equal(await shares.remove(name, shareId(older.id)), true);
  assert.deepEqual((await shares.list(name)).map((build) => build.shareId), [shareId(newer.id)]);
  assert.equal(await shares.remove(name, shareId(newer.id)), true);
  assert.deepEqual(await shares.list(name), [], 'a b/ folder emptied by removals has no builds');
});

test('a removal by share id needs no chat, and one under a publish of that build is refused 409', async () => {
  let release;
  const held = new Promise((resolve) => { release = resolve; });
  let hold = false;
  const { shares, repos, repo } = harness({ exportBuild: async () => { if (hold) await held; return BUILD; }, deployAt: 0 });
  const name = repo('builds');
  const conversation = chat();
  const id = shareId(conversation.id);
  await publish(shares, { repo: name, conversation });
  hold = true;
  const running = publish(shares, { repo: name, conversation });
  await assert.rejects(shares.remove(name, id), (error) => error.statusCode === 409 && error.message === 'Wait for the build to finish publishing');
  release();
  await running;
  assert.equal(await shares.remove(name, id), true);
  assert.ok(!listed(repos.get(name)).some((file) => file.startsWith(`b/${id}/`)));
  assert.equal(await shares.remove(name, 'abcdefabcdef'), false);
});

// A bench of three models in `order`; the one named in `empty` built nothing.
function bench({ blind = false, revealed = false, empty } = {}) {
  const members = ['a', 'b', 'c'].map((name) => ({
    id: `${name}-${crypto.randomUUID()}`,
    title: null,
    model: `model/${name}`,
    world: { size: () => (name === empty ? 0 : 3) },
  }));
  const [a, b, c] = members;
  return { bench: { id: crypto.randomUUID(), title: 'A bridge', blind, revealed, order: [c.id, a.id, b.id] }, members };
}

test('a bench share holds the builds of the models that built something, in the bench\'s order, with a card of their shots', async () => {
  const exported = [];
  const { shares, repos, repo, viewerCalls } = harness({
    deployAt: 0,
    exportBuild: async (member, size) => {
      exported.push([member.model, size]);
      return { ...BUILD, blueprint: Buffer.from(`blueprint of ${member.model}`), preview: Buffer.from(`shot of ${member.model}`) };
    },
  });
  const name = repo('builds');
  const { bench: subject, members } = bench({ empty: 'a' });
  const id = shareId(subject.id);
  const url = `https://owner.github.io/builds/b/${id}/`;
  const states = await publish(shares, { repo: name, bench: subject, members });
  assert.equal(states.at(-1).state, 'live', states.at(-1).error);

  // c, a, b in order, a left out; each shot framed for its half of the card.
  assert.deepEqual(exported, [['model/c', [512, 1024]], ['model/b', [512, 1024]]]);
  const bare = repos.get(name);
  assert.deepEqual(listed(bare).filter((file) => file.startsWith('b/')).sort(), ['0.bin.gz', '1.bin.gz', 'build.json', 'index.html', 'preview.jpg'].map((file) => `b/${id}/${file}`));
  const build = JSON.parse(shown(bare, `b/${id}/build.json`));
  assert.deepEqual(build, {
    title: 'A bridge',
    kind: 'bench',
    version: build.version,
    url,
    builds: [
      { label: 'model/c', blueprint: '0.bin.gz', mask: BUILD.mask, bounds: BUILD.bounds },
      { label: 'model/b', blueprint: '1.bin.gz', mask: BUILD.mask, bounds: BUILD.bounds },
    ],
  });
  assert.equal(zlib.gunzipSync(shown(bare, `b/${id}/0.bin.gz`)).toString(), 'blueprint of model/c');
  assert.equal(zlib.gunzipSync(shown(bare, `b/${id}/1.bin.gz`)).toString(), 'blueprint of model/b');
  assert.deepEqual(JSON.parse(shown(bare, `b/${id}/preview.jpg`)), { tiles: ['shot of model/c', 'shot of model/b'], cells: montageCells(2, 1024), size: [1024, 1024] });
  assert.deepEqual(viewerCalls, [{ webRoot: '/baked/web', mesher: '/mesher.wasm' }]);
  const page = JSON.parse(shown(bare, `b/${id}/index.html`).toString().slice(6, -7));
  assert.deepEqual(page, {
    viewerId: 'v1',
    shareId: id,
    title: 'A bridge',
    description: '2 models built this in mcbench-lite. Switch between them and drag to look around.',
    url,
    image: `${url}preview.jpg`,
    labels: ['model/c', 'model/b'],
  });
  const [listedShare] = await shares.list(name);
  assert.deepEqual({ ...listedShare, publishedAt: null }, { shareId: id, title: 'A bridge', kind: 'bench', labels: ['model/c', 'model/b'], url, publishedAt: null });
  assert.equal(await shares.url(name, id), url);
});

test('a bench that hides names labels its builds with the app\'s letters, skipping a model left out; a revealed one names them', async () => {
  const { shares, repos, repo } = harness({ deployAt: 0 });
  const name = repo('builds');
  const labels = async (subject, members) => {
    await publish(shares, { repo: name, bench: subject, members });
    return JSON.parse(shown(repos.get(name), `b/${shareId(subject.id)}/build.json`)).builds.map((build) => build.label);
  };
  // In order c, a, b the letters are A, B, C; a built nothing, so B is not shown.
  const hidden = bench({ blind: true, empty: 'a' });
  assert.deepEqual(hidden.bench.order.map((id) => benchLabel(hidden.bench, hidden.members.find((member) => member.id === id), hidden.members)), ['A', 'B', 'C']);
  assert.deepEqual(await labels(hidden.bench, hidden.members), ['A', 'C']);
  const all = bench({ blind: true });
  assert.deepEqual(await labels(all.bench, all.members), ['A', 'B', 'C']);
  const revealed = bench({ blind: true, revealed: true, empty: 'a' });
  assert.deepEqual(await labels(revealed.bench, revealed.members), ['model/c', 'model/b']);
  const open = bench();
  assert.deepEqual(await labels(open.bench, open.members), ['model/c', 'model/a', 'model/b']);
  // A model run more than once is told apart by its effort.
  const twice = [{ id: 'x', model: 'model/a', effort: 'low' }, { id: 'y', model: 'model/a', effort: 'high' }, { id: 'z', model: 'model/b', effort: 'high' }];
  assert.deepEqual(twice.map((member) => benchLabel({ blind: false, order: ['x', 'y', 'z'] }, member, twice)), ['model/a · low', 'model/a · high', 'model/b']);
});

test('a chat\'s share shot is the whole square card', async () => {
  const sizes = [];
  const { shares, repo } = harness({ deployAt: 0, exportBuild: async (conversation, size) => { sizes.push(size); return BUILD; } });
  await publish(shares, { repo: repo('builds'), conversation: chat() });
  assert.deepEqual(sizes, [[1024, 1024]]);
});

// Commits to the remote `bare`, through its seed clone, what `change(seed)`
// does to the tree, and the index entries `staged(seed)` adds.
function pushToRemote(bare, change, staged = () => {}) {
  const seed = `${bare}-seed`;
  change(seed);
  git('-C', seed, 'add', '-A');
  staged(seed);
  git('-C', seed, 'commit', '--quiet', '-m', 'change');
  git('-C', seed, 'push', '--quiet', 'origin', 'HEAD:main');
}

// A file and a folder of the owner's outside the data folder, which a
// hostile repository's links point at.
function outsideFiles(id) {
  const home = fs.mkdtempSync(path.join(scratch, 'home-'));
  fs.writeFileSync(path.join(home, '.zshrc'), 'export PATH=kept\n');
  fs.mkdirSync(path.join(home, 'Documents', id), { recursive: true });
  fs.writeFileSync(path.join(home, 'Documents', id, 'thesis.txt'), 'kept\n');
  const untouched = () => {
    assert.equal(fs.readFileSync(path.join(home, '.zshrc'), 'utf8'), 'export PATH=kept\n');
    assert.equal(fs.readFileSync(path.join(home, 'Documents', id, 'thesis.txt'), 'utf8'), 'kept\n');
  };
  return { home, untouched };
}

test('a repository whose tree holds symbolic links or a submodule fails the publish naming them, writes nothing through them and drops the copy', async () => {
  const { shares, repos, repo, root } = harness({ deployAt: 0 });
  const name = repo('builds');
  const conversation = chat();
  const id = shareId(conversation.id);
  const { home, untouched } = outsideFiles(id);
  pushToRemote(repos.get(name), (seed) => {
    fs.symlinkSync(path.join(home, '.zshrc'), path.join(seed, '.nojekyll'));
    fs.symlinkSync(path.join(home, 'Documents'), path.join(seed, 'b'));
  }, (seed) => git('-C', seed, 'update-index', '--add', '--cacheinfo', `160000,${git('-C', seed, 'rev-parse', 'HEAD').trim()},module`));
  const states = await publish(shares, { repo: name, conversation });
  assert.deepEqual(states.map((event) => event.state), ['exporting', 'publishing', 'failed']);
  assert.equal(states.at(-1).error, 'owner/builds holds symbolic links or submodules (.nojekyll, b, module). Shared builds need a repository of plain files. Remove them from the repository and try again.');
  untouched();
  assert.ok(!fs.existsSync(path.join(root, 'site')), 'the copy is removed');
  await assert.rejects(shares.remove(name, id), /holds symbolic links or submodules \(\.nojekyll, b, module\)/);
  untouched();
});

test('links pushed to a repository after its first publish fail the next fetch the same way', async () => {
  const { shares, repos, repo, root } = harness({ deployAt: 0 });
  const name = repo('builds');
  const conversation = chat();
  const id = shareId(conversation.id);
  assert.equal((await publish(shares, { repo: name, conversation })).at(-1).state, 'live');
  const { home, untouched } = outsideFiles(id);
  pushToRemote(repos.get(name), (seed) => {
    git('-C', seed, 'pull', '--quiet', 'origin', 'main');
    fs.rmSync(path.join(seed, 'b'), { recursive: true });
    fs.symlinkSync(path.join(home, 'Documents'), path.join(seed, 'b'));
  });
  await assert.rejects(shares.remove(name, id), { message: 'owner/builds holds symbolic links or submodules (b). Shared builds need a repository of plain files. Remove them from the repository and try again.' });
  untouched();
  assert.ok(!fs.existsSync(path.join(root, 'site')));
});

test('a link made in the site after its tree was checked is not written or removed through', async () => {
  let plant = null;
  const { shares, repo, root } = harness({
    deployAt: 0,
    // The real ls-files, then `plant`, as another program could do between
    // the check and the publish's writes.
    step: (command, args, options) => (command === 'git' && args[0] === 'ls-files' && plant
      ? execFileAsync(command, args, options).then((result) => { plant(path.join(root, 'site')); return result; })
      : undefined),
  });
  const name = repo('builds');
  const conversation = chat();
  const id = shareId(conversation.id);
  const { home, untouched } = outsideFiles(id);
  plant = (site) => fs.symlinkSync(path.join(home, '.zshrc'), path.join(site, '.nojekyll'));
  const states = await publish(shares, { repo: name, conversation });
  assert.equal(states.at(-1).error, `${path.join(root, 'site', '.nojekyll')} is a symbolic link. Shared builds need a repository of plain files.`);
  untouched();
  plant = (site) => fs.symlinkSync(path.join(home, 'Documents'), path.join(site, 'b'));
  await assert.rejects(shares.remove(name, id), { message: `${path.join(root, 'site', 'b')} is a symbolic link. Shared builds need a repository of plain files.` });
  untouched();
  plant = null;
  assert.equal((await publish(shares, { repo: name, conversation })).at(-1).state, 'live');
});

test('a site holding a share in the old format fails the listing and names it', async () => {
  const { shares, repos, repo } = harness({ deployAt: 0 });
  const name = repo('builds');
  const seed = `${repos.get(name)}-seed`;
  fs.mkdirSync(path.join(seed, 'b', 'abcdefabcdef'), { recursive: true });
  fs.writeFileSync(path.join(seed, 'b', 'abcdefabcdef', 'build.json'), JSON.stringify({ title: 'Old', model: 'model/x', url: 'u', blueprint: 'blueprint.bin.gz' }));
  git('-C', seed, 'add', '-A');
  git('-C', seed, 'commit', '--quiet', '-m', 'old share');
  git('-C', seed, 'push', '--quiet', 'origin', 'HEAD:main');
  await assert.rejects(shares.list(name), /b\/abcdefabcdef was published in an older format/);
});
