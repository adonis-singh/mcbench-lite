'use strict';

// Shared builds: a chat's build, or a bench's builds, as static files in a
// GitHub Pages repository the owner names in Settings (shareRepo), so a link
// to it shows the builds in a viewer anyone can move around in (X's Player
// Card embeds the page). The site is a git clone of the repository at
// DATA_DIR/share/site:
//   .nojekyll
//   viewer/<viewerId>/...     the viewer (share-viewer.js writeViewer)
//   b/<shareId>/index.html    the page (share-viewer.js sharePageHtml)
//   b/<shareId>/build.json    what the page loads: { title, kind ('chat' or
//                             'bench'), version, url, builds: [{ label,
//                             blueprint, mask, bounds }] }
//   b/<shareId>/<n>.bin.gz    build n as Minecraft settles it, gzipped
//   b/<shareId>/preview.jpg   the card's image
// shareId is shareId() of the chat's or the bench's id. A chat is a share of
// one build; a bench's has one per model that built something.
// A publish exports the builds, pulls, writes, commits and pushes, then waits
// until Pages serves the version it wrote. The site is one working tree, so
// one publish or removal changes it at a time.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { DATA_DIR } = require('./storage');
const { httpError } = require('./http-guard');
const { settle, currentTicks } = require('./shot-world');
const { resolveShot, renderShots, montageCells, composeMontage } = require('./shot');
const { worldBlueprint } = require('./blueprint');
const { blueMap, renderMask } = require('./bluemap');
const { minecraftLive } = require('./minecraft-live');

const execFileAsync = promisify(execFile);
const WAIT_MS = 5 * 60 * 1000;
const POLL_MS = 5000;
// How long a git or gh command may take: one that talks to GitHub (clone,
// fetch, push) and one that does not or asks it one thing (local git, gh api).
const NETWORK_MS = 2 * 60 * 1000;
const LOCAL_MS = 30 * 1000;
// git never asks for a username or password: the app has no terminal to
// answer it, so it would wait forever. An empty GIT_ASKPASS stops git from
// looking for an askpass program (core.askPass, SSH_ASKPASS).
const NO_PROMPTS = { GIT_TERMINAL_PROMPT: '0', GCM_INTERACTIVE: 'never', GIT_ASKPASS: '' };
// What git says when it needed to sign in and could not.
const SIGN_IN_FAILED = /terminal prompts disabled|could not read (Username|Password)|Authentication failed/i;

const duration = (ms) => (ms >= 60_000 ? `${ms / 60_000} minute${ms === 60_000 ? '' : 's'}` : `${ms / 1000} seconds`);
const said = (error) => String(error.stderr || error.message).trim();
// The card's image: square, as X's Player Card crops one.
const PREVIEW_SIDE = 1024;

const shareId = (id) => crypto.createHash('sha256').update(id).digest('hex').slice(0, 12);

// The git modes of a symbolic link and of a submodule. A site whose tree
// holds either is refused (Shares.update): a write or removal in the site
// could go through one to a folder outside the data folder.
const LINK_MODES = new Set(['120000', '160000']);
const PLAIN_FILES = 'Shared builds need a repository of plain files.';

// Fails when `file` in the site is a symbolic link, so nothing is written or
// removed through one, even a link made after the site's tree was checked.
function refuseLink(file) {
  if (fs.lstatSync(file, { throwIfNoEntry: false })?.isSymbolicLink()) throw new Error(`${file} is a symbolic link. ${PLAIN_FILES}`);
}

// What a bench shows for `member`, one of its `members`: while the bench
// hides names, its letter as the app shows it (its place in bench.order);
// otherwise the model's name, with the effort when the bench runs that model
// more than once.
function benchLabel(bench, member, members) {
  if (bench.blind && !bench.revealed) return String.fromCharCode(65 + bench.order.indexOf(member.id));
  const repeated = members.filter((other) => other.model === member.model).length > 1;
  return repeated && member.effort ? `${member.model} · ${member.effort}` : member.model;
}

// A publish: 'exporting' the build (settle and preview shot), 'publishing'
// it (the site's git work and push), 'waiting' for Pages to serve it, then
// 'live'; 'failed' from any of the first three. Each move is told to
// `onState` as { state, ...details }.
const PUBLISH_MOVES = {
  exporting: ['publishing', 'failed'],
  publishing: ['waiting', 'failed'],
  waiting: ['live', 'failed'],
  live: [],
  failed: [],
};

