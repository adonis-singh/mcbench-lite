// What Minecraft's client draws of an entity, from what its own entity
// renderers drew in the bake (server/McbenchEntities.java, written to
// mcbench-entities.json): the geometry both views draw entities with. The
// live viewer (entities.mjs, three.js in BlueMap) turns its quads into its
// buffers, with its own atlas coordinates and lighting; see_build's renderer
// (native/draw/src/entities.rs) draws the same quads, checked against this
// module run in Node.
//
// An entity is the engine's (core/src/entity/mod.rs json): its type, id,
// position, yaw and pitch, and the data its look changes with. Its kind's
// variant is picked by the kind's `by` fields; the variant's draws each
// hold their quads under a pose the renderer gave them at the variant's
// base values, and how a change of a value moves that pose (a turn about a
// line or a move along a vector, outermost first), or, for a draw that
// follows a model part, that part and the draw's pose in it. A model
// draw's parts each hold their own pose (ModelPart's x, y, z, rotations and
// scales) and its change per unit of each value.
//
// The values that turn every draw alike and first (an entity's yaw, a
// minecart's pitch) are the entity's own frame (frame()); the geometry
// (geometry()) is built in that frame at their base and depends only on the
// rest (appearance()), so a view keeps it while the entity moves.

const decodeBase64 = (text) => {
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
};

// ---- 4x4 matrices, column major ----

export const IDENTITY = Object.freeze([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);

export function multiply(a, b) {
  const out = new Array(16);
  for (let c = 0; c < 4; c += 1) {
    for (let r = 0; r < 4; r += 1) {
      let sum = 0;
      for (let k = 0; k < 4; k += 1) sum += a[k * 4 + r] * b[c * 4 + k];
      out[c * 4 + r] = sum;
    }
  }
  return out;
}

export const translation = (x, y, z) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1];

// A turn of `degrees` about the line through `point` along the unit `axis`.
export function rotation(axis, point, degrees) {
  const t = (degrees * Math.PI) / 180;
  const c = Math.cos(t);
  const s = Math.sin(t);
  const [x, y, z] = axis;
  const r = [
    [c + x * x * (1 - c), x * y * (1 - c) - z * s, x * z * (1 - c) + y * s],
    [y * x * (1 - c) + z * s, c + y * y * (1 - c), y * z * (1 - c) - x * s],
    [z * x * (1 - c) - y * s, z * y * (1 - c) + x * s, c + z * z * (1 - c)],
  ];
  const m = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
  for (let row = 0; row < 3; row += 1) for (let col = 0; col < 3; col += 1) m[col * 4 + row] = r[row][col];
  for (let row = 0; row < 3; row += 1) m[12 + row] = point[row] - (r[row][0] * point[0] + r[row][1] * point[1] + r[row][2] * point[2]);
  return m;
}

// ModelPart.translateAndRotate: x, y, z sixteenths, then
// Quaternionf.rotationZYX(zRot, yRot, xRot), then the scales.
export function partMatrix(p) {
  const cx = Math.cos(p[3]), sx = Math.sin(p[3]), cy = Math.cos(p[4]), sy = Math.sin(p[4]), cz = Math.cos(p[5]), sz = Math.sin(p[5]);
  const r = [
    [cz * cy, cz * sy * sx - sz * cx, cz * sy * cx + sz * sx],
    [sz * cy, sz * sy * sx + cz * cx, sz * sy * cx - cz * sx],
    [-sy, cy * sx, cy * cx],
  ];
  const m = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, p[0] / 16, p[1] / 16, p[2] / 16, 1];
  for (let row = 0; row < 3; row += 1) for (let col = 0; col < 3; col += 1) m[col * 4 + row] = r[row][col] * p[6 + col];
  return m;
}

export function transformPoint(m, x, y, z) {
  return [m[0] * x + m[4] * y + m[8] * z + m[12], m[1] * x + m[5] * y + m[9] * z + m[13], m[2] * x + m[6] * y + m[10] * z + m[14]];
}

// A motion's matrix for a change of `amount` in its value.
function motionMatrix(motion, amount) {
  if (motion.axis) return rotation(motion.axis, motion.point, motion.rate * amount);
  return translation(motion.vector[0] * amount, motion.vector[1] * amount, motion.vector[2] * amount);
}

