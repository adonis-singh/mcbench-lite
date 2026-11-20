'use strict';

// node adversarial.js <blocks.json> <corpus> [--big]
// Writes the adversarial and fidelity-probe cases (adv-*) and the
// scripts/benchmark-build.js workloads (bench-*). --big adds the cases that
// allocate gigabytes (meant for adipc, where RLIMIT_DATA caps the child).

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const W = path.resolve(__dirname, '../../../..');
const { World } = require(path.join(W, 'server/world.js'));
const { encodeWorld } = require(path.join(W, 'server/build-transfer.js'));

const [blocksFile, corpus, flag] = process.argv.slice(2);
const registryText = fs.readFileSync(blocksFile, 'utf8');
const registryName = `${crypto.createHash('sha256').update(registryText).digest('hex').slice(0, 16)}.json`;
fs.mkdirSync(path.join(corpus, 'registries'), { recursive: true });
fs.writeFileSync(path.join(corpus, 'registries', registryName), registryText);

function write(id, source, { world = new World(), readFile = null, settled = null } = {}) {
  const dir = path.join(corpus, 'cases', id);
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'source.js'), source);
  fs.writeFileSync(path.join(dir, 'world.bin'), encodeWorld(world));
  fs.writeFileSync(path.join(dir, 'case.json'), JSON.stringify({ id, origin: id.split('-')[0], registry: registryName, readFile, settled }));
}

const body = (code) => `module.exports = function (mc) {\n${code}\n};\n`;
const probe = (code) => body(`  const out = [];\n  const p = (label, f) => { try { out.push(label + '=' + String(f())); } catch (e) { out.push(label + '!' + (e && e.message)); } };\n${code}\n  for (const line of out) mc.log(line);`);

