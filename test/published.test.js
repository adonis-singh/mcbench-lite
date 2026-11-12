'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const build = (shareId, extra = {}) => ({
  shareId, title: `Build ${shareId}`, kind: 'chat', labels: ['m'], url: `https://o.github.io/r/${shareId}/`, publishedAt: '2026-11-01T00:00:00.000Z', conversationId: `c-${shareId}`, benchId: null, archived: false, ...extra,
});
const data = { repo: 'o/r', builds: [build('a'), build('b')] };

test('the list reads, lands or fails, can be read again, and goes unread when Settings close', async () => {
  const { PUBLISHED_UNREAD, publishedReading, publishedRead, publishedFailed, publishedClosed, publishedView } = await import('../web/published.mjs');
  assert.throws(() => publishedView(PUBLISHED_UNREAD), /show nothing/);
  let published = publishedReading(PUBLISHED_UNREAD);
  assert.deepStrictEqual(published, { phase: 'loading', data: null, removing: [], errors: {} });
  assert.equal(publishedView(published), 'loading');
  published = publishedFailed(published, new Error('gh is not signed in'));
  assert.equal(publishedView(published), 'failed');
  assert.equal(published.error, 'gh is not signed in');
  assert.throws(() => publishedRead(published, data), /cannot go from failed to loaded/);
  published = publishedRead(publishedReading(published), data);
  assert.equal(published.phase, 'loaded');
  assert.equal('error' in published, false);
  assert.equal(publishedView(published), 'list');
  // A read again keeps the rows on show until it lands.
  const again = publishedReading(published);
  assert.equal(again.data, data);
  assert.equal(publishedView(again), 'list');
  assert.throws(() => publishedFailed(published, new Error('late')), /cannot go from loaded to failed/);
  assert.equal(publishedClosed(again), PUBLISHED_UNREAD);
  assert.throws(() => publishedClosed(PUBLISHED_UNREAD), /cannot go from unread to unread/);
  // Settings opened again read from nothing.
  assert.equal(publishedReading(publishedClosed(published)).data, null);
});

test('no repository and no builds are their own views', async () => {
  const { PUBLISHED_UNREAD, publishedReading, publishedRead, publishedView } = await import('../web/published.mjs');
  const read = (body) => publishedRead(publishedReading(PUBLISHED_UNREAD), body);
  assert.equal(publishedView(read({ repo: null, builds: [] })), 'no-repo');
  assert.equal(publishedView(read({ repo: 'o/r', builds: [] })), 'empty');
});

test('a removal marks its row, then drops it or keeps it with why', async () => {
  const { PUBLISHED_UNREAD, publishedReading, publishedRead, publishedRemoving, publishedRemoved, publishedKept } = await import('../web/published.mjs');
  const loaded = publishedRead(publishedReading(PUBLISHED_UNREAD), data);
  assert.throws(() => publishedRemoving(loaded, 'zz'), /not a published build on show/);
  let published = publishedRemoving(loaded, 'a');
  assert.deepStrictEqual(published.removing, ['a']);
  assert.throws(() => publishedRemoving(published, 'a'), /already being removed/);
  published = publishedKept(published, 'a', new Error('This build is not shared'));
  assert.deepStrictEqual(published.removing, []);
  assert.deepStrictEqual(published.errors, { a: 'This build is not shared' });
  assert.equal(published.data.builds.length, 2);
  assert.throws(() => publishedKept(published, 'a', new Error('again')), /not being removed/);
  // Trying again lets go of the error; a read meanwhile keeps the removal.
  published = publishedReading(publishedRemoving(published, 'a'));
  assert.deepStrictEqual(published.errors, {});
  assert.deepStrictEqual(published.removing, ['a']);
  published = publishedRemoved(published, 'a');
  assert.deepStrictEqual(published.data.builds.map((row) => row.shareId), ['b']);
  assert.deepStrictEqual(published.removing, []);
  assert.throws(() => publishedRemoved(published, 'a'), /not being removed/);
});

test('published times read in their largest whole unit', async () => {
  const { publishedAgo } = await import('../web/published.mjs');
  const at = Date.parse('2026-11-01T00:00:00.000Z');
  const iso = '2026-11-01T00:00:00.000Z';
  assert.equal(publishedAgo(iso, at + 30_000), 'just now');
  assert.equal(publishedAgo(iso, at - 5_000), 'just now');
  assert.equal(publishedAgo(iso, at + 60_000), '1 minute ago');
  assert.equal(publishedAgo(iso, at + 2 * 3600_000 + 59_000), '2 hours ago');
  assert.equal(publishedAgo(iso, at + 86400_000), '1 day ago');
  assert.equal(publishedAgo(iso, at + 45 * 86400_000), '1 month ago');
  assert.equal(publishedAgo(iso, at + 800 * 86400_000), '2 years ago');
});

test('a published build names the chat or bench it came from, or null once that is deleted', async () => {
  const { publishedSubject } = await import('../web/published.mjs');
  assert.deepStrictEqual(publishedSubject(build('a')), { kind: 'chat', id: 'c-a' });
  assert.equal(publishedSubject(build('a', { conversationId: null, archived: undefined })), null);
  const bench = build('b', { kind: 'bench', labels: ['A', 'B'], conversationId: null, benchId: 'bench-1' });
  assert.deepStrictEqual(publishedSubject(bench), { kind: 'bench', id: 'bench-1' });
  assert.equal(publishedSubject({ ...bench, benchId: null }), null);
  assert.throws(() => publishedSubject(build('c', { kind: 'prompt' })), /not a kind of published build/);
});
