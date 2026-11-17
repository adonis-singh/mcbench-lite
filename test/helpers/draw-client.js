'use strict';

// A fresh server/shot.js whose helper is test/helpers/fake-draw.js, which
// speaks mcbench-draw's protocol (native/draw/PROTOCOL.md) and draws nothing:
// `modes` are the fake's behaviour at each start (its header lists them),
// the last one for every later start. Worlds are settled-world stand-ins
// with a scene of their own (shot-world.js sceneOf).

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const FAKE = path.join(__dirname, 'fake-draw.js');

function drawClient(t, modes) {
  fs.chmodSync(FAKE, 0o755);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-fake-draw-'));
  const controlFile = path.join(dir, 'control.json');
  const logFile = path.join(dir, 'log.ndjson');
  let control = { modes, release: 0 };
  // Written whole (a rename), as the fake reads it every few milliseconds.
  const write = () => {
    fs.writeFileSync(`${controlFile}.part`, JSON.stringify(control));
    fs.renameSync(`${controlFile}.part`, controlFile);
  };
  write();
  process.env.FAKE_DRAW_CONTROL = controlFile;
  process.env.FAKE_DRAW_LOG = logFile;
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const draw = require('../../server/draw');
  t.mock.method(draw, 'installDraw', async () => FAKE);
  t.mock.method(draw, 'helperAssets', async () => ({ pack: '/fixture/pack' }));
  const shotWorlds = require('../../server/shot-world');
  const scenes = new WeakMap();
  let serials = 0;
  t.mock.method(shotWorlds, 'sceneOf', (world) => {
    if (!scenes.has(world)) scenes.set(world, { serial: ++serials, edition: world.edition, blueprint: Buffer.from('blueprint'), entities: [] });
    return scenes.get(world);
  });
  delete require.cache[require.resolve('../../server/shot')];
  const shot = require('../../server/shot');
  t.after(() => shot.closeRenderer().catch(() => {}));

  const { World } = require('../../server/world');
  // A settled world of the build's `edition` (shot-world.js sceneOf).
  const world = (edition = 'e1') => {
    const made = new World();
    made.edition = edition;
    made.set(0, 0, 0, 'minecraft:stone');
    made.set(1, 1, 1, 'minecraft:stone');
    return made;
  };
  const fixture = world();
  const log = () => (fs.existsSync(logFile) ? fs.readFileSync(logFile, 'utf8').split('\n').filter(Boolean).map(JSON.parse) : []);
  return {
    ...shot,
    world,
    // A render of `views` of `of` (the fixture world unless given).
    render: (views = ['front'], signal, of = fixture, conversation = 'fixture') => shot.renderShots(conversation, views.map((view) => ({ ...shot.resolveShot({ view }, of, []), tick: 0 })), signal),
    // A call of shots of several settled worlds: [world, tick] for each shot.
    renderOf: (conversation, worlds) => shot.renderShots(conversation, worlds.map(([of, tick]) => ({ ...shot.resolveShot({ view: 'front' }, of, []), tick }))),
    log,
    starts: () => log().filter((entry) => entry.type === 'start'),
    asked: (type) => log().filter((entry) => entry.type === type),
    set: (fields) => { control = { ...control, ...fields }; write(); },
    // The pid of the helper now running.
    alive: (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } },
  };
}

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
// How `promise` ended within `ms`: 'resolved', its error, or 'pending'.
const within = (promise, ms) => Promise.race([promise.then(() => 'resolved', (error) => error), pause(ms).then(() => 'pending')]);
const until = async (condition, what, ms = 5000) => {
  for (let waited = 0; !condition(); waited += 5) {
    if (waited > ms) throw new Error(`timed out waiting for ${what}`);
    await pause(5);
  }
};

module.exports = { drawClient, pause, within, until };
