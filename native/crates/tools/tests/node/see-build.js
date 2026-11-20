'use strict';

// Today's side of the see_build differential: each case of see-build-cases.json
// run through server/agent.js executeToolCall, with the real engine for the
// settles (the tools folder's) and the real helper, mcbench-draw built from
// this tree's native/draw, which server/shot.js starts and speaks to as it
// does in the app. Three things are given instead of the app's: the helper's
// program and its assets (the files named by the environment, the same the
// Rust side's Draw is given), and, with MCBENCH_DIFF_ADAPTER=software, the
// `adapter: "software"` field of its `hello` (PROTOCOL.md), so both sides draw
// on the same software floor. One JSON line per case:
//
//   { key, content, screenshots, images: [sha256 of each image file],
//     drawn: [the shots requests' shots, as the helper was sent them],
//     card: sha256 of a montage, threw }
//
//   MCBENCH_DATA_DIR=<scratch> MCBENCH_TOOLS_DIR=<tools with the engine>
//   MCBENCH_DIFF_HELPER=<mcbench-draw> MCBENCH_DIFF_MINECRAFT=<minecraft/<id>>
//   MCBENCH_DIFF_TEXTURES=<textures/<id>> MCBENCH_DIFF_WORLDS=<folder of blueprints>
//   [MCBENCH_DIFF_SOFTWARE_VULKAN=<libvk_swiftshader.so>] [MCBENCH_DIFF_ADAPTER=software]
//   node see-build.js <cases.json> <out.jsonl>

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const root = path.resolve(__dirname, '../../../../..');
const [casesFile, outFile] = process.argv.slice(2);
const env = (name) => {
  if (!process.env[name]) throw new Error(`${name} is not set`);
  return process.env[name];
};
// Today's sandbox must not reach Docker on a machine the owner uses
// (replay.js says why).
const childProcess = require('child_process');
const spawn = childProcess.spawn;
childProcess.spawn = (command, ...rest) => {
  if (command === 'docker') throw Object.assign(new Error('spawn docker ENOENT'), { code: 'ENOENT' });
  return spawn(command, ...rest);
};
const server = (name) => require(path.join(root, 'server', name));
const draw = server('draw.js');
const minecraft = env('MCBENCH_DIFF_MINECRAFT');
const textures = env('MCBENCH_DIFF_TEXTURES');
const helper = env('MCBENCH_DIFF_HELPER');
draw.installDraw = async () => helper;
draw.helperAssets = async () => ({
  pack: path.join(minecraft, 'mcbench-pack.bin'),
  textures: path.join(textures, 'textures.json'),
  atlas: path.join(textures, 'atlas.png'),
  atlasMap: path.join(textures, 'atlas_map.json'),
  blockTable: path.join(minecraft, 'block-table.tsv'),
  renderLayers: path.join(minecraft, 'render-layers.tsv'),
  entities: path.join(minecraft, 'mcbench-entities.json'),
  clientJar: path.join(textures, 'client-entries.zip'),
  ...(process.env.MCBENCH_DIFF_SOFTWARE_VULKAN ? { softwareVulkan: process.env.MCBENCH_DIFF_SOFTWARE_VULKAN } : {}),
});
let drawn = [];
let onFrame = () => {};
const encodeFrame = draw.encodeFrame;
draw.encodeFrame = (header, payloads) => {
  if (header.type === 'hello' && process.env.MCBENCH_DIFF_ADAPTER === 'software') header = { ...header, adapter: 'software' };
  if (header.type === 'shots') drawn.push(header.shots);
  onFrame(header);
  return encodeFrame(header, payloads);
};
const agent = server('agent.js');
const shot = server('shot.js');
const { blueprintWorld } = server('blueprint.js');
const { World } = server('world.js');
const { DATA_DIR } = server('storage.js');

const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');

(async () => {
  const cases = JSON.parse(fs.readFileSync(casesFile, 'utf8'));
  const worlds = env('MCBENCH_DIFF_WORLDS');
  const chats = new Map();
  const pictures = new Map();
  const out = fs.openSync(outFile, 'w');
  for (const one of cases) {
    drawn = [];
    let line;
    if (one.montage) {
      const card = await shot.composeMontage(pictures.get(one.montage.of), one.montage.cells, one.montage.size);
      line = { key: one.key, card: sha(card) };
    } else {
      if (!chats.has(one.chat)) {
        const world = one.world === 'empty' ? new World() : blueprintWorld(fs.readFileSync(path.join(worlds, `${one.world}.blueprint`)), 0);
        chats.set(one.chat, { id: one.chat, model: 'fixture', world });
      }
      const conversation = chats.get(one.chat);
      for (const [x, y, z, id] of one.edit || []) conversation.world.set(x, y, z, id, {});
      if (one.edit) conversation.world.finalize();
      const controller = new AbortController();
      onFrame = (header) => { if (one.cancel && header.type === one.cancel) controller.abort(); };
      let executed = null;
      let threw = null;
      try {
        executed = await agent.executeToolCall(conversation, 'see_build', one.arguments, { imageInput: true, signal: controller.signal });
      } catch (error) {
        threw = String(error && error.message || error);
      }
      onFrame = () => {};
      const urls = executed ? (executed.screenshots?.images || executed.images || []).map((image) => image.url) : [];
      const bytes = urls.map((url) => fs.readFileSync(path.join(DATA_DIR, 'images', ...url.split('/').slice(3))));
      pictures.set(one.key, bytes);
      line = {
        key: one.key,
        content: executed ? agent.toolMessageContent('see_build', executed.result) : null,
        screenshots: executed?.screenshots ? { text: executed.screenshots.text, images: urls.length } : null,
        images: bytes.map(sha),
        drawn,
        threw,
      };
    }
    fs.writeSync(out, `${JSON.stringify(line)}\n`);
    console.error(`see-build: ${line.key}`);
  }
  fs.closeSync(out);
  await shot.closeRenderer();
  process.exit(0);
})().catch((error) => { console.error(error); process.exit(1); });
