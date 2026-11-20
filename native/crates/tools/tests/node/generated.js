'use strict';

// The generated calls of the replay (prepare.js): every argument error and
// refusal of the ported tools, programs that throw, time out, run out of
// memory, read files, use mc.settled, change nothing, write files, leave a
// bed half or a block that cannot survive, on a few synthetic worlds. All
// synthetic: no chat text.

const fs = require('fs');
const path = require('path');

const ids = {};
const id = (name) => {
  ids[name] ||= `00000000-0000-4000-8000-${String(Object.keys(ids).length + 1).padStart(12, '0')}`;
  return ids[name];
};

function worlds(World) {
  const empty = () => new World();
  const platform = () => {
    const world = new World();
    world.fill([0, 0, 0], [9, 0, 9], 'minecraft:stone', {});
    world.finalize();
    return world;
  };
  const house = () => {
    const world = new World();
    world.fill([0, 0, 0], [6, 0, 6], 'minecraft:cobblestone', {});
    world.fill([0, 1, 0], [6, 3, 0], 'minecraft:oak_planks', {});
    world.set(3, 1, 3, 'minecraft:red_bed', { part: 'foot', facing: 'north' });
    world.set(3, 1, 2, 'minecraft:red_bed', { part: 'head', facing: 'north' });
    world.set(1, 1, 1, 'minecraft:torch', {});
    world.finalize();
    return world;
  };
  const ticked = () => {
    const world = platform();
    world.settleTicks = 0;
    return world;
  };
  const sand = () => {
    const world = new World();
    world.set(0, 5, 0, 'minecraft:sand', {});
    world.set(4, 0, 4, 'minecraft:stone', {});
    world.finalize();
    return world;
  };
  return { empty, platform, house, ticked, sand };
}

const build = (program, extra = {}) => JSON.stringify({ summary: 'generated', program, ...extra });

