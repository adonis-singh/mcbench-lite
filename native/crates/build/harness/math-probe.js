'use strict';

// The Math investigation: every result of adv-math-bits's functions over
// its own inputs, as bit patterns, so two platforms' files can be compared
// input by input.
//
//   node math-probe.js <out-dir>                  (Node: writes the files)
//   examples/run_program.rs <mcbench-build-child> math-probe.js <out-dir>
//                                                 (the V8 child: the same
//                                                  function as a build program,
//                                                  writing with mc.writeFile)
//
// Files: inputs.txt (each input's bits), fns.txt (for each function a
// '#<fn>.txt' line, then each result's bits, one line per input), pow.txt
// (one line per input and exponent, in that order), pow-cold.txt (the first
// 2,000 pow pairs, each through a fresh function run once, which the
// interpreter runs), info.txt (Node only).

function probe(write) {
  const view = new DataView(new ArrayBuffer(8));
  const bits = (x) => { view.setFloat64(0, x); return view.getUint32(0).toString(16).padStart(8, '0') + view.getUint32(4).toString(16).padStart(8, '0'); };
  const inputs = [];
  for (let i = -2000; i <= 2000; i += 1) inputs.push(i * 0.0137, i * 1.7, i / 7, Math.PI * i / 64, 1e-8 * i, i * 1e5 + 0.5);
  const files = {};
  files['inputs.txt'] = inputs.map(bits).join('\n');
  const fns = ['sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'sinh', 'cosh', 'tanh', 'asinh', 'acosh', 'atanh', 'exp', 'expm1', 'log', 'log1p', 'log2', 'log10', 'sqrt', 'cbrt', 'fround'];
  for (const name of fns) files[`${name}.txt`] = inputs.map((x) => bits(Math[name](x))).join('\n');
  const ys = [0.5, 2, 3, -1.5, 1 / 3, 10, 0.1];
  const pow = [];
  for (const x of inputs) for (const y of ys) pow.push(bits(Math.pow(Math.abs(x), y)));
  files['pow.txt'] = pow.join('\n');
  files['atan2.txt'] = inputs.map((x) => bits(Math.atan2(x, 1.3 - x))).join('\n');
  files['hypot.txt'] = inputs.map((x) => bits(Math.hypot(x, 2 * x + 1, 3))).join('\n');
  files['starstar.txt'] = inputs.map((x) => bits(x ** 1.5)).join('\n');
  const cold = [];
  let n = 0;
  for (const x of inputs) {
    for (const y of ys) {
      if (n >= 2000) break;
      cold.push(bits(new Function('x', 'y', 'return Math.pow(x, y)')(Math.abs(x), y)));
      n += 1;
    }
    if (n >= 2000) break;
  }
  files['pow-cold.txt'] = cold.join('\n');
  // Four files, within mc.writeFile's limits: the functions of one input
  // each go into fns.txt under a '#name' line.
  const fnsText = Object.entries(files).filter(([name]) => !name.startsWith('pow') && name !== 'inputs.txt').map(([name, text]) => `#${name}\n${text}`).join('\n');
  write('inputs.txt', files['inputs.txt']);
  write('fns.txt', fnsText);
  write('pow.txt', files['pow.txt']);
  write('pow-cold.txt', files['pow-cold.txt']);
}

if (typeof require === 'function' && require.main === module) {
  const fs = require('fs');
  const path = require('path');
  const out = process.argv[2];
  fs.mkdirSync(out, { recursive: true });
  probe((name, text) => fs.writeFileSync(path.join(out, name), text));
  fs.writeFileSync(path.join(out, 'info.txt'), `${process.version} ${process.versions.v8} ${process.platform} ${process.arch}\n`);
} else {
  module.exports = (mc) => probe((name, text) => mc.writeFile(name, text));
}
