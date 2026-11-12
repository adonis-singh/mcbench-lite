'use strict';

// What the page takes from outside it: a paste (any page can put HTML on the
// clipboard) and a title it names a download's folder after.

const assert = require('node:assert/strict');
const test = require('node:test');

// The one DOMParser call readMessageClipboard makes: the copy's element and
// its attribute, decoded as the browser decodes it.
globalThis.DOMParser = class {
  parseFromString(html) {
    const match = /data-mcbench-message="([^"]*)"/.exec(html);
    const value = match && match[1].replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, '<').replace(/&gt;/g, '>').replace(/&amp;/g, '&');
    return { querySelector: () => (match ? { getAttribute: () => value } : null) };
  }
};
const clipboard = (html) => ({ getData: (type) => (type === 'text/html' ? html : '') });

test('a pasted message keeps text and attachments with their bytes, and nothing they could link to', async () => {
  const { messageClipboardHtml, readMessageClipboard } = await import('../web/clipboard.mjs');
  const data = `data:image/png;base64,${Buffer.from('png!').toString('base64')}`;
  const copied = await messageClipboardHtml({ text: 'Build <this>', attachments: [{ name: 'ref.png', type: 'image/png', size: 4, data }] });
  assert.deepEqual(readMessageClipboard(clipboard(copied)), { text: 'Build <this>', attachments: [{ name: 'ref.png', type: 'image/png', size: 4, data }] });
  // A page's own copy: a file chip linking to script, a lying size and type.
  const forged = (attachments) => `<div data-mcbench-message="${JSON.stringify({ text: 'x', attachments }).replace(/&/g, '&amp;').replace(/"/g, '&quot;')}"></div>`;
  assert.deepEqual(readMessageClipboard(clipboard(forged([{ name: 'a.pdf', type: 'image/png', size: 1e12, data, url: 'javascript:alert(1)' }]))).attachments,
    [{ name: 'a.pdf', type: 'image/png', size: 4, data }]);
  for (const attachment of [{ name: 'a.png', data: 'https://example.com/a.png' }, { name: 'a.png', url: 'javascript:alert(1)' }, { data }]) {
    assert.throws(() => readMessageClipboard(clipboard(forged([attachment]))), /not a file/);
  }
  assert.equal(readMessageClipboard(clipboard('<p>plain</p>')), null);
});

test('a world export of a title of dots stays in a folder of its own', async () => {
  const { worldZip } = await import('../web/world-export.mjs');
  for (const [name, folder] of [['..', 'world'], ['.', 'world'], [' ... ', 'world'], ['../x', '.._x'], ['My build', 'My build']]) {
    assert.equal((await worldZip({ name, gameTime: 0, chunks: [] }, [0, 64, 0])).folder, folder, name);
  }
});