function moved(motions, values, base, pose) {
  let m = IDENTITY;
  for (const motion of motions) m = multiply(m, motionMatrix(motion, values[motion.value] - base[motion.value]));
  return multiply(m, pose);
}

// ---- Minecraft's own small functions the renderers compute from data ----

// Mth.sin: the client's table of 65,536 sines, indexed by the angle times
// 10430.378 (as a float), masked.
const SINES = (() => {
  const table = new Float32Array(65536);
  for (let i = 0; i < 65536; i += 1) table[i] = Math.sin((i * Math.PI * 2) / 65536);
  return table;
})();
export const mthSin = (value) => SINES[Math.trunc(Math.fround(Math.fround(value) * Math.fround(10430.378))) & 65535];

// AbstractMinecartRenderer.extractRenderState: the offset a minecart is
// drawn at by its id (a seed of it, its bits 16-27 three per axis), so no
// two carts fight over the same depth.
export function minecartOffset(id) {
  let seed = BigInt.asIntN(64, BigInt(id) * 493286711n);
  seed = BigInt.asIntN(64, seed * seed * 4392167121n + seed * 98761n);
  const part = (shift) => Math.fround(Math.fround(Math.fround(Math.fround(Number((seed >> BigInt(shift)) & 7n) + 0.5) / 8) - 0.5) * Math.fround(0.004));
  return [part(16), part(20), part(24)];
}

// ItemEntityRenderer.submit's bob (its height) and ItemEntity.getSpin (its
// turn, radians) at an age in ticks, for a bob offset.
export const itemBob = (age, offset) => Math.fround(Math.fround(mthSin(Math.fround(age / 10) + offset) * 0.1) + 0.1);
export const itemSpin = (age, offset) => Math.fround(age / 20) + offset;

// ExperienceOrbRenderer.submit's colour at an age in ticks (RGB, 0-255).
export function orbColor(age) {
  const f = Math.fround(age / 2);
  return [Math.trunc((mthSin(f) + 1) * 0.5 * 255), 255, Math.trunc((mthSin(Math.fround(f + 4.1887903)) + 1) * 0.1 * 255)];
}

// OverlayTexture's colour at an overlay (u in the low bits, v in the high):
// red where v < 8 (hurt), else white blended in by 1 - u / 15 * 0.75 of
// the colour kept. Answers [r, g, b, kept] (0-1); a draw of no overlay
// keeps all its colour.
export function overlayColor(overlay) {
  if (overlay === undefined) return [1, 1, 1, 1];
  const u = overlay & 0xffff;
  const v = overlay >>> 16;
  if (v < 8) return [1, 0, 0, 1 - 0xb2 / 255];
  return [1, 1, 1, Math.trunc((1 - (u / 15) * 0.75) * 255) / 255];
}

// ---- the capture ----

export class EntityData {
  constructor(json) {
    this.json = json;
    this.textures = json.textures;
    this.shapes = new Float32Array(decodeBase64(json.shapes).buffer);
    this.quads = new Int32Array(decodeBase64(json.quads).buffer);
    this.meshOffsets = new Int32Array(decodeBase64(json.meshes).buffer);
    // Every pose, kept once: a draw names its pose (and a copy, an
    // attachment its matrix) by index.
    const poses = new Float32Array(decodeBase64(json.poses).buffer);
    this.poses = Array.from({ length: poses.length / 16 }, (_, i) => Array.from(poses.subarray(i * 16, i * 16 + 16)));
    this.kinds = json.kinds;
    this.items = json.items;
    this.specials = json.specials;
  }

  // Mesh `index`'s quads: { shape, texture, color } each.
  mesh(index) {
    const out = [];
    for (let q = this.meshOffsets[index]; q < this.meshOffsets[index + 1]; q += 1) out.push({ shape: this.quads[q * 3], texture: this.quads[q * 3 + 1], color: this.quads[q * 3 + 2] });
    return out;
  }

  kind(type) {
    const kind = this.kinds[type];
    if (!kind) throw new Error(`No entity kind ${type} was captured`);
    return kind;
  }
}

// The value an entity's `by` field names, as the variant keys write it.
function field(entity, name) {
  const value = entity[name];
  if (value === undefined) throw new Error(`An entity of ${entity.type} does not say its ${name}`);
  return String(value);
}

