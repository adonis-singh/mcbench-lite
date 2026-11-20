'use strict';

// The request today's apiMessages makes at every assistant message of the
// active branch of every stored chat (the history before that message), for
// a model that takes images and a text-only one, as one JSON line per
// request: { chat, step, variant, sha256, bytes } or { chat, step, variant,
// error }. The budget is MCBENCH_CONTEXT_MAX_BYTES, set by the caller.
//
//   MCBENCH_NODE_DIFF=<checkout> MCBENCH_DATA_DIR=<clone> node api.js <out.jsonl> [chat:step:variant <dump.json>]

const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');

const W = process.env.MCBENCH_NODE_DIFF;
const store = require(path.join(W, 'server', 'store'));
const { _internal } = require(path.join(W, 'server', 'agent'));

const [out, dumpKey, dumpFile] = process.argv.slice(2);
const lines = [];
const ids = store.listConversationIds().filter((chat) => !chat.archived).map((chat) => chat.id).sort();
for (const id of ids) {
  const conversation = store.getConversation(id);
  conversation.messages.forEach((message, step) => {
    if (message.role !== 'assistant') return;
    const at = { ...conversation, messages: conversation.messages.slice(0, step) };
    for (const variant of ['image', 'text']) {
      const key = `${id}:${step}:${variant}`;
      try {
        const text = JSON.stringify(_internal.apiMessages(at, { imageInput: variant === 'image' }));
        if (key === dumpKey) fs.writeFileSync(dumpFile, text);
        lines.push(JSON.stringify({ chat: id, step, variant, sha256: crypto.createHash('sha256').update(text).digest('hex'), bytes: Buffer.byteLength(text) }));
      } catch (error) {
        lines.push(JSON.stringify({ chat: id, step, variant, error: String(error.message) }));
      }
    }
  });
}
fs.writeFileSync(out, lines.join('\n') + '\n');
store.close();
