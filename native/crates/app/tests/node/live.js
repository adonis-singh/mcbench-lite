'use strict';

// The live world's differential, Node's side (`tests/core_live.rs`
// `the_live_world_drives_the_engine_as_the_page_does`): the page's engine
// (web/core-engine.mjs over the real web/core-worker.mjs in a Node worker
// thread, scripts/wasm-node-engine.js) driven as web/live-world.mjs drives it:
// a fresh placement, ticks, a click on the lever, a tick, an edit, ticks, the
// save. Writes every answer as JSON, bytes in base64.
//
// Usage: node live.js <checkout> <engine.wasm> <tables folder> <commands folder> <out.json>
// The commands folder holds fresh.txt and edit.txt (the import's lines the
// core placed) and steps.json ({ before, after, lever: [x, y, z] }).

const fs = require('fs');
const path = require('path');

async function main() {
  const [checkout, wasm, tables, commands, out] = process.argv.slice(2);
  const { startNodeEngine } = require(path.join(checkout, 'scripts', 'wasm-node-engine.js'));
  const steps = JSON.parse(fs.readFileSync(path.join(commands, 'steps.json'), 'utf8'));
  const b64 = (bytes) => Buffer.from(bytes).toString('base64');
  const engine = await startNodeEngine(wasm, tables);
  const frames = [];
  engine.onframe = ({ ticks, bytes, entities, inputs }) => frames.push({ ticks, bytes: b64(bytes), entities, inputs });
  try {
    const placed = await engine.place(fs.readFileSync(path.join(commands, 'fresh.txt'), 'utf8'), { fresh: true });
    for (let i = 0; i < steps.before; i += 1) await engine.step();
    const [x, y, z] = steps.lever;
    const clicked = await engine.click(x, y, z, Math.fround(0));
    await engine.step();
    const edited = await engine.edit(fs.readFileSync(path.join(commands, 'edit.txt'), 'utf8'));
    for (let i = 0; i < steps.after; i += 1) await engine.step();
    const save = await engine.export();
    fs.writeFileSync(out, JSON.stringify({
      blueprint: b64(placed.bytes),
      inputs: placed.inputs,
      frames,
      clicked,
      edit: { bytes: b64(edited.bytes), entities: JSON.stringify(edited.entities), inputs: edited.inputs },
      save: { gameTime: String(save.gameTime), chunks: save.chunks.map(({ x: cx, z: cz, nbt }) => ({ x: cx, z: cz, nbt: b64(nbt) })) },
    }));
  } finally {
    engine.terminate();
  }
}

main().then(() => process.exit(0), (error) => {
  console.error(error);
  process.exit(1);
});
