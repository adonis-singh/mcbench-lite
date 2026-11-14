'use strict';

// A small baked pack in McbenchBake.java's format, for testing the mesher
// without Minecraft's assets: air, stone, glass, glowstone and a lamp with
// a `lit` property, each a full cube drawn from one textured element.

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
// flags: 1 culling, 2 occluding, 4 culling identical, 128 air; faces: the
// six face shapes light is tested through (down, up, north, south, west,
// east), 0 the empty shape and 1 the full one.
const FULL = [1, 1, 1, 1, 1, 1];
const STATES = [
  { block: 0, flags: 128, emission: 0, lightBlock: 0, selection: NONE }, // air
  { block: 1, flags: 3, emission: 0, lightBlock: 15, faces: FULL, selection: 0 }, // stone
  { block: 2, flags: 4, emission: 0, lightBlock: 0, selection: 1 }, // glass
  { block: 3, flags: 3, emission: 15, lightBlock: 15, faces: FULL, selection: 0 }, // glowstone
  { block: 4, flags: 3, emission: 0, lightBlock: 15, faces: FULL, selection: 0 }, // lamp[lit=false]
  { block: 4, flags: 3, emission: 15, lightBlock: 15, faces: FULL, selection: 0 }, // lamp[lit=true]
];

// Whether each ordered pair of face shapes stops light, row by row: of the
// two shapes, 0 empty and 1 full, a full face stops it.
const FULL_CLOSES = Uint8Array.of(0, 1, 1, 1);

function mesherPack() {
  return pack([['minecraft:air', 0], ['minecraft:stone', 1], ['minecraft:glass', 2], ['minecraft:glowstone', 3], ['minecraft:lamp', 4]], STATES, 2, FULL_CLOSES);
}

// A pack of `states` (each with the light fields above; state 0 air, the
// rest drawn as a full cube), one block of no properties each, named
// minecraft:state_<n>, whose `shapes` face shapes stop light in the pairs
// `occludes` holds (row by row, a byte each): for testing the mesher's light.
function lightPack(states, shapes, occludes) {
  return pack(states.map((state, index) => [index === 0 ? 'minecraft:air' : `minecraft:state_${index}`, index]),
    states.map((state, index) => ({ block: index, flags: index === 0 ? 128 : 3, selection: index === 0 ? NONE : 0, ...state })), shapes, occludes);
}

function pack(blocks, states, shapes, occludes) {
  const out = writer().bytes(Buffer.from('MCBP')).u32(2);
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
      .u16(state.block).u32(0).u32(state.selection);
  }
  out.u32(faceSets.length);
  for (const faces of faceSets) for (const shape of faces.split(',')) out.u8(Number(shape));
  const bits = Buffer.alloc(Math.ceil((shapes * shapes) / 8));
  occludes.forEach((closes, at) => { if (closes) bits[at >> 3] |= 1 << (at & 7); });
  out.u16(shapes).bytes(bits);
  out.u32(0).u32(NONE).u32(0).u8(0); // air, water selection, tint, blends
  out.u32(1).f32(1).f32(1).f32(1); // tints
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
  out.u32(2); // variants
  out.u8(1).u32(0).u8(0).u32(1).u32(0);
  out.u8(1).u32(1).u8(0).u32(1).u32(0);
  out.u32(2).f64(1).u16(1).f64(1).u32(0).f64(1).u16(1).f64(1).u32(1); // variant sets
  out.u32(2).u16(1).u32(0).u16(1).u32(1); // selections
  for (let angle = -180; angle <= 180; angle += 1) for (let i = 0; i < 8; i += 1) out.f32(0);
  for (let i = 0; i < 8; i += 1) out.f32(0);
  return out.done();
}

module.exports = { mesherPack, lightPack };
