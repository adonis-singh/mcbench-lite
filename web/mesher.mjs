// The WebAssembly mesher (renderer/, see its lib.rs): BlueMap's hires tile
// renderer for blueprints, byte for byte, over the baked pack the app made
// with BlueMap (server/McbenchBake.java). It holds one world at a time.

// A blueprint goes into the module in parts of this many bytes at most, so
// the module never holds a copy of it whole.
const PART = 1 << 20;

export class Mesher {
  static async create(wasm, pack) {
    const { instance } = await WebAssembly.instantiate(wasm, {});
    const mesher = new Mesher(instance.exports);
    mesher.check(mesher.module.load_pack(mesher.give(pack), pack.length));
    return mesher;
  }

  constructor(module) {
    this.module = module;
  }

  // Copies `bytes` into the module's memory, which the call it is given to
  // frees. The module's addresses and lengths are unsigned (`>>> 0`): past
  // 2 GiB they come out of an i32 negative.
  give(bytes) {
    const at = this.module.alloc(bytes.length) >>> 0;
    new Uint8Array(this.module.memory.buffer, at, bytes.length).set(bytes);
    return at;
  }

  check(status) {
    if (status === 0) return;
    const { memory, error_ptr: errorPtr, error_len: errorLen } = this.module;
    throw new Error(`The mesher failed: ${new TextDecoder().decode(new Uint8Array(memory.buffer, errorPtr() >>> 0, errorLen() >>> 0))}`);
  }

  output() {
    return new Uint8Array(this.module.memory.buffer, this.module.output_ptr() >>> 0, this.module.output_len() >>> 0);
  }

  // The u32-counted lists of tiles ([x, z] each) the output holds, `count` of them.
  lists(count) {
    const output = this.output();
    const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
    let at = 0;
    return Array.from({ length: count }, () => {
      const tiles = [];
      for (let n = view.getUint32(at, true), i = 0; i < n; i += 1) tiles.push([view.getInt32(at + 4 + i * 8, true), view.getInt32(at + 8 + i * 8, true)]);
      at += 4 + tiles.length * 8;
      return tiles;
    });
  }

  // Reading a blueprint as the world to draw, under `mask`: `begin`, then
  // `take` its bytes in parts of any length as they come, then `finish`.
  begin(mask) {
    this.check(this.module.load_start(mask.minX, mask.maxX, mask.minZ, mask.maxZ));
  }

  take(bytes) {
    for (let at = 0; at < bytes.length; at += PART) {
      const part = bytes.subarray(at, at + PART);
      this.check(this.module.load_take(this.give(part), part.length));
    }
  }

  // Makes the blueprint read the world to draw. Returns its tiles and, with
  // `compare`, those that can draw differently from the world it replaces
  // (else every tile), each as [x, z].
  finish(compare) {
    this.check(this.module.load_finish(compare ? 1 : 0));
    const [tiles, changed] = this.lists(2);
    return { tiles, changed };
  }

  // A blueprint (bytes) whole: begin, take and finish.
  load(blueprint, mask, compare) {
    this.begin(mask);
    this.take(blueprint);
    return this.finish(compare);
  }

  // Applies a patch (bytes; the live contract's format) to the world, which
  // grows or shrinks with it. Returns { tiles, changed }: the world's tiles
  // after it, and those that can draw differently, each as [x, z].
  patch(bytes) {
    this.check(this.module.patch(this.give(bytes), bytes.length));
    const [tiles, changed] = this.lists(2);
    return { tiles, changed };
  }

  // Tile x, z of the world as BlueMap writes it (unindexed .prbm), copied
  // out of the module's memory.
  draw(x, z) {
    this.check(this.module.draw(x, z));
    return this.output().slice();
  }

  // Tile x, z indexed for the GPU (renderer/src/prbm.rs write_indexed): the
  // same triangles, groups and corner bytes with a quad's repeated corners
  // stored once, copied out of the module's memory.
  drawIndexed(x, z) {
    this.check(this.module.draw_indexed(x, z));
    return this.output().slice();
  }

  // What tile x, z reads of the world (World::snapshot), for a mesher that
  // holds no world to draw it with drawSnapshot.
  snapshot(x, z) {
    this.check(this.module.snapshot(x, z));
    return this.output().slice();
  }

  // A snapshot's tile, indexed as drawIndexed draws it from the world.
  drawSnapshot(snapshot) {
    this.check(this.module.draw_snapshot(this.give(snapshot), snapshot.length));
    return this.output().slice();
  }

  // The faces the tile mesher draws of the block state `state` (named as
  // Minecraft names it, "minecraft:oak_sign[rotation=4]", a property left
  // out at its default) alone (lone in renderer/src/lib.rs), those with a
  // cullface toward `culled` hidden (bits over down, up, north, south,
  // west, east): { models, entities, partner }, its models' faces and what
  // Minecraft's block entity renderer draws of it, and the step to its other
  // half (a double chest's, a bed's; 0, 0, 0 for none), each list { count, position
  // (9 per triangle, blocks), uv (6, over the texture), color (3), material
  // (the map's texture index) }, copied out of the module's memory.
  lone(state, culled) {
    const name = new TextEncoder().encode(state);
    this.check(this.module.lone(this.give(name), name.length, culled));
    const output = this.output();
    const view = new DataView(output.buffer, output.byteOffset, output.byteLength);
    let at = 0;
    const list = () => {
      const count = view.getUint32(at, true);
      at += 4;
      const faces = { count, position: new Float32Array(count * 9), uv: new Float32Array(count * 6), color: new Float32Array(count * 3), material: new Int32Array(count) };
      for (let face = 0; face < count; face += 1, at += 76) {
        for (let k = 0; k < 9; k += 1) faces.position[face * 9 + k] = view.getFloat32(at + k * 4, true);
        for (let k = 0; k < 6; k += 1) faces.uv[face * 6 + k] = view.getFloat32(at + 36 + k * 4, true);
        for (let k = 0; k < 3; k += 1) faces.color[face * 3 + k] = view.getFloat32(at + 60 + k * 4, true);
        faces.material[face] = view.getInt32(at + 72, true);
      }
      return faces;
    };
    const models = list();
    const entities = list();
    const partner = [0, 1, 2].map((axis) => view.getInt32(at + axis * 4, true));
    return { models, entities, partner };
  }
}
