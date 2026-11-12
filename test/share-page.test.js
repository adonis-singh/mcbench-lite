'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const URL_A = 'https://owner.github.io/builds/a1/';

test('a share is read once: unknown, loading, then none or shared', async () => {
  const { SHARE_UNKNOWN, shareReading, shareRead, shareUnread } = await import('../web/share.mjs');
  const loading = shareReading(SHARE_UNKNOWN);
  assert.deepStrictEqual(loading, { phase: 'loading', url: null });
  assert.deepStrictEqual(shareRead(loading, { url: null }), { phase: 'none', url: null });
  assert.deepStrictEqual(shareRead(loading, { url: URL_A }), { phase: 'shared', url: URL_A });
  // A read that failed leaves the share unknown, to be read again.
  assert.equal(shareUnread(loading), SHARE_UNKNOWN);
  assert.throws(() => shareRead(SHARE_UNKNOWN, { url: null }), /cannot go from unknown to none/);
  assert.throws(() => shareReading(loading), /cannot go from loading to loading/);
  assert.throws(() => shareReading(shareRead(loading, { url: null })), /cannot go from none to loading/);
});

test('a publish steps through the server\'s states to shared, one at a time', async () => {
  const { sharePublishing, shareStep } = await import('../web/share.mjs');
  let share = sharePublishing({ phase: 'none', url: null });
  assert.deepStrictEqual(share, { phase: 'exporting', url: null });
  // The server's first line says what the click already moved to.
  share = shareStep(share, { state: 'exporting' });
  assert.deepStrictEqual(share, { phase: 'exporting', url: null });
  share = shareStep(share, { state: 'publishing' });
  assert.deepStrictEqual(share, { phase: 'publishing', url: null });
  // No second publish while one runs.
  assert.throws(() => sharePublishing(share), /cannot go from publishing to exporting/);
  assert.throws(() => shareStep(share, { state: 'live', url: URL_A }), /cannot go from publishing to shared/);
  share = shareStep(share, { state: 'waiting' });
  assert.deepStrictEqual(share, { phase: 'waiting', url: null });
  share = shareStep(share, { state: 'live', url: URL_A });
  assert.deepStrictEqual(share, { phase: 'shared', url: URL_A });
  assert.throws(() => shareStep(share, { state: 'publishing' }), /cannot go from shared to publishing/);
  assert.throws(() => shareStep({ phase: 'waiting', url: null }, { state: 'paused' }), /cannot go from waiting to paused/);
});

test('a failed publish keeps the url it had, and its message; try again publishes anew', async () => {
  const { sharePublishing, shareStep, shareFailed, shareStoppedEarly } = await import('../web/share.mjs');
  const again = sharePublishing({ phase: 'shared', url: URL_A });
  assert.deepStrictEqual(again, { phase: 'exporting', url: URL_A });
  const failed = shareStep(shareStep(again, { state: 'publishing' }), { state: 'failed', error: 'git push failed' });
  assert.deepStrictEqual(failed, { phase: 'failed', url: URL_A, error: 'git push failed' });
  // A refusal (409 during a running turn) fails the publish the click began.
  assert.deepStrictEqual(shareFailed({ phase: 'exporting', url: null }, new Error('A turn is running')), { phase: 'failed', url: null, error: 'A turn is running' });
  assert.deepStrictEqual(sharePublishing(failed), { phase: 'exporting', url: URL_A });
  assert.throws(() => shareFailed({ phase: 'none', url: null }, new Error('x')), /cannot go from none to failed/);
  // A stream that ended before live or failed is a failed publish.
  assert.deepStrictEqual(shareStoppedEarly({ phase: 'waiting', url: null }), { phase: 'failed', url: null, error: 'The publish stopped before the page was live' });
  assert.deepStrictEqual(shareStoppedEarly(failed), failed);
  assert.deepStrictEqual(shareStoppedEarly({ phase: 'shared', url: URL_A }), { phase: 'shared', url: URL_A });
});

test('a shared build is removed, and a removal that fails leaves it shared', async () => {
  const { shareRemoving, shareRemoved, shareKept } = await import('../web/share.mjs');
  const removing = shareRemoving({ phase: 'shared', url: URL_A });
  assert.deepStrictEqual(removing, { phase: 'removing', url: URL_A });
  assert.deepStrictEqual(shareRemoved(removing), { phase: 'none', url: null });
  assert.deepStrictEqual(shareKept(removing), { phase: 'shared', url: URL_A });
  assert.throws(() => shareRemoving({ phase: 'none', url: null }), /cannot go from none to removing/);
  assert.throws(() => shareRemoving({ phase: 'failed', url: URL_A, error: 'x' }), /cannot go from failed to removing/);
  assert.throws(() => shareRemoved({ phase: 'shared', url: URL_A }), /cannot go from shared to none/);
});

test('the post link carries the encoded url', async () => {
  const { postUrl } = await import('../web/share.mjs');
  assert.equal(postUrl('https://o.github.io/b/a 1/?x=1&y'), 'https://x.com/intent/post?url=https%3A%2F%2Fo.github.io%2Fb%2Fa%201%2F%3Fx%3D1%26y');
});

test('the stream reader parses one JSON object per line, across chunk edges', async () => {
  const { readShareStream } = await import('../web/share.mjs');
  const chunks = ['{"state":"export', 'ing"}\n{"state":"publishing"}\n\n{"sta', 'te":"live","url":"', URL_A, '"}\n'];
  const encoder = new TextEncoder();
  const stream = new ReadableStream({
    start(controller) {
      for (const chunk of chunks) controller.enqueue(encoder.encode(chunk));
      controller.close();
    },
  });
  const seen = [];
  await readShareStream(stream, (message) => seen.push(message));
  assert.deepStrictEqual(seen, [{ state: 'exporting' }, { state: 'publishing' }, { state: 'live', url: URL_A }]);
});

test('the step shown while publishing is said in plain words', async () => {
  const { shareStepText } = await import('../web/share.mjs');
  assert.equal(shareStepText({ phase: 'exporting', url: null }), 'Preparing the build…');
  assert.equal(shareStepText({ phase: 'publishing', url: null }), 'Uploading…');
  assert.equal(shareStepText({ phase: 'waiting', url: null }), 'Waiting for GitHub Pages…');
  assert.throws(() => shareStepText({ phase: 'shared', url: URL_A }), /not publishing/);
});

test('a share belongs to a subject, a chat or a bench, with its own key and endpoint', async () => {
  const { shareKey, shareUrl } = await import('../web/share.mjs');
  assert.equal(shareKey({ kind: 'chat', id: 'c1' }), 'chat:c1');
  assert.equal(shareKey({ kind: 'bench', id: 'c1' }), 'bench:c1');
  assert.equal(shareUrl({ kind: 'chat', id: 'c1' }), '/api/conversations/c1/share');
  assert.equal(shareUrl({ kind: 'bench', id: 'b1' }), '/api/benches/b1/share');
  assert.throws(() => shareKey({ kind: 'prompt', id: 'p1' }), /not a kind of share/);
  assert.throws(() => shareUrl({ kind: 'prompt', id: 'p1' }), /not a kind of share/);
});
