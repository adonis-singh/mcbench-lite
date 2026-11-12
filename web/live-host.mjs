// What the page gives a LiveWorld (live-world.mjs): the server over fetch, the
// engine in a Web Worker, blobs for blueprints.

import { CoreEngine } from './core-engine.mjs';

async function checked(response) {
  if (response.ok) return response;
  let message = `HTTP ${response.status}`;
  try {
    message = (await response.json()).error || message;
  } catch {
    // Not a JSON error: the status says it.
  }
  throw new Error(message);
}

function init({ method = 'GET', body } = {}) {
  return body === undefined ? { method } : { method, headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) };
}

export function browserHost() {
  return {
    async json(path, options) {
      return (await checked(await fetch(path, init(options)))).json();
    },
    startEngine: (wasm) => CoreEngine.start(wasm),
    blobUrl: (bytes) => URL.createObjectURL(new Blob([bytes])),
    revoke: (url) => URL.revokeObjectURL(url),
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  };
}
