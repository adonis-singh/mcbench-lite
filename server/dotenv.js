'use strict';

// Sets each variable `.env` at the repository root names that the
// environment does not already set. The server and `npm run url` call it
// before any module reads a setting, so both see the same port and data folder.

const fs = require('fs');
const path = require('path');

const DOTENV_FILE = path.join(__dirname, '..', '.env');

// `.env` holds API keys and names the programs the server runs, so it must
// be the owner's alone: a file the group or other accounts can read or write
// fails before anything in it is used.
function loadDotEnv(file = DOTENV_FILE) {
  if (!fs.existsSync(file)) return;
  const mode = fs.statSync(file).mode & 0o777;
  if (mode & 0o077) {
    throw new Error(`${file} can be read or changed by other accounts on this machine (mode ${mode.toString(8)}). It holds API keys: run \`chmod 600 .env\` in ${path.dirname(file)}, then start the app again.`);
  }
  for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
    const match = line.match(/^\s*([A-Za-z_][A-Za-z0-9_]*)\s*=\s*(.*)\s*$/);
    if (!match || line.trim().startsWith('#')) continue;
    if (process.env[match[1]] === undefined) {
      process.env[match[1]] = match[2].replace(/^["']|["']$/g, '');
    }
  }
}

module.exports = { loadDotEnv, DOTENV_FILE };