// The variant of a kind for an entity.
export function variantOf(kind, entity) {
  let values = kind.icons ? [] : kind.by.map((name) => field(entity, name));
  if (kind.fold) {
    const at = kind.by.indexOf(kind.fold.value);
    const n = Number(values[at]);
    if (n >= kind.fold.from) values[at] = String(kind.fold.from + ((n - kind.fold.from) % kind.fold.period));
  }
  if (kind.icons) {
    // An orb by its value's icon.
    let icon = kind.icons[0][1];
    for (const [least, own] of kind.icons) if (entity.value >= least) icon = own;
    values = [String(icon)];
  }
  const key = values.join(',');
  const variant = kind.variants[key];
  if (!variant) throw new Error(`No variant ${key} of ${entity.type} was captured`);
  return variant;
}

// The values an entity's variant is drawn at, by name (as the capture
// names them: yaw, pitch, a minecart's offset, an armor stand's pose).
export function valuesOf(entity, variant) {
  const values = {};
  for (const name of Object.keys(variant.base)) {
    if (name === 'yaw') values.yaw = entity.yaw;
    else if (name === 'pitch') values.pitch = entity.pitch;
    else if (name === 'offset') values.offset = entity.offset;
    else {
      // An armor stand's pose: "Head.x", by part and axis.
      const [part, axis] = name.split('.');
      const pose = entity.pose?.[POSE_PARTS.indexOf(part)];
      if (!pose) throw new Error(`An entity of ${entity.type} does not say its pose`);
      values[name] = pose['xyz'.indexOf(axis)];
    }
    if (!Number.isFinite(values[name])) throw new Error(`An entity of ${entity.type} has no ${name}`);
  }
  return values;
}

// ArmorStand's poses, in the order the engine writes them.
const POSE_PARTS = ['Head', 'Body', 'LeftArm', 'RightArm', 'LeftLeg', 'RightLeg'];

// The values every draw of a variant moves alike with, outermost, first:
// the entity's frame. Answers the motions.
export function frameMotions(variant) {
  const draws = variant.draws.filter((draw) => !draw.attach);
  const first = draws[0]?.motions ?? [];
  const same = (a, b) => a.value === b.value && JSON.stringify(a.axis) === JSON.stringify(b.axis) && JSON.stringify(a.vector) === JSON.stringify(b.vector)
    && (!a.point || a.point.every((v, i) => Math.abs(v - b.point[i]) < 1e-4)) && Math.abs((a.rate ?? 0) - (b.rate ?? 0)) < 1e-4;
  const out = [];
  for (let i = 0; i < first.length; i += 1) {
    if (!draws.every((draw) => draw.motions[i] && same(draw.motions[i], first[i]))) break;
    // A part that changes with the value is not moved alike.
    if (variant.draws.some((draw) => draw.parts?.some((part) => part.deps[first[i].value]))) break;
    out.push(first[i]);
  }
  return out;
}

// Everything about an entity's look that its frame does not move, with
// the light it is drawn in (`light(x, y, z)`, as for geometry): its geometry
// is the same while this is. An orb's colour goes with its age.
export function appearance(data, entity, light) {
  const kind = data.kind(entity.type);
  const lit = lightCells(data, entity).map((cell) => light(...cell));
  if (entity.type === 'minecraft:item') return JSON.stringify([entity.type, entity.item, kind.tiers[Math.min(entity.count, kind.tiers.length) - 1], lit]);
  const variant = variantOf(kind, entity);
  const frame = new Set(frameMotions(variant).map((motion) => motion.value));
  const values = valuesOf(entity, variant);
  const own = Object.entries(values).filter(([name]) => !frame.has(name));
  return JSON.stringify([entity.type, kind.by.map((name) => entity[name]), own, entity.block, entity.item, entity.equipment, kind.icons ? [entity.value, entity.ticks] : null, lit]);
}

