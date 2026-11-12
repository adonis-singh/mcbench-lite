'use strict';

// How server/index.js serves a chat's attachments and images, through its
// real request path (route) over HTTP: only types a browser shows and never
// runs keep their type, every other file is a download, no page may frame
// one, and every load is checked with the server, never answered from the
// browser's cache past the access check.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { after, before, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-attachment-serving-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
// The Host the requests name: the server answers only its own port's names.
process.env.PORT = '4321';
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
const store = require('../server/store');
const auth = require('../server/auth');
const app = require('../server/index');

let server;
let port;
before(async () => {
  server = http.createServer((req, res) => app.route(req, res));
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  ({ port } = server.address());
});
after(() => {
  server.close();
  fs.rmSync(dataDir, { recursive: true, force: true });
});

function get(pathname, headers = {}) {
  return new Promise((resolve, reject) => {
    http.get({ host: '127.0.0.1', port, path: pathname, headers: { host: '127.0.0.1:4321', authorization: `Bearer ${auth.token()}`, ...headers } }, (res) => {
      const chunks = [];
      res.on('data', (chunk) => chunks.push(chunk));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body: Buffer.concat(chunks) }));
    }).on('error', reject);
  });
}

const data = (type, text) => `data:${type};base64,${Buffer.from(text).toString('base64')}`;

test('attachments of a type a browser shows keep it; scripts, pages and the rest are downloads of bytes', async () => {
  const chat = store.createConversation('openrouter', 'test/model');
  const files = [
    ['shot.png', 'image/png', 'png bytes', 'image/png', 'inline'],
    ['photo.JPEG', 'image/jpeg', 'jpeg bytes', 'image/jpeg', 'inline'],
    ['plan.svg', 'image/svg+xml', '<svg xmlns="http://www.w3.org/2000/svg"/>', 'image/svg+xml', 'inline'],
    ['spec.pdf', 'application/pdf', '%PDF-1.7', 'application/pdf', 'inline'],
    ['notes.txt', 'text/plain', 'notes', 'text/plain; charset=utf-8', 'inline'],
    ['readme.md', 'text/markdown', '# notes', 'text/markdown; charset=utf-8', 'inline'],
    ['table.csv', 'text/csv', 'a,b', 'text/csv; charset=utf-8', 'inline'],
    ['gadget.js', 'text/javascript', 'window.GADGET=1', 'application/octet-stream', 'attachment'],
    ['module.mjs', 'text/javascript', 'export {}', 'application/octet-stream', 'attachment'],
    ['page.html', 'text/html', '<script>parent.PWN=1</script>', 'application/octet-stream', 'attachment'],
    ['style.css', 'text/css', 'body{}', 'application/octet-stream', 'attachment'],
    ['data.json', 'application/json', '{}', 'application/octet-stream', 'attachment'],
    ['build.schem', 'application/octet-stream', 'nbt', 'application/octet-stream', 'attachment'],
  ];
  const saved = store.saveAttachments(chat.id, files.map(([name, type, text]) => ({ name, data: data(type, text) })));
  for (const [i, [name, , text, type, disposition]] of files.entries()) {
    const response = await get(saved[i].url);
    assert.equal(response.status, 200, name);
    assert.equal(response.headers['content-type'], type, name);
    assert.equal(response.headers['content-disposition'], `${disposition}; filename="${path.basename(saved[i].url)}"`, name);
    assert.equal(response.headers['content-security-policy'], "sandbox; frame-ancestors 'none'", name);
    assert.equal(response.headers['cache-control'], 'private, no-cache', name);
    assert.equal(response.headers['x-content-type-options'], 'nosniff', name);
    assert.equal(response.headers['cross-origin-resource-policy'], 'same-origin', name);
    assert.equal(response.body.toString(), text, name);
  }
  // Revalidation is a 304 that still carries the attachment's policy.
  const first = await get(saved[0].url);
  const again = await get(saved[0].url, { 'if-none-match': first.headers.etag });
  assert.equal(again.status, 304);
  assert.equal(again.headers['content-security-policy'], "sandbox; frame-ancestors 'none'");
  assert.equal(again.headers['cache-control'], 'private, no-cache');
  // Without the token there is nothing, cached or not.
  assert.equal((await get(saved[0].url, { authorization: '' })).status, 401);
});

test('a chat\'s images are checked with the server on every load, and an unchanged one is a 304', async () => {
  const chat = store.createConversation('openrouter', 'test/model');
  const [url] = store.saveImages(chat.id, [Buffer.from('png bytes')]);
  const response = await get(url);
  assert.equal(response.status, 200);
  assert.equal(response.headers['content-type'], 'image/png');
  assert.equal(response.headers['cache-control'], 'private, no-cache');
  assert.equal(response.body.toString(), 'png bytes');
  const again = await get(url, { 'if-none-match': response.headers.etag });
  assert.equal(again.status, 304);
});

test('a client that sends its request slowly is cut off; the response is not limited', () => {
  // Both limits count receiving the request: headers within 10 seconds, the
  // whole request (a message of 200 MiB of attachments at most) within 2
  // minutes. A turn's response streams for as long as the turn runs.
  assert.equal(app.server.headersTimeout, 10_000);
  assert.equal(app.server.requestTimeout, 120_000);
  assert.equal(app.server.timeout, 0, 'no limit on an open response');
});
