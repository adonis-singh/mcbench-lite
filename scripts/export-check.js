'use strict';

// Whether what a person gets in Minecraft from each export is the build as
// the app shows it at tick 0, block for block and state for state, with its
// block entities, in the pinned Paper server. The tick-0 world is the
// app's: the world the engine holds once it has placed the build
// (shot-world.js settle at 0 ticks, the live world's start). For each build
// (fixtures whose blocks Minecraft connects or joins, and with
// MCBENCH_CORPUS every --sample'th build of the corpus by name, read only):
//   world save   the live world's export (web/world-export.mjs) of the
//                engine's world right after the placement, loaded by Paper
//   schematic    the chat's schematic (server/schematic.js buildSchematic),
//                pasted without updates (setblock ... strict, as a paste
//                that sets states as written does)
//   authored     the schematic as it was written before it took the tick-0
//                world (the build's blocks as the model wrote them), pasted
//                the same way: how far that was from what the app shows
// The builds stand side by side, far enough apart that none reaches
// another: the world save holds them all, and each schematic is pasted in
// a band of its own beside it. Paper's ticks are frozen before any of them
// loads, the pastes run as a data pack's functions, and Paper's own save of
// the world is read back and compared, block entities by id. Run against a
// scratch data folder:
//
//   MCBENCH_DATA_DIR=<scratch copy> [MCBENCH_CORPUS=<dir>] node scripts/export-check.js [--sample 6]
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/export-check.js --ticked
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/export-check.js --zip <file>
//
// --ticked exports the live world after a lever is pulled and ticks run (the
// world as it stands when the export button is pressed) and checks it in
// Paper. With --zip, the zip is one the viewer's export button saved: Paper
// loads it and agrees with the region reader (server/region.js) on 200 of
// its blocks.

const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { spawn, execFileSync } = require('child_process');
const { minecraftLive } = require('../server/minecraft-live');
const { RconConnection } = require('../server/rcon');
const { freePort } = require('../server/paper-server');
const { graalvmHome } = require('../server/engine');
const { World } = require('../server/world');
const { importCommands, blockSpec } = require('../server/live-placement');
const { writeSchematic, buildSchematic } = require('../server/schematic');
const { settle, stopShotEngines } = require('../server/shot-world');
const { readNbt, readChunks, readBlockStates } = require('../server/region');
const { corpusArtifacts } = require('./corpus');
const { build } = require('./build-wasm');

const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};