// The entity's frame at its (possibly interpolated) place: its position,
// the minecart's offset by id, its frame's motions, an item's bob and spin
// and an orb's turn to the camera. `camera` is the view's orientation (a
// rotation matrix, column major) for kinds drawn facing it.
export function frame(data, entity, place, camera) {
  const kind = data.kind(entity.type);
  let m = translation(place.pos[0], place.pos[1], place.pos[2]);
  if (entity.type.endsWith('minecart')) {
    // The capture drew carts of one id; this one is offset by its own.
    const own = minecartOffset(entity.id), captured = minecartOffset(data.json.id);
    m = multiply(m, translation(own[0] - captured[0], own[1] - captured[1], own[2] - captured[2]));
  }
  if (entity.type === 'minecraft:item') {
    const offset = bobOffset(entity.id);
    const age = place.age;
    m = multiply(m, translation(0, itemBob(age, offset) - itemBob(0, 0), 0));
    m = multiply(m, rotation([0, 1, 0], [0, 0, 0], (itemSpin(age, offset) * 180) / Math.PI));
    return m;
  }
  const variant = variantOf(kind, entity);
  const values = { ...valuesOf(entity, variant), yaw: place.yaw, pitch: place.pitch };
  for (const motion of frameMotions(variant)) m = multiply(m, motionMatrix(motion, values[motion.value] - variant.base[motion.value]));
  const billboard = variant.draws[0]?.billboard;
  if (billboard && camera) m = multiply(m, multiply(translation(...billboard), multiply(camera, translation(-billboard[0], -billboard[1], -billboard[2]))));
  return m;
}

// An item entity's bob offset: the client draws it at random
// (ItemEntity.bobOffs); here a fixed draw of the id.
export function bobOffset(id) {
  let h = Math.imul(id | 0, 0x9e3779b1) >>> 0;
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b) >>> 0;
  return Math.fround((((h ^ (h >>> 13)) >>> 0) / 2 ** 32) * Math.PI * 2);
}

// The light a kind draws in, from the light at its probe ([block, sky]).
export function kindLight(kind, [block, sky]) {
  return [kind.light.block[block], kind.light.sky[sky]];
}

// The cells an entity's light is read from ([x, y, z] each): its probe's,
// then a painting's own cell for each of its quads.
export function lightCells(data, entity) {
  const kind = data.kind(entity.type);
  const [px, py, pz] = kind.light.probe;
  const cells = [[Math.floor(entity.pos[0] + px), Math.floor(entity.pos[1] + py), Math.floor(entity.pos[2] + pz)]];
  if (entity.type === 'minecraft:item') return cells;
  const base = [Math.floor(entity.pos[0]), Math.floor(entity.pos[1]), Math.floor(entity.pos[2])];
  for (const draw of variantOf(kind, entity).draws) {
    if (draw.light?.cells) for (const step of draw.light.cells) cells.push([base[0] + step[0], base[1] + step[1], base[2] + step[2]]);
  }
  return cells;
}

// ---- geometry ----

