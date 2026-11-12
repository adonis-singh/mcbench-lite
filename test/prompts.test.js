'use strict';

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-prompts-test-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const prompts = require('../server/prompts');
const llm = require('../server/llm');
const requests = [];
llm.chatCompletion = async (options) => { requests.push(options); return { content: 'done' }; };
llm.modelInfo = async () => ({ imageInput: true });
const store = require('../server/store');
const { runTurn, toolsForImageInput, executeToolCall } = require('../server/agent');

after(() => { store.close(); fs.rmSync(dataDir, { recursive: true, force: true }); });

test('the library starts from the seed prompts and saves the order the user gives it', () => {
  const seeded = prompts.list();
  assert.ok(seeded.length > 0);
  const saved = prompts.save([...seeded].reverse());
  assert.deepEqual(prompts.list().map((p) => p.id), saved.map((p) => p.id));
  assert.equal(prompts.list()[0].id, seeded.at(-1).id);
});

// The page renders a prompt's attachments: one saved with a link (a pasted
// javascript: URL) or a remote image instead of its bytes is refused, and
// only the file's own fields are kept.
test('a prompt keeps attachments that carry their bytes, and nothing else', () => {
  const data = `data:image/png;base64,${Buffer.from('png').toString('base64')}`;
  const [saved] = prompts.save([{ id: 'ref', title: 'Ref', text: 'x', attachments: [{ name: 'ref.png', type: 'text/html', size: 3, data, url: 'javascript:alert(1)' }] }]);
  assert.deepEqual(saved.attachments, [{ name: 'ref.png', type: 'image/png', size: 3, data }]);
  for (const attachment of [{ name: 'a.png', data: 'https://example.com/a.png' }, { name: 'a.png', url: '/api/attachments/x/a.png' }, { data }]) {
    assert.throws(() => prompts.save([{ id: 'ref', title: 'Ref', text: 'x', attachments: [attachment] }]), (error) => error.statusCode === 400);
  }
});

const CHAT = { chat: 'chat-1', model: 'test/model' };

test('a model reads the library and adds prompts, which are marked with its chat and model', () => {
  prompts.save([
    { id: 'a', title: 'Castle', text: 'Build a castle', tags: ['Medieval'], attachments: [{ name: 'ref.png', type: 'image/png', data: 'data:image/png;base64,AA==' }] },
    { id: 'b', title: 'Tower', text: 'Build a tower', tags: [] },
  ]);
  const read = prompts.apply({}, CHAT);
  assert.deepEqual(read.map((p) => p.id), ['a', 'b']);
  assert.deepEqual(read[0].attachments, ['ref.png']);
  assert.deepEqual(read[0].tags, ['medieval']);
  assert.equal(read[0].addedBy, undefined);

  const after = prompts.apply({ save: [{ title: 'Moat', text: 'Dig a {{shape}} moat', tags: ['medieval'] }] }, CHAT);
  assert.deepEqual(after.map((p) => p.title), ['Castle', 'Tower', 'Moat']);
  assert.deepEqual(after[2].addedBy, CHAT);
  assert.deepEqual(prompts.list()[2].addedBy, CHAT);
  assert.throws(() => prompts.apply({ save: [{ title: 'Empty' }] }, CHAT), /needs text/);
  assert.throws(() => prompts.apply({ remove: 'b' }, CHAT), /remove must be an array/);
});

// A rewritten prompt would keep the user's title and go to every model of a
// later bench, so a model cannot touch a prompt it did not add.
test('a model cannot change, remove or reorder a prompt it did not add', () => {
  prompts.save([
    { id: 'user', title: 'Castle', text: 'Build a castle' },
    { id: 'other-chat', title: 'Hut', text: 'Build a hut', addedBy: { chat: 'chat-2', model: 'test/model' } },
    { id: 'other-model', title: 'Barn', text: 'Build a barn', addedBy: { chat: 'chat-1', model: 'other/model' } },
  ]);
  const before = prompts.list();
  for (const id of ['user', 'other-chat', 'other-model']) {
    assert.throws(() => prompts.apply({ save: [{ id, text: 'Something else' }] }, CHAT), new RegExp(`Prompt ${id} was not added by you in this chat`));
    assert.throws(() => prompts.apply({ remove: [id] }, CHAT), new RegExp(`Prompt ${id} was not added by you in this chat`));
  }
  assert.throws(() => prompts.apply({ order: ['other-model', 'user'] }, CHAT), /takes only save and remove; not order/);
  assert.throws(() => prompts.apply({ save: [{ id: 'missing', title: 'x' }] }, CHAT), /No prompt with id missing/);
  // A model cannot set the mark, favorites or attachments of what it adds.
  for (const field of [{ addedBy: null }, { favorite: true }, { attachments: [] }]) {
    assert.throws(() => prompts.apply({ save: [{ text: 'Build a well', ...field }] }, CHAT), /has only the fields id, title, text, tags/);
  }
  assert.deepEqual(prompts.list(), before);
});

test('a model changes and removes the prompts it added, and the user keeps or edits them as any other', () => {
  prompts.save([{ id: 'user', title: 'Castle', text: 'Build a castle' }]);
  const [, added] = prompts.apply({ save: [{ title: 'Moat', text: 'Dig a moat' }] }, CHAT);
  prompts.apply({ save: [{ id: added.id, text: 'Dig a deep moat', tags: ['Water'] }] }, CHAT);
  assert.deepEqual(prompts.list()[1], { id: added.id, title: 'Moat', text: 'Dig a deep moat', tags: ['water'], favorite: false, attachments: [], addedBy: CHAT });
  // The page saves the whole list; the mark stays on a prompt the user edits.
  prompts.save(prompts.list().map((p) => (p.id === added.id ? { ...p, title: 'Deep moat', favorite: true } : p)));
  assert.deepEqual(prompts.list()[1].addedBy, CHAT);
  prompts.apply({ remove: [added.id] }, CHAT);
  assert.deepEqual(prompts.list().map((p) => p.id), ['user']);
});

test('the tool call marks a prompt with the chat and model that added it', async () => {
  prompts.save([]);
  const conversation = store.createConversation('openrouter', 'test/model');
  const { result } = await executeToolCall(conversation, 'prompt_library', { save: [{ title: 'Well', text: 'Build a well' }] }, { chat: true });
  assert.equal(result.ok, true);
  assert.deepEqual(prompts.list()[0].addedBy, { chat: conversation.id, model: 'test/model' });
  const refused = await executeToolCall(conversation, 'prompt_library', { save: [{ id: 'nope', text: 'x' }] }, { chat: true });
  assert.deepEqual(refused.result, { ok: false, error: 'No prompt with id nope' });
});

test('prompt_library is a chat tool, never a benchmark tool', async () => {
  assert.equal(prompts.TOOL.function.name, 'prompt_library');
  assert.match(prompts.TOOL.function.description, /\{\{like this\}\}/);
  const names = (options) => options.tools.map((tool) => tool.function.name);
  const turn = async (chat) => {
    const conversation = store.createConversation('openrouter', 'test/model');
    store.appendMessage(conversation, { role: 'user', content: 'hi' });
    await runTurn(conversation, {}, () => {}, new AbortController().signal, () => {}, chat === undefined ? undefined : { chat });
    return requests.at(-1);
  };
  const benchmark = await turn(undefined);
  assert.deepEqual(names(benchmark), toolsForImageInput(true).map((tool) => tool.function.name));
  assert.equal(benchmark.chat, false);
  const chat = await turn(true);
  assert.deepEqual(names(chat), [...names(benchmark), 'prompt_library']);
  assert.equal(chat.chat, true);
});
