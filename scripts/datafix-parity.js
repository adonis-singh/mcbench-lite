'use strict';

// Checks the Rust port of Minecraft's data fixer (core/src/datafix) against
// the server's own (scripts/DataFixReference.java, DataFixers.getDataFixer()
// as the experiment calls it): the same saved data, at the same data version,
// upgraded to the server's version by both, must print the same NBT. A line
// the port refuses ("unported: ...") counts as refused (separately when the
// server's fixer throws on it too), not as a match.
//
//   MCBENCH_DATA_DIR=<scratch copy> node scripts/datafix-parity.js
//     [--real]                 every saved block entity, entity and stack with a data version in the
//                              directories MCBENCH_CORPUS names (scripts/corpus.js), the data folder's
//                              worlds and scripts/verifier-fixtures.js
//     [--fuzz N] [--seed S]    N random old inputs of each type (default 2000)
//     [--min V] [--max V]      the data versions the fuzzer draws from
//     [--lines file]           lines "<type> <version> <snbt>" as they are
//     [--keep folder]          keep the lines and both answers there
//     [--dump]                 rewrite core/minecraft/<version>/datafix.txt from the server

const fs = require('fs');
const os = require('os');
const path = require('path');
const { execFileSync, spawnSync } = require('child_process');
const { minecraftLive } = require('../server/minecraft-live');
const { buildCore, graalvmHome } = require('../server/engine');
const { corpusDirs } = require('./corpus');

const ROOT = path.join(__dirname, '..');
const option = (name, fallback) => {
  const at = process.argv.indexOf(`--${name}`);
  return at < 0 ? fallback : process.argv[at + 1];
};
const flag = (name) => process.argv.includes(`--${name}`);

// --- The Java reference, compiled against the server's classpath.
async function reference() {
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const classes = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-datafix-'));
  const bin = path.join(await graalvmHome(), 'bin');
  execFileSync(path.join(bin, 'javac'), ['--release', '25', '-proc:none', '-nowarn', '-classpath', classpath.join(path.delimiter), '-d', classes, path.join(__dirname, 'DataFixReference.java')], { stdio: 'inherit' });
  const run = (args, input) => {
    const r = spawnSync(path.join(bin, 'java'), ['-Xss16m', '-cp', [...classpath, classes].join(path.delimiter), 'DataFixReference', ...args], { input, maxBuffer: 1 << 30, cwd: classes });
    if (r.status !== 0) throw new Error(`DataFixReference ${args.join(' ')} failed: ${r.stderr}`);
    return r.stdout.toString('utf8').split('\n').filter((l) => l.includes('@@')).map((l) => l.slice(l.indexOf('@@') + 2));
  };
  return { run, classes, classpath, bin };
}

