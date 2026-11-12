'use strict';

// Opens an exported world (web/world-export.mjs) in the pinned Paper server
// and asks it about the build: the engine places a build with a lever, wire,
// a lamp, a chest and water, runs ticks, exports; the zip is unpacked as a
// server's world folder, Paper loads it (and upgrades it as it would any
// save), and RCON checks blocks, a block entity and that scheduled ticks
// carry on. Run against a scratch data folder:
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/export-check.js [--keep folder] [--zip file]
//
// With --zip, the zip is one the viewer's export button saved: Paper loads it
// and agrees with the region reader (server/region.js) on 200 of its blocks.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { spawn, execFileSync } = require('child_process');
const { minecraftLive } = require('../server/minecraft-live');
const { RconConnection } = require('../server/rcon');
const { graalvmHome } = require('../server/engine');
const { build } = require('./build-wasm');

const COMMANDS = [
  'fill -20 63 -20 20 63 20 minecraft:stone',
  'setblock 1 64 1 minecraft:lever[face=floor,facing=north,powered=false]',
  'setblock 2 64 1 minecraft:redstone_wire',
  'setblock 3 64 1 minecraft:redstone_lamp',
  'setblock 3 64 3 minecraft:chest[facing=north]',
  'setblock 18 65 18 minecraft:water',
  'setblock 8 64 8 minecraft:repeater[facing=east,delay=4]',
  'setblock 9 64 8 minecraft:redstone_block',
].map((line) => `${line}\n`).join('');