const cases = {
  // Limits.
  'adv-loop': body('  while (true) {}'),
  'adv-loop-catch': body('  for (;;) { try { while (true) {} } catch (e) { mc.log("caught", String(e)); } }'),
  'adv-loop-finally': body('  try { while (true) {} } finally { mc.set([0, 0, 0], "stone"); }'),
  'adv-regex-backtrack': body('  mc.log(/(a+)+$/.test("a".repeat(34) + "b"));'),
  'adv-recursion': body('  function f(n) { return f(n + 1) + 1; }\n  f(0);'),
  'adv-recursion-depth': body('  let d = 0;\n  function f() { d += 1; f(); }\n  try { f(); } catch (e) { mc.log(String(e)); }\n  mc.log(d > 5000);'),
  'adv-recursion-depth-exact': body('  let d = 0;\n  function f() { d += 1; f(); }\n  try { f(); } catch (e) {}\n  mc.log(d);'),
  'adv-recursion-big-frames': body('  let d = 0;\n  function f(a, b, c, e, g, h, i, j) { const x = [a, b, c, e, g, h, i, j]; d += 1; return f(...x) + x.length; }\n  try { f(1, 2, 3, 4, 5, 6, 7, 8); } catch (e) { mc.log(String(e), d > 1000); }'),
  'adv-string-bomb': body('  let s = "x";\n  for (;;) s += s;'),
  'adv-string-bomb-caught': body('  let s = "x"; let n = 0;\n  try { for (;;) { s += s; n += 1; } } catch (e) { mc.log(String(e), n); }'),
  'adv-array-length': body('  new Array(-1);'),
  'adv-arraybuffer-huge': body('  try { new ArrayBuffer(2 ** 53); } catch (e) { mc.log(String(e)); }\n  try { new Uint8Array(2 ** 40); } catch (e) { mc.log(String(e)); }'),
  'adv-typed-2g-lazy': body('  const a = new Uint8Array(2 ** 31); mc.log(a.length, a[12345]);'),
  'adv-heap-300mb': body('  const keep = []; for (let i = 0; i < 300; i += 1) keep.push(new Array(131072).fill(i)); mc.log(keep.length, keep[299][5]);'),
  'adv-log-huge': body('  const big = "y".repeat(1 << 20);\n  for (let i = 0; i < 10000; i += 1) mc.log(i, big);'),
  'adv-console-log': body('  console.log("via console", { a: 1 }, [1, 2]);'),
  'adv-files-limit': body('  for (let i = 0; i < 17; i += 1) mc.writeFile("f" + i + ".txt", "x");'),
  'adv-files-total': body('  const big = "z".repeat(16 * 1024 * 1024);\n  mc.writeFile("a.txt", big); mc.writeFile("b.txt", big); mc.writeFile("c.txt", "1");'),
  'adv-files-ok': body('  mc.writeFile("data/out.json", JSON.stringify({ a: [1, 2, 3], s: "\\u00e9\\ud83d\\ude00" }));\n  mc.set([0, 0, 0], "stone");\n  mc.dump("dump.json");'),
  'adv-lone-surrogate': body('  mc.writeFile("s.txt", "a\\ud800b"); mc.log("a\\udc00b"); mc.set([0, 0, 0], "stone");'),
  'adv-blocks-limit': body('  mc.fill([0, 0, 0], [199, 99, 199], "stone");\n  mc.fill([0, 100, 0], [10, 100, 10], "stone");'),
  // Thrown values.
  'adv-throw-number': body('  throw 42;'),
  'adv-throw-null': body('  throw null;'),
  'adv-throw-undefined': body('  throw undefined;'),
  'adv-throw-object': body('  throw { message: "plain object" };'),
  'adv-throw-tostring': body('  throw { toString() { return "custom toString"; } };'),
  'adv-throw-symbol': body('  throw Symbol("s");'),
  'adv-throw-stack-getter': body('  const e = new Error("with getter"); Object.defineProperty(e, "stack", { get() { throw new Error("stack getter"); } }); throw e;'),
  'adv-throw-stack-number': body('  const e = new Error("stack is number"); e.stack = 7; throw e;'),
  'adv-throw-custom-class': body('  class MyError extends Error { constructor(m) { super(m); this.name = "MyError"; } }\n  throw new MyError("custom");'),
  'adv-throw-aggregate': body('  throw new AggregateError([new Error("a"), new Error("b")], "many");'),
  'adv-throw-error-cause': body('  throw new Error("outer", { cause: new Error("inner") });'),
  'adv-throw-after-logs': body('  mc.log("one"); mc.log({ two: 2 }); mc.set([0, 0, 0], "stone"); throw new Error("late");'),
  'adv-throw-nested-15': body('  function a(n) { if (n === 0) throw new Error("deep"); return a(n - 1); }\n  a(15);'),
  'adv-throw-in-callback': body('  [1, 2, 3].map((x) => { if (x === 2) throw new TypeError("in map " + x); return x; });'),
  'adv-throw-from-translate': body('  mc.translate(1, 2, 3, () => { mc.set([0, 0, 0], "not_a_block"); });'),
  'adv-mc-error-shape': body('  try { mc.set([0, 0, 0], "stone[foo=bar]"); } catch (e) { mc.log(e instanceof Error, e.constructor === Error, Object.keys(e).join(","), e.message); }'),
  // Async, promises, timers.
  'adv-async-export': 'module.exports = async function (mc) { mc.set([0, 0, 0], "stone"); };\n',
  'adv-promise-pending': body('  Promise.resolve().then(() => mc.set([1, 0, 0], "stone"));\n  mc.set([0, 0, 0], "stone");'),
  'adv-async-inner': body('  (async () => { await null; mc.set([2, 0, 0], "stone"); })();\n  mc.set([0, 0, 0], "stone");'),
  'adv-thenable-return': 'module.exports = function (mc) { mc.set([0, 0, 0], "stone"); return { then() {} }; };\n',
  'adv-timers': probe('  p("setTimeout", () => typeof setTimeout);\n  p("setInterval", () => typeof setInterval);\n  p("queueMicrotask", () => typeof queueMicrotask);\n  p("setImmediate", () => typeof setImmediate);\n  p("call", () => setTimeout(() => {}, 1));'),
  'adv-top-await': 'await 1;\nmodule.exports = function (mc) {};\n',
  'adv-dynamic-import': body('  const pr = import("fs"); mc.log(typeof pr.then);'),
  // Reaching the host.
  'adv-globals': body('  mc.log(Object.getOwnPropertyNames(globalThis).sort().join(","));'),
  'adv-globals-order': body('  mc.log(Object.getOwnPropertyNames(globalThis).join(","));\n  mc.log(Object.keys(globalThis).join(","));'),
  'adv-host-names': probe('  for (const name of ["process", "require", "Buffer", "global", "module", "exports", "__filename", "console", "fetch", "structuredClone", "TextEncoder", "URL", "performance", "navigator", "WebAssembly", "SharedArrayBuffer", "Atomics", "Intl", "Temporal", "ShadowRealm", "Iterator", "AsyncIterator", "FinalizationRegistry", "WeakRef", "escape", "Float16Array", "DisposableStack", "eval", "Function"]) p(name, () => typeof globalThis[name]);'),
  'adv-constructor-escape': probe('  p("fn-ctor", () => mc.set.constructor("return typeof process")());\n  p("this-ctor", () => this === undefined);\n  p("global-ctor", () => globalThis.constructor.constructor("return typeof process")());\n  p("mc-proto", () => Object.getPrototypeOf(mc) === Object.prototype);\n  p("log-ctor", () => mc.log.constructor === Function);'),
  'adv-prepare-stack-trace': probe('  Error.prepareStackTrace = (e, frames) => frames.map((f) => [f.getFileName(), f.getFunctionName(), typeof f.getThis(), f.getLineNumber(), f.getColumnNumber()].join(":")).join("|");\n  p("frames", () => { try { mc.set([0, 0, 0], "nope"); } catch (e) { return e.stack; } });\n  p("own", () => new Error("x").stack);'),
  'adv-capture-stack-trace': probe('  const o = {}; Error.captureStackTrace(o); p("captured", () => o.stack);\n  p("limit", () => Error.stackTraceLimit);\n  Error.stackTraceLimit = 2; p("short", () => new Error("s").stack);'),
  'adv-stack-limit-infinity': body('  Error.stackTraceLimit = Infinity;\n  function f(n) { if (n === 0) throw new Error("bottom"); return f(n - 1); }\n  f(30);'),
  'adv-proto-pollution': body('  Object.prototype.id = "tnt"; Object.prototype.states = { lit: "true" }; Array.prototype.length2 = 1;\n  JSON.stringify = () => "[]"; JSON.parse = () => ({ hacked: true });\n  mc.set([0, 0, 0], "stone"); mc.log(JSON.stringify(mc.get([0, 0, 0])), mc.get([0, 0, 0]));'),
  'adv-proto-pollution-host': body('  Object.prototype.toJSON = function () { return "polluted"; };\n  mc.set([0, 0, 0], "stone"); mc.log("after", { a: 1 }); mc.writeFile("p.json", "{}");'),
  'adv-array-proto-pollution': body('  Array.prototype.map = function () { return ["minecraft:tnt"]; };\n  Array.isArray = () => true;\n  mc.set([0, 0, 0], "stone"); mc.fill([0, 1, 0], [2, 1, 2], "stone");'),
  'adv-getter-block': body('  let n = 0;\n  const spec = { get id() { n += 1; return n === 1 ? "stone" : "tnt"; }, states: {} };\n  mc.set([0, 0, 0], spec); mc.log(n, JSON.stringify(mc.get([0, 0, 0])));'),
  'adv-getter-pos': body('  let n = 0;\n  const pos = [0, 0, 0]; Object.defineProperty(pos, 0, { get() { n += 1; return n * 10; } });\n  mc.set(pos, "stone"); mc.log(n, JSON.stringify(mc.find("stone")));'),
  'adv-proxy-block': body('  const spec = new Proxy({ id: "stone", states: {} }, { get(t, k) { return k === "id" ? "dirt" : t[k]; }, ownKeys() { return ["id", "states", "x"]; } });\n  mc.set([0, 0, 0], spec); mc.log(JSON.stringify(mc.get([0, 0, 0])));'),
  'adv-proxy-log': body('  mc.log(new Proxy({}, { get() { throw new Error("trap"); } }));'),
  'adv-tojson-throws': body('  mc.log({ toJSON() { throw new Error("no json"); }, toString() { return "fallback"; } });'),
  'adv-symbol-log': body('  mc.log(Symbol("x"), 10n === 10n, [undefined, () => 1, NaN, -0, Infinity]);'),
  'adv-bigint-log': body('  mc.log(10n);'),
  'adv-states-object-getter': body('  const states = {}; Object.defineProperty(states, "facing", { enumerable: true, get() { return "north"; } });\n  mc.set([0, 0, 0], { id: "oak_stairs", states }); mc.log(JSON.stringify(mc.get([0, 0, 0])));'),
  'adv-return-value-getter': body('  const b = mc.bounds(); mc.set([0, 0, 0], "stone"); const c = mc.bounds(); c.min[0] = 99; mc.log(JSON.stringify(b), JSON.stringify(mc.bounds()));'),
  'adv-mc-mutation': body('  mc.set = () => mc.log("replaced"); delete mc.fill; mc.set([0, 0, 0], "stone"); mc.log(typeof mc.fill, Object.keys(mc).join(","));'),
  'adv-module-reassign': 'module = { exports: function () {} };\nexports.x = 1;\n',
  'adv-not-function': 'module.exports = { build() {} };\n',
  'adv-empty-source': '\n',
  'adv-eval': body('  mc.log(eval("1 + 1"), new Function("a", "return a * 2")(21), typeof eval("this"));'),
  'adv-with': 'module.exports = function (mc) { with ({ x: 1 }) { mc.log(x); } };\n',
  'adv-strict-reserved': '"use strict";\nmodule.exports = function (mc) { var interface = 1; };\n',
  // Engine wording.
  'adv-errors-wording': probe([
    'const u = undefined; const n = null; const o = {};',
    'p("undef.x", () => u.x);', 'p("null.x", () => n.x);', 'p("o.x.y", () => o.x.y);', 'p("call undef", () => u());',
    'p("o.f()", () => o.f());', 'p("mc.foo()", () => mc.foo());', 'p("notdefined", () => notDefinedAnywhere);',
    'p("const", () => { const c = 1; c = 2; });', 'p("new 5", () => new 5());', 'p("reduce", () => [].reduce((a, b) => a));',
    'p("json", () => JSON.parse("{"));', 'p("json2", () => JSON.parse("{\\"a\\": 1,}"));', 'p("json3", () => JSON.parse(""));',
    'p("repeat", () => "a".repeat(-1));', 'p("bigint", () => BigInt(1.5));', 'p("symbol+", () => Symbol() + "");',
    'p("in", () => "a" in 5);', 'p("instanceof", () => ({}) instanceof 5);', 'p("tdz", () => { x; let x = 1; });',
    'p("iter", () => { const [a] = 5; });', 'p("spread", () => [...undefined]);', 'p("destructure", () => { const { a } = null; });',
    'p("cyclic", () => { const c = {}; c.c = c; return JSON.stringify(c); });', 'p("toFixed", () => (1).toFixed(101));',
    'p("precision", () => (1).toPrecision(0));', 'p("radix", () => (1).toString(1));', 'p("set", () => new Set().add.call(1));',
    'p("map.get", () => Map.prototype.get.call({}));', 'p("invalid date", () => new Date(NaN).toISOString());',
    'p("regex", () => new RegExp("("));', 'p("class call", () => { class A {} A(); });', 'p("super", () => { class A { constructor() { this.x; } } class B extends A { constructor() { this.y = 1; } } new B(); });',
    'p("frozen", () => { "use strict"; const f = Object.freeze({ a: 1 }); f.a = 2; });', 'p("getter only", () => { "use strict"; const g = { get a() { return 1; } }; g.a = 2; });',
    'p("array len", () => { const a = []; a.length = -1; });', 'p("stack size", () => { const f = () => f(); f(); });',
    'p("fn.prototype.name", () => (function namedFn() {}).name);', 'p("arrow name", () => { const arrowName = () => {}; return arrowName.name; });',
  ].map((line) => `  ${line}`).join('\n')),
  'adv-syntax-1': 'module.exports = function (mc) {\n  mc.set([0, 0, 0], "stone")\n  for (let i = 0; i < 3; i++ {\n  }\n};\n',
  'adv-syntax-2': 'module.exports = function (mc) {\n  const x = "unterminated;\n};\n',
  'adv-syntax-3': 'module.exports = function (mc) {\n  let let = 1;\n};\n',
  'adv-syntax-4': 'module.exports = function (mc) {\n  mc.set([0, 0, 0], "stone");\n',
  'adv-syntax-5': 'module.exports = function (mc) {\n  const a = await mc.get([0, 0, 0]);\n};\n',
  'adv-syntax-6': 'module.exports = function (mc) {\n\tconst r = /(/;\n};\n',
  'adv-syntax-7': 'module.exports = (mc) => {\n  const obj = { a: 1, b: 2 c: 3 };\n};\n',
  'adv-syntax-8': 'module.exports = (mc) => {\n  const héllo = "é"; const 𝒳 = 1; mc.log(héllo +);\n};\n',
  'adv-syntax-9': 'module.exports = (mc) => {\n  return <div/>;\n};\n',
  'adv-syntax-10': 'module.exports = (mc) => { mc.log("a") }}\n',
  'adv-syntax-11': 'module.exports = function (mc) { const a = 1; const a = 2; };\n',
  'adv-syntax-12': 'import fs from "fs";\nmodule.exports = function (mc) {};\n',
  'adv-runtime-unicode-caret': 'module.exports = (mc) => {\n  const s = "ééé"; const u = undefined; mc.log(s, u.prop);\n};\n',
  'adv-runtime-tab-caret': 'module.exports = (mc) => {\n\t\tconst u = undefined;\n\t\tmc.log(u.prop);\n};\n',
  'adv-runtime-long-line': `module.exports = (mc) => { const u = undefined; ${'mc.log(1); '.repeat(150)} mc.log(u.prop); };\n`,
  // Numbers, Math, ordering, locale.
  'adv-math-bits': body(`  const fns = ["sin", "cos", "tan", "asin", "acos", "atan", "sinh", "cosh", "tanh", "asinh", "acosh", "atanh", "exp", "expm1", "log", "log1p", "log2", "log10", "sqrt", "cbrt", "fround"];
  const view = new DataView(new ArrayBuffer(8));
  const bits = (x) => { view.setFloat64(0, x); return view.getUint32(0).toString(16) + view.getUint32(4).toString(16); };
  let h = 2166136261;
  const mix = (x) => { view.setFloat64(0, x); for (let i = 0; i < 8; i += 1) { h ^= view.getUint8(i); h = Math.imul(h, 16777619); } };
  const inputs = [];
  for (let i = -2000; i <= 2000; i += 1) inputs.push(i * 0.0137, i * 1.7, i / 7, Math.PI * i / 64, 1e-8 * i, i * 1e5 + 0.5);
  for (const name of fns) { let f = 2166136261; h = 2166136261; for (const x of inputs) mix(Math[name](x)); mc.log(name, (h >>> 0).toString(16)); }
  h = 2166136261; for (const x of inputs) for (const y of [0.5, 2, 3, -1.5, 1 / 3, 10, 0.1]) mix(Math.pow(Math.abs(x), y)); mc.log("pow", (h >>> 0).toString(16));
  h = 2166136261; for (const x of inputs) mix(Math.atan2(x, 1.3 - x)); mc.log("atan2", (h >>> 0).toString(16));
  h = 2166136261; for (const x of inputs) mix(Math.hypot(x, 2 * x + 1, 3)); mc.log("hypot", (h >>> 0).toString(16));
  h = 2166136261; for (const x of inputs) mix(x ** 1.5); mc.log("**", (h >>> 0).toString(16));
  mc.log(bits(Math.sin(1e22)), bits(Math.cos(1e300)), bits(Math.exp(709.7)), bits(Math.pow(2, 0.5)), bits(Math.sin(Math.PI)), bits(Math.tan(Math.PI / 2)));`),
  'adv-circle': body('  const r = 40.5;\n  for (let a = 0; a < 3600; a += 1) { const t = a * Math.PI / 1800; mc.set([Math.round(r * Math.cos(t)), 0, Math.round(r * Math.sin(t))], "stone"); }\n  for (let x = -30; x <= 30; x += 1) for (let z = -30; z <= 30; z += 1) { const y = Math.floor(Math.sqrt(Math.max(0, 900 - x * x - z * z)) * Math.cos(x / 9) ** 2); mc.set([x, y + 2, z], "glass"); }'),
  'adv-numbers': probe([
    'for (const v of [0.1 + 0.2, 1 / 3, 2 ** 53 + 1, -0, 1e-7, 123e20, 1e21, 5e-324, 1.7976931348623157e308, 0.000001, 123456789.123456789, 4.35, 1.005])',
    '  { p("s " + v, () => String(v)); p("f " + v, () => v.toFixed(3)); p("p " + v, () => v.toPrecision(7)); p("e " + v, () => v.toExponential(4)); p("r " + v, () => v.toString(36)); }',
    'p("parse", () => [parseFloat("1e1000"), parseFloat(".5e-3x"), Number("0x1F"), Number(" 12 "), parseInt("08"), Number("1_000")].join(","));',
  ].map((line) => `  ${line}`).join('\n')),
  'adv-sort': body(`  const nums = []; let s = 7; for (let i = 0; i < 200; i += 1) { s = (s * 1103515245 + 12345) % 2147483648; nums.push(s % 50); }
  mc.log(JSON.stringify(nums.slice().sort((a, b) => a > b)));
  mc.log(JSON.stringify(nums.slice().sort((a, b) => a < b ? -1 : 1)));
  mc.log(JSON.stringify(nums.slice().sort(() => Math.random() - 0.5)));
  mc.log(JSON.stringify(nums.map((n, i) => ({ n, i })).sort((a, b) => a.n - b.n).map((o) => o.i)));
  mc.log(JSON.stringify([10, 9, 1, "a", undefined, null, "B", 2].sort()));
  let calls = 0; nums.slice(0, 50).sort((a, b) => { calls += 1; return a - b; }); mc.log(calls);`),
  'adv-key-order': body('  const o = { b: 1, 2: 1, a: 1, 1: 1, [Symbol("s")]: 1, "-1": 1, "01": 1, 4294967295: 1, 4294967294: 1 };\n  mc.log(Object.keys(o).join(","), JSON.stringify(o), Reflect.ownKeys(o).length);\n  const m = new Map([[3, 1], [1, 1], ["x", 1]]); mc.log([...m.keys()].join(","));'),
  'adv-random': body('  mc.log(Math.random(), Math.random(), Math.random());'),
  'adv-date': probe('  p("now type", () => typeof Date.now());\n  p("offset", () => new Date(2020, 0, 1).getTimezoneOffset());\n  p("toString", () => new Date(0).toString());\n  p("iso", () => new Date(Date.UTC(2020, 1, 29, 12)).toISOString());\n  p("parse", () => Date.parse("2020-01-01"));\n  p("parse2", () => Date.parse("March 7, 2021 10:00"));\n  p("toDateString", () => new Date(0).toDateString());\n  p("utc", () => new Date(0).toUTCString());'),
  'adv-intl': probe([
    'p("default locale", () => Intl.DateTimeFormat().resolvedOptions().locale);', 'p("tz", () => Intl.DateTimeFormat().resolvedOptions().timeZone);',
    'p("n", () => (1234567.891).toLocaleString());', 'p("n de", () => (1234567.891).toLocaleString("de-DE"));', 'p("n hi", () => (1234567.891).toLocaleString("hi-IN"));',
    'p("date", () => new Date(0).toLocaleString());', 'p("date en", () => new Date(0).toLocaleDateString("en-US", { weekday: "long", year: "numeric", month: "long", day: "numeric" }));',
    'p("time", () => new Date(0).toLocaleTimeString("en-GB"));', 'p("nf", () => new Intl.NumberFormat("en-US", { style: "currency", currency: "EUR" }).format(-1234.5));',
    'p("compact", () => new Intl.NumberFormat("en", { notation: "compact" }).format(1234567));', 'p("unit", () => new Intl.NumberFormat("en", { style: "unit", unit: "kilometer-per-hour" }).format(50));',
    'p("collate", () => ["b", "a", "ä", "Z", "z"].sort(new Intl.Collator("de").compare).join(""));', 'p("localeCompare", () => "ä".localeCompare("z"));',
    'p("plural", () => new Intl.PluralRules("ar").select(3));', 'p("list", () => new Intl.ListFormat("en").format(["a", "b", "c"]));',
    'p("relative", () => new Intl.RelativeTimeFormat("en").format(-1, "day"));', 'p("upper", () => "ß".toLocaleUpperCase("de") + "i".toLocaleUpperCase("tr"));',
    'p("segment", () => [...new Intl.Segmenter("en", { granularity: "word" }).segment("Hi there")].length);', 'p("displayNames", () => new Intl.DisplayNames("en", { type: "region" }).of("DE"));',
    'p("supported", () => Intl.NumberFormat.supportedLocalesOf(["de", "xx", "zh-Hant"]).join(","));',
  ].map((line) => `  ${line}`).join('\n')),
  'adv-regex': probe([
    'p("lookbehind", () => "a1b2".replace(/(?<=[a-z])\\d/g, "#"));', 'p("named", () => JSON.stringify("2020-01".match(/(?<y>\\d+)-(?<m>\\d+)/).groups));',
    'p("unicode prop", () => /\\p{Script=Greek}/u.test("α"));', 'p("v flag", () => /[\\p{L}--[a-z]]/v.test("A"));', 'p("dup names", () => /(?<a>x)|(?<a>y)/.exec("y").groups.a);',
    'p("sticky", () => { const r = /a/y; r.lastIndex = 1; return r.test("ba"); });', 'p("dotall", () => /a.b/s.test("a\\nb"));', 'p("indices", () => JSON.stringify(/b/d.exec("ab").indices));',
    'p("modifiers", () => /(?i:a)b/.test("Ab"));', 'p("escape", () => RegExp.escape("a.b"));', 'p("split", () => "a,b,,c".split(/,/).length);', 'p("replaceAll", () => "aaa".replaceAll("a", "$&$&"));',
    'p("case fold", () => /\\u212A/iu.test("k"));', 'p("matchAll", () => [..."a1a2".matchAll(/a(\\d)/g)].map((m) => m[1]).join(""));',
  ].map((line) => `  ${line}`).join('\n')),
  'adv-builtins': probe([
    'p("findLast", () => [1, 2, 3].findLast((x) => x < 3));', 'p("groupBy", () => JSON.stringify(Object.groupBy([1, 2, 3], (x) => x % 2)));', 'p("toSorted", () => [3, 1, 2].toSorted().join(""));',
    'p("set union", () => [...new Set([1]).union(new Set([2]))].join(""));', 'p("iterator helpers", () => [1, 2, 3].values().map((x) => x * 2).toArray().join(""));',
    'p("sumPrecise", () => Math.sumPrecise([0.1, 0.2]));', 'p("isError", () => Error.isError(new Error()));', 'p("fromBase64", () => Uint8Array.fromBase64("AQI=").join(""));',
    'p("f16", () => Math.f16round(1.337));', 'p("array fromAsync", () => typeof Array.fromAsync);', 'p("withResolvers", () => typeof Promise.withResolvers);', 'p("using", () => typeof Symbol.dispose);',
    'p("structuredClone", () => typeof structuredClone);', 'p("at", () => [1, 2].at(-1));', 'p("hasOwn", () => Object.hasOwn({ a: 1 }, "a"));', 'p("wellformed", () => "a\\ud800".isWellFormed());',
    'p("String.raw", () => String.raw`a\\n${1}`);', 'p("Intl.DurationFormat", () => typeof Intl.DurationFormat);', 'p("Temporal", () => typeof Temporal);', 'p("Atomics.pause", () => typeof Atomics.pause);',
    'p("Iterator.concat", () => typeof Iterator.concat);', 'p("Map.getOrInsert", () => typeof Map.prototype.getOrInsert);', 'p("Error.captureStackTrace", () => typeof Error.captureStackTrace);',
    'p("fn toString", () => (function (a, b) { return a + b; }).toString());', 'p("class toString", () => (class K { m() {} }).toString());', 'p("native toString", () => Math.max.toString());',
  ].map((line) => `  ${line}`).join('\n')),
  'adv-cpu-2e8': body('  let s = 0; for (let i = 0; i < 2e8; i += 1) s += i % 7; mc.log(s);'),
  'adv-cpu-objects': body('  const m = new Map(); for (let i = 0; i < 2e6; i += 1) m.set("k" + i, { i, v: [i, i * 2] }); let t = 0; for (const [, v] of m) t += v.v[1]; mc.log(m.size, t);'),
  'adv-cpu-mc-set': body('  for (let x = 0; x < 200; x += 1) for (let y = 0; y < 50; y += 1) for (let z = 0; z < 100; z += 1) mc.set([x, y, z], (x + y + z) % 3 ? "stone" : "oak_planks");'),
  'adv-cpu-strings': body('  const parts = []; for (let i = 0; i < 3e5; i += 1) parts.push(`row ${i}: ${(i * 1.1).toFixed(2)}`); const s = parts.join("\\n"); mc.log(s.length, s.split("\\n").length, JSON.parse(JSON.stringify({ s })).s.length);'),
};