class Publish {
  constructor(onState) {
    this.state = 'exporting';
    this.onState = onState;
    onState({ state: 'exporting' });
  }

  to(state, details = {}) {
    if (!PUBLISH_MOVES[this.state].includes(state)) throw new Error(`A publish that is ${this.state} cannot become ${state}`);
    this.state = state;
    this.onState({ state, ...details });
  }
}

// The build as see_build draws it: the chat's world settled at its settle
// ticks, as a blueprint, the viewer's mask and bounds for it, and one
// three-quarter shot of it of `size` ([width, height]). With the files of the
// current bake the viewer is made from.
async function exportBuild(conversation, size) {
  if (!blueMap.assets) throw new Error('The viewer\'s assets are not baked yet');
  const ticks = currentTicks(conversation.world);
  const { world } = (await settle(conversation.id, conversation.world, [ticks])).get(ticks);
  if (world.size() === 0) throw new Error(`Minecraft keeps none of the blocks of this build at tick ${ticks}`);
  const bounds = world.bounds();
  const lift = ([x, y, z]) => [x, y + minecraftLive.yOffset, z];
  const [preview] = await renderShots(conversation.id, [{ ...resolveShot({ view: 'iso_se', size: 'preview' }, world, { preview: size }), tick: ticks }]);
  return {
    blueprint: worldBlueprint(world, minecraftLive.yOffset),
    mask: renderMask(bounds),
    // At Minecraft height, as the blueprint is: the viewer fits its camera to them.
    bounds: { min: lift(bounds.min), max: lift(bounds.max) },
    preview,
    webRoot: path.join(blueMap.root, blueMap.assets.id, 'web'),
    mesher: blueMap.mesher,
  };
}

class Shares {
  // `run(command, args, options)` runs git and gh as execFile does;
  // `fetch` reads the public build.json; `viewer` is share-viewer.js.
  constructor({
    root = path.join(DATA_DIR, 'share'),
    remote = (repo) => `https://github.com/${repo}.git`,
    run = execFileAsync,
    fetch = globalThis.fetch,
    exportBuild: exporter = exportBuild,
    montage = composeMontage,
    viewer,
    waitMs = WAIT_MS,
    pollMs = POLL_MS,
    networkMs = NETWORK_MS,
    localMs = LOCAL_MS,
  }) {
    this.site = path.join(root, 'site');
    this.remote = remote;
    this.run = run;
    this.fetch = fetch;
    this.exportBuild = exporter;
    this.montage = montage;
    this.viewer = viewer;
    this.waitMs = waitMs;
    this.pollMs = pollMs;
    this.networkMs = networkMs;
    this.localMs = localMs;
    this.publishes = new Map(); // share id -> Publish
    this.tree = Promise.resolve();
  }

  publishing(id) {
    return this.publishes.has(id);
  }

  // The page's address while the site, cloned from `repo`, holds build `id`;
  // null otherwise.
  async url(repo, id) {
    const file = path.join(this.site, 'b', id, 'build.json');
    if (!fs.existsSync(file) || await this.origin() !== this.remote(repo)) return null;
    return JSON.parse(fs.readFileSync(file, 'utf8')).url;
  }

  // The address the site was cloned from, as written: `git remote get-url`
  // would give it rewritten by the owner's url.<base>.insteadOf settings.
  async origin() {
    return (await this.git(['config', '--get', 'remote.origin.url'])).stdout.trim();
  }

  // Runs `command` with `args` through `run`, killed after `ms`: a command
  // still running then fails, naming itself.
  async exec(command, args, { cwd, env = {}, ms }) {
    const signal = AbortSignal.timeout(ms);
    try {
      return await this.run(command, args, { cwd, env: { ...process.env, ...env }, signal });
    } catch (error) {
      if (signal.aborted) throw new Error(`\`${command} ${args[0]}\` did not finish within ${duration(ms)}`);
      throw error;
    }
  }