// Builds whose blocks take their states from their neighbours, or come in
// parts, as a model writes them: every state the default one.
const FIXTURES = {
  connections(world) {
    world.fill([0, 0, 0], [12, 0, 8], 'minecraft:stone');
    world.fill([0, 1, 0], [4, 1, 0], 'minecraft:glass_pane');
    world.fill([0, 2, 0], [4, 2, 0], 'minecraft:iron_bars');
    world.fill([0, 1, 2], [4, 1, 2], 'minecraft:oak_fence');
    world.set(2, 1, 3, 'minecraft:oak_fence');
    world.fill([0, 1, 4], [4, 1, 4], 'minecraft:cobblestone_wall');
    world.set(2, 2, 4, 'minecraft:cobblestone_wall');
    world.fill([6, 1, 0], [10, 1, 0], 'minecraft:redstone_wire');
    world.set(8, 1, 1, 'minecraft:redstone_wire');
    world.set(10, 1, 1, 'minecraft:redstone_torch');
    world.set(6, 1, 2, 'minecraft:oak_stairs', { facing: 'north' });
    world.set(7, 1, 2, 'minecraft:oak_stairs', { facing: 'east' });
    world.set(6, 1, 3, 'minecraft:oak_stairs', { facing: 'east' });
    world.fill([8, 1, 3], [10, 1, 3], 'minecraft:glass_pane');
    world.set(9, 1, 4, 'minecraft:glass_pane');
  },
  parts(world) {
    world.fill([0, 0, 0], [10, 0, 6], 'minecraft:grass_block');
    world.set(0, 1, 0, 'minecraft:oak_door', { facing: 'south', half: 'lower' });
    world.set(0, 2, 0, 'minecraft:oak_door', { facing: 'south', half: 'upper' });
    world.set(2, 1, 0, 'minecraft:red_bed', { facing: 'south', part: 'foot' });
    world.set(2, 1, 1, 'minecraft:red_bed', { facing: 'south', part: 'head' });
    world.set(4, 1, 0, 'minecraft:tall_grass', { half: 'lower' });
    world.set(4, 2, 0, 'minecraft:tall_grass', { half: 'upper' });
    world.set(5, 1, 0, 'minecraft:sunflower', { half: 'lower' });
    world.set(5, 2, 0, 'minecraft:sunflower', { half: 'upper' });
    world.set(6, 1, 0, 'minecraft:chest', { facing: 'north' });
    world.set(7, 1, 0, 'minecraft:chest', { facing: 'north' });
    world.set(8, 1, 0, 'minecraft:oak_stairs', { facing: 'north', waterlogged: 'true' });
    world.set(9, 1, 0, 'minecraft:oak_slab', { type: 'bottom', waterlogged: 'true' });
    world.set(0, 1, 3, 'minecraft:piston', { facing: 'east', extended: 'true' });
    world.set(1, 1, 3, 'minecraft:piston_head', { facing: 'east', type: 'normal' });
    world.set(0, 1, 4, 'minecraft:redstone_block');
    world.set(3, 1, 3, 'minecraft:sticky_piston', { facing: 'up', extended: 'true' });
    world.set(3, 2, 3, 'minecraft:piston_head', { facing: 'up', type: 'sticky' });
    world.set(3, 1, 4, 'minecraft:redstone_block');
    world.set(6, 1, 3, 'minecraft:torch');
    world.set(7, 1, 3, 'minecraft:wall_torch', { facing: 'north' });
    world.set(8, 1, 3, 'minecraft:lever', { face: 'floor', facing: 'north' });
    world.set(8, 1, 4, 'minecraft:redstone_lamp');
  },
};

function fixtures() {
  return Object.entries(FIXTURES).map(([name, make]) => {
    const world = new World();
    make(world);
    return { name, world: world.finalize() };
  });
}

function corpus(every) {
  const builds = [...new Map(corpusArtifacts())].sort(([a], [b]) => a.localeCompare(b));
  return builds.filter((_, i) => i % every === 0).map(([name, file]) => {
    const artifact = JSON.parse(fs.readFileSync(file, 'utf8'));
    const world = new World();
    for (const block of artifact.blocks) world.set(block.pos[0], block.pos[1], block.pos[2], block.id, block.states || {});
    return { name, world: world.finalize() };
  });
}

// `world` moved by `by` ([x, y, z]).
function moved(world, by) {
  const out = new World();
  world.forEach((x, y, z, entry) => out.set(x + by[0], y + by[1], z + by[2], entry.id, entry.states));
  return out.finalize();
}

// A state as both sides name it: properties sorted the one way.
const canonical = (spec) => {
  const open = spec.indexOf('[');
  if (open < 0) return spec;
  return `${spec.slice(0, open)}[${spec.slice(open + 1, -1).split(',').sort().join(',')}]`;
};

// "x,y,z" -> state of a World at Minecraft height.
function statesOf(world, yOffset) {
  const states = new Map();
  world.forEach((x, y, z, entry) => states.set(`${x},${y + yOffset},${z}`, canonical(blockSpec(entry))));
  return states;
}

// "x,y,z" -> block entity id of chunks' NBT (the engine's export or Paper's save).
function blockEntitiesOf(chunks) {
  const out = new Map();
  for (const chunk of chunks) for (const entity of chunk.block_entities || []) out.set(`${entity.x},${entity.y},${entity.z}`, entity.id);
  return out;
}

