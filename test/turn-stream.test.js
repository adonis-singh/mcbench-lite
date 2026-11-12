'use strict';

const { test } = require('node:test');
const assert = require('node:assert/strict');
const { EventEmitter } = require('node:events');
const { TurnStream, Edit, Reservations } = require('../server/turn-stream');

function response() {
  const res = new EventEmitter();
  res.lines = [];
  res.writeHead = (status, headers) => { res.status = status; res.headers = headers; };
  res.write = (line) => { res.lines.push(line); };
  res.end = () => { res.writableEnded = true; res.emit('close'); };
  res.events = () => res.lines.map((line) => JSON.parse(line));
  return res;
}

test('refresh replays the snapshot, reasoning, and partial tool input exactly once', () => {
  const turn = new TurnStream();
  const snapshot = { type: 'turn_snapshot', messages: [{ id: 'u1', role: 'user', content: 'Build it' }], messageTree: { headId: 'u1', messages: [] } };
  turn.emit(snapshot);
  const first = response();
  turn.attach(first);
  turn.emit({ type: 'reasoning_delta', text: '**Plan the layout**' });
  turn.emit({ type: 'reasoning_delta', text: '\n\n**Choose materials**' });
  turn.emit({ type: 'tool_call_delta', id: 'call_1', index: 0, name: 'build', args: '{"program":' });
  first.end();
  assert.equal(turn.controller.signal.aborted, false);
  turn.emit({ type: 'tool_call_delta', id: 'call_1', index: 0, name: 'build', args: '"code"}' });
  const refreshed = response();
  turn.attach(refreshed);
  assert.deepEqual(refreshed.events().slice(0, 4), first.events());
  assert.equal(refreshed.events().length, 5);
  assert.equal(refreshed.events().filter((event) => event.type === 'reasoning_delta').map((event) => event.text).join(''), '**Plan the layout**\n\n**Choose materials**');
  assert.equal(refreshed.events().filter((event) => event.type === 'tool_call_delta').map((event) => event.args).join(''), '{"program":"code"}');
  turn.emit({ type: 'assistant', text: 'Finished', final: true });
  assert.equal(refreshed.events().at(-1).text, 'Finished');
  assert.equal(first.events().length, 4);
  const again = response();
  turn.attach(again);
  assert.deepEqual(again.events(), refreshed.events());
  turn.end();
  assert.equal(refreshed.writableEnded, true);
  assert.equal(again.writableEnded, true);
  assert.equal(turn.listeners.size, 0);
});

test('replay captures the original baseline even as the stored transcript changes', () => {
  const turn = new TurnStream();
  const messages = [{ role: 'user', content: 'Build' }];
  turn.emit({ type: 'turn_snapshot', messages, messageTree: {} });
  turn.emit({ type: 'assistant', text: 'Starting', final: false });
  messages.push({ role: 'assistant', content: 'Starting' });
  const refreshed = response();
  turn.attach(refreshed);
  assert.equal(refreshed.events()[0].messages.length, 1);
  assert.equal(refreshed.events().filter((event) => event.type === 'assistant').length, 1);
  turn.end();
});

test('a client attached while the prompt is accepted receives the snapshot when ready', () => {
  const turn = new TurnStream();
  const early = response();
  turn.attach(early);
  turn.emit({ type: 'turn_snapshot', messages: [], messageTree: {} });
  assert.equal(early.events()[0].type, 'turn_snapshot');
  turn.end();
  assert.equal(early.writableEnded, true);
});

test('an ended turn takes no more events or clients', () => {
  const turn = new TurnStream();
  turn.emit({ type: 'done' });
  turn.end();
  assert.equal(turn.state, 'ended');
  assert.throws(() => turn.emit({ type: 'assistant', text: 'late' }), /This turn has ended/);
  assert.throws(() => turn.attach(response()), /This turn has ended/);
  assert.throws(() => turn.end(), /This turn has ended/);
  assert.equal(turn.lines.length, 1);
});

test('reservations hold keys all or none, refuse once closed, and idle waits for holds taken later', async () => {
  const reservations = new Reservations();
  const [turn] = reservations.take(['a'], () => new TurnStream(), 'busy');
  assert.equal(reservations.turn('a'), turn);
  assert.throws(() => reservations.take(['b', 'a'], () => new Edit(), 'a is busy'), (error) => error.statusCode === 409 && error.message === 'a is busy');
  assert.equal(reservations.has('b'), false, 'nothing is taken when one key is held');
  const [edit] = reservations.take(['b'], () => new Edit(), 'busy');
  assert.equal(reservations.turn('b'), undefined, 'an edit is not a turn');
  assert.deepEqual(reservations.turns(), [turn]);
  assert.throws(() => reservations.release('b', turn), /not held by this hold/);

  let idle = false;
  const waiting = reservations.idle().then(() => { idle = true; });
  reservations.release('a', turn);
  // A hold taken while waiting is waited for too.
  const [late] = reservations.take(['c'], () => new Edit(), 'busy');
  reservations.release('b', edit);
  await new Promise(setImmediate);
  assert.equal(idle, false);
  reservations.close();
  assert.throws(() => reservations.take(['d'], () => new Edit(), 'busy'), (error) => error.statusCode === 503);
  reservations.release('c', late);
  await waiting;
  assert.equal(idle, true);
});