// The quads of an entity in its frame, each { corners (12 numbers), uv (8,
// over its texture), texture (its name), color ([r, g, b, a], 0-1, the
// overlay folded in as [r, g, b, kept] in `overlay`), layer (the render
// type's name), light ([block, sky]) }.
//
// `blockQuads(state)` answers the quads of a block state's own models
// drawn alone (the live viewer's mesher's lone, the same in both views),
// in the same form without light; `light(x, y, z)` the [block, sky] light
// of a cell.
export function geometry(data, entity, { blockQuads, light }) {
  const kind = data.kind(entity.type);
  const out = [];
  const probe = kindLight(kind, light(...lightCells(data, entity)[0]));
  if (entity.type === 'minecraft:item') {
    itemEntity(data, kind, entity, out, probe);
    return out;
  }
  const variant = variantOf(kind, entity);
  const frameAt = frameMotions(variant);
  const values = valuesOf(entity, variant);
  for (const motion of frameAt) values[motion.value] = variant.base[motion.value];
  const poses = []; // by draw: its pose, and its parts' poses
  variant.draws.forEach((draw, d) => {
    let pose;
    let parts = null;
    if (draw.attach) pose = multiply(poses[draw.attach.draw].parts.get(draw.attach.part), data.poses[draw.attach.matrix]);
    else pose = moved(draw.motions, values, variant.base, data.poses[draw.pose]);
    if (draw.parts) {
      const fields = new Map(draw.parts.map((part) => {
        const p = part.pose.slice();
        for (const [value, rate] of Object.entries(part.deps)) {
          const change = values[value] - variant.base[value];
          for (let i = 0; i < 9; i += 1) p[i] += rate[i] * change;
        }
        return [part.path, p];
      }));
      const byPath = new Map(draw.parts.map((part) => [part.path, part]));
      parts = new Map();
      for (const part of draw.parts) {
        let m = partMatrix(fields.get(part.path));
        for (let at = part.parent; at !== undefined; at = byPath.get(at).parent) m = multiply(partMatrix(fields.get(at)), m);
        parts.set(part.path, multiply(pose, m));
      }
    }
    poses[d] = { pose, parts };
    const lit = drawLight(kind, entity, draw, probe, light);
    const overlay = overlayColor(draw.overlay);
    if (draw.kind === 'parts') {
      for (const part of draw.parts) addMesh(data, out, part.mesh, parts.get(part.path), draw.layer, lit, overlay);
    } else if (draw.kind === 'mesh') {
      const from = out.length;
      addMesh(data, out, draw.mesh, pose, draw.layer, lit, overlay);
      // An orb's colour (its alpha the capture's).
      if (kind.icons) {
        const [r, g, b] = orbColor(entity.ticks);
        for (let q = from; q < out.length; q += 1) out[q].color = [r / 255, g / 255, b / 255, out[q].color[3]];
      }
    } else if (draw.kind === 'block' || draw.kind === 'moving') {
      const state = draw.block === 'entity' ? entity.block : draw.block;
      if (!state) throw new Error(`An entity of ${entity.type} does not say its block`);
      addBlock(data, out, state, pose, lit, overlay, blockQuads, draw.kind === 'block');
    } else if (draw.kind === 'item') {
      const item = draw.slot === 'item' ? entity.item : entity.equipment?.[draw.slot];
      // A slot the entity has nothing in draws nothing; an armor stand's
      // head holding what is drawn as worn (equipment) neither.
      if (item && !(draw.slot === 'head' && worn(kind, entity, 'head'))) addItem(data, out, item, draw.context, pose, lit, overlay);
    }
  });
  // What an armor stand wears: by item, slot and size, the draws it adds.
  if (kind.equipment) {
    for (const [slot, item] of Object.entries(entity.equipment ?? {})) {
      const added = kind.equipment[item]?.[slot]?.[String(entity.small)];
      if (!added) continue;
      added.forEach((draw) => {
        let pose;
        if (draw.attach) pose = multiply(poses[draw.attach.draw].parts.get(draw.attach.part), data.poses[draw.attach.matrix]);
        else pose = moved(draw.motions, values, variant.base, data.poses[draw.pose]);
        const lit = drawLight(kind, entity, draw, probe, light);
        const overlay = overlayColor(draw.overlay);
        if (draw.kind === 'parts') {
          const byPath = new Map(draw.parts.map((part) => [part.path, part]));
          const fields = new Map(draw.parts.map((part) => {
            const p = part.pose.slice();
            for (const [value, rate] of Object.entries(part.deps)) {
              const change = values[value] - variant.base[value];
              for (let i = 0; i < 9; i += 1) p[i] += rate[i] * change;
            }
            return [part.path, p];
          }));
          for (const part of draw.parts) {
            let m = partMatrix(fields.get(part.path));
            for (let at = part.parent; at !== undefined; at = byPath.get(at).parent) m = multiply(partMatrix(fields.get(at)), m);
            addMesh(data, out, part.mesh, multiply(pose, m), draw.layer, lit, overlay);
          }
        } else addMesh(data, out, draw.mesh, pose, draw.layer, lit, overlay);
      });
    }
  }
  return out;
}

const worn = (kind, entity, slot) => Boolean(kind.equipment[entity.equipment?.[slot]]?.[slot]);

function drawLight(kind, entity, draw, probe, light) {
  if (draw.light === 'entity') return () => probe;
  if (typeof draw.light === 'number') return () => [draw.light >> 4 & 15, draw.light >> 20 & 15];
  // A painting: each quad in its own cell, a step from the entity's cell
  // (the steps captured from an entity whose cell was the origin's).
  const base = [Math.floor(entity.pos[0]), Math.floor(entity.pos[1]), Math.floor(entity.pos[2])];
  return (q) => {
    const step = draw.light.cells[q];
    return kindLight(kind, light(base[0] + step[0], base[1] + step[1], base[2] + step[2]));
  };
}

