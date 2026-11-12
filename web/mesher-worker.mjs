// The viewer's mesher, off BlueMap's main thread (see MesherClient in
// bluemap-tiles.mjs): one worker holds the world, the others draw from its
// snapshots. Each loads the module and the pack once (their addresses name
// one version each), then takes one request at a time, in order:
//   { id, type: 'load', wasm, pack, blueprint, mask, compare }
//       reads the view's blueprint as it downloads and makes it the world
//       to draw; answers { id, tiles, changed } (see Mesher.finish)
//   { id, type: 'patch', patch }
//       applies a patch (an ArrayBuffer, transferred; see Mesher.patch) to
//       the current world; answers { id, tiles, changed }
//   { id, type: 'snapshot', x, z }
//       what tile x, z reads of the current world (Mesher.snapshot);
//       answers { id, bytes } with the snapshot's buffer
//   { id, type: 'prepare', wasm, pack }
//       loads the module and the pack; answers { id }
// A load whose module and pack are new keeps the world held until the new
// one is read whole; with the same ones, the world is replaced as it streams.
//   { id, type: 'draw', wasm, pack, snapshot }
//       draws a snapshot's tile (an ArrayBuffer, transferred), indexed for
//       the GPU (Mesher.drawSnapshot); answers { id, bytes } with the tile's buffer
// A failure answers { id, error }, with `broken` when the module crashed (a
// WebAssembly trap): it cannot take another request, and the worker is ended.
// An address ending in .gz is gzip, read as it streams: the shared viewer's
// files (server/share-viewer.js), as GitHub Pages compresses no binary file.
import { Mesher } from './mesher.mjs';

let mesher = null;
let loaded = null; // `${wasm} ${pack}` of the mesher

async function body(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url} is unavailable (HTTP ${response.status})`);
  return url.endsWith('.gz') ? response.body.pipeThrough(new DecompressionStream('gzip')) : response.body;
}

async function bytes(url) {
  return new Uint8Array(await new Response(await body(url)).arrayBuffer());
}

// The mesher of the module and pack: the one held, or a new one, which holds
// no world yet. The one held stays until its successor is ready.
async function mesherOf(wasm, pack) {
  if (loaded === `${wasm} ${pack}`) return mesher;
  const [module, baked] = await Promise.all([bytes(wasm), bytes(pack)]);
  return Mesher.create(module, baked);
}

function hold(next, wasm, pack) {
  mesher = next;
  loaded = `${wasm} ${pack}`;
}

async function handle(request) {
  if (request.type === 'load') {
    const next = await mesherOf(request.wasm, request.pack);
    const stream = await body(request.blueprint);
    next.begin(request.mask);
    for (const reader = stream.getReader(); ;) {
      const { done, value } = await reader.read();
      if (done) break;
      next.take(value);
    }
    const { tiles, changed } = next.finish(request.compare);
    hold(next, request.wasm, request.pack);
    self.postMessage({ id: request.id, tiles, changed });
    return;
  }
  if (request.type === 'prepare' || request.type === 'draw') {
    hold(await mesherOf(request.wasm, request.pack), request.wasm, request.pack);
    if (request.type === 'prepare') return self.postMessage({ id: request.id });
    const tile = mesher.drawSnapshot(new Uint8Array(request.snapshot));
    self.postMessage({ id: request.id, bytes: tile.buffer }, [tile.buffer]);
    return;
  }
  if (!mesher) throw new Error('No world is loaded');
  if (request.type === 'patch') {
    const { tiles, changed } = mesher.patch(new Uint8Array(request.patch));
    self.postMessage({ id: request.id, tiles, changed });
    return;
  }
  const snapshot = mesher.snapshot(request.x, request.z);
  self.postMessage({ id: request.id, bytes: snapshot.buffer }, [snapshot.buffer]);
}

let queue = Promise.resolve();
self.onmessage = ({ data }) => {
  queue = queue.then(() => handle(data)).catch((error) => {
    const broken = error instanceof WebAssembly.RuntimeError;
    self.postMessage({ id: data.id, error: broken ? `The mesher crashed (${error.message})` : String(error.message || error), broken });
  });
};
