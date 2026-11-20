// The Node facilities server/build-child.js and the four modules it loads use,
// for the embedded V8 of `mcbench-build-child` (from the JavaScript
// spike, native/spikes/js/shim/prelude.js). The child evaluates this file in
// its main realm as a function expression and calls it with `host` (the Rust
// host functions of child/mod.rs), `sources` (file name -> unchanged source
// text) and `env` (the variables the parent passed). It then runs
// build-child.js.
//
// What the five files touch, exactly:
//   require: 'fs', 'path', 'vm', './mc-dsl', './world', './minecraft-dimension',
//            './build-transfer'
//   Buffer:  allocUnsafe, alloc, from(string, 'utf8'), from(arrayBuffer,
//            offset, length), concat, and on instances readUInt8, readUInt16LE,
//            readUInt32LE, readInt32LE, writeUInt8, writeUInt16LE,
//            writeUInt32LE, writeInt32LE, toString('utf8', start, end), copy,
//            subarray (Uint8Array's), length
//   fs:      readSync(0, buffer, offset, length, null), writeSync(1, buffer, offset)
//            (mkdirSync/writeFileSync only in writeDimensionPack, never called here)
//   path:    join (writeDimensionPack only)
//   process: env.MCBENCH_MC_Y_OFFSET, exit(code)
//   vm:      createContext({}), new Script(code, { filename }),
//            script.runInContext(context, { timeout })
// Everything else they use is ECMAScript (Map, Set, typed arrays, JSON,
// WeakMap, Object.freeze, String.fromCharCode, Math).
//
// host: read(u8, offset, length) -> n           (fd 0)
//       write(u8, offset) -> n                  (fd 1)
//       exit(code)
//       utf8Encode(string) -> ArrayBuffer
//       utf8Decode(u8, start, end) -> string
//       compileFunction(code, filename) -> function, the CommonJS wrapper
//       createContext(sandbox) -> handle      (Node's contextified global over `sandbox`)
//       compile(code, filename) -> handle       (throws Node's decorated SyntaxError)
//       run(scriptHandle, contextHandle, timeoutMs|undefined) -> value
(function (host, sources, env) {
  'use strict';

  const outOfRange = (offset, size, length) => {
    if (!Number.isInteger(offset) || offset < 0 || offset + size > length) {
      throw new RangeError(`The value of "offset" is out of range. It must be >= 0 and <= ${length - size}. Received ${offset}`);
    }
  };

  class Buffer extends Uint8Array {
    static allocUnsafe(size) { return new Buffer(size); }

    static alloc(size) { return new Buffer(size); }

    static from(value, a, b) {
      if (typeof value === 'string') {
        if (a !== undefined && a !== 'utf8') throw new TypeError(`Buffer.from: encoding ${a} is not supported here`);
        return new Buffer(host.utf8Encode(value));
      }
      if (value instanceof ArrayBuffer) return new Buffer(value, a, b);
      throw new TypeError('Buffer.from: only strings and ArrayBuffers are supported here');
    }

    static concat(list) {
      let length = 0;
      for (const part of list) length += part.length;
      const out = new Buffer(length);
      let offset = 0;
      for (const part of list) { out.set(part, offset); offset += part.length; }
      return out;
    }

    readUInt8(offset = 0) { outOfRange(offset, 1, this.length); return this[offset]; }

    readUInt16LE(offset = 0) { outOfRange(offset, 2, this.length); return this[offset] | (this[offset + 1] << 8); }

    readUInt32LE(offset = 0) {
      outOfRange(offset, 4, this.length);
      return (this[offset] | (this[offset + 1] << 8) | (this[offset + 2] << 16)) + this[offset + 3] * 0x1000000;
    }

    readInt32LE(offset = 0) {
      outOfRange(offset, 4, this.length);
      return this[offset] | (this[offset + 1] << 8) | (this[offset + 2] << 16) | (this[offset + 3] << 24);
    }

    writeUInt8(value, offset = 0) { outOfRange(offset, 1, this.length); this[offset] = value; return offset + 1; }

    writeUInt16LE(value, offset = 0) {
      outOfRange(offset, 2, this.length);
      this[offset] = value; this[offset + 1] = value >>> 8;
      return offset + 2;
    }

    writeUInt32LE(value, offset = 0) {
      outOfRange(offset, 4, this.length);
      this[offset] = value; this[offset + 1] = value >>> 8; this[offset + 2] = value >>> 16; this[offset + 3] = value >>> 24;
      return offset + 4;
    }

    writeInt32LE(value, offset = 0) { return this.writeUInt32LE(value >>> 0, offset); }

    toString(encoding, start = 0, end = this.length) {
      if (encoding !== undefined && encoding !== 'utf8') throw new TypeError(`Buffer.toString: encoding ${encoding} is not supported here`);
      return host.utf8Decode(this, start, end);
    }

    copy(target, targetStart = 0, sourceStart = 0, sourceEnd = this.length) {
      const part = this.subarray(sourceStart, Math.min(sourceEnd, sourceStart + target.length - targetStart));
      target.set(part, targetStart);
      return part.length;
    }
  }

  const fs = {
    readSync(fd, buffer, offset, length) {
      if (fd !== 0) throw new Error(`fs.readSync: only fd 0 here, not ${fd}`);
      return host.read(buffer, offset, length);
    },
    writeSync(fd, buffer, offset = 0) {
      if (fd !== 1) throw new Error(`fs.writeSync: only fd 1 here, not ${fd}`);
      return host.write(buffer, offset);
    },
    mkdirSync() { throw new Error('fs.mkdirSync is not available in the build process'); },
    writeFileSync() { throw new Error('fs.writeFileSync is not available in the build process'); },
  };
  const path = { join: (...parts) => parts.join('/') };
  const process = { env, exit: (code) => host.exit(code) };

  class Script {
    constructor(code, options) { this.handle = host.compile(code, options.filename); }

    runInContext(context, options) { return host.run(this.handle, context, options ? options.timeout : undefined); }
  }
  const vm = { createContext: (sandbox) => host.createContext(sandbox), Script };

  const builtins = { fs, path, vm };
  const cache = new Map();
  function load(name) {
    if (builtins[name]) return builtins[name];
    if (!name.startsWith('./')) throw new Error(`Cannot find module '${name}'`);
    const file = `${name.slice(2)}.js`;
    if (cache.has(file)) return cache.get(file).exports;
    if (!(file in sources)) throw new Error(`Cannot find module '${name}'`);
    const module = { exports: {} };
    cache.set(file, module);
    host.compileFunction(sources[file], `/server/${file}`)(module.exports, load, module, `/server/${file}`, '/server');
    return module.exports;
  }

  globalThis.Buffer = Buffer;
  globalThis.process = process;
  return (file) => {
    const module = { exports: {} };
    host.compileFunction(sources[file], `/server/${file}`)(module.exports, load, module, `/server/${file}`, '/server');
  };
})
