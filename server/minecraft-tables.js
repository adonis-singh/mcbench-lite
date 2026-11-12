'use strict';

// The tables of Minecraft the Rust engine compiles in (include_str! of
// core/minecraft/<version>/), made on this machine from the server jar Paper
// downloaded, by the project's Java reference code on the JDK (bluemap.js:
// JAVA, JAVAC). The repository holds none of them (.gitignore):
//   blocks.txt   the Java engine's dump (McbenchEngine.dump): blocks, states,
//                properties, tags, collision shapes, items
//   loot.json    the server jar's block loot tables, by block key
//   datafix.txt  the server's data fixer: its schemas, types and rules
//                (scripts/DataFixReference.java --dump)
// scripts/core-registry.js and `scripts/datafix-parity.js --dump` write them
// with these functions too, and scripts/prepare-minecraft.js makes all three.

const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { jvmEngine } = require('./engine');
const { readZipEntry } = require('./zip-entry');

const execFileAsync = promisify(execFile);
const TABLES = ['blocks.txt', 'loot.json', 'datafix.txt'];

function tablesFolder(minecraftVersion) {
  return path.join(__dirname, '..', 'core', 'minecraft', minecraftVersion);
}

// The tables of `minecraftVersion` that are not there.
function missingTables(minecraftVersion) {
  return TABLES.filter((name) => !fs.existsSync(path.join(tablesFolder(minecraftVersion), name)));
}

// Fails, saying how to make them, when a table is not there.
function checkTables(minecraftVersion) {
  const missing = missingTables(minecraftVersion);
  if (missing.length) {
    throw new Error(`Minecraft's tables are not made yet (${missing.join(', ')} in core/minecraft/${minecraftVersion}): run \`node scripts/prepare-minecraft.js\`, or start the app once`);
  }
}

// blocks.txt and loot.json from the server's `classpath` (absolute paths)
// for `minecraftVersion`, into `target`; resolves with the count of loot
// tables.
async function writeRegistry({ classpath, minecraftVersion, target, java, javac }) {
  fs.mkdirSync(target, { recursive: true });
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-core-registry-'));
  try {
    fs.mkdirSync(path.join(scratch, 'classes'));
    const engine = jvmEngine({ classpath, classes: path.join(scratch, 'classes'), java, javac });
    const dump = path.join(scratch, 'blocks.txt');
    try {
      await engine.warm({ classpath, binaries: scratch, folder: path.join(scratch, 'engine') });
      await engine.dump(dump);
    } finally {
      await engine.stop();
    }
    // Lines dumped from maps keyed by identity hash come in a different order
    // each run; the engine only looks them up, so each run of them is sorted.
    const unordered = new Set(['compost', 'itemcomponent']);
    const lines = fs.readFileSync(dump, 'utf8').split('\n');
    for (let start = 0; start < lines.length;) {
      const type = lines[start].split('\t')[0];
      let end = start + 1;
      while (end < lines.length && lines[end].split('\t')[0] === type) end += 1;
      if (unordered.has(type)) lines.splice(start, end - start, ...lines.slice(start, end).sort());
      start = end;
    }
    const jar = classpath.find((file) => path.basename(file) === `paper-${minecraftVersion}.jar`);
    const prefix = 'data/minecraft/loot_table/blocks/';
    const names = (await zipNames(jar)).filter((name) => name.startsWith(prefix) && name.endsWith('.json')).sort();
    const loot = {};
    for (const name of names) loot[`minecraft:${name.slice(prefix.length, -'.json'.length)}`] = JSON.parse(readZipEntry(jar, name).toString('utf8'));
    writeWhole(path.join(target, 'blocks.txt'), lines.join('\n'));
    writeWhole(path.join(target, 'loot.json'), `${JSON.stringify(loot)}\n`);
    return names.length;
  } finally {
    fs.rmSync(scratch, { recursive: true, force: true });
  }
}

// DataFixReference (scripts/DataFixReference.java), compiled against the
// server's `classpath` into a scratch folder: `run(args, input)` resolves
// with its process's stdout and stderr.
async function dataFixReference({ classpath, java, javac }) {
  const classes = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-datafix-'));
  await execFileAsync(javac, ['--release', '25', '-proc:none', '-nowarn', '-classpath', classpath.join(path.delimiter), '-d', classes, path.join(__dirname, '..', 'scripts', 'DataFixReference.java')], { timeout: 120_000 });
  return {
    classes,
    run: (args) => execFileAsync(java, ['-Xss16m', '-cp', [...classpath, classes].join(path.delimiter), 'DataFixReference', ...args], { cwd: classes, maxBuffer: 1 << 30, timeout: 600_000 }),
  };
}

// datafix.txt from the server's `classpath`, into `target`.
async function writeDataFix({ classpath, target, java, javac }) {
  fs.mkdirSync(target, { recursive: true });
  const reference = await dataFixReference({ classpath, java, javac });
  try {
    const file = path.join(target, 'datafix.txt');
    await reference.run(['--dump', `${file}.part`]);
    fs.renameSync(`${file}.part`, file);
  } finally {
    fs.rmSync(reference.classes, { recursive: true, force: true });
  }
}

// The entries of a zip file, by name.
async function zipNames(jar) {
  const { stdout } = await execFileAsync('unzip', ['-Z1', jar], { maxBuffer: 1 << 26 });
  return stdout.split('\n');
}

// Writes `file` so that it appears whole (a rename).
function writeWhole(file, text) {
  fs.writeFileSync(`${file}.part`, text);
  fs.renameSync(`${file}.part`, file);
}

module.exports = { TABLES, tablesFolder, missingTables, checkTables, writeRegistry, writeDataFix, dataFixReference };
