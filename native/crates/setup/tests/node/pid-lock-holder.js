'use strict';
// Takes a data folder's lock as today's server/index.js does (its lines
// from `const { DATA_DIR } = require('./storage')` to the pidLock call), on
// the folder MCBENCH_DATA_DIR names, and prints `held <pid>`. Arguments: a
// checkout whose server/ is used, the lock's file in the folder (index.js's
// is `server.pid`), and what to do once it is held: `hold` (until stdin
// ends, then release it as index.js does when it stops), `die` (exit
// without releasing, as a killed server leaves it) or `take` (release it at
// once). A lock it cannot take throws, uncaught, as index.js's does.
const fs = require('node:fs');
const path = require('node:path');

const [checkout, file, mode] = process.argv.slice(2);
const { DATA_DIR } = require(path.join(checkout, 'server', 'storage'));
const { pidLock } = require(path.join(checkout, 'server', 'pid-lock'));
fs.mkdirSync(DATA_DIR, { recursive: true });
const releaseDataFolder = pidLock(path.join(DATA_DIR, file));
console.log(`held ${process.pid}`);
if (mode === 'die') {
  // Ends without releasing it.
} else if (mode === 'take') {
  releaseDataFolder();
} else {
  if (mode !== 'hold') throw new Error(`unknown mode ${mode}`);
  process.stdin.resume();
  process.stdin.on('end', () => {
    releaseDataFolder();
    console.log('released');
  });
}
