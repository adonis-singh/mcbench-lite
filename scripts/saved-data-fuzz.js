'use strict';

// Random saved data (SNBT) for the parity scripts: item stacks with data
// components, the compounds block entities and item entities are saved as,
// and SNBT values of every type and spelling, mostly valid and sometimes not.
// scripts/core-probe-parity.js asks both engines to read them one by one, and
// scripts/core-verify-parity.js imports them into verifier experiments.


function random(seed) {
  let a = seed >>> 0;
  const next = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return {
    next,
    int: (n) => Math.floor(next() * n),
    pick: (list) => list[Math.floor(next() * list.length)],
    chance: (p) => next() < p,
  };
}

const text = (s) => JSON.stringify(s);
const compound = (entries) => `{${entries.filter(Boolean).map(([k, v]) => `${/^[A-Za-z_.][A-Za-z0-9_.+-]*$/.test(k) ? k : text(k)}:${v}`).join(',')}}`;
const list = (items) => `[${items.join(',')}]`;

// r.hostile scales how often a generator writes data Minecraft rejects, and
// r.exotic how often it writes data that Minecraft takes but the Rust engine
// does not read (a styled text component, say): 1 by default, near 0 for
// experiments that should mostly run.
const bad = (r, p) => r.chance(p * (r.hostile ?? 1));
const exotic = (r, p) => r.chance(p * (r.exotic ?? 1));

const ITEMS = ['minecraft:stone', 'minecraft:diamond', 'minecraft:iron_ingot', 'minecraft:redstone', 'minecraft:cobblestone', 'minecraft:ender_pearl', 'minecraft:snowball', 'minecraft:oak_sign',
  'minecraft:diamond_sword', 'minecraft:iron_pickaxe', 'minecraft:shulker_box', 'minecraft:white_shulker_box', 'minecraft:bundle', 'minecraft:written_book', 'minecraft:writable_book',
  'minecraft:book', 'minecraft:enchanted_book', 'minecraft:potion', 'minecraft:music_disc_13', 'minecraft:chest', 'minecraft:hopper', 'minecraft:egg', 'minecraft:bucket', 'minecraft:crossbow',
  'minecraft:white_banner', 'minecraft:player_head'];
const BAD_ITEMS = ['minecraft:air', 'diamond', 'Minecraft:Stone', 'minecraft:nope', '5', ':stone', 'a:b:c'];
const ENCHANTMENTS = ['minecraft:sharpness', 'minecraft:unbreaking', 'minecraft:mending', 'minecraft:efficiency'];

function number(r, valid) {
  if (valid && !bad(r, 0.1)) return String(r.int(30));
  return r.pick(['0', '-1', '256', '1b', '2s', '3L', '2.7d', '1.5f', '4294967297L', '3000000000d', text('3'), '[3]', '[I;3]', '{}', '1e3', '0x10', '1_0', '99999999999L', '-0.5d', 'true']);
}

// A small non-negative integer written the ways Minecraft reads one.
function small(r) {
  return r.pick([String(r.int(30)), `${r.int(30)}b`, `${r.int(30)}s`, `${r.int(30)}L`, `${r.int(30)}.5d`, `${r.int(30)}.0f`]);
}


// A text component as an NBT value: a plain string, a literal or translatable or keybind or score or selector
// with styles, siblings and click and hover events, a list of them, mostly valid and sometimes not.
function richComponent(r, depth = 0) {
  const str = () => text(r.pick(['x', '', 'Token A', 'a\nb', 'it\'s', '"q"', 'é€', 'hello world', 'a,b}', '§', '\\', 'Steve', '12']));
  const sub = () => (depth < 3 ? richComponent(r, depth + 1) : str());
  if (!bad(r, 0.05) && r.chance(0.3)) return str();
  if (bad(r, 0.03)) return r.pick(['5', '[]', '{}', '1.5d', '[I;1]', '{extra:[]}', '{text:5}', '{type:"text"}']);
  const entries = [];
  const kind = r.pick(['text', 'text', 'text', 'translate', 'keybind', 'score', 'selector', 'text']);
  const matching = { text: 'text', translate: 'translatable', keybind: 'keybind', score: 'score', selector: 'selector' }[kind];
  const type = !bad(r, 0.05) ? r.pick([null, null, matching]) : r.pick(['Text', 'minecraft:text', 'nope', 'translatable', 'keybind', null]);
  if (type) entries.push(['type', text(type)]);
  if (kind === 'text') entries.push(['text', bad(r, 0.03) ? r.pick(['5', '1b', '[]']) : str()]);
  else if (kind === 'translate') {
    entries.push(['translate', bad(r, 0.03) ? '5' : text(r.pick(['block.minecraft.stone', 'chat.type.text', 'x', '']))]);
    if (r.chance(0.3)) entries.push(['fallback', bad(r, 0.1) ? '5' : str()]);
    if (r.chance(0.5)) {
      const arg = () => r.pick([str(), '1b', '2s', '3', '4L', '1.5f', '2.5d', 'true', '-0.0d', sub(), sub(), bad(r, 0.1) ? r.pick(['[I;1]', '[1]', '0.0f / 0.0f', '1e400d']) : str()]);
      entries.push(['with', bad(r, 0.05) ? r.pick(['5', text('x')]) : list(Array.from({ length: r.int(4) }, arg))]);
    }
  } else if (kind === 'keybind') entries.push(['keybind', bad(r, 0.03) ? '5' : text(r.pick(['key.jump', '', 'x']))]);
  else if (kind === 'score') entries.push(['score', bad(r, 0.1) ? r.pick(['5', compound([['name', text('a')]])]) : compound([['name', text(r.pick(['a', '@s', '']))], ['objective', text(r.pick(['b', '']))], r.chance(0.2) && ['value', text('zz')]])]);
  else entries.push(['selector', bad(r, 0.03) ? '5' : text(r.pick(['@a', '@e', '@p', '@r', '@s', '@n', 'Steve', 'a-b.c', '', '@x', '@a[]', 'é', 'abcdefghijklmnopq', '@a[limit=1]', '.', '~'])), r.chance(0.3) && ['separator', sub()]]);
  if (r.chance(0.7)) {
    const style = [];
    if (r.chance(0.4)) style.push(['color', text(bad(r, 0.1) ? r.pick(['nope', 'RED', '#GGGGGG', '#1234567', 'reset', '']) : r.pick(['red', 'gold', 'black', 'white', 'light_purple', '#ff0000', '#12345', '#ABCDEF', '#000001', '#00FF00', '#FFFFFF', '#FFFFFE']))]);
    if (r.chance(0.2)) style.push(['shadow_color', r.pick(['5', '-1', '1.5d', '1b', '99999999999L', '[1.0f,0.5f,0.0f,1.0f]', '[0.2d,0.4d,0.6d,0.8d]', '[I;1,2,3,4]', '[1,2,3]', text('red')])]);
    for (const flag of ['bold', 'italic', 'underlined', 'strikethrough', 'obfuscated']) if (r.chance(0.15)) style.push([flag, bad(r, 0.05) ? r.pick([text('true'), '[1]']) : r.pick(['1b', '0b', 'true', 'false', '2', '0.5d', '256'])]);
    if (r.chance(0.1)) style.push(['insertion', bad(r, 0.1) ? '5' : str()]);
    if (r.chance(0.1)) style.push(['font', bad(r, 0.1) ? r.pick(['5', text('Bad')]) : text(r.pick(['minecraft:uniform', 'uniform', 'minecraft:default', 'a:b/c']))]);
    if (r.chance(0.15)) {
      const action = r.pick(['run_command', 'suggest_command', 'change_page', 'copy_to_clipboard', 'run_command', 'open_url', 'open_file', 'custom', 'nope']);
      const fields = { run_command: ['command', text(r.pick(['/say hi', 'say hi', '', 'a§b', 'a\nb']))], suggest_command: ['command', str()], change_page: ['page', r.pick(['5', '0', '-1', '2.5d', text('5')])], copy_to_clipboard: ['value', str()], open_url: ['url', text('https://example.com/a')], open_file: ['path', str()], custom: ['id', text('a:b')], nope: ['x', '1'] };
      style.push(['click_event', compound([['action', text(action)], r.chance(0.95) && fields[action]])]);
    }
    if (r.chance(0.15)) {
      const action = r.pick(['show_text', 'show_text', 'show_item', 'show_entity', 'nope']);
      const fields = {
        show_text: [['value', sub()]],
        show_item: [['id', text(r.pick(['minecraft:stone', 'minecraft:diamond_sword', 'stone', 'minecraft:air', 'minecraft:nope']))], r.chance(0.7) && ['count', r.pick(['1', '5', '0', '500', text('3'), '1.9d'])],
          r.chance(0.4) && ['components', compound([['minecraft:custom_name', sub()], r.chance(0.3) && ['minecraft:max_stack_size', '64'], r.chance(0.2) && ['!minecraft:rarity', '{}']])]],
        show_entity: [['id', text(r.pick(['minecraft:pig', 'pig', 'minecraft:player', 'minecraft:nope']))], ['uuid', r.pick(['[I;1,2,3,4]', '[1,2,3,4]', text('00000000-0000-0000-0000-000000000001'), text('1-2-3-4-5'), '[I;1,2]', text('zz'), '[B;1,2,3,4]'])], r.chance(0.4) && ['name', sub()]],
        nope: [['x', '1']],
      };
      style.push(['hover_event', compound([['action', text(action)], ...fields[action]])]);
    }
    entries.push(...style);
  }
  if (r.chance(0.25)) entries.push(['extra', bad(r, 0.05) ? r.pick(['[]', text('y'), '5', '[[]]']) : list(Array.from({ length: 1 + r.int(3) }, sub))]);
  const value = compound(entries);
  return r.chance(0.1) ? list([value, ...Array.from({ length: r.int(3) }, sub)]) : value;
}