function addMesh(data, out, mesh, pose, layer, lit, overlay) {
  const { shapes, textures } = data;
  data.mesh(mesh).forEach((quad, q) => {
    const at = quad.shape * 20;
    const corners = [];
    for (let k = 0; k < 4; k += 1) corners.push(...transformPoint(pose, shapes[at + k * 3], shapes[at + k * 3 + 1], shapes[at + k * 3 + 2]));
    const c = quad.color;
    out.push({
      corners,
      uv: Array.from(shapes.subarray(at + 12, at + 20)),
      texture: textures[quad.texture],
      color: [(c >>> 16 & 255) / 255, (c >>> 8 & 255) / 255, (c & 255) / 255, (c >>> 24 & 255) / 255],
      overlay,
      layer,
      light: lit(q),
    });
  });
}

// A block drawn by an entity (BlockRenderDispatcher.renderSingleBlock, or a
// falling block's moving block): its models' quads, and what the client's
// special block renderer draws of it (a chest's chest) where the client's
// collector draws that too (a submitted block, not a moving one).
function addBlock(data, out, state, pose, lit, overlay, blockQuads, special) {
  for (const quad of blockQuads(state)) {
    const corners = [];
    for (let k = 0; k < 4; k += 1) corners.push(...transformPoint(pose, quad.corners[k * 3], quad.corners[k * 3 + 1], quad.corners[k * 3 + 2]));
    out.push({ ...quad, corners, overlay, light: lit(0) });
  }
  const block = state.replace(/\[.*$/, '');
  if (special) for (const draw of data.specials[block] ?? []) addMesh(data, out, draw.mesh, multiply(pose, data.poses[draw.pose]), draw.layer, lit, overlay);
}

// An item's draws in a display context, under `pose`.
function addItem(data, out, item, context, pose, lit, overlay) {
  const own = data.items[item];
  if (!own) throw new Error(`No item ${item} was captured`);
  for (const draw of own[context]) addMesh(data, out, draw.mesh, multiply(pose, data.poses[draw.pose]), draw.layer, lit, overlay);
}

// A dropped stack: its copies (ItemEntityRenderer), as many as its count
// draws, at age 0 and no bob offset (frame() moves them by both).
function itemEntity(data, kind, entity, out, probe) {
  const own = data.items[entity.item];
  if (!own) throw new Error(`No item ${entity.item} was captured`);
  const copies = kind.tiers[Math.min(entity.count, kind.tiers.length) - 1];
  const lit = () => probe;
  for (let c = 0; c < copies; c += 1) {
    const move = c === 0 ? IDENTITY : data.poses[own.copies[c - 1]];
    for (const draw of own.dropped) addMesh(data, out, draw.mesh, multiply(move, data.poses[draw.pose]), draw.layer, lit, overlayColor(draw.overlay));
  }
}

// The faces a block state's own models draw alone, from the live viewer's
// mesher (Mesher.lone, nothing culled: the client's renderSingleBlock and
// moving block culls nothing), as quads: each pair of its triangles, (a, b,
// c) and (a, c, d), is the quad a, b, c, d. `textures` names the map's
// textures by index.
export function loneQuads(mesher, textures, state) {
  const { models } = mesher.lone(state, 0);
  if (models.count % 2 !== 0) throw new Error(`${state} has a face that is not half of a quad`);
  const out = [];
  const CORNERS = [[0, 0], [0, 1], [0, 2], [1, 2]];
  for (let first = 0; first < models.count; first += 2) {
    const corners = [];
    const uv = [];
    for (const [half, k] of CORNERS) {
      const face = first + half;
      corners.push(models.position[face * 9 + k * 3], models.position[face * 9 + k * 3 + 1], models.position[face * 9 + k * 3 + 2]);
      uv.push(models.uv[face * 6 + k * 2], models.uv[face * 6 + k * 2 + 1]);
    }
    out.push({
      corners,
      uv,
      texture: textures[models.material[first]],
      color: [models.color[first * 3], models.color[first * 3 + 1], models.color[first * 3 + 2], 1],
      layer: 'block',
    });
  }
  return out;
}
