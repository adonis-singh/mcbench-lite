'use strict';

// Opens the app's access link in the default browser once the server answers
// on it: `open` on macOS, `xdg-open` on Linux. ./start runs it beside the
// server with the server's pid, and it stops, opening nothing, when that
// process has exited. The server answers with its page only to a request
// that carries this data folder's token, so another program on the port is
// never taken for it.

const http = require('http');
const { spawn } = require('child_process');

require('../server/dotenv').loadDotEnv();
const auth = require('../server/auth');

const serverPid = Number(process.argv[2]);
const port = Number(process.env.PORT || 3020);
const POLL_MS = 250;

function serverRuns() {
  try {
    process.kill(serverPid, 0);
    return true;
  } catch {
    return false;
  }
}

// Whether the server serves its page to the token: false while nothing listens.
function answers() {
  return new Promise((resolve) => {
    const request = http.get({ host: '127.0.0.1', port, path: '/', headers: { authorization: `Bearer ${auth.token()}` } }, (response) => {
      response.resume();
      resolve(response.statusCode === 200);
    });
    request.on('error', () => resolve(false));
  });
}

async function main() {
  while (serverRuns()) {
    if (await answers()) {
      const opener = process.platform === 'darwin' ? 'open' : 'xdg-open';
      const child = spawn(opener, [auth.link(port)], { stdio: 'ignore', detached: true });
      child.on('error', (error) => console.error(`[start] could not open a browser (${opener}: ${error.message}): open the address above.`));
      child.unref();
      return;
    }
    await new Promise((resolve) => setTimeout(resolve, POLL_MS));
  }
}

main();
