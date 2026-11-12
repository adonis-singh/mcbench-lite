// A Minecraft world save (a zip of one world folder: level.dat and region/
// r.x.z.mca) of the chunks the engine holds (CoreWorld.export), openable in
// Minecraft 1.21.10 as a singleplayer world or a server's. The chunks are the
// engine's own saved chunks (blocks, block entities, scheduled ticks, the
// light it computed); entities are not saved. Works in any host with
// CompressionStream (browsers and Node).

const DATA_VERSION = 4556;
const MC_VERSION = '1.21.10';

async function transform(bytes, format) {
  const stream = new Blob([bytes]).stream().pipeThrough(new CompressionStream(format));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

// --- NBT (just enough to write level.dat) ---
// A value is [type, payload]: ['int', 3], ['string', 'x'], ['compound', { name: value }],
// ['list', element type, [payloads]], ['longArray'|'intArray', [..]].
const IDS = { byte: 1, short: 2, int: 3, long: 4, float: 5, double: 6, string: 8, list: 9, compound: 10, intArray: 11, longArray: 12 };

function writeNbt(value, out) {
  const [type, payload, items] = value; // a list: [ 'list', element type, payloads ]
  const bytes = (n) => new DataView(new ArrayBuffer(n));
  const push = (view) => out.push(new Uint8Array(view.buffer));
  const text = (string) => {
    const encoded = new TextEncoder().encode(string);
    const length = bytes(2);
    length.setUint16(0, encoded.length);
    push(length);
    out.push(encoded);
  };
  switch (type) {
    case 'byte': out.push(Uint8Array.of(payload & 255)); break;
    case 'short': { const v = bytes(2); v.setInt16(0, payload); push(v); break; }
    case 'int': { const v = bytes(4); v.setInt32(0, payload); push(v); break; }
    case 'long': { const v = bytes(8); v.setBigInt64(0, BigInt(payload)); push(v); break; }
    case 'float': { const v = bytes(4); v.setFloat32(0, payload); push(v); break; }
    case 'double': { const v = bytes(8); v.setFloat64(0, payload); push(v); break; }
    case 'string': text(payload); break;
    case 'list': {
      out.push(Uint8Array.of(payload === 'end' ? 0 : IDS[payload]));
      const n = bytes(4);
      n.setInt32(0, items.length);
      push(n);
      for (const item of items) writeNbt([payload, item], out);
      break;
    }
    case 'compound':
      for (const [name, child] of Object.entries(payload)) {
        out.push(Uint8Array.of(IDS[child[0]]));
        text(name);
        writeNbt(child, out);
      }
      out.push(Uint8Array.of(0));
      break;
    case 'intArray': {
      const v = bytes(4 + payload.length * 4);
      v.setInt32(0, payload.length);
      payload.forEach((n, i) => v.setInt32(4 + i * 4, n));
      push(v);
      break;
    }
    default: throw new Error(`Cannot write an NBT ${type}`);
  }
}

function concat(parts) {
  const out = new Uint8Array(parts.reduce((n, part) => n + part.length, 0));
  let at = 0;
  for (const part of parts) { out.set(part, at); at += part.length; }
  return out;
}

// The root compound of a file, nameless (NbtIo.write).
function nbtFile(compound) {
  const parts = [Uint8Array.of(10, 0, 0)];
  writeNbt(['compound', compound], parts);
  return concat(parts);
}

const bool = (on) => ['byte', on ? 1 : 0];
const str = (text) => ['string', text];

// The data pack that gives the overworld the height builds are placed in
// (the same as server/minecraft-dimension.js writes for the engine; the test
// checks they agree). The world needs it: the engine's chunks are 1024
// blocks tall.
export const MIN_MC_Y = -64;
export const WORLD_HEIGHT = 1024;
export function dimensionPack() {
  return new Map([
    ['datapacks/mcbench/pack.mcmeta', JSON.stringify({ pack: { description: 'mcbench build height', min_format: [88, 0], max_format: [88, 0] } }, null, 2)],
    ['datapacks/mcbench/data/minecraft/dimension_type/overworld.json', JSON.stringify({
      ambient_light: 0, bed_works: true, cloud_height: 192,
      coordinate_scale: 1, effects: 'minecraft:overworld', has_ceiling: false,
      has_raids: true, has_skylight: true, height: WORLD_HEIGHT,
      infiniburn: '#minecraft:infiniburn_overworld', logical_height: WORLD_HEIGHT,
      min_y: MIN_MC_Y, monster_spawn_block_light_limit: 0,
      monster_spawn_light_level: { type: 'minecraft:uniform', max_inclusive: 7, min_inclusive: 0 },
      natural: true, piglin_safe: false, respawn_anchor_works: false, ultrawarm: false,
    }, null, 2)],
  ]);
}

// level.dat's compound (LevelData): a void overworld (one layer of air) in
// creative mode, the engine's game rules (no weather, no daylight cycle, no
// random ticks, no mobs, no fire spread), and the clock where the engine left it.
export function levelData({ name, gameTime, spawn, now = Date.now() }) {
  const rules = {
    doDaylightCycle: 'false', doWeatherCycle: 'false', doMobSpawning: 'false', doFireTick: 'false', randomTickSpeed: '0',
    keepInventory: 'true', spawnRadius: '0', commandBlocksEnabled: 'true', announceAdvancements: 'false',
  };
  const flat = { type: str('minecraft:flat'), settings: ['compound', { layers: ['list', 'compound', [{ block: str('minecraft:air'), height: ['int', 1] }]], features: bool(false), lakes: bool(false), biome: str('minecraft:plains') }] };
  return {
    Data: ['compound', {
      DataVersion: ['int', DATA_VERSION],
      version: ['int', 19133],
      Version: ['compound', { Id: ['int', DATA_VERSION], Name: str(MC_VERSION), Series: str('main'), Snapshot: bool(false) }],
      LevelName: str(name),
      GameType: ['int', 1],
      Difficulty: ['byte', 0],
      DifficultyLocked: bool(false),
      hardcore: bool(false),
      allowCommands: bool(true),
      initialized: bool(true),
      Time: ['long', gameTime],
      DayTime: ['long', 6000],
      LastPlayed: ['long', now],
      raining: bool(false), rainTime: ['int', 0], thundering: bool(false), thunderTime: ['int', 0], clearWeatherTime: ['int', 0],
      spawn: ['compound', { pos: ['intArray', spawn], yaw: ['float', 0], pitch: ['float', 0], dimension: str('minecraft:overworld') }],
      GameRules: ['compound', Object.fromEntries(Object.entries(rules).map(([key, value]) => [key, str(value)]))],
      DragonFight: ['compound', { DragonKilled: bool(false), PreviouslyKilled: bool(false), NeedsStateScanning: bool(true) }],
      CustomBossEvents: ['compound', {}],
      ScheduledEvents: ['list', 'end', []],
      WanderingTraderSpawnDelay: ['int', 0],
      WanderingTraderSpawnChance: ['int', 0],
      DataPacks: ['compound', { Enabled: ['list', 'string', ['vanilla', 'file/mcbench']], Disabled: ['list', 'string', ['minecart_improvements', 'redstone_experiments', 'trade_rebalance']] }],
      WorldGenSettings: ['compound', {
        seed: ['long', 0], generate_features: bool(false), bonus_chest: bool(false),
        dimensions: ['compound', {
          'minecraft:overworld': ['compound', { type: str('minecraft:overworld'), generator: ['compound', flat] }],
          'minecraft:the_nether': ['compound', { type: str('minecraft:the_nether'), generator: ['compound', { type: str('minecraft:noise'), settings: str('minecraft:nether'), biome_source: ['compound', { type: str('minecraft:multi_noise'), preset: str('minecraft:nether') }] }] }],
          'minecraft:the_end': ['compound', { type: str('minecraft:the_end'), generator: ['compound', { type: str('minecraft:noise'), settings: str('minecraft:end'), biome_source: ['compound', { type: str('minecraft:the_end') }] }] }],
        }],
      }],
    }],
  };
}

// --- Anvil region files ---
const SECTOR = 4096;

// The files of a region folder: name -> bytes. `chunks`: [{ x, z, nbt }]
// (nbt uncompressed, as CoreWorld.export gives them).
export async function regionFiles(chunks, timestamp = Math.floor(Date.now() / 1000)) {
  const regions = new Map();
  for (const chunk of chunks) {
    const key = `r.${chunk.x >> 5}.${chunk.z >> 5}.mca`;
    if (!regions.has(key)) regions.set(key, []);
    regions.get(key).push(chunk);
  }
  const files = new Map();
  for (const [name, members] of regions) {
    const header = new DataView(new ArrayBuffer(2 * SECTOR));
    const bodies = [];
    let sector = 2;
    for (const chunk of members) {
      const compressed = await transform(chunk.nbt, 'deflate');
      const length = compressed.length + 1;
      const sectors = Math.ceil((length + 4) / SECTOR);
      if (sectors > 255) throw new Error(`Chunk ${chunk.x}, ${chunk.z} is too large for a region file`);
      const index = (chunk.x & 31) + (chunk.z & 31) * 32;
      header.setUint32(index * 4, (sector << 8) | sectors);
      header.setUint32(SECTOR + index * 4, timestamp);
      const body = new Uint8Array(sectors * SECTOR);
      new DataView(body.buffer).setInt32(0, length);
      body[4] = 2; // zlib
      body.set(compressed, 5);
      bodies.push(body);
      sector += sectors;
    }
    files.set(name, concat([new Uint8Array(header.buffer), ...bodies]));
  }
  return files;
}

// --- zip (entries stored, not compressed: region files are already) ---
const CRC_TABLE = (() => {
  const table = new Uint32Array(256);
  for (let n = 0; n < 256; n += 1) {
    let c = n;
    for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    table[n] = c >>> 0;
  }
  return table;
})();

function crc32(bytes) {
  let crc = 0xffffffff;
  for (let i = 0; i < bytes.length; i += 1) crc = CRC_TABLE[(crc ^ bytes[i]) & 255] ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}

// A zip of `files` (name -> bytes).
export function zip(files, date = new Date()) {
  const dosTime = (date.getHours() << 11) | (date.getMinutes() << 5) | (date.getSeconds() >> 1);
  const dosDate = ((date.getFullYear() - 1980) << 9) | ((date.getMonth() + 1) << 5) | date.getDate();
  const local = [];
  const central = [];
  let offset = 0;
  for (const [name, bytes] of files) {
    const encoded = new TextEncoder().encode(name);
    const crc = crc32(bytes);
    const head = new DataView(new ArrayBuffer(30));
    head.setUint32(0, 0x04034b50, true);
    head.setUint16(4, 20, true);
    head.setUint16(6, 0x0800, true); // UTF-8 names
    head.setUint16(10, dosTime, true);
    head.setUint16(12, dosDate, true);
    head.setUint32(14, crc, true);
    head.setUint32(18, bytes.length, true);
    head.setUint32(22, bytes.length, true);
    head.setUint16(26, encoded.length, true);
    local.push(new Uint8Array(head.buffer), encoded, bytes);
    const entry = new DataView(new ArrayBuffer(46));
    entry.setUint32(0, 0x02014b50, true);
    entry.setUint16(4, 20, true);
    entry.setUint16(6, 20, true);
    entry.setUint16(8, 0x0800, true);
    entry.setUint16(12, dosTime, true);
    entry.setUint16(14, dosDate, true);
    entry.setUint32(16, crc, true);
    entry.setUint32(20, bytes.length, true);
    entry.setUint32(24, bytes.length, true);
    entry.setUint16(28, encoded.length, true);
    entry.setUint32(42, offset, true);
    central.push(new Uint8Array(entry.buffer), encoded);
    offset += 30 + encoded.length + bytes.length;
  }
  const centralBytes = concat(central);
  const end = new DataView(new ArrayBuffer(22));
  end.setUint32(0, 0x06054b50, true);
  end.setUint16(8, files.size, true);
  end.setUint16(10, files.size, true);
  end.setUint32(12, centralBytes.length, true);
  end.setUint32(16, offset, true);
  return concat([...local, centralBytes, new Uint8Array(end.buffer)]);
}

// The save of an engine export ({ gameTime, chunks }) as a zip's bytes: one
// folder `name` holding level.dat and region/. `spawn`: [x, y, z].
export async function worldZip({ name, gameTime, chunks }, spawn) {
  // A title of dots alone would name the zip's parent folder.
  const cleaned = name.replace(/[\\/:*?"<>|\u0000-\u001f]/g, '_').trim();
  const folder = /^\.*$/.test(cleaned) ? 'world' : cleaned;
  const files = new Map();
  files.set(`${folder}/level.dat`, await transform(nbtFile(levelData({ name, gameTime, spawn })), 'gzip'));
  for (const [file, text] of dimensionPack()) files.set(`${folder}/${file}`, new TextEncoder().encode(text));
  for (const [file, bytes] of await regionFiles(chunks)) files.set(`${folder}/region/${file}`, bytes);
  return { folder, bytes: zip(files) };
}
