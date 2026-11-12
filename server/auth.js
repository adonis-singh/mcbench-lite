'use strict';

// Who may use the app: whoever holds its access token. The token is made once
// per data folder (data/auth-token, readable by its owner alone) and printed
// with the app's address when the server starts. Every request needs the
// token: no other account or process on the machine can read chats, spend the
// providers' keys or change settings.
//
// Browsers send a cookie to every port of its host name, so a cookie set on
// 127.0.0.1 reaches any other server on 127.0.0.1. The app's pages therefore
// live on a host name of their own, mcbench-<label>.localhost (browsers and
// macOS resolve every *.localhost name to loopback), where <label> comes from
// the token and cannot be guessed. The cookie is set and read on that host
// alone. On 127.0.0.1, localhost and [::1] only `Authorization: Bearer
// <token>` counts (scripts, tests, the see_build renderer); the sign-in link
// opened there is sent on to the named host.

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');

const DATA_DIR = path.resolve(process.env.MCBENCH_DATA_DIR || path.join(__dirname, '..', 'data'));
const TOKEN_FILE = path.join(DATA_DIR, 'auth-token');
// The browser keeps the cookie as long as it allows (400 days in Chromium).
const COOKIE_SECONDS = 400 * 24 * 60 * 60;

let cached = null;

// The data folder's token, made (32 random bytes, hex) the first time.
function token() {
  if (cached) return cached;
  fs.mkdirSync(DATA_DIR, { recursive: true, mode: 0o700 });
  try {
    fs.writeFileSync(TOKEN_FILE, `${crypto.randomBytes(32).toString('hex')}\n`, { flag: 'wx', mode: 0o600 });
  } catch (error) {
    if (error.code !== 'EEXIST') throw error;
  }
  // Checked before it is kept: a file that holds no token is refused on every call.
  const held = fs.readFileSync(TOKEN_FILE, 'utf8').trim();
  if (!/^[0-9a-f]{64}$/.test(held)) throw new Error(`${TOKEN_FILE} does not hold an access token (64 hex digits)`);
  cached = held;
  return cached;
}

// The app's own host name: 10 hex digits of sha256("host\n" + token), which
// do not give the token back.
function hostName() {
  return `mcbench-${crypto.createHash('sha256').update(`host\n${token()}`).digest('hex').slice(0, 10)}.localhost`;
}

const appHost = (port) => `${hostName()}:${port}`;

// The address that signs a browser in (npm run url prints it).
function link(port) {
  return `http://${appHost(port)}/?token=${token()}`;
}

const COOKIE = 'mcbench_token';

function matches(candidate) {
  const expected = Buffer.from(token());
  const given = Buffer.from(String(candidate));
  return given.length === expected.length && crypto.timingSafeEqual(given, expected);
}

// Every value of the cookies named COOKIE: a page on another *.localhost name
// can set one for the whole localhost domain, which must not hide the app's.
function cookies(req) {
  const values = [];
  for (const part of (req.headers.cookie || '').split(';')) {
    const at = part.indexOf('=');
    if (at !== -1 && part.slice(0, at).trim() === COOKIE) values.push(part.slice(at + 1).trim());
  }
  return values;
}

// A bearer header counts on every host the app answers; the cookie only on
// the app's own host name.
function authorized(req, port) {
  const bearer = /^Bearer (\S+)$/.exec(req.headers.authorization || '');
  if (bearer) return matches(bearer[1]);
  if (req.headers.host !== appHost(port)) return false;
  return cookies(req).some(matches);
}

// /?token=... on the app's host name: the host-only cookie is set and the
// address loads again without the token, so it is neither bookmarked nor
// left in the history. On any other host the app answers (127.0.0.1,
// localhost) it goes on to the same address on the app's host name, token
// included, and sets nothing.
// The path's leading slashes are collapsed to one: `/a/..//evil.example/`
// comes out of URL parsing as `//evil.example/`, which a browser reads as
// another site's address.
function signIn(req, res, url, port) {
  if (req.method !== 'GET' || !matches(url.searchParams.get('token'))) return refuse(req, res);
  const pathname = url.pathname.replace(/^\/+/, '/');
  if (req.headers.host !== appHost(port)) {
    res.writeHead(303, { location: `http://${appHost(port)}${pathname}${url.search}`, 'cache-control': 'no-store' });
    return res.end();
  }
  url.searchParams.delete('token');
  res.writeHead(303, {
    location: `${pathname}${url.search}`,
    'set-cookie': `${COOKIE}=${token()}; Max-Age=${COOKIE_SECONDS}; Path=/; HttpOnly; SameSite=Strict`,
    'cache-control': 'no-store',
  });
  res.end();
}

const REFUSED = 'mcbench-lite needs its access link: open the address the server printed when it started (it ends in ?token=...), or print it with npm run url.';

function refuse(req, res) {
  if ((req.headers.accept || '').includes('text/html')) {
    res.writeHead(401, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
    return res.end(`<!doctype html><meta charset="utf-8"><title>mcbench-lite</title><p>${REFUSED}</p>`);
  }
  res.writeHead(401, { 'content-type': 'application/json', 'cache-control': 'no-store' });
  res.end(JSON.stringify({ error: REFUSED }));
}

module.exports = { token, hostName, link, authorized, signIn, refuse, COOKIE, TOKEN_FILE };
