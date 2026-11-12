'use strict';

// Publishes this machine's engine program to the project's releases, where
// every machine with the same sources, Paper build, GraalVM release and
// platform downloads it instead of building it (server/engine.js). Builds it
// first if needed. Run it after an engine change, once per platform.
//
//   node scripts/publish-engine.js

const fs = require('fs');
const os = require('os');
const path = require('path');
const zlib = require('zlib');
const { execFileSync } = require('child_process');
const { minecraftLive } = require('../server/minecraft-live');
const { buildEngine, RELEASES } = require('../server/engine');

async function main() {
  const classpath = (await minecraftLive.ensurePaper()).map((file) => path.join(minecraftLive.paperDir, file));
  const binary = await buildEngine({ classpath, folder: minecraftLive.engineFolder, minecraftVersion: minecraftLive.minecraftVersion, signal: new AbortController().signal });
  const scratch = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-publish-'));
  const asset = path.join(scratch, `${path.basename(binary)}.gz`);
  fs.writeFileSync(asset, zlib.gzipSync(fs.readFileSync(binary), { level: 9 }));
  const gh = (args) => execFileSync('gh', [...args, '--repo', RELEASES.repo], { stdio: ['ignore', 'pipe', 'pipe'] }).toString();
  try {
    gh(['release', 'view', RELEASES.tag]);
  } catch (error) {
    if (!/release not found/i.test(String(error.stderr))) throw error;
    gh(['release', 'create', RELEASES.tag, '--title', 'Engine programs', '--notes', 'Native engine programs by name (scripts/publish-engine.js), downloaded by server/engine.js.']);
  }
  gh(['release', 'upload', RELEASES.tag, asset, '--clobber']);
  fs.rmSync(scratch, { recursive: true, force: true });
  console.log(`Published ${path.basename(asset)} (${(fs.statSync(binary).size / 1e6).toFixed(0)} MB program)`);
}

main().then(() => process.exit(0), (error) => { console.error(error); process.exit(1); });
