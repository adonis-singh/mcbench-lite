'use strict';

// Publishes see_build's renderer's release assets that scripts/release-draw.js
// built and pinned in <folder> as the release draw-<version> of the public
// repository, from which every app whose native/draw and renderer/src are
// that version downloads them (server/tools.js PINS). It uploads exactly the
// assets server/draw-release.json pins, each checked against the pin first,
// and SHA256SUMS, with one `gh release create`; a release of that name that
// exists already fails it.
//
//   node scripts/publish-draw.js <folder>

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFileSync } = require('child_process');

const RELEASE = require('../server/draw-release.json');

const REPOSITORY = 'adonis-singh/mcbench-lite';

function main() {
  const folder = process.argv[2];
  if (!folder) throw new Error('usage: node scripts/publish-draw.js <folder>');
  const files = Object.values(RELEASE.assets).map(({ name, sha256, bytes }) => {
    const file = path.join(folder, name);
    const data = fs.readFileSync(file);
    const got = crypto.createHash('sha256').update(data).digest('hex');
    if (got !== sha256 || data.length !== bytes) throw new Error(`${file} is not the pinned asset (sha256 ${got}, ${data.length} bytes; pinned ${sha256}, ${bytes} bytes)`);
    return file;
  });
  const sums = path.join(folder, 'SHA256SUMS');
  execFileSync('gh', ['release', 'create', RELEASE.tag, '--repo', REPOSITORY, '--title', `Screenshot renderer ${RELEASE.version}`,
    '--notes', `see_build's renderer, mcbench-draw, built from native/draw and renderer/src at version ${RELEASE.version} (scripts/release-draw.js), for server/tools.js to download. It holds no data of Minecraft's: it reads the files each machine makes.`,
    ...files, sums], { stdio: 'inherit' });
}

main();
