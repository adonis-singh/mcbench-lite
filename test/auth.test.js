'use strict';

const { test } = require('node:test');
const assert = require('node:assert');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');

process.env.MCBENCH_DATA_DIR = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-auth-test-'));
const auth = require('../server/auth');

const crypto = require('node:crypto');

// A server that answers 200 to whoever auth.js lets through, as index.js
// routes. Requests are sent with node:http so each names its own Host.
async function serve(t) {
  const server = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://app.invalid');
    if (url.searchParams.has('token')) return auth.signIn(req, res, url, 4321);
    if (!auth.authorized(req, 4321)) return auth.refuse(req, res);
    res.end('ok');
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => server.close());
  const { port } = server.address();
  return (path, { method = 'GET', ...headers } = {}) => new Promise((resolve, reject) => {
    http.request({ host: '127.0.0.1', port, path, method, headers }, (res) => {
      let body = '';
      res.on('data', (chunk) => { body += chunk; });
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body }));
    }).on('error', reject).end();
  });
}

test('the token is made once, 64 hex digits, readable by its owner alone', () => {
  const token = auth.token();
  assert.match(token, /^[0-9a-f]{64}$/);
  assert.strictEqual(fs.readFileSync(auth.TOKEN_FILE, 'utf8').trim(), token);
  assert.strictEqual(fs.statSync(auth.TOKEN_FILE).mode & 0o777, 0o600);
});

test('the app\'s host name comes from the token, and the link names it', () => {
  const token = auth.token();
  const label = crypto.createHash('sha256').update(`host\n${token}`).digest('hex').slice(0, 10);
  assert.strictEqual(auth.hostName(), `mcbench-${label}.localhost`);
  assert.strictEqual(auth.link(3155), `http://mcbench-${label}.localhost:3155/?token=${token}`);
});

test('the cookie is set and honoured on the app\'s host name alone; a bearer header counts everywhere', async (t) => {
  const send = await serve(t);
  const token = auth.token();
  const named = `${auth.hostName()}:4321`;
  const loopback = '127.0.0.1:4321';
  assert.strictEqual((await send('/api/config', { host: named })).status, 401);
  assert.strictEqual((await send('/', { host: named, accept: 'text/html' })).status, 401);
  assert.strictEqual((await send('/api/config', { host: loopback, authorization: `Bearer ${'0'.repeat(64)}` })).status, 401);
  assert.strictEqual((await send('/api/config', { host: loopback, authorization: `Bearer ${token}` })).status, 200);
  assert.strictEqual((await send('/api/config', { host: named, authorization: `Bearer ${token}` })).status, 200);
  assert.strictEqual((await send('/api/config', { host: named, cookie: `mcbench_token=${'f'.repeat(64)}` })).status, 401);
  // The link on the app's host name sets a host-only cookie and loads the
  // address again without the token.
  const signIn = await send(`/?view=a&token=${token}`, { host: named });
  assert.strictEqual(signIn.status, 303);
  assert.strictEqual(signIn.headers.location, '/?view=a');
  const [cookie] = signIn.headers['set-cookie'];
  assert.strictEqual(cookie, `mcbench_token=${token}; Max-Age=${400 * 24 * 60 * 60}; Path=/; HttpOnly; SameSite=Strict`);
  assert.strictEqual((await send('/api/config', { host: named, cookie: `mcbench_token=${token}` })).status, 200);
  // A cookie another *.localhost page set for the whole domain does not hide it.
  assert.strictEqual((await send('/api/config', { host: named, cookie: `mcbench_token=x; mcbench_token=${token}` })).status, 200);
  // The same cookie on a loopback name is the kind every port receives: refused.
  for (const host of [loopback, 'localhost:4321', '[::1]:4321']) {
    assert.strictEqual((await send('/api/config', { host, cookie: `mcbench_token=${token}` })).status, 401);
  }
  // The link on a loopback name goes on to the app's host name, token kept,
  // and sets no cookie there.
  const old = await send(`/?view=a&token=${token}`, { host: loopback });
  assert.strictEqual(old.status, 303);
  assert.strictEqual(old.headers.location, `http://${named}/?view=a&token=${token}`);
  assert.strictEqual(old.headers['set-cookie'], undefined);
  // A wrong token signs nobody in, and its refusal does not name the app's host.
  for (const host of [named, loopback]) {
    const wrong = await send(`/?token=${'a'.repeat(64)}`, { host, accept: 'text/html' });
    assert.strictEqual(wrong.status, 401);
    assert.strictEqual(wrong.headers.location, undefined);
    assert.doesNotMatch(wrong.body, /mcbench-[0-9a-f]{10}/);
    assert.strictEqual((await send(`/?token=${token}`, { host, method: 'POST' })).status, 401);
  }
});

test('the sign-in link\'s redirect is always a path on the app, however its path is written', async (t) => {
  const send = await serve(t);
  const token = auth.token();
  const named = `${auth.hostName()}:4321`;
  // URL parsing makes the path of each of these `//evil.example/` or
  // `///evil.example/`, which a browser would read as an address on another site.
  for (const written of ['/a/..//evil.example/', '/./\\evil.example/', '/a/..///evil.example/']) {
    const signIn = await send(`${written}?token=${token}`, { host: named });
    assert.strictEqual(signIn.status, 303);
    assert.strictEqual(signIn.headers.location, '/evil.example/', written);
    const old = await send(`${written}?token=${token}`, { host: '127.0.0.1:4321' });
    assert.strictEqual(old.headers.location, `http://${named}/evil.example/?token=${token}`, written);
  }
});

test('a token file that holds no token is refused on every call, and read once it holds one', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-auth-test-'));
  fs.writeFileSync(path.join(dir, 'auth-token'), 'not-a-token\n');
  // A second copy of the module, on a data folder of its own.
  const file = require.resolve('../server/auth');
  const loaded = require.cache[file];
  const dataDir = process.env.MCBENCH_DATA_DIR;
  delete require.cache[file];
  process.env.MCBENCH_DATA_DIR = dir;
  try {
    const other = require('../server/auth');
    assert.throws(() => other.token(), /does not hold an access token/);
    assert.throws(() => other.token(), /does not hold an access token/);
    assert.throws(() => other.authorized({ headers: { authorization: 'Bearer not-a-token' } }, 4321), /does not hold an access token/);
    fs.writeFileSync(other.TOKEN_FILE, `${'a'.repeat(64)}\n`);
    assert.strictEqual(other.token(), 'a'.repeat(64));
  } finally {
    process.env.MCBENCH_DATA_DIR = dataDir;
    require.cache[file] = loaded;
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
