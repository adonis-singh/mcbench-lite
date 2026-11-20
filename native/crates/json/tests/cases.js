#!/usr/bin/env node
'use strict';
// Differential cases for mcbench-json: inputs and what V8 makes of them, one
// JSON object per line. The Rust side (tests/differential.rs) reads them and
// requires every output identical.
//
//   node cases.js --count <n> --seed <s> --out <file>   n cases of every kind, mixed
//   node cases.js --golden --out <file>                  the small fixed set committed as tests/golden.jsonl
//
// Kinds (field k):
//   num    i: the double's bits as 16 hex digits; o: String(x); j: JSON.stringify(x)
//   int    i: a 64-bit integer in decimal (u or s); o: JSON.stringify(Number(BigInt(i)))
//   str    i: the string's code points; o: JSON.stringify(s)
//   val    i: the value as a tree (below); o: JSON.stringify(v); p: [indent, JSON.stringify(v, null, indent)]
//   parse  i: JSON text; o: JSON.stringify(JSON.parse(i))
//   lone   i: JSON text holding a lone surrogate; o: JSON.stringify(JSON.parse(i)) (Rust refuses it)
//
// A value tree: null, true, false, a string (valid Unicode), an array of trees,
// {"n": "<bits>"} a double, {"i": "<decimal>"} a 64-bit integer, {"o": [[key, tree], ...]}
// an object built by assigning the keys in that order (a key may repeat).

const fs = require('node:fs');

function args() {
  const out = { count: 0, seed: 1, out: null, golden: false };
  const argv = process.argv.slice(2);
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--count') out.count = Number(argv[++i]);
    else if (argv[i] === '--seed') out.seed = Number(argv[++i]);
    else if (argv[i] === '--out') out.out = argv[++i];
    else if (argv[i] === '--golden') out.golden = true;
    else throw new Error(`unknown argument ${argv[i]}`);
  }
  if (!out.out) throw new Error('--out <file> is required');
  if (!out.golden && !(Number.isSafeInteger(out.count) && out.count > 0)) throw new Error('--count <n> is required');
  return out;
}

// sfc32, seeded.
function rng(seed) {
  let a = 0x9e3779b9, b = 0x243f6a88, c = 0xb7e15162, d = seed >>> 0;
  const next = () => {
    a >>>= 0; b >>>= 0; c >>>= 0; d >>>= 0;
    const t = (a + b | 0) + d | 0;
    d = d + 1 | 0;
    a = b ^ b >>> 9;
    b = c + (c << 3) | 0;
    c = c << 21 | c >>> 11;
    c = c + t | 0;
    return t >>> 0;
  };
  for (let i = 0; i < 16; i += 1) next();
  const int = n => Math.floor(next() / 4294967296 * n);
  return { u32: next, int, pick: list => list[int(list.length)], chance: p => next() / 4294967296 < p };
}

const view = new DataView(new ArrayBuffer(8));
const bitsOf = x => { view.setFloat64(0, x); return view.getBigUint64(0).toString(16).padStart(16, '0'); };
const fromBits = (hi, lo) => { view.setUint32(0, hi); view.setUint32(4, lo); return view.getFloat64(0); };
const step = (x, k) => { view.setFloat64(0, x); view.setBigUint64(0, BigInt.asUintN(64, view.getBigUint64(0) + BigInt(k))); return view.getFloat64(0); };

function randomDouble(r) {
  switch (r.int(10)) {
    case 0: return fromBits(r.u32(), r.u32()); // any bits: every exponent, NaN, infinities
    case 1: return fromBits((r.u32() & 0x800fffff) >>> 0, r.u32()); // subnormals
    case 2: { const e = 1023 + r.int(140) - 70; return fromBits(((r.u32() & 0x800fffff) | (e << 20)) >>> 0, r.u32()); } // near 1
    case 3: return step(2 ** 53, r.int(64) - 32) * (r.chance(0.5) ? -1 : 1); // integers around 2^53
    case 4: return step(r.pick([1e21, 1e-6, 1e-7, 1e20, 1e22, 1e-5, 9.999999999999999e20, 1e15, 1e16, 1e17]), r.int(64) - 32);
    case 5: return Number(`${r.int(1e9)}e${r.int(660) - 340}`); // short decimals at every scale
    case 6: return r.int(2 ** 31) / 10 ** r.int(12); // money-like and tick-like quotients
    case 7: return (r.u32() * 4294967296 + r.u32()) * (r.chance(0.5) ? -1 : 1); // large integers
    case 8: return r.pick([0, -0, 1, -1, 0.1, 0.2, 0.3, 1 / 3, 2 / 3, Math.PI, Math.E, 5e-324, -5e-324, Number.MAX_VALUE, Number.MIN_VALUE, Number.EPSILON, 2 ** 53, 2 ** 53 + 2, 123456789012345680000, 1e21, 1e-7, NaN, Infinity, -Infinity, 2147483647 / 1048576, 900000 / 1000]);
    default: return r.int(4) === 0 ? r.int(2 ** 32) : (r.u32() / (1 + r.u32())) * 10 ** (r.int(50) - 25);
  }
}