const PROGRAMS = {
  nothing: 'module.exports = (mc) => {}',
  log: 'module.exports = (mc) => { mc.log("one", 2, {a: 1}); mc.log([1, 2]) }',
  set: 'module.exports = (mc) => { mc.set([20, 1, 20], "gold_block") }',
  setSame: 'module.exports = (mc) => { mc.set([0, 0, 0], "stone") }',
  fill: 'module.exports = (mc) => { mc.fill([0, 1, 0], [15, 3, 15], "oak_planks") }',
  clear: 'module.exports = (mc) => { mc.fill([0, 0, 0], [9, 0, 9], "air") }',
  stairs: 'module.exports = (mc) => { mc.set([2, 1, 2], "oak_stairs[facing=east,half=top]"); mc.set([3, 1, 2], {id: "oak_stairs", states: {facing: "west"}}) }',
  fence: 'module.exports = (mc) => { for (let x = 0; x < 5; x++) mc.set([x, 1, 5], "oak_fence") }',
  sandFall: 'module.exports = (mc) => { mc.set([30, 10, 30], "sand"); mc.set([31, 10, 30], "gravel") }',
  water: 'module.exports = (mc) => { mc.set([40, 2, 40], "water"); mc.set([40, 0, 40], "stone") }',
  lava: 'module.exports = (mc) => { mc.set([44, 2, 44], "lava") }',
  torchAir: 'module.exports = (mc) => { mc.set([50, 5, 50], "torch") }',
  torchsAir: 'module.exports = (mc) => { for (let i = 0; i < 12; i++) mc.set([50 + i * 2, 5, 50], "torch") }',
  ladderAir: 'module.exports = (mc) => { mc.set([55, 5, 55], "ladder[facing=north]") }',
  poppyAir: 'module.exports = (mc) => { mc.set([60, 5, 60], "poppy") }',
  torchHeld: 'module.exports = (mc) => { mc.set([50, 4, 50], "stone"); mc.set([50, 5, 50], "torch") }',
  bedFoot: 'module.exports = (mc) => { mc.set([5, 1, 5], "red_bed[part=foot,facing=south]") }',
  bedHead: 'module.exports = (mc) => { mc.set([5, 1, 5], "blue_bed[part=head,facing=east]") }',
  bedPair: 'module.exports = (mc) => { mc.set([5, 1, 5], "red_bed[part=foot,facing=south]"); mc.set([5, 1, 6], "red_bed[part=head,facing=south]") }',
  bedMismatch: 'module.exports = (mc) => { mc.set([5, 1, 5], "red_bed[part=foot,facing=south]"); mc.set([5, 1, 6], "red_bed[part=head,facing=north]") }',
  bedColour: 'module.exports = (mc) => { mc.set([5, 1, 5], "red_bed[part=foot,facing=south]"); mc.set([5, 1, 6], "blue_bed[part=head,facing=south]") }',
  bedBorder: 'module.exports = (mc) => { mc.set([15, 1, 5], "red_bed[part=foot,facing=east]") }',
  bedDefault: 'module.exports = (mc) => { mc.set([7, 1, 7], "red_bed") }',
  removeBedHalf: 'module.exports = (mc) => { mc.set([3, 1, 2], "air") }',
  throwError: 'module.exports = (mc) => { throw new Error("deliberate") }',
  throwString: 'module.exports = (mc) => { throw "a string" }',
  throwNull: 'module.exports = (mc) => { throw null }',
  throwAfterLog: 'module.exports = (mc) => { mc.log("before"); mc.set([1, 1, 1], "stone"); throw new Error("after") }',
  throwLong: 'module.exports = (mc) => { throw new Error("x".repeat(9000)) }',
  throwLongEmoji: 'module.exports = (mc) => { throw new Error("\u{1F600}".repeat(4500)) }',
  syntax: 'module.exports = (mc) => { mc.set([0, 0, 0], ',
  reference: 'module.exports = (mc) => { undefinedThing.call() }',
  typeError: 'module.exports = (mc) => { null.x }',
  unknownBlock: 'module.exports = (mc) => { mc.set([0, 1, 0], "oak") }',
  badState: 'module.exports = (mc) => { mc.set([0, 1, 0], "oak_stairs[facing=up]") }',
  noState: 'module.exports = (mc) => { mc.set([0, 1, 0], "stone[x=1]") }',
  notFunction: 'module.exports = 5',
  noExports: 'const x = 1',
  outOfRange: 'module.exports = (mc) => { mc.set([5000, 0, 0], "stone") }',
  tooHigh: 'module.exports = (mc) => { mc.set([0, 400, 0], "stone") }',
  readFile: 'module.exports = (mc) => { mc.log(mc.readFile("data/points.txt")) }',
  readJson: 'module.exports = (mc) => { const p = JSON.parse(mc.readFile("/work/data/points.json")); for (const [x, y, z] of p) mc.set([x, y, z], "stone") }',
  readMissing: 'module.exports = (mc) => { mc.readFile("data/missing.txt") }',
  readEscape: 'module.exports = (mc) => { mc.readFile("../../outside.txt") }',
  readAbsolute: 'module.exports = (mc) => { mc.readFile("/etc/passwd") }',
  readLink: 'module.exports = (mc) => { mc.readFile("data/link-out") }',
  readDir: 'module.exports = (mc) => { mc.readFile("data") }',
  readBig: 'module.exports = (mc) => { mc.readFile("data/big.txt") }',
  readBinary: 'module.exports = (mc) => { mc.log(mc.readFile("data/binary.bin").length) }',
  readCaught: 'module.exports = (mc) => { try { mc.readFile("nope") } catch (e) { mc.log(e.message) } }',
  settled: 'module.exports = (mc) => { mc.log(JSON.stringify(mc.settled(0, 0, 0)), mc.settledTicks, JSON.stringify(mc.settled(100, 100, 100))) }',
  settledBed: 'module.exports = (mc) => { mc.log(JSON.stringify(mc.settled(3, 1, 3)), JSON.stringify(mc.settled(1, 1, 1))) }',
  settledBad: 'module.exports = (mc) => { mc.settled(0.5, 0, 0) }',
  settledArgs: 'module.exports = (mc) => { mc.settled("a", 0) }',
  writeFile: 'module.exports = (mc) => { mc.writeFile("out/a.txt", "hello"); mc.writeFile("/work/out/b.json", "{}") }',
  writeAndSet: 'module.exports = (mc) => { mc.writeFile("out/c.txt", "x".repeat(1000)); mc.set([25, 1, 25], "stone") }',
  writeEscape: 'module.exports = (mc) => { mc.writeFile("../escape.txt", "no") }',
  writeLink: 'module.exports = (mc) => { mc.writeFile("data/link-out", "no") }',
  writeDir: 'module.exports = (mc) => { mc.writeFile("data", "no") }',
  dump: 'module.exports = (mc) => { mc.dump("out/dump.json") }',
  timeout: 'module.exports = (mc) => { while (true) {} }',
  oom: 'module.exports = (mc) => { const a = []; while (true) a.push(new Array(1e6).fill(1)) }',
};

