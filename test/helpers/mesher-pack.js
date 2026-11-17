'use strict';

// A small baked pack in McbenchBake.java's format, for testing the mesher
// without Minecraft's assets: air, stone, glass, glowstone and a lamp with
// a `lit` property, each a full cube drawn from one textured element, and a
// shrine: the cube without ambient occlusion and, as a block entity
// renderer's quads are, a quad of texture 6 at its middle facing north,
// tinted red.

function writer() {
  const parts = [];
  const out = {
    bytes: (values) => { parts.push(Buffer.from(values)); return out; },
    u8: (value) => out.bytes([value]),
    i8: (value) => out.bytes([value & 0xff]),
    u16: (value) => { const b = Buffer.alloc(2); b.writeUInt16LE(value); parts.push(b); return out; },
    u32: (value) => { const b = Buffer.alloc(4); b.writeUInt32LE(value >>> 0); parts.push(b); return out; },
    f32: (value) => { const b = Buffer.alloc(4); b.writeFloatLE(value); parts.push(b); return out; },
    f64: (value) => { const b = Buffer.alloc(8); b.writeDoubleLE(value); parts.push(b); return out; },
    str: (value) => { const b = Buffer.from(value, 'utf8'); out.u16(b.length); parts.push(b); return out; },
    done: () => Buffer.concat(parts),
  };
  return out;
}

const NONE = 0xffffffff;
// flags: 1 shades, 32 air; faces: the six face shapes light is tested
// through (down, up, north, south, west, east), 0 the empty shape and 1 the
// full one; cull: the cull class (CULLS), 0 empty faces, 1 six full faces,
// 2 glass's empty faces that skip rendering beside glass.
const FULL = [1, 1, 1, 1, 1, 1];
const STATES = [
  { block: 0, flags: 32, emission: 0, lightBlock: 0, cull: 0, selection: NONE }, // air
  { block: 1, flags: 1, emission: 0, lightBlock: 15, faces: FULL, cull: 1, selection: 0 }, // stone
  { block: 2, flags: 0, emission: 0, lightBlock: 0, cull: 2, selection: 1 }, // glass
  { block: 3, flags: 1, emission: 15, lightBlock: 15, faces: FULL, cull: 1, selection: 0 }, // glowstone
  { block: 4, flags: 1, emission: 0, lightBlock: 15, faces: FULL, cull: 1, selection: 0 }, // lamp[lit=false]
  { block: 4, flags: 1, emission: 15, lightBlock: 15, faces: FULL, cull: 1, selection: 0 }, // lamp[lit=true]
  { block: 5, flags: 0, emission: 0, lightBlock: 0, cull: 0, selection: 2 }, // shrine
];
// Cull classes: six cull shapes (0 Shapes.empty(), 1 Shapes.block()),
// skipRendering row and column classes, and the row class of the fluid.
const CULLS = [
  { shapes: [0, 0, 0, 0, 0, 0], row: 0, column: 0 },
  { shapes: [1, 1, 1, 1, 1, 1], row: 0, column: 0 },
  { shapes: [0, 0, 0, 0, 0, 0], row: 1, column: 1 },
];

// Whether each ordered pair of face shapes stops light, row by row: of the
// two shapes, 0 empty and 1 full, a full face stops it.
const FULL_CLOSES = Uint8Array.of(0, 1, 1, 1);

function mesherPack() {
  return pack([['minecraft:air', 0], ['minecraft:stone', 1], ['minecraft:glass', 2], ['minecraft:glowstone', 3], ['minecraft:lamp', 4], ['minecraft:shrine', 6]], STATES, 2, FULL_CLOSES);
}

// A pack of `states` (each with the light fields above; state 0 air, the
// rest drawn as a full cube), one block of no properties each, named
// minecraft:state_<n>, whose `shapes` face shapes stop light in the pairs
// `occludes` holds (row by row, a byte each): for testing the mesher's light.
function lightPack(states, shapes, occludes) {
  return pack(states.map((state, index) => [index === 0 ? 'minecraft:air' : `minecraft:state_${index}`, index]),
    states.map((state, index) => ({ block: index, flags: index === 0 ? 32 : 1, cull: index === 0 ? 0 : 1, selection: index === 0 ? NONE : 0, ...state })), shapes, occludes);
}

