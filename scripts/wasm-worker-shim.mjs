// Runs web/core-worker.mjs in a Node worker thread: the page's Web Worker
// API (self.postMessage, self.onmessage) over worker_threads.
import { parentPort } from 'node:worker_threads';

globalThis.self = { postMessage: (message, transfer) => parentPort.postMessage(message, transfer), onmessage: null };
parentPort.on('message', (data) => globalThis.self.onmessage({ data }));
await import('../web/core-worker.mjs');
