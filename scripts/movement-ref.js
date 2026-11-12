'use strict';

// The reference for the player's movement: Mojang's own 1.21.10 client,
// downloaded from Mojang's version manifest into the data folder (never into
// the repository), its class files renamed to Mojang's official names with
// Mojang's mappings (tiny-remapper, from Fabric's maven), and the Java side of
// scripts/movement-parity.js (MovementReference.java and the classes beside
// it) compiled against it. Nothing of the client is rewritten: the renamed
// classes run headless.
//
//   const ref = await prepare(dataDir); ref.run(casesFile) -> the client's trace
//   node scripts/movement-ref.js [--sources]   prepares (and decompiles with --sources, for reading)

const crypto = require('crypto');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync } = require('child_process');

const VERSION = '1.21.10';
const MANIFEST = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json';
const TINY_REMAPPER = 'https://maven.fabricmc.net/net/fabricmc/tiny-remapper/0.11.0/tiny-remapper-0.11.0-fat.jar';
const VINEFLOWER = 'https://repo1.maven.org/maven2/org/vineflower/vineflower/1.11.1/vineflower-1.11.1.jar';
const SOURCES = ['MovementReference.java', 'MovementLevel.java', 'MovementPlayer.java'].map((name) => path.join(__dirname, name));

async function download(url, target, sha1) {
  if (fs.existsSync(target)) return;
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  if (sha1 && crypto.createHash('sha1').update(bytes).digest('hex') !== sha1) throw new Error(`${url}: the download does not match its published hash`);
  fs.writeFileSync(`${target}.part`, bytes);
  fs.renameSync(`${target}.part`, target);
}

// GraalVM 25 (the JVM the engine's parity runs use), as server/engine.js finds it.
function java() {
  let home = process.env.MCBENCH_GRAALVM_HOME;
  const unpacked = path.join(os.homedir(), '.local', 'graalvm');
  if (!home && fs.existsSync(unpacked)) {
    const release = fs.readdirSync(unpacked).filter((name) => name.startsWith('graalvm')).sort().at(-1);
    if (release) home = path.join(unpacked, release, process.platform === 'darwin' ? 'Contents/Home' : '');
  }
  if (!home) throw new Error('GraalVM 25 is not unpacked under ~/.local/graalvm (or set MCBENCH_GRAALVM_HOME)');
  return { java: path.join(home, 'bin', 'java'), javac: path.join(home, 'bin', 'javac') };
}

async function prepare(dataDir = process.env.MCBENCH_DATA_DIR, { sources = false } = {}) {
  if (!dataDir) throw new Error('MCBENCH_DATA_DIR is not set');
  const folder = path.join(dataDir, 'minecraft-client');
  fs.mkdirSync(path.join(folder, 'tools'), { recursive: true });
  const { java: javaBin, javac } = java();

  const manifest = JSON.parse(await (await fetch(MANIFEST)).text());
  const entry = manifest.versions.find((version) => version.id === VERSION);
  const versionFile = path.join(folder, 'version.json');
  if (!fs.existsSync(versionFile)) fs.writeFileSync(versionFile, await (await fetch(entry.url)).text());
  const version = JSON.parse(fs.readFileSync(versionFile, 'utf8'));
  await download(version.downloads.client.url, path.join(folder, 'client.jar'), version.downloads.client.sha1);
  await download(version.downloads.client_mappings.url, path.join(folder, 'client.txt'), version.downloads.client_mappings.sha1);
  const remapper = path.join(folder, 'tools', 'tiny-remapper.jar');
  await download(TINY_REMAPPER, remapper);

  const mojmap = path.join(folder, 'client-mojmap.jar');
  if (!fs.existsSync(mojmap)) {
    execFileSync('node', [path.join(__dirname, 'proguard-to-tiny.js'), path.join(folder, 'client.txt'), path.join(folder, 'client.tiny')]);
    execFileSync(javaBin, ['-jar', remapper, path.join(folder, 'client.jar'), `${mojmap}.part`, path.join(folder, 'client.tiny'), 'official', 'named'], { stdio: 'ignore' });
    fs.renameSync(`${mojmap}.part`, mojmap);
  }
  if (sources) {
    const vineflower = path.join(folder, 'tools', 'vineflower.jar');
    await download(VINEFLOWER, vineflower);
    const classes = path.join(folder, 'cls');
    if (!fs.existsSync(classes)) {
      fs.mkdirSync(classes);
      execFileSync('unzip', ['-q', '-o', mojmap, 'net/minecraft/*', '-d', classes]);
    }
    fs.mkdirSync(path.join(folder, 'src'), { recursive: true });
    execFileSync(javaBin, ['-jar', vineflower, '-dgs=1', '-log=ERROR', classes, path.join(folder, 'src')], { stdio: 'inherit' });
  }

  // The libraries Minecraft runs on are the ones the pinned server brings.
  const libraries = [];
  const walk = (directory) => {
    for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
      const file = path.join(directory, entry.name);
      if (entry.isDirectory()) walk(file);
      else if (file.endsWith('.jar') && !/netty-transport-native|jline|maven-|mysql|msal|adventure|configurate|bungee/.test(entry.name)) libraries.push(file);
    }
  };
  walk(path.join(dataDir, 'minecraft-live', 'paper', 'libraries'));

  const classes = path.join(folder, 'classes');
  const stamp = path.join(classes, '.built');
  const newest = Math.max(...SOURCES.map((file) => fs.statSync(file).mtimeMs), fs.statSync(mojmap).mtimeMs);
  if (!fs.existsSync(stamp) || fs.statSync(stamp).mtimeMs < newest) {
    fs.mkdirSync(classes, { recursive: true });
    try {
      execFileSync(javac, ['-nowarn', '-Xlint:none', '-proc:none', '-cp', [mojmap, ...libraries].join(path.delimiter), '-d', classes, ...SOURCES], { stdio: ['ignore', 'pipe', 'pipe'] });
    } catch (error) {
      throw new Error(`javac failed:\n${error.stderr}`);
    }
    fs.writeFileSync(stamp, '');
  }
  const classpath = [classes, mojmap, ...libraries].join(path.delimiter);
  // HotSpot's intrinsics for Math.sin and Math.cos differ in the last place from
  // StrictMath's on aarch64 (see jvmEngine in server/engine.js).
  const flags = process.arch === 'arm64' ? ['-XX:+UnlockDiagnosticVMOptions', '-XX:DisableIntrinsic=_dsin,_dcos'] : [];
  return {
    folder,
    // The client's trace of the cases in a file.
    run(casesFile) {
      return execFileSync(javaBin, [...flags, '-Xss16m', '-cp', classpath, 'MovementReference', casesFile], { cwd: folder, maxBuffer: 1 << 30, stdio: ['ignore', 'pipe', 'ignore'], encoding: 'utf8' });
    },
    // The states whose isSuffocating differs from its default.
    dump(file) {
      execFileSync(javaBin, [...flags, '-cp', classpath, 'MovementReference', '--dump', file], { cwd: folder, stdio: 'ignore' });
    },
  };
}

module.exports = { prepare };

if (require.main === module) {
  prepare(process.env.MCBENCH_DATA_DIR, { sources: process.argv.includes('--sources') }).then((ref) => console.log(`ready: ${ref.folder}`), (error) => {
    console.error(error);
    process.exit(1);
  });
}