// An item predicate (a container's lock): mostly valid, sometimes not.
function lockPredicate(r) {
  const str = () => text(r.pick(['stone', 'minecraft:dirt', 'minecraft:air', '#logs', '#minecraft:planks', 'minecraft:nope', '#minecraft:nope', 'Bad', '#', '#a:b']));
  return compound([r.chance(0.5) && ['items', bad(r, 0.1) ? '5' : r.chance(0.6) ? str() : list(Array.from({ length: r.int(4) }, str))],
    r.chance(0.4) && ['count', r.pick(['3', '3.7d', '-1', '0', '{}', '{min:2}', '{max:2}', '{min:2,max:2}', '{min:5,max:2}', '{min:2.5d,max:9}', text('3'), '[1]', '3000000000L'])],
    r.chance(0.3) && ['components', bad(r, 0.1) ? '5' : compound([r.chance(0.7) && ['minecraft:custom_name', richComponent(r, 2)], r.chance(0.4) && ['minecraft:rarity', text('rare')], r.chance(0.2) && ['!minecraft:rarity', '{}'], r.chance(0.2) && ['minecraft:nope', '1']])],
    r.chance(0.2) && ['predicates', r.pick(['{}', '{"minecraft:damage":{}}', '5'])]]);
}

function component(r, depth) {
  const key = r.pick(['custom_data', 'bucket_entity_data', 'max_stack_size', 'max_damage', 'damage', 'repair_cost', 'unbreakable', 'enchantment_glint_override', 'rarity', 'item_name', 'custom_name', 'lore',
    'enchantments', 'stored_enchantments', 'container', 'bundle_contents', 'charged_projectiles', 'written_book_content', 'writable_book_content', 'item_model',
    'banner_patterns', 'potion_contents', 'banner_patterns', 'potion_contents', 'dyed_color', 'base_color', 'pot_decorations', 'fireworks', 'firework_explosion', 'suspicious_stew_effects', 'lock']);
  const name = !bad(r, 0.1) ? `minecraft:${key}` : r.pick([key, `!minecraft:${key}`, `minecraft:${key}x`]);
  const str = () => r.pick([text('x'), text(''), text('Token A'), text('a\nb'), text('it\'s'), text('"q"'), text('é€'), text('\\\\'), text('a,b}')]);
  const comp = () => !exotic(r, 0.3) ? (r.chance(0.8) ? str() : compound([['text', str()]])) : r.chance(0.7) ? richComponent(r) : r.pick([compound([['text', str()], ['bold', '1b']]), list([str(), str()]), compound([['translate', text('block.minecraft.stone')]]),
    compound([['text', str()], ['extra', list([str()])]]), compound([['type', text('text')], ['text', str()]])]);
  let value;
  switch (key) {
    case 'custom_data': case 'bucket_entity_data': value = bad(r, 0.1) ? r.pick([text('{a:'), '5']) : r.pick([compound([['a', '1b'], ['b', list(['1', '2'])]]), text('{a:1b}'), '{}', text('{x:{y:[1.5f,2d]}}')]); break;
    case 'max_stack_size': value = bad(r, 0.1) ? number(r, false) : String(1 + r.int(99)); break;
    case 'max_damage': case 'damage': case 'repair_cost': value = bad(r, 0.1) ? number(r, false) : small(r); break;
    case 'unbreakable': value = bad(r, 0.2) ? r.pick(['5', text('x')]) : r.pick(['{}', '{a:1}']); break;
    case 'enchantment_glint_override': value = bad(r, 0.1) ? text('true') : r.pick(['1b', '0b', 'true', '256', '0.5d', '2.5d']); break;
    case 'rarity': value = text(bad(r, 0.1) ? r.pick(['EPIC', 'x']) : r.pick(['common', 'epic', 'rare', 'uncommon'])); break;
    case 'item_model': value = text(bad(r, 0.1) ? r.pick(['Bad:Name', 'a b']) : r.pick(['minecraft:stone', 'stone', 'a:b/c'])); break;
    case 'item_name': case 'custom_name': value = bad(r, 0.1) ? '5' : comp(); break;
    case 'lore': value = bad(r, 0.1) ? r.pick(['5', text('x')]) : list(Array.from({ length: r.int(4) }, comp)); break;
    case 'enchantments': case 'stored_enchantments': {
      const names = r.chance(0.5) ? ENCHANTMENTS : ENCHANTMENTS.slice(0, 2);
      value = compound(names.slice(0, r.int(names.length + 1)).map((name) => [bad(r, 0.1) ? 'minecraft:nope' : name, bad(r, 0.1) ? number(r, false) : String(1 + r.int(5))]));
      break;
    }
    case 'container': value = list(Array.from({ length: r.int(4) }, (_, i) => compound([!bad(r, 0.05) && ['slot', bad(r, 0.1) ? number(r, false) : String(i * 3 + r.int(3))], ['item', stack(r, depth + 1)]]))); break;
    case 'bundle_contents': case 'charged_projectiles': value = list(Array.from({ length: r.int(4) }, () => stack(r, depth + 1))); break;
    case 'written_book_content': value = compound([['title', r.chance(0.5) ? text('T') : compound([['raw', text('T')], r.chance(0.3) && ['filtered', text('F')]])], !bad(r, 0.1) && ['author', text('A')],
      r.chance(0.5) && ['generation', bad(r, 0.1) ? number(r, false) : String(r.int(4))], r.chance(0.8) && ['pages', list(Array.from({ length: r.int(4) }, () => r.chance(0.6) ? comp() : compound([['raw', comp()]])))], r.chance(0.5) && ['resolved', r.pick(['1b', 'true', '0b'])]]); break;
    case 'writable_book_content': value = compound([r.chance(0.9) && ['pages', list(Array.from({ length: r.int(4) }, () => r.chance(0.6) ? str() : compound([['raw', str()], ['filtered', str()]])))]]); break;
    case 'banner_patterns': {
      const layer = () => compound([!bad(r, 0.05) && ['pattern', bad(r, 0.1) ? r.pick(['5', text('minecraft:nope'), text('Bad'), text(''), compound([['asset_id', text('a')]])]) : r.chance(0.2) ? compound([['asset_id', text(r.pick(['a', 'minecraft:b']))], ['translation_key', text('k')]]) : text(r.pick(['minecraft:border', 'stripe_top', 'minecraft:creeper', 'minecraft:globe']))],
        !bad(r, 0.05) && ['color', text(bad(r, 0.1) ? r.pick(['WHITE', 'x']) : r.pick(['white', 'red', 'black', 'light_blue']))], r.chance(0.05) && ['x', '1b']]);
      value = bad(r, 0.05) ? r.pick(['5', layer()]) : list(Array.from({ length: r.int(8) }, layer));
      break;
    }
    case 'potion_contents': {
      const effect = () => compound([!bad(r, 0.05) && ['id', text(bad(r, 0.1) ? r.pick(['minecraft:nope', 'x']) : r.pick(['minecraft:speed', 'slowness', 'minecraft:instant_health', 'minecraft:poison']))],
        r.chance(0.4) && ['amplifier', bad(r, 0.05) ? text('x') : r.pick(['0', '1', '2b', '255', '300', '-1', '3.9d', '129L'])], r.chance(0.4) && ['duration', bad(r, 0.05) ? text('x') : r.pick(['0', '100', '-5', '2.5d', '99999999999L'])],
        r.chance(0.2) && ['ambient', r.pick(['1b', '0b', 'true'])], r.chance(0.2) && ['show_particles', r.pick(['1b', '0b', 'false'])], r.chance(0.2) && ['show_icon', r.pick(['1b', '0b'])],
        r.chance(0.15) && ['hidden_effect', bad(r, 0.1) ? '5' : compound([r.chance(0.5) && ['duration', '5'], r.chance(0.3) && ['hidden_effect', '{}']])]]);
      value = bad(r, 0.05) ? r.pick([text('minecraft:nope'), '5', text('minecraft:water')]) : r.chance(0.2) ? text(r.pick(['minecraft:water', 'healing', 'minecraft:strong_swiftness'])) : compound([
        r.chance(0.5) && ['potion', text(bad(r, 0.05) ? r.pick(['minecraft:nope', '5']) : r.pick(['minecraft:water', 'minecraft:long_poison', 'awkward']))],
        r.chance(0.4) && ['custom_color', bad(r, 0.1) ? text('red') : r.pick(['0', '16711680', '-1', '1.5d', '99999999999L'])],
        r.chance(0.5) && ['custom_effects', bad(r, 0.05) ? r.pick(['5', '{}']) : list(Array.from({ length: r.int(3) }, effect))],
        r.chance(0.3) && ['custom_name', bad(r, 0.1) ? '5' : str()]]);
      break;
    }
    case 'lock': value = lockPredicate(r); break;
    case 'dyed_color': value = bad(r, 0.1) ? r.pick([text('x'), '[1.0f,1.0f,1.0f]', '[1,2,3]']) : r.pick(['0', '16711680', '-1', '1.5d', '99999999999L']); break;
    case 'base_color': value = text(bad(r, 0.1) ? r.pick(['WHITE', 'x', '5']) : r.pick(['white', 'red', 'black', 'light_blue'])); break;
    case 'pot_decorations': value = bad(r, 0.05) ? r.pick(['5', text('x')]) : list(Array.from({ length: r.int(6) }, () => text(bad(r, 0.1) ? r.pick(['minecraft:nope', 'Bad']) : r.pick(['minecraft:brick', 'minecraft:arms_up_pottery_sherd', 'angler_pottery_sherd', 'minecraft:stone'])))); break;
    case 'fireworks': value = bad(r, 0.05) ? r.pick(['5', '{flight_duration:"x"}']) : compound([r.chance(0.7) && ['flight_duration', r.pick(['1b', '3', '255b', '-1b', '2.5d', '300'])],
      r.chance(0.6) && ['explosions', list(Array.from({ length: r.int(3) }, () => compound([['shape', text(bad(r, 0.1) ? 'x' : r.pick(['small_ball', 'star', 'creeper']))], r.chance(0.5) && ['colors', list([r.pick(['1', '16711680', '2.5d'])])], r.chance(0.3) && ['has_trail', r.pick(['1b', '0b'])]])))]]); break;
    case 'firework_explosion': value = bad(r, 0.05) ? r.pick(['5', '{}']) : compound([['shape', text(bad(r, 0.1) ? 'x' : r.pick(['small_ball', 'large_ball', 'burst']))], r.chance(0.5) && ['fade_colors', list([r.pick(['1', '255'])])], r.chance(0.5) && ['has_twinkle', r.pick(['1b', '0b'])]]); break;
    case 'suspicious_stew_effects': value = bad(r, 0.05) ? r.pick(['5', '{}']) : list(Array.from({ length: r.int(3) }, () => compound([!bad(r, 0.05) && ['id', text(bad(r, 0.1) ? 'minecraft:nope' : r.pick(['minecraft:speed', 'poison']))], r.chance(0.5) && ['duration', bad(r, 0.1) ? text('x') : r.pick(['160', '5', '-1', '2.5d'])]]))); break;
    default: value = '{}';
  }
  return [name, value];
}