// The non-air blocks of a Sponge v3 schematic: [[x, y, z, state]] from its corner.
function schematicBlocks(bytes) {
  const { Schematic: schematic } = readNbt(zlib.gunzipSync(bytes));
  const [width, length] = [schematic.Width, schematic.Length];
  const palette = new Map(Object.entries(schematic.Blocks.Palette).map(([spec, index]) => [index, spec]));
  const blocks = [];
  const data = schematic.Blocks.Data;
  for (let at = 0, cell = 0; at < data.length; cell += 1) {
    let value = 0;
    for (let shift = 0; ; shift += 7) {
      const byte = data[at++];
      value |= (byte & 0x7f) << shift;
      if (!(byte & 0x80)) break;
    }
    const spec = palette.get(value);
    if (spec !== 'minecraft:air') blocks.push([cell % width, Math.floor(cell / (width * length)), Math.floor(cell / width) % length, spec]);
  }
  return blocks;
}

// Where each build goes: side by side along x, chunk-aligned, 64 blocks
// apart, its lowest z at 0. A schematic band is the same at another z.
const GAP = 64;
const BANDS = { world: 0, schematic: 8192, authored: 16384 };

async function compareExports(paperRun) {
  const sample = Number(option('sample', 6));
  const builds = [...fixtures(), ...(process.env.MCBENCH_CORPUS ? corpus(sample) : [])];
  const { yOffset } = minecraftLive;
  const placedBuilds = [];
  let cursor = 0;
  for (const [i, { name, world }] of builds.entries()) {
    const bounds = world.bounds();
    const by = [cursor - bounds.min[0], 0, -bounds.min[2]];
    const shifted = moved(world, by);
    const id = `export-check-${i}`;
    let placed;
    try {
      placed = (await settle(id, shifted, [0])).get(0).world;
    } catch (error) {
      console.log(`${name}: the engine does not place it (${String(error.message || error).slice(0, 160)}), left out`);
      continue;
    }
    const box = shifted.bounds();
    placedBuilds.push({ name, id, authored: shifted, placed, box, schematic: await buildSchematic(id, shifted), authoredSchematic: writeSchematic(shifted) });
    cursor = Math.ceil((box.max[0] + 1 + GAP) / 16) * 16;
  }

  // The world save: the live world's engine places every build at once and exports before any tick.
  const { CoreWorld } = await import('../web/core-world.mjs');
  const { worldZip } = await import('../web/world-export.mjs');
  const engine = new CoreWorld(await WebAssembly.compile(fs.readFileSync(await build())));
  const all = new World();
  for (const { authored } of placedBuilds) authored.forEach((x, y, z, entry) => all.set(x, y, z, entry.id, entry.states));
  all.finalize();
  engine.place(importCommands(all, null, yOffset).commands.map((line) => `${line}\n`).join(''));
  const save = engine.export();
  const engineEntities = blockEntitiesOf(save.chunks.map(({ nbt }) => readNbt(Buffer.from(nbt))));
  // The spawn is far from the builds: no chunk of theirs loads before Paper's ticks are frozen.
  const { folder, bytes } = await worldZip({ name: 'mcbench export check', gameTime: save.gameTime, chunks: save.chunks }, [0, 80, -60000]);

  // Each schematic pasted without updates, as a data pack function per band.
  const functions = new Map();
  const pastes = { schematic: [], authored: [] };
  for (const build of placedBuilds) {
    for (const [band, bytesOf, corner] of [['schematic', build.schematic, build.placed.bounds().min], ['authored', build.authoredSchematic, build.authored.bounds().min]]) {
      for (const [x, y, z, spec] of schematicBlocks(bytesOf)) pastes[band].push(`setblock ${corner[0] + x} ${corner[1] + y + yOffset} ${corner[2] + z + BANDS[band]} ${spec} strict`);
    }
  }
  for (const band of Object.keys(pastes)) functions.set(`paste_${band}`, pastes[band]);

  const extent = { min: [Math.min(...placedBuilds.map((b) => b.box.min[0])), Math.min(...placedBuilds.map((b) => b.box.min[2]))], max: [Math.max(...placedBuilds.map((b) => b.box.max[0])), Math.max(...placedBuilds.map((b) => b.box.max[2]))] };
  const saved = await paperRun({ zip: bytes, folder, functions, extent });

  const results = [];
  const shift = (states, dz) => new Map([...states].map(([key, state]) => {
    const [x, y, z] = key.split(',').map(Number);
    return [`${x},${y},${z + dz}`, state];
  }));
  for (const build of placedBuilds) {
    const reference = statesOf(build.placed, yOffset);
    const inBox = (dz) => (key) => {
      const [x, , z] = key.split(',').map(Number);
      return x >= build.box.min[0] - 1 && x <= build.box.max[0] + 1 && z >= build.box.min[2] + dz - 1 && z <= build.box.max[2] + dz + 1;
    };
    const differences = (dz) => {
      const want = shift(reference, dz);
      const got = new Map([...saved.states].filter(([key]) => inBox(dz)(key)));
      let differ = 0;
      const examples = [];
      for (const key of new Set([...want.keys(), ...got.keys()])) {
        const a = want.get(key) ?? 'minecraft:air';
        const b = got.has(key) ? canonical(got.get(key)) : 'minecraft:air';
        if (a === b) continue;
        differ += 1;
        if (examples.length < 3) examples.push(`${key} ${a.replace('minecraft:', '')} -> ${b.replace('minecraft:', '')}`);
      }
      const wantEntities = shift(new Map([...engineEntities].filter(([key]) => inBox(0)(key))), dz);
      const gotEntities = new Map([...saved.entities].filter(([key]) => inBox(dz)(key)));
      let entities = 0;
      for (const key of new Set([...wantEntities.keys(), ...gotEntities.keys()])) if (wantEntities.get(key) !== gotEntities.get(key)) entities += 1;
      return { differ, entities, examples };
    };
    const world = differences(BANDS.world);
    const schematic = differences(BANDS.schematic);
    const authored = differences(BANDS.authored);
    results.push({
      build: build.name, blocks: reference.size, entities: [...engineEntities.keys()].filter(inBox(0)).length,
      'world save differs': `${world.differ} (+${world.entities} BE)`,
      'schematic differs': `${schematic.differ} (+${schematic.entities} BE)`,
      'authored schematic differed': `${authored.differ} (+${authored.entities} BE)`,
      example: [...world.examples, ...schematic.examples].join('; ') || authored.examples[0] || '',
    });
  }
  console.table(results);
  await stopShotEngines();
  const failed = results.filter((row) => !/^0 \(\+0 BE\)$/.test(row['world save differs']) || !/^0 \(\+0 BE\)$/.test(row['schematic differs']));
  if (failed.length) throw new Error(`Paper's world differs from the app's tick-0 world for ${failed.map((row) => row.build).join(', ')}`);
  console.log(`Paper ${minecraftLive.minecraftVersion}: every export of ${results.length} builds is the app's tick-0 world block for block`);
}

