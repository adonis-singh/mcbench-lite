'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const places = [
  [{ kind: 'chat', id: 'c1' }, '/chat/c1'],
  [{ kind: 'new-chat', id: null }, '/new'],
  [{ kind: 'new-bench' }, '/bench'],
  [{ kind: 'compare', benchId: 'b1' }, '/bench/b1'],
  [{ kind: 'member', benchId: 'b1', memberId: 'm1' }, '/bench/b1/m1'],
  [{ kind: 'leaderboard' }, '/leaderboard'],
  [{ kind: 'prompt' }, '/prompts'],
];

test('every place has an address that names it again', async () => {
  const { placeAddress, addressPlace, isPageAddress } = await import('../web/address.mjs');
  for (const [place, address] of places) {
    assert.equal(placeAddress(place), address);
    assert.deepStrictEqual(addressPlace(address), place);
    assert.equal(isPageAddress(address), true);
  }
  // The new chat has one address before and after it has a row.
  assert.equal(placeAddress({ kind: 'new-chat', id: 'c9' }), '/new');
  // An id is one path segment, whatever it holds.
  assert.equal(placeAddress({ kind: 'chat', id: 'a/b?c' }), '/chat/a%2Fb%3Fc');
  assert.deepStrictEqual(addressPlace('/chat/a%2Fb%3Fc'), { kind: 'chat', id: 'a/b?c' });
  assert.equal(placeAddress({ kind: 'missing', mode: 'build', id: 'gone', address: '/chat/gone' }), '/chat/gone');
  assert.deepStrictEqual(addressPlace('/'), { kind: 'landing' });
  assert.throws(() => placeAddress({ kind: 'nowhere' }), /Unknown place: nowhere/);
});

test('the app\'s files and anything else are not page addresses', async () => {
  const { addressPlace, isPageAddress } = await import('../web/address.mjs');
  for (const path of ['/app.js', '/style.css', '/icons/block.svg', '/share/bridge.js', '/shot.html', '/bluemap/x', '/api/config', '/chat', '/chat/', '/chat/c1/x', '/bench/b1/m1/x', '/bench//m1', '/new/x', '/chat/%E0%A4%A']) {
    assert.equal(addressPlace(path), null, path);
    assert.equal(isPageAddress(path), false, path);
  }
});

test('while the lists load the page shows what needs none, and the landing shows the mode last used', async () => {
  const { startPlace } = await import('../web/address.mjs');
  assert.deepStrictEqual(startPlace({ kind: 'landing' }, 'build'), { kind: 'new-chat', id: null });
  assert.deepStrictEqual(startPlace({ kind: 'landing' }, 'bench'), { kind: 'new-bench' });
  assert.deepStrictEqual(startPlace({ kind: 'chat', id: 'c1' }, 'bench'), { kind: 'new-chat', id: null });
  assert.deepStrictEqual(startPlace({ kind: 'compare', benchId: 'b1' }, 'build'), { kind: 'new-bench' });
  assert.deepStrictEqual(startPlace({ kind: 'member', benchId: 'b1', memberId: 'm1' }, 'build'), { kind: 'new-bench' });
  for (const place of [{ kind: 'leaderboard' }, { kind: 'prompt' }, { kind: 'new-bench' }, { kind: 'new-chat', id: null }]) {
    assert.deepStrictEqual(startPlace(place, 'build'), place);
  }
});

test('a chat or bench the lists do not have is missing, never another one', async () => {
  const { listedPlace } = await import('../web/address.mjs');
  const chats = [{ id: 'c1', benchId: null }, { id: 'm1', benchId: 'b1' }, { id: 'm2', benchId: 'b2' }];
  const benches = [{ id: 'b1' }];
  const at = (target, address) => listedPlace(target, address, chats, benches);
  assert.deepStrictEqual(at({ kind: 'chat', id: 'c1' }, '/chat/c1'), { kind: 'chat', id: 'c1' });
  assert.deepStrictEqual(at({ kind: 'chat', id: 'gone' }, '/chat/gone'), { kind: 'missing', mode: 'build', id: 'gone', address: '/chat/gone' });
  // A bench model's chat is not a Build chat.
  assert.deepStrictEqual(at({ kind: 'chat', id: 'm1' }, '/chat/m1'), { kind: 'missing', mode: 'build', id: 'm1', address: '/chat/m1' });
  assert.deepStrictEqual(at({ kind: 'compare', benchId: 'b1' }, '/bench/b1'), { kind: 'compare', benchId: 'b1' });
  assert.deepStrictEqual(at({ kind: 'compare', benchId: 'b2' }, '/bench/b2'), { kind: 'missing', mode: 'bench', id: 'b2', address: '/bench/b2' });
  assert.deepStrictEqual(at({ kind: 'member', benchId: 'b1', memberId: 'm1' }, '/bench/b1/m1'), { kind: 'member', benchId: 'b1', memberId: 'm1' });
  // A model of another bench, under this bench's address, is not shown.
  assert.deepStrictEqual(at({ kind: 'member', benchId: 'b1', memberId: 'm2' }, '/bench/b1/m2'), { kind: 'missing', mode: 'bench', id: 'b1', address: '/bench/b1/m2' });
  assert.deepStrictEqual(at({ kind: 'leaderboard' }, '/leaderboard'), { kind: 'leaderboard' });
  assert.throws(() => at({ kind: 'landing' }, '/'), /names no place/);
});

test('a missing place has a mode and nothing to show or write', async () => {
  const { placeKeys } = await import('../web/place.mjs');
  assert.deepStrictEqual(placeKeys({ kind: 'missing', mode: 'bench', id: 'b2', address: '/bench/b2' }, {}), { mode: 'bench', chatId: null, viewKey: null, draftKey: null });
});
