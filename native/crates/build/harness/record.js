'use strict';

// Preload (node --require record.js some.test.js): every call the process
// makes to runProgram of server/mc-dsl.js or server/build-sandbox.js is
// written to the corpus as a case: the source, the registry, the starting
// world (build-transfer.js's encodeWorld), the readFile answers the test's
// host gave and the world mc.settled read, and the name of the node:test
// test that made the call (`test`), so the Rust ports of a test file find
// their runs. REC_OUT names the corpus folder, REC_PREFIX the case id prefix.
// From the JavaScript spike (native/spikes/js/harness/record.js).

const Module = require('module');
const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const OUT = process.env.REC_OUT;
const PREFIX = process.env.REC_PREFIX || 'rec';
let counter = 0;
const wrapped = new WeakSet();

function writeCase({ source, registry, world, readFile, settled }) {
  const { encodeWorld } = require(path.resolve(__dirname, '../../../../server/build-transfer.js'));
  const id = `${PREFIX}-${String(counter += 1).padStart(4, '0')}`;
  const dir = path.join(OUT, 'cases', id);
  fs.mkdirSync(dir, { recursive: true });
  fs.mkdirSync(path.join(OUT, 'registries'), { recursive: true });
  const registryText = JSON.stringify(registry);
  const registryName = `${crypto.createHash('sha256').update(registryText).digest('hex').slice(0, 16)}.json`;
  const registryFile = path.join(OUT, 'registries', registryName);
  if (!fs.existsSync(registryFile)) fs.writeFileSync(registryFile, registryText);
  fs.writeFileSync(path.join(dir, 'source.js'), source);
  fs.writeFileSync(path.join(dir, 'world.bin'), encodeWorld(world));
  const answers = readFile ? {} : null;
  const meta = { id, origin: PREFIX, test: currentTest, registry: registryName, readFile: answers, settled: null };
  const save = () => fs.writeFileSync(path.join(dir, 'case.json'), JSON.stringify(meta));
  save();
  return {
    readFile: readFile && ((relPath) => {
      try {
        const text = readFile(relPath);
        answers[relPath] = { ok: true, text };
        save();
        return text;
      } catch (error) {
        answers[relPath] = { ok: false, text: error.message };
        save();
        throw error;
      }
    }),
    // The ticks alone: mc.settledTicks is readable without mc.settled's world,
    // which the harness then gives as an empty world (never read).
    ticks: (ticks) => { meta.settled = { ticks, world: null }; save(); },
    settled: (settledWorld, ticks) => {
      if (meta.settled && meta.settled.world) return;
      fs.writeFileSync(path.join(dir, 'settled.bin'), encodeWorld(settledWorld));
      meta.settled = { ticks, world: 'settled.bin' };
      save();
    },
  };
}

// The test running now: node:test runs a file's tests one after another.
let currentTest = null;
const load = Module._load;
Module._load = function (request, parent, isMain) {
  const exports = load.apply(this, arguments);
  if (request === 'node:test' && !wrapped.has(exports)) {
    wrapped.add(exports);
    const real = exports.test;
    const named = (name, ...rest) => real(name, ...rest.map((arg) => (typeof arg === 'function'
      ? function (...args) { currentTest = name; return arg.apply(this, args); }
      : arg)));
    return Object.assign(Object.create(exports), { test: named });
  }
  const file = (() => { try { return Module._resolveFilename(request, parent, isMain); } catch { return ''; } })();
  if (!exports || wrapped.has(exports)) return exports;
  if (file.endsWith('/server/mc-dsl.js')) {
    wrapped.add(exports);
    const real = exports.runProgram;
    exports.runProgram = function (source, world, registry, readFile, settled) {
      const rec = writeCase({ source, registry, world, readFile });
      if (settled) rec.ticks(settled.ticks);
      const recSettled = settled && { ticks: settled.ticks, world: () => { const w = settled.world(); rec.settled(w, settled.ticks); return w; } };
      return real(source, world, registry, rec.readFile || readFile, recSettled || settled);
    };
  } else if (file.endsWith('/server/build-sandbox.js')) {
    wrapped.add(exports);
    const real = exports.runProgram;
    exports.runProgram = function (source, world, registry, readFile, minecraft) {
      const rec = writeCase({ source, registry, world, readFile });
      if (minecraft) rec.settled(minecraft.world, minecraft.ticks);
      return real(source, world, registry, rec.readFile || readFile, minecraft);
    };
  }
  return exports;
};
