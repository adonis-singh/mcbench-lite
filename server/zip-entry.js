'use strict';

// One file's bytes from a zip (or jar), read through its central directory
// without unpacking the rest.

const fs = require('fs');
const zlib = require('zlib');

function readZipEntry(file, name) {
  const descriptor = fs.openSync(file, 'r');
  try {
    const { size } = fs.fstatSync(descriptor);
    // The end-of-central-directory record sits in the last 64 KiB (plus its
    // own 22 bytes), after any archive comment.
    const tailLength = Math.min(size, 65_557);
    const tail = Buffer.alloc(tailLength);
    fs.readSync(descriptor, tail, 0, tailLength, size - tailLength);
    const end = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
    if (end < 0) throw new Error(`${file} is not a zip archive`);
    const entries = tail.readUInt16LE(end + 10);
    const directorySize = tail.readUInt32LE(end + 12);
    const directory = Buffer.alloc(directorySize);
    fs.readSync(descriptor, directory, 0, directorySize, tail.readUInt32LE(end + 16));
    for (let at = 0, index = 0; index < entries; index += 1) {
      const nameLength = directory.readUInt16LE(at + 28);
      const entryName = directory.toString('utf8', at + 46, at + 46 + nameLength);
      if (entryName === name) {
        const method = directory.readUInt16LE(at + 10);
        const compressed = directory.readUInt32LE(at + 20);
        const local = Buffer.alloc(30);
        fs.readSync(descriptor, local, 0, 30, directory.readUInt32LE(at + 42));
        const data = Buffer.alloc(compressed);
        fs.readSync(descriptor, data, 0, compressed, directory.readUInt32LE(at + 42) + 30 + local.readUInt16LE(26) + local.readUInt16LE(28));
        if (method === 0) return data;
        if (method === 8) return zlib.inflateRawSync(data);
        throw new Error(`${name} in ${file} uses unsupported compression ${method}`);
      }
      at += 46 + nameLength + directory.readUInt16LE(at + 30) + directory.readUInt16LE(at + 32);
    }
    throw new Error(`${file} has no ${name}`);
  } finally {
    fs.closeSync(descriptor);
  }
}

module.exports = { readZipEntry };
