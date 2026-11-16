'use strict';

// Publishes the engine's release assets that scripts/release-engine.js built
// and pinned in <folder> as the release engine-<version> of the public
// repository, from which every app whose core/ is that version downloads
// them (server/tools.js PINS). It uploads exactly the assets
// server/engine-release.json pins, each checked against the pin first, and
// SHA256SUMS, with one `gh release create`; a release of that name that
// exists already fails it.
//
//   node scripts/publish-engine.js <folder>

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const RELEASE = require('../server/engine-release.json');

const REPOSITORY = 'adonis-singh/mcbench-lite';

function main() {
  const folder = process.argv[2];
  if (!folder) throw new Error('usage: node scripts/publish-engine.js <folder>');
  const files = Object.values(RELEASE.assets).map(({ name, sha256, bytes }) => {
    const file = path.join(folder, name);
    const data = fs.readFileSync(file);
    const got = crypto.createHash('sha256').update(data).digest('hex');
    if (got !== sha256 || data.length !== bytes) throw new Error(`${file} is not the pinned asset (sha256 ${got}, ${data.length} bytes; pinned ${sha256}, ${bytes} bytes)`);
    return file;
  });
  const sums = path.join(folder, 'SHA256SUMS');
  execFileSync('gh', ['release', 'create', RELEASE.tag, '--repo', REPOSITORY, '--title', `Engine ${RELEASE.version}`,
    '--notes', `The Rust engine built from core/ at engine version ${RELEASE.version} (scripts/release-engine.js), for server/tools.js to download. It holds no data of Minecraft's: the engine reads the tables each machine makes.`,
    ...files, sums], { stdio: 'inherit' });
}

main();
