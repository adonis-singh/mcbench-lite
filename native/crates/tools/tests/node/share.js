'use strict';

// Today's side of the share differential (tests/share_diff.rs): one stored
// chat and one stored bench of a clone of the fixture, each published by
// server/share.js Shares to a local bare repository, with `gh`, the fetch
// of the public build.json and the viewer given (the viewer writes fixed
// bytes). The export is today's composition over the chat's world as stored
// (worldBlueprint at MCBENCH_MC_Y_OFFSET's height, renderMask, the bounds
// lifted) with a shot named by its model and size in place of the settle and
// the helper's picture, which their own differentials cover (settle,
// see_build). Writes <out>/manifest.json (the chats and the bench as the Rust
// side needs them, each world as <out>/worlds/<id>.blueprint) and the site in
// <out>/node/site.
//
//   MCBENCH_DATA_DIR=<clone of the fixture without settings> node share.js <out>

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');
const { execFileSync, execFile } = require('child_process');
const { promisify } = require('util');

const root = path.resolve(__dirname, '../../../../..');
const [out] = process.argv.slice(2);
const server = (name) => require(path.join(root, 'server', name));
// The page's version and every check= of the fetch: fixed, as the Rust side's.
const VERSION = '00000000-0000-4000-8000-000000000000';
crypto.randomUUID = () => VERSION;

fs.mkdirSync(path.join(out, 'worlds'), { recursive: true });
fs.writeFileSync(path.join(out, 'gitconfig'), '');
Object.assign(process.env, {
  GIT_CONFIG_GLOBAL: path.join(out, 'gitconfig'),
  GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Test',
  GIT_AUTHOR_EMAIL: 'test@example.invalid',
  GIT_COMMITTER_NAME: 'Test',
  GIT_COMMITTER_EMAIL: 'test@example.invalid',
});
const git = (...args) => execFileSync('git', args, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });

const store = server('store.js');
const { Shares, shareId } = server('share.js');
const { worldBlueprint, blueprintWorld } = server('blueprint.js');
const { renderMask } = server('bluemap.js');
const Y_OFFSET = Number(process.env.MCBENCH_MC_Y_OFFSET || 64);

// The bare remote with one commit on main.
function remote(name) {
  const bare = path.join(out, `${name}.git`);
  git('init', '--quiet', '--bare', '-b', 'main', bare);
  const seed = `${bare}-seed`;
  git('clone', '--quiet', bare, seed);
  fs.writeFileSync(path.join(seed, 'README.md'), 'shared builds\n');
  git('-C', seed, 'add', '-A');
  git('-C', seed, 'commit', '--quiet', '-m', 'init');
  git('-C', seed, 'push', '--quiet', 'origin', 'HEAD:main');
  return bare;
}

// A source as both sides read it: the stored world written as a blueprint
// at its own heights and read back.
function source(conversation) {
  const file = path.join(out, 'worlds', `${conversation.id}.blueprint`);
  fs.writeFileSync(file, worldBlueprint(conversation.world, 0));
  return {
    id: conversation.id,
    title: conversation.title,
    model: conversation.model,
    effort: conversation.effort || null,
    world: blueprintWorld(fs.readFileSync(file), 0),
  };
}

(async () => {
  const chats = store.listConversationIds().map(({ id }) => store.getConversation(id)).filter(Boolean);
  const chat = chats.find((conversation) => !conversation.benchId && conversation.world.size() > 0);
  if (!chat) throw new Error('the fixture has no chat with a build');
  let bench = null;
  let members = null;
  for (const { id } of store.listBenchIds()) {
    const found = store.getBench(id);
    if (!found) continue;
    const its = store.benchMembers(id);
    if (its.filter((member) => member.world.size() > 0).length >= 2) {
      bench = found;
      members = its;
      break;
    }
  }
  if (!bench) throw new Error('the fixture has no bench with two builds');

  const chatSource = source(chat);
  const memberSources = members.map(source);
  const benchSource = { id: bench.id, title: bench.title, blind: Boolean(bench.blind), revealed: Boolean(bench.revealed), order: bench.order };
  fs.writeFileSync(path.join(out, 'manifest.json'), JSON.stringify({
    yOffset: Y_OFFSET,
    version: VERSION,
    chat: { ...chatSource, world: undefined },
    bench: benchSource,
    members: memberSources.map((member) => ({ ...member, world: undefined })),
  }));

  const bare = remote('node-remote');
  const execFileAsync = promisify(execFile);
  const shares = new Shares({
    root: path.join(out, 'node'),
    remote: () => bare,
    run: async (command, args, options) => {
      if (command === 'gh' && args[1] === 'user') return { stdout: 'owner\n42+owner@users.noreply.github.com\n', stderr: '' };
      if (command === 'gh') return { stdout: JSON.stringify({ html_url: 'https://owner.github.io/builds/', build_type: 'legacy', source: { branch: 'main', path: '/' } }), stderr: '' };
      return execFileAsync(command, args, options);
    },
    fetch: async () => ({ ok: true, status: 200, json: async () => ({ version: VERSION }) }),
    exportBuild: async (conversation, size) => {
      const world = conversation.world;
      const bounds = world.bounds();
      const lift = ([x, y, z]) => [x, y + Y_OFFSET, z];
      return {
        blueprint: worldBlueprint(world, Y_OFFSET),
        mask: renderMask(bounds),
        bounds: { min: lift(bounds.min), max: lift(bounds.max) },
        preview: Buffer.from(`shot of ${conversation.model} ${size.join('x')}`),
        webRoot: '/baked/web',
        mesher: '/mesher.wasm',
      };
    },
    montage: async (tiles, cells, size) => Buffer.from(JSON.stringify({ tiles: tiles.map(String), cells, size })),
    viewer: {
      writeViewer(siteDir) {
        fs.mkdirSync(path.join(siteDir, 'viewer', 'v1'), { recursive: true });
        fs.writeFileSync(path.join(siteDir, 'viewer', 'v1', 'viewer.js'), 'viewer');
        return 'v1';
      },
      sharePageHtml: (page) => `<html>${JSON.stringify(page)}</html>`,
    },
  });
  const done = [];
  for (const [sourceId, build] of [
    [chatSource.id, () => shares.chatShare(chatSource)],
    [benchSource.id, () => shares.benchShare(benchSource, memberSources)],
  ]) {
    const states = [];
    await shares.publish({ repo: 'owner/builds', sourceId, build, onState: (event) => states.push(event) });
    if (states.at(-1).state !== 'live') throw new Error(`${sourceId}: ${JSON.stringify(states)}`);
    done.push({ shareId: shareId(sourceId), states });
  }
  fs.writeFileSync(path.join(out, 'node.json'), JSON.stringify({
    shares: done,
    commits: git('--git-dir', bare, 'log', '--format=%an <%ae> %cn <%ce> %s', 'main').trim().split('\n'),
    list: (await shares.list('owner/builds')).map(({ publishedAt, ...build }) => build),
  }));
  console.error(`share: chat ${chat.id}, bench ${bench.id} (${members.length} members)`);
  process.exit(0);
})().catch((error) => { console.error(error); process.exit(1); });