// --- Inputs from the files: {kind, version, snbt} of every object with a
// data_version and snbt, block entities without an id given their block's.
function realInputs(blockEntityTypes) {
  const roots = [...corpusDirs(), path.join(process.env.MCBENCH_DATA_DIR || path.join(ROOT, 'data'), 'worlds')];
  const files = [];
  const walk = (p) => {
    let st;
    try { st = fs.lstatSync(p); } catch { return; }
    if (st.isSymbolicLink()) return;
    if (st.isDirectory()) { for (const n of fs.readdirSync(p)) if (n !== 'node_modules' && n !== '.git') walk(path.join(p, n)); return; }
    if (p.endsWith('.json') && st.size < 300e6) files.push(p);
  };
  for (const r of roots) walk(r);
  const seen = new Set();
  const lines = [];
  const add = (kind, version, snbt) => {
    const line = `${kind} ${version} ${snbt.replace(/\n/g, ' ')}`;
    if (!seen.has(line)) { seen.add(line); lines.push(line); }
  };
  const visit = (v, key, blocks) => {
    if (Array.isArray(v)) { for (const x of v) visit(x, key, blocks); return; }
    if (!v || typeof v !== 'object') return;
    if (Array.isArray(v.blocks) && v.blocks[0] && v.blocks[0].pos) blocks = new Map(v.blocks.map((b) => [String(b.pos), b.id]));
    if ('data_version' in v && typeof v.snbt === 'string' && Number.isInteger(v.data_version)) {
      if (key === 'block_entities') {
        let snbt = v.snbt;
        if (!hasTopId(snbt)) {
          const type = blocks && v.at && blockEntityTypes.get(blocks.get(String(v.at)));
          if (!type) return;
          snbt = /^\{\s*\}$/.test(snbt) ? `{id: "${type}"}` : snbt.replace(/^\{\s*/, `{id: "${type}", `);
        }
        add('block_entity', v.data_version, snbt);
      } else if (key === 'entities') add(/Passengers/.test(v.snbt) ? 'entity_tree' : 'entity', v.data_version, v.snbt);
      else if (key === 'stack') add('item_stack', v.data_version, v.snbt);
    }
    for (const [k, x] of Object.entries(v)) visit(x, k, blocks);
  };
  for (const f of files) {
    let j;
    try { j = JSON.parse(fs.readFileSync(f, 'utf8')); } catch { continue; }
    visit(j, '', null);
  }
  // The verifier fixtures' requests.
  try {
    const fixtures = require('./verifier-fixtures');
    visit(JSON.parse(JSON.stringify(fixtures, (k, v) => (typeof v === 'function' ? undefined : v))), '', null);
  } catch { /* the fixtures module exports functions only */ }
  return lines;
}

// Whether the compound's own keys (depth 1, outside strings) include id.
function hasTopId(snbt) {
  let depth = 0, quote = null;
  for (let i = 0; i < snbt.length; i++) {
    const c = snbt[i];
    if (quote) { if (c === '\\') i++; else if (c === quote) quote = null; continue; }
    if (c === '"' || c === "'") { quote = c; continue; }
    if (c === '{' || c === '[') depth++;
    else if (c === '}' || c === ']') depth--;
    else if (depth === 1 && /[{,\s]/.test(snbt[i - 1] || '{') && /^"?id"?\s*:/.test(snbt.slice(i))) return true;
  }
  return false;
}

// --- The fuzzer: old saved data of each type, drawn from the schemas' ids
// (datafix.txt) and the fixer's own string constants.
function mulberry(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6D2B79F5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

// Each schema's block entity and entity ids (the choices of their types).
function schemaIds() {
  const lines = fs.readFileSync(path.join(ROOT, 'core', 'minecraft', minecraftLive.minecraftVersion, 'datafix.txt'), 'utf8').split('\n');
  const nodes = new Map();
  const schemas = [];
  for (const l of lines) {
    const p = l.split(' ');
    if (p[0] === 'T') nodes.set(Number(p[1]), p.slice(2));
    else if (p[0] === 'S') schemas.push(p);
  }
  const choices = (id, seen = new Set()) => {
    if (seen.has(id)) return [];
    seen.add(id);
    const n = nodes.get(id);
    if (n[0] === 'choice') { const keys = []; for (let i = 0; i < Number(n[3]); i++) keys.push(n[4 + 2 * i]); return keys; }
    const kids = n[0] === 'named' || n[0] === 'field' ? [Number(n[2])] : n[0] === 'and' || n[0] === 'or' ? [Number(n[1]), Number(n[2])] : n[0] === 'hook' || n[0] === 'list' ? [Number(n[1])] : [];
    return kids.flatMap((k) => choices(k, seen));
  };
  return schemas.map((p) => {
    const types = Object.fromEntries(p.slice(p.indexOf('types') + 1).map((x) => x.split('=').map((y, i) => (i ? Number(y) : y))));
    return { key: Number(p[1]), blockEntities: choices(types.block_entity), entities: choices(types.entity) };
  });
}

// SNBT for the generated values: {t, v} with t one of b s i l f d str list compound ia la ba.
function snbt(x) {
  switch (x.t) {
    case 'b': return `${x.v}b`;
    case 's': return `${x.v}s`;
    case 'i': return `${x.v}`;
    case 'l': return `${x.v}L`;
    case 'f': return `${x.v}f`;
    case 'd': return `${x.v}d`;
    case 'str': return JSON.stringify(x.v);
    case 'ia': return `[I; ${x.v.join(', ')}]`;
    case 'la': return `[L; ${x.v.map((n) => `${n}L`).join(', ')}]`;
    case 'ba': return `[B; ${x.v.map((n) => `${n}B`).join(', ')}]`;
    case 'list': return `[${x.v.map(snbt).join(', ')}]`;
    case 'compound': return `{${Object.entries(x.v).map(([k, v]) => `${/^[A-Za-z._][A-Za-z0-9._+-]*$/.test(k) && !/^(true|false)$/i.test(k) ? k : JSON.stringify(k)}: ${snbt(v)}`).join(', ')}}`;
    default: throw new Error(x.t);
  }
}

function generator(seed, vocabulary) {
  const r = mulberry(seed);
  const int = (n) => Math.floor(r() * n);
  const pick = (a) => a[int(a.length)];
  const chance = (p) => r() < p;
  const ids = vocabulary.filter((s) => /^minecraft:[a-z0-9_./-]+$/.test(s));
  const keys = vocabulary.filter((s) => /^[A-Za-z_][A-Za-z0-9_]{0,40}$/.test(s));
  const components = vocabulary.filter((s) => /^minecraft:[a-z_/]+$/.test(s));
  const translations = vocabulary.filter((s) => /^[a-z_]+\.[a-z_.]+$/.test(s));
  const plainIds = ids.map((s) => s.slice('minecraft:'.length));
  const NUMBERS = [0, 1, -1, 2, 3, 4, 5, 7, 8, 10, 15, 16, 20, 33, 40, 48, 64, 100, 127, 255, 256, 1000];
  const FLOATS = ['0.0', '0.085', '0.5', '1.0', '1.6', '2.0', '6.0', '16.0', '20.0', '40.0', '48.0', '180.0', '-1.5', '0.25'];
  const num = () => {
    const t = pick(['b', 's', 'i', 'i', 'l', 'f', 'd']);
    if (t === 'f' || t === 'd') return { t, v: pick(FLOATS) };
    let v = pick(NUMBERS) * (chance(0.1) ? -1 : 1);
    if (t === 'b') v = Math.max(-128, Math.min(127, v));
    if (t === 's') v = Math.max(-32768, Math.min(32767, v));
    return { t, v };
  };
  const word = () => pick(['a', 'x', 'Steve', 'chest', 'Filter', '1', '', ' ', 'name with space', 'ünïcode', 'quote"s', "apos'", 'back\\slash', '§cRed', 'gold', 'true', 'TRUE', 'null']);
  const uuidString = () => {
    const hex = (n) => Array.from({ length: n }, () => '0123456789abcdef'[int(16)]).join('');
    return pick([`${hex(8)}-${hex(4)}-${hex(4)}-${hex(4)}-${hex(12)}`, `${hex(3)}-${hex(1)}-${hex(2)}-${hex(4)}-${hex(5)}`, 'not-a-uuid', hex(32), `${hex(8)}-${hex(4)}-${hex(4)}-${hex(4)}-${hex(12)}-1`]);
  };
  // Text components as JSON (strict, lenient and not JSON at all).
  const textJson = (depth = 0) => {
    const s = (v) => JSON.stringify(v);
    const color = () => pick(['red', 'gold', '#ff0000', 'dark_blue']);
    const base = pick([
      () => s({ text: word() }),
      () => s({ text: word(), color: color(), bold: pick([true, 'true', 'false', 1]) }),
      () => s({ translate: pick(translations.length ? translations : ['block.minecraft.chest']) }),
      () => s({ translate: pick(['block.minecraft.ominous_banner', 'block.minecraft.illager_banner', 'filled_map.mansion', 'filled_map.monument']) }),
      () => s(word()),
      () => word(),
      () => '',
      () => 'null',
      () => `[${s({ text: word() })},${s(word())}]`,
      () => `{"text":"${word().replace(/[\\"]/g, '')}","extra":[${depth < 2 ? textJson(depth + 1) : '"e"'}]}`,
      () => s({ text: 'h', hoverEvent: { action: 'show_text', value: pick([word(), { text: word() }]) } }),
      () => s({ text: 'h', hoverEvent: { action: 'show_text', contents: { text: word() } } }),
      () => s({ text: 'h', hoverEvent: { action: 'show_item', value: '{id:"minecraft:stone",Count:1b}' } }),
      () => s({ text: 'h', hoverEvent: { action: 'show_item', contents: pick([{ id: 'minecraft:stone', count: 2 }, 'minecraft:dirt']) } }),
      () => s({ text: 'h', hoverEvent: { action: 'show_entity', contents: { id: uuidString(), type: 'minecraft:pig', name: { text: 'p' } } } }),
      () => s({ text: 'c', clickEvent: { action: pick(['open_url', 'run_command', 'suggest_command', 'change_page', 'open_file', 'copy_to_clipboard']), value: pick(['https://example.com/a?b=c#d', 'http://x.y', 'ftp://x', 'www.example.com', '/say hi', 'say §c', '3', 3, '0', 'abc', 'C:/file']) } }),
      () => `{text:'${word().replace(/['\\]/g, '')}'}`,
      () => `{'text':1.5e2,"italic":TRUE}`,
      () => `{"text":007}`,
      () => `{"text":"a"} trailing`,
      () => `{"text":"a",}`,
      () => `// comment\n{"text":"c"}`,
      () => `{"text":"a\\u00e9\\n"}`,
      () => s({ text: '', extra: [s({ text: 'nested string' })] }),
    ])();
    return base;
  };
  const strValue = () => pick([
    () => ({ t: 'str', v: word() }),
    () => ({ t: 'str', v: pick(ids) }),
    () => ({ t: 'str', v: pick(plainIds) }),
    () => ({ t: 'str', v: textJson() }),
    () => ({ t: 'str', v: uuidString() }),
    () => ({ t: 'str', v: pick(keys) }),
  ])();
  const anyValue = (depth) => {
    const choices = [() => num(), () => num(), () => strValue(), () => strValue()];
    if (depth < 3) choices.push(() => compound(depth + 1, 3), () => ({ t: 'list', v: Array.from({ length: int(3) }, () => (chance(0.5) ? compound(depth + 1, 3) : num())) }));
    choices.push(() => ({ t: 'ia', v: Array.from({ length: pick([0, 3, 4, 4, 5]) }, () => int(2000) - 1000) }));
    return pick(choices)();
  };
  const compound = (depth, n) => {
    const v = {};
    for (let i = int(n + 1); i > 0; i--) v[pick(keys)] = anyValue(depth);
    return { t: 'compound', v };
  };
  const textValue = () => ({ t: 'str', v: textJson() });
  const itemId = () => pick([pick(ids), pick(ids), pick(plainIds), pick(['minecraft:written_book', 'minecraft:writable_book', 'minecraft:shulker_box', 'minecraft:white_banner', 'minecraft:filled_map', 'minecraft:potion', 'minecraft:tipped_arrow', 'minecraft:player_head', 'minecraft:crossbow', 'minecraft:bundle', 'minecraft:compass', 'minecraft:firework_rocket', 'minecraft:firework_star', 'minecraft:enchanted_book', 'minecraft:salmon_bucket', 'minecraft:axolotl_bucket', 'minecraft:tropical_fish_bucket', 'minecraft:painting', 'minecraft:goat_horn', 'minecraft:suspicious_stew', 'minecraft:spawn_egg', 'minecraft:pig_spawn_egg', 'minecraft:chain', 'minecraft:grass', 'minecraft:scute', 'minecraft:debug_stick', 'minecraft:knowledge_book'])]);
  // An item stack of the old form (Count, tag) or the new (count, components).
  const item = (depth) => {
    const v = { id: { t: 'str', v: itemId() } };
    if (chance(0.8)) v.Count = { t: pick(['b', 'b', 'i']), v: pick([1, 2, 7, 64, 0]) };
    else v.count = { t: 'i', v: pick([1, 3, 64]) };
    if (chance(0.3)) v.Slot = { t: 'b', v: int(27) };
    if (chance(0.15)) v.Damage = { t: 's', v: int(16) };
    if (chance(0.7) && depth < 3) v.tag = itemTag(depth + 1);
    if (chance(0.15) && depth < 3) v.components = componentsValue(depth + 1);
    if (chance(0.05)) delete v.id;
    return { t: 'compound', v };
  };
  const enchantments = () => ({ t: 'list', v: Array.from({ length: int(3) }, () => ({ t: 'compound', v: { id: { t: 'str', v: pick(['minecraft:sharpness', 'minecraft:sweeping', 'sweeping', 'minecraft:efficiency', 'unbreaking']) }, lvl: pick([{ t: 's', v: pick([0, 1, 3, 300, -1]) }, { t: 'i', v: pick([0, 1, 300]) }, { t: 'b', v: pick([0, 1, -1]) }]) } })) });
  const itemTag = (depth) => {
    const v = {};
    const add = (k, f) => { if (chance(0.18)) v[k] = f(); };
    add('Damage', () => ({ t: 'i', v: int(5) }));
    add('RepairCost', () => ({ t: 'i', v: int(3) }));
    add('CustomModelData', () => ({ t: 'i', v: int(9) }));
    add('HideFlags', () => ({ t: 'i', v: int(256) }));
    add('Unbreakable', () => ({ t: 'b', v: int(2) }));
    add('Enchantments', enchantments);
    add('StoredEnchantments', enchantments);
    add('display', () => ({ t: 'compound', v: Object.fromEntries([['Name', textValue()], ['Lore', { t: 'list', v: Array.from({ length: int(3) }, textValue) }], ['color', { t: 'i', v: int(16777215) }], ['LocName', { t: 'str', v: pick(translations.length ? translations : ['item.x']) }], ['MapColor', { t: 'i', v: 3 }]].filter(() => chance(0.5))) }));
    add('BlockEntityTag', () => blockEntity(depth + 1, pick(['minecraft:chest', 'minecraft:shulker_box', 'minecraft:banner', 'minecraft:decorated_pot', 'minecraft:skull', 'minecraft:beehive', 'minecraft:barrel', 'minecraft:sign', 'minecraft:furnace']), true));
    add('EntityTag', () => entity(depth + 1, null));
    add('BlockStateTag', () => ({ t: 'compound', v: { [pick(['powered', 'lit', 'facing', 'level', 'axis'])]: pick([{ t: 'b', v: 1 }, { t: 'str', v: 'true' }, { t: 'i', v: 3 }, { t: 'f', v: '1.5' }]) } }));
    add('CanDestroy', () => ({ t: 'list', v: [{ t: 'str', v: pick(['minecraft:stone', 'stone[axis=x, a = b]', 'minecraft:chest{Items:[]}', ' dirt ', 'a]b[', 'x[a=b]{c:d}']) }] }));
    add('CanPlaceOn', () => ({ t: 'list', v: [{ t: 'str', v: pick(['minecraft:stone', 'stone[axis=x]']) }] }));
    add('AttributeModifiers', () => ({ t: 'list', v: [{ t: 'compound', v: { AttributeName: { t: 'str', v: pick(['generic.maxHealth', 'generic.attackDamage', 'minecraft:horse.jump_strength', 'minecraft:generic.max_health']) }, Name: { t: 'str', v: pick(['Random spawn bonus', 'x']) }, Amount: { t: 'd', v: '2.0' }, Operation: { t: 'i', v: int(4) }, UUIDMost: { t: 'l', v: 123 }, UUIDLeast: { t: 'l', v: 456 }, UUID: { t: 'ia', v: pick([[1, 2, 3, 4], [-1868810074, 869744711, -1904826485, 152698757]]) }, Slot: { t: 'str', v: 'mainhand' } } }] }));
    add('Trim', () => ({ t: 'compound', v: { material: { t: 'str', v: 'minecraft:gold' }, pattern: { t: 'str', v: 'minecraft:coast' } } }));
    add('Charged', () => ({ t: 'b', v: 1 }));
    add('ChargedProjectiles', () => ({ t: 'list', v: depth < 3 ? [item(depth + 1)] : [] }));
    add('Items', () => ({ t: 'list', v: depth < 3 ? [item(depth + 1)] : [] }));
    add('map', () => ({ t: 'i', v: 4 }));
    add('Decorations', () => ({ t: 'list', v: [{ t: 'compound', v: { id: { t: 'str', v: pick(['a', '+']) }, type: { t: 'b', v: int(40) }, x: { t: 'd', v: '1.0' }, z: { t: 'd', v: '2.0' }, rot: { t: 'd', v: '180.0' } } }] }));
    add('Potion', () => ({ t: 'str', v: pick(['minecraft:empty', 'minecraft:water', 'minecraft:strength']) }));
    add('CustomPotionColor', () => ({ t: 'i', v: 255 }));
    add('CustomPotionEffects', () => ({ t: 'list', v: [{ t: 'compound', v: { Id: { t: 'b', v: int(35) }, Amplifier: { t: 'b', v: 1 }, Duration: { t: 'i', v: 200 }, FactorCalculationData: { t: 'compound', v: { effect_changed_timestamp: { t: 'i', v: 50 } } } } }] }));
    add('custom_potion_effects', () => ({ t: 'list', v: [] }));
    add('pages', () => ({ t: 'list', v: Array.from({ length: int(3) }, textValue) }));
    add('filtered_pages', () => ({ t: 'compound', v: { 0: textValue(), 5: textValue() } }));
    add('title', () => ({ t: 'str', v: word() }));
    add('filtered_title', () => ({ t: 'str', v: word() }));
    add('author', () => ({ t: 'str', v: word() }));
    add('resolved', () => ({ t: 'b', v: 1 }));
    add('generation', () => ({ t: 'i', v: int(3) }));
    add('Effects', () => ({ t: 'list', v: [{ t: 'compound', v: { EffectId: { t: 'b', v: int(35) }, EffectDuration: { t: 'i', v: 160 } } }] }));
    add('effects', () => ({ t: 'list', v: [] }));
    add('DebugProperty', () => ({ t: 'compound', v: {} }));
    add('Variant', () => ({ t: 'i', v: int(5) }));
    add('BucketVariantTag', () => ({ t: 'i', v: int(1 << 30) }));
    add('Health', () => ({ t: 'f', v: '4.0' }));
    add('instrument', () => ({ t: 'str', v: 'minecraft:ponder_goat_horn' }));
    add('SoundVariant', () => ({ t: 'i', v: int(10) - 1 }));
    add('Recipes', () => ({ t: 'list', v: [] }));
    add('LodestonePos', () => ({ t: 'compound', v: { X: { t: 'i', v: 1 }, Y: { t: 'i', v: 2 }, Z: { t: 'i', v: 3 } } }));
    add('LodestoneDimension', () => ({ t: 'str', v: 'minecraft:overworld' }));
    add('LodestoneTracked', () => ({ t: 'b', v: int(2) }));
    add('Fireworks', () => ({ t: 'compound', v: { Flight: { t: 'b', v: 2 }, Explosions: { t: 'list', v: [{ t: 'compound', v: { Type: { t: 'b', v: int(6) }, Colors: { t: 'ia', v: [1, 2] }, Trail: { t: 'b', v: 1 } } }] } } }));
    add('Explosion', () => ({ t: 'compound', v: { Type: { t: 'b', v: int(6) }, Flicker: { t: 'b', v: 1 } } }));
    add('SkullOwner', () => pick([() => ({ t: 'str', v: pick(['Notch', 'a b', 'waytoolongname_for_a_player']) }), () => ({ t: 'compound', v: { Name: { t: 'str', v: 'Steve' }, Id: pick([{ t: 'str', v: uuidString() }, { t: 'ia', v: [1, 2, 3, 4] }]), Properties: { t: 'compound', v: { textures: { t: 'list', v: [{ t: 'compound', v: { Value: { t: 'str', v: 'abc' }, Signature: { t: 'str', v: 'sig' } } }] } } } } })])());
    if (chance(0.3)) Object.assign(v, compound(depth + 1, 2).v);
    return { t: 'compound', v };
  };
  const componentsValue = (depth) => {
    const v = {};
    for (let i = int(4); i > 0; i--) v[pick(components)] = anyValue(depth + 1);
    if (chance(0.3)) v['minecraft:custom_name'] = textValue();
    if (chance(0.2)) v['minecraft:bucket_entity_data'] = { t: 'compound', v: { Variant: { t: 'i', v: 2 }, type: { t: 'str', v: 'large' }, BucketVariantTag: { t: 'i', v: 65536 } } };
    if (chance(0.2)) v['minecraft:lock'] = pick([{ t: 'str', v: 'key' }, { t: 'str', v: '' }, { t: 'compound', v: { components: { t: 'compound', v: { 'minecraft:custom_name': { t: 'str', v: '""' } } } } }]);
    if (chance(0.2)) v['minecraft:food'] = { t: 'compound', v: { eat_seconds: { t: 'f', v: '2.0' }, effects: { t: 'list', v: [{ t: 'compound', v: { effect: { t: 'compound', v: { id: { t: 'str', v: 'minecraft:speed' } } }, probability: { t: 'f', v: '0.5' } } }] } } };
    if (chance(0.2)) v['minecraft:attribute_modifiers'] = { t: 'compound', v: { modifiers: { t: 'list', v: [{ t: 'compound', v: { type: { t: 'str', v: 'generic.armor' }, uuid: { t: 'ia', v: [1, 2, 3, 4] }, name: { t: 'str', v: 'Random spawn bonus' }, amount: { t: 'd', v: '1.0' }, operation: { t: 'str', v: 'add_value' } } }] }, show_in_tooltip: { t: 'b', v: 0 } } };
    if (chance(0.2)) v['minecraft:enchantments'] = { t: 'compound', v: { levels: { t: 'compound', v: { 'minecraft:sharpness': { t: 'i', v: 2 } } }, show_in_tooltip: { t: 'b', v: int(2) } } };
    if (chance(0.15)) v['minecraft:hide_additional_tooltip'] = { t: 'compound', v: {} };
    if (chance(0.15)) v['minecraft:hide_tooltip'] = { t: 'compound', v: {} };
    if (chance(0.15)) v['minecraft:can_break'] = { t: 'compound', v: { predicates: { t: 'list', v: [{ t: 'compound', v: { blocks: { t: 'str', v: 'minecraft:stone' } } }] }, show_in_tooltip: { t: 'b', v: 0 } } };
    if (chance(0.15) && depth < 3) v['minecraft:container'] = { t: 'list', v: [{ t: 'compound', v: { slot: { t: 'i', v: 0 }, item: item(depth + 1) } }] };
    if (chance(0.15)) v['minecraft:custom_model_data'] = { t: 'i', v: 3 };
    return { t: 'compound', v };
  };
  const signText = () => ({ t: 'compound', v: { messages: { t: 'list', v: Array.from({ length: 4 }, textValue) }, ...(chance(0.5) ? { filtered_messages: { t: 'list', v: Array.from({ length: 4 }, textValue) } } : {}), color: { t: 'str', v: 'black' } } });
  const blockEntity = (depth, id, inItem = false) => {
    const v = {};
    if (id) v.id = { t: 'str', v: id };
    if (chance(0.5)) v.Items = { t: 'list', v: depth < 3 ? Array.from({ length: int(4) }, () => item(depth + 1)) : [] };
    if (chance(0.3)) v.CustomName = textValue();
    if (chance(0.2)) v.Lock = { t: 'str', v: pick(['key', '', 'a"b']) };
    if (chance(0.1)) v.lock = pick([{ t: 'compound', v: { components: { t: 'compound', v: { 'minecraft:custom_name': { t: 'str', v: '""' } } } } }, { t: 'compound', v: {} }]);
    if (chance(0.15)) v.LootTable = { t: 'str', v: 'minecraft:chests/simple_dungeon' };
    if (chance(0.1)) v.LootTableSeed = { t: 'l', v: int(5) };
    const extra = {
      'minecraft:sign': () => { for (let i = 1; i <= 4; i++) if (chance(0.7)) v[`Text${i}`] = textValue(); if (chance(0.3)) v.FilteredText2 = textValue(); if (chance(0.3)) v.Color = { t: 'str', v: 'red' }; if (chance(0.3)) v.GlowingText = { t: 'b', v: 1 }; if (chance(0.3)) { v.front_text = signText(); v.back_text = signText(); } if (chance(0.3)) v._filtered_correct = { t: 'b', v: int(2) }; },
      'minecraft:hanging_sign': () => extra['minecraft:sign'](),
      'minecraft:banner': () => { v.Patterns = { t: 'list', v: [{ t: 'compound', v: { Pattern: { t: 'str', v: pick(['bs', 'cre', 'zzz']) }, Color: { t: 'i', v: int(16) } } }] }; if (chance(0.5)) v.Base = { t: 'i', v: int(16) }; },
      'minecraft:skull': () => { v[pick(['SkullOwner', 'ExtraType', 'Owner'])] = pick([{ t: 'str', v: 'Notch' }, { t: 'compound', v: { Name: { t: 'str', v: 'X' }, Id: { t: 'str', v: uuidString() } } }]); if (chance(0.3)) v.note_block_sound = { t: 'str', v: 'minecraft:block.note_block.harp' }; },
      'minecraft:beehive': () => { v.Bees = { t: 'list', v: [{ t: 'compound', v: { EntityData: entity(depth + 1, 'minecraft:bee'), TicksInHive: { t: 'i', v: 5 }, MinOccupationTicks: { t: 'i', v: 10 } } }] }; v.FlowerPos = { t: 'compound', v: { X: { t: 'i', v: 1 }, Y: { t: 'i', v: 2 }, Z: { t: 'i', v: 3 } } }; },
      'minecraft:furnace': () => { v.RecipesUsedSize = { t: 's', v: 2 }; v.RecipeLocation0 = { t: 'str', v: 'iron_ingot' }; v.RecipeAmount0 = { t: 'i', v: 3 }; v.BurnTime = { t: 's', v: 20 }; v.CookTime = { t: 's', v: 5 }; v.CookTimeTotal = { t: 's', v: 200 }; },
      'minecraft:jukebox': () => { v.RecordItem = item(depth + 1); v.TickCount = { t: 'l', v: 500 }; v.RecordStartTick = { t: 'l', v: 100 }; v.IsPlaying = { t: 'b', v: 1 }; },
      'minecraft:lectern': () => { v.Book = item(depth + 1); v.Page = { t: 'i', v: 1 }; },
      'minecraft:decorated_pot': () => { v.shards = { t: 'list', v: [{ t: 'str', v: 'minecraft:brick' }] }; v.sherds = { t: 'list', v: [] }; if (chance(0.5)) v.item = item(depth + 1); },
      'minecraft:beacon': () => { v.Primary = { t: 'i', v: int(35) }; v.Secondary = { t: 'i', v: int(35) }; },
      'minecraft:conduit': () => { v.target_uuid = { t: 'compound', v: { M: { t: 'l', v: 5 }, L: { t: 'l', v: 9 } } }; },
      'minecraft:jigsaw': () => { v.attachement_type = { t: 'str', v: 'minecraft:x' }; v.target_pool = { t: 'str', v: 'minecraft:p' }; },
      'minecraft:end_gateway': () => { v.ExitPortal = { t: 'compound', v: { X: { t: 'i', v: 1 }, Y: { t: 'i', v: 2 }, Z: { t: 'i', v: 3 } } }; },
      'minecraft:trial_spawner': () => { v.normal_config = { t: 'compound', v: { simultaneous_mobs: { t: 'f', v: '3.0' } } }; v.spawn_range = { t: 'i', v: 4 }; v.ticks_between_spawn = { t: 'i', v: 20 }; },
      'minecraft:brushable_block': () => { v.item = pick([item(depth + 1), { t: 'compound', v: { id: { t: 'str', v: 'minecraft:air' }, count: { t: 'i', v: 1 } } }]); },
      'minecraft:mob_spawner': () => { v.SpawnData = entity(depth + 1, 'minecraft:pig'); v.SpawnPotentials = { t: 'list', v: [{ t: 'compound', v: { Entity: entity(depth + 1, 'minecraft:zombie'), Weight: { t: 'i', v: 3 } } }] }; },
    };
    if (id && extra[id]) extra[id]();
    if (!inItem || chance(0.5)) Object.assign(v, compound(depth + 1, 2).v);
    return { t: 'compound', v };
  };
  const uuidLongs = (v, k) => { v[`${k}Most`] = { t: 'l', v: int(1e9) + 1 }; v[`${k}Least`] = { t: 'l', v: int(1e9) + 1 }; };
  const entity = (depth, id) => {
    const v = {};
    if (id) v.id = { t: 'str', v: id };
    if (chance(0.5)) uuidLongs(v, 'UUID'); else v.UUID = { t: 'ia', v: [int(100), int(100), int(100), int(100)] };
    if (chance(0.7)) v.Pos = { t: 'list', v: [{ t: 'd', v: '0.5' }, { t: 'd', v: '1.0' }, { t: 'd', v: '0.5' }] };
    if (chance(0.3)) v.Rotation = { t: 'list', v: [{ t: 'f', v: '90.0' }, { t: 'f', v: '0.0' }] };
    if (chance(0.3)) v.CustomName = textValue();
    if (chance(0.3) && depth < 3) v.Item = item(depth + 1);
    if (chance(0.3) && depth < 3) v.HandItems = { t: 'list', v: [item(depth + 1), { t: 'compound', v: {} }] };
    if (chance(0.3) && depth < 3) v.ArmorItems = { t: 'list', v: [item(depth + 1), { t: 'compound', v: {} }, item(depth + 1), { t: 'compound', v: {} }] };
    if (chance(0.2)) v.ArmorDropChances = { t: 'list', v: [{ t: 'f', v: '0.085' }, { t: 'f', v: '2.0' }, { t: 'f', v: '0.5' }, { t: 'f', v: '0.085' }] };
    if (chance(0.2)) v.HandDropChances = { t: 'list', v: [{ t: 'f', v: '0.085' }, { t: 'f', v: '1.0' }] };
    if (chance(0.2)) v.Attributes = { t: 'list', v: [{ t: 'compound', v: { Name: { t: 'str', v: pick(['generic.maxHealth', 'minecraft:generic.max_health', 'generic.follow_range', 'minecraft:horse.jump_strength']) }, Base: { t: 'd', v: pick(['20.0', '16.0', '48.0']) }, Modifiers: { t: 'list', v: [{ t: 'compound', v: { Name: { t: 'str', v: 'Random spawn bonus' }, Amount: { t: 'd', v: '0.5' }, Operation: { t: 'i', v: 1 }, UUIDMost: { t: 'l', v: 7 }, UUIDLeast: { t: 'l', v: 8 } } }] } } }] };
    if (chance(0.2)) v.ActiveEffects = { t: 'list', v: [{ t: 'compound', v: { Id: { t: 'b', v: int(35) }, Duration: { t: 'i', v: 100 }, ShowIcon: { t: 'b', v: 1 } } }] };
    if (chance(0.15)) v.Leash = { t: 'compound', v: { X: { t: 'i', v: 1 }, Y: { t: 'i', v: 2 }, Z: { t: 'i', v: 3 } } };
    if (chance(0.15)) { v.TileX = { t: 'i', v: 1 }; v.TileY = { t: 'i', v: 2 }; v.TileZ = { t: 'i', v: 3 }; v.Facing = { t: 'b', v: int(6) }; v.Motive = { t: 'str', v: 'kebab' }; }
    if (chance(0.1)) { v.SleepingX = { t: 'i', v: 4 }; v.SleepingY = { t: 'i', v: 5 }; v.SleepingZ = { t: 'i', v: 6 }; }
    if (chance(0.15)) v.FallDistance = { t: 'f', v: '3.0' };
    if (chance(0.1)) v.locator_bar_icon = { t: 'compound', v: { color: { t: 'i', v: 5 } } };
    if (chance(0.15)) v.Type = { t: 'str', v: pick(['spruce', 'bamboo', 'oak', 'x']) };
    if (chance(0.15)) { v.Potion = { t: 'str', v: 'minecraft:strength' }; v.Color = { t: 'i', v: 5 }; }
    if (chance(0.1)) v.SaddleItem = item(depth + 1);
    if (chance(0.1)) v.Saddle = { t: 'b', v: 1 };
    if (chance(0.1)) v.ArmorItem = item(depth + 1);
    if (chance(0.1)) v.DecorItem = item(depth + 1);
    if (chance(0.1)) v.Items = { t: 'list', v: [item(depth + 1)] };
    if (chance(0.1)) { v.CatType = { t: 'i', v: int(12) }; v.Variant = { t: 'i', v: int(3) }; }
    if (chance(0.1)) v.weather_state = { t: 'i', v: int(5) };
    if (chance(0.1)) v.owner = { t: 'compound', v: { M: { t: 'l', v: 3 }, L: { t: 'l', v: 4 } } };
    if (chance(0.1)) v.Fuse = { t: 's', v: 80 };
    if (chance(0.1)) v.BlockState = { t: 'compound', v: { Name: { t: 'str', v: pick(['minecraft:chain', 'minecraft:grass', 'minecraft:stone']) } } };
    if (chance(0.15) && depth < 2) v.Passengers = { t: 'list', v: [entity(depth + 1, chance(0.8) ? 'minecraft:pig' : null)] };
    if (chance(0.5)) Object.assign(v, compound(depth + 1, 2).v);
    return { t: 'compound', v };
  };
  const blockState = () => {
    const v = { Name: { t: 'str', v: pick([pick(ids), pick(plainIds), 'minecraft:redstone_wire', 'minecraft:cauldron', 'minecraft:jigsaw', 'minecraft:cobblestone_wall', 'minecraft:chain', 'minecraft:creaking_heart', 'minecraft:test_block']) } };
    if (chance(0.8)) v.Properties = { t: 'compound', v: Object.fromEntries(Array.from({ length: int(4) }, () => [pick(['east', 'west', 'north', 'south', 'level', 'facing', 'active', 'test_block_mode', 'axis', 'powered']), { t: 'str', v: pick(['none', 'side', 'up', 'true', 'false', '0', '3', 'north', 'up', 'x']) }])) };
    return { t: 'compound', v };
  };
  return { r, int, pick, chance, item, entity, blockEntity, blockState };
}

const RELEASES = [1952, 1976, 2230, 2586, 2730, 2860, 2975, 3105, 3120, 3218, 3337, 3465, 3578, 3700, 3839, 3953, 3955, 4082, 4189, 4325, 4435, 4438, 4440, 4556];

function fuzzInputs(count, seed, vocabulary, min, max) {
  const g = generator(seed, vocabulary);
  const schemas = schemaIds();
  const keyed = schemas.filter((s) => s.key >= min * 10 && s.key <= max * 10);
  const versions = [...new Set([...RELEASES.filter((v) => v >= min && v <= max), ...keyed.map((s) => Math.floor(s.key / 10)), ...keyed.map((s) => Math.floor(s.key / 10) - 1)])].filter((v) => v >= min && v <= max);
  const schemaAt = (v) => schemas.filter((s) => s.key <= v * 10).pop();
  const lines = [];
  for (let i = 0; i < count; i++) {
    const version = g.pick(versions);
    const schema = schemaAt(version);
    lines.push(`block_entity ${version} ${snbt(g.blockEntity(0, g.chance(0.9) ? g.pick(schema.blockEntities) : g.pick(['minecraft:nonsense', 'chest', 'Chest'])))}`);
    lines.push(`item_stack ${version} ${snbt(g.item(0))}`);
    const e = g.entity(0, g.chance(0.9) ? g.pick(schema.entities) : 'minecraft:nonsense');
    lines.push(`${e.v.Passengers ? 'entity_tree' : 'entity'} ${version} ${snbt(e)}`);
    lines.push(`block_state ${version} ${snbt(g.blockState())}`);
  }
  return lines;
}

function compare(lines, java, rust) {
  // refused: the server's fixer throws too; refusedJavaRan: the server upgrades it.
  const stats = { lines: lines.length, same: 0, differ: 0, refused: 0, refusedJavaRan: 0, unparsed: 0 };
  const reasons = new Map();
  const shown = [];
  for (let i = 0; i < lines.length; i++) {
    const j = java[i] ?? '', r = rust[i] ?? '';
    if (r.startsWith('unported:')) {
      if (j.startsWith('error')) stats.refused++;
      else stats.refusedJavaRan++;
      const why = (j.startsWith('error') ? '' : '[java ran] ') + r.replace(/data of version \d+/, 'data of version N').slice(0, 160);
      reasons.set(why, (reasons.get(why) || 0) + 1);
    } else if (r.startsWith('unparsed')) stats.unparsed++;
    else if (j === r) stats.same++;
    else {
      stats.differ++;
      if (shown.length < 12) shown.push(`  IN   ${lines[i].slice(0, 700)}\n  JAVA ${j.slice(0, 700)}\n  RUST ${r.slice(0, 700)}`);
    }
  }
  return { stats, reasons, shown };
}

async function main() {
  // The dump is made as the app's first start makes it (server/minecraft-tables.js).
  if (flag('dump')) {
    const { writeDataFix, tablesFolder } = require('../server/minecraft-tables');
    const { checkJdk, JAVA, JAVAC } = require('../server/bluemap');
    await checkJdk();
    const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
    const target = tablesFolder(minecraftLive.minecraftVersion);
    await writeDataFix({ classpath, target, java: JAVA, javac: JAVAC });
    console.log(`wrote ${path.join(target, 'datafix.txt')}`);
    return;
  }
  const ref = await reference();
  const binary = await buildCore();
  let lines = [];
  if (flag('real')) {
    const types = new Map(ref.run(['--block-entity-types']).map((l) => l.split(' ')));
    lines.push(...realInputs(types));
  }
  if (option('lines')) lines.push(...fs.readFileSync(option('lines'), 'utf8').split('\n').filter(Boolean));
  if (option('fuzz') || (!flag('real') && !option('lines'))) {
    const vocabulary = ref.run(['--vocabulary']);
    lines.push(...fuzzInputs(Number(option('fuzz', 2000)), Number(option('seed', 1)), vocabulary, Number(option('min', 1948)), Number(option('max', 4556))));
  }
  const input = `${lines.join('\n')}\n`;
  const java = ref.run(['--upgrade'], input).map((l) => (l.startsWith('error ') ? `error ${l.slice(6)}` : l));
  const rust = spawnSync(binary, ['--datafix'], { input, maxBuffer: 1 << 30 }).stdout.toString('utf8').split('\n');
  const keep = option('keep');
  if (keep) {
    fs.mkdirSync(keep, { recursive: true });
    fs.writeFileSync(path.join(keep, 'lines.txt'), input);
    fs.writeFileSync(path.join(keep, 'java.txt'), `${java.join('\n')}\n`);
    fs.writeFileSync(path.join(keep, 'rust.txt'), rust.join('\n'));
  }
  const { stats, reasons, shown } = compare(lines, java, rust);
  console.log(JSON.stringify(stats));
  for (const [k, v] of [...reasons].sort((a, b) => b[1] - a[1])) console.log(`  refused ${v}\t${k}`);
  for (const s of shown) console.log(s);
  fs.rmSync(ref.classes, { recursive: true, force: true });
  // A line the port could not parse was not compared: it errored.
  console.log(`${stats.same} matched, ${stats.differ} differed, ${stats.refused} refused by both, ${stats.refusedJavaRan} refused by the port alone, ${stats.unparsed} errored`);
  // A run that compared nothing proves nothing.
  process.exit(stats.differ || stats.unparsed || !stats.same ? 1 : 0);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
