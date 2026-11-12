'use strict';
// One process at a time per output directory: `file` holds the pid of the
// process that holds it. A lock whose process is gone (killed, crashed) is
// taken over; one whose process lives fails. Answers the release.
const fs = require('node:fs');

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch (error) { return error.code === 'EPERM'; }
}

// Makes `file` holding this process's pid, or answers false if it exists. The
// file is a link to one written whole first, so no reader ever sees it empty.
function create(file) {
  const own = `${file}.${process.pid}`;
  fs.writeFileSync(own, String(process.pid));
  try {
    fs.linkSync(own, file);
    return true;
  } catch (error) {
    if (error.code === 'EEXIST') return false;
    throw error;
  } finally {
    fs.rmSync(own);
  }
}

// The pid `file` holds, or null if it is gone (its holder released it).
function holder(file) {
  try { return Number(fs.readFileSync(file, 'utf8')); } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

// Takes `file`, or throws naming the live process that holds it. `waits` is
// how many times a live holder is waited 10 ms for before that.
function pidLock(file, waits = 0) {
  for (;;) {
    if (create(file)) return () => fs.rmSync(file);
    const pid = holder(file);
    if (pid === null) continue;
    if (alive(pid)) {
      if (waits-- > 0) { Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 10); continue; }
      throw new Error(`${file} is held by process ${pid}, which is still running`);
    }
    // A dead holder's lock is removed under a lock of its own: two processes
    // that both found it dead would otherwise both remove it, the second one
    // the lock the first had just made. That lock is a pid lock too: one left
    // by a process that died holding it is taken over the same way, and a live
    // process holds it only for this removal, so it is waited for.
    const release = pidLock(`${file}.takeover`, 100);
    try {
      if (holder(file) === pid) fs.rmSync(file);
    } finally {
      release();
    }
  }
}

module.exports = { pidLock, alive };