for (const [id, source] of Object.entries(cases)) write(id, source);

// mc.settled and mc.readFile with recorded answers.
const small = new World();
small.set(0, 0, 0, 'minecraft:stone', {});
small.set(1, 0, 0, 'minecraft:water', { level: '0' });
write('adv-settled', body('  mc.log(mc.settledTicks, JSON.stringify(mc.settled(0, 0, 0)), JSON.stringify(mc.settled(5, 5, 5)));'), { world: small, settled: { ticks: 210, world: 'world.bin' } });
write('adv-readfile', body('  mc.log(mc.readFile("a.json").length); try { mc.readFile("missing.txt"); } catch (e) { mc.log(String(e)); }'), { readFile: { 'a.json': { ok: true, text: '{"a":[1,2,3]}' }, 'missing.txt': { ok: false, text: 'mc.readFile: missing.txt: no such file' } } });

// scripts/benchmark-build.js's workloads: its village on its 2.1M-block
// terrain (readFile heightmap) and a one-block edit of a large world.
const bench = fs.readFileSync(path.join(W, 'scripts/benchmark-build.js'), 'utf8');
const SIZE = 256;
const heights = Array.from({ length: SIZE }, (_, x) => Array.from({ length: SIZE }, (_, z) => 31 + ((x * 7 + z * 13) % 8)));
const village = new Function('SIZE', `return \`${/const VILLAGE = `([\s\S]*?)`;/.exec(bench)[1]}\`;`)(SIZE);
const terrain = new World();
terrain.fill([0, 0, 0], [SIZE - 1, 30, SIZE - 1], 'minecraft:stone');
terrain.fill([0, 0, 0], [SIZE - 1, 4, SIZE - 1], 'minecraft:deepslate');
terrain.finalize();
write('bench-village', village, { world: terrain, readFile: { 'heightmap.json': { ok: true, text: JSON.stringify(heights) } } });
write('bench-one-block-large', 'module.exports = (mc) => mc.set([0, 60, 0], "gold_block")', { world: terrain });
write('bench-one-block-empty', 'module.exports = (mc) => mc.set([0, 60, 0], "gold_block")');

if (flag === '--big') {
  write('big-heap-oom', body('  const keep = []; for (;;) keep.push(new Array(1 << 20).fill(1.5));'));
  write('big-typed-bomb', body('  const keep = []; for (let i = 0; i < 64; i += 1) { const a = new Float64Array(1 << 27); a.fill(i); keep.push(a); } mc.log(keep.length);'));
  write('big-string-array-oom', body('  const keep = []; for (let i = 0; ; i += 1) keep.push("k" + i + "x".repeat(i % 64));'));
  write('big-heap-1500mb', body('  const keep = []; for (let i = 0; i < 1500; i += 1) keep.push(new Array(131072).fill(i)); mc.log(keep.length);'));
}
console.log(`wrote ${Object.keys(cases).length + 5}${flag === '--big' ? ' + 4 big' : ''} cases`);
