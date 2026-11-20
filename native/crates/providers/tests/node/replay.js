'use strict';

// The replay server of mcbench-providers' differential test. It plays the
// providers' endpoints from scripted streams and logs every request it
// receives byte for byte, so today's server/llm.js and the Rust providers can
// be run against the same answers and their requests compared.
//
//   node replay.js --log <requests.jsonl>
//
// It listens on 127.0.0.1 port 0 and prints {"port":N} as its first line.
// Nothing here contacts a real provider and every stream is synthetic.
//
// Control requests (not logged):
//   POST /__case   {"id": "...", "routes": {"<METHOD> <path>": [answer, ...]}}
//                  starts a case: its answers are served in order, per route.
//   POST /__end    closes the log and the server.
// Every other request is logged as one JSON line
//   {"case","seq","method","target","version","headers":[[name,value],...],"body"}
// with the Host header's port written as PORT (the two sides run on different
// ports), and answered with the case's next answer for its route:
//   {"status": 200, "headers": [[name, value], ...], "chunks": [chunk, ...],
//    "gzip": false}
// where a chunk is a string written as one write (each write is flushed and
// followed by a short pause, so records arrive split as a network splits
// them), {"bytes": base64} (raw bytes, to split a character between writes),
// {"destroy": true} (the connection is cut there), or {"wait": ms}.
// With "gzip": true the strings are written through one gzip stream (each
// flushed), ending with its whole trailer; with "close": true the body has
// neither content-length nor chunked encoding and ends with the connection.
// A request without an answer gets 599 and is logged with "unscripted": true.

const http = require('node:http');
const fs = require('node:fs');
const zlib = require('node:zlib');

const args = process.argv.slice(2);
const logFile = args[args.indexOf('--log') + 1];
if (!logFile || args.indexOf('--log') < 0) throw new Error('usage: node replay.js --log <requests.jsonl>');
const log = fs.openSync(logFile, 'w');

let current = { id: null, routes: {} };
let seq = 0;
const sockets = new Set();

function readBody(req) {
  return new Promise((resolve, reject) => {
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => resolve(Buffer.concat(chunks)));
    req.on('error', reject);
  });
}

const pause = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function answer(req, res, scripted) {
  // A close-delimited body: no content-length and no chunked encoding, so
  // the end of the body is the end of the connection.
  if (scripted.close) {
    res.useChunkedEncodingByDefault = false;
    res.shouldKeepAlive = false;
  }
  res.writeHead(scripted.status, scripted.headers.flat());
  // Headers go out before the body, as a provider's stream starts.
  res.flushHeaders();
  const gzip = scripted.gzip ? zlib.createGzip() : null;
  if (gzip) gzip.on('data', (data) => res.write(data));
  for (const chunk of scripted.chunks) {
    if (typeof chunk === 'string') {
      if (gzip) {
        gzip.write(chunk);
        await new Promise((resolve) => gzip.flush(zlib.constants.Z_SYNC_FLUSH, resolve));
      } else {
        res.write(chunk);
      }
      await pause(2);
    } else if (chunk.bytes !== undefined) {
      res.write(Buffer.from(chunk.bytes, 'base64'));
      await pause(2);
    } else if (chunk.destroy) {
      await pause(5);
      req.socket.destroy();
      return;
    } else if (chunk.wait !== undefined) {
      await pause(chunk.wait);
    } else {
      throw new Error(`unknown chunk ${JSON.stringify(chunk)}`);
    }
  }
  if (gzip) {
    // 'end' comes after the last 'data' (the final block and the trailer);
    // gzip.end's callback ('finish') can come before it, which cut the
    // trailer off about one answer in three.
    const ended = new Promise((resolve) => gzip.on('end', resolve));
    gzip.end();
    await ended;
  }
  res.end();
}

const server = http.createServer(async (req, res) => {
  const body = await readBody(req);
  if (req.url === '/__case' && req.method === 'POST') {
    current = JSON.parse(body.toString('utf8'));
    seq = 0;
    res.end('{}');
    return;
  }
  if (req.url === '/__end' && req.method === 'POST') {
    res.end('{}');
    fs.closeSync(log);
    server.close();
    for (const socket of sockets) socket.destroy();
    return;
  }
  const headers = [];
  for (let i = 0; i < req.rawHeaders.length; i += 2) {
    const name = req.rawHeaders[i];
    const value = name.toLowerCase() === 'host' ? req.rawHeaders[i + 1].replace(/:\d+$/, ':PORT') : req.rawHeaders[i + 1];
    headers.push([name, value]);
  }
  const route = `${req.method} ${req.url}`;
  const queue = current.routes[route];
  const scripted = queue && queue.shift();
  const text = body.toString('utf8');
  const entry = {
    case: current.id,
    seq: seq++,
    method: req.method,
    target: req.url,
    version: req.httpVersion,
    headers,
    body: Buffer.from(text, 'utf8').equals(body) ? text : { base64: body.toString('base64') },
    ...(scripted ? {} : { unscripted: true }),
  };
  fs.writeSync(log, `${JSON.stringify(entry)}\n`);
  if (!scripted) {
    res.writeHead(599, { 'content-type': 'text/plain' });
    res.end(`no scripted answer for ${route} in case ${current.id}`);
    return;
  }
  try {
    await answer(req, res, scripted);
  } catch (error) {
    console.error(error);
    req.socket.destroy();
  }
});
server.on('connection', (socket) => {
  sockets.add(socket);
  socket.on('close', () => sockets.delete(socket));
});
server.keepAliveTimeout = 60_000;
server.listen(0, '127.0.0.1', () => {
  process.stdout.write(`${JSON.stringify({ port: server.address().port })}\n`);
});
