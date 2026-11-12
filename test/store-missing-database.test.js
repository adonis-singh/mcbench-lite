'use strict';

// Startup removes the world folders no chat in the database names, so a data
// folder whose database is gone but whose worlds are there must not start:
// a new, empty database would delete every world.
const { after, test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-store-missing-'));
process.env.MCBENCH_DATA_DIR = dataDir;
after(() => {
  fs.rmSync(dataDir, { recursive: true, force: true });
  delete process.env.MCBENCH_DATA_DIR;
});

const ID = 'aaaaaaaa-0000-0000-0000-000000000000';
const database = path.join(dataDir, 'mcbench.sqlite3');
const chunk = path.join(dataDir, 'worlds', ID, 'chunks', 'c.json');

function load() {
  for (const name of ['store', 'storage']) delete require.cache[require.resolve(`../server/${name}`)];
  return require('../server/store');
}

test('a missing database beside worlds fails before it is created, and the worlds are kept', () => {
  fs.mkdirSync(path.dirname(chunk), { recursive: true });
  fs.writeFileSync(chunk, '[]');
  assert.throws(load, {
    message: `The database ${database} is missing, but ${path.join(dataDir, 'worlds')} holds worlds. Restore the database first: starting without it would create an empty one and delete those worlds.`,
  });
  assert.equal(fs.existsSync(database), false);
  assert.equal(fs.readFileSync(chunk, 'utf8'), '[]');
});

test('a data folder whose worlds folder is empty gets a new database', () => {
  fs.rmSync(path.join(dataDir, 'worlds', ID), { recursive: true });
  const store = load();
  assert.equal(fs.existsSync(database), true);
  assert.deepEqual(store.listConversations(), []);
  store.close();
});
