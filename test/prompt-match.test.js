'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');
const { certainPrompt } = require('../server/prompt-match');

const castle = { id: 'castle', title: 'Castle', text: 'Build a castle with a moat and four towers.' };
const tower = { id: 'tower', title: 'Tower', text: '  Build a {{size}} tower with a spiral stair, lit by {{light}}.\n' };

test('a prompt without fill-ins matches exactly its text, as the composer sends it', () => {
  const sentFrom = certainPrompt([castle, tower]);
  assert.equal(sentFrom(castle.text), castle);
  assert.equal(sentFrom(`\n ${castle.text}  `), castle, 'the composer trims what it sends');
  assert.equal(sentFrom(`${castle.text} Add a drawbridge.`), null, 'text added to it is not certain');
  assert.equal(sentFrom('Build a castle with a moat and three towers.'), null);
  assert.equal(sentFrom(''), null);
});

test('a prompt with fill-ins matches its text with every fill-in replaced by text that is not blank', () => {
  const sentFrom = certainPrompt([castle, tower]);
  assert.equal(sentFrom('Build a huge tower with a spiral stair, lit by torches.'), tower);
  assert.equal(sentFrom('Build a huge,\ncrooked tower with a spiral stair, lit by {{light}}.'), tower, 'a fill-in may hold any text, braces and new lines too');
  assert.equal(sentFrom('Build a   tower with a spiral stair, lit by torches.'), null, 'a blank fill-in is never sent');
  assert.equal(sentFrom('Build a huge tower with a spiral stair, lit by torches. And a moat'), null, 'its own words must all be there');
  assert.equal(sentFrom('Build a huge tower, lit by torches.'), null);
});

test('a match is certain only when one prompt matches', () => {
  const bridge = { id: 'bridge', title: 'Bridge', text: 'Build a {{material}} bridge over the river.' };
  const crossing = { id: 'crossing', title: 'Crossing', text: 'Build a stone {{thing}} over the river.' };
  const sentFrom = certainPrompt([bridge, crossing, { ...castle, id: 'castle-copy' }, castle]);
  assert.equal(sentFrom('Build a stone bridge over the river.'), null, 'either prompt gives this message');
  assert.equal(sentFrom('Build a wooden bridge over the river.'), bridge);
  assert.equal(sentFrom(castle.text), null, 'two prompts with the same text');
});

test("a prompt that is mostly fill-ins says nothing, and does not stand in another prompt's way", () => {
  const anything = { id: 'anything', title: 'Anything', text: '{{subject}}' };
  const short = { id: 'short', title: 'Short', text: 'Build {{subject}}' };
  const sentFrom = certainPrompt([anything, short, castle, { id: 'empty', title: '', text: '' }]);
  assert.equal(sentFrom('A hut by the sea'), null);
  assert.equal(sentFrom('Build a hut by the sea, with a jetty and a boat'), null, "the prompt's own words are under half the message");
  assert.equal(sentFrom('Build a hut'), short);
  assert.equal(sentFrom(castle.text), castle);
});
