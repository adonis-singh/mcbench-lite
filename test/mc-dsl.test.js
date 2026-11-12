'use strict';

const { test } = require('node:test');
const assert = require('node:assert');

const { runProgram, formatProgramError } = require('../server/mc-dsl');
const { World } = require('../server/world');

const REGISTRY = {
  oak_planks: [{}, {}],
  stone: [{}, {}],
  oak_stairs: [{ facing: ['north', 'south', 'east', 'west'], half: ['top', 'bottom'] }, { facing: 'north', half: 'bottom' }],
  torch: [{}, {}],
  wall_torch: [{ facing: ['north', 'south', 'east', 'west'] }, { facing: 'north' }],
  red_bed: [{ facing: ['north', 'south', 'east', 'west'], part: ['head', 'foot'], occupied: ['true', 'false'] }, { facing: 'north', part: 'foot', occupied: 'false' }],
};

function run(src, world = new World()) {
  return { result: runProgram(src, world, REGISTRY, null), world };
}

test('build places blocks and reports logs', () => {
  const { result, world } = run(
    'module.exports = (mc) => { mc.fill([0,0,0],[3,0,3],"oak_planks"); mc.set([0,1,0],"stone"); mc.log("n", mc.count()); }',
  );
  assert.strictEqual(world.size(), 17);
  assert.deepStrictEqual(result.logs, ['n 17']);
});

// A region narrows mc.find to its box, in the order its chunks
// are walked (x, then y, then z).
test('mc.find reads only the region asked', () => {
  const { result } = run(`module.exports = (mc) => {
    mc.fill([0, 0, 0], [40, 0, 0], "stone");
    mc.set([20, 5, 20], "stone");
    mc.log(JSON.stringify(mc.find("stone", { from: [15, 0, 0], to: [17, 9, 9] })));
    mc.log(mc.find("stone").length, mc.find("stone", { from: [0, 0, 0], to: [40, 9, 30] }).length, mc.find("stone", { from: [16, 0, 0], to: [16, 0, 0] }).length);
  }`);
  assert.deepStrictEqual(result.logs, ['[[15,0,0],[16,0,0],[17,0,0]]', '42 42 1']);
});

test('block state validation rejects unknown states', () => {
  assert.throws(
    () => run('module.exports = (mc) => mc.set([0,0,0], "oak_stairs[facing=up]")'),
    /facing=up is invalid/,
  );
});

test('torches fail fast without the correct support', () => {
  assert.throws(
    () => run('module.exports = (mc) => { mc.set([0,0,0], "stone"); mc.set([0,1,0], "wall_torch[facing=east]"); }'),
    /wall torch at \[0, 1, 0\].*needs a block at \[-1, 1, 0\]/,
  );
  assert.throws(
    () => run('module.exports = (mc) => mc.set([0,1,0], "torch")'),
    /floor torch at \[0, 1, 0\].*needs a block at \[0, 0, 0\]/,
  );
});

test('floor and wall torches accept support in the correct cell', () => {
  const { world } = run(`module.exports = (mc) => {
    mc.set([0,0,0], "stone");
    mc.set([0,1,0], "torch");
    mc.set([2,1,0], "stone");
    mc.set([3,1,0], "wall_torch[facing=east]");
  }`);
  assert.strictEqual(world.size(), 4);
});

test('a throwing program leaves the passed world untouched (caller swaps clones)', () => {
  const world = new World();
  world.set(0, 0, 0, 'minecraft:stone', {});
  assert.throws(() => run('module.exports = (mc) => { mc.set([1,0,0],"stone"); throw new Error("abort"); }', world));
  // runProgram mutates the world it is given; atomicity is the caller cloning
  // first. Here both sets applied before the throw — proving the throw is what
  // the caller keys off, not a rollback inside runProgram.
  assert.strictEqual(world.size(), 2);
});

test('Math.random is deterministic across runs (seeded from source)', () => {
  const src = 'module.exports = (mc) => mc.log(Math.random(), Math.random(), Math.random())';
  const a = run(src).result.logs;
  const b = run(src).result.logs;
  assert.deepStrictEqual(a, b);
});

test('sandbox escape via host function constructor is blocked', () => {
  // The classic node:vm escape: reach the host realm through a host function
  // handed into the context. mc.log must be an in-realm facade.
  const src = 'module.exports = (mc) => { const P = mc.log.constructor("return process")(); mc.log(typeof P); }';
  assert.throws(() => run(src), /process is not defined/);
});

