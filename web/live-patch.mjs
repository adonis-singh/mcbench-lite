// Live world patches (the engine's TICK, core/src/engine.rs): what a tick of
// the live world changed since the frame before, big-endian like blueprints.
// The world grows and shrinks with them: a block past the bounds or in a
// chunk column that had none, or air at the bounds' edge or in a column's
// last block, moves the bounds and the built columns the patch carries.
//   int newEntries, then each palette entry as blueprints write it (utf id,
//     int n, n x (utf key, utf value)), appended to the stream's palette,
//     whose indices start as the base's and continue from its end
//   int bounds[6]   the bounds of the blocks after the patch (min x, y, z, max x, y, z)
//   int columns     the number of built chunk columns (holding a block) after the patch
//   int n, then n x (int x, int y, int z, short index): each block that differs
//     from the world before, now palette entry `index` (0 is air)
// Also the blueprint format's reader (palette and sections), for what the
// page needs to know of the blocks it shows.

const DECODER = new TextDecoder();
const ENCODER = new TextEncoder();

export function readPatch(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  const int = () => { const value = view.getInt32(at); at += 4; return value; };
  const utf = () => { const n = view.getUint16(at); at += 2; const text = DECODER.decode(bytes.subarray(at, at + n)); at += n; return text; };
  const entries = [];
  for (let count = int(); count > 0; count -= 1) {
    const id = utf();
    const properties = [];
    for (let n = int(); n > 0; n -= 1) properties.push([utf(), utf()]);
    entries.push({ id, properties });
  }
  const bounds = [int(), int(), int(), int(), int(), int()];
  const columns = int();
  const changes = [];
  for (let n = int(); n > 0; n -= 1) {
    const x = int(); const y = int(); const z = int();
    changes.push([x, y, z, view.getUint16(at)]);
    at += 2;
  }
  if (at !== bytes.length) throw new Error('The patch has trailing bytes');
  return { entries, bounds, columns, changes };
}

export function writePatch({ entries, bounds, columns, changes }) {
  const parts = [];
  const int = (value) => { const b = new DataView(new ArrayBuffer(4)); b.setInt32(0, value); parts.push(new Uint8Array(b.buffer)); };
  const utf = (text) => { const body = ENCODER.encode(text); const b = new DataView(new ArrayBuffer(2)); b.setUint16(0, body.length); parts.push(new Uint8Array(b.buffer), body); };
  int(entries.length);
  for (const { id, properties } of entries) {
    utf(id);
    int(properties.length);
    for (const [key, value] of properties) { utf(key); utf(value); }
  }
  for (const value of bounds) int(value);
  int(columns);
  int(changes.length);
  for (const [x, y, z, index] of changes) {
    int(x); int(y); int(z);
    const b = new DataView(new ArrayBuffer(2)); b.setUint16(0, index); parts.push(new Uint8Array(b.buffer));
  }
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) { out.set(part, at); at += part.length; }
  return out;
}

// Everything the stream changed since its base, as one patch: a viewer that
// loads the base and then applies it holds the world the stream's latest
// frame left. A position keeps its latest block.
export class Changes {
  constructor() {
    this.entries = [];
    this.blocks = new Map(); // 'x,y,z' -> [x, y, z, index]
    this.bounds = null;
    this.columns = null;
  }

  add(patch) {
    this.entries.push(...patch.entries);
    for (const change of patch.changes) this.blocks.set(`${change[0]},${change[1]},${change[2]}`, change);
    this.bounds = patch.bounds;
    this.columns = patch.columns;
  }

  patch() {
    return writePatch({ entries: this.entries, bounds: this.bounds, columns: this.columns, changes: [...this.blocks.values()] });
  }
}