function pack(blocks, states, shapes, occludes) {
  const out = writer().bytes(Buffer.from('MCBP')).u32(7);
  out.u32(blocks.length);
  for (const [id, first] of blocks) {
    out.str(id).u32(first);
    if (id === 'minecraft:lamp') out.u8(1).str('lit').u8(0).u8(2).str('false').str('true');
    else out.u8(0);
  }
  // Face sets, set 0 the six empty shapes.
  const faceSets = ['0,0,0,0,0,0'];
  out.u32(states.length);
  for (const state of states) {
    const faces = (state.faces || [0, 0, 0, 0, 0, 0]).join(',');
    if (!faceSets.includes(faces)) faceSets.push(faces);
    out.u8(state.flags).u8(0).u8(state.emission).u8(state.lightBlock).u8(0).u8(faceSets.indexOf(faces))
      .u16(state.cull).u16(state.block).u32(0).u32(state.selection).u16(0);
  }
  out.u32(faceSets.length);
  for (const faces of faceSets) for (const shape of faces.split(',')) out.u8(Number(shape));
  const bits = Buffer.alloc(Math.ceil((shapes * shapes) / 8));
  occludes.forEach((closes, at) => { if (closes) bits[at >> 3] |= 1 << (at & 7); });
  out.u16(shapes).bytes(bits);
  out.u32(CULLS.length);
  for (const { shapes: cull, row, column } of CULLS) {
    for (const shape of cull) out.u8(shape);
    out.u16(row).u16(column).u16(0);
  }
  // Two cull shapes: a face is hidden beside a full one. Row class 1 (glass)
  // skips rendering beside column class 1 (glass) in every direction.
  out.u16(2).bytes([0b1010]);
  out.u16(2).u16(0).u16(0).u16(1).u16(1).u8(63);
  out.u32(0).u32(NONE).u32(0).u8(0); // air, water selection, tint, blends
  // One render class: the seed of its own position, no offset, no fluid,
  // hiding no fluid's side; no fluid groups; the water overlay texture 0.
  out.u32(1).i8(0).i8(0).i8(0).u8(0).f32(0).f32(0).u8(0).u8(0).u8(0).u8(0).f32(-1).f32(-1).f32(-1).f32(-1).u8(0).u8(0);
  out.u32(0).u32(0);
  out.u32(2).f32(1).f32(1).f32(1).f32(1).f32(0).f32(0); // tints: white, red
  out.u32(2); // matrices: an element's 1/16 scale, identity
  for (const scale of [1 / 16, 1]) for (const value of [scale, 0, 0, 0, 0, scale, 0, 0, 0, 0, scale, 0]) out.f32(value);
  out.u32(1); // direction tables: unrotated
  for (let dx = -1; dx <= 1; dx += 1) for (let dy = -1; dy <= 1; dy += 1) for (let dz = -1; dz <= 1; dz += 1) out.i8(dx).i8(dy).i8(dz);
  out.u32(1); // elements: one cube, faces textured by direction, each culled by its neighbour
  for (const value of [0, 0, 0, 16, 16, 16]) out.f32(value);
  out.u32(0).u32(0).u8(6);
  for (let direction = 0; direction < 6; direction += 1) {
    out.u8(direction);
    for (const value of [0, 1, 1, 1, 1, 0, 0, 0]) out.f32(value);
    out.u32(direction).u8(0).u8(direction);
  }
  out.u32(2).u8(1).u16(1).u32(0).u8(0).u16(1).u32(0); // meshes: with and without ambient occlusion
  out.u32(3); // variants: the cube with and without ambient occlusion, the entity mesh
  out.u8(1).u32(0).u8(0).u32(1).u32(0);
  out.u8(1).u32(1).u8(0).u32(1).u32(0);
  out.u8(3).u32(0).u8(0).u8(0).u8(0);
  out.u32(3).f64(1).u16(1).f64(1).u32(0).f64(1).u16(1).f64(1).u32(1).f64(1).u16(1).f64(1).u32(2); // variant sets
  out.u32(3).u8(0).u16(1).u32(0).u8(0).u16(1).u32(1).u8(0).u16(2).u32(1).u32(2); // selections
  out.u32(1); // entity shapes: a square in the middle facing north
  for (const value of [0.75, 0.75, 0.5, 0.75, 0.25, 0.5, 0.25, 0.25, 0.5, 0.25, 0.75, 0.5]) out.f32(value);
  for (const value of [0, 0, 0, 1, 1, 1, 1, 0]) out.f32(value);
  out.u32(1).u32(0).u32(6).u32(1); // entity quads: the square of texture 6 in tint 1
  out.u32(1).u32(1).u32(0); // entity meshes: the quad
  for (let angle = -180; angle <= 180; angle += 1) for (let i = 0; i < 8; i += 1) out.f32(0);
  for (let i = 0; i < 8; i += 1) out.f32(0);
  return out.done();
}

module.exports = { mesherPack, lightPack };
