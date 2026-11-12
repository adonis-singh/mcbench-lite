// Runs web/core-worker.mjs in a Node worker thread: the page's Web Worker
// API (self.postMessage, self.onmessage, fetch of a module) over
// worker_threads.
import fs from 'node:fs';
import { parentPort } from 'node:worker_threads';
import { fileURLToPath } from 'node:url';

globalThis.self = { postMessage: (message, transfer) => parentPort.postMessage(message, transfer), onmessage: null };
parentPort.on('message', (data) => globalThis.self.onmessage({ data }));
globalThis.fetch = async (url) => {
  const bytes = fs.readFileSync(fileURLToPath(url));
  return { ok: true, arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length) };
};
await import('../web/core-worker.mjs');
