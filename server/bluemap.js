'use strict';

// BlueMap as a build-time tool only. Its checksum-pinned CLI jar makes the
// viewer's assets once per content (McbenchBake.java): BlueMap's web app,
// the map's settings and texture list, the baked pack the browser's mesher
// reads, and the entity layer's atlas (McbenchEntityAssets.java, made from
// Minecraft's client jar beside the pack). The mesher (renderer/,
// WebAssembly, run in the viewer's worker: web/mesher-worker.mjs) then draws
// every tile in the browser, byte for byte as BlueMap draws it; no BlueMap
// runs while the app does.
// This module serves those assets at addresses that never change meaning.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const { MC_VERSION } = require('./assets');
const { sendFile } = require('./static-file');
const { MODULE: MESHER } = require('../scripts/build-renderer');
const { step } = require('./progress');

const execFileAsync = promisify(execFile);
const DATA_DIR = process.env.MCBENCH_DATA_DIR
  ? path.resolve(process.env.MCBENCH_DATA_DIR)
  : path.join(__dirname, '..', 'data');
const ROOT = path.join(DATA_DIR, 'minecraft-live', 'bluemap');
const VERSION = '5.22';
const JAR_NAME = `bluemap-${VERSION}-cli.jar`;
const JAR_URL = `https://github.com/BlueMap-Minecraft/BlueMap/releases/download/v${VERSION}/${JAR_NAME}`;
const JAR_SHA256 = 'a2289d649f6a83e04cf222abcd3d4f2df4e32e61147015f921c05066f63eefb7';
// Download sizes, for the terminal's account of the first start.
const JAR_MB = 7;
const CLIENT_MB = 31;
// The JDK the app runs Java with (Paper's launcher, Minecraft's tables, the
// block table and the bake): `java` and `javac` on PATH, or the programs these name.
const JAVA = process.env.MCBENCH_JAVA || 'java';
const JAVAC = process.env.MCBENCH_JAVAC || 'javac';
// The data fixer's reference (scripts/DataFixReference.java) is compiled for
// Java 25; Paper 1.21 and BlueMap 5 run on it.
const JAVA_RELEASE = 25;
const JDK_MISSING = `The app reads Minecraft's tables from the server jar and makes the 3D viewer's assets with Java, once: install a JDK, version ${JAVA_RELEASE} or newer (https://adoptium.net, or on a Mac \`brew install --cask temurin\`), so that \`java -version\` and \`javac -version\` work, then start the app again`;
const MEMORY = process.env.MCBENCH_BLUEMAP_MEMORY || '2G';
const BAKE = [path.join(__dirname, 'McbenchBake.java'), path.join(__dirname, 'McbenchEntityAssets.java')];
// BlueMap's download cache (the Minecraft client jar it reads textures and
// models from), shared by every bake.
const DOWNLOADS = 'data';
const MAP_ID = 'live';

function sha256(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
}

