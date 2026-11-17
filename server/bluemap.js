'use strict';

// BlueMap as a build-time tool only. Its checksum-pinned CLI jar makes the
// viewer's assets once per content (McbenchBake.java): BlueMap's web app,
// the map's settings and texture list, the baked pack the browser's mesher
// reads, and what Minecraft's entity renderers draw of every entity the
// engine holds with its atlas (McbenchEntities.java and
// McbenchEntityAssets.java, made from Minecraft's client jar, whose classes
// it names by Mojang's mappings of the jar, beside the pack, which the live
// viewer's entity layer and see_build draw entities from); in the same run Minecraft's client says what it says of every
// block state (McbenchClient.java: the render layers and colours see_build
// and the icons read). The mesher (renderer/,
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
const { JAVA, JAVAC } = require('./tools');
const { BIOME } = require('./minecraft-dimension');
const tools = require('./tools');
const setup = require('./setup');
const { withLock } = require('./pid-lock');

const execFileAsync = promisify(execFile);
const DATA_DIR = process.env.MCBENCH_DATA_DIR
  ? path.resolve(process.env.MCBENCH_DATA_DIR)
  : path.join(__dirname, '..', 'data');
const ROOT = path.join(DATA_DIR, 'minecraft-live', 'bluemap');
const VERSION = '5.22';
const JAR_NAME = `bluemap-${VERSION}-cli.jar`;
const JAR_URL = `https://github.com/BlueMap-Minecraft/BlueMap/releases/download/v${VERSION}/${JAR_NAME}`;
const JAR_SHA256 = 'a2289d649f6a83e04cf222abcd3d4f2df4e32e61147015f921c05066f63eefb7';
// Mojang's mappings of the Minecraft client jar BlueMap downloads (the
// version's client_mappings): McbenchEntities names the client's classes
// and members by their Mojang names and reads their names in the jar here.
const MAPPINGS_NAME = `minecraft-client-${MC_VERSION}-mappings.txt`;
const MAPPINGS_URL = 'https://piston-data.mojang.com/v1/objects/7e62354a697f95cf5e7d5981face0583676a9ef7/client.txt';
const MAPPINGS_SHA256 = '2a6f53f4c1fd048e8fa956e3a3fbbb0afc02d5bbc23e52a16b328aed61b9bf39';
const MAPPINGS_BYTES = 11511143;
// Download sizes, for the terminal's account of the first start.
const JAR_MB = 7;
const CLIENT_MB = 31;
const MAPPINGS_MB = Math.round(MAPPINGS_BYTES / 1e6);
const MEMORY = process.env.MCBENCH_BLUEMAP_MEMORY || '2G';
const BAKE = ['McbenchBake.java', 'McbenchEntityAssets.java', 'McbenchClient.java', 'McbenchBlockEntities.java', 'McbenchEntities.java', 'McbenchRenderStates.java'].map((name) => path.join(__dirname, name));
// What Minecraft's server entry point (McbenchClient bootstraps Minecraft by
// it) writes in the bake's folder: its settings and its log.
const MINECRAFT_LEAVES = ['server.properties', 'eula.txt', 'logs'];
// BlueMap's download cache (the Minecraft client jar it reads textures and
// models from), shared by every bake.
const DOWNLOADS = 'data';
const MAP_ID = 'live';