function workFiles(dataDir, conversation) {
  const work = path.join(dataDir, 'sandbox', conversation);
  fs.mkdirSync(path.join(work, 'data'), { recursive: true });
  fs.writeFileSync(path.join(work, 'data/points.txt'), 'line one\nline two\n');
  fs.writeFileSync(path.join(work, 'data/points.json'), JSON.stringify([[1, 1, 1], [2, 1, 1], [3, 1, 1]]));
  fs.writeFileSync(path.join(work, 'data/big.txt'), Buffer.alloc(9 * 1024 * 1024, 97));
  fs.writeFileSync(path.join(work, 'data/binary.bin'), Buffer.from([0xff, 0xfe, 0x41, 0xc3]));
  fs.writeFileSync(path.join(dataDir, 'sandbox', 'outside.txt'), 'outside');
  fs.mkdirSync(path.join(work, 'images'), { recursive: true });
  // A 1x1 PNG.
  fs.writeFileSync(path.join(work, 'images/dot.png'), Buffer.from('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082', 'hex'));
  fs.writeFileSync(path.join(work, 'notes.txt'), 'not an image');
  try { fs.symlinkSync('/etc/passwd', path.join(work, 'data/link-out')); } catch (error) { if (error.code !== 'EEXIST') throw error; }
  try { fs.symlinkSync('/etc/hostname', path.join(work, 'images/out.png')); } catch (error) { if (error.code !== 'EEXIST') throw error; }
}