  // git `args` in the site (`cwd`: elsewhere), with no prompts, within the
  // network time limit when `network`. A sign-in git could not make fails
  // saying how to let git use gh's sign-in.
  async git(args, { cwd = this.site, env = {}, network = false } = {}) {
    try {
      return await this.exec('git', args, { cwd, env: { ...NO_PROMPTS, ...env }, ms: network ? this.networkMs : this.localMs });
    } catch (error) {
      if (SIGN_IN_FAILED.test(said(error))) {
        throw new Error(`git could not sign in to GitHub over HTTPS (git ${args[0]}). Run \`gh auth setup-git\` in a terminal so git signs in with the GitHub CLI's account, then try again.`);
      }
      throw error;
    }
  }

  // gh `args`. gh missing, or not signed in, fails saying so.
  async gh(args) {
    try {
      return await this.exec('gh', args, { ms: this.localMs });
    } catch (error) {
      if (error.code === 'ENOENT') throw new Error('The GitHub CLI (gh) is not installed: install it from https://cli.github.com and run `gh auth login`');
      // gh exits 4 when it has no sign-in; GitHub answers 401 to one that has expired.
      if (error.code === 4 || /HTTP 401/.test(said(error))) throw new Error(`The GitHub CLI is not signed in to GitHub: run \`gh auth login\` (gh ${args.join(' ')}: ${said(error)})`);
      throw error;
    }
  }

  // What a chat's share holds: its one build, labelled with its model.
  async chatShare(conversation) {
    const { preview, webRoot, mesher, ...build } = await this.exportBuild(conversation, [PREVIEW_SIDE, PREVIEW_SIDE]);
    return {
      title: conversation.title,
      kind: 'chat',
      description: `Built by ${conversation.model} in mcbench-lite. Drag to look around.`,
      builds: [{ label: conversation.model, ...build }],
      preview,
      webRoot,
      mesher,
    };
  }

  // What a bench's share holds: the build of each of `members` (its model
  // chats) whose world is not empty, in bench.order, labelled as the app
  // shows them; its card is every build's shot, each framed for its cell.
  async benchShare(bench, members) {
    const shown = bench.order.map((id) => members.find((member) => member.id === id)).filter((member) => member.world.size() > 0);
    const cells = montageCells(shown.length, PREVIEW_SIDE);
    const exported = [];
    for (const [i, member] of shown.entries()) exported.push(await this.exportBuild(member, [cells[i].width, cells[i].height]));
    const { webRoot, mesher } = exported[0];
    return {
      title: bench.title,
      kind: 'bench',
      description: `${shown.length} models built this in mcbench-lite. Switch between them and drag to look around.`,
      builds: exported.map(({ blueprint, mask, bounds }, i) => ({ label: benchLabel(bench, shown[i], members), blueprint, mask, bounds })),
      preview: await this.montage(exported.map((build) => build.preview), cells, [PREVIEW_SIDE, PREVIEW_SIDE]),
      webRoot,
      mesher,
    };
  }

  // Publishes to `repo` the share of id `shareId(sourceId)` that `build()`
  // exports (chatShare, benchShare), telling `onState` each move. Refused
  // (409) at once while that share is publishing; otherwise it ends live or
  // failed, never rejected.
  publish({ repo, sourceId, build, onState }) {
    const id = shareId(sourceId);
    if (this.publishes.has(id)) throw httpError(409, 'This build is already being published');
    const publish = new Publish(onState);
    this.publishes.set(id, publish);
    return this.carry(publish, repo, build, id).finally(() => this.publishes.delete(id));
  }

