'use strict';

// The page's addresses (web/address.mjs) through server/index.js's real
// request path: each is the page, files are themselves, anything else is
// not found, and the sign-in link keeps the address it was opened on.

const assert = require('node:assert/strict');
const fs = require('node:fs');
const http = require('node:http');
const os = require('node:os');
const path = require('node:path');
const { after, before, test } = require('node:test');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-page-addresses-'));
process.env.MCBENCH_DATA_DIR = dataDir;
process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
process.env.PORT = '4321';
require('../server/assets').getBlockRegistry = async () => ({ stone: [{}, {}] });
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
      let body = '';
      res.on('data', (chunk) => { body += chunk; });
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, body }));
    }).on('error', reject);
  });
}

test('every address of the page is the page; its files are themselves and anything else is not found', async () => {
  const page = fs.readFileSync(path.join(__dirname, '..', 'web', 'index.html'), 'utf8');
  for (const address of ['/', '/new', '/chat/0521fc61-aa65-48dd-b473-cced680b0b49', '/bench', '/bench/b1', '/bench/b1/m1', '/leaderboard', '/prompts']) {
    const response = await get(address);
    assert.deepEqual([address, response.status, response.headers['content-type']], [address, 200, 'text/html; charset=utf-8']);
    assert.equal(response.body, page);
  }
  assert.match((await get('/app.js')).headers['content-type'], /javascript/);
  for (const address of ['/chat', '/chat/c1/x', '/nowhere', '/chat/%E0%A4%A']) assert.equal((await get(address)).status, 404, address);
  // The page is the app's like any of its files: no token, no page.
  assert.equal((await get('/chat/c1', { authorization: '', accept: 'text/html' })).status, 401);
});

test('the sign-in link opened on a chat\'s address lands there without the token', async () => {
  const named = `${auth.hostName()}:4321`;
  const signIn = await get(`/chat/c1?token=${auth.token()}`, { host: named, authorization: '' });
  assert.equal(signIn.status, 303);
  assert.equal(signIn.headers.location, '/chat/c1');
});
