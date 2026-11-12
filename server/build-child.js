'use strict';

// Entry point of the isolated build process launched by server/build-sandbox.js.
// It has no filesystem, network, process, or environment access of its own.
// stdin carries the request and then mc.readFile replies; stdout carries
// mc.readFile requests and finally one result frame. Everything here is
// synchronous, so nothing a program schedules ever runs after it returns.

const fs = require('fs');
const { runProgram } = require('./mc-dsl');
const { decodeWorld, encodeChanges } = require('./build-transfer');

const READ = 1;
const DONE = 2;
const FAIL = 3;
const SETTLED = 4;

function readExact(length) {
  const buffer = Buffer.allocUnsafe(length);
  for (let offset = 0; offset < length;) {
    const n = fs.readSync(0, buffer, offset, length - offset, null);
    if (n === 0) throw new Error('build request stream closed');
    offset += n;
  }
  return buffer;
}

const readFrame = () => readExact(readExact(4).readUInt32LE(0));

function send(type, payload) {
  const head = Buffer.allocUnsafe(5);
  head.writeUInt8(type, 0);
  head.writeUInt32LE(payload.length, 1);
  for (const part of [head, payload]) {
    for (let offset = 0; offset < part.length;) offset += fs.writeSync(1, part, offset);
  }
}

// mc.readFile, served by the parent's symlink-safe /work reader.
function readFromHost(relPath) {
  send(READ, Buffer.from(relPath, 'utf8'));
  const ok = readExact(1)[0] === 1;
  const text = readFrame().toString('utf8');
  if (!ok) throw new Error(text);
  return text;
}

// mc.settled's world, asked for from the parent on the first call.
let settledWorld = null;
function settledFromHost() {
  if (!settledWorld) {
    send(SETTLED, Buffer.alloc(0));
    settledWorld = decodeWorld(readFrame());
  }
  return settledWorld;
}

const request = JSON.parse(readFrame().toString('utf8'));
const candidate = decodeWorld(readFrame());
const paletteStart = candidate.palette.entries.length;
let result;
try {
  result = runProgram(request.source, candidate, request.registry, request.readFile ? readFromHost : null, request.settledTicks === null ? null : { world: settledFromHost, ticks: request.settledTicks });
} catch (err) {
  // The parent rebuilds an error that formats identically: the stack when
  // there is one, otherwise the thrown value's own string form.
  const stack = err && err.stack ? String(err.stack) : '';
  send(FAIL, Buffer.from(JSON.stringify({
    stack,
    message: stack ? String(err.message) : String(err),
    logs: err && typeof err === 'object' && Array.isArray(err.logs) ? err.logs : null,
  }), 'utf8'));
  process.exit(0);
}
const meta = Buffer.from(JSON.stringify(result), 'utf8');
const metaLength = Buffer.allocUnsafe(4);
metaLength.writeUInt32LE(meta.length, 0);
send(DONE, Buffer.concat([metaLength, meta, encodeChanges(candidate, paletteStart)]));
process.exit(0);