  async carry(publish, repo, exportShare, id) {
    try {
      const share = await exportShare();
      publish.to('publishing');
      const version = crypto.randomUUID();
      const url = await this.inTree(repo, {
        look: async () => `${await this.pagesBase(repo, (await this.git(['branch', '--show-current'])).stdout.trim())}b/${id}/`,
        change: async (url) => {
          const folder = path.join(this.site, 'b', id);
          for (const file of ['.nojekyll', 'viewer', 'b', path.join('b', id)]) refuseLink(path.join(this.site, file));
          fs.writeFileSync(path.join(this.site, '.nojekyll'), '');
          const viewerId = this.viewer.writeViewer(this.site, { webRoot: share.webRoot, mesher: share.mesher });
          fs.rmSync(folder, { recursive: true, force: true });
          fs.mkdirSync(folder, { recursive: true });
          const { title, kind, description } = share;
          const builds = share.builds.map(({ label, blueprint, mask, bounds }, i) => {
            fs.writeFileSync(path.join(folder, `${i}.bin.gz`), zlib.gzipSync(blueprint));
            return { label, blueprint: `${i}.bin.gz`, mask, bounds };
          });
          fs.writeFileSync(path.join(folder, 'build.json'), JSON.stringify({ title, kind, version, url, builds }));
          fs.writeFileSync(path.join(folder, 'preview.jpg'), share.preview);
          fs.writeFileSync(path.join(folder, 'index.html'), this.viewer.sharePageHtml({
            viewerId,
            shareId: id,
            title,
            description,
            url,
            image: `${url}preview.jpg`,
            labels: builds.map((build) => build.label),
          }));
          await this.push(`Share ${id}: ${title}`);
          return url;
        },
      });
      publish.to('waiting');
      await this.served(url, version);
      publish.to('live', { url });
    } catch (error) {
      console.error('[share]', error);
      publish.to('failed', { error: String(error.message || error) });
    }
  }

  // The Pages site's address, ending in '/'. Builds are pushed to the root
  // of `branch`; a site that Pages builds from another branch or folder
  // would never serve them, so it fails, saying what to set. One that a
  // GitHub Actions workflow deploys serves what that workflow deploys.
  async pagesBase(repo, branch) {
    const setting = `in the repository's Settings → Pages, set Source to "Deploy from a branch", branch ${branch}, folder / (root)`;
    let pages;
    try {
      pages = JSON.parse((await this.gh(['api', `repos/${repo}/pages`])).stdout);
    } catch (error) {
      if (/HTTP 404/.test(said(error))) throw new Error(`GitHub Pages is not set up for ${repo} (${said(error)}): ${setting}`);
      throw error;
    }
    if (pages.build_type !== 'workflow' && (pages.source.branch !== branch || pages.source.path !== '/')) {
      throw new Error(`GitHub Pages for ${repo} serves the ${pages.source.branch} branch's ${pages.source.path} folder, but builds are pushed to the root of ${branch}: ${setting}`);
    }
    return pages.html_url;
  }

  // Pages deploys a push within a minute or so; its CDN is asked past its
  // cache each time.
  async served(url, version) {
    const deadline = Date.now() + this.waitMs;
    for (;;) {
      const response = await this.fetch(`${url}build.json?check=${crypto.randomUUID()}`, { cache: 'no-store' });
      if (response.ok && (await response.json()).version === version) return;
      if (Date.now() >= deadline) {
        throw new Error(`The files are pushed, but GitHub Pages has not served them yet after ${Math.round(this.waitMs / 60_000)} minutes: ${url}`);
      }
      await new Promise((resolve) => setTimeout(resolve, this.pollMs));
    }
  }

  // The shares on the site cloned from `repo`, newest first:
  // { shareId, title, kind, labels, url, publishedAt }, where publishedAt is the ISO
  // time of the last commit that changed the build's build.json.
  async list(repo) {
    return this.inTree(repo, { look: async () => {
      const builds = path.join(this.site, 'b');
      if (!fs.existsSync(builds)) return [];
      // One pass over the history, newest commit first: a build's first
      // appearance is its last change.
      const { stdout } = await this.git(['log', '--format=%x00%cI', '--name-only', '--', 'b/*/build.json']);
      const dates = new Map();
      let date;
      for (const line of stdout.split('\n')) {
        if (line.startsWith('\0')) date = line.slice(1);
        else if (line) {
          const id = line.split('/')[1];
          if (!dates.has(id)) dates.set(id, date);
        }
      }
      return [...dates]
        .filter(([id]) => fs.existsSync(path.join(builds, id, 'build.json')))
        .map(([id, publishedAt]) => {
          const { title, kind, url, builds: shown } = JSON.parse(fs.readFileSync(path.join(builds, id, 'build.json'), 'utf8'));
          if (!Array.isArray(shown)) throw new Error(`The shared build b/${id} was published in an older format: publish it again or remove it`);
          return { shareId: id, title, kind, labels: shown.map((build) => build.label), url, publishedAt };
        });
    } });
  }