test('returned objects carry only the in-realm constructor', () => {
  const src = 'module.exports = (mc) => { const o = mc.spec("x"); mc.log(String(o.constructor.constructor("return typeof process")())); }';
  assert.deepStrictEqual(run(src).result.logs, ['undefined']);
});

test('async programs are rejected', () => {
  assert.throws(() => run('module.exports = async (mc) => mc.set([0,0,0],"stone")'), /synchronous/);
});

test('replace to the same id terminates and rewrites states past the find cap', () => {
  const world = new World();
  for (let i = 0; i < 600; i += 1) world.set(i, 0, 0, 'minecraft:oak_stairs', { facing: 'north', half: 'bottom' });
  const { result } = run('module.exports = (mc) => mc.log(mc.replace("oak_stairs", "oak_stairs[facing=south]"))', world);
  assert.deepStrictEqual(result.logs, ['600']);
  assert.strictEqual(world.get(599, 0, 0).states.facing, 'south');
  assert.strictEqual(world.get(599, 0, 0).states.half, 'bottom'); // untouched state carries over
});

test('replace honors the region', () => {
  const world = new World();
  world.fill([0, 0, 0], [9, 0, 0], 'minecraft:stone', {});
  const { result } = run('module.exports = (mc) => mc.log(mc.replace("stone", "oak_planks", { from: [0,0,0], to: [4,0,0] }))', world);
  assert.deepStrictEqual(result.logs, ['5']);
  assert.strictEqual(world.get(5, 0, 0).id, 'minecraft:stone');
});

test('bulk replacements preserve checkpoints, carry compatible states, and count no-ops', () => {
  const base = new World();
  base.fill([-17, -1, -2], [18, 3, 2], 'minecraft:oak_stairs', { facing: 'north', half: 'top' });
  base.set(32, 0, 0, 'minecraft:stone');
  base.finalize();
  const candidate = base.clone();
  const result = run(`module.exports = mc => {
    mc.log(mc.replace('oak_stairs', 'oak_stairs[facing=south]', { from: [-1,0,-1], to: [16,2,1] }));
  }`, candidate).result;
  assert.deepStrictEqual(result.logs, ['162']);
  assert.deepStrictEqual(candidate.diff(base), { added: 0, removed: 0, changed: 162 });
  assert.deepStrictEqual(candidate.get(16, 2, 1).states, { facing: 'south', half: 'top' });
  assert.strictEqual(base.get(16, 2, 1).states.facing, 'north');
  assert.strictEqual(candidate.get(17, 2, 1).states.facing, 'north');
  assert.strictEqual(candidate.chunks.get(World.keyOf(2, 0, 0)), base.chunks.get(World.keyOf(2, 0, 0)));

  const noOp = base.clone();
  assert.deepStrictEqual(run('module.exports = mc => mc.log(mc.replace("oak_stairs", "oak_stairs"))', noOp).result.logs, ['900']);
  assert.strictEqual(noOp.changedChunks.size, 0);
  for (const [key, chunk] of base.chunks) assert.strictEqual(noOp.chunks.get(key), chunk);

  run('module.exports = mc => mc.replace("oak_stairs", "air")', candidate);
  assert.strictEqual(candidate.size(), 1);
  assert.deepStrictEqual(candidate.bounds(), { min: [32, 0, 0], max: [32, 0, 0] });
  assert.strictEqual(base.size(), 901);
});

test('bulk replacement still rejects removing a torch support across a chunk boundary', () => {
  const base = new World();
  base.set(15, 0, 0, 'minecraft:stone');
  base.set(16, 0, 0, 'minecraft:wall_torch', { facing: 'east' });
  base.finalize();
  assert.throws(() => run('module.exports = mc => mc.replace("stone", "air")', base.clone()), /Unsupported wall torch/);
  assert.strictEqual(base.size(), 2);
});

test('outline matches edge geometry for volumes, planes, lines, and single blocks', () => {
  for (const dimensions of [[35, 19, 7], [1, 5, 6], [4, 1, 6], [4, 5, 1], [1, 1, 6], [1, 1, 1], [2, 3, 2]]) {
    const min = [-18, -3, -4];
    const max = min.map((v, i) => v + dimensions[i] - 1);
    const world = new World();
    world.fill(min, max, 'minecraft:oak_planks');
    const { result } = run(`module.exports = mc => mc.fill(${JSON.stringify(min)}, ${JSON.stringify(max)}, "stone", "outline")`, world);
    let edges = 0;
    for (let x = min[0]; x <= max[0]; x++) for (let y = min[1]; y <= max[1]; y++) for (let z = min[2]; z <= max[2]; z++) {
      const edge = Number(x === min[0] || x === max[0]) + Number(y === min[1] || y === max[1]) + Number(z === min[2] || z === max[2]) >= 2;
      assert.strictEqual(world.get(x, y, z)?.id || null, edge ? 'minecraft:stone' : null);
      if (edge) edges++;
    }
    assert.strictEqual(world.size(), edges);
    assert.strictEqual(result.touched, 2 * dimensions.reduce((a, b) => a * b) - edges);
  }
});

