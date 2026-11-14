'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const load = () => import('../web/filters.mjs');

const chat = (id, resolution, model, prompts = [], provider = 'openrouter') => ({ id, resolution, provider, model, prompts, title: id });
const CHATS = [
  chat('castle', 10, 'vendor/alpha', ['p-castle']),
  chat('tower', 2, 'vendor/beta', ['p-tower', 'p-castle']),
  chat('hut', 1, 'vendor/alpha'),
  chat('shed', 5, 'vendor/alpha', [], 'anthropic'),
];

test('no filter lists every row, in the order given or the search ranked', async () => {
  const { NO_FILTERS, listed, chatFacets, filtering } = await load();
  assert.equal(filtering(NO_FILTERS), false);
  assert.deepEqual(listed(CHATS, null, NO_FILTERS, chatFacets), CHATS);
  const found = [{ id: 'hut' }, { id: 'gone' }, { id: 'castle' }];
  assert.deepEqual(listed(CHATS, found, NO_FILTERS, chatFacets).map((row) => row.id), ['hut', 'castle'], 'a result no longer listed is dropped');
  assert.deepEqual(listed(CHATS, [], NO_FILTERS, chatFacets), [], 'a search that found nothing lists nothing');
});

test('each group alone: any chosen value matches', async () => {
  const { NO_FILTERS, listed, chatFacets, toggled, modelKey } = await load();
  const ids = (filters) => listed(CHATS, null, filters, chatFacets).map((row) => row.id);
  assert.deepEqual(ids(toggled(NO_FILTERS, 'size', 10)), ['castle']);
  assert.deepEqual(ids(toggled(toggled(NO_FILTERS, 'size', 10), 'size', 1)), ['castle', 'hut']);
  assert.deepEqual(ids(toggled(NO_FILTERS, 'prompt', 'p-castle')), ['castle', 'tower'], 'a chat matches any prompt one of its messages came from');
  assert.deepEqual(ids(toggled(NO_FILTERS, 'prompt', 'p-tower')), ['tower']);
  assert.deepEqual(ids(toggled(NO_FILTERS, 'model', modelKey('openrouter', 'vendor/alpha'))), ['castle', 'hut'], 'the same model through another provider is another model');
  assert.deepEqual(ids(toggled(NO_FILTERS, 'size', 1)), ['hut']);
  const off = toggled(toggled(NO_FILTERS, 'size', 10), 'size', 10);
  assert.deepEqual(off, NO_FILTERS, 'toggling again takes the choice back');
  assert.deepEqual(ids(off), CHATS.map((row) => row.id), 'an empty choice is no filter');
});

test('groups combine: a row must match every group with a choice, and the search text', async () => {
  const { NO_FILTERS, listed, chatFacets, toggled, modelKey } = await load();
  const alpha = modelKey('openrouter', 'vendor/alpha');
  const castleAlpha = toggled(toggled(NO_FILTERS, 'prompt', 'p-castle'), 'model', alpha);
  assert.deepEqual(listed(CHATS, null, castleAlpha, chatFacets).map((row) => row.id), ['castle']);
  assert.deepEqual(listed(CHATS, null, toggled(castleAlpha, 'size', 2), chatFacets), [], 'no row is both');
  assert.deepEqual(listed(CHATS, [{ id: 'hut' }, { id: 'castle' }, { id: 'tower' }], toggled(NO_FILTERS, 'model', alpha), chatFacets).map((row) => row.id), ['hut', 'castle'], 'in the search\'s order');
  assert.deepEqual(listed(CHATS, [{ id: 'tower' }], toggled(NO_FILTERS, 'model', alpha), chatFacets), []);
});

test('a bench matches by its resolution, any of its rounds\' prompts and any of its models', async () => {
  const { NO_FILTERS, listed, benchFacets, toggled, modelKey } = await load();
  const benches = [{ id: 'b1', resolution: 5, prompts: ['p-castle'] }, { id: 'b2', resolution: 1, prompts: [] }];
  const members = { b1: [chat('m1', 5, 'vendor/alpha'), chat('m2', 5, 'vendor/beta')], b2: [chat('m3', 1, 'vendor/gamma'), chat('m4', 1, 'vendor/beta')] };
  const facets = (bench) => benchFacets(bench, members[bench.id]);
  const ids = (filters) => listed(benches, null, filters, facets).map((row) => row.id);
  assert.deepEqual(ids(toggled(NO_FILTERS, 'model', modelKey('openrouter', 'vendor/beta'))), ['b1', 'b2']);
  assert.deepEqual(ids(toggled(NO_FILTERS, 'model', modelKey('openrouter', 'vendor/gamma'))), ['b2']);
  assert.deepEqual(ids(toggled(NO_FILTERS, 'size', 5)), ['b1']);
  assert.deepEqual(ids(toggled(toggled(NO_FILTERS, 'prompt', 'p-castle'), 'model', modelKey('openrouter', 'vendor/gamma'))), []);
});

test('the menu offers every size, the prompts the rows came from in library order then deleted ones, and the rows\' models', async () => {
  const { offered, chatFacets, modelKey } = await load();
  const library = [{ id: 'p-tower', title: 'Tower' }, { id: 'p-unused', title: 'Unused' }, { id: 'p-castle', title: '' }];
  const rows = [...CHATS, chat('old', 1, 'vendor/alpha', ['p-gone-b', 'p-gone-a'])];
  const offer = offered(rows.map(chatFacets), library, { 'p-gone-a': 'Windmill', 'p-gone-b': 'Barn', 'p-castle': 'Old castle' });
  assert.deepEqual(offer.size, [1, 2, 5, 10]);
  assert.deepEqual(offer.prompt, [
    { id: 'p-tower', title: 'Tower', deleted: false },
    { id: 'p-castle', title: '', deleted: false },
    { id: 'p-gone-b', title: 'Barn', deleted: true },
    { id: 'p-gone-a', title: 'Windmill', deleted: true },
  ], "a prompt in the library goes by its title there; one deleted by the title it was last sent under");
  assert.deepEqual(offer.model, [modelKey('anthropic', 'vendor/alpha'), modelKey('openrouter', 'vendor/alpha'), modelKey('openrouter', 'vendor/beta')]);
  assert.deepEqual(offered([], library, {}), { size: [1, 2, 5, 10], prompt: [], model: [] });
});

test('a chosen prompt or model no listed row has is not in effect, and is again once a row has it', async () => {
  const { NO_FILTERS, offered, inEffect, chatFacets, toggled, modelKey } = await load();
  const chosen = toggled(toggled(toggled(NO_FILTERS, 'prompt', 'p-tower'), 'model', modelKey('openrouter', 'vendor/beta')), 'size', 2);
  const library = [{ id: 'p-tower', title: 'Tower' }];
  assert.deepEqual(inEffect(chosen, offered(CHATS.map(chatFacets), library, {})), chosen);
  const without = CHATS.filter((row) => row.id !== 'tower');
  assert.deepEqual(inEffect(chosen, offered(without.map(chatFacets), library, {})), { size: [2], prompt: [], model: [] }, 'every size is always offered');
});
