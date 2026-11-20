'use strict';

// V8's Math.sin, cos, tan, asin, atan2 and hypot on see_build's arguments
// (each named view's yaw and pitch in radians, half a field of view) and on
// random ones, each input and answer as the double's hex bits, one line a
// case: `<function> <input>[,<input>...] <answer>`. tests/math.rs reads it.
//
//   node math.js <out.txt>

const fs = require('fs');

const buf = new Float64Array(1);
const bits = new BigUint64Array(buf.buffer);
const hex = (x) => { buf[0] = x; return bits[0].toString(16); };
let seed = 12345n;
const rnd = () => { seed = (seed * 6364136223846793005n + 1442695040888963407n) & ((1n << 64n) - 1n); return Number(seed >> 11n) / 2 ** 53; };
const out = [];
const one = (name, inputs, answer) => out.push(`${name} ${inputs.map(hex).join(',')} ${hex(answer)}`);
// server/shot.js: a view's yaw is -(yaw * PI) / 180, its pitch (pitch * PI) / 180;
// tan((fov / 2) * PI / 180).
for (const degrees of [45, -45, 135, -135, 0, 180, 90, -90, 30]) {
  for (const angle of [-(degrees * Math.PI) / 180, (degrees * Math.PI) / 180]) {
    one('view-sin', [angle], Math.sin(angle));
    one('view-cos', [angle], Math.cos(angle));
  }
}
for (let fov = 5; fov <= 120; fov += 1) one('view-tan', [(fov / 2) * Math.PI / 180], Math.tan((fov / 2) * Math.PI / 180));
for (let i = 0; i < 100000; i++) {
  const angle = (rnd() * 2 - 1) * 4;
  one('sin', [angle], Math.sin(angle));
  one('cos', [angle], Math.cos(angle));
  one('tan', [angle], Math.tan(angle));
  const x = rnd() * 2 - 1;
  one('asin', [x], Math.asin(x));
  const [y, z] = [(rnd() * 2 - 1) * 300, (rnd() * 2 - 1) * 300];
  one('atan2', [y, z], Math.atan2(y, z));
  const [a, b, c] = [(rnd() * 2 - 1) * 300, (rnd() * 2 - 1) * 300, (rnd() * 2 - 1) * 300];
  one('hypot', [a, b, c], Math.hypot(a, b, c));
}
fs.writeFileSync(process.argv[2], `${out.join('\n')}\n`);