// The blueprint's palette and sections (renderer/src/world.rs reads the same
// bytes): { palette: [{ id, properties }], sections: [{ x, y, z, indices (Uint16Array) }] }.
export function readBlueprint(bytes) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let at = 0;
  const int = () => { const value = view.getInt32(at); at += 4; return value; };
  const utf = () => { const n = view.getUint16(at); at += 2; const text = DECODER.decode(bytes.subarray(at, at + n)); at += n; return text; };
  const palette = [];
  for (let count = int(); count > 0; count -= 1) {
    const id = utf();
    const properties = [];
    for (let n = int(); n > 0; n -= 1) properties.push([utf(), utf()]);
    palette.push({ id, properties });
  }
  at += 24; // bounds
  const sections = [];
  for (let count = int(); count > 0; count -= 1) {
    const [x, y, z] = [int(), int(), int()];
    const indices = new Uint16Array(4096);
    for (let i = 0; i < 4096; i += 1) indices[i] = view.getUint16(at + i * 2);
    at += 8192;
    sections.push({ x, y, z, indices });
  }
  return { palette, sections };
}

// What a click on a block does to it, for the blocks that take an input
// (the engine's INPUT): 'toggle' a lever, 'press' a button, 'use' (a right
// click with an empty hand) a door, trapdoor, fence gate, repeater,
// comparator, note block, daylight detector, decorated pot or a dot of
// redstone dust, or null. Iron doors and iron trapdoors cannot be opened by
// hand (BlockSetType.IRON): a click does nothing, so they take none. Dust
// takes one as a dot, which always becomes a cross; a cross becomes a dot
// only when nothing connects to it, which its state does not tell, and other
// dust never changes. A palette entry is a block in one state, so the page's
// index holds the input by entry; it keeps this index of the blocks on show,
// from the blueprint and every patch, to know what a click on a position means.
const USED = /^minecraft:(?!iron_)(?:[a-z_]+_door|[a-z_]*trapdoor|[a-z_]+_fence_gate|repeater|comparator|note_block|daylight_detector|decorated_pot)$/;
const SIDES = new Set(['north', 'east', 'south', 'west']);
export const inputOf = (id, properties = []) => {
  if (id === 'minecraft:lever') return 'toggle';
  if (id.endsWith('_button')) return 'press';
  if (id === 'minecraft:redstone_wire') return properties.every(([key, value]) => !SIDES.has(key) || value === 'none') ? 'use' : null;
  return USED.test(id) ? 'use' : null;
};

export class InputBlocks {
  constructor() {
    this.kinds = []; // palette index -> input kind or null
    this.blocks = new Map(); // 'x,y,z' -> kind
  }

  load(blueprint) {
    this.kinds = blueprint.palette.map(({ id, properties }) => inputOf(id, properties));
    this.blocks.clear();
    if (!this.kinds.some(Boolean)) return;
    for (const { x, y, z, indices } of blueprint.sections) {
      for (let i = 0; i < 4096; i += 1) {
        const kind = this.kinds[indices[i]];
        if (kind) this.blocks.set(`${x * 16 + (i & 15)},${y * 16 + (i >> 8)},${z * 16 + ((i >> 4) & 15)}`, kind);
      }
    }
  }

  apply(patch) {
    for (const { id, properties } of patch.entries) this.kinds.push(inputOf(id, properties));
    for (const [x, y, z, index] of patch.changes) {
      const kind = this.kinds[index];
      if (kind) this.blocks.set(`${x},${y},${z}`, kind);
      else this.blocks.delete(`${x},${y},${z}`);
    }
  }

  at(x, y, z) {
    return this.blocks.get(`${x},${y},${z}`) ?? null;
  }
}

// The blueprint of a world with no blocks: air in its palette, no sections.
export function emptyBlueprint() {
  const air = ENCODER.encode('minecraft:air');
  const bytes = new Uint8Array(4 + 2 + air.length + 4 + 24 + 4);
  const view = new DataView(bytes.buffer);
  view.setInt32(0, 1);
  view.setUint16(4, air.length);
  bytes.set(air, 6);
  return bytes;
}
