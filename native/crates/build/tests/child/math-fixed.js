// The Math probe of fixed inputs (tests/child, math-fixed): every input is
// a bit pattern made with integer operations only (a xorshift128 sequence
// and a few special values, written into a DataView), never through another
// Math function or floating-point arithmetic, so every platform feeds every
// function the same doubles. For each function it logs two FNV-1a hashes of
// the results' bits: all bits, and with every NaN replaced by one pattern
// (a NaN's sign and payload are visible only through typed arrays and
// DataView). `pow`, `**` and `atan2` also take a second input of fixed bits.
module.exports = (mc) => {
  const view = new DataView(new ArrayBuffer(8));
  const fromBits = (hi, lo) => { view.setUint32(0, hi >>> 0); view.setUint32(4, lo >>> 0); return view.getFloat64(0); };
  let s0 = 0x9e3779b9; let s1 = 0x7f4a7c15; let s2 = 0x12345678; let s3 = 0xdeadbeef;
  const next = () => {
    let t = s3;
    const s = s0;
    s3 = s2; s2 = s1; s1 = s;
    t ^= t << 11; t ^= t >>> 8;
    s0 = (t ^ s ^ (s >>> 19)) >>> 0;
    return s0;
  };
  const specials = [
    [0, 0], [0x80000000, 0], [0x3ff00000, 0], [0xbff00000, 0], [0x3fe00000, 0], [0x40000000, 0], [0x7ff00000, 0], [0xfff00000, 0],
    [0x7ff80000, 0], [0, 1], [0x7fefffff, 0xffffffff], [0x00100000, 0], [0x3fefffff, 0xffffffff], [0x3ff00000, 1], [0x400921fb, 0x54442d18],
    [0x40862e42, 0xfefa39ef], [0xc0874910, 0xd52d3051], [0x43300000, 0], [0x3cb00000, 0], [0x40590000, 0],
  ];
  const xs = specials.map(([hi, lo]) => fromBits(hi, lo));
  // Anything: NaNs, infinities, subnormals and huge numbers.
  for (let i = 0; i < 500; i += 1) xs.push(fromBits(next(), next()));
  // |x| between 2^-12 and 2^12, either sign: where programs compute.
  for (let i = 0; i < 4000; i += 1) {
    const sign = next() & 0x80000000;
    const exponent = 1023 - 12 + (next() % 25);
    xs.push(fromBits(sign | (exponent << 20) | (next() & 0xfffff), next()));
  }
  const ys = [];
  for (let i = 0; i < xs.length; i += 1) {
    const exponent = 1023 - 4 + (next() % 9);
    ys.push(fromBits((next() & 0x80000000) | (exponent << 20) | (next() & 0xfffff), next()));
  }
  const hash = (results) => {
    let all = 0x811c9dc5; let canon = 0x811c9dc5;
    for (const r of results) {
      view.setFloat64(0, r);
      const nan = r !== r;
      for (let i = 0; i < 8; i += 1) {
        const byte = view.getUint8(i);
        all ^= byte; all = Math.imul(all, 16777619);
        canon ^= nan ? [0x7f, 0xf8, 0, 0, 0, 0, 0, 0][i] : byte; canon = Math.imul(canon, 16777619);
      }
    }
    return `${(all >>> 0).toString(16).padStart(8, '0')} ${(canon >>> 0).toString(16).padStart(8, '0')}`;
  };
  const unary = ['sin', 'cos', 'tan', 'asin', 'acos', 'atan', 'sinh', 'cosh', 'tanh', 'asinh', 'acosh', 'atanh', 'exp', 'expm1', 'log', 'log1p', 'log2', 'log10',
    'sqrt', 'cbrt', 'fround', 'abs', 'floor', 'ceil', 'round', 'trunc', 'sign', 'f16round'];
  for (const name of unary) mc.log(`${name} ${hash(xs.map((x) => Math[name](x)))}`);
  for (const y of [0.5, 2, 3, -1.5, 10, -2, 0.25, 1.5, -0.5, 4]) mc.log(`pow(x,${y}) ${hash(xs.map((x) => Math.pow(x, y)))}`);
  mc.log(`pow(x,y) ${hash(xs.map((x, i) => Math.pow(x, ys[i])))}`);
  mc.log(`x**1.5 ${hash(xs.map((x) => x ** 1.5))}`);
  mc.log(`x**y ${hash(xs.map((x, i) => x ** ys[i]))}`);
  mc.log(`atan2 ${hash(xs.map((x, i) => Math.atan2(x, ys[i])))}`);
  mc.log(`hypot ${hash(xs.map((x, i) => Math.hypot(x, ys[i], 3)))}`);
  mc.log(`+-*/% ${hash(xs.flatMap((x, i) => [x + ys[i], x - ys[i], x * ys[i], x / ys[i], x % ys[i]]))}`);
  mc.log(`inputs ${hash(xs.concat(ys))}`);
};
