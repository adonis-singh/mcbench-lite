'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const draft = { titleModel: { provider: 'p', model: 'm', effort: '' }, providers: {} };

test('the panel opens from a read, saves once, and closes only when not saving', async () => {
  const { CLOSED, panelTo, panelDraft } = await import('../web/settings-panel.mjs');
  let panel = CLOSED;
  assert.equal(panelDraft(panel), null);
  panel = panelTo(panel, { phase: 'opening', view: 'general' });
  // Another pane asked for while the read is on its way.
  panel = panelTo(panel, { phase: 'opening', view: 'openrouter' });
  assert.equal(panelDraft(panel), null);
  panel = panelTo(panel, { phase: 'open', view: panel.view, draft, error: '' });
  assert.equal(panelDraft(panel), draft);
  panel = panelTo(panel, { phase: 'saving', view: panel.view, draft });
  assert.equal(panelDraft(panel), draft);
  // No second save, and no opening anew, while one is on its way.
  assert.throws(() => panelTo(panel, { phase: 'saving', view: panel.view, draft }), /cannot go from saving to saving/);
  assert.throws(() => panelTo(panel, { phase: 'opening', view: 'general' }), /cannot go from saving to opening/);
  // A failed save opens the panel again with its error; one that lands closes it.
  const failed = panelTo(panel, { phase: 'open', view: panel.view, draft, error: 'Choose a title model' });
  assert.equal(failed.error, 'Choose a title model');
  assert.equal(panelTo(panel, CLOSED), CLOSED);
  // A read that fails closes the opening.
  assert.equal(panelTo({ phase: 'opening', view: 'general' }, CLOSED), CLOSED);
  assert.throws(() => panelTo(CLOSED, { phase: 'open', view: 'general', draft, error: '' }), /cannot go from closed to open/);
  assert.throws(() => panelTo(CLOSED, { phase: 'saving', view: 'general', draft }), /cannot go from closed to saving/);
  assert.throws(() => panelTo({ phase: 'opening', view: 'general' }, { phase: 'open', view: 'general', error: '' }), /hold a draft/);
  assert.throws(() => panelTo({ phase: 'opening', view: 'general' }, { phase: 'closed', view: 'general' }), /are on a pane/);
});

test('only open settings are edited, and an edit lets go of the last save\'s error', async () => {
  const { CLOSED, panelEdit } = await import('../web/settings-panel.mjs');
  const open = { phase: 'open', view: 'general', draft, error: 'Choose a title model' };
  const edited = panelEdit(open, (current) => ({ ...current, titleModel: { provider: 'p', model: 'n', effort: '' } }));
  assert.deepStrictEqual(edited, { phase: 'open', view: 'general', draft: { ...draft, titleModel: { provider: 'p', model: 'n', effort: '' } }, error: '' });
  assert.throws(() => panelEdit({ phase: 'saving', view: 'general', draft }, (current) => current), /cannot be edited while saving/);
  assert.throws(() => panelEdit(CLOSED, (current) => current), /cannot be edited while closed/);
  assert.throws(() => panelEdit({ phase: 'opening', view: 'general' }, (current) => current), /cannot be edited while opening/);
});

test('the title model is a model picked, or null for the chat\'s own model, and saves as such', async () => {
  const { panelTo, panelEdit, withTitleModel } = await import('../web/settings-panel.mjs');
  // The server answers null when no title model was chosen.
  let panel = panelTo({ phase: 'opening', view: 'general' }, { phase: 'open', view: 'general', draft: { ...draft, titleModel: null }, error: '' });
  panel = panelEdit(panel, (current) => withTitleModel(current, { provider: 'google', model: 'gemini-2.5-flash', effort: 'high' }));
  assert.deepStrictEqual(panel.draft.titleModel, { provider: 'google', model: 'gemini-2.5-flash', effort: '' });
  panel = panelEdit(panel, (current) => withTitleModel(current, null));
  assert.equal(panel.draft.titleModel, null);
  assert.equal(JSON.parse(JSON.stringify(panel.draft)).titleModel, null);
});

test('the repository for shared builds saves as typed, and empty saves null', async () => {
  const { panelTo, panelEdit, withShareRepo } = await import('../web/settings-panel.mjs');
  let panel = panelTo({ phase: 'opening', view: 'general' }, { phase: 'open', view: 'general', draft: { ...draft, shareRepo: null }, error: '' });
  panel = panelEdit(panel, (current) => withShareRepo(current, ' owner/builds '));
  assert.equal(panel.draft.shareRepo, 'owner/builds');
  panel = panelEdit(panel, (current) => withShareRepo(current, '  '));
  assert.equal(JSON.parse(JSON.stringify(panel.draft)).shareRepo, null);
});

const provider = (connected, enabled = true) => ({ label: 'x', connected, enabled });
const everyPane = ['openrouter', 'openai', 'anthropic', 'google', 'groq', 'qwencloud', 'claude-code', 'codex-cli', 'codex', 'cursor'];

test('Settings open on Connect an AI while no provider can be used, and on General once one can', async () => {
  const { firstPane, usable, CONNECT, panelTo, CLOSED } = await import('../web/settings-panel.mjs');
  assert.equal(CONNECT, 'connect');
  assert.equal(firstPane({}), 'connect');
  assert.equal(firstPane({ openrouter: provider(false), cursor: provider(false) }), 'connect');
  // Connected but off in Settings is not usable.
  assert.equal(usable(provider(true, false)), false);
  assert.equal(firstPane({ openrouter: provider(true, false) }), 'connect');
  assert.equal(firstPane({ openrouter: provider(false), 'claude-code': provider(true) }), 'general');
  // Connect is a pane like any other: the panel opens on it and turns from it.
  let panel = panelTo(CLOSED, { phase: 'opening', view: CONNECT });
  panel = panelTo(panel, { phase: 'open', view: CONNECT, draft, error: '' });
  assert.equal(panelTo(panel, { ...panel, view: 'openrouter' }).view, 'openrouter');
});

test('the ways to connect name every provider pane once, each with its entry', async () => {
  const { connectWays, CONNECT_WAYS } = await import('../web/settings-panel.mjs');
  const panes = CONNECT_WAYS.flatMap((way) => way.panes);
  assert.deepStrictEqual([...panes].sort(), [...everyPane].sort());
  assert.equal(CONNECT_WAYS[0].panes[0], 'openrouter');
  const providers = Object.fromEntries(everyPane.map((id) => [id, provider(id === 'codex')]));
  const ways = connectWays(providers);
  assert.deepStrictEqual(ways.map((way) => way.key), ['api-key', 'claude', 'chatgpt', 'cursor']);
  assert.equal(ways[2].panes[1].id, 'codex');
  assert.equal(ways[2].panes[1].connection, providers.codex);
  // A pane the server does not report fails, not a way quietly missing.
  const { cursor, ...withoutCursor } = providers;
  assert.ok(cursor);
  assert.throws(() => connectWays(withoutCursor), /No provider cursor to connect/);
});
