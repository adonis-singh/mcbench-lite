'use strict';

// A page in headless Chrome that runs the WebAssembly engine the way the
// app will: core-engine.mjs on the page, the engine in a module Web Worker.
// A small server gives it web/ (at /web/), the module (at /core.wasm, as
// application/wasm) and the files a script adds. Chrome is the one in
// /Applications (override with MCBENCH_CHROME).

const fs = require('fs');
const http = require('http');
const path = require('path');

const WEB = path.join(__dirname, '..', 'web');
const CHROME = process.env.MCBENCH_CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

// `script` is the page's module script; `files` maps urls to [file, type].
async function openPage(wasmFile, script, files = {}) {
  const { chromium } = require('playwright-core');
  const served = { '/core.wasm': [wasmFile, 'application/wasm'], ...files };
  const server = http.createServer((request, response) => {
    const url = request.url.split('?')[0];
    if (url === '/') return response.writeHead(200, { 'content-type': 'text/html' }).end(`<!doctype html><meta charset=utf-8><script type=module>${script}</script>`);
    const file = url.startsWith('/web/') ? [path.join(WEB, url.slice('/web/'.length)), 'text/javascript'] : served[url];
    if (!file || !fs.existsSync(file[0])) return response.writeHead(404).end();
    response.writeHead(200, { 'content-type': file[1] }).end(fs.readFileSync(file[0]));
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ executablePath: CHROME, headless: true });
  const page = await browser.newPage();
  page.on('pageerror', (error) => console.error('page error', error));
  await page.goto(`http://127.0.0.1:${server.address().port}/`);
  return { page, close: async () => { await browser.close(); server.close(); } };
}

module.exports = { openPage };