// A Paper server on the world in `zip` (folder `folder` in it), with the
// data pack functions `functions` (name -> commands) beside it. Its ticks
// are frozen, the chunks over `extent` (x and z, every band) are loaded,
// the functions run, and Paper's own save is read: { states, entities }.
function frozenPaper(scratch) {
  return async ({ zip, folder, functions, extent }) => {
    const server = path.join(scratch, 'server');
    const zipFile = path.join(scratch, 'world.zip');
    fs.writeFileSync(zipFile, zip);
    fs.mkdirSync(server);
    execFileSync('unzip', ['-q', zipFile, '-d', server]);
    const pack = path.join(server, folder, 'datapacks', 'export_check');
    fs.mkdirSync(path.join(pack, 'data', 'export_check', 'function'), { recursive: true });
    fs.writeFileSync(path.join(pack, 'pack.mcmeta'), JSON.stringify({ pack: { description: 'export check pastes', min_format: [88, 0], max_format: [88, 0] } }));
    for (const [name, lines] of functions) fs.writeFileSync(path.join(pack, 'data', 'export_check', 'function', `${name}.mcfunction`), `${lines.join('\n')}\n`);
    return withPaper(server, folder, async (run) => {
      await run('tick freeze');
      await run('gamerule maxCommandChainLength 2147483647');
      await run('gamerule maxCommandForkCount 2147483647');
      const chunks = [];
      for (const dz of Object.values(BANDS)) {
        const [x1, x2] = [Math.floor(extent.min[0] / 16) - 1, Math.floor(extent.max[0] / 16) + 1];
        const [z1, z2] = [Math.floor((extent.min[1] + dz) / 16) - 1, Math.floor((extent.max[1] + dz) / 16) + 1];
        for (let cx = x1; cx <= x2; cx += 1) for (let cz = z1; cz <= z2; cz += 1) chunks.push([cx, cz]);
        // forceload takes 256 chunks at most at once.
        for (let cx = x1; cx <= x2; cx += 16) {
          for (let cz = z1; cz <= z2; cz += 16) {
            const answer = await run(`forceload add ${cx * 16} ${cz * 16} ${Math.min(cx + 15, x2) * 16} ${Math.min(cz + 15, z2) * 16}`);
            if (!/[Mm]arked|already/.test(answer)) throw new Error(`forceload: ${answer}`);
          }
        }
      }
      // Every chunk is loaded once a block in each answers.
      for (const [cx, cz] of chunks) {
        for (let tries = 0; !/Test passed|Test failed/.test(await run(`execute if block ${cx * 16} 0 ${cz * 16} minecraft:air`)); tries += 1) {
          if (tries > 200) throw new Error(`Chunk ${cx}, ${cz} did not load`);
          await new Promise((resolve) => setTimeout(resolve, 50));
        }
      }
      for (const name of functions.keys()) {
        const answer = await run(`function export_check:${name}`);
        if (!/Executed \d+ command|Running function/.test(answer)) throw new Error(`function ${name}: ${answer}`);
      }
      await run('save-all flush');
      const region = path.join(server, folder, 'region');
      return { states: readBlockStates(region), entities: blockEntitiesOf(readChunks(region)) };
    });
  };
}