// Checks once that `java` and `javac` are a JDK of JAVA_RELEASE or newer;
// rejects with what to install.
let jdkCheck = null;
function checkJdk() {
  jdkCheck ||= (async () => {
    for (const program of [JAVA, JAVAC]) {
      let output;
      try {
        const { stdout, stderr } = await execFileAsync(program, ['-version'], { timeout: 30_000 });
        output = `${stdout}${stderr}`;
      } catch (error) {
        throw new Error(`${JDK_MISSING} (\`${program} -version\` failed: ${String(error.stderr || error.message).trim()})`);
      }
      const release = Number(output.match(/(?:version "|javac )(\d+)/)?.[1]);
      if (!(release >= JAVA_RELEASE)) throw new Error(`${JDK_MISSING} (\`${program}\` is: ${output.trim().split('\n')[0]})`);
    }
  })().catch((error) => {
    jdkCheck = null;
    throw error;
  });
  return jdkCheck;
}

async function downloadFile(url, target) {
  const response = await fetch(url, { redirect: 'follow' });
  if (!response.ok) throw new Error(`BlueMap download failed (HTTP ${response.status})`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const temporary = `${target}.${process.pid}.tmp`;
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(temporary, bytes);
  fs.renameSync(temporary, target);
}

function cameraHash(bounds, yOffset = 64) {
  if (!bounds) return `#${MAP_ID}:0:80:0:80:0:0.47:0:0:perspective`;
  const center = bounds.min.map((value, axis) => (value + bounds.max[axis]) / 2);
  const span = Math.max(...bounds.min.map((value, axis) => bounds.max[axis] - value + 1));
  const distance = Math.max(50, Math.round(span * 1.6));
  return `#${MAP_ID}:${Math.round(center[0])}:${Math.round(center[1] + yOffset)}:${Math.round(center[2])}:${distance}:0:0.47:0:0:perspective`;
}

// A horizontal box around the build, rounded up to a power of two so small
// edits rarely change it (a changed mask redraws every tile). It has no
// height limit: the world holds nothing but the build, so one would change
// no pixel.
function renderMask(bounds) {
  const { min, max } = bounds || { min: [0, 0, 0], max: [0, 0, 0] };
  const horizontalSpan = Math.max(max[0] - min[0] + 1, max[2] - min[2] + 1);
  let size = 128;
  while (size < horizontalSpan + 64) size *= 2;
  const centerX = (min[0] + max[0]) / 2;
  const centerZ = (min[2] + max[2]) / 2;
  const minX = Math.floor((centerX - size / 2) / 16) * 16;
  const minZ = Math.floor((centerZ - size / 2) / 16) * 16;
  return {
    minX,
    maxX: minX + size - 1,
    minZ,
    maxZ: minZ + size - 1,
  };
}

// The map's render settings, as BlueMap reads them from config/maps/live.conf.
// The bake needs no mask (scripts/render-parity.js gives BlueMap's own
// renderer one to compare against).
function mapConfig(mask = null) {
  return [
    'world: "."',
    'dimension: "minecraft:overworld"',
    'name: "Live build"',
    'start-pos: { x: 0, z: 0 }',
    'sky-color: "#7dabff"',
    'ambient-light: 0.1',
    'remove-caves-below-y: -10000',
    'render-edges: true',
    'enable-hires: true',
    'storage: "file"',
    'ignore-missing-light-data: true',
    ...(mask ? ['render-mask: [{', `  min-x: ${mask.minX}`, `  max-x: ${mask.maxX}`, `  min-z: ${mask.minZ}`, `  max-z: ${mask.maxZ}`, '}]'] : []),
    '',
  ].join('\n');
}

// BlueMap's configuration for a bake in `folder`, as path -> text.
function configFiles(downloads, mask = null) {
  return {
    'config/core.conf': [
      'accept-download: true',
      `data: ${JSON.stringify(downloads)}`,
      'scan-for-mod-resources: false',
      'metrics: false',
      '',
    ].join('\n'),
    'config/webapp.conf': [
      'enabled: true',
      'webroot: "web"',
      'update-settings-file: true',
      'use-cookies: true',
      'default-to-flat-view: false',
      // hires view distance is forced by bluemap-bridge.js at runtime
      'lowres-slider-default: 2000',
      'scripts: ["/bluemap-bridge.js"]',
      '',
    ].join('\n'),
    'config/webserver.conf': 'enabled: false\nwebroot: "web"\n',
    'config/storages/file.conf': 'storage-type: file\nroot: "web/maps"\ncompression: none\natomic: true\n',
    [`config/maps/${MAP_ID}.conf`]: mapConfig(mask),
  };
}

function writeFiles(folder, files) {
  for (const [name, text] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(folder, name)), { recursive: true });
    fs.writeFileSync(path.join(folder, name), text);
  }
}

function contentType(file) {
  return {
    '.css': 'text/css; charset=utf-8',
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.json': 'application/json',
    '.png': 'image/png',
    '.svg': 'image/svg+xml',
    '.ttf': 'font/ttf',
    '.wasm': 'application/wasm',
    '.webmanifest': 'application/manifest+json',
  }[path.extname(file).toLowerCase()] || 'application/octet-stream';
}

class BlueMap {
  constructor({ root = ROOT, java = JAVA, memory = MEMORY, download = downloadFile, run = execFileAsync, mesher = MESHER } = {}) {
    this.root = root;
    this.java = java;
    this.memory = memory;
    this.download = download;
    this.run = run;
    this.jar = path.join(root, JAR_NAME);
    this.mesher = mesher;
    this.mesherHash = sha256(fs.readFileSync(mesher)).slice(0, 16);
    this.assets = null;
    this.preparing = null;
  }

  async ensureJar() {
    if (fs.existsSync(this.jar) && sha256(fs.readFileSync(this.jar)) === JAR_SHA256) return;
    fs.rmSync(this.jar, { force: true });
    await step(`[viewer] downloading BlueMap ${VERSION} (about ${JAR_MB} MB)`, `[viewer] BlueMap downloaded`, () => this.download(JAR_URL, this.jar));
    if (sha256(fs.readFileSync(this.jar)) !== JAR_SHA256) {
      fs.rmSync(this.jar, { force: true });
      throw new Error('Downloaded BlueMap jar failed its checksum');
    }
  }

  // The viewer's assets for `blockTable` (Paper's, see McbenchBlockTable),
  // made unless this exact bake exists. Resolves with their addresses.
  prepare(blockTable) {
    this.preparing ||= this.bake(blockTable).catch((error) => {
      this.preparing = null;
      throw error;
    });
    return this.preparing;
  }

  async bake(blockTable) {
    await this.ensureJar();
    const files = configFiles(path.join(this.root, DOWNLOADS));
    const id = sha256(JSON.stringify([JAR_SHA256, MC_VERSION, BAKE.map((file) => sha256(fs.readFileSync(file))), sha256(fs.readFileSync(blockTable)), files])).slice(0, 16);
    const folder = path.join(this.root, id);
    // Everything else here is an older bake, or files of an older app.
    for (const name of fs.readdirSync(this.root)) {
      if (![JAR_NAME, DOWNLOADS, id].includes(name)) fs.rmSync(path.join(this.root, name), { recursive: true, force: true });
    }
    if (!fs.existsSync(folder)) {
      const staging = path.join(this.root, `${id}.partial`);
      writeFiles(staging, files);
      const classes = path.join(staging, 'classes');
      const client = fs.existsSync(path.join(this.root, DOWNLOADS)) ? '' : `, downloading the Minecraft ${MC_VERSION} client jar from Mojang (about ${CLIENT_MB} MB) for its textures`;
      await step(`[viewer] making the 3D viewer's assets with BlueMap${client}`, '[viewer] 3D viewer assets made', async () => {
        await this.run(JAVAC, ['-proc:none', '-classpath', this.jar, '-d', classes, ...BAKE], { timeout: 120_000 });
        await this.run(this.java, [`-Xmx${this.memory}`, '-cp', [this.jar, classes].join(path.delimiter), 'McbenchBake', MC_VERSION, blockTable],
          { cwd: staging, timeout: 600_000, maxBuffer: 16 * 1024 * 1024 });
      });
      fs.rmSync(classes, { recursive: true, force: true });
      fs.renameSync(staging, folder);
    }
    this.assets = {
      id,
      viewerUrl: `/bluemap/${id}/`,
      pack: `/bluemap/${id}/mcbench-pack.bin`,
      wasm: `/bluemap/mesher-${this.mesherHash}.wasm`,
    };
    return this.assets;
  }

  // The web app and pack of the current bake under /bluemap/<id>/, and the
  // mesher under /bluemap/mesher-<hash>.wasm: each address names one
  // version, so browsers keep them.
  serve(req, res, pathname) {
    if (!pathname.startsWith('/bluemap/')) return false;
    if (req.method !== 'GET' && req.method !== 'HEAD') return false;
    const headers = (file) => ({
      'content-type': contentType(file),
      'cache-control': 'public, max-age=31536000, immutable',
      'x-content-type-options': 'nosniff',
    });
    if (pathname === `/bluemap/mesher-${this.mesherHash}.wasm`) {
      sendFile(req, res, this.mesher, headers(this.mesher));
      return true;
    }
    const prefix = this.assets && `/bluemap/${this.assets.id}/`;
    if (!prefix || !pathname.startsWith(prefix)) {
      res.writeHead(404).end();
      return true;
    }
    let relative;
    try {
      relative = decodeURIComponent(pathname.slice(prefix.length)) || 'index.html';
    } catch {
      res.writeHead(400).end();
      return true;
    }
    const root = path.join(this.root, this.assets.id, 'web');
    const target = path.resolve(root, relative);
    if (!target.startsWith(`${root}${path.sep}`) || !fs.existsSync(target) || !fs.statSync(target).isFile()) {
      // The web app polls each map's live markers and players, which this
      // map never has.
      res.writeHead(relative.startsWith('maps/') ? 204 : 404).end();
      return true;
    }
    sendFile(req, res, target, headers(target));
    return true;
  }
}

const blueMap = new BlueMap();

module.exports = {
  BlueMap,
  blueMap,
  cameraHash,
  renderMask,
  mapConfig,
  configFiles,
  downloadFile,
  checkJdk,
  JAVA,
  JAVAC,
};