async function main() {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-export-check-'));
  const given = process.argv.indexOf('--zip');
  let zip = given >= 0 ? path.resolve(process.argv[given + 1]) : path.join(scratch, 'world.zip');
  if (given < 0) {
    const { CoreWorld } = await import('../web/core-world.mjs');
    const { worldZip } = await import('../web/world-export.mjs');
    const world = new CoreWorld(await WebAssembly.compile(fs.readFileSync(await build())));
    world.place(COMMANDS);
    world.input('toggle', 1, 64, 1, 0);
    world.tick(9);
    const save = world.export();
    const { bytes } = await worldZip({ name: 'mcbench export check', gameTime: save.gameTime, chunks: save.chunks }, [0, 80, 0]);
    fs.writeFileSync(zip, bytes);
  }
  const folder = execFileSync('unzip', ['-Z1', zip], { encoding: 'utf8' }).split('\n')[0].split('/')[0];
  const server = path.join(scratch, 'server');
  fs.mkdirSync(server);
  execFileSync('unzip', ['-q', zip, '-d', server]);
  // Paper's files are shared, not copied: its libraries and jars are read-only here.
  const classpath = await minecraftLive.ensurePaper();
  for (const name of ['libraries', 'versions']) fs.symlinkSync(path.join(minecraftLive.paperDir, name), path.join(server, name));
  const jar = classpath.find((file) => /paper-.*\.jar$/.test(file) && !file.includes('/'));
  if (jar) fs.symlinkSync(path.join(minecraftLive.paperDir, jar), path.join(server, jar));
  const rconPort = 25000 + Math.floor(Math.random() * 3000);
  const gamePort = rconPort + 3000;
  fs.writeFileSync(path.join(server, 'server.properties'), `level-name=${folder}\nonline-mode=false\nenable-rcon=true\nrcon.password=check\nrcon.port=${rconPort}\nserver-port=${gamePort}\nserver-ip=127.0.0.1\ngenerate-structures=false\nlevel-type=flat\ngenerator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n`);
  fs.writeFileSync(path.join(server, 'eula.txt'), 'eula=true\n');
  const child = spawn(path.join(graalvmHome(), 'bin', 'java'), ['-Xmx2G', '-cp', classpath.join(path.delimiter), 'org.bukkit.craftbukkit.Main', '--nogui'], { cwd: server, stdio: ['ignore', 'pipe', 'pipe'] });
  let output = '';
  child.stdout.on('data', (chunk) => { output += chunk; });
  child.stderr.on('data', (chunk) => { output += chunk; });
  try {
    for (const deadline = Date.now() + 240_000; !/Done \([0-9.]+s\)!/.test(output);) {
      if (child.exitCode !== null) throw new Error(`Paper exited: ${output.slice(-2000)}`);
      if (Date.now() > deadline) throw new Error(`Paper did not start: ${output.slice(-2000)}`);
      await new Promise((resolve) => setTimeout(resolve, 200));
    }
    const rcon = await RconConnection.connect('127.0.0.1', rconPort, 'check', 10_000);
    const run = (command) => rcon.exec(command, 30_000);
    await run('forceload add -32 -32 32 32');
    await new Promise((resolve) => setTimeout(resolve, 1500));
    if (given >= 0) return await checkGiven(run, path.join(server, folder), rcon, output);
    const checks = [
      ['the lever the player pulled stays pulled', 'execute if block 1 64 1 minecraft:lever[powered=true]'],
      ['the wire carries it', 'execute if block 2 64 1 minecraft:redstone_wire[power=15]'],
      ['the lamp is lit', 'execute if block 3 64 1 minecraft:redstone_lamp[lit=true]'],
      ['the chest is a block entity', 'data get block 3 64 3 id'],
      ['the stone floor runs over the chunk borders', 'execute if blocks -20 63 -20 20 63 20 -20 63 -20 all'],
      ['the corner of the floor', 'execute if block -20 63 -20 minecraft:stone'],
      ['beyond the build is void', 'execute if block 60 63 60 minecraft:air'],
    ];
    const results = [];
    for (const [what, command] of checks) {
      // The server settles what it loaded (a wire it relights as its chunk loads): a check is asked until it passes, for up to 5 seconds.
      let answer;
      let ok = false;
      for (let tries = 0; tries < 10 && !ok; tries += 1) {
        if (tries) await new Promise((resolve) => setTimeout(resolve, 500));
        answer = await run(command);
        ok = /Test passed|Block has the following|has the following block entity data|"minecraft:chest"/i.test(answer);
      }
      results.push({ what, ok, answer: answer.slice(0, 120) });
    }
    console.table(results);
    // Scheduled ticks carry on: the water spreads from its source.
    await run('tick rate 20');
    await new Promise((resolve) => setTimeout(resolve, 2500));
    const spread = await run('execute if block 19 64 18 minecraft:water');
    console.log('water has spread from the source after loading:', /Test passed/.test(spread), spread);
    const failed = results.filter((result) => !result.ok);
    rcon.close();
    if (failed.length) console.log(output.split('\n').filter((line) => /WARN|ERROR|level|chunk|region|Exception/i.test(line)).slice(0, 60).join('\n'));
    if (failed.length) throw new Error(`Paper disagrees: ${JSON.stringify(failed)}`);
    if (!/Loading|Preparing|Done/.test(output)) throw new Error(output.slice(-1500));
    console.log(`Paper ${minecraftLive.minecraftVersion} opened the world and every check passed`);
    const bad = output.split('\n').filter((line) => /ERROR|Exception|Failed to load|corrupt/i.test(line));
    if (bad.length) console.log('server log problems:\n', bad.slice(0, 10).join('\n'));
    const keep = process.argv.indexOf('--keep');
    if (keep >= 0) fs.cpSync(zip, path.join(process.argv[keep + 1], 'mcbench-export-check.zip'));
  } finally {
    child.kill('SIGTERM');
    await new Promise((resolve) => { child.once('exit', resolve); setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 20_000); });
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

// A world the export button saved: Paper loads it and agrees with the
// region reader on a sample of its blocks.
async function checkGiven(run, world, rcon, output) {
  const { readBlockStates } = require('../server/region');
  const states = readBlockStates(path.join(world, 'region'));
  const entries = [...states.entries()];
  const xs = entries.map(([position]) => position.split(',').map(Number));
  const [min, max] = [0, 1, 2].map((axis) => [Math.min(...xs.map((p) => p[axis])), Math.max(...xs.map((p) => p[axis]))]).reduce((acc, [lo, hi]) => [[...acc[0], lo], [...acc[1], hi]], [[], []]);
  await run(`forceload add ${min[0]} ${min[2]} ${max[0]} ${max[2]}`);
  await new Promise((resolve) => setTimeout(resolve, 3000));
  let agreed = 0;
  const step = Math.max(1, Math.floor(entries.length / 200));
  const disagreements = [];
  for (let i = 0; i < entries.length; i += step) {
    const [position, state] = entries[i];
    const answer = await run(`execute if block ${position.split(',').join(' ')} ${state}`);
    if (/Test passed/.test(answer)) agreed += 1;
    else disagreements.push([position, state, answer]);
  }
  rcon.close();
  console.log(`Paper agrees with the region reader on ${agreed} of ${Math.ceil(entries.length / step)} sampled blocks of ${entries.length}`);
  if (disagreements.length) throw new Error(`Paper disagrees: ${JSON.stringify(disagreements.slice(0, 5))}`);
  console.log('log problems:', output.split('\n').filter((line) => /ERROR|Exception|Failed to load|corrupt/i.test(line)).length);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
