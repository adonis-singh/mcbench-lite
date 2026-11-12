'use strict';

const assert = require('node:assert/strict');
const http = require('node:http');
const net = require('node:net');
const { Readable } = require('node:stream');
const test = require('node:test');
const { BODY_LIMITS, readJsonBody, requestUrl, localHosts, foreignRequest, securityHeaders, pagePolicy, APP_CSP, BLUEMAP_CSP } = require('../server/http-guard');

const hosts = localHosts(3020, 'mcbench-0123456789.localhost');
const request = (method, path, headers = {}) => ({ method, url: path, headers: { host: '127.0.0.1:3020', ...headers } });
const foreign = (req) => foreignRequest(req, requestUrl(req), hosts);

test('only this app on this machine gets through', () => {
  // DNS rebinding: another name for the same address.
  assert.equal(foreign(request('GET', '/api/settings', { host: 'evil.example:3020' })), true);
  // A cross-site form post, and a page on another local port (same site).
  assert.equal(foreign(request('POST', '/api/conversations', { origin: 'https://evil.example' })), true);
  assert.equal(foreign(request('PUT', '/api/settings', { origin: 'http://localhost:8080', 'sec-fetch-site': 'same-site' })), true);
  // An image or script tag another site embeds, and a link to the API.
  assert.equal(foreign(request('GET', '/api/settings?refresh=1', { 'sec-fetch-site': 'cross-site', 'sec-fetch-mode': 'no-cors' })), true);
  assert.equal(foreign(request('GET', '/app.js', { 'sec-fetch-site': 'cross-site', 'sec-fetch-mode': 'no-cors' })), true);
  assert.equal(foreign(request('GET', '/api/settings', { 'sec-fetch-site': 'cross-site', 'sec-fetch-mode': 'navigate' })), true);
  // Another *.localhost name on the app's port.
  assert.equal(foreign(request('GET', '/', { host: 'mcbench-wrongname.localhost:3020' })), true);
  // The app's own requests, a link to the app, and a typed address.
  assert.equal(foreign(request('POST', '/api/conversations', { host: 'mcbench-0123456789.localhost:3020', origin: 'http://mcbench-0123456789.localhost:3020', 'sec-fetch-site': 'same-origin' })), false);
  assert.equal(foreign(request('POST', '/api/conversations', { origin: 'http://127.0.0.1:3020', 'sec-fetch-site': 'same-origin' })), false);
  assert.equal(foreign(request('GET', '/', { 'sec-fetch-site': 'cross-site', 'sec-fetch-mode': 'navigate' })), false);
  assert.equal(foreign(request('GET', '/', { 'sec-fetch-site': 'none', 'sec-fetch-mode': 'navigate' })), false);
  // A client that is not a browser sends neither header (the app's own
  // scripts, curl): only its Host is checked. See SECURITY.md.
  assert.equal(foreign(request('POST', '/api/conversations')), false);
});

test('every response carries nosniff, no Referer, a same-origin resource policy and the policy for its page', () => {
  const headers = (path) => {
    const set = new Map();
    const res = { setHeader: (name, value) => set.set(name, value) };
    securityHeaders(res);
    pagePolicy(res, requestUrl(request('GET', path)));
    return Object.fromEntries(set);
  };
  const always = { 'x-content-type-options': 'nosniff', 'referrer-policy': 'no-referrer', 'cross-origin-resource-policy': 'same-origin' };
  assert.deepEqual(headers('/'), { ...always, 'content-security-policy': APP_CSP });
  assert.deepEqual(headers('/bluemap/abc/index.html'), { ...always, 'content-security-policy': BLUEMAP_CSP });
  assert.match(APP_CSP, /frame-ancestors 'none'/);
  assert.match(BLUEMAP_CSP, /frame-ancestors 'self'/);
  assert.doesNotMatch(APP_CSP, /'unsafe-inline'|'unsafe-eval'/);
});

// A request line Node's parser accepts but URL cannot parse used to throw
// inside the request listener, which ended the server.
test('a malformed request URL is refused, and the server keeps serving', async () => {
  const server = http.createServer((req, res) => {
    Promise.resolve().then(() => requestUrl(req)).then(
      () => { res.writeHead(200); res.end('ok'); },
      (error) => { res.writeHead(error.statusCode); res.end(error.message); },
    );
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address();
  try {
    const raw = await new Promise((resolve, reject) => {
      const socket = net.connect(port, '127.0.0.1', () => socket.end('GET http://[ HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n'));
      let text = '';
      socket.on('data', (chunk) => { text += chunk; });
      socket.on('end', () => resolve(text));
      socket.on('error', reject);
    });
    assert.match(raw, /^HTTP\/1\.1 400 [^]*\r\nMalformed request URL\r\n/);
    const ok = await fetch(`http://127.0.0.1:${port}/`);
    assert.equal(await ok.text(), 'ok');
  } finally {
    server.close();
  }
});

test('a JSON body is read up to its route\'s limit and no further', async () => {
  const body = (text) => Object.assign(Readable.from([Buffer.from(text)]), { headers: {} });
  assert.deepEqual(await readJsonBody(body('{"a":1}')), { a: 1 });
  const big = JSON.stringify({ text: 'x'.repeat(BODY_LIMITS.form) });
  await assert.rejects(readJsonBody(body(big)), (error) => error.statusCode === 413 && /exceeds 1 MiB/.test(error.message));
  assert.equal((await readJsonBody(body(big), BODY_LIMITS.message)).text.length, BODY_LIMITS.form);
  await assert.rejects(readJsonBody(body('[1]')), (error) => error.statusCode === 400);
  await assert.rejects(readJsonBody(body('{')), (error) => error.statusCode === 400);
});
