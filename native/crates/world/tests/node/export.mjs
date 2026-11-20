// The Node side of the world export's differential (tests/world_export_test.rs):
// the page's own module (web/world-export.mjs) writes the save of each case.
//
//   node export.mjs <checkout> <cases folder>
//
// Each case is <name>.export (the engine's export bytes, as Session::export
// writes them) and <name>.json ({ name, spawn, now }); this writes
// <name>.node.zip and <name>.node.json ({ folder }). The clock reads `now`
// (milliseconds since 1970) wherever the module reads it; the time zone is
// the process's (TZ). Nothing here writes outside the cases folder.

import fs from 'fs';
import path from 'path';
import { pathToFileURL } from 'url';

const [root, cases] = process.argv.slice(2);
if (!cases) throw new Error('usage: node export.mjs <checkout> <cases folder>');
const { worldZip } = await import(pathToFileURL(path.join(root, 'web', 'world-export.mjs')).href);

// Date.now() and new Date() give `now`; every other use of Date is Date's.
let now = null;
const RealDate = Date;
globalThis.Date = class extends RealDate {
  constructor(...args) {
    if (args.length === 0) {
      if (now === null) throw new Error('The clock was read outside a case');
      super(now);
    } else {
      super(...args);
    }
  }
  static now() {
    if (now === null) throw new Error('The clock was read outside a case');
    return now;
  }
};

// web/core-world.mjs export(), on the bytes of a file.
function readExport(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const gameTime = view.getBigInt64(0);
  const chunks = [];
  let at = 12;
  for (let i = view.getInt32(8); i > 0; i -= 1) {
    const length = view.getInt32(at + 8);
    chunks.push({ x: view.getInt32(at), z: view.getInt32(at + 4), nbt: bytes.subarray(at + 12, at + 12 + length) });
    at += 12 + length;
  }
  return { gameTime, chunks };
}

const names = fs.readdirSync(cases).filter((file) => file.endsWith('.export')).map((file) => file.slice(0, -'.export'.length)).sort();
for (const name of names) {
  const input = JSON.parse(fs.readFileSync(path.join(cases, `${name}.json`), 'utf8'));
  const save = readExport(new Uint8Array(fs.readFileSync(path.join(cases, `${name}.export`))));
  now = input.now;
  const { folder, bytes } = await worldZip({ name: input.name, gameTime: save.gameTime, chunks: save.chunks }, input.spawn);
  now = null;
  fs.writeFileSync(path.join(cases, `${name}.node.zip`), bytes);
  fs.writeFileSync(path.join(cases, `${name}.node.json`), JSON.stringify({ folder }));
}
console.log(`export.mjs: ${names.length} saves, TZ ${process.env.TZ ?? '(unset)'}, ${process.platform} ${process.arch}, zlib ${process.versions.zlib}`);
