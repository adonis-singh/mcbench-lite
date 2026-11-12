'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');

const source = fs.readFileSync(path.join(__dirname, '..', 'web', 'page-errors.js'), 'utf8');

// web/page-errors.js in a page of its own: its window listeners, and the
// #page-error element it writes into.
function page() {
  class ErrorEvent { constructor(fields) { Object.assign(this, fields); } }
  class HTMLScriptElement { constructor(src) { this.src = src; } }
  const listeners = {};
  const text = { textContent: '' };
  const button = { onclick: null };
  const sink = { hidden: true, querySelector: (selector) => (selector === 'button' ? button : text) };
  const reloads = [];
  const context = {
    ErrorEvent,
    HTMLScriptElement,
    URL,
    location: { reload: () => reloads.push(1) },
    window: { addEventListener: (type, listener, capture) => { listeners[type] = { listener, capture }; } },
    document: { getElementById: (id) => (id === 'page-error' ? sink : null) },
  };
  vm.runInNewContext(source, context);
  return { context, listeners, sink, text, button, reloads };
}

test('an uncaught error shows outside the app, with a Reload button', () => {
  const { context, listeners, sink, text, button, reloads } = page();
  assert.equal(listeners.error.capture, true);
  listeners.error.listener(new context.ErrorEvent({ message: 'Uncaught Error: x', error: new Error('Unknown place kind: nowhere') }));
  assert.equal(sink.hidden, false);
  assert.equal(text.textContent, 'The page hit an error: Unknown place kind: nowhere');
  button.onclick();
  assert.equal(reloads.length, 1);
  // The first failure stays: later ones usually follow from it.
  listeners.unhandledrejection.listener({ reason: new Error('later') });
  assert.equal(text.textContent, 'The page hit an error: Unknown place kind: nowhere');
});

test('a script that failed to load says which, and a failed image says nothing', () => {
  const { context, listeners, sink, text } = page();
  listeners.error.listener({ target: { src: 'http://127.0.0.1:3020/logo.png' } });
  assert.equal(sink.hidden, true);
  listeners.error.listener({ target: new context.HTMLScriptElement('http://127.0.0.1:3020/app.js') });
  assert.equal(sink.hidden, false);
  assert.equal(text.textContent, 'The page could not load /app.js or a module it imports.');
});

test('an unhandled rejection shows its reason', () => {
  const { listeners, text } = page();
  listeners.unhandledrejection.listener({ reason: 'plain string' });
  assert.equal(text.textContent, 'The page hit an error: plain string');
});
