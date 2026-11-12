// What each kind of entity looks like, as mesh data and transforms, without
// three.js: entities.mjs puts them on screen, and the tests read them as
// numbers. Drawn from the atlas and index McbenchEntityAssets.java bakes from
// Minecraft's client jar (see its header for the index's format).
//
// Meshes are triangle lists whose vertices carry what BlueMap's hires shader
// reads: position, normal, uv (atlas texels over the atlas size) and color.
// Block models are centered on the origin and one block wide.

const RAD = Math.PI / 180;
const FACES = ['down', 'up', 'north', 'south', 'west', 'east'];
const NORMALS = {
  down: [0, -1, 0], up: [0, 1, 0], north: [0, 0, -1], south: [0, 0, 1], west: [-1, 0, 0], east: [1, 0, 0],
};
// Three's rotation about Y that turns a model facing north to face each side.
export const FACING_YAW = { north: 0, east: -90, south: 180, west: 90 };
export const FACING_VECTOR = {
  north: [0, 0, -1], south: [0, 0, 1], west: [-1, 0, 0], east: [1, 0, 0], up: [0, 1, 0], down: [0, -1, 0],
};

// ---- 4x4 matrices, column-major arrays as three.js reads them ----

export const IDENTITY = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];

export function multiply(a, b) {
  const out = new Array(16);
  for (let column = 0; column < 4; column += 1) {
    for (let row = 0; row < 4; row += 1) {
      let sum = 0;
      for (let k = 0; k < 4; k += 1) sum += a[k * 4 + row] * b[column * 4 + k];
      out[column * 4 + row] = sum;
    }
  }
  return out;
}

export const compose = (...matrices) => matrices.reduce(multiply);
export const translation = (x, y, z) => [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, x, y, z, 1];
export const scaling = (x, y = x, z = x) => [x, 0, 0, 0, 0, y, 0, 0, 0, 0, z, 0, 0, 0, 0, 1];

export function rotationX(angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [1, 0, 0, 0, 0, c, s, 0, 0, -s, c, 0, 0, 0, 0, 1];
}

export function rotationY(angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [c, 0, -s, 0, 0, 1, 0, 0, s, 0, c, 0, 0, 0, 0, 1];
}

