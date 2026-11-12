'use strict';

// A pool of engines for work that needs a whole world to itself (a settle):
// each member holds one world at a time, jobs with the same key (a
// conversation) run one after another in the order they were asked, and jobs
// with different keys run at once on as many members as the pool has.

const os = require('os');
const { execFileSync } = require('child_process');
const { BUDGET_ENV } = require('./verification-contract');

// Memory one member is expected to hold: the Rust engine with a large build,
// and the node process's copy of its answer.
const MEMBER_BYTES = 1.5 * 1024 ** 3;
const MAX_MEMBERS = 8;

// Memory that can be given to a program without swapping. macOS reports the
// pages it could drop (inactive, speculative, purgeable) apart from free.
function availableMemory() {
  if (process.platform !== 'darwin') return os.freemem();
  const text = execFileSync('vm_stat', { encoding: 'utf8' });
  const pages = (name) => Number(new RegExp(`Pages ${name}:\\s+(\\d+)`).exec(text)[1]);
  const size = Number(/page size of (\d+) bytes/.exec(text)[1]);
  return (pages('free') + pages('inactive') + pages('speculative') + pages('purgeable')) * size;
}

// How many members the machine holds: one core stays for the server, and
// each member needs MEMBER_BYTES. MCBENCH_SETTLE_ENGINES sets it.
function poolSize(env = process.env) {
  if (env.MCBENCH_SETTLE_ENGINES !== undefined) {
    const size = Number(env.MCBENCH_SETTLE_ENGINES);
    if (!Number.isInteger(size) || size < 1) throw new Error(`MCBENCH_SETTLE_ENGINES must be an integer of at least 1, not ${env.MCBENCH_SETTLE_ENGINES}`);
    return size;
  }
  return Math.max(1, Math.min(MAX_MEMBERS, os.availableParallelism() - 1, Math.floor(availableMemory() / MEMBER_BYTES)));
}

class EnginePool {
  // `make(slot)` makes a member for slot 0, 1, ...: an object with stop()
  // (resolves once its engine process exited) and alive() (false once its
  // engine died). A slot is reused once its last member has stopped, and a
  // stopping member still counts against `size`. `wallMs` is how long a job
  // may hold its member.
  constructor({ size, make, wallMs }) {
    this.size = size;
    this.make = make;
    this.wallMs = wallMs;
    this.members = new Map(); // slot -> member, free or running a job
    this.closing = new Map(); // slot -> the stop of the member that had it
    this.free = [];
    this.waiting = []; // { resolve, reject } of acquire()
    this.tails = new Map(); // key -> the key's latest job
    this.stopped = false;
  }

  // Runs `job(member)` on a member of its own, after the key's earlier jobs.
  // A job that finds its member broken (it did not start, or its engine
  // stopped) calls `drop(member)`: the member is not used again. A job that
  // needs no member (`member: false`) is called with null, still after the
  // key's earlier jobs. A job still running the pool's `wallMs` after it got
  // its member fails, and its member is stopped and dropped, so no build holds
  // an engine for good.
  run(key, job, { member: needsMember = true } = {}) {
    const previous = this.tails.get(key) || Promise.resolve();
    const result = previous.then(() => (needsMember ? this.acquire() : null)).then(async (member) => {
      if (!member) return job(null);
      const work = job(member);
      let timer;
      const expired = new Promise((resolve, reject) => {
        timer = setTimeout(() => {
          this.drop(member);
          reject(new Error(`The settle ran past its ${this.wallMs / 1000}-second wall-clock budget (${BUDGET_ENV.wallMs}) and was stopped: the build is too active to settle in time. Ask for fewer ticks or make the build less active.`));
        }, this.wallMs);
      });
      try {
        return await Promise.race([work, expired]);
      } finally {
        clearTimeout(timer);
        work.catch(() => {});
        this.release(member);
      }
    });
    const tail = result.catch(() => {});
    this.tails.set(key, tail);
    tail.then(() => { if (this.tails.get(key) === tail) this.tails.delete(key); });
    return result;
  }

  acquire() {
    if (this.stopped) return Promise.reject(new Error('The engine pool was stopped'));
    return new Promise((resolve, reject) => {
      this.waiting.push({ resolve, reject });
      this.serve();
    });
  }

  // Hands members to waiters, first come first served.
  serve() {
    while (this.waiting.length && !this.stopped) {
      const member = this.take();
      if (!member) return;
      this.waiting.shift().resolve(member);
    }
  }

  // A free member whose engine is alive, or a new one if a slot is open.
  take() {
    while (this.free.length) {
      const member = this.free.pop();
      if (member.alive()) return member;
      this.retire(member);
    }
    if (this.members.size + this.closing.size >= this.size) return null;
    let slot = 0;
    while (this.members.has(slot) || this.closing.has(slot)) slot += 1;
    const member = this.make(slot);
    member.slot = slot;
    this.members.set(slot, member);
    return member;
  }

  // Stops `member`; its slot opens once its engine has exited.
  retire(member) {
    this.members.delete(member.slot);
    this.closing.set(member.slot, member.stop().catch(() => {}).then(() => {
      this.closing.delete(member.slot);
      this.serve();
    }));
  }

  // A job's member back: a stopped pool already stopped it.
  release(member) {
    if (this.stopped) return;
    if (member.dropped) this.retire(member);
    else this.free.push(member);
    this.serve();
  }

  drop(member) {
    member.dropped = true;
  }

  // Stops every member and fails every job still waiting for one; the pool
  // takes no more.
  async stop() {
    this.stopped = true;
    for (const waiter of this.waiting.splice(0)) waiter.reject(new Error('The engine pool was stopped'));
    const members = [...this.members.values()];
    this.members.clear();
    this.free = [];
    await Promise.all([...members.map((member) => member.stop()), ...this.closing.values()]);
  }
}

module.exports = { EnginePool, poolSize, availableMemory, MEMBER_BYTES };
