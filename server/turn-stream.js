'use strict';

const { httpError } = require('./http-guard');

// Retain this turn's wire events until it finishes. Every connection receives
// the same initial transcript snapshot, history, and subsequent live events.
// A turn is open until it ends; nothing is emitted or attached after that.
class TurnStream {
  constructor() {
    this.controller = new AbortController();
    this.startedAt = Date.now();
    this.state = 'open';
    this.listeners = new Set();
    this.lines = [];
    this.done = new Promise(resolve => { this.resolveDone = resolve; });
  }

  open() {
    if (this.state !== 'open') throw new Error('This turn has ended');
  }

  attach(res) {
    this.open();
    res.writeHead(200, {
      'content-type': 'application/x-ndjson',
      'cache-control': 'no-cache',
      'x-accel-buffering': 'no',
    });
    for (const line of this.lines) res.write(line);
    this.listeners.add(res);
    res.once('close', () => this.listeners.delete(res));
  }

  emit(event) {
    this.open();
    const line = `${JSON.stringify(event)}\n`;
    this.lines.push(line);
    for (const listener of this.listeners) {
      if (!listener.writableEnded) listener.write(line);
    }
  }

  end() {
    this.open();
    this.state = 'ended';
    for (const listener of this.listeners) listener.end();
    this.listeners.clear();
    this.resolveDone();
  }
}

// A change to a chat that awaits (a revert's sandbox reset): it holds the
// chat as a turn does, without a stream.
class Edit {
  constructor() {
    this.done = new Promise(resolve => { this.end = resolve; });
  }
}

// What holds each chat (or bench, by its own key): a running turn or an
// edit. A hold is taken synchronously, before the first await of whatever
// takes it, so nothing else starts on that chat until it is released. Once
// closed (the server is stopping) no hold is given.
class Reservations {
  constructor() {
    this.holds = new Map(); // key -> TurnStream | Edit
    this.closed = false;
  }

  has(key) {
    return this.holds.has(key);
  }

  get(key) {
    return this.holds.get(key);
  }

  // The running turn of a chat, if any.
  turn(key) {
    const hold = this.holds.get(key);
    return hold instanceof TurnStream ? hold : undefined;
  }

  turns() {
    return [...this.holds.values()].filter((hold) => hold instanceof TurnStream);
  }

  // Holds every key with a hold of its own, or none of them: a 503 once
  // closed, a 409 with `busy` when one is held already.
  take(keys, makeHold, busy) {
    if (this.closed) throw httpError(503, 'The server is shutting down');
    if (keys.some((key) => this.holds.has(key))) throw httpError(409, busy);
    return keys.map((key) => {
      const hold = makeHold();
      this.holds.set(key, hold);
      return hold;
    });
  }

  release(key, hold) {
    if (this.holds.get(key) !== hold) throw new Error(`${key} is not held by this hold`);
    this.holds.delete(key);
    hold.end();
  }

  close() {
    this.closed = true;
  }

  // Resolves once nothing is held.
  async idle() {
    while (this.holds.size) await Promise.all([...this.holds.values()].map((hold) => hold.done));
  }
}

module.exports = { TurnStream, Edit, Reservations };
