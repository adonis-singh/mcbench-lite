'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const picked = (name, size = 10) => ({ name, size });
const read = (name, size = 10) => ({ name, size, type: 'text/plain', data: 'data:text/plain;base64,' });

test('files being read are part of the draft until their read ends', async () => {
  const { EMPTY_DRAFT, draftEmpty, draftFiles, beginRead, endRead, holdsRead } = await import('../web/draft.mjs');
  assert.equal(draftEmpty(EMPTY_DRAFT), true);
  const batch = [picked('a.png'), picked('b.txt')];
  const reading = beginRead(EMPTY_DRAFT, batch);
  assert.deepStrictEqual(reading.reading, batch);
  // A draft that only holds files being read is not empty: nothing is put
  // in its place, and it is what the composer shows.
  assert.equal(draftEmpty(reading), false);
  assert.equal(holdsRead(reading, batch), true);
  assert.equal(holdsRead(EMPTY_DRAFT, batch), false);
  assert.deepStrictEqual(draftFiles(reading), batch);
  const files = [read('a.png'), read('b.txt')];
  const done = endRead(reading, batch, files);
  assert.deepStrictEqual(done.reading, []);
  assert.deepStrictEqual(done.attachments, files);
  assert.equal(holdsRead(done, batch), false);
  assert.throws(() => endRead(done, batch, files), /not reading those files/);
  assert.deepStrictEqual(EMPTY_DRAFT, { segments: [], attachments: [], selections: [], reading: [], choice: null });
});

test('a failed read leaves the draft with what it had', async () => {
  const { EMPTY_DRAFT, beginRead, endRead } = await import('../web/draft.mjs');
  const kept = read('kept.txt');
  const batch = [picked('big.bin')];
  const done = endRead(beginRead({ ...EMPTY_DRAFT, attachments: [kept] }, batch), batch, []);
  assert.deepStrictEqual(done.attachments, [kept]);
  assert.deepStrictEqual(done.reading, []);
});

test('batches end on their own, and files still being read count toward the limits', async () => {
  const { EMPTY_DRAFT, draftFiles, beginRead, endRead, holdsRead } = await import('../web/draft.mjs');
  const first = [picked('one', 5)];
  const second = [picked('two', 7), picked('three', 9)];
  let draft = beginRead(beginRead({ ...EMPTY_DRAFT, attachments: [read('zero', 1)] }, first), second);
  assert.deepStrictEqual(draftFiles(draft).map((file) => file.size), [1, 5, 7, 9]);
  draft = endRead(draft, second, [read('two', 7), read('three', 9)]);
  assert.equal(holdsRead(draft, first), true);
  assert.deepStrictEqual(draft.reading, first);
  assert.deepStrictEqual(draft.attachments.map((file) => file.name), ['zero', 'two', 'three']);
  // Two files of the same name and size are still two files.
  const twin = [picked('one', 5)];
  assert.equal(holdsRead(draft, twin), false);
});

const text = (value, choice = null) => ({ segments: [{ type: 'text', text: value }], attachments: [], selections: [], reading: [], choice });

test('a draft is parked or in the composer, never both', async () => {
  const { Drafts, EMPTY_DRAFT } = await import('../web/draft.mjs');
  const drafts = new Drafts();
  // A place never left has nothing parked.
  assert.equal(drafts.take('chat:new'), EMPTY_DRAFT);
  drafts.park('chat:new', text('a tower'));
  assert.deepStrictEqual(drafts.peek('chat:new'), text('a tower'));
  // Coming back takes it out: the composer holds it now.
  assert.deepStrictEqual(drafts.take('chat:new'), text('a tower'));
  assert.equal(drafts.peek('chat:new'), EMPTY_DRAFT);
  assert.equal(drafts.parked.size, 0);
  // Parking over a parked draft means it was in the composer too.
  drafts.park('chat:a', text('one'));
  assert.throws(() => drafts.park('chat:a', text('two')), /is parked: it cannot be in the composer too/);
  drafts.drop('chat:a');
  assert.equal(drafts.peek('chat:a'), EMPTY_DRAFT);
});

test('a prompt put in a parked draft keeps the model picked there, and an unsent message does not replace newer text', async () => {
  const { Drafts } = await import('../web/draft.mjs');
  const drafts = new Drafts();
  const choice = { provider: 'p', model: 'm', effort: '' };
  drafts.park('chat:a', text('', choice));
  drafts.putBack('chat:a', text('not sent', { provider: 'other', model: 'x', effort: '' }));
  assert.deepStrictEqual(drafts.peek('chat:a'), text('not sent', choice));
  drafts.putBack('chat:a', text('older'));
  assert.deepStrictEqual(drafts.peek('chat:a'), text('not sent', choice));
  // An edit's prompt replaces what was parked.
  drafts.put('chat:a', text('edited prompt'));
  assert.deepStrictEqual(drafts.peek('chat:a'), text('edited prompt', choice));
  drafts.putBack('chat:never-left', text('kept'));
  assert.deepStrictEqual(drafts.take('chat:never-left'), text('kept'));
});

test('a read ends in the parked draft that holds it', async () => {
  const { Drafts, EMPTY_DRAFT, beginRead } = await import('../web/draft.mjs');
  const drafts = new Drafts();
  const batch = [picked('a.png')];
  drafts.park('chat:a', beginRead(EMPTY_DRAFT, batch));
  drafts.park('chat:b', text('other'));
  drafts.endRead(batch, [read('a.png')]);
  assert.deepStrictEqual(drafts.peek('chat:a').attachments, [read('a.png')]);
  assert.deepStrictEqual(drafts.peek('chat:a').reading, []);
  assert.deepStrictEqual(drafts.peek('chat:b'), text('other'));
});
