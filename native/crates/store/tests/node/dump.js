'use strict';

// Dumps what a data folder holds, in a fixed order, for comparing two
// folders: every table of the database row by row (FTS5's shadow tables and
// sqlite_master included), the user_version and auto_vacuum pragmas, and the
// files under images/, attachments/ and worlds/ and settings.json with their
// sha256, and prompts.json.
//
//   MCBENCH_NODE_DIFF=<checkout> node dump.js <folder> <out.json>

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const W = process.env.MCBENCH_NODE_DIFF;
const Database = require(path.join(W, 'node_modules', 'better-sqlite3'));
const [folder, out] = process.argv.slice(2);

const db = new Database(path.join(folder, 'mcbench.sqlite3'), { readonly: true });
const cell = (value) => (Buffer.isBuffer(value) ? `hex:${value.toString('hex')}` : typeof value === 'bigint' ? `int:${value}` : value);
const doc = {
  user_version: db.pragma('user_version', { simple: true }),
  auto_vacuum: db.pragma('auto_vacuum', { simple: true }),
  sqlite_master: db.prepare('SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name').all(),
  tables: {},
};
db.defaultSafeIntegers(true);
for (const { name } of db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").all()) {
  // WITHOUT ROWID tables (FTS5's search_idx, search_config) by every column.
  const { sql } = db.prepare('SELECT sql FROM sqlite_master WHERE name = ?').get(name);
  const columns = db.prepare(`SELECT count(*) AS n FROM pragma_table_info('${name}')`).get().n;
  const order = /WITHOUT ROWID/i.test(sql) ? Array.from({ length: Number(columns) }, (_, i) => i + 1).join(', ') : 'rowid';
  doc.tables[name] = db.prepare(`SELECT * FROM "${name}" ORDER BY ${order}`).raw().all().map((row) => row.map(cell));
}
// The index as a reader sees it: every term and its documents.
doc.searchVocabulary = db.prepare("CREATE VIRTUAL TABLE IF NOT EXISTS temp.v USING fts5vocab(main, 'search', 'instance')").run() && db.prepare('SELECT * FROM temp.v ORDER BY term, doc, col, offset').raw().all().map((row) => row.map(cell));
db.close();

const sha = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');
doc.files = [];
for (const top of ['images', 'attachments', 'worlds']) {
  const walk = (relative) => {
    const full = path.join(folder, relative);
    if (!fs.existsSync(full)) return;
    const stat = fs.lstatSync(full);
    if (stat.isDirectory()) {
      doc.files.push([relative, 'dir']);
      for (const name of fs.readdirSync(full).sort()) walk(path.join(relative, name));
    } else doc.files.push([relative, sha(full)]);
  };
  walk(top);
}
// settings.json holds keys: its sha256 only.
const settingsFile = path.join(folder, 'settings.json');
doc.files.push(['settings.json', fs.existsSync(settingsFile) ? sha(settingsFile) : null]);
const promptsFile = path.join(folder, 'prompts.json');
doc.files.push(['prompts.json', fs.existsSync(promptsFile) ? fs.readFileSync(promptsFile, 'utf8') : null]);
fs.writeFileSync(out, JSON.stringify(doc));