function calls({ World, dataDir }) {
  const made = worlds(World);
  const list = [];
  const add = (label, call) => list.push({ label, ...call });

  // build: every program on each world, in its own conversation per world.
  const programWorlds = ['empty', 'platform', 'house', 'ticked', 'sand'];
  for (const worldName of programWorlds) {
    const conversation = id(`build-${worldName}`);
    workFiles(dataDir, conversation);
    for (const [programName, program] of Object.entries(PROGRAMS)) {
      if ((programName === 'timeout' || programName === 'oom') && worldName !== 'platform') continue;
      add(`build/${worldName}/${programName}`, { conversation, name: 'build', arguments: build(program), world: made[worldName]() });
    }
  }
  // build: argument errors and settle ticks.
  const args = id('build-args');
  const argCases = {
    missing: '{"summary":"x"}', empty: build(''), spaces: build(' \n\t'), bom: build('﻿'), zero: '{"program":0}', no: '{"program":false}',
    nullProgram: '{"program":null}', number: '{"program":5}', array: '{"program":["module.exports = (mc) => {}"]}', object: '{"program":{}}',
    ticksNegative: build(PROGRAMS.set, { settle_ticks: -1 }), ticksFraction: build(PROGRAMS.set, { settle_ticks: 1.5 }),
    ticksString: build(PROGRAMS.set, { settle_ticks: '210' }), ticksNull: build(PROGRAMS.set, { settle_ticks: null }),
    ticksOver: build(PROGRAMS.set, { settle_ticks: 72001 }), ticksHuge: build(PROGRAMS.set, { settle_ticks: 1e21 }),
    ticksTrue: build(PROGRAMS.set, { settle_ticks: true }), ticksZero: build(PROGRAMS.sandFall, { settle_ticks: 0 }),
    ticksOne: build(PROGRAMS.water, { settle_ticks: 1 }), ticksMany: build(PROGRAMS.lava, { settle_ticks: 400 }),
    ticksFloat: build(PROGRAMS.set, { settle_ticks: 30.0 }), noSummary: JSON.stringify({ program: PROGRAMS.set }),
  };
  for (const [label, text] of Object.entries(argCases)) add(`build-args/${label}`, { conversation: args, name: 'build', arguments: text, world: made.platform() });

  // bash: argument errors, the limit and network as passed, and each kind of
  // sandbox answer.
  const shell = id('bash');
  workFiles(dataDir, shell);
  const ok = { formatted: 'Exit code: 0\nWall time: 0.1 seconds\nOutput:\nhi', storage: null };
  const bashCases = [
    ['empty', '{"command":""}', ok], ['spaces', '{"command":"  \\n "}', ok], ['missing', '{}', ok], ['null', '{"command":null}', ok],
    ['zero', '{"command":0}', ok], ['false', '{"command":false}', ok], ['number', '{"command":42}', ok], ['array', '{"command":["ls","-la"]}', ok],
    ['object', '{"command":{}}', ok], ['plain', '{"command":"echo hi"}', ok],
    ['exit', '{"command":"false"}', { formatted: 'Exit code: 1\nWall time: 0 seconds\nOutput:\n', storage: null }],
    ['timeout5000', '{"command":"sleep 1","timeout_ms":5000}', ok], ['timeoutString', '{"command":"x","timeout_ms":"5000"}', ok],
    ['timeoutNegative', '{"command":"x","timeout_ms":-1}', ok], ['timeoutZero', '{"command":"x","timeout_ms":0}', ok],
    ['timeoutNull', '{"command":"x","timeout_ms":null}', ok], ['timeoutHuge', '{"command":"x","timeout_ms":1e9}', ok],
    ['timeoutArray', '{"command":"x","timeout_ms":[2000]}', ok], ['timeoutTrue', '{"command":"x","timeout_ms":true}', ok],
    ['timeoutFraction', '{"command":"x","timeout_ms":2500.5}', ok], ['timeoutWord', '{"command":"x","timeout_ms":"soon"}', ok],
    ['network', '{"command":"curl x","network":true}', ok], ['networkString', '{"command":"curl x","network":"true"}', ok],
    ['networkFalse', '{"command":"x","network":false}', ok], ['networkOne', '{"command":"x","network":1}', ok],
    ['storage', '{"command":"rm big"}', { formatted: 'Exit code: 0\nOutput:\n', storage: 'This chat\'s storage is full: ... After this command the chat is back within its limits; the next command runs normally.' }],
    ['throws', '{"command":"x"}', { throw: 'Sandbox shell exited before the command completed' }],
    ['unavailable', '{"command":"x"}', { throw: 'The sandbox is not available on this machine' }],
    ['halt', '{"command":"x"}', { throw: 'The sandbox\'s processes were ended: full Delete files in /work to continue.' }],
    ['unicode', '{"command":"echo \\u00e9\\ud83d\\ude00"}', { formatted: 'Exit code: 0\nOutput:\né\u{1F600}', storage: null }],
  ];
  for (const [label, text, exec] of bashCases) add(`bash/${label}`, { conversation: shell, name: 'bash', arguments: text, record: { exec } });

  // view_image: the path rules; the sandbox's normaliser is recorded as the
  // file's own bytes on both sides.
  const view = id('view');
  workFiles(dataDir, view);
  const viewCases = {
    png: '{"path":"images/dot.png"}', workPrefix: '{"path":"/work/images/dot.png"}', missing: '{"path":"images/none.png"}',
    empty: '{"path":""}', none: '{}', escape: '{"path":"../../outside.txt"}', absolute: '{"path":"/etc/hostname"}',
    link: '{"path":"images/out.png"}', dir: '{"path":"images"}', text: '{"path":"notes.txt"}', number: '{"path":5}', nullPath: '{"path":null}',
  };
  for (const [label, text] of Object.entries(viewCases)) add(`view_image/${label}`, { conversation: view, name: 'view_image', arguments: text, world: made.platform() });

  // The vision gate, an unknown tool and prompt_library outside a chat.
  const gate = id('gate');
  add('gate/view_image', { conversation: gate, name: 'view_image', arguments: viewCases.png, imageInput: false });
  add('gate/see_build', { conversation: gate, name: 'see_build', arguments: '{}', imageInput: false });
  add('gate/unknown', { conversation: gate, name: 'paint', arguments: '{}' });
  add('gate/prompt_library', { conversation: gate, name: 'prompt_library', arguments: '{}', chat: false });
  add('gate/build-text-only', { conversation: gate, name: 'build', arguments: build(PROGRAMS.set), imageInput: false, world: made.platform() });

  // see_build's argument checks (what is ported of it).
  const shots = id('see');
  const seeCases = {
    empty: ['{}', made.empty()], shotsEmpty: ['{"shots":[]}', made.platform()], shotsSeven: [JSON.stringify({ shots: Array(7).fill({}) }), made.platform()],
    shotsObject: ['{"shots":{}}', made.platform()], attach: ['{"attach":"no"}', made.platform()], shotNumber: ['{"shots":[{},3]}', made.platform()],
    shotArray: ['{"shots":[[]]}', made.platform()], tickFraction: ['{"shots":[{"tick":1.5}]}', made.platform()],
    tickOver: ['{"shots":[{"tick":99999}]}', made.platform()], tickString: ['{"shots":[{"view":"top"},{"tick":"5"}]}', made.platform()],
  };
  for (const [label, [text, world]] of Object.entries(seeCases)) add(`see_build/${label}`, { conversation: shots, name: 'see_build', arguments: text, world });

  // test_build: argument errors and refusals (callTests is today's on both
  // sides through the record), and the wrapping of recorded experiments.
  const tests = id('tests');
  workFiles(dataDir, tests);
  const report = (extra = {}) => ({ ok: true, ticks: 20, initial: { a: 1 }, observations: [{ tick: 0, blocks: [] }], probes: [[0, 0, 0]], actions: [], inputs: [], sampleTicks: [0, 20], preview: 'p', blockStateMigrations: [], importedEntities: 0, importedBlockEntities: 0, summary: 'fine', ...extra });
  const big = (n) => report({ observations: Array.from({ length: n }, (_, i) => ({ tick: i, state: 'x'.repeat(50) })) });
  const testCases = [
    ['none', '{}', []], ['emptyList', '{"tests":[]}', []], ['object', '{"tests":{}}', []], ['extra', '{"tests":[{}],"x":1}', []],
    ['nine', JSON.stringify({ tests: Array(9).fill({}) }), []], ['nullArgs', '{"tests":null}', []],
    ['one', '{"tests":[{}]}', [{ report: report() }]], ['two', '{"tests":[{},{}]}', [{ report: report() }, { report: report({ ticks: 40 }) }]],
    ['failed', '{"tests":[{}]}', [{ error: { message: 'The blueprint is empty' } }]],
    ['mixed', '{"tests":[{},{},{}]}', [{ report: report() }, { error: { message: 'probes requires 1..9007199254740991 entries' } }, { report: report({ ok: false, failure: 'design' }) }]],
    ['budget', '{"tests":[{}]}', [{ error: { message: 'over budget', budget: { resource: 'simulationTicks', requested: 80000, limit: 72000 }, code: 'VERIFICATION_BUDGET' } }]],
    ['budgetNoCode', '{"tests":[{}]}', [{ error: { message: 'over budget', budget: { resource: 'columns' } } }]],
    ['trimObservations', '{"tests":[{}]}', [{ report: big(400) }]],
    ['trimPreview', '{"tests":[{},{}]}', [{ report: report({ preview: 'p'.repeat(30000) }) }, { report: big(10) }]],
    ['trimAll', '{"tests":[{}]}', [{ report: report({ summary: 's'.repeat(30000) }) }]],
    ['reportKey', '{"tests":[{}]}', [{ report: { report: 'first', ok: true } }]],
    ['okNumber', '{"tests":[{}]}', [{ report: { ok: 1, x: 2.5 } }]],
  ];
  for (const [label, text, runs] of testCases) add(`test_build/${label}`, { conversation: tests, name: 'test_build', arguments: text, world: made.platform(), runs });
  // Experiments the verifier runs (on the engine, both sides).
  const experiments = {
    probe: ['{"tests":[{"probes":[[0,0,0],[1,0,0]]}]}', made.platform()],
    probeTwo: ['{"tests":[{"probes":[[0,0,0]]},{"probes":[[5,0,5]],"samples":[0,5,10]}]}', made.platform()],
    sandFalls: ['{"tests":[{"probes":[[0,5,0],[0,0,0]],"samples":[0,1,10,40]}]}', made.sand()],
    house: ['{"tests":[{"probes":[[3,1,3],[3,1,2],[1,1,1]],"settle_ticks":5}]}', made.house()],
    emptyWorld: ['{"tests":[{"probes":[[0,0,0]]}]}', made.empty()],
    badProbe: ['{"tests":[{"probes":[[0,0]]}]}', made.platform()],
    unknownArg: ['{"tests":[{"probes":[[0,0,0]],"wat":1}]}', made.platform()],
  };
  for (const [label, [text, world]] of Object.entries(experiments)) add(`test_build/run/${label}`, { conversation: tests, name: 'test_build', arguments: text, world });

  // prompt_library in a chat: read, add, change, refusals.
  const library = id('library');
  const libraryCases = {
    read: '{}', add: '{"save":[{"title":"Generated","text":"A generated prompt","tags":["Gen"," gen ","x"]}]}',
    addNoText: '{"save":[{"title":"No text"}]}', unknownField: '{"save":[{"text":"x","favorite":true}]}', extraKey: '{"save":[],"other":1}',
    saveNotArray: '{"save":{}}', removeNotArray: '{"remove":"x"}', removeUnknown: '{"remove":["nope"]}', changeSeed: '{"save":[{"id":"medieval-house","text":"mine"}]}',
    removeSeed: '{"remove":["cliff-lighthouse"]}',
  };
  for (const [label, text] of Object.entries(libraryCases)) add(`prompt_library/${label}`, { conversation: library, name: 'prompt_library', arguments: text, chat: true });
  return list;
}

module.exports = { calls, PROGRAMS };
