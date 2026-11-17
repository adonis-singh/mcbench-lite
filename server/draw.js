'use strict';

// mcbench-draw, the native program that draws see_build's pictures
// (native/draw, its protocol in native/draw/PROTOCOL.md): its version, where
// it is installed, the files it draws with, and the frames the server speaks
// to it in (shot.js).

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const setup = require('./setup');
const tools = require('./tools');
const { step } = require('./progress');
const { minecraftLive } = require('./minecraft-live');
const { getAsset, MC_VERSION } = require('./assets');
const { DATA_DIR } = require('./storage');
const { trackedFiles } = require('./source-files');

const ROOT = path.join(__dirname, '..');
const DRAW = path.join(ROOT, 'native', 'draw');
const RENDERER = path.join(ROOT, 'renderer');

// The files the helper is built from, relative to the repository: every file
// of native/draw (its sources, manifest, lock file and protocol) but its
// build folder, and the mesher it links (renderer/'s sources and manifest:
// its lock file does not decide what the helper links, native/draw's does).
// The Rust release that builds it is core/'s (core/rust-toolchain.toml), so
// that file counts too.
const SOURCES = ['native/draw', 'renderer/src', 'renderer/Cargo.toml', 'core/rust-toolchain.toml'];
async function sourceFiles() {
  return trackedFiles(ROOT, SOURCES, { skip: ['native/draw/target'] });
}

// The helper's version: the first 16 hex digits of the sha256 of its source
// files, each as its path, a zero byte, its bytes and a zero byte (as the
// engine's, engine.js sourcesHash).
async function sourcesHash() {
  const files = await sourceFiles();
  const contents = await Promise.all(files.map((name) => fs.promises.readFile(path.join(ROOT, name))));
  const hash = crypto.createHash('sha256');
  files.forEach((name, index) => hash.update(`${name}\0`).update(contents[index]).update('\0'));
  return hash.digest('hex').slice(0, 16);
}

// Where the helper of version `version` is in the tools folder, where its
// pinned download unpacks (tools.js PINS draw) and scripts/build-draw.js
// puts what it builds.
function drawProgram(version, dir = tools.TOOLS_DIR) {
  return path.join(dir, `draw-${version}`, tools.PINS.draw.platforms[tools.PLATFORM].bin);
}

// The helper for this checkout's sources: downloaded once when they are the
// released helper's (tools.js PINS), or built from them by
// scripts/build-draw.js. Sources that are neither fail, naming that command.
// The first start's step `draw` runs this and then starts the helper
// (shot.js startRenderer).
let drawInstall = null;
function installDraw() {
  drawInstall ||= (async () => {
    const version = await sourcesHash();
    const program = drawProgram(version);
    if (version !== tools.PINS.draw.version) {
      if (fs.existsSync(program)) return program;
      throw new Error(`native/draw and renderer/src are not the released renderer's sources (this checkout's renderer is ${version}, server/draw-release.json pins ${tools.PINS.draw.version}). Build it from its sources with \`node scripts/build-draw.js\`, which puts it in ${path.dirname(program)}`);
    }
    return step(`[see_build] downloading ${tools.PINS.draw.title} ${version} (${tools.downloadMb('draw')} MB) into ${tools.TOOLS_DIR}`, `[see_build] ${tools.PINS.draw.title} installed`,
      () => tools.install('draw', { onProgress: setup.progress('draw') }));
  })().catch((error) => {
    drawInstall = null;
    throw error;
  });
  return drawInstall;
}

// The software Vulkan driver installed beside the helper `program`, on the
// platforms whose release ships one (scripts/release-draw.js FLOOR: Linux
// x64), or null.
function softwareVulkan(program) {
  return tools.PLATFORM === 'linux-x64' ? path.join(path.dirname(program), 'libvk_swiftshader.so') : null;
}

// The files `hello` gives the helper `program` (PROTOCOL.md), all made on
// this machine from Minecraft's own data: the viewer's bake (pack, texture
// names, render layers, entity capture), see_build's atlas, Paper's block
// table and the Minecraft client jar BlueMap downloaded (the entities'
// textures); and on Linux x64 the software Vulkan driver shipped with it.
async function helperAssets(program) {
  await minecraftLive.prepareViewer();
  const blockTable = await minecraftLive.blockTable();
  await Promise.all([getAsset('atlas.png'), getAsset('atlas_map.json')]);
  const baked = (name) => minecraftLive.blueMap.baked(name);
  const cached = (name) => path.join(DATA_DIR, 'assets', MC_VERSION, name);
  return {
    pack: baked('web/mcbench-pack.bin'),
    textures: baked('web/maps/live/textures.json'),
    atlas: cached('atlas.png'),
    atlasMap: cached('atlas_map.json'),
    blockTable,
    renderLayers: baked('render-layers.tsv'),
    entities: baked('web/mcbench-entities.json'),
    clientJar: path.join(minecraftLive.blueMap.root, 'data', `minecraft-client-${MC_VERSION}.jar`),
    ...(softwareVulkan(program) ? { softwareVulkan: softwareVulkan(program) } : {}),
  };
}

// A frame (PROTOCOL.md): the header's length, 32-bit little-endian, the
// header as JSON, which lists the payloads' lengths, then the payloads.
function encodeFrame(header, payloads = []) {
  const json = Buffer.from(JSON.stringify(payloads.length ? { ...header, payloads: payloads.map((payload) => payload.length) } : header), 'utf8');
  const length = Buffer.alloc(4);
  length.writeUInt32LE(json.length);
  return Buffer.concat([length, json, ...payloads]);
}

// Reads frames off a stream of bytes: push() each chunk as it comes, and
// `onFrame(header, payloads)` is called for every whole frame, in order. The
// chunks are kept as they came and joined once per frame, when it is whole,
// so a frame of many chunks costs its size, not its size times its chunks.
class FrameReader {
  constructor(onFrame) {
    this.onFrame = onFrame;
    this.chunks = [];
    this.bytes = 0;
    // The frame being read: its header and its whole length, once known.
    this.header = null;
    this.length = 0;
  }

  // The first `n` buffered bytes, joining only the chunks they span.
  peek(n) {
    const parts = [];
    for (let got = 0, i = 0; got < n; i += 1) {
      parts.push(this.chunks[i]);
      got += this.chunks[i].length;
    }
    return Buffer.concat(parts).subarray(0, n);
  }

  push(chunk) {
    this.chunks.push(chunk);
    this.bytes += chunk.length;
    for (;;) {
      if (!this.header) {
        if (this.bytes < 4) return;
        const headerLength = this.peek(4).readUInt32LE(0);
        if (this.bytes < 4 + headerLength) return;
        this.header = JSON.parse(this.peek(4 + headerLength).subarray(4).toString('utf8'));
        this.headerLength = headerLength;
        this.length = 4 + headerLength + (this.header.payloads || []).reduce((sum, n) => sum + n, 0);
      }
      if (this.bytes < this.length) return;
      const buffer = this.chunks.length === 1 ? this.chunks[0] : Buffer.concat(this.chunks);
      const payloads = [];
      let at = 4 + this.headerLength;
      for (const n of this.header.payloads || []) {
        payloads.push(buffer.subarray(at, at + n));
        at += n;
      }
      const header = this.header;
      const rest = buffer.subarray(this.length);
      this.chunks = rest.length ? [rest] : [];
      this.bytes = rest.length;
      this.header = null;
      this.onFrame(header, payloads);
    }
  }
}

module.exports = { sourceFiles, sourcesHash, drawProgram, installDraw, helperAssets, softwareVulkan, encodeFrame, FrameReader, DRAW, RENDERER };
