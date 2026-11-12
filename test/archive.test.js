'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const data = {
  conversations: [{ id: 'c1', title: 'House', archivedAt: 1, expiresAt: 2 }],
  benches: [{ id: 'b1', title: 'Tower', archivedAt: 1, expiresAt: 2, models: 3 }],
};

test('the archive opens loading, lands or fails, can be read again, and closes', async () => {
  const { ARCHIVE_CLOSED, archiveReading, archiveRead, archiveFailed, archiveClosed, archivedRows } = await import('../web/archive.mjs');
  let archive = archiveReading(ARCHIVE_CLOSED, 'build');
  assert.deepStrictEqual(archive, { phase: 'loading', mode: 'build', data: null, error: null });
  assert.equal(archivedRows(archive), null);
  archive = archiveFailed(archive, new Error('HTTP 500'));
  assert.deepStrictEqual(archive, { phase: 'failed', mode: 'build', data: null, error: 'HTTP 500' });
  // Nothing lands on a failed read: "Try again" reads it first.
  assert.throws(() => archiveRead(archive, data), /cannot go from failed to loaded/);
  archive = archiveRead(archiveReading(archive, 'build'), data);
  assert.equal(archive.phase, 'loaded');
  assert.deepStrictEqual(archivedRows(archive), data.conversations);
  assert.throws(() => archiveFailed(archive, new Error('late')), /cannot go from loaded to failed/);
  archive = archiveClosed(archive);
  assert.equal(archive, ARCHIVE_CLOSED);
  assert.throws(() => archiveRead(archive, data), /cannot go from closed to loaded/);
  assert.throws(() => archiveClosed(archive), /cannot go from closed to closed/);
  assert.throws(() => archivedRows(archive), /closed archive shows no rows/);
});

test('a read again keeps the rows on show in the same mode, not in the other', async () => {
  const { ARCHIVE_CLOSED, archiveReading, archiveRead, archiveFailed, archivedRows } = await import('../web/archive.mjs');
  const loaded = archiveRead(archiveReading(ARCHIVE_CLOSED, 'bench'), data);
  assert.deepStrictEqual(archivedRows(loaded), data.benches);
  const again = archiveReading(loaded, 'bench');
  assert.deepStrictEqual(archivedRows(again), data.benches);
  assert.deepStrictEqual(archivedRows(archiveFailed(again, new Error('offline'))), data.benches);
  assert.equal(archivedRows(archiveReading(loaded, 'build')), null);
});

test('time left counts whole days, and under one day says so', async () => {
  const { timeLeft } = await import('../web/archive.mjs');
  const day = 24 * 60 * 60 * 1000;
  const now = 1_000_000;
  assert.equal(timeLeft(now + 7 * day - 1, now), '6 days left');
  assert.equal(timeLeft(now + 7 * day, now), '7 days left');
  assert.equal(timeLeft(now + day + 5, now), '1 day left');
  assert.equal(timeLeft(now + day - 1, now), 'less than a day left');
  assert.equal(timeLeft(now - 5, now), 'less than a day left');
});