// How hollow and outline used to run: fill the whole box, then carve the
// interior back out with air.
function fillAndCarve(world, min, max, mode, id, states) {
  world.fill(min, max, id, states);
  if (mode === 'hollow') {
    if (max[0] - min[0] > 1 && max[1] - min[1] > 1 && max[2] - min[2] > 1) {
      world.fill([min[0] + 1, min[1] + 1, min[2] + 1], [max[0] - 1, max[1] - 1, max[2] - 1], 'minecraft:air', {});
    }
    return;
  }
  for (let axis = 0; axis < 3; axis += 1) {
    const from = min.map((value, i) => value + (i === axis ? 0 : 1));
    const to = max.map((value, i) => value - (i === axis ? 0 : 1));
    if (from.some((value, i) => value > to[i])) continue;
    world.fill(from, to, 'minecraft:air', {});
  }
}

test('hollow and outline leave the world block for block as filling the box and carving it did', () => {
  // Blocks with states in and around every box, so erased interiors and kept
  // neighbours both show.
  const base = new World();
  let seed = 7;
  for (let i = 0; i < 4000; i += 1) {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    const p = [seed % 44 - 22, (seed >> 8) % 44 - 22, (seed >> 16) % 44 - 22];
    if (i % 2) base.set(p[0], p[1], p[2], 'minecraft:oak_stairs', { facing: 'south', half: 'bottom' });
    else base.set(p[0], p[1], p[2], 'minecraft:stone', {});
  }
  base.finalize();
  const sizes = [1, 2, 3, 4, 17, 33];
  for (const mode of ['hollow', 'outline']) {
    for (const nx of sizes) for (const ny of sizes) for (const nz of sizes) {
      const min = [-17, -3, -9];
      const max = [min[0] + nx - 1, min[1] + ny - 1, min[2] + nz - 1];
      const expected = base.clone();
      fillAndCarve(expected, min, max, mode, 'minecraft:oak_stairs', { facing: 'east', half: 'top' });
      const { world } = run(`module.exports = mc => mc.fill(${JSON.stringify(min)}, ${JSON.stringify(max)}, "oak_stairs[facing=east,half=top]", "${mode}")`, base.clone());
      const label = `${mode} ${nx}x${ny}x${nz}`;
      assert.strictEqual(world.size(), expected.size(), label);
      for (let x = min[0] - 1; x <= max[0] + 1; x++) for (let y = min[1] - 1; y <= max[1] + 1; y++) for (let z = min[2] - 1; z <= max[2] + 1; z++) {
        const got = world.get(x, y, z);
        const want = expected.get(x, y, z);
        if (got?.id !== want?.id || JSON.stringify(got?.states) !== JSON.stringify(want?.states)) {
          assert.fail(`${label}: [${x}, ${y}, ${z}] is ${JSON.stringify(got)}, expected ${JSON.stringify(want)}`);
        }
      }
    }
  }
});

test('the block limit counts what hollow and outline leave, not the whole box', () => {
  const { world } = run(`module.exports = mc => {
    mc.fill([0, 0, 0], [199, 199, 199], "stone", "hollow");
    mc.fill([300, 0, 0], [599, 299, 299], "stone", "outline");
  }`);
  assert.strictEqual(world.size(), (200 ** 3 - 198 ** 3) + (4 * 300 + 8 * 298));
  // The interior a hollow fill erases makes room for its walls.
  const full = new World();
  full.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
  run('module.exports = mc => mc.fill([0, 0, 0], [299, 199, 59], "oak_planks", "hollow")', full);
  assert.strictEqual(full.get(0, 0, 59).id, 'minecraft:oak_planks');
  assert.strictEqual(full.get(1, 1, 1), null);
});

