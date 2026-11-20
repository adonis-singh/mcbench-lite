'use strict';

// One run of the store timings through today's Node store, in process, on
// a clone: opening the store, listing chats, loading the chat with the most
// messages and the chat with the largest world, a search, and appending a
// message with a save. Prints one JSON line of milliseconds.
//
//   MCBENCH_NODE_DIFF=<checkout> MCBENCH_DATA_DIR=<clone> node bench.js

const path = require('node:path');

const W = process.env.MCBENCH_NODE_DIFF;
const Database = require(path.join(W, 'node_modules', 'better-sqlite3'));
const dataDir = process.env.MCBENCH_DATA_DIR;
const probe = new Database(path.join(dataDir, 'mcbench.sqlite3'), { readonly: true });
const most = probe.prepare('SELECT conversation_id AS id FROM messages m JOIN conversations c ON c.id = m.conversation_id WHERE c.archived_at IS NULL GROUP BY conversation_id ORDER BY count(*) DESC LIMIT 1').get().id;
const largest = probe.prepare('SELECT id FROM conversations WHERE archived_at IS NULL ORDER BY length(world_manifest_json) DESC LIMIT 1').get().id;
probe.close();

const time = (fn) => {
  const start = process.hrtime.bigint();
  const value = fn();
  return [Number(process.hrtime.bigint() - start) / 1e6, value];
};
const out = {};
let store;
[out.open, store] = time(() => require(path.join(W, 'server', 'store')));
[out.list] = time(() => store.listConversations());
[out.loadMost] = time(() => store.getConversation(most));
[out.loadLargestWorld] = time(() => store.getConversation(largest));
[out.search] = time(() => store.search('build', 'tower'));
const chat = store.getConversation(most);
[out.appendSave] = time(() => {
  store.appendMessage(chat, { role: 'user', content: 'Timing prompt' }, chat.world.reference());
  store.save(chat);
});
store.close();
console.log(JSON.stringify(out));
