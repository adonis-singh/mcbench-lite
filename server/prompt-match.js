'use strict';

// Which library prompt a message stored before messages recorded their
// prompts was sent from, where that is certain. Store's v14 migration links
// old messages and bench rounds with it once; a message sent since records
// its prompts itself (the composer knows what it inserted).
//
// A prompt matches a message when the message is exactly what inserting it
// and sending gives: its text for a prompt without fill-ins, and its text
// with each fill-in replaced by some text that is not blank (the composer
// sends no box left empty) for one with them, and the prompt's own words
// make up at least half of the message: a prompt that is mostly fill-ins (or
// all, as "{{subject}}" is) matches messages typed by hand, and so says
// nothing. The message is trimmed as the composer sends it, and so is the
// prompt. The match is certain only when exactly one prompt in the library
// matches.

const { FILL_IN } = require('../web/composer.mjs');

const escaped = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

// The prompt's text outside its fill-ins, piece by piece, and the expression
// a message sent from it matches.
function pattern(prompt) {
  // split() with FILL_IN's one group puts the fill-ins' names at odd places.
  const fixed = prompt.text.trim().split(FILL_IN).filter((_, index) => index % 2 === 0);
  return {
    length: fixed.join('').length,
    expression: new RegExp(`^${fixed.map(escaped).join('[\\s\\S]*?\\S[\\s\\S]*?')}$`),
  };
}

// `library` is prompts.list(); returns a function of a message's text giving
// the prompt it certainly came from, or null.
function certainPrompt(library) {
  const patterns = library.map((prompt) => ({ prompt, ...pattern(prompt) }));
  return (text) => {
    const message = text.trim();
    const matches = patterns.filter((entry) => entry.length > 0 && entry.length * 2 >= message.length && entry.expression.test(message));
    return matches.length === 1 ? matches[0].prompt : null;
  };
}

module.exports = { certainPrompt };