  // Removes build `id` from `repo` and pushes: true, or false when the site
  // has no such build. Refused (409) while it is publishing.
  async remove(repo, id) {
    if (this.publishes.has(id)) throw httpError(409, 'Wait for the build to finish publishing');
    return this.inTree(repo, { change: async () => {
      const folder = path.join(this.site, 'b', id);
      refuseLink(path.join(this.site, 'b'));
      refuseLink(folder);
      if (!fs.existsSync(folder)) return false;
      fs.rmSync(folder, { recursive: true });
      await this.push(`Remove ${id}`);
      return true;
    } });
  }

  // The site is public: its commits are made as the signed-in GitHub account
  // at its noreply address, not as git's own default, the owner's name and
  // machine.
  async author() {
    const [name, email] = (await this.gh(['api', 'user', '--jq', '.login, "\\(.id)+\\(.login)@users.noreply.github.com"'])).stdout.trim().split('\n');
    return { GIT_AUTHOR_NAME: name, GIT_AUTHOR_EMAIL: email, GIT_COMMITTER_NAME: name, GIT_COMMITTER_EMAIL: email };
  }

  async push(message) {
    await this.git(['add', '-A']);
    await this.git(['commit', '--quiet', '-m', message], { env: await this.author() });
    await this.git(['push', '--quiet'], { network: true });
  }

  // Runs on the site, cloned from `repo` and brought up to date with it,
  // after any work queued before it: `look`, which only reads it, then
  // `change(what look gave)`, which writes and pushes; gives what `change`
  // gives. A copy that cannot be brought up to date is kept as it is: it
  // still holds what was last pushed, so url() answers from it. A copy whose
  // change failed part-way (it may hold half-written files or a commit not
  // pushed) is dropped, as is one cloned from another repository, so the
  // next job starts from a fresh clone.
  inTree(repo, { look = async () => {}, change = async (looked) => looked }) {
    const job = this.tree.then(async () => {
      await this.update(repo);
      const looked = await look();
      try {
        return await change(looked);
      } catch (error) {
        fs.rmSync(this.site, { recursive: true, force: true });
        throw error;
      }
    });
    this.tree = job.catch(() => {});
    return job;
  }

  // Clones `repo` to the site, or fetches it and moves the copy to what was
  // fetched, then checks its tree. A clone that fails is removed: it may be
  // partial.
  async update(repo) {
    if (fs.existsSync(this.site) && await this.origin() !== this.remote(repo)) fs.rmSync(this.site, { recursive: true });
    if (!fs.existsSync(this.site)) {
      fs.mkdirSync(path.dirname(this.site), { recursive: true });
      try {
        await this.git(['clone', '--quiet', this.remote(repo), this.site], { cwd: path.dirname(this.site), network: true });
      } catch (error) {
        fs.rmSync(this.site, { recursive: true, force: true });
        throw new Error(`Could not get a copy of ${repo} from GitHub: ${said(error)}`);
      }
    } else {
      try {
        await this.git(['fetch', '--quiet'], { network: true });
      } catch (error) {
        throw new Error(`Could not bring the site's copy up to date with ${repo} on GitHub: ${said(error)}`);
      }
      await this.git(['reset', '--quiet', '--hard', '@{upstream}']);
    }
    await this.plainFiles(repo);
  }

  // A site whose tree holds a symbolic link or a submodule is removed and
  // fails, naming them: the publish writes `.nojekyll`, `viewer/` and
  // `b/<id>/` and removes `b/<id>/`, and through a link those would reach
  // files outside the data folder.
  async plainFiles(repo) {
    // `-z`: each entry is "<mode> <object> <stage>\t<path>", ended by NUL.
    const { stdout } = await this.git(['ls-files', '-s', '-z']);
    const links = stdout.split('\0').filter(Boolean)
      .map((entry) => entry.split('\t'))
      .filter(([info]) => LINK_MODES.has(info.split(' ')[0]))
      .map(([, file]) => file);
    if (links.length === 0) return;
    fs.rmSync(this.site, { recursive: true });
    throw new Error(`${repo} holds symbolic links or submodules (${links.join(', ')}). ${PLAIN_FILES} Remove them from the repository and try again.`);
  }
}

module.exports = { Shares, Publish, PUBLISH_MOVES, shareId, benchLabel, exportBuild, PREVIEW_SIDE };