export function rotationZ(angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  return [c, s, 0, 0, -s, c, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
}

// Minecraft's part rotation: about Z, then Y, then X.
export const rotationZYX = (x, y, z) => compose(rotationZ(z), rotationY(y), rotationX(x));

export function transformPoint(m, [x, y, z]) {
  return [
    m[0] * x + m[4] * y + m[8] * z + m[12],
    m[1] * x + m[5] * y + m[9] * z + m[13],
    m[2] * x + m[6] * y + m[10] * z + m[14],
  ];
}

export function transformDirection(m, [x, y, z]) {
  return [m[0] * x + m[4] * y + m[8] * z, m[1] * x + m[5] * y + m[9] * z, m[2] * x + m[6] * y + m[10] * z];
}

// ---- mesh data ----

export class MeshBuilder {
  constructor() {
    this.position = [];
    this.normal = [];
    this.uv = [];
    this.color = [];
  }

  get vertexCount() {
    return this.position.length / 3;
  }

  // A quad of four corners in order, with a uv for each (atlas texels
  // already divided by the atlas size), as two triangles.
  quad(corners, uvs, normal, color = [1, 1, 1]) {
    for (const i of [0, 1, 2, 0, 2, 3]) {
      this.position.push(...corners[i]);
      this.normal.push(...normal);
      this.uv.push(...uvs[i]);
      this.color.push(...color);
    }
  }

  data() {
    return {
      position: new Float32Array(this.position),
      normal: new Float32Array(this.normal),
      uv: new Float32Array(this.uv),
      color: new Float32Array(this.color),
    };
  }
}

// ---- the index ----

export class EntityIndex {
  constructor(index) {
    this.index = index;
    this.width = index.atlas.width;
    this.height = index.atlas.height;
    validateIndex(index);
  }

  rect(texture) {
    const rect = this.index.textures[texture];
    if (!rect) throw new Error(`The entity atlas has no texture ${texture}`);
    return rect;
  }

  // The atlas uv of a point of `texture`, given as 0..16 over it.
  uv(texture, u, v) {
    const [x, y, w, h] = this.rect(texture);
    return [(x + u * w / 16) / this.width, (y + v * h / 16) / this.height];
  }

  // The same, given in texels of `texture`.
  texel(texture, u, v) {
    const [x, y] = this.rect(texture);
    return [(x + u) / this.width, (y + v) / this.height];
  }
}

// Every name the index uses must exist. The bake writes it; a mismatch is a
// bug there, found here at load.
export function validateIndex(index) {
  const need = (table, key, what) => {
    if (!index[table] || !(key in index[table])) throw new Error(`The entity index names ${what} that it does not hold`);
  };
  for (const [name, [x, y, w, h]] of Object.entries(index.textures)) {
    if (x < 0 || y < 0 || w <= 0 || h <= 0 || x + w > index.atlas.width || y + h > index.atlas.height) {
      throw new Error(`The texture ${name} is outside the atlas`);
    }
  }
  for (const [name, model] of Object.entries(index.models)) {
    for (const element of model.elements) {
      for (const face of Object.values(element.faces)) need('textures', face.texture, `texture ${face.texture} (model ${name})`);
    }
  }
  const place = (placement, owner) => need('models', placement.model, `model ${placement.model} (${owner})`);
  for (const [id, block] of Object.entries(index.blocks)) {
    Object.values(block.variants || {}).forEach((variant) => place(variant, id));
    (block.multipart || []).forEach((part) => place(part.apply, id));
  }
  for (const [id, item] of Object.entries(index.items)) {
    if (item.sprite) need('textures', item.sprite, `sprite ${item.sprite} (${id})`);
    else need('models', item.model, `model ${item.model} (${id})`);
  }
  for (const [type, texture] of Object.entries(index.entities)) need('textures', texture, `texture ${texture} (${type})`);
}

// ---- block states ----

// "minecraft:oak_stairs[facing=east,half=top]" -> { id, props }
export function parseState(state) {
  const open = state.indexOf('[');
  if (open < 0) return { id: state, props: {} };
  if (!state.endsWith(']')) throw new Error(`Not a block state: ${state}`);
  const props = {};
  for (const pair of state.slice(open + 1, -1).split(',')) {
    const [key, value] = pair.split('=');
    props[key] = value;
  }
  return { id: state.slice(0, open), props };
}

// A multipart `when`: properties that must hold ("a|b" is either), or an
// AND / OR list of them.
export function matchesWhen(when, props) {
  if (when.OR) return when.OR.some((part) => matchesWhen(part, props));
  if (when.AND) return when.AND.every((part) => matchesWhen(part, props));
  return Object.entries(when).every(([key, wanted]) => {
    const value = String(wanted);
    const negated = value.startsWith('!');
    const options = (negated ? value.slice(1) : value).split('|');
    return key in props && options.includes(props[key]) !== negated;
  });
}

// The models a block state draws: a variant whose properties agree with
// every property the state gives (the first, as a state may give few), or the
// multipart parts that apply.
export function blockPlacements(index, state) {
  const { id, props } = parseState(state);
  const block = index.blocks[id];
  if (!block) throw new Error(`The entity index has no block ${id}`);
  if (block.variants) {
    for (const [key, placement] of Object.entries(block.variants)) {
      const agrees = key === '' || key.split(',').every((pair) => {
        const [name, value] = pair.split('=');
        return !(name in props) || props[name] === value;
      });
      if (agrees) return [placement];
    }
    if (Object.keys(block.variants).length > 0) throw new Error(`No model of ${id} fits ${state}`);
  }
  return (block.multipart || []).filter((part) => !part.when || matchesWhen(part.when, props)).map((part) => part.apply);
}

// ---- block models ----

// The four corners of an element's face, top-left first as seen from outside,
// in model pixels.
function faceCorners(face, f, t) {
  switch (face) {
    case 'up': return [[f[0], t[1], f[2]], [f[0], t[1], t[2]], [t[0], t[1], t[2]], [t[0], t[1], f[2]]];
    case 'down': return [[f[0], f[1], t[2]], [f[0], f[1], f[2]], [t[0], f[1], f[2]], [t[0], f[1], t[2]]];
    case 'north': return [[t[0], t[1], f[2]], [t[0], f[1], f[2]], [f[0], f[1], f[2]], [f[0], t[1], f[2]]];
    case 'south': return [[f[0], t[1], t[2]], [f[0], f[1], t[2]], [t[0], f[1], t[2]], [t[0], t[1], t[2]]];
    case 'west': return [[f[0], t[1], f[2]], [f[0], f[1], f[2]], [f[0], f[1], t[2]], [f[0], t[1], t[2]]];
    case 'east': return [[t[0], t[1], t[2]], [t[0], f[1], t[2]], [t[0], f[1], f[2]], [t[0], t[1], f[2]]];
    default: throw new Error(`Not a face: ${face}`);
  }
}

// Turns a point about an axis through `origin`, as Minecraft's element
// rotation does; `rescale` stretches the other two axes by 1 / cos(angle).
function rotateElement(point, rotation) {
  const angle = rotation.angle * RAD;
  const [ox, oy, oz] = rotation.origin;
  const rescale = rotation.rescale ? 1 / Math.cos(angle) : 1;
  const [x, y, z] = [point[0] - ox, point[1] - oy, point[2] - oz];
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  switch (rotation.axis) {
    case 'x': return [ox + x * rescale, oy + (y * c - z * s) * rescale, oz + (y * s + z * c) * rescale];
    case 'y': return [ox + (x * c + z * s) * rescale, oy + y * rescale, oz + (-x * s + z * c) * rescale];
    case 'z': return [ox + (x * c - y * s) * rescale, oy + (x * s + y * c) * rescale, oz + z * rescale];
    default: throw new Error(`Not an axis: ${rotation.axis}`);
  }
}

// Adds the quads of model `name` to `builder`, turned about the block's
// center by the blockstate's x then y (Minecraft turns them the other way
// round about each axis), tinted where a face asks for it.
export function addModel(builder, entityIndex, name, { x = 0, y = 0 } = {}, tint = null) {
  const model = entityIndex.index.models[name];
  if (!model) throw new Error(`The entity index has no model ${name}`);
  const turn = compose(rotationY(-y * RAD), rotationX(-x * RAD));
  const color = tint ? tint.map((channel) => channel / 255) : [1, 1, 1];
  for (const element of model.elements) {
    const from = element.from;
    const to = element.to;
    for (const [face, spec] of Object.entries(element.faces)) {
      let corners = faceCorners(face, from, to);
      let normal = NORMALS[face];
      if (element.rotation) {
        corners = corners.map((corner) => rotateElement(corner, element.rotation));
        normal = rotateElement(normal, { ...element.rotation, origin: [0, 0, 0], rescale: false });
      }
      const world = corners.map((corner) => transformPoint(turn, corner.map((value) => value - 8)).map((value) => value / 16));
      normal = transformDirection(turn, normal);
      const [u0, v0, u1, v1] = spec.uv;
      const sides = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
      const shift = ((spec.rotation || 0) / 90) % 4;
      const uvs = [0, 1, 2, 3].map((i) => entityIndex.uv(spec.texture, ...sides[(i + shift) % 4]));
      builder.quad(world, uvs, normal, spec.tinted ? color : [1, 1, 1]);
    }
  }
}

// A block state as one mesh centered on the origin, or null when it draws
// nothing (air).
export function blockMesh(entityIndex, state) {
  const { id } = parseState(state);
  const placements = blockPlacements(entityIndex.index, state);
  if (placements.length === 0) return null;
  const builder = new MeshBuilder();
  for (const placement of placements) addModel(builder, entityIndex, placement.model, placement, entityIndex.index.blocks[id].tint);
  return builder.data();
}

// An item drawn as a block model (stone, a chest: its particle cube).
export function itemBlockMesh(entityIndex, id) {
  const item = entityIndex.index.items[id];
  const builder = new MeshBuilder();
  addModel(builder, entityIndex, item.model, {}, item.tint);
  return builder.data();
}

// ---- sprites ----

// A sprite as a slab one block wide and a sixteenth thick, centered on the
// origin; its front faces north. The slab's edges take the sprite's border
// texels. Transparent texels are cut out by the shader.
export function spriteMesh(entityIndex, texture) {
  const [x, y, w, h] = entityIndex.rect(texture);
  const W = entityIndex.width;
  const H = entityIndex.height;
  const builder = new MeshBuilder();
  const t = 1 / 32;
  const u0 = x / W;
  const u1 = (x + w) / W;
  const v0 = y / H;
  const v1 = (y + h) / H;
  const du = 0.5 / W;
  const dv = 0.5 / H;
  builder.quad([[0.5, 0.5, -t], [0.5, -0.5, -t], [-0.5, -0.5, -t], [-0.5, 0.5, -t]], [[u0, v0], [u0, v1], [u1, v1], [u1, v0]], [0, 0, -1]);
  builder.quad([[-0.5, 0.5, t], [-0.5, -0.5, t], [0.5, -0.5, t], [0.5, 0.5, t]], [[u0, v0], [u0, v1], [u1, v1], [u1, v0]], [0, 0, 1]);
  builder.quad([[-0.5, 0.5, -t], [-0.5, 0.5, t], [0.5, 0.5, t], [0.5, 0.5, -t]], [[u0, v0 + dv], [u0, v0 + dv], [u1, v0 + dv], [u1, v0 + dv]], [0, 1, 0]);
  builder.quad([[-0.5, -0.5, t], [-0.5, -0.5, -t], [0.5, -0.5, -t], [0.5, -0.5, t]], [[u0, v1 - dv], [u0, v1 - dv], [u1, v1 - dv], [u1, v1 - dv]], [0, -1, 0]);
  builder.quad([[-0.5, 0.5, t], [-0.5, 0.5, -t], [-0.5, -0.5, -t], [-0.5, -0.5, t]], [[u0 + du, v0], [u0 + du, v0], [u0 + du, v1], [u0 + du, v1]], [-1, 0, 0]);
  builder.quad([[0.5, 0.5, -t], [0.5, 0.5, t], [0.5, -0.5, t], [0.5, -0.5, -t]], [[u1 - du, v0], [u1 - du, v0], [u1 - du, v1], [u1 - du, v1]], [1, 0, 0]);
  return builder.data();
}

// A flat panel of `texture` (all of it) `width` by `height` blocks, front
// facing north, centered on the origin; a painting is this with a back.
export function panelMesh(entityIndex, front, back, width, height, thickness = 1 / 16) {
  const builder = new MeshBuilder();
  const [hw, hh, hd] = [width / 2, height / 2, thickness / 2];
  const full = (texture) => [entityIndex.uv(texture, 0, 0), entityIndex.uv(texture, 0, 16), entityIndex.uv(texture, 16, 16), entityIndex.uv(texture, 16, 0)];
  builder.quad([[hw, hh, -hd], [hw, -hh, -hd], [-hw, -hh, -hd], [-hw, hh, -hd]], full(front), [0, 0, -1]);
  builder.quad([[-hw, hh, hd], [-hw, -hh, hd], [hw, -hh, hd], [hw, hh, hd]], full(back), [0, 0, 1]);
  builder.quad([[-hw, hh, -hd], [-hw, hh, hd], [hw, hh, hd], [hw, hh, -hd]], full(back), [0, 1, 0]);
  builder.quad([[-hw, -hh, hd], [-hw, -hh, -hd], [hw, -hh, -hd], [hw, -hh, hd]], full(back), [0, -1, 0]);
  builder.quad([[-hw, hh, hd], [-hw, hh, -hd], [-hw, -hh, -hd], [-hw, -hh, hd]], full(back), [-1, 0, 0]);
  builder.quad([[hw, hh, -hd], [hw, hh, hd], [hw, -hh, hd], [hw, -hh, -hd]], full(back), [1, 0, 0]);
  return builder.data();
}

// ---- entity models: Minecraft's cuboid parts ----

// One part of an entity model as Minecraft writes it: a box at `offset`
// (pixels from the part's pivot) of `size`, textured from (u, v) as a
// cuboid unwrap, whose pivot is `pivot` and rotation `rotation` (radians).
// Entity models have y pointing down and are drawn mirrored in x and y;
// the mesh comes out the right way up, in blocks, `lift` blocks above the
// origin and `scale` times as large.
export function addCuboid(builder, entityIndex, texture, part, { lift = 0, scale = 1 } = {}) {
  const [u, v] = part.uv;
  const [ox, oy, oz] = part.offset;
  const [w, h, d] = part.size;
  const [x0, y0, z0, x1, y1, z1] = [ox, oy, oz, ox + w, oy + h, oz + d];
  const rotation = compose(translation(...part.pivot), rotationZYX(...(part.rotation || [0, 0, 0])));
  const flip = compose(scaling(scale), translation(0, lift, 0), scaling(1 / 16), scaling(-1, -1, 1));
  const toWorld = compose(flip, rotation);
  const A = [x0, y0, z0];
  const B = [x1, y0, z0];
  const C = [x1, y1, z0];
  const D = [x0, y1, z0];
  const E = [x0, y0, z1];
  const F = [x1, y0, z1];
  const G = [x1, y1, z1];
  const H = [x0, y1, z1];
  const f13 = u;
  const f14 = u + d;
  const f15 = u + d + w;
  const f16 = u + d + w + w;
  const f17 = u + d + w + d;
  const f18 = u + d + w + d + w;
  const f19 = v;
  const f20 = v + d;
  const f21 = v + d + h;
  // Each polygon: its vertices in order and the uv box Minecraft gives them
  // (u1, v1, u2, v2): vertex 0 takes (u2, v1), 1 (u1, v1), 2 (u1, v2), 3 (u2, v2).
  const polygons = [
    [[F, E, A, B], f14, f19, f15, f20, [0, -1, 0]],
    [[C, D, H, G], f15, f20, f16, f19, [0, 1, 0]],
    [[A, B, C, D], f14, f20, f15, f21, [0, 0, -1]],
    [[F, E, H, G], f17, f20, f18, f21, [0, 0, 1]],
    [[B, F, G, C], f15, f20, f17, f21, [1, 0, 0]],
    [[E, A, D, H], f13, f20, f14, f21, [-1, 0, 0]],
  ];
  for (const [corners, u1, v1, u2, v2, normal] of polygons) {
    const uvs = [[u2, v1], [u1, v1], [u1, v2], [u2, v2]].map(([pu, pv]) => entityIndex.texel(texture, pu, pv));
    builder.quad(corners.map((corner) => transformPoint(toWorld, corner)), uvs, transformDirection(toWorld, normal));
  }
}

// Minecraft's models, as their parts: uv, offset, size, pivot, rotation.
const PI = Math.PI;
const CART = [
  { uv: [0, 10], offset: [-10, -8, -1], size: [20, 16, 2], pivot: [0, 4, 0], rotation: [PI / 2, 0, 0] },
  { uv: [0, 0], offset: [-8, -9, -1], size: [16, 8, 2], pivot: [-9, 4, 0], rotation: [0, PI * 1.5, 0] },
  { uv: [0, 0], offset: [-8, -9, -1], size: [16, 8, 2], pivot: [9, 4, 0], rotation: [0, PI / 2, 0] },
  { uv: [0, 0], offset: [-8, -9, -1], size: [16, 8, 2], pivot: [0, 4, -7], rotation: [0, PI, 0] },
  { uv: [0, 0], offset: [-8, -9, -1], size: [16, 8, 2], pivot: [0, 4, 7], rotation: [0, 0, 0] },
];
const BOAT = [
  { uv: [0, 0], offset: [-14, -9, -3], size: [28, 16, 3], pivot: [0, 3, 1], rotation: [PI / 2, 0, 0] },
  { uv: [0, 19], offset: [-13, -7, -1], size: [18, 6, 2], pivot: [-15, 4, 4], rotation: [0, PI * 1.5, 0] },
  { uv: [0, 27], offset: [-8, -7, -1], size: [16, 6, 2], pivot: [15, 4, 0], rotation: [0, PI / 2, 0] },
  { uv: [0, 35], offset: [-14, -7, -1], size: [28, 6, 2], pivot: [0, 4, -9], rotation: [0, PI, 0] },
  { uv: [0, 43], offset: [-14, -7, -1], size: [28, 6, 2], pivot: [0, 4, 9], rotation: [0, 0, 0] },
];
const BOAT_CHEST = [
  { uv: [0, 76], offset: [0, 0, 0], size: [12, 8, 12], pivot: [-2, -5, -6], rotation: [0, -PI / 2, 0] },
  { uv: [0, 59], offset: [0, 0, 0], size: [12, 4, 12], pivot: [-2, -9, -6], rotation: [0, -PI / 2, 0] },
  { uv: [0, 59], offset: [0, 0, 0], size: [2, 4, 1], pivot: [-1, -6, -1], rotation: [0, -PI / 2, 0] },
];
// A raft is a slab of its planks: the boat's texture over one cuboid. Its
// chest sits a pixel lower than a boat's.
const RAFT = [{ uv: [0, 0], offset: [-11, -4, -11], size: [22, 3, 22], pivot: [0, 6, 0], rotation: [0, 0, 0] }];
const RAFT_CHEST = BOAT_CHEST.map((part) => ({ ...part, pivot: [part.pivot[0], part.pivot[1] + 1, part.pivot[2]] }));
const STAND = {
  head: { uv: [0, 0], offset: [-1, -7, -1], size: [2, 7, 2], pivot: [0, 1, 0], rotation: [0, 0, 0] },
  body: { uv: [0, 26], offset: [-6, 0, -1.5], size: [12, 3, 3], pivot: [0, 0, 0], rotation: [0, 0, 0] },
  rightArm: { uv: [24, 0], offset: [-2, -2, -1], size: [2, 12, 2], pivot: [-5, 2, 0], rotation: [-15 * RAD, 0, 10 * RAD] },
  leftArm: { uv: [32, 16], offset: [0, -2, -1], size: [2, 12, 2], pivot: [5, 2, 0], rotation: [-10 * RAD, 0, -10 * RAD] },
  rightLeg: { uv: [8, 0], offset: [-1, 0, -1], size: [2, 11, 2], pivot: [-1.9, 12, 0], rotation: [1 * RAD, 0, 1 * RAD] },
  leftLeg: { uv: [40, 16], offset: [-1, 0, -1], size: [2, 11, 2], pivot: [1.9, 12, 0], rotation: [-1 * RAD, 0, -1 * RAD] },
  rightStick: { uv: [16, 0], offset: [-3, 3, -1], size: [2, 7, 2], pivot: [0, 0, 0], rotation: [0, 0, 0] },
  leftStick: { uv: [48, 16], offset: [1, 3, -1], size: [2, 7, 2], pivot: [0, 0, 0], rotation: [0, 0, 0] },
  shoulder: { uv: [0, 48], offset: [-4, 10, -1], size: [8, 2, 2], pivot: [0, 0, 0], rotation: [0, 0, 0] },
  base: { uv: [0, 32], offset: [-6, 11, -6], size: [12, 1, 12], pivot: [0, 12, 0], rotation: [0, 0, 0] },
};

function cuboids(entityIndex, texture, parts, options) {
  const builder = new MeshBuilder();
  for (const part of parts) addCuboid(builder, entityIndex, texture, part, options);
  return builder.data();
}

// The cart, in blocks, with the origin at its feet (Minecraft lifts it 0.375).
export const cartMesh = (entityIndex) => cuboids(entityIndex, entityIndex.index.entities['minecraft:minecart'], CART, { lift: 0.375 });

// A boat or raft of `wood`, its chest too when asked. `type` is the entity's.
export function boatMesh(entityIndex, type) {
  const texture = entityIndex.index.entities[type];
  if (!texture) throw new Error(`The entity index has no texture for ${type}`);
  const raft = type.endsWith('_raft');
  const chest = type.includes('chest');
  const parts = raft ? RAFT.concat(chest ? RAFT_CHEST : []) : BOAT.concat(chest ? BOAT_CHEST : []);
  return cuboids(entityIndex, texture, parts, { lift: 0.375 });
}

// The armor stand, half size when small.
export function standMesh(entityIndex, small) {
  const scale = small ? 0.5 : 1;
  const texture = entityIndex.index.entities['minecraft:armor_stand'];
  const builder = new MeshBuilder();
  const options = { lift: 1.501, scale };
  for (const part of Object.values(STAND)) addCuboid(builder, entityIndex, texture, part, options);
  return builder.data();
}

// Where the armor stand's right hand holds an item, as the matrix that takes
// the item's own frame (one block, centered) there.
export function standHand(small) {
  const arm = STAND.rightArm;
  const scale = small ? 0.5 : 1;
  const toWorld = compose(scaling(scale), translation(0, 1.501, 0), scaling(1 / 16), scaling(-1, -1, 1),
    translation(...arm.pivot), rotationZYX(...arm.rotation), translation(-1, 10, -2));
  return compose(toWorld, scaling(-16, -16, 16));
}

// Crossed panels of an arrow's shaft, its tail to the left; the tip is +x.
export function arrowMesh(entityIndex, type) {
  const texture = entityIndex.index.entities[type];
  if (!texture) throw new Error(`The entity index has no texture for ${type}`);
  const builder = new MeshBuilder();
  const length = 0.9;
  const half = 0.14;
  const [x, y, w, h] = entityIndex.rect(texture);
  const u0 = x / entityIndex.width;
  const u1 = (x + w / 2) / entityIndex.width;
  const v0 = y / entityIndex.height;
  const v1 = (y + h * 5 / 32) / entityIndex.height;
  const uvs = [[u0, v0], [u0, v1], [u1, v1], [u1, v0]];
  builder.quad([[-length / 2, half, 0], [-length / 2, -half, 0], [length / 2, -half, 0], [length / 2, half, 0]], uvs, [0, 0, 1]);
  builder.quad([[-length / 2, 0, half], [-length / 2, 0, -half], [length / 2, 0, -half], [length / 2, 0, half]], uvs, [0, 1, 0]);
  return builder.data();
}

// ---- what an entity is drawn as ----

const hash = (n) => {
  let h = Math.imul(n | 0, 0x9e3779b1) >>> 0;
  h = Math.imul(h ^ (h >>> 15), 0x85ebca6b) >>> 0;
  return ((h ^ (h >>> 13)) >>> 0) / 2 ** 32;
};

const MINECARTS = new Set(['minecart', 'chest_minecart', 'furnace_minecart', 'hopper_minecart', 'tnt_minecart', 'spawner_minecart', 'command_block_minecart']
  .map((name) => `minecraft:${name}`));

// A stack of count drawn as Minecraft draws it: up to five copies.
export const copiesOf = (count) => (count <= 1 ? 1 : count <= 16 ? 2 : count <= 32 ? 3 : count <= 48 ? 4 : 5);

// Everything that decides what an entity looks like, other than where it
// is; it changes, the entity is drawn again from scratch.
export function appearance(entity) {
  return JSON.stringify([entity.type, entity.item, entity.count, entity.block, entity.facing, entity.rotation, entity.variant,
    entity.width, entity.height]);
}

// The yaw-and-pitch part of an entity's transform, by kind.
const orientYaw = (yaw) => rotationY((180 - yaw) * RAD);

// How to draw `entity`: { parts, orient, animated }. A part is
//   { mesh, material: 'atlas' | 'flash', matrix, update? }  (mesh: from this module)
//   { box: { width, height, label }, matrix }               (a type with no model)
// `orient(yaw, pitch)` turns the whole entity; `update(seconds, entity)` of a
// part returns its matrix for that moment, or null to hide it.
export class EntityModels {
  constructor(entityIndex) {
    this.entityIndex = entityIndex;
    this.meshes = new Map();
  }

  mesh(key, make) {
    if (!this.meshes.has(key)) this.meshes.set(key, make());
    return this.meshes.get(key);
  }

  describe(entity) {
    const { type } = entity;
    const bare = type.replace(/^minecraft:/, '');
    if (type === 'minecraft:item') return this.dropped(entity);
    if (type === 'minecraft:falling_block') return this.falling(entity);
    if (type === 'minecraft:tnt') return this.tnt(entity);
    if (MINECARTS.has(type)) return this.cart(entity);
    if (type in this.entityIndex.index.entities && /_(boat|raft|chest_boat|chest_raft)$/.test(type)) return this.boat(entity);
    if (type === 'minecraft:arrow' || type === 'minecraft:spectral_arrow') return this.arrow(entity);
    if (type === 'minecraft:armor_stand') return this.stand(entity);
    if (type === 'minecraft:item_frame' || type === 'minecraft:glow_item_frame') return this.frame(entity);
    if (type === 'minecraft:painting') return this.painting(entity);
    return this.box(entity, bare);
  }

  box(entity, label) {
    return {
      parts: [{ box: { width: entity.width, height: entity.height, label }, matrix: translation(0, entity.height / 2, 0) }],
      orient: () => IDENTITY,
      animated: false,
    };
  }

  // What an item looks like at one block across: { mesh, scale, lift } or null.
  itemDrawing(id) {
    const item = this.entityIndex.index.items[id];
    if (!item) return null;
    if (item.sprite) return { mesh: this.mesh(`sprite:${item.sprite}`, () => spriteMesh(this.entityIndex, item.sprite)), sprite: true };
    return { mesh: this.mesh(`item:${id}`, () => itemBlockMesh(this.entityIndex, id)), sprite: false };
  }

  dropped(entity) {
    const drawing = this.itemDrawing(entity.item);
    if (!drawing) return this.box(entity, entity.item.replace(/^minecraft:/, ''));
    const scale = drawing.sprite ? 0.5 : 0.25;
    const base = hash(entity.id) * 2 * PI;
    const parts = [];
    for (let copy = 0; copy < copiesOf(entity.count || 1); copy += 1) {
      const dx = copy === 0 ? 0 : (hash(entity.id * 7 + copy) - 0.5) * 0.15;
      const dz = copy === 0 ? 0 : (hash(entity.id * 13 + copy) - 0.5) * 0.15;
      const dy = copy * 0.02;
      parts.push({
        mesh: drawing.mesh,
        material: 'atlas',
        matrix: IDENTITY,
        // Minecraft's own: a turn of a radian a second, a bob of a tenth of a block.
        update: (seconds) => compose(
          translation(dx, 0.1 + Math.sin(seconds * 2 + base) * 0.1 + scale * 0.5 + dy, dz),
          rotationY(seconds + base), scaling(scale)),
      });
    }
    return { parts, orient: () => IDENTITY, animated: true };
  }

  falling(entity) {
    const mesh = this.mesh(`block:${entity.block}`, () => blockMesh(this.entityIndex, entity.block));
    if (!mesh) return this.box(entity, 'falling_block');
    return { parts: [{ mesh, material: 'atlas', matrix: translation(0, 0.5, 0) }], orient: () => IDENTITY, animated: false };
  }

  tnt(entity) {
    const mesh = this.mesh('block:minecraft:tnt', () => blockMesh(this.entityIndex, 'minecraft:tnt'));
    // Minecraft's: the block swells in the last ten ticks, and is white for five of every ten.
    const swell = (e) => 1 + Math.min(Math.max(1 - (e.fuse + 1) / 10, 0), 1) ** 4 * 0.3;
    const at = (e, grow) => compose(translation(0, 0.5, 0), scaling(swell(e) * grow));
    return {
      parts: [
        { mesh, material: 'atlas', matrix: at(entity, 1), update: (_, e) => at(e, 1) },
        { mesh, material: 'flash', matrix: at(entity, 1.002), update: (_, e) => (Math.floor(e.fuse / 5) % 2 === 0 ? at(e, 1.002) : null) },
      ],
      orient: () => IDENTITY,
      animated: true,
    };
  }

  cart(entity) {
    const parts = [{ mesh: this.mesh('cart', () => cartMesh(this.entityIndex)), material: 'atlas', matrix: IDENTITY }];
    if (entity.block && !/^minecraft:(air|cave_air|void_air)$/.test(entity.block)) {
      const mesh = this.mesh(`block:${entity.block}`, () => blockMesh(this.entityIndex, entity.block));
      // Minecraft's display block: three quarters size, turned a quarter, standing proud of the cart.
      if (mesh) parts.push({ mesh, material: 'atlas', matrix: compose(translation(0, 0.656, 0), rotationY(PI / 2), scaling(0.75)) });
    }
    return { parts, orient: orientYaw, animated: false };
  }

  boat(entity) {
    const mesh = this.mesh(`boat:${entity.type}`, () => boatMesh(this.entityIndex, entity.type));
    return { parts: [{ mesh, material: 'atlas', matrix: IDENTITY }], orient: (yaw) => rotationY((90 - yaw) * RAD), animated: false };
  }

  arrow(entity) {
    const mesh = this.mesh(`arrow:${entity.type}`, () => arrowMesh(this.entityIndex, entity.type));
    // The arrow's own: its tip (+x) along its flight, pitch down for positive pitch.
    return {
      parts: [{ mesh, material: 'atlas', matrix: translation(0, entity.height / 2, 0) }],
      orient: (yaw, pitch) => compose(rotationY((yaw - 90) * RAD), rotationZ(-pitch * RAD)),
      animated: false,
    };
  }

  stand(entity) {
    const small = entity.height < 1;
    const mesh = this.mesh(`stand:${small}`, () => standMesh(this.entityIndex, small));
    const parts = [{ mesh, material: 'atlas', matrix: IDENTITY }];
    const held = entity.item && this.itemDrawing(entity.item);
    if (held) {
      parts.push({ mesh: held.mesh, material: 'atlas', matrix: compose(standHand(small), scaling(held.sprite ? 0.5 : 0.25)) });
    }
    return { parts, orient: orientYaw, animated: false };
  }

  // A frame is the model at the block next to its wall; the entity is on the wall side of that.
  frame(entity) {
    const glow = entity.type === 'minecraft:glow_item_frame';
    const state = glow ? 'minecraft:glow_item_frame[map=false]' : 'minecraft:item_frame[map=false]';
    const mesh = this.mesh(`block:${state}`, () => blockMesh(this.entityIndex, state));
    const facing = FACING_VECTOR[entity.facing];
    if (!facing) throw new Error(`An item frame faces ${entity.facing}`);
    const turn = facingMatrix(entity.facing);
    const parts = [{ mesh, material: 'atlas', matrix: compose(translation(...facing.map((v) => v * 0.46875)), turn) }];
    const drawing = entity.item && this.itemDrawing(entity.item);
    if (drawing) {
      const size = drawing.sprite ? 0.5 : 0.25;
      parts.push({
        mesh: drawing.mesh,
        material: 'atlas',
        matrix: compose(translation(...facing.map((v) => v * 0.04)), turn, rotationZ((entity.rotation || 0) * 45 * RAD), scaling(size)),
      });
    }
    return { parts, orient: () => IDENTITY, animated: false };
  }

  painting(entity) {
    const facing = FACING_VECTOR[entity.facing];
    if (!facing) throw new Error(`A painting faces ${entity.facing}`);
    const name = entity.variant ? `painting/${entity.variant.replace(/^minecraft:/, '')}` : 'painting/back';
    const mesh = this.mesh(`painting:${name}:${entity.width}:${entity.height}`,
      () => panelMesh(this.entityIndex, name, 'painting/back', entity.width, entity.height));
    return { parts: [{ mesh, material: 'atlas', matrix: facingMatrix(entity.facing) }], orient: () => IDENTITY, animated: false };
  }
}

// Turns a model that faces north to face `facing`.
export function facingMatrix(facing) {
  if (facing === 'up') return rotationX(PI / 2);
  if (facing === 'down') return rotationX(-PI / 2);
  return rotationY(FACING_YAW[facing] * RAD);
}
