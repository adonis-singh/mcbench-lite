'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

test('a place gives its mode, chat, world and draft, and only a place with a composer has a draft', async () => {
  const { placeKeys } = await import('../web/place.mjs');
  const rounds = { b1: 2 };
  assert.deepStrictEqual(placeKeys({ kind: 'chat', id: 'c1' }, rounds), { mode: 'build', chatId: 'c1', viewKey: 'c1', draftKey: 'chat:c1' });
  assert.deepStrictEqual(placeKeys({ kind: 'new-bench' }, rounds), { mode: 'bench', chatId: null, viewKey: null, draftKey: 'bench:new' });
  assert.deepStrictEqual(placeKeys({ kind: 'compare', benchId: 'b1' }, rounds), { mode: 'bench', chatId: null, viewKey: 'bench:b1:2', draftKey: 'bench:b1' });
  // A bench model's chat is read, never written to: no composer, no draft.
  assert.deepStrictEqual(placeKeys({ kind: 'member', benchId: 'b1', memberId: 'm1' }, rounds), { mode: 'bench', chatId: 'm1', viewKey: 'm1', draftKey: null });
  for (const kind of ['leaderboard', 'prompt']) {
    assert.deepStrictEqual(placeKeys({ kind }, rounds), { mode: 'bench', chatId: null, viewKey: null, draftKey: null });
  }
  // No Bench place shares a draft with Build's new chat.
  const benchKeys = ['new-bench', 'leaderboard', 'prompt'].map((kind) => placeKeys({ kind }, rounds).draftKey);
  assert.equal(benchKeys.includes(placeKeys({ kind: 'new-chat', id: null }, rounds).draftKey), false);
  assert.throws(() => placeKeys({ kind: 'compare', benchId: 'b2' }, rounds), /no round to show/);
  assert.throws(() => placeKeys({ kind: 'new' }, rounds), /Unknown place: new/);
});

test('the new chat keeps one draft key before and after its row exists, and gives it up as a chat', async () => {
  const { placeKeys, withRow, asChat, NEW_CHAT_DRAFT, chatDraft } = await import('../web/place.mjs');
  const fresh = { kind: 'new-chat', id: null };
  assert.deepStrictEqual(placeKeys(fresh, {}), { mode: 'build', chatId: null, viewKey: null, draftKey: NEW_CHAT_DRAFT });
  const made = withRow(fresh, 'c9');
  // The row gives the new chat a world to show; the draft is the same one.
  assert.deepStrictEqual(placeKeys(made, {}), { mode: 'build', chatId: 'c9', viewKey: 'c9', draftKey: NEW_CHAT_DRAFT });
  const chat = asChat(made);
  assert.deepStrictEqual(chat, { kind: 'chat', id: 'c9' });
  assert.equal(placeKeys(chat, {}).draftKey, chatDraft('c9'));
  assert.notEqual(chatDraft('c9'), NEW_CHAT_DRAFT);
  assert.throws(() => withRow(made, 'c10'), /Only a new chat without a row takes one/);
  assert.throws(() => withRow(chat, 'c10'), /not chat/);
  assert.throws(() => asChat(fresh), /Only a new chat with a row becomes a chat/);
  assert.throws(() => asChat({ kind: 'new-bench' }), /not new-bench/);
});