// Runs `work(run)` against a Paper server in `server` on world `folder`,
// `run` an RCON command; stops the server after.
async function withPaper(server, folder, work) {
  // Paper's files are shared, not copied: its libraries and jars are read-only here.
  const classpath = await minecraftLive.ensurePaper();
  for (const name of ['libraries', 'versions']) fs.symlinkSync(path.join(minecraftLive.paperDir, name), path.join(server, name));
  const jar = classpath.find((file) => /paper-.*\.jar$/.test(file) && !file.includes('/'));
  if (jar) fs.symlinkSync(path.join(minecraftLive.paperDir, jar), path.join(server, jar));
  // Ports the system gives as free, as the verifier's Paper servers take
  // theirs, so this server meets no other on this machine.
  const rconPort = await freePort('127.0.0.1');
  const gamePort = await freePort('127.0.0.1');
  fs.writeFileSync(path.join(server, 'server.properties'), `level-name=${folder}\nonline-mode=false\nenable-rcon=true\nrcon.password=check\nrcon.port=${rconPort}\nserver-port=${gamePort}\nserver-ip=127.0.0.1\ngenerate-structures=false\nlevel-type=flat\ngenerator-settings={"layers":[{"block":"minecraft:air","height":1}],"biome":"minecraft:plains"}\n`);
  fs.writeFileSync(path.join(server, 'eula.txt'), 'eula=true\n');
  const child = spawn(path.join(graalvmHome(), 'bin', 'java'), ['-Xmx4G', '-cp', classpath.join(path.delimiter), 'org.bukkit.craftbukkit.Main', '--nogui'], { cwd: server, stdio: ['ignore', 'pipe', 'pipe'] });
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
    try {
      return await work((command) => rcon.exec(command, 120_000), output);
    } finally {
      rcon.close();
      const bad = output.split('\n').filter((line) => /ERROR|Exception|Failed to load|corrupt/i.test(line));
      if (bad.length) console.log('server log problems:\n', bad.slice(0, 10).join('\n'));
    }
  } finally {
    child.kill('SIGTERM');
    await new Promise((resolve) => { child.once('exit', resolve); setTimeout(() => { child.kill('SIGKILL'); resolve(); }, 20_000); });
  }
}

