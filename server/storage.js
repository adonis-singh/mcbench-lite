'use strict';

// Storage limits. Each chat has a byte budget covering its
// attachments, images, world chunks and /work folder, and nothing new is
// written while the disk holding the data folder has less than a reserve
// free. A chat's use is what du counts in its four folders (disk blocks),
// plus what the app has written there since it last measured.

const { spawn, spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');

const DATA_DIR = process.env.MCBENCH_DATA_DIR
  ? path.resolve(process.env.MCBENCH_DATA_DIR)
  : path.join(__dirname, '..', 'data');

function setting(name, standard, min) {
  const value = Number(process.env[name] || standard);
  if (!Number.isSafeInteger(value) || value < min) throw new Error(`${name} must be an integer of at least ${min}`);
  return value;
}

const GiB = 1024 ** 3;
const CHAT_MAX_BYTES = setting('MCBENCH_CHAT_MAX_BYTES', 4 * GiB, 1);
const DISK_RESERVE_BYTES = setting('MCBENCH_DISK_RESERVE_BYTES', 2 * GiB, 0);
const REPORTS_KEPT = setting('MCBENCH_REPORTS_KEPT', 100, 0);

const size = (bytes) => (bytes >= GiB ? `${(bytes / GiB).toFixed(2)} GiB`
  : bytes >= 1024 ** 2 ? `${(bytes / 1024 ** 2).toFixed(1)} MiB` : `${Math.ceil(bytes / 1024)} KiB`);

// A write the limits refuse; the HTTP layer answers it 507 Insufficient Storage.
const refused = (message) => Object.assign(new Error(message), { statusCode: 507, code: 'STORAGE_FULL' });

// The disk's free space, read at most every READ_MS: a statfs just after a
// large write waits on the filesystem, and slows back-to-back uploads.
// Writes reserved since the reading are taken off it.
const READ_MS = 500;
let reading = null; // { at, free }

function freeBytes() {
  const now = Date.now();
  if (!reading || now - reading.at >= READ_MS) {
    const { bavail, bsize } = fs.statfsSync(DATA_DIR);
    reading = { at: now, free: bavail * bsize };
  }
  return reading.free;
}

function diskRefusal(bytes) {
  const free = freeBytes();
  if (free - bytes >= DISK_RESERVE_BYTES) return null;
  return `The disk holding the data folder has ${size(free)} free${bytes ? ` and this write needs ${size(bytes)}` : ''}; mcbench-lite keeps ${size(DISK_RESERVE_BYTES)} free (MCBENCH_DISK_RESERVE_BYTES) and saves nothing new until space is freed.`;
}

// ---- per-chat use ---------------------------------------------------------

const FOLDERS = ['worlds', 'images', 'attachments', 'sandbox'];
// chat id -> { bytes: what it holds, or null until measured (again, after a
// measurement failed); charged: every byte ever reserved, so a measurement
// adds those reserved while it ran; running, next: measure()'s }
const chats = new Map();

function entry(id) {
  let chat = chats.get(id);
  if (!chat) chats.set(id, chat = { bytes: null, charged: 0, running: null, next: null });
  return chat;
}

// Every folder chat `id` holds, whether or not it exists.
function chatFolders(id) {
  return FOLDERS.map((folder) => path.join(DATA_DIR, folder, id));
}

function folders(id) {
  return chatFolders(id).filter((folder) => fs.existsSync(folder));
}

// du prints one total per folder even when a file vanished during its walk
// (and then exits 1), so the totals decide, not the exit code.
function duBytes(output, list) {
  const totals = output.trim().split('\n').filter(Boolean).map((line) => line.match(/^(\d+)\t/));
  if (totals.length !== list.length || totals.some((match) => !match)) throw new Error(`du did not measure ${list.join(', ')}: ${output.slice(0, 400)}`);
  return totals.reduce((sum, match) => sum + Number(match[1]) * 1024, 0);
}

function measureNow(id) {
  const list = folders(id);
  if (!list.length) return 0;
  const result = spawnSync('du', ['-sk', ...list], { encoding: 'utf8' });
  if (result.error) throw result.error;
  return duBytes(result.stdout, list);
}

function measureLater(id) {
  const list = folders(id);
  if (!list.length) return Promise.resolve(0);
  return new Promise((resolve, reject) => {
    const child = spawn('du', ['-sk', ...list]);
    let output = '';
    child.stdout.on('data', (data) => { output += data; });
    child.on('error', reject);
    child.on('close', () => {
      try { resolve(duBytes(output, list)); } catch (error) { reject(error); }
    });
  });
}

// What chat `id` holds now. An ask while it is unknown (the first in a
// process, or the first after a measurement failed) measures it, and throws
// what that measurement threw.
function used(id) {
  const chat = entry(id);
  if (chat.bytes === null) chat.bytes = measureNow(id);
  return chat.bytes;
}

function chatRefusal(id, bytes) {
  const held = used(id);
  if (held + bytes <= CHAT_MAX_BYTES) return null;
  return `This chat's storage is full: its attachments, images, world and /work hold ${size(held)} of its ${size(CHAT_MAX_BYTES)} budget (MCBENCH_CHAT_MAX_BYTES)${bytes ? ` and this write needs ${size(bytes)}` : ''}.`;
}

// Counts `bytes` about to be written for chat `id`, or throws when they do
// not fit its budget or the disk's reserve. `disk: false` skips the disk
// (a world save asks it once, not for every chunk).
function reserve(id, bytes, { disk = true } = {}) {
  const refusal = (disk ? diskRefusal(bytes) : null) ?? chatRefusal(id, bytes);
  if (refusal) throw refused(refusal);
  const chat = entry(id);
  chat.bytes += bytes;
  chat.charged += bytes;
  if (reading) reading.free -= bytes;
}

// Bytes reserved and then not written (a save that failed). A measurement
// running meanwhile still adds them: it cannot tell whether du saw them, and
// counting too much until the next one is safe. A chat forgotten or unknown
// since is measured afresh, which counts what was written.
function refund(id, bytes) {
  const chat = chats.get(id);
  if (chat?.bytes != null) chat.bytes -= bytes;
}

// Why nothing more may be written for chat `id` (over its budget, or the
// disk under its reserve), or null.
function refusal(id) {
  return diskRefusal(0) ?? chatRefusal(id, 0);
}

// Measures chat `id` again in the background (after a bash call, or when the
// disk fills); nothing waits for it but whoever awaits the promise. Asked
// while one runs, it measures once more after that one, so the result
// postdates the ask. Writes reserved meanwhile stay counted.
function measure(id) {
  const chat = entry(id);
  if (chat.next) return chat.next;
  const next = (chat.running ?? Promise.resolve()).catch(() => {}).then(async () => {
    chat.next = null;
    chat.running = next;
    const before = chat.charged;
    try {
      chat.bytes = await measureLater(id) + chat.charged - before;
    } catch (error) {
      chat.bytes = null;
      throw error;
    }
  }).finally(() => {
    if (chat.running === next) chat.running = null;
  });
  chat.next = next;
  next.catch(() => {}); // the next used() measures again
  return next;
}

function forget(id) {
  chats.delete(id);
}

// Which of `names` to delete so at most `keep` remain: those `referenced`
// does not hold, oldest first by `age(name)` (then by name). Only when some
// are kept is anything aged.
function expendable(names, referenced, keep, age) {
  const excess = names.length - keep;
  if (excess <= 0) return [];
  const candidates = names.filter((name) => !referenced.has(name));
  if (candidates.length <= excess) return candidates;
  return candidates.map((name) => [age(name), name])
    .sort((a, b) => a[0] - b[0] || (a[1] < b[1] ? -1 : 1))
    .slice(0, excess)
    .map(([, name]) => name);
}

module.exports = {
  DATA_DIR, CHAT_MAX_BYTES, DISK_RESERVE_BYTES, REPORTS_KEPT,
  freeBytes, reserve, refund, refusal, used, measure, forget, expendable, chatFolders,
  _internal: { dropReading: () => { reading = null; } },
};
