'use strict';

// What every request to the app passes before a route sees it: who may send
// it, the headers every response carries, its URL, and how much of its body
// is read.

const MIB = 1024 * 1024;
// A JSON body is read whole into memory, so each route takes what it needs:
// a message with attachments (200 MiB expands to about 267 MiB in base64, plus
// JSON metadata), the chunk versions a viewer holds, or a small form.
const BODY_LIMITS = { message: 280 * MIB, world: 128 * MIB, form: 1 * MIB };

function httpError(statusCode, message) {
  return Object.assign(new Error(message), { statusCode });
}

// Chunks are joined as bytes before decoding, so a character split across
// two chunks survives.
async function readJsonBody(req, limit = BODY_LIMITS.form) {
  const chunks = [];
  let bytes = 0;
  for await (const chunk of req) {
    bytes += chunk.length;
    if (bytes > limit) throw httpError(413, `Request body exceeds ${limit / MIB} MiB`);
    chunks.push(chunk);
  }
  if (!bytes) return {};
  let body;
  try {
    body = JSON.parse(Buffer.concat(chunks).toString('utf8'));
  } catch (error) {
    throw httpError(400, `Invalid JSON body: ${error.message}`);
  }
  if (!body || typeof body !== 'object' || Array.isArray(body)) throw httpError(400, 'Expected a JSON object');
  return body;
}

// The request's URL; only its path and query are read.
function requestUrl(req) {
  try {
    return new URL(req.url, 'http://app.invalid');
  } catch {
    throw httpError(400, 'Malformed request URL');
  }
}

// Only this machine's own pages may talk to the app: a request must name the
// app's own host (no DNS rebinding), a write must come from its origin (no
// cross-site form posts spending the user's model quota), and a browser's
// request from another site (an image, script or fetch another page embeds,
// or a top-level visit to the API) is refused unless it opens one of the
// app's pages. The hosts are the app's own host name (auth.js) and the
// loopback names programs use.
function localHosts(port, appHostName) {
  return new Set([`${appHostName}:${port}`, `127.0.0.1:${port}`, `localhost:${port}`, `[::1]:${port}`]);
}

function foreignRequest(req, url, hosts) {
  if (!hosts.has(req.headers.host)) return true;
  const write = !['GET', 'HEAD'].includes(req.method);
  if (write && req.headers.origin !== undefined && req.headers.origin !== `http://${req.headers.host}`) return true;
  const site = req.headers['sec-fetch-site'];
  if (site === undefined || site === 'same-origin' || site === 'none') return false;
  return !(req.method === 'GET' && req.headers['sec-fetch-mode'] === 'navigate' && !url.pathname.startsWith('/api/'));
}

// The app's pages load scripts, styles, workers, WebAssembly and images from
// the app alone, and no other site may frame them. A dedicated worker takes
// the policy its script was served with, so every response carries it. The
// BlueMap frame's web app evaluates code (vue-i18n compiles its messages),
// styles its markup inline and draws textures from data: URLs; only the
// app's own pages may frame it. A route may replace the policy (an
// attachment is sandboxed).
const APP_CSP = [
  "default-src 'self'", "script-src 'self' 'wasm-unsafe-eval'", "style-src 'self'", "img-src 'self' data:",
  "connect-src 'self' blob:", "worker-src 'self'", "frame-src 'self'", "object-src 'none'", "base-uri 'none'",
  "form-action 'self'", "frame-ancestors 'none'",
].join('; ');
const BLUEMAP_CSP = [
  "default-src 'self'", "script-src 'self' 'unsafe-eval'", "style-src 'self' 'unsafe-inline'", "img-src 'self' data:",
  "connect-src 'self'", "worker-src 'self'", "object-src 'none'", "base-uri 'none'", "form-action 'none'",
  "frame-ancestors 'self'",
].join('; ');

// Every response, a refused one too: no sniffing; no Referer, so no page
// the app opens or links to learns the app's host name; and no page of
// another origin may load one as an image, script, style or media
// (cross-origin-resource-policy), from the network or from the browser's
// cache. The app's pages, the BlueMap frame and the see_build renderer load
// only from their own origin.
function securityHeaders(res) {
  res.setHeader('x-content-type-options', 'nosniff');
  res.setHeader('referrer-policy', 'no-referrer');
  res.setHeader('cross-origin-resource-policy', 'same-origin');
}

function pagePolicy(res, url) {
  res.setHeader('content-security-policy', url.pathname.startsWith('/bluemap/') ? BLUEMAP_CSP : APP_CSP);
}

module.exports = { BODY_LIMITS, httpError, readJsonBody, requestUrl, localHosts, foreignRequest, securityHeaders, pagePolicy, APP_CSP, BLUEMAP_CSP };
