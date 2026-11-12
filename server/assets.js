'use strict';

const crypto = require('crypto');
const fs = require('fs');
const path = require('path');
const { DATA_DIR } = require('./storage');

const MC_VERSION = '1.21.10';
const MC_DATA_VERSION = 4556; // world_version in that release's version.json
const MCMETA = 'https://raw.githubusercontent.com/misode/mcmeta';
// The commits of misode/mcmeta's 1.21.10-summary and 1.21.10-atlas branches
// the files are read from. A branch can be pushed to; a commit cannot change.
const SUMMARY = 'cb195b930dd0f9c61b6858107aaa164e19e1d323';
const ATLAS = '704582c2fc4577a8ba40f5bfca68842dd8757d83';

// Each file's address and the sha256 of its bytes at that commit. A file
// whose bytes differ, downloaded or cached, is not used.
const ASSETS = {
  'blocks.json': { url: `${MCMETA}/${SUMMARY}/blocks/data.min.json`, sha256: '178a12096f59f863758a6c685e5eb6de38721b376a30a30383e171d0799f3ee7' },
  'block_definitions.json': { url: `${MCMETA}/${SUMMARY}/assets/block_definition/data.min.json`, sha256: 'f0072b0aacc3b449ee1f277c4796beb4efeb6fd47e061d13974a85efc5047298' },
  'models.json': { url: `${MCMETA}/${SUMMARY}/assets/model/data.min.json`, sha256: 'b39bcc4a136b5ef76be6127ad2c49bf08cce591de2ecb8937724eb74d98d0b83' },
  'atlas_map.json': { url: `${MCMETA}/${ATLAS}/all/data.min.json`, sha256: '40c464f5d242d2c64799ef271571c43b760df9929adbb8d6c6565b3dbe8d8ff0' },
  'atlas.png': { url: `${MCMETA}/${ATLAS}/all/atlas.png`, sha256: '1c4a46946bfe836dec40cc27fd6326c01a07590914124700687f25459bda3fe7' },
};

const cacheDir = path.join(DATA_DIR, 'assets', MC_VERSION);

// Fails unless `bytes` are the file `name` the table names, saying `where`
// they came from and `next`, what to do.
function checkAsset(name, bytes, where, next) {
  const sha256 = crypto.createHash('sha256').update(bytes).digest('hex');
  if (sha256 !== ASSETS[name].sha256) {
    throw new Error(`${name} ${where} has sha256 ${sha256}, but mcbench-lite expects ${ASSETS[name].sha256}. ${next}`);
  }
  return bytes;
}

async function getAsset(name) {
  if (!Object.hasOwn(ASSETS, name)) throw new Error(`Unknown asset: ${name}`);
  const { url } = ASSETS[name];
  const cached = path.join(cacheDir, name);
  if (fs.existsSync(cached)) return checkAsset(name, fs.readFileSync(cached), `in ${cacheDir}`, 'Delete it and start the app again to download it again.');
  const res = await fetch(url);
  if (!res.ok) throw new Error(`Failed to download ${url}: HTTP ${res.status}`);
  const buf = checkAsset(name, Buffer.from(await res.arrayBuffer()), `downloaded from ${url}`, 'It was not saved.');
  fs.mkdirSync(cacheDir, { recursive: true });
  fs.writeFileSync(cached, buf);
  console.log(`[assets] cached ${name} (${buf.length} bytes)`);
  return buf;
}

// { name: [properties, defaults] } for every real block in this MC version.
// Used to validate build programs and to feed block properties to the renderer.
async function getBlockRegistry() {
  return JSON.parse((await getAsset('blocks.json')).toString('utf8'));
}

module.exports = { getAsset, getBlockRegistry, MC_VERSION, MC_DATA_VERSION, ASSETS };
