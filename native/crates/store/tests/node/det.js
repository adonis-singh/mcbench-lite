'use strict';

// Preloaded (`node -r det.js`) before server/store.js: makes Node's store
// deterministic the way the Rust test's host is, without editing server/.
// Time is `globalThis.__now` (ms), ids count up, randomInt counts up, and
// the background measurement the store starts after a chat loads is not run
// (the Rust host drops it too): a chat's use is what the first sync
// measurement saw plus what was reserved since, on both sides.

const crypto = require('node:crypto');

globalThis.__now = Number(process.env.MCBENCH_DIFF_NOW || 1794873600000);
const RealDate = Date;
class FixedDate extends RealDate {
  constructor(...args) {
    if (args.length === 0) super(globalThis.__now);
    else super(...args);
  }
  static now() { return globalThis.__now; }
}
globalThis.Date = FixedDate;

let ids = 0;
crypto.randomUUID = () => {
  ids += 1;
  return `${ids.toString(16).padStart(8, "0")}-0000-4000-8000-${ids.toString(16).padStart(12, "0")}`;
};
let ints = 0;
crypto.randomInt = (below) => {
  ints += 1;
  return (ints * 7919) % below;
};

const W = process.env.MCBENCH_NODE_DIFF;
if (!W) throw new Error('MCBENCH_NODE_DIFF must name the checkout whose server/ and node_modules/ run');
const path = require('node:path');
const storage = require(path.join(W, 'server', 'storage'));
storage.measure = () => Promise.resolve();
