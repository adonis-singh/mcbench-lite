// The see_build page's script (web/shot.html), driven by server/shot.js
// through window.__shot.
import { createViewer } from './viewer.js';

const canvas = document.getElementById('c');
// The shot as returned: the WebGL canvas drawn at `supersample` times the
// size, scaled down here, with the grid drawn over it.
const output = document.createElement('canvas');
// JPEG: a large shot as PNG runs to megabytes, and every request carries
// the latest screenshot batches.
const JPEG_QUALITY = 0.9;
let viewer = null;
// The loaded world persists between calls: the server sends only chunks
// whose version differs from what we hold, so a see_build after a small
// edit, or a clip box that keeps whole chunks, remeshes few chunks.
let loadedId = null;
let known = {}; // chunk key -> version
let renderLod = null;

function requireContext() {
  const gl = canvas.getContext('webgl2');
  if (!gl || gl.isContextLost()) throw new Error('Screenshot WebGL context lost');
  return gl;
}

// Block edges (every 1 block, stronger every labelled step) and coordinate
// labels along the top and left edges, with the axes in the corner.
function drawGrid(context, grid, width, height) {
  context.save();
  for (const { at, major } of grid.columns.edges) {
    context.strokeStyle = major ? 'rgba(0, 0, 0, 0.55)' : 'rgba(0, 0, 0, 0.22)';
    context.lineWidth = major ? 2 : 1;
    context.beginPath();
    context.moveTo(Math.round(at) + 0.5, 0);
    context.lineTo(Math.round(at) + 0.5, height);
    context.stroke();
  }
  for (const { at, major } of grid.rows.edges) {
    context.strokeStyle = major ? 'rgba(0, 0, 0, 0.55)' : 'rgba(0, 0, 0, 0.22)';
    context.lineWidth = major ? 2 : 1;
    context.beginPath();
    context.moveTo(0, Math.round(at) + 0.5);
    context.lineTo(width, Math.round(at) + 0.5);
    context.stroke();
  }
  context.font = 'bold 20px sans-serif';
  context.textBaseline = 'middle';
  const label = (text, x, y, align) => {
    context.textAlign = align;
    const w = context.measureText(text).width + 8;
    const left = align === 'center' ? x - w / 2 : x;
    context.fillStyle = 'rgba(255, 255, 255, 0.85)';
    context.fillRect(left, y - 13, w, 26);
    context.fillStyle = '#000';
    context.fillText(text, align === 'center' ? x : x + 4, y);
  };
  for (const { at, text } of grid.columns.labels) if (at > 20 && at < width - 20) label(text, at, 16, 'center');
  for (const { at, text } of grid.rows.labels) if (at > 40 && at < height - 20) label(text, 2, at, 'left');
  label(grid.legend, width - context.measureText(grid.legend).width - 20, height - 18, 'left');
  context.restore();
}

window.__shot = {
  async init() {
    if (!viewer) {
      try {
        viewer = await createViewer(canvas);
      } catch (error) {
        // Context loss during shader setup otherwise looks like a shader
        // compilation error, and still needs a completely fresh renderer.
        requireContext();
        throw error;
      }
    }
    requireContext();
  },

  async load(conversationId, clip, tick) {
    requireContext();
    if (conversationId !== loadedId) {
      viewer.reset();
      known = {};
      renderLod = null;
      loadedId = conversationId;
    }
    const res = await fetch(`/api/conversations/${conversationId}/world`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ known, renderLod, engine: true, clip, tick }),
    });
    if (!res.ok) throw new Error(`World load failed (HTTP ${res.status})`);
    const reader = res.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    let meta = null;
    for (;;) {
      const { value, done } = await reader.read();
      buffer += decoder.decode(value || new Uint8Array(), { stream: !done });
      const lines = buffer.split('\n');
      buffer = lines.pop();
      for (const line of lines) {
        if (!line) continue;
        const event = JSON.parse(line);
        if (event.type === 'meta') {
          meta = event;
          if (JSON.stringify(event.renderLod) !== JSON.stringify(renderLod)) {
            viewer.reset();
            known = {};
            renderLod = event.renderLod;
          }
          viewer.applyDelta({ chunks: [], removed: event.removed });
          for (const key of event.removed) delete known[key];
        } else if (event.type === 'chunk') {
          viewer.applyDelta({ chunks: [event.chunk], removed: [] });
          known[event.chunk.key] = event.chunk.version;
        }
      }
      if (done) break;
    }
    viewer.applyDelta({ bounds: meta.bounds, chunks: [], removed: [] });
  },

  settle() {
    requireContext();
    viewer.flush();
    requireContext();
    return viewer.pending();
  },

  // The shots of one world, in one task: each JPEG data URL.
  frames(specs) {
    return specs.map((spec) => this.frame(spec));
  },

  frame({ camera, width, height, supersample, grid }) {
    requireContext();
    if (canvas.width !== width * supersample || canvas.height !== height * supersample) {
      canvas.width = width * supersample;
      canvas.height = height * supersample;
    }
    viewer.setCamera(camera);
    viewer.renderNow();
    // Read the drawing buffer in the same task as the draw. A later browser
    // screenshot can capture a discarded buffer or a lost-context placeholder.
    requireContext();
    output.width = width;
    output.height = height;
    const context = output.getContext('2d');
    context.imageSmoothingEnabled = true;
    context.imageSmoothingQuality = 'high';
    context.drawImage(canvas, 0, 0, width, height);
    requireContext();
    if (grid) drawGrid(context, grid, width, height);
    return output.toDataURL('image/jpeg', JPEG_QUALITY);
  },
};
