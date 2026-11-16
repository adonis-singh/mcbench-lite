// What the page gives a LiveWorld (live-world.mjs): the server over fetch, the
// engine in a Web Worker, blobs for blueprints, and the views' last fresh
// placements in IndexedDB.

import { CoreEngine } from './core-engine.mjs';

// The placements kept, the least recently kept let go once they hold more
// than this many bytes.
const PLACEMENT_BYTES = 256 * 2 ** 20;

const settled = (request) => new Promise((resolve, reject) => {
  request.onsuccess = () => resolve(request.result);
  request.onerror = () => reject(request.error);
});

// Each view's last fresh placement, across page loads: { core, digest,
// bytes } by view key. Their sizes and times ('kept') are apart from
// their bytes, so letting the oldest go reads no blueprint.
function placementStore() {
  const opening = new Promise((resolve, reject) => {
    const request = indexedDB.open('mcbench-placements', 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore('kept', { keyPath: 'key' });
      request.result.createObjectStore('bytes');
    };
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
  return {
    async get(key) {
      const db = await opening;
      const tx = db.transaction(['kept', 'bytes']);
      const [entry, bytes] = await Promise.all([settled(tx.objectStore('kept').get(key)), settled(tx.objectStore('bytes').get(key))]);
      return entry ? { ...entry, bytes } : null;
    },
    async put(key, { core, digest, bytes }) {
      const db = await opening;
      const tx = db.transaction(['kept', 'bytes'], 'readwrite');
      const kept = tx.objectStore('kept');
      kept.put({ key, core, digest, size: bytes.length, kept: Date.now() });
      tx.objectStore('bytes').put(bytes, key);
      const all = (await settled(kept.getAll())).sort((a, b) => a.kept - b.kept);
      let total = all.reduce((sum, entry) => sum + entry.size, 0);
      for (const entry of all) {
        if (total <= PLACEMENT_BYTES || entry.key === key) continue;
        kept.delete(entry.key);
        tx.objectStore('bytes').delete(entry.key);
        total -= entry.size;
      }
      await new Promise((resolve, reject) => {
        tx.oncomplete = resolve;
        tx.onerror = () => reject(tx.error);
        tx.onabort = () => reject(tx.error);
      });
    },
  };
}

async function digest(text) {
  const hash = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, '0')).join('');
}

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
    startEngine: (address) => CoreEngine.start(address),
    blobUrl: (bytes) => URL.createObjectURL(new Blob([bytes])),
    revoke: (url) => URL.revokeObjectURL(url),
    sleep: (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
    digest,
    placements: placementStore(),
  };
}