const TICKED = [
  'fill -20 63 -20 20 63 20 minecraft:stone',
  'setblock 1 64 1 minecraft:lever[face=floor,facing=north,powered=false]',
  'setblock 2 64 1 minecraft:redstone_wire',
  'setblock 3 64 1 minecraft:redstone_lamp',
  'setblock 3 64 3 minecraft:chest[facing=north]',
  'setblock 18 65 18 minecraft:water',
  'setblock 8 64 8 minecraft:repeater[facing=east,delay=4]',
  'setblock 9 64 8 minecraft:redstone_block',
].map((line) => `${line}\n`).join('');

// The world as it stands after a lever is pulled and ticks run: Paper loads
// it, the blocks and a block entity are as the engine left them, and its
// scheduled ticks carry on.
async function checkTicked(scratch) {
  const { CoreWorld } = await import('../web/core-world.mjs');
  const { worldZip } = await import('../web/world-export.mjs');
  const world = new CoreWorld(await WebAssembly.compile(fs.readFileSync(await build())));
  world.place(TICKED);
  world.input('toggle', 1, 64, 1, 0);
  world.tick(9);
  const save = world.export();
  const { folder, bytes } = await worldZip({ name: 'mcbench export check', gameTime: save.gameTime, chunks: save.chunks }, [0, 80, 0]);
  const server = path.join(scratch, 'server');
  fs.writeFileSync(path.join(scratch, 'world.zip'), bytes);
  fs.mkdirSync(server);
  execFileSync('unzip', ['-q', path.join(scratch, 'world.zip'), '-d', server]);
  await withPaper(server, folder, async (run) => {
    await run('forceload add -32 -32 32 32');
    await new Promise((resolve) => setTimeout(resolve, 1500));
    const checks = [
      ['the lever the player pulled stays pulled', 'execute if block 1 64 1 minecraft:lever[powered=true]'],
      ['the wire carries it', 'execute if block 2 64 1 minecraft:redstone_wire[power=15]'],
      ['the lamp is lit', 'execute if block 3 64 1 minecraft:redstone_lamp[lit=true]'],
      ['the chest is a block entity', 'data get block 3 64 3 id'],
      ['the stone floor runs over the chunk borders', 'execute if blocks -20 63 -20 20 63 20 -20 63 -20 all'],
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
    console.log('water has spread from the source after loading:', /Test passed/.test(spread));
    const failed = results.filter((result) => !result.ok);
    if (failed.length) throw new Error(`Paper disagrees: ${JSON.stringify(failed)}`);
  });
}

// A world the export button saved: Paper loads it and agrees with the
// region reader on a sample of its blocks.
async function checkGiven(scratch, zip) {
  const folder = execFileSync('unzip', ['-Z1', zip], { encoding: 'utf8' }).split('\n')[0].split('/')[0];
  const server = path.join(scratch, 'server');
  fs.mkdirSync(server);
  execFileSync('unzip', ['-q', zip, '-d', server]);
  await withPaper(server, folder, async (run, output) => {
    const states = readBlockStates(path.join(server, folder, 'region'));
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
    console.log(`Paper agrees with the region reader on ${agreed} of ${Math.ceil(entries.length / step)} sampled blocks of ${entries.length}`);
    if (disagreements.length) throw new Error(`Paper disagrees: ${JSON.stringify(disagreements.slice(0, 5))}`);
    console.log('log problems:', output.split('\n').filter((line) => /ERROR|Exception|Failed to load|corrupt/i.test(line)).length);
  });
}

async function main() {
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-export-check-'));
  try {
    const zip = option('zip', null);
    if (zip) await checkGiven(scratch, path.resolve(zip));
    else if (process.argv.includes('--ticked')) await checkTicked(scratch);
    else await compareExports(frozenPaper(scratch));
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

main().catch(async (error) => {
  console.error(error);
  await stopShotEngines();
  process.exit(1);
});