function randomInt(r) {
  const hi = BigInt(r.u32()), lo = BigInt(r.u32());
  let n = (hi << 32n) | lo;
  switch (r.int(4)) {
    case 0: n = 2n ** 53n + BigInt(r.int(2000) - 1000); break;
    case 1: n >>= BigInt(r.int(64)); break;
    default: break;
  }
  if (r.chance(0.5)) {
    n = BigInt.asIntN(64, n);
    return { i: `${n}`, o: JSON.stringify(Number(n)) };
  }
  return { i: `${n}`, o: JSON.stringify(Number(n)) };
}

function randomCodePoint(r) {
  switch (r.int(10)) {
    case 0: return r.int(0x20); // controls
    case 1: return r.pick([0x22, 0x5c, 0x2f, 0x7f, 0x2028, 0x2029, 0x85, 0xa0, 0xfeff, 0xfffe, 0xffff, 0x8, 0xc, 0xa, 0xd, 0x9, 0x0, 0x1f]);
    case 2: case 3: case 4: return 0x20 + r.int(0x5f); // printable ASCII
    case 5: case 6: { const c = r.int(0x10000); return c >= 0xd800 && c < 0xe000 ? 0x41 : c; } // the BMP
    default: { const c = r.int(0x110000); return c >= 0xd800 && c < 0xe000 ? 0x10000 + c : c; } // all of Unicode
  }
}

const randomString = (r, max = 24) => Array.from({ length: r.int(max) }, () => randomCodePoint(r));
const fromCodePoints = points => String.fromCodePoint(...points);

function randomKey(r) {
  switch (r.int(6)) {
    case 0: return `${r.int(20)}`;
    case 1: return `${r.pick([4294967294, 4294967295, 4294967296, 2147483647, 9007199254740991, r.u32()])}`;
    case 2: return r.pick(['01', '-1', '1.5', '1e3', ' 1', '1 ', '+1', '00', '-0', '0x1', '١']);
    default: return fromCodePoints(randomString(r, 8));
  }
}

// A tree and the JavaScript value it stands for.
function randomTree(r, depth) {
  const leaf = depth <= 0 || r.chance(0.35);
  if (leaf) {
    switch (r.int(7)) {
      case 0: return [null, null];
      case 1: { const b = r.chance(0.5); return [b, b]; }
      case 2: case 3: { const s = fromCodePoints(randomString(r)); return [s, s]; }
      case 4: { const x = randomDouble(r); return Number.isFinite(x) ? [{ n: bitsOf(x) }, x] : [null, null]; }
      case 5: { const { i } = randomInt(r); return [{ i }, Number(BigInt(i))]; }
      default: { const x = r.int(100000) - 50000; return [{ n: bitsOf(x) }, x]; }
    }
  }
  if (r.chance(0.45)) {
    const items = Array.from({ length: r.int(6) }, () => randomTree(r, depth - 1));
    return [items.map(([t]) => t), items.map(([, v]) => v)];
  }
  const entries = [];
  const object = {};
  const keys = [];
  for (let n = r.int(8); n > 0; n -= 1) {
    const key = keys.length && r.chance(0.15) ? r.pick(keys) : randomKey(r);
    keys.push(key);
    const [t, v] = randomTree(r, depth - 1);
    entries.push([key, t]);
    object[key] = v;
  }
  return [{ o: entries }, object];
}

function deepArray(r) {
  const x = randomDouble(r);
  let [tree, value] = Number.isFinite(x) && r.chance(0.5) ? [{ n: bitsOf(x) }, x] : [[], []];
  for (let d = 1 + r.int(100); d > 0; d -= 1) { tree = [tree]; value = [value]; }
  return [tree, value];
}

