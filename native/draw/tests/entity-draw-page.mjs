// The page's own entity geometry for tests/entities.rs: runs
// web/entity-draw.mjs (the module see_build's page and the live viewer
// draw entities with) on the same capture, entities, light and block quads
// the Rust port gets, and prints what it computes.
//
//   node entity-draw-page.mjs <entity-draw.mjs> <mcbench-entities.json> <entities.json> <lone.json>
//
// Per entity: { quads: geometry(), frame: frame() at its place, facing:
// frame() turned to a camera of yaw 0.7, pitch 0.4 } or { error }.
import fs from 'node:fs';
import { pathToFileURL } from 'node:url';

const [modulePath, capturePath, entitiesPath, lonePath] = process.argv.slice(2);
const { EntityData, geometry, frame } = await import(pathToFileURL(modulePath).href);
const data = new EntityData(JSON.parse(fs.readFileSync(capturePath, 'utf8')));
const entities = JSON.parse(fs.readFileSync(entitiesPath, 'utf8'));
const lone = JSON.parse(fs.readFileSync(lonePath, 'utf8'));

// The light both sides use: a fixed function of the cell.
const mod = (v) => ((v % 16) + 16) % 16;
const light = (x, y, z) => [mod(x * 7 + y * 13 + z * 5), mod(x * 3 + y * 5 + z * 11)];
const blockQuads = (state) => {
  if (!lone[state]) throw new Error(`No lone quads for ${state}`);
  return lone[state];
};

// mat4.rotateY(-yaw) then rotateX(-pitch) of the identity (drawEntities).
function turn(yaw, pitch) {
  const ry = (a) => { const c = Math.cos(a), s = Math.sin(a); return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1]; };
  const rx = (a) => { const c = Math.cos(a), s = Math.sin(a); return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1]; };
  const a = ry(-yaw), b = rx(-pitch), out = new Array(16);
  for (let c = 0; c < 4; c += 1) for (let r = 0; r < 4; r += 1) {
    let sum = 0;
    for (let k = 0; k < 4; k += 1) sum += a[k * 4 + r] * b[c * 4 + k];
    out[c * 4 + r] = sum;
  }
  return out;
}

const out = entities.map((entity) => {
  try {
    const quads = geometry(data, entity, { light, blockQuads });
    const place = { pos: entity.pos, yaw: entity.yaw, pitch: entity.pitch, age: entity.ticks };
    return { quads, frame: frame(data, entity, place, null), facing: frame(data, entity, place, turn(0.7, 0.4)) };
  } catch (error) {
    return { error: String(error.message || error) };
  }
});
process.stdout.write(JSON.stringify(out));