function stack(r, depth = 0) {
  const entries = [];
  entries.push(['id', !bad(r, 0.05) ? text(r.pick(ITEMS)) : (r.chance(0.8) ? text(r.pick(BAD_ITEMS)) : r.pick(['5', '[]']))]);
  if (r.chance(0.9)) entries.push(['count', number(r, true)]);
  if (r.chance(0.6) && depth < 3) {
    // A component named twice is a duplicate Minecraft rejects.
    const seen = new Set();
    const components = Array.from({ length: 1 + r.int(3) }, () => component(r, depth)).filter(([name]) => {
      const key = name.replace(/^!?(minecraft:)?/, '').replace(/x$/, '');
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
    entries.push(['components', compound(components)]);
  }
  if (r.chance(0.05)) entries.push(['extra', '1b']);
  return compound(entries);
}

const BLOCK_ENTITIES = [
  'minecraft:chest', 'minecraft:trapped_chest', 'minecraft:barrel', 'minecraft:shulker_box', 'minecraft:red_shulker_box', 'minecraft:hopper', 'minecraft:dispenser', 'minecraft:dropper',
  'minecraft:crafter', 'minecraft:ender_chest', 'minecraft:comparator', 'minecraft:oak_sign', 'minecraft:oak_wall_sign', 'minecraft:oak_hanging_sign', 'minecraft:oak_wall_hanging_sign',
  'minecraft:white_banner', 'minecraft:red_wall_banner', 'minecraft:lectern', 'minecraft:furnace', 'minecraft:blast_furnace', 'minecraft:smoker', 'minecraft:brewing_stand', 'minecraft:jukebox',
  'minecraft:chiseled_bookshelf', 'minecraft:bell', 'minecraft:daylight_detector', 'minecraft:stone', 'minecraft:beacon',
  'minecraft:command_block', 'minecraft:repeating_command_block', 'minecraft:chain_command_block', 'minecraft:enchanting_table',
  'minecraft:jigsaw', 'minecraft:end_gateway', 'minecraft:end_portal', 'minecraft:test_block', 'minecraft:structure_block', 'minecraft:decorated_pot', 'minecraft:suspicious_sand', 'minecraft:suspicious_gravel',
  'minecraft:white_bed', 'minecraft:skeleton_skull', 'minecraft:zombie_wall_head', 'minecraft:player_head', 'minecraft:oak_shelf', 'minecraft:copper_golem_statue', 'minecraft:waxed_copper_golem_statue',
  'minecraft:sculk_sensor', 'minecraft:calibrated_sculk_sensor', 'minecraft:sculk_shrieker', 'minecraft:sculk_catalyst',
];

function itemList(r, size) {
  const slot = () => !bad(r, 0.15) ? `${r.int(size + (r.chance(0.1) ? 3 : 0))}b` : r.pick([number(r, false), text('x'), '-1b', '255b', '256', '27', '2.5d']);
  return list(Array.from({ length: r.int(5) }, () => {
    const entries = stack(r, 1).slice(1, -1);
    return `{${[!bad(r, 0.05) && `Slot:${slot()}`, entries].filter(Boolean).join(',')}}`;
  }));
}

function customName(r) {
  return bad(r, 0.1) ? r.pick(['5', list([text('a')])]) : exotic(r, 0.3) ? richComponent(r) : r.pick([text('Name'), text(''), compound([['text', text('x')]]), text('é€')]);
}

// What the workstations hold when they are used: smeltables and fuel in the
// furnaces, potions and ingredients in the brewing stands, records in the
// jukeboxes, recipe ingredients in the crafters, things to fire in the
// dispensers. Read from the engine's dump.
let dumped = null;
function dump() {
  if (dumped) return dumped;
  const fs = require('fs');
  const path = require('path');
  dumped = { cooking: { 'minecraft:smelting': [], 'minecraft:blasting': [], 'minecraft:smoking': [] }, fuel: [], items: [], recipes: [], potionMixes: [], containerMixes: [], compost: [], songs: [] };
  require('../server/minecraft-tables').checkTables('1.21.10');
  for (const line of fs.readFileSync(path.join(__dirname, '..', 'core', 'minecraft', '1.21.10', 'blocks.txt'), 'utf8').split('\n')) {
    const p = line.split('\t');
    if (p[0] === 'cooking' && dumped.cooking[p[1]]) dumped.cooking[p[1]].push(...p[3].split(','));
    else if (p[0] === 'fuel') dumped.fuel.push(p[1]);
    else if (p[0] === 'item') dumped.items.push(p[1]);
    else if (p[0] === 'recipe' && p[1] !== 'other') dumped.recipes.push(p);
    else if (p[0] === 'potionmix') dumped.potionMixes.push({ from: p[1], ingredient: p[2].split(','), to: p[3] });
    else if (p[0] === 'containermix') dumped.containerMixes.push({ from: p[1], ingredient: p[2].split(','), to: p[3] });
    else if (p[0] === 'compost') dumped.compost.push(p[1]);
    else if (p[0] === 'jukebox') dumped.songs.push(p[1]);
  }
  return dumped;
}

const potionNbt = (item, potion) => compound([['id', text(item)], ['count', '1'], ['components', compound([['minecraft:potion_contents', compound([['potion', text(potion)]])]])]]);

// The Items list (and the other fields) of a workstation holding what it is used with.
function realisticEntries(r, short) {
  const d = dump();
  const slotted = (slot, stackText) => `{Slot:${slot}b,${stackText.slice(1, -1)}}`;
  const plain = (id, count) => compound([['id', text(id)], ['count', String(count)]]);
  const entries = [];
  if (/furnace$|smoker/.test(short)) {
    const type = short === 'blast_furnace' ? 'minecraft:blasting' : short === 'smoker' ? 'minecraft:smoking' : 'minecraft:smelting';
    const items = [];
    if (r.chance(0.8)) items.push(slotted(0, plain(r.pick(d.cooking[type]), 1 + r.int(r.chance(0.2) ? 64 : 5))));
    if (r.chance(0.7)) items.push(slotted(1, plain(r.pick(r.chance(0.8) ? d.fuel : ['minecraft:bucket', 'minecraft:water_bucket', 'minecraft:lava_bucket']), 1 + r.int(4))));
    if (r.chance(0.3)) items.push(slotted(2, plain(r.pick(['minecraft:iron_ingot', 'minecraft:charcoal', 'minecraft:glass', 'minecraft:cooked_beef']), 1 + r.int(60))));
    entries.push(['Items', list(items)]);
    if (r.chance(0.3)) entries.push(['lit_time_remaining', `${1 + r.int(200)}s`], ['lit_total_time', `${200 + r.int(1600)}s`]);
    if (r.chance(0.3)) entries.push(['cooking_time_spent', `${r.int(100)}s`], ['cooking_total_time', r.pick(['200s', '100s', '10s'])]);
    if (r.chance(0.2)) entries.push(['RecipesUsed', compound([[r.pick(['minecraft:iron_ingot', 'minecraft:baked_potato', 'minecraft:cooked_beef_from_smoking']), String(1 + r.int(4))]])]);
    return entries;
  }
  if (short === 'brewing_stand') {
    const items = [];
    for (let slot = 0; slot < 3; slot++) {
      if (!r.chance(0.7)) continue;
      const kind = r.pick(['minecraft:potion', 'minecraft:potion', 'minecraft:splash_potion', 'minecraft:lingering_potion']);
      const from = r.pick([...new Set(d.potionMixes.map((mix) => mix.from)), 'minecraft:water', 'minecraft:awkward']);
      items.push(slotted(slot, r.chance(0.15) ? plain('minecraft:glass_bottle', 1) : potionNbt(kind, from)));
    }
    if (r.chance(0.8)) items.push(slotted(3, plain(r.pick(r.chance(0.8) ? [...d.potionMixes.flatMap((mix) => mix.ingredient), ...d.containerMixes.flatMap((mix) => mix.ingredient)] : ['minecraft:dirt', 'minecraft:stick']), 1 + r.int(3))));
    if (r.chance(0.6)) items.push(slotted(4, plain(r.chance(0.9) ? 'minecraft:blaze_powder' : 'minecraft:dirt', 1 + r.int(3))));
    entries.push(['Items', list(items)]);
    if (r.chance(0.5)) entries.push(['BrewTime', r.pick(['400s', '399s', '2s', '1s', '200s'])]);
    if (r.chance(0.6)) entries.push(['Fuel', r.pick(['20b', '5b', '1b', '0b'])]);
    return entries;
  }
  if (short === 'jukebox') {
    if (r.chance(0.8)) entries.push(['RecordItem', plain(r.pick(d.items.filter((id) => /music_disc|disc_fragment/.test(id))), 1)]);
    if (r.chance(0.5)) entries.push(['ticks_since_song_started', r.pick(['0L', '19L', '20L', '1000L', '3000L', '5000L', `${r.int(4000)}L`])]);
    return entries;
  }
  if (short === 'crafter') {
    // A recipe's ingredients in the grid.
    const p = r.pick(d.recipes);
    const cells = p[1] === 'shaped' ? p[5].split(';') : p[3].split(';');
    const width = p[1] === 'shaped' ? Number(p[3]) : 3;
    const items = [];
    cells.forEach((cell, i) => {
      if (cell === '-') return;
      const slot = p[1] === 'shaped' ? (i % width) + Math.floor(i / width) * 3 : i;
      items.push(slotted(slot, plain(r.pick(cell.split(',')), 1 + r.int(3))));
    });
    entries.push(['Items', list(items)]);
    return entries;
  }
  if (short === 'chiseled_bookshelf') {
    const items = [];
    for (let slot = 0; slot < 6; slot++) if (r.chance(0.5)) items.push(slotted(slot, plain(r.pick(['minecraft:book', 'minecraft:enchanted_book', 'minecraft:writable_book', 'minecraft:written_book', 'minecraft:dirt']), 1)));
    entries.push(['Items', list(items)]);
    return entries;
  }
  if (/dispenser$|dropper$|hopper$/.test(short)) {
    const pool = ['minecraft:water_bucket', 'minecraft:lava_bucket', 'minecraft:bucket', 'minecraft:flint_and_steel', 'minecraft:bone_meal', 'minecraft:tnt', 'minecraft:glowstone', 'minecraft:honeycomb', 'minecraft:glass_bottle',
      'minecraft:shulker_box', 'minecraft:blue_shulker_box', 'minecraft:minecart', 'minecraft:oak_boat', 'minecraft:shears', 'minecraft:brush', 'minecraft:carved_pumpkin', 'minecraft:iron_helmet', 'minecraft:arrow', 'minecraft:snowball', 'minecraft:egg',
      'minecraft:wheat', 'minecraft:apple', 'minecraft:oak_leaves', 'minecraft:coal', 'minecraft:iron_ore', 'minecraft:sand', 'minecraft:redstone', 'minecraft:nether_wart', 'minecraft:blaze_powder', 'minecraft:book', 'minecraft:stick', ...d.compost.slice(0, 20)];
    const items = [];
    const n = /hopper/.test(short) ? 5 : 9;
    for (let slot = 0; slot < n; slot++) if (r.chance(0.5)) items.push(slotted(slot, plain(r.pick(pool), 1 + r.int(r.chance(0.2) ? 30 : 3))));
    entries.push(['Items', list(items)]);
    return entries;
  }
  return null;
}

function blockEntity(r, block, typeId = block) {
  const entries = [];
  const items = (size) => r.chance(0.8) && entries.push(['Items', !bad(r, 0.03) ? itemList(r, size) : r.pick(['5', '[5]', '[]'])]);
  const maybe = (key, value, p = 0.3) => r.chance(p) && entries.push([key, value]);
  const base = () => {
    maybe('CustomName', customName(r), 0.15);
    if (bad(r, 0.03)) maybe('lock', text('key'), 1);
    if (exotic(r, 0.06)) maybe('lock', lockPredicate(r), 1);
    if (exotic(r, 0.03)) maybe('LootTable', text('minecraft:chests/simple_dungeon'), 1);
    if (bad(r, 0.04)) maybe('LootTableSeed', number(r, false), 1);
    else maybe('LootTableSeed', small(r), 0.04);
    maybe('components', !bad(r, 0.3) ? (r.chance(0.5) ? '{}' : compound(Array.from({ length: 1 + r.int(2) }, () => component(r, 2)).filter(([name], i, all) => all.findIndex(([other]) => other.replace(/^!?(minecraft:)?/, '').replace(/x$/, '') === name.replace(/^!?(minecraft:)?/, '').replace(/x$/, '')) === i))) : r.pick([compound([['minecraft:custom_name', text('x')]]), '5']), 0.1);
    if (exotic(r, 0.02)) maybe('PublicBukkitValues', r.pick(['{a:1}', '{}', '{"a:b":1b,x:{y:[1,2]}}', '5']), 1);
    if (bad(r, 0.05)) maybe('id', text(r.pick(['minecraft:chest', 'barrel'])), 1);
    else maybe('id', text(typeId), 0.3);
  };
  base();
  const short = block.replace('minecraft:', '');
  // Most of the workstations hold what they are used with (r.realistic is the chance).
  const real = r.chance(r.realistic ?? 0.6) ? realisticEntries(r, short) : null;
  if (real) {
    entries.push(...real);
    if (short === 'hopper') maybe('TransferCooldown', small(r), 0.3);
    if (short === 'crafter') { maybe('triggered', small(r), 0.3); maybe('crafting_ticks_remaining', small(r), 0.3); }
    if (!entries.length) entries.push(['id', text(typeId)]);
    return compound(entries);
  }
  if (/chest$|barrel|dispenser|dropper/.test(short) && !/ender/.test(short) || /shulker_box/.test(short)) items(27);
  else if (short === 'hopper') { items(5); maybe('TransferCooldown', bad(r, 0.1) ? number(r, false) : small(r), 0.6); }
  else if (short === 'crafter') {
    items(9);
    maybe('crafting_ticks_remaining', bad(r, 0.1) ? number(r, false) : small(r), 0.5);
    maybe('disabled_slots', bad(r, 0.1) ? r.pick(['[1,2]', '[B;1]']) : r.pick(['[I;1,2]', '[I;0,8,9,-1]', '[I;]', '[I;3,3]', '[I;0,4]']), 0.6);
    maybe('triggered', bad(r, 0.1) ? number(r, false) : small(r), 0.5);
  } else if (short === 'comparator') maybe('OutputSignal', bad(r, 0.1) ? number(r, false) : small(r), 0.8);
  else if (/sign$/.test(short)) {
    const line = () => bad(r, 0.05) ? '5' : exotic(r, 0.2) ? richComponent(r) : r.pick([text(''), text('a'), text('Hi there'), compound([['text', text('t')]])]);
    const side = () => compound([!bad(r, 0.05) && ['messages', list(Array.from({ length: !bad(r, 0.1) ? 4 : r.int(6) }, line))],
      r.chance(0.2) && ['filtered_messages', list(Array.from({ length: 4 }, () => text(r.pick(['', 'f', 'g']))))], r.chance(0.5) && ['color', text(r.pick(['red', 'black', 'light_blue', 'white']))],
      r.chance(0.4) && ['has_glowing_text', r.pick(['1b', '0b', 'true'])]]);
    maybe('front_text', side(), 0.8);
    maybe('back_text', side(), 0.5);
    maybe('is_waxed', r.pick(['1b', '0b', '5']), 0.4);
  } else if (/banner$/.test(short)) { maybe('patterns', exotic(r, 0.3) ? list([compound([['color', text('red')], ['pattern', text('minecraft:stripe_top')]])]) : '[]', 0.4); }
  else if (short === 'lectern') {
    const book = () => stack(r, 1).replace(/^\{id:"[^"]*"/, () => `{id:${text(r.pick(['minecraft:written_book', 'minecraft:writable_book', 'minecraft:book', 'minecraft:written_book']))}`);
    maybe('Book', bad(r, 0.05) ? '5' : book(), 0.9);
    maybe('Page', bad(r, 0.1) ? number(r, false) : small(r), 0.7);
  } else if (/furnace$|smoker/.test(short)) {
    items(3);
    for (const key of ['cooking_time_spent', 'cooking_total_time', 'lit_time_remaining', 'lit_total_time']) maybe(key, bad(r, 0.1) ? text('x') : r.pick(['5s', '100', '40000', '-3s', '2.5d', '60s']), 0.4);
    maybe('RecipesUsed', bad(r, 0.1) ? r.pick(['{"minecraft:iron_ingot":2,iron_ingot:3}', '5']) : r.pick(['{}', '{"minecraft:iron_ingot":2}']), 0.2);
    maybe('Paper.CookSpeedMultiplier', bad(r, 0.1) ? text('x') : r.pick(['1.0d', '2.5d', '3']), 0.2);
  } else if (short === 'brewing_stand') { items(5); maybe('BrewTime', r.pick(['100s', '0s', '70000', '-1']), 0.5); maybe('Fuel', r.pick(['5b', '20', '300', '-1b']), 0.5); }
  else if (short === 'jukebox') { if (exotic(r, 0.2)) maybe('RecordItem', stack(r, 1), 1); maybe('ticks_since_song_started', small(r), 0.2); }
  else if (short === 'enchanting_table') { /* CustomName only */ }
  else if (short === 'jigsaw') {
    const loc = () => bad(r, 0.1) ? r.pick(['5', text('Bad Name'), text('a:b:c')]) : text(r.pick(['a', 'minecraft:empty', 'b:c/d', 'x']));
    for (const key of ['name', 'target', 'pool']) maybe(key, loc(), 0.5);
    maybe('final_state', bad(r, 0.05) ? '5' : text(r.pick(['minecraft:stone', 'stone[x=1]', '', 'minecraft:air'])), 0.4);
    maybe('joint', bad(r, 0.1) ? r.pick(['5', text('nope'), text('ROLLABLE')]) : text(r.pick(['rollable', 'aligned'])), 0.5);
    for (const key of ['placement_priority', 'selection_priority']) maybe(key, bad(r, 0.05) ? text('x') : r.pick(['3', '-2', '2.5d', '7L']), 0.4);
  } else if (short === 'end_gateway') {
    maybe('Age', bad(r, 0.05) ? text('x') : r.pick(['5L', '0', '2399L', '199L', '7.9d']), 0.6);
    maybe('exit_portal', bad(r, 0.1) ? r.pick(['[I;1,2]', '5', compound([['X', '1']])]) : r.pick(['[I;1,2,3]', '[1,2,3]', '[I;1,30000000,3]', '[I;40000000,2,3]', '[I;1,20000000,3]', '[B;1,2,3]']), 0.5);
    maybe('ExactTeleport', r.pick(['1b', '0b', 'true']), 0.4);
  } else if (short === 'test_block') {
    maybe('mode', bad(r, 0.1) ? r.pick(['5', text('nope'), text('START')]) : text(r.pick(['start', 'log', 'fail', 'accept'])), 0.7);
    maybe('message', bad(r, 0.05) ? '5' : text(r.pick(['', 'hi', 'a\nb'])), 0.4);
    maybe('powered', r.pick(['1b', '0b', 'true']), 0.5);
  } else if (short === 'structure_block') {
    maybe('name', bad(r, 0.05) ? '5' : text(r.pick(['', 'a', 'minecraft:b/c', 'Bad:Name', 'a b', 'x'])), 0.6);
    for (const key of ['author', 'metadata']) maybe(key, bad(r, 0.05) ? '5' : text(r.pick(['', 'me', 'é'])), 0.3);
    for (const key of ['posX', 'posY', 'posZ', 'sizeX', 'sizeY', 'sizeZ']) maybe(key, bad(r, 0.05) ? text('x') : r.pick(['0', '3', '100', '-100', '2.7d', '-4']), 0.4);
    maybe('rotation', bad(r, 0.1) ? r.pick(['5', text('nope'), text('none')]) : text(r.pick(['NONE', 'CLOCKWISE_90', 'CLOCKWISE_180', 'COUNTERCLOCKWISE_90'])), 0.4);
    maybe('mirror', bad(r, 0.1) ? r.pick(['5', text('nope')]) : text(r.pick(['NONE', 'LEFT_RIGHT', 'FRONT_BACK'])), 0.4);
    maybe('mode', bad(r, 0.1) ? r.pick(['5', text('nope'), text('save')]) : text(r.pick(['SAVE', 'LOAD', 'CORNER', 'DATA'])), 0.6);
    for (const key of ['ignoreEntities', 'strict', 'powered', 'showair', 'showboundingbox']) maybe(key, bad(r, 0.05) ? text('x') : r.pick(['1b', '0b', 'true']), 0.3);
    maybe('integrity', bad(r, 0.05) ? text('x') : r.pick(['1.0f', '2.5f', '-1.0f', '0.5d', '3']), 0.3);
    maybe('seed', bad(r, 0.05) ? text('x') : r.pick(['5L', '2.9d', '-1L']), 0.3);
  } else if (short === 'decorated_pot') {
    const sherd = () => bad(r, 0.05) ? r.pick(['5', text('minecraft:nope'), text('Bad')]) : text(r.pick(['minecraft:brick', 'minecraft:angler_pottery_sherd', 'minecraft:stone', 'arms_up_pottery_sherd']));
    maybe('sherds', bad(r, 0.05) ? r.pick(['5', text('x')]) : list(Array.from({ length: r.int(6) }, sherd)), 0.7);
    if (exotic(r, 0.2)) maybe('item', stack(r, 1), 1);
  } else if (/^suspicious_/.test(short)) {
    if (r.chance(0.6)) maybe('item', bad(r, 0.1) ? r.pick(['5', '{}']) : stack(r, 1), 1);
    maybe('hit_direction', bad(r, 0.1) ? r.pick([text('up'), '[1]']) : r.pick(['2', '7', '-1', '2.5d']), 0.4);
    if (exotic(r, 0.1)) maybe('LootTable', text('minecraft:archaeology/desert_pyramid'), 1);
  }
  else if (/sculk_/.test(short)) {
    maybe('last_vibration_frequency', bad(r, 0.1) ? text('x') : r.pick(['0', '5', '15', '-3', '2.5d']), 0.5);
    maybe('warning_level', bad(r, 0.1) ? text('x') : r.pick(['0', '2', '4', '-1']), 0.5);
    if (r.chance(0.7)) {
      const selector = bad(r, 0.1) ? r.pick(['{}', '5']) : compound([['tick', r.pick(['-1L', '0L', '7L', '5'])]]);
      const fields = [['event_delay', bad(r, 0.1) ? text('x') : r.pick(['0', '3', '40', '-2'])]];
      if (r.chance(0.8)) fields.push(['selector', selector]);
      if (exotic(r, 0.1)) fields.push(['event', compound([['distance', '2.0f'], ['game_event', text('minecraft:step')], ['pos', '[1.0d,2.0d,3.0d]']])]);
      maybe('listener', r.chance(0.95) ? compound(fields) : '5', 1);
    }
    if (exotic(r, 0.2)) maybe('cursors', r.pick(['[]', list([compound([['pos', '[I;1,2,3]']])])]), 1);
  } else if (short === 'beacon') {
    const effect = () => bad(r, 0.1) ? r.pick(['5', text('minecraft:nope'), text('Bad')]) : text(r.pick(['minecraft:speed', 'haste', 'minecraft:resistance', 'minecraft:jump_boost', 'minecraft:strength', 'minecraft:regeneration', 'minecraft:slowness', 'minecraft:poison']));
    maybe('primary_effect', effect(), 0.6);
    maybe('secondary_effect', effect(), 0.5);
    maybe('Levels', bad(r, 0.1) ? text('x') : r.pick(['0', '1', '4', '5', '300', '-1', '2.5d']), 0.6);
    maybe('Paper.Range', bad(r, 0.1) ? text('x') : r.pick(['-1.0d', '20.0d', '5', '0.5f', '-3.5d']), 0.4);
  } else if (/command_block$/.test(short)) {
    maybe('Command', bad(r, 0.05) ? '5' : text(r.pick(['', 'say hi', 'Searge', 'searge', 'SEARGE', 'ſearge', ' ', '/tp @a 0 0 0', 'é'])), 0.7);
    maybe('SuccessCount', bad(r, 0.1) ? text('x') : small(r), 0.4);
    maybe('CustomName', bad(r, 0.1) ? '5' : richComponent(r), 0.3);
    maybe('TrackOutput', r.pick(['1b', '0b', 'true', '5']), 0.4);
    maybe('LastOutput', bad(r, 0.1) ? '5' : richComponent(r), 0.3);
    maybe('UpdateLastExecution', r.pick(['1b', '0b']), 0.4);
    maybe('LastExecution', bad(r, 0.1) ? text('x') : r.pick(['-1L', '0L', '5L', '100L', '3']), 0.4);
    for (const key of ['powered', 'conditionMet', 'auto']) maybe(key, bad(r, 0.05) ? text('x') : r.pick(['1b', '0b', 'true']), 0.5);
  } else if (/skull$|_head$/.test(short)) {
    maybe('note_block_sound', bad(r, 0.1) ? r.pick(['5', text('Bad')]) : text(r.pick(['minecraft:block.note_block.harp', 'x', 'a:b/c'])), 0.5);
    maybe('custom_name', bad(r, 0.1) ? '5' : richComponent(r), 0.4);
    if (exotic(r, 0.1)) maybe('profile', r.pick([text('Steve'), compound([['name', text('Alex')]])]), 1);
  } else if (/shelf$/.test(short)) {
    maybe('align_items_to_bottom', bad(r, 0.1) ? text('x') : r.pick(['1b', '0b', 'true']), 0.6);
    if (r.chance(0.5)) maybe('Items', r.pick(['[]', itemList(r, 3)]), 1);
  } else if (short === 'chiseled_bookshelf') { items(6); maybe('last_interacted_slot', bad(r, 0.1) ? number(r, false) : small(r), 0.6); }
  // Never the text {}: the server's parser returns one shared compound for it, which the importer changes.
  if (!entries.length) entries.push(['id', text(typeId)]);
  return compound(entries);
}

// A saved item entity, or with `frame` ({ id, block, facing } or just the id) an item frame.
function entityItem(r, i, at = null, frame = null) {
  const frameId = frame && (frame.id || frame);
  const entries = [];
  const maybe = (key, value, p = 0.3) => r.chance(p) && entries.push([key, value]);
  const num = () => bad(r, 0.1) ? r.pick(['[1.0d]', text('x')]) : r.pick(['0.0d', '1.5d', '7.25d', '-3.5d', '0.5f', '3', '2b', '100s', '4L', '1e3', '-0.0d', '12.7d', '30000000.0d', '99999999.0d']);
  entries.push(['id', text(frame ? frameId : !bad(r, 0.03) ? 'minecraft:item' : r.pick(['item', 'minecraft:egg', 'minecraft:tnt']))]);
  const triple = (a, b, c) => list([a, b, c]);
  const pos = at ? triple(`${at[0]}d`, `${at[1]}d`, `${at[2]}d`) : triple(`${r.int(16)}.5d`, `${1 + r.int(20)}.0d`, `${r.int(16)}.25d`);
  maybe('Pos', !bad(r, 0.1) ? pos : r.pick([triple(num(), num(), num()), list([text('a')]), '5', list(['1.0d', '2.0d']), triple('1e300d', '2e300d', '-1e300d')]), 0.95);
  const motion = () => `${(r.int(40) - 20) / 100}d`;
  maybe('Motion', !bad(r, 0.2) ? triple(motion(), motion(), motion()) : r.pick([triple(num(), num(), num()), triple('11.0d', '-12.5d', '10.0d'), '5']), 0.7);
  maybe('Rotation', !bad(r, 0.2) ? r.pick([list([`${r.int(720) - 360}.0f`, `${r.int(200) - 100}.0f`]), list(['1d', '2d']), list(['400f', '100f']), list(['-0.0f', '0.0f'])]) : r.pick([list(['1f']), '5']), 0.5);
  maybe('fall_distance', num(), 0.3);
  maybe('Fire', bad(r, 0.1) ? text('x') : r.pick(['0s', '-1s', '20s', '40000']), 0.2);
  maybe('Air', bad(r, 0.1) ? text('x') : r.pick(['300s', '100', '400']), 0.2);
  maybe('OnGround', bad(r, 0.1) ? '5' : r.pick(['1b', '0b', 'true']), 0.3);
  maybe('Invulnerable', r.pick(['1b', '0b', 'true']), 0.15);
  maybe('PortalCooldown', num(), 0.15);
  maybe('UUID', !bad(r, 0.1) ? (r.chance(0.9) ? `[I;${1000 + i},${r.int(1000)},${r.int(1000)},${r.int(1000)}]` : list([String(5000 + i), '2', '3', '4'])) : r.pick(['[I;1,2,3]', '[B;1,2,3,4]', text('x'), '[I;1,2,3,4,5]']), 0.95);
  maybe('CustomName', bad(r, 0.1) ? '5' : exotic(r, 0.3) ? richComponent(r) : r.pick([text('N'), compound([['text', text('x')]])]), 0.15);
  for (const key of ['CustomNameVisible', 'Silent', 'NoGravity', 'Glowing']) maybe(key, r.pick(['1b', '0b', 'true']), 0.1);
  if (exotic(r, 0.05)) maybe('TicksFrozen', r.pick(['5', '300']), 1);
  if (exotic(r, 0.05)) maybe('Tags', list([text('a')]), 1);
  if (exotic(r, 0.03)) maybe('data', '{a:1}', 1);
  maybe('Health', bad(r, 0.1) ? text('x') : r.pick(['5s', '1s', '0s', '-3s', '40000']), 0.3);
  maybe('Age', bad(r, 0.1) ? text('x') : r.pick(['0s', '5900s', '6000s', '-32768s', '100000']), 0.3);
  maybe('PickupDelay', bad(r, 0.1) ? text('x') : r.pick(['0s', '10s', '32767s', '-1s']), 0.3);
  maybe('Owner', bad(r, 0.1) ? '5' : r.pick(['[I;1,2,3,4]', '[1,2,3,4]']), 0.1);
  if (exotic(r, 0.05)) maybe('Thrower', r.pick(['[I;1,2,3,4]', compound([['uuid', '[I;1,2,3,4]']])]), 1);
  maybe('Item', !bad(r, 0.05) ? stack(r, 1) : r.pick(['5', compound([])]), 0.95);
  maybe('Paper.Origin', triple('1.0d', '2.0d', '3.0d'), 0.2);
  maybe('Paper.OriginWorld', '[I;5,6,7,8]', 0.1);
  maybe('Paper.SpawnReason', text(bad(r, 0.1) ? 'nope' : r.pick(['DEFAULT', 'NATURAL', 'SPAWNER'])), 0.15);
  if (exotic(r, 0.05)) maybe('Paper.FromMobSpawner', '1b', 1);
  if (exotic(r, 0.03)) maybe('Paper.FrictionState', text('NOT_SET'), 1);
  if (exotic(r, 0.05)) maybe(r.pick(['Bukkit.persist', 'Bukkit.visibleByDefault', 'Bukkit.invisible']), r.pick(['1b', '0b']), 1);
  maybe('Bukkit.MaxAirSupply', '300', 0.05);
  if (frame) {
    const block = frame.block || (at ? at.map((v) => Math.floor(v)) : [r.int(16), 1 + r.int(20), r.int(16)]);
    maybe('block_pos', !bad(r, 0.1) ? `[I;${block[0] + (r.chance(0.1) ? r.int(40) - 20 : 0)},${block[1]},${block[2]}]` : r.pick(['[I;1,2]', list(['1', '2', '3']), '5', '[I;1,2,3,4]']), 0.9);
    maybe('Facing', bad(r, 0.1) ? text('x') : frame.facing !== undefined && !r.chance(0.1) ? `${frame.facing}b` : r.pick(['0b', '1b', '2b', '3b', '4b', '5b', '6b', '9b', '-1b', '-7b', '3', '2s']), 0.9);
    maybe('ItemRotation', bad(r, 0.1) ? text('x') : r.pick(['0b', '1b', '7b', '8b', '9b', '-3b', '-8b', '127b', '-128b', '5']), 0.5);
    maybe('ItemDropChance', bad(r, 0.1) ? text('x') : r.pick(['0.0f', '1.0f', '0.5f', '2.5f', '-1.0f', '1', '0.25d']), 0.4);
    maybe('Invisible', bad(r, 0.1) ? text('x') : r.pick(['1b', '0b', 'true']), 0.2);
    maybe('Fixed', bad(r, 0.1) ? text('x') : r.pick(['1b', '0b', 'true']), 0.2);
  }
  return compound(entries);
}

function snbtValue(r, depth = 0) {
  const kinds = depth > 3 ? 8 : 11;
  switch (r.int(kinds)) {
    case 0: return r.pick(['0', '1', '-1', '127', '255', '65535', '2147483647', '-2147483648', '0x7f', '0xff', '0b101', '1_000', '+5', '007', '0_1']);
    case 1: return r.pick(['1b', '255ub', '-1sb', '1s', '40000us', '1L', '9223372036854775807L', '0xffffffffffUL', '1sI', '1uL']);
    case 2: return r.pick(['1.5', '.5', '1.', '1e3', '1E-3', '1f', '1.5F', '1d', '1e400', '1_0.5_0', '-0.0', '1.7976931348623157e308', '3.4028235e38f', '3.5e38f', '1e-50f', '0.1', '123456789.123456789']);
    case 3: return text(r.pick(['', 'x', 'a b', 'a"b', "a'b", 'a\\b', 'é', '\n\t', '\u0001', '😀', 'a\u0000b']));
    case 4: return r.pick(["'x'", "'a\"b'", "'a\\'b'", '"\\s\\x41\\u00e9\\U0001F600"', '"\\ud83d\\ude00"', '"\\ud83d"', '"\\N{BULLET}"', '"\\q"', '"x', "'x"]);
    case 5: return r.pick(['true', 'false', 'True', 'FALSE', 'abc', 'a.b-c+d', 'x_y', '-abc', '1abc', 'abc(1)', 'bool(1)', 'uuid("01234567-89ab-cdef-0123-456789abcdef")']);
    case 6: return r.pick(['[B;1,2]', '[I;1,2b,3s]', '[L;1,2]', '[B;1L]', '[I;]', '[B;255ub,-1b]', '[I;1,2,]', '[L;0xffUL]', '[I;1.5]', '[B;', '[X;1]', '[B]']);
    case 7: return r.pick(['{}', '{ }', '{a:1}', '{"":1}', '{a:1,}', '{,}', '{a:1 b:2}', '{a:1,a:2}', '{ a : 1 , b : [ 1 , 2 ] }', '{true:1}', '{1:2}', '{-:1}']);
    case 8: return list(Array.from({ length: r.int(4) }, () => snbtValue(r, depth + 1)));
    case 9: return compound(Array.from({ length: r.int(4) }, () => [r.pick(['a', 'b', 'c d', 'true', 'x.y', '1', 'é']), snbtValue(r, depth + 1)]));
    default: return r.pick(['[1,2.5,"x",{a:1}]', '[[1],[2,3]]', '{a:{b:{c:[{d:1b}]}}}']);
  }
}


// A stack of this item and count, with components some of the time.
function stackOf(r, id, count, components) {
  const entries = [['id', text(id)], ['count', String(count)]];
  if (components) entries.push(['components', compound(Array.from({ length: 1 + r.int(3) }, () => component(r, 1)))]);
  return compound(entries);
}

// The block entity type a block is saved as, or null for a block with none this port could hold.
function blockEntityType(id) {
  const short = id.replace('minecraft:', '');
  if (/shulker_box$/.test(short)) return 'minecraft:shulker_box';
  if (/hanging_sign$/.test(short)) return 'minecraft:hanging_sign';
  if (/sign$/.test(short)) return 'minecraft:sign';
  if (/banner$/.test(short)) return 'minecraft:banner';
  if (/^(chest|trapped_chest|barrel|hopper|dispenser|dropper|crafter|comparator|lectern|furnace|blast_furnace|smoker|brewing_stand|jukebox|chiseled_bookshelf|bell|daylight_detector)$/.test(short)) return id;
  return null;
}

// ---- Saved data of an older data version, as old worlds and the community
// builds hold it: stacks with Count and tag (before 1.20.5's components,
// data version 3818), text as JSON in strings, signs' Text1..4 (before 1.20,
// 3439), item frames' TileX..Z (before 3813). The data fixer upgrades it.

const OLD_VERSIONS = [1952, 2586, 2730, 2975, 3120, 3218, 3465, 3700];
const OLD_ITEMS = ['minecraft:stone', 'minecraft:diamond', 'minecraft:iron_ingot', 'minecraft:redstone', 'minecraft:cobblestone', 'minecraft:ender_pearl', 'minecraft:diamond_sword',
  'minecraft:iron_pickaxe', 'minecraft:written_book', 'minecraft:writable_book', 'minecraft:book', 'minecraft:potion', 'minecraft:oak_sign', 'minecraft:chest', 'minecraft:hopper',
  'minecraft:egg', 'minecraft:white_banner', 'minecraft:grass', 'minecraft:scute', 'minecraft:music_disc_13', 'minecraft:shulker_box', 'minecraft:player_head', 'minecraft:enchanted_book'];
// The items of the version: grass became short_grass at data version 3692.
const oldItemsOf = (version) => (version < 3692 ? OLD_ITEMS : OLD_ITEMS.filter((id) => id !== 'minecraft:grass'));
const oldText = (r) => text(JSON.stringify(r.pick([{ text: 'Name' }, { text: 'A', color: 'red' }, { translate: 'block.minecraft.chest' }, { text: '' }, { text: 'x', bold: true, extra: [{ text: 'y' }] }])));

// An old stack of this item and count: its tag holds a name and lore, damage
// and enchantments, a book's pages, a potion, a banner's patterns.
function oldStack(r, id, count) {
  const tag = [];
  if (r.chance(0.2)) tag.push(['display', compound([['Name', oldText(r)], r.chance(0.5) && ['Lore', list([text(JSON.stringify({ text: 'l1' })), text(JSON.stringify('l2'))])]])]);
  if (/sword|pickaxe/.test(id)) {
    if (r.chance(0.5)) tag.push(['Damage', String(r.int(40))]);
    if (r.chance(0.4)) tag.push(['Enchantments', list([compound([['id', text(r.pick(['minecraft:sharpness', 'minecraft:efficiency', 'minecraft:unbreaking']))], ['lvl', `${1 + r.int(3)}s`]])])]);
  }
  if (id === 'minecraft:enchanted_book' && r.chance(0.8)) tag.push(['StoredEnchantments', list([compound([['id', text('minecraft:mending')], ['lvl', '1s']])])]);
  if (id === 'minecraft:written_book' && r.chance(0.9)) {
    tag.push(['title', text(r.pick(['T', 'Levels']))], ['author', text('A')], ['pages', list(Array.from({ length: r.int(5) }, () => (r.chance(0.7) ? text(JSON.stringify({ text: `page ${r.int(100)}` })) : text(`"p${r.int(9)}"`))))]);
    if (r.chance(0.5)) tag.push(['resolved', r.pick(['1b', '0b'])]);
    if (r.chance(0.3)) tag.push(['generation', String(r.int(3))]);
  }
  if (id === 'minecraft:writable_book' && r.chance(0.9)) tag.push(['pages', list(Array.from({ length: r.int(5) }, () => text(`line ${r.int(100)}`)))]);
  if (id === 'minecraft:potion' && r.chance(0.9)) tag.push(['Potion', text(r.pick(['minecraft:water', 'minecraft:swiftness', 'minecraft:awkward']))]);
  if (id === 'minecraft:white_banner' && r.chance(0.5)) tag.push(['BlockEntityTag', compound([['Patterns', list([compound([['Pattern', text(r.pick(['bs', 'cr', 'mr']))], ['Color', String(r.int(16))]])])]])]);
  if (id === 'minecraft:player_head' && r.chance(0.5)) tag.push(['SkullOwner', text('Steve')]);
  if (r.chance(0.1)) tag.push(['RepairCost', String(r.int(5))]);
  return compound([['id', text(id)], ['Count', `${count}b`], tag.length && ['tag', compound(tag)]]);
}

// An old stack of the item, of a count within its stack size.
function oldCount(r, id) {
  const max = /sword|pickaxe|potion|music_disc|shulker_box|enchanted_book|writable_book/.test(id) ? 1 : /written_book|banner|sign|egg|ender_pearl/.test(id) ? 16 : 64;
  return oldStack(r, id, 1 + r.int(Math.min(max, 16)));
}

function oldItems(r, size, version, pool = oldItemsOf(version)) {
  return list(Array.from({ length: r.int(5) }, (_, i) => `{Slot:${(i * 3 + r.int(3)) % size}b,${oldCount(r, r.pick(pool)).slice(1)}`));
}

// An old block entity of the type (blockEntityType), or null for a type the
// version had not.
function oldBlockEntity(r, type, version) {
  const short = type.replace('minecraft:', '');
  if (short === 'crafter' || (short === 'chiseled_bookshelf' && version < 3120) || (short === 'hanging_sign' && version < 3120)) return null;
  const entries = [r.chance(0.7) && ['id', text(type)]];
  if (r.chance(0.15) && !/sign|comparator|bell|daylight|jukebox/.test(short)) entries.push(['CustomName', oldText(r)]);
  if (/chest$|barrel|dispenser|dropper|shulker_box/.test(short)) entries.push(['Items', oldItems(r, short === 'dispenser' || short === 'dropper' ? 9 : 27, version)]);
  else if (short === 'hopper') entries.push(['Items', oldItems(r, 5, version)], r.chance(0.5) && ['TransferCooldown', String(r.int(9))]);
  else if (/furnace$|smoker/.test(short)) entries.push(['Items', oldItems(r, 3, version, ['minecraft:iron_ore', 'minecraft:coal', 'minecraft:cobblestone', 'minecraft:iron_ingot'])], ['BurnTime', `${r.int(300)}s`], ['CookTime', `${r.int(200)}s`], ['CookTimeTotal', '200s'], r.chance(0.3) && ['RecipesUsed', '{"minecraft:iron_ingot":2}']);
  else if (short === 'brewing_stand') entries.push(['Items', oldItems(r, 5, version, ['minecraft:potion', 'minecraft:nether_wart', 'minecraft:blaze_powder'])], ['BrewTime', `${r.int(400)}s`], ['Fuel', `${r.int(20)}b`]);
  else if (short === 'comparator') entries.push(['OutputSignal', String(r.int(16))]);
  else if (short === 'jukebox') { if (r.chance(0.5)) entries.push(['RecordItem', oldStack(r, 'minecraft:music_disc_13', 1)], ['IsPlaying', r.pick(['1b', '0b'])]); }
  else if (short === 'lectern') { if (r.chance(0.8)) entries.push(['Book', oldStack(r, r.pick(['minecraft:written_book', 'minecraft:writable_book']), 1)], ['Page', String(r.int(4))]); }
  else if (short === 'chiseled_bookshelf') entries.push(['Items', oldItems(r, 6, version, ['minecraft:book', 'minecraft:written_book', 'minecraft:enchanted_book'])], ['last_interacted_slot', String(r.int(6) - 1)]);
  else if (short === 'banner') entries.push(['Patterns', list([compound([['Pattern', text(r.pick(['bs', 'cr', 'mr', 'gra']))], ['Color', String(r.int(16))]])])]);
  else if (/sign$/.test(short)) {
    const line = () => text(JSON.stringify(r.pick([{ text: '' }, { text: 'Hi' }, { text: 'a', color: 'red' }, '', 'plain'])));
    if (version < 3439) entries.push(['Text1', line()], ['Text2', line()], ['Text3', line()], ['Text4', line()], ['Color', text(r.pick(['black', 'red']))], version >= 2690 && r.chance(0.5) && ['GlowingText', r.pick(['1b', '0b'])]);
    else entries.push(['front_text', compound([['messages', list([line(), line(), line(), line()])], ['color', text('black')], ['has_glowing_text', '0b']])], ['is_waxed', r.pick(['1b', '0b'])]);
  }
  const kept = entries.filter(Boolean);
  // Never the text {}: the server's parser returns one shared compound for it, which the importer changes.
  return compound(kept.length ? kept : [['id', text(type)]]);
}

// An old item entity, or with `frame` ({ id, block, facing }) an item frame.
function oldEntityItem(r, i, at, version, frame = null) {
  const pos = list([`${at[0]}d`, `${at[1]}d`, `${at[2]}d`]);
  const base = [['id', text(frame ? frame.id : 'minecraft:item')], ['UUID', `[I;${2000 + i},${r.int(1000)},${r.int(1000)},${r.int(1000)}]`], ['Pos', pos], ['Motion', '[0.0d,0.0d,0.0d]'], ['Rotation', '[0.0f,0.0f]']];
  if (!frame) return compound([...base, ['Item', oldCount(r, r.pick(oldItemsOf(version)))], ['Age', '0s'], ['PickupDelay', r.pick(['0s', '10s', '32767s'])], ['Health', '5s']]);
  return compound([...base, ['TileX', String(frame.block[0])], ['TileY', String(frame.block[1])], ['TileZ', String(frame.block[2])], ['Facing', `${frame.facing}b`],
    r.chance(0.8) && ['Item', oldStack(r, r.pick(oldItemsOf(version)), 1)], ['ItemRotation', `${r.int(8)}b`], ['ItemDropChance', '1.0f']]);
}

module.exports = { OLD_VERSIONS, oldStack, oldBlockEntity, oldEntityItem, richComponent, random, text, compound, list, number, stack, stackOf, snbtValue, blockEntity, blockEntityType, entityItem, itemList, BLOCK_ENTITIES };