// JSON text JavaScript did not write: whitespace, other number forms,
// duplicate keys, escapes JSON.stringify does not use.
function handText(r) {
  const ws = () => r.pick(['', ' ', '\n', '\t', '\r\n  ', '']);
  const num = () => r.pick(['1.0', '1E5', '1e+5', '-0', '-0.0', '0.1e1', '123456789012345678901234567890', '1e-400', '12.50', '9007199254740993', '18446744073709551615', '-9223372036854775809', '2.2250738585072014e-308', '4.9e-324', `${r.int(1e6)}.${r.int(1e6)}e${r.int(40) - 20}`]);
  const str = () => r.pick(['"\\u0041"', '"\\/"', '"a\\u00e9b"', '"\\ud83d\\ude00"', '"\\u2028"', '"\\u007f"', `${JSON.stringify(fromCodePoints(randomString(r, 6)))}`]);
  const value = depth => {
    const k = depth <= 0 ? r.int(3) : r.int(5);
    if (k === 0) return num();
    if (k === 1) return str();
    if (k === 2) return r.pick(['true', 'false', 'null']);
    if (k === 3) return `[${ws()}${Array.from({ length: r.int(4) }, () => value(depth - 1)).join(`${ws()},${ws()}`)}${ws()}]`;
    const keys = Array.from({ length: 1 + r.int(5) }, () => r.pick(['"a"', '"b"', '"0"', '"1"', '"10"', '"x"', '"\\u0061"']));
    return `{${ws()}${keys.map(key => `${key}${ws()}:${ws()}${value(depth - 1)}`).join(`${ws()},${ws()}`)}${ws()}}`;
  };
  return `${ws()}${value(3)}${ws()}`;
}

function loneText(r) {
  const lone = r.chance(0.5) ? 0xd800 + r.int(0x400) : 0xdc00 + r.int(0x400);
  const before = fromCodePoints(randomString(r, 4)), after = fromCodePoints(randomString(r, 4));
  const s = before + String.fromCharCode(lone) + (r.chance(0.3) ? String.fromCharCode(lone) : '') + after;
  return JSON.stringify(r.chance(0.5) ? s : { [s]: s });
}

function cases(r, count, write) {
  const kinds = [
    ['num', 50], ['int', 5], ['str', 20], ['val', 15], ['parse', 9], ['lone', 1],
  ];
  const total = kinds.reduce((sum, [, w]) => sum + w, 0);
  for (let n = 0; n < count; n += 1) {
    let pick = r.int(total), kind;
    for (const [name, w] of kinds) { if (pick < w) { kind = name; break; } pick -= w; }
    if (kind === 'num') { const x = randomDouble(r); write({ k: 'num', i: bitsOf(x), o: String(x), j: JSON.stringify(x) }); }
    else if (kind === 'int') write({ k: 'int', ...randomInt(r) });
    else if (kind === 'str') { const points = randomString(r, 40); write({ k: 'str', i: points, o: JSON.stringify(fromCodePoints(points)) }); }
    else if (kind === 'val') {
      const [tree, value] = r.chance(0.05) ? deepArray(r) : randomTree(r, 1 + r.int(6));
      const indent = r.int(13);
      write({ k: 'val', i: tree, o: JSON.stringify(value), p: [indent, JSON.stringify(value, null, indent)] });
    } else if (kind === 'parse') {
      let text;
      if (r.chance(0.5)) text = handText(r);
      else { const [, value] = randomTree(r, 1 + r.int(5)); text = r.chance(0.5) ? JSON.stringify(value) : JSON.stringify(value, null, r.int(11)); }
      write({ k: 'parse', i: text, o: JSON.stringify(JSON.parse(text)) });
    } else { const text = loneText(r); write({ k: 'lone', i: text, o: JSON.stringify(JSON.parse(text)) }); }
  }
}

// The golden set: fixed edge cases, then a small random sample.
function golden(write) {
  for (const x of [0, -0, 1, -1, 2, 0.1, 0.5, 1.5, 100, 1e21, 1e-7, 1e-6, 123456789012345680000, 5e-324, Number.MAX_VALUE, Number.MIN_VALUE, 2 ** 53, 2 ** 53 + 2, -(2 ** 63), 1e300, 1.7976931348623157e308, 2147483647 / 1048576, 0.000001, 0.0000001, 1.2345e-7, 1e20, 1e22, 9.999999999999999e20, NaN, Infinity, -Infinity]) {
    write({ k: 'num', i: bitsOf(x), o: String(x), j: JSON.stringify(x) });
  }
  for (const i of ['0', '9007199254740993', '18446744073709551615', '-9223372036854775808', '12345678901234567', '-1']) write({ k: 'int', i, o: JSON.stringify(Number(BigInt(i))) });
  for (const s of ['', 'a', '"', '\\', '/', '\b\f\n\r\t', '\u0000\u0001\u001f\u007f', '  ', 'é😀', '﻿￿']) {
    const points = Array.from(s, c => c.codePointAt(0));
    write({ k: 'str', i: points, o: JSON.stringify(s) });
  }
  const r = rng(20261117);
  cases(r, 260, write);
}

function main() {
  const { count, seed, out, golden: isGolden } = args();
  const fd = fs.openSync(out, 'w');
  let lines = [];
  const flush = () => { fs.writeSync(fd, lines.join('')); lines = []; };
  const write = row => { lines.push(`${JSON.stringify(row)}\n`); if (lines.length >= 4096) flush(); };
  if (isGolden) golden(write);
  else cases(rng(seed), count, write);
  flush();
  fs.closeSync(fd);
}

main();
