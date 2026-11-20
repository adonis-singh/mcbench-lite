'use strict';

// Fills the extractor's cache of Mojang's and Paper's jars (README.md, The
// cache) with the repository's own pins and code, each file checked as it
// is downloaded: Paper's jar laid out by its launcher (paper-files.js), the
// BlueMap jar and Mojang's client mappings (bluemap.js ensureDownloads),
// Mojang's client jar and version file, by the sha1 Mojang's version
// manifest names (the address and size the client-jar manifest records).
// A file the cache holds is not downloaded again.
//
//   node cache.js <repository> <cache> <jdk home>

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { execFile } = require('child_process');
const { promisify } = require('util');

const execFileAsync = promisify(execFile);
const [repo, cache, jdk] = process.argv.slice(2, 5).map((arg) => arg && path.resolve(arg));
if (!repo || !cache || !jdk) throw new Error('usage: node cache.js <repository> <cache> <jdk home>');
const { MC_VERSION } = require(path.join(repo, 'server', 'assets.js'));
const { PaperFiles } = require(path.join(repo, 'server', 'paper-files.js'));
const { BlueMap } = require(path.join(repo, 'server', 'bluemap.js'));
const VERSION_MANIFEST = 'https://piston-meta.mojang.com/mc/game/version_manifest_v2.json';

const sha1 = (bytes) => crypto.createHash('sha1').update(bytes).digest('hex');

async function get(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

// `file` from `url` unless the cache holds it, its sha1 `sha` either way.
async function pinned(file, url, sha) {
  if (!fs.existsSync(file)) {
    const bytes = await get(url);
    if (sha1(bytes) !== sha) throw new Error(`${url} has sha1 ${sha1(bytes)}, but Mojang's manifest names ${sha}. It was not kept.`);
    fs.writeFileSync(`${file}.part`, bytes);
    fs.renameSync(`${file}.part`, file);
  }
  const bytes = fs.readFileSync(file);
  if (sha1(bytes) !== sha) throw new Error(`${file} has sha1 ${sha1(bytes)}, but Mojang's manifest names ${sha}`);
  return bytes;
}

(async () => {
  fs.mkdirSync(cache, { recursive: true });
  const runJava = (args, { cwd, timeout }) => execFileAsync(path.join(jdk, 'bin', 'java'), args, { cwd, timeout, maxBuffer: 1 << 24 });
  await new PaperFiles({ paperDir: path.join(cache, 'paper'), runJava }).ensurePaper();
  await new BlueMap({ root: path.join(cache, 'bluemap'), java: path.join(jdk, 'bin', 'java') }).ensureDownloads();
  // The version manifest changes with every Minecraft release; the version
  // file it names is pinned by its sha1 there, and the client jar by its
  // sha1 in the version file.
  const manifest = JSON.parse(await get(VERSION_MANIFEST));
  const entry = manifest.versions.find((version) => version.id === MC_VERSION);
  if (!entry) throw new Error(`Mojang's version manifest has no ${MC_VERSION}`);
  const version = JSON.parse(await pinned(path.join(cache, `${MC_VERSION}.json`), entry.url, entry.sha1));
  await pinned(path.join(cache, 'client.jar'), version.downloads.client.url, version.downloads.client.sha1);
  console.log(`${cache}: Paper, BlueMap, the client mappings, Minecraft ${MC_VERSION}'s version file and client jar`);
})().catch((error) => {
  console.error(error.message);
  process.exit(1);
});
