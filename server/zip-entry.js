'use strict';

// Files of a zip (or jar), read through its central directory: the names of
// its entries (zipNames), one file's bytes without unpacking the rest
// (readZipEntry), or every file unpacked into a folder with its Unix mode and
// symbolic links (extractZip).

const fs = require('fs');
const path = require('path');
const zlib = require('zlib');
const { pipeline } = require('stream/promises');

const S_IFMT = 0o170000;
const S_IFLNK = 0o120000;

// The archive's entries: name, compression method, compressed size, Unix
// mode (0 when the archive was not made on Unix), and where its data starts.
function entries(descriptor, file) {
  const { size } = fs.fstatSync(descriptor);
  // The end-of-central-directory record sits in the last 64 KiB (plus its
  // own 22 bytes), after any archive comment.
  const tailLength = Math.min(size, 65_557);
  const tail = Buffer.alloc(tailLength);
  fs.readSync(descriptor, tail, 0, tailLength, size - tailLength);
  const end = tail.lastIndexOf(Buffer.from([0x50, 0x4b, 0x05, 0x06]));
  if (end < 0) throw new Error(`${file} is not a zip archive`);
  const count = tail.readUInt16LE(end + 10);
  const directorySize = tail.readUInt32LE(end + 12);
  const directory = Buffer.alloc(directorySize);
  fs.readSync(descriptor, directory, 0, directorySize, tail.readUInt32LE(end + 16));
  const list = [];
  for (let at = 0, index = 0; index < count; index += 1) {
    const nameLength = directory.readUInt16LE(at + 28);
    const offset = directory.readUInt32LE(at + 42);
    const local = Buffer.alloc(30);
    fs.readSync(descriptor, local, 0, 30, offset);
    list.push({
      name: directory.toString('utf8', at + 46, at + 46 + nameLength),
      method: directory.readUInt16LE(at + 10),
      compressed: directory.readUInt32LE(at + 20),
      mode: directory.readUInt8(at + 5) === 3 ? directory.readUInt32LE(at + 38) >>> 16 : 0,
      data: offset + 30 + local.readUInt16LE(26) + local.readUInt16LE(28),
    });
    at += 46 + nameLength + directory.readUInt16LE(at + 30) + directory.readUInt16LE(at + 32);
  }
  return list;
}

// The names of the archive's entries, in the central directory's order (the
// order `unzip -Z1` lists them in).
function zipNames(file) {
  const descriptor = fs.openSync(file, 'r');
  try {
    return entries(descriptor, file).map((entry) => entry.name);
  } finally {
    fs.closeSync(descriptor);
  }
}

function readZipEntry(file, name) {
  const descriptor = fs.openSync(file, 'r');
  try {
    const entry = entries(descriptor, file).find((candidate) => candidate.name === name);
    if (!entry) throw new Error(`${file} has no ${name}`);
    const data = Buffer.alloc(entry.compressed);
    fs.readSync(descriptor, data, 0, entry.compressed, entry.data);
    if (entry.method === 0) return data;
    if (entry.method === 8) return zlib.inflateRawSync(data);
    throw new Error(`${name} in ${file} uses unsupported compression ${entry.method}`);
  } finally {
    fs.closeSync(descriptor);
  }
}

// Unpacks every entry of `file` into `folder`, streamed, keeping each file's
// mode (an executable stays executable) and making its symbolic links.
async function extractZip(file, folder) {
  const descriptor = fs.openSync(file, 'r');
  try {
    for (const entry of entries(descriptor, file)) {
      const target = path.join(folder, entry.name);
      if (entry.name.endsWith('/')) {
        fs.mkdirSync(target, { recursive: true });
        continue;
      }
      fs.mkdirSync(path.dirname(target), { recursive: true });
      if (![0, 8].includes(entry.method)) throw new Error(`${entry.name} in ${file} uses unsupported compression ${entry.method}`);
      const source = () => fs.createReadStream(null, { fd: descriptor, autoClose: false, start: entry.data, end: entry.data + entry.compressed - 1 });
      if ((entry.mode & S_IFMT) === S_IFLNK) {
        const chunks = [];
        await pipeline(source(), ...(entry.method === 8 ? [zlib.createInflateRaw()] : []), async function* collect(stream) {
          for await (const chunk of stream) chunks.push(chunk);
        });
        fs.symlinkSync(Buffer.concat(chunks).toString('utf8'), target);
        continue;
      }
      if (entry.compressed === 0) fs.writeFileSync(target, '');
      else await pipeline(source(), ...(entry.method === 8 ? [zlib.createInflateRaw()] : []), fs.createWriteStream(target));
      if (entry.mode & 0o777) fs.chmodSync(target, entry.mode & 0o777);
    }
  } finally {
    fs.closeSync(descriptor);
  }
}

module.exports = { zipNames, readZipEntry, extractZip };
