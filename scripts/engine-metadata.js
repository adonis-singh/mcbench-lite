'use strict';

// Records, for one Minecraft version, what the native build of the engine
// needs ahead of time: what the code reaches through reflection (the
// engine's classes on GraalVM's JVM under its tracing agent) and the profile
// it is optimised with (an instrumented native build), each from the same
// exercise: every community build in the corpus MCBENCH_CORPUS names placed, ticked as
// the live world and given an experiment in both placement modes, then every
// verifier fixture (scripts/verifier-fixtures.js) with and without a seed.
// Writes server/native/<version>/reachability-metadata.json and
// engine.iprof.gz. Run it when the pinned server or the engine changes, and
// commit the result.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/engine-metadata.js

const fs = require('fs');
const os = require('os');
const path = require('path');
const ROOT = path.join(__dirname, '..');
if (!process.env.MCBENCH_DATA_DIR || fs.realpathSync(path.resolve(process.env.MCBENCH_DATA_DIR)) === fs.realpathSync(path.join(ROOT, 'data'))) {
  throw new Error('Set MCBENCH_DATA_DIR to a scratch copy of the data folder');
}
const { minecraftLive } = require('../server/minecraft-live');
const { spawn } = require('child_process');
const zlib = require('zlib');
const { Engine, compileNative, jvmEngine, NATIVE, PROFILE } = require('../server/engine');
const { MinecraftVerifier } = require('../server/minecraft-verify');
const { importCommands } = require('../server/live-placement');
const { Y_OFFSET } = require('../server/minecraft-dimension');
const { World } = require('../server/world');
const fixtures = require('./verifier-fixtures');
const { corpusArtifacts } = require('./corpus');


function communityBuilds() {
  const builds = corpusArtifacts().map(([, file]) => file);
  return builds.sort();
}

// Every community build placed, ticked as the live world, and given an
// experiment in both placement modes; then every verifier fixture with and
// without a seed. Returns how many experiments ran.
async function exercise(engine, scratch) {
  const verifier = new MinecraftVerifier({ root: path.join(scratch, 'verify') });
  let experiments = 0;
  for (const file of communityBuilds()) {
    const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
    const world = new World();
    for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
    world.finalize();
    const commands = path.join(scratch, 'commands.txt');
    fs.writeFileSync(commands, `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`);
    await engine.place(commands, path.join(scratch, 'blueprint.bin'));
    for (let tick = 0; tick < 20; tick += 1) await engine.tick(1, path.join(scratch, 'patch.bin'), path.join(scratch, 'entities.json'));
    const containers = artifact.blocks.filter((block) => /chest$|barrel|hopper$|dropper|dispenser|furnace$/.test(block.id)).slice(0, 4).map((block) => block.pos);
    const args = {
      probes: [...containers, ...artifact.blocks.slice(0, 32).map((block) => block.pos)],
      actions: [
        ...artifact.blocks.filter((block) => block.id === 'minecraft:lever').slice(0, 8).map((block) => ({ tick: 2, type: 'toggle', at: block.pos })),
        ...containers.map((at) => ({ tick: 3, type: 'insert', at, slot: 0, count: 8, item: 'minecraft:cobblestone' })),
      ],
      samples: Array.from({ length: 41 }, (_, tick) => tick),
      ...(containers.length ? { inventory_probes: containers, world_inventories: true } : {}),
    };
    for (const mode of ['updates', 'strict']) {
      verifier.placementMode = mode;
      const request = path.join(scratch, 'request.json');
      fs.writeFileSync(request, JSON.stringify(verifier.transport(verifier.request(world, args))));
      await engine.verify(request, commands, path.join(scratch, 'experiment'));
      experiments += 1;
    }
  }
  for (const fixture of fixtures) {
    const world = fixture.world();
    const commands = path.join(scratch, 'commands.txt');
    fs.writeFileSync(commands, `${importCommands(world, null, Y_OFFSET).commands.join('\n')}\n`);
    verifier.placementMode = fixture.placementMode || 'updates';
    for (const seed of [null, 1]) {
      verifier.seed = seed;
      const request = path.join(scratch, 'request.json');
      fs.writeFileSync(request, JSON.stringify(verifier.transport(verifier.request(world, fixture.args))));
      // The fixtures named fails-* are requests the experiment may refuse.
      if (fixture.name.includes('/fails-')) await engine.verify(request, commands, path.join(scratch, 'experiment')).catch(() => {});
      else await engine.verify(request, commands, path.join(scratch, 'experiment'));
      experiments += 1;
    }
  }
  return experiments;
}

async function main() {
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-engine-metadata-'));
  const classes = path.join(scratch, 'classes');
  fs.mkdirSync(classes);
  const output = path.join(NATIVE, minecraftLive.minecraftVersion);
  fs.mkdirSync(output, { recursive: true });
  // What the code reaches through reflection, from the JVM's tracing agent,
  // which writes it as the JVM exits.
  const traced = jvmEngine({ classpath, classes, jvmArgs: [`-agentlib:native-image-agent=config-output-dir=${output}`] });
  await traced.warm({ classpath, binaries: scratch, folder: path.join(scratch, 'traced'), minecraftVersion: minecraftLive.minecraftVersion });
  const experiments = await exercise(traced, scratch);
  await traced.stop();
  // The profile the native build optimises with, from an instrumented native
  // build, which writes it as it exits when its input ends.
  const instrumented = await compileNative({ classpath, folder: path.join(scratch, 'native'), minecraftVersion: minecraftLive.minecraftVersion, signal: new AbortController().signal, instrument: true });
  const profile = path.join(scratch, 'engine.iprof');
  const profiled = new Engine({ build: async () => instrumented, spawnEngine: (binary, args, options) => spawn(binary, [`-XX:ProfilesDumpFile=${profile}`], options) });
  await profiled.warm({ classpath, binaries: scratch, folder: path.join(scratch, 'profiled'), minecraftVersion: minecraftLive.minecraftVersion });
  await exercise(profiled, scratch);
  const exited = new Promise((resolve) => profiled.child.once('close', resolve));
  profiled.child.stdin.end();
  await exited;
  fs.writeFileSync(path.join(output, PROFILE), zlib.gzipSync(fs.readFileSync(profile), { level: 9 }));
  fs.rmSync(scratch, { recursive: true, force: true });
  console.log(`Recorded ${path.relative(ROOT, output)} from ${experiments} experiments`);
}

main().then(() => process.exit(0), (error) => { console.error(error); process.exit(1); });