function sha256(bytes) {
  return crypto.createHash('sha256').update(bytes).digest('hex');
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
    // The viewers draw hires tiles only (bluemap-view.mjs): no lowres layer.
    'lod-count: 0',
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
  constructor({ root = ROOT, java = JAVA, memory = MEMORY, download = tools.download, run = execFileAsync, mesher = MESHER } = {}) {
    this.root = root;
    this.java = java;
    this.memory = memory;
    this.download = download;
    this.run = run;
    this.jar = path.join(root, JAR_NAME);
    this.mappings = path.join(root, MAPPINGS_NAME);
    this.mesher = mesher;
    this.mesherHash = sha256(fs.readFileSync(mesher)).slice(0, 16);
    this.assets = null;
    this.folder = null;
    this.preparing = null;
  }

  // The BlueMap jar and Mojang's client mappings, side by side, each
  // unless it is there: a download appears whole only once its checksum
  // matched. The step's progress is the two downloads' bytes together.
  async ensureDownloads() {
    const wanted = [
      { file: this.jar, url: JAR_URL, sha256: JAR_SHA256, what: `BlueMap ${VERSION}`, mb: JAR_MB },
      { file: this.mappings, url: MAPPINGS_URL, sha256: MAPPINGS_SHA256, what: `Mojang's mappings of the Minecraft ${MC_VERSION} client`, mb: MAPPINGS_MB },
    ].filter((download) => !fs.existsSync(download.file));
    const received = wanted.map(() => ({ done: 0, total: null }));
    const progress = (index) => (bytes) => {
      received[index] = bytes;
      setup.progress('viewer')({
        done: received.reduce((sum, part) => sum + part.done, 0),
        total: received.every((part) => part.total !== null) ? received.reduce((sum, part) => sum + part.total, 0) : null,
      });
    };
    await Promise.all(wanted.map((download, index) => step(`[viewer] downloading ${download.what} (about ${download.mb} MB)`, `[viewer] ${download.what} downloaded`,
      () => this.download(download.url, download.file, { sha256: download.sha256, onProgress: progress(index) }))));
  }

  // The viewer's assets for `blockTable` (Paper's, see McbenchBlockTable),
  // made unless this exact bake exists, Minecraft's client run on
  // `libraries` (Paper's, the server's: the client shares them). Resolves
  // with their addresses.
  prepare(blockTable, libraries) {
    this.preparing ||= this.bake(blockTable, libraries).catch((error) => {
      this.preparing = null;
      throw error;
    });
    return this.preparing;
  }

  // The folder of the bake for `blockTable` and these sources, whether
  // made yet or not.
  bakeFolder(blockTable) {
    const files = configFiles(path.join(this.root, DOWNLOADS));
    return path.join(this.root, sha256(JSON.stringify([JAR_SHA256, MAPPINGS_SHA256, MC_VERSION, BIOME, BAKE.map((file) => sha256(fs.readFileSync(file))), sha256(fs.readFileSync(blockTable)), files])).slice(0, 16));
  }

  // One process bakes in the folder at a time, under bluemap.lock beside
  // it: the app holds its data folder alone, but a script or a test reading
  // the block states of the same data folder bakes too, and two bakes would
  // share the staging folder (and each remove the other's as an old bake).
  // One that waited finds the bake made.
  async bake(blockTable, libraries) {
    await this.ensureDownloads();
    return withLock(`${this.root}.lock`, () => this.bakeAlone(blockTable, libraries));
  }

  async bakeAlone(blockTable, libraries) {
    const files = configFiles(path.join(this.root, DOWNLOADS));
    const folder = this.bakeFolder(blockTable);
    const id = path.basename(folder);
    // Everything else here is an older bake, or files of an older app.
    for (const name of fs.readdirSync(this.root)) {
      if (![JAR_NAME, MAPPINGS_NAME, DOWNLOADS, id].includes(name)) fs.rmSync(path.join(this.root, name), { recursive: true, force: true });
    }
    if (!fs.existsSync(folder)) {
      const staging = path.join(this.root, `${id}.partial`);
      writeFiles(staging, files);
      const classes = path.join(staging, 'classes');
      const client = fs.existsSync(path.join(this.root, DOWNLOADS)) ? '' : `, downloading the Minecraft ${MC_VERSION} client jar from Mojang (about ${CLIENT_MB} MB) for its textures`;
      await step(`[viewer] making the 3D viewer's assets with BlueMap and reading every block state's render layer and colours from Minecraft's client${client}`, '[viewer] 3D viewer assets, render layers and colours made', async () => {
        await this.run(JAVAC, ['-proc:none', '-classpath', this.jar, '-d', classes, ...BAKE], { timeout: 120_000 });
        await this.run(this.java, [`-Xmx${this.memory}`, '-cp', [this.jar, classes].join(path.delimiter), 'McbenchBake', MC_VERSION, blockTable, libraries.join(path.delimiter), BIOME, this.mappings],
          { cwd: staging, timeout: 600_000, maxBuffer: 16 * 1024 * 1024 });
      });
      for (const name of ['classes', 'vanilla', ...MINECRAFT_LEAVES]) fs.rmSync(path.join(staging, name), { recursive: true, force: true });
      fs.renameSync(staging, folder);
    }
    this.folder = folder;
    this.assets = {
      id,
      viewerUrl: `/bluemap/${id}/`,
      pack: `/bluemap/${id}/mcbench-pack.bin`,
      wasm: `/bluemap/mesher-${this.mesherHash}.wasm`,
    };
    return this.assets;
  }

  // A file of the current bake that the app reads itself (McbenchClient's
  // render-layers.tsv and tints.tsv), not served.
  baked(name) {
    return path.join(this.folder, name);
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
  renderMask,
  mapConfig,
  configFiles,
};