test('a hollow fill the block limit refuses says how many blocks it would add and writes nothing', () => {
  const world = new World();
  world.fill([0, 0, 0], [299, 199, 49], 'minecraft:stone', {});
  world.fill([0, 0, 0], [0, 0, 1], 'minecraft:air', {});
  world.set(0, 0, 100, 'minecraft:stone', {});
  world.finalize();
  assert.throws(
    () => run('module.exports = mc => mc.fill([0, 0, 99], [2, 2, 101], "oak_planks", "hollow")', world),
    /Build exceeds the 3000000 block limit: the world has 2999999 blocks and this call would add 25$/,
  );
  assert.strictEqual(world.size(), 2999999);
  assert.strictEqual(world.get(0, 0, 100).id, 'minecraft:stone');
  assert.strictEqual(world.get(0, 0, 99), null);
});

test('find throws past the result cap instead of silently truncating', () => {
  const world = new World();
  world.fill([0, 0, 0], [600, 0, 0], 'minecraft:stone', {});
  assert.throws(
    () => run('module.exports = (mc) => mc.find("stone")', world),
    /more than 500 "stone" matches/,
  );
});

test('failed programs carry their logs and a build.js stack frame', () => {
  const src = `module.exports = (mc) => {
  mc.log('phase 1 done');
  const parts = [{ h: 2 }];
  mc.set([0, parts[1].h, 0], 'stone');
};`;
  try {
    run(src);
    assert.fail('expected the program to throw');
  } catch (err) {
    assert.deepStrictEqual(err.logs, ['phase 1 done']);
    const formatted = formatProgramError(err);
    assert.match(formatted, /Cannot read properties of undefined/);
    assert.match(formatted, /build\.js:4/);
    assert.doesNotMatch(formatted, /node:vm|bootstrap\.js/);
  }
});

test('a bed needs its other half one block toward its facing', () => {
  const placed = run(`module.exports = (mc) => {
    mc.set([0,0,0], "red_bed[facing=south,part=foot]");
    mc.set([0,0,1], "red_bed[facing=south,part=head]");
  }`).world;
  assert.equal(placed.get(0, 0, 1).states.part, 'head');
  // The head on the wrong side of the foot: what a model built facing south.
  assert.throws(
    () => run('module.exports = (mc) => { mc.set([0,0,-2], "red_bed[facing=south,part=head]"); mc.set([0,0,-1], "red_bed[facing=south,part=foot]"); }'),
    /Incomplete bed at \[0, 0, -2\]: red_bed\[part=head,facing=south\] needs its foot at \[0, 0, -3\]/,
  );
  assert.throws(() => run('module.exports = (mc) => { mc.set([0,0,0], "red_bed[facing=east,part=foot]"); }'), /needs its head at \[1, 0, 0\]/);
  // Across a chunk border.
  run(`module.exports = (mc) => { mc.set([15,0,0], "red_bed[facing=east,part=foot]"); mc.set([16,0,0], "red_bed[facing=east,part=head]"); }`);
});

test('mc.settled reads the settled world at mc.get coordinates, offsets included', () => {
  const settled = new World();
  settled.set(2, 3, 4, 'minecraft:oak_stairs', { facing: 'north', half: 'bottom' });
  let loads = 0;
  const world = new World();
  const { result } = { result: runProgram(`module.exports = (mc) => {
    mc.log(mc.settled(2, 3, 4), mc.settled(0, 0, 0));
    mc.translate(2, 0, 0, () => mc.log(mc.settled(0, 3, 4)));
    const block = mc.settled(2, 3, 4);
    block.states.facing = 'south';
    mc.log(mc.settled(2, 3, 4).states.facing, mc.settledTicks);
  }`, world, REGISTRY, null, { world: () => { loads += 1; return settled; }, ticks: 50 }) };
  assert.deepStrictEqual(result.logs, ['{"id":"minecraft:oak_stairs","states":{"facing":"north","half":"bottom"}} null', '{"id":"minecraft:oak_stairs","states":{"facing":"north","half":"bottom"}}', 'north 50']);
  assert.strictEqual(loads, 5);
  assert.throws(() => run('module.exports = (mc) => mc.settled(0, 0, 0)'), /no Minecraft settle for this program/);
  assert.throws(() => runProgram('module.exports = (mc) => mc.settled([0, 0, 0])', world, REGISTRY, null, { world: () => settled, ticks: 1 }), /mc\.settled takes three integers/);
  assert.throws(() => runProgram('module.exports = (mc) => mc.settled(0, 0, 5000)', world, REGISTRY, null, { world: () => settled, ticks: 1 }), /out of range/);
});
