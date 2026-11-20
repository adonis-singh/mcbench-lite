'use strict';

// node math-compare.js <dir A> <dir B>
// For two math-probe.js outputs: whether the inputs are the same bits, and
// per function how many results differ, how many of those are NaNs on both
// sides (differing only in the NaN's sign or payload), and the first
// difference that is not a NaN: the input and both results as bits.

const fs = require('fs');
const path = require('path');

const [a, b] = process.argv.slice(2);
const read = (dir) => {
  const out = { inputs: fs.readFileSync(path.join(dir, 'inputs.txt'), 'utf8').split('\n') };
  let name = null;
  for (const line of fs.readFileSync(path.join(dir, 'fns.txt'), 'utf8').split('\n')) {
    if (line.startsWith('#')) { name = line.slice(1).replace('.txt', ''); out[name] = []; } else out[name].push(line);
  }
  out.pow = fs.readFileSync(path.join(dir, 'pow.txt'), 'utf8').split('\n');
  out['pow-cold'] = fs.readFileSync(path.join(dir, 'pow-cold.txt'), 'utf8').split('\n');
  return out;
};
const A = read(a);
const B = read(b);
const isNaN = (hex) => { const hi = parseInt(hex.slice(0, 8), 16); const lo = parseInt(hex.slice(8), 16); return ((hi >>> 20) & 0x7ff) === 0x7ff && ((hi & 0xfffff) !== 0 || lo !== 0); };
const ys = [0.5, 2, 3, -1.5, 1 / 3, 10, 0.1];
console.log(`inputs identical: ${A.inputs.join() === B.inputs.join()} (${A.inputs.length})`);
console.log('| function | results | differ | NaN on both sides | first non-NaN difference: input bits | A | B |');
console.log('|---|---:|---:|---:|---|---|---|');
for (const name of Object.keys(A).filter((k) => k !== 'inputs')) {
  const x = A[name]; const y = B[name];
  let differ = 0; let nans = 0; let first = null;
  for (let i = 0; i < x.length; i += 1) {
    if (x[i] === y[i]) continue;
    differ += 1;
    if (isNaN(x[i]) && isNaN(y[i])) { nans += 1; continue; }
    if (!first) {
      const pow = name.startsWith('pow');
      const input = pow ? `|x| of ${A.inputs[Math.floor(i / 7)]}, y = ${ys[i % 7]}` : A.inputs[i];
      first = `${input} | ${x[i]} | ${y[i]}`;
    }
  }
  console.log(`| ${name} | ${x.length} | ${differ} | ${nans} | ${first || '-'} |`);
}
