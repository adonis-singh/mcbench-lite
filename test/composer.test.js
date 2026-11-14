'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

const castle = { id: 'castle', title: 'Castle', text: 'Build a castle.' };
const tower = { id: 'tower', title: 'Tower', text: 'Build a {{size}} tower lit by {{light}}.' };

test('a prompt arrives as a token, or as its text with boxes that name the prompt they came from', async () => {
  const { promptSegments } = await import('../web/composer.mjs');
  assert.deepEqual(promptSegments(castle), [{ type: 'prompt', id: 'castle', title: 'Castle', text: 'Build a castle.' }]);
  const from = { id: 'tower', title: 'Tower' };
  assert.deepEqual(promptSegments(tower), [
    { type: 'text', text: 'Build a ' },
    { type: 'field', name: 'size', value: '', from },
    { type: 'text', text: ' tower lit by ' },
    { type: 'field', name: 'light', value: '', from },
    { type: 'text', text: '.' },
  ]);
});

test('a message names each prompt it was inserted from once, while its token or any of its boxes is in it', async () => {
  const { promptSegments, promptLinks } = await import('../web/composer.mjs');
  const [size, , light] = promptSegments(tower).slice(1);
  assert.deepEqual(promptLinks([]), []);
  assert.deepEqual(promptLinks([{ type: 'text', text: 'Build a hut' }]), []);
  // The prompt's text was edited around its boxes, and one box was deleted.
  const edited = [{ type: 'text', text: 'Please build a ' }, { ...size, value: 'huge' }, { type: 'text', text: ' tower, lit well' }];
  assert.deepEqual(promptLinks(edited), [{ id: 'tower', title: 'Tower' }]);
  assert.deepEqual(promptLinks([...promptSegments(castle), ...edited, { ...light, value: 'torches' }, ...promptSegments(castle)]), [{ id: 'castle', title: 'Castle' }, { id: 'tower', title: 'Tower' }]);
  assert.deepEqual(promptLinks([{ type: 'text', text: 'Build a huge tower lit by torches.' }]), [], 'its text alone, every box gone, is no longer the prompt');
});
