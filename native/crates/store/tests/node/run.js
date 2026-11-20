'use strict';

// Runs ops.json (gen.js) through today's Node store on a clone, writing each
// op's result to results.json. Each op builds its message as the server's
// writer of that kind does, in that writer's key order, and its world edits
// through `World`'s own calls (clone, set, finalize).
//
//   MCBENCH_NODE_DIFF=<checkout> MCBENCH_DATA_DIR=<clone> \
//     node -r ./det.js run.js <inputs folder> <out folder>

const fs = require('node:fs');
const path = require('node:path');

const W = process.env.MCBENCH_NODE_DIFF;
const [inputs, out] = process.argv.slice(2);
const store = require(path.join(W, 'server', 'store'));
const storage = require(path.join(W, 'server', 'storage'));
const prompts = require(path.join(W, 'server', 'prompts'));
const settings = require(path.join(W, 'server', 'settings'));
const { settingsSummary } = require('./read');
const ops = JSON.parse(fs.readFileSync(path.join(inputs, 'ops.json'), 'utf8'));

const vars = new Map();
const benches = new Map();
function resolve(value) {
  if (typeof value === 'string' && value.startsWith('$')) {
    const indexed = value.match(/^(.*)\[(\d+)\]$/);
    if (indexed) return resolve(indexed[1])[Number(indexed[2])];
    if (!vars.has(value)) throw new Error(`unknown variable ${value}`);
    return vars.get(value);
  }
  if (Array.isArray(value)) return value.map(resolve);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, resolve(v)]));
  return value;
}
function chat(id) {
  const conversation = store.getConversation(resolve(id));
  if (!conversation) throw new Error(`no chat ${resolve(id)}`);
  return conversation;
}
const shown = (message) => JSON.stringify(message);
const remember = (as, value) => {
  if (as) vars.set(as, value);
  return value;
};

function built(conversation, blocks) {
  const world = conversation.world.clone();
  for (const [x, y, z, id, states] of blocks) world.set(x, y, z, id, states);
  world.finalize();
  return world;
}
function file(entry) {
  if (!entry.fill) return entry;
  const { byte, count, mime } = entry.fill;
  return { name: entry.name, data: `data:${mime};base64,${Buffer.alloc(count, byte).toString('base64')}` };
}
const replyPayload = (message) => {
  const toolCalls = message.tool_calls || [];
  return {
    role: 'assistant',
    content: message.content || '',
    ...(message.reasoning ? { reasoning: message.reasoning } : {}),
    ...(message.reasoning_details ? { reasoning_details: message.reasoning_details } : {}),
    ...(message.finish_reason ? { finish_reason: message.finish_reason } : {}),
    ...(message.telemetry ? { telemetry: message.telemetry } : {}),
    ...(message.notices ? { notices: message.notices } : {}),
    ...(message.plan ? { plan: message.plan } : {}),
    ...(message.rateLimit ? { rateLimit: message.rateLimit } : {}),
    ...(toolCalls.length ? { tool_calls: toolCalls } : {}),
  };
};
const finalPayload = (final) => (final.kind === 'stopped'
  ? { role: 'assistant', content: final.content, stopped: true, ...(final.reply ? { reply: final.reply } : {}), ...(final.telemetry ? { telemetry: final.telemetry } : {}) }
  : { role: 'error', content: final.content, ...(final.telemetry ? { telemetry: final.telemetry } : {}) });
const appended = (conversation, payload, checkpoint) => {
  const message = store.appendMessage(conversation, payload, checkpoint ? conversation.world.reference() : null);
  vars.set('$last', message.id);
  return shown(message);
};
const member = (op, m = op.member) => chat(`${op.bench}.member${m}`);
const benchOf = (op) => {
  const bench = benches.get(op.bench);
  if (!bench) throw new Error(`no bench ${op.bench}`);
  return bench;
};
const branchOf = (conversation) => ({ head: conversation.currentMessageId, messages: conversation.messages.length });

const handlers = {
  now: (op) => { globalThis.__now = op.ms; return null; },
  get: (op) => branchOf(chat(op.chat)),
  create: (op) => remember(op.as, store.createConversation(op.provider, op.model, { effort: op.effort ?? null, resolution: op.resolution ?? 1 }).id),
  prompt: (op) => {
    const conversation = chat(op.chat);
    const links = resolve(op.prompts);
    const text = appended(conversation, { role: 'user', content: resolve(op.content), ...(links.length ? { prompts: links } : {}) }, true);
    remember(op.as, vars.get('$last'));
    return text;
  },
  reply: (op) => appended(chat(op.chat), replyPayload(op.reply), false),
  toolResult: (op) => appended(chat(op.chat), { role: 'tool', tool_call_id: op.callId, telemetry: op.telemetry, content: op.content }, false),
  unanswered: (op) => appended(chat(op.chat), { role: 'tool', tool_call_id: op.callId, content: JSON.stringify(store.unansweredResult(op.stopped, op.reason)) }, false),
  screenshots: (op) => appended(chat(op.chat), { role: 'user', seeBuild: true, content: [{ type: 'text', text: op.text }, ...resolve(op.urls).map((url) => ({ type: 'image_url', image_url: { url } }))] }, false),
  save: (op) => { store.save(chat(op.chat)); return null; },
  build: (op, index) => {
    const conversation = chat(op.chat);
    conversation.world = built(conversation, op.blocks);
    return conversation.world.size();
  },
  buildMember: (op, index) => {
    const conversation = member(op);
    conversation.world = built(conversation, op.blocks);
    return conversation.world.size();
  },
  persistWorld: (op, index) => {
    const conversation = chat(op.chat);
    store.persistWorld(conversation.id, built(conversation, op.blocks));
    return null;
  },
  revert: (op) => shown(store.revert(chat(op.chat), resolve(op.message))),
  editPrompt: (op) => {
    const conversation = chat(op.chat);
    store.editPrompt(conversation, resolve(op.message), () => {
      appended(conversation, { role: 'user', content: resolve(op.content) }, true);
      remember(op.as, vars.get('$last'));
      store.save(conversation);
    });
    return branchOf(conversation);
  },
  activate: (op) => {
    const conversation = chat(op.chat);
    store.activateBranch(conversation, resolve(op.message));
    return branchOf(conversation);
  },
  activateAll: (op) => {
    const conversation = chat(op.chat);
    const ids = store.messageTree(conversation).messages.filter((message) => message.branchable).map((message) => message.id);
    return ids.map((id) => {
      store.activateBranch(store.getConversation(conversation.id), id);
      const now = store.getConversation(conversation.id);
      return [id, now.currentMessageId, now.messages.length, JSON.stringify(now.world.serializeManifest(() => { throw new Error('stored'); }))];
    });
  },
  setTitle: (op) => { store.setTitle(chat(op.chat), op.title); return null; },
  markFinished: (op) => { store.markFinished(resolve(op.chat)); return null; },
  markSeen: (op) => { store.markSeen(resolve(op.chat)); return null; },
  dismiss: (op) => { store.dismissError(chat(op.chat), resolve(op.message)); return null; },
  endTurn: (op) => {
    const message = store.endTurn(chat(op.chat), finalPayload(op.final));
    vars.set('$last', message.id);
    return shown(message);
  },
  endUnfinished: (op) => store.endUnfinishedTurn(chat(op.chat), op.reason),
  interrupt: () => store.interruptUnfinished(),
  archive: (op) => { store.archiveConversation(resolve(op.chat)); return null; },
  restore: (op) => { store.restoreConversation(resolve(op.chat)); return null; },
  delete: (op) => { store.deleteConversation(resolve(op.chat)); return null; },
  archiveBench: (op) => store.archiveBench(resolve(op.bench)),
  restoreBench: (op) => { store.restoreBench(resolve(op.bench)); return null; },
  deleteBench: (op) => { store.deleteBench(resolve(op.bench)); return null; },
  purge: (op) => { store.clearExpiredArchive(op.now); return null; },
  nextExpiry: (op) => store.nextArchiveExpiry(op.now),
  collect: () => { store.collectChunks(); return null; },
  createBench: (op) => {
    const { bench, members } = store.createBench(op.id, { title: op.title, blind: op.blind, resolution: op.resolution, content: resolve(op.content), prompts: op.prompts, members: op.members });
    if (op.as) {
      vars.set(op.as, bench.id);
      benches.set(op.as, bench);
      members.forEach((chat, m) => vars.set(`${op.as}.member${m}`, chat.id));
    }
    return { bench, members: members.map((chat) => chat.id) };
  },
  loadBench: (op) => {
    const bench = store.getBench(op.id);
    vars.set(op.as, bench.id);
    benches.set(op.as, bench);
    store.benchMembers(bench.id).forEach((chat, m) => vars.set(`${op.as}.member${m}`, chat.id));
    return bench;
  },
  benchPrompt: (op) => {
    const bench = benchOf(op);
    const conversation = member(op);
    const round = bench.rounds[op.index];
    const sent = { role: 'user', round: round.id, ...(round.prompts ? { prompts: round.prompts } : {}) };
    let { content } = round;
    if (op.share && typeof content !== 'string') {
      content = content.map((part) => (part.type === 'attachment' ? { ...part, attachment: store.shareAttachment(part.attachment, conversation.id) } : part));
    }
    const text = appended(conversation, { ...sent, content }, true);
    store.save(conversation);
    return text;
  },
  memberReply: (op) => {
    const conversation = member(op);
    if (!op.reply) return shown(store.endTurn(conversation, { role: 'error', content: 'Provider failed' }));
    const text = appended(conversation, replyPayload(op.reply), false);
    store.save(conversation);
    return text;
  },
  benchProgress: (op) => [0, 1, 2].map((m) => store.benchProgress(resolve(`${op.bench}.member${m}`))),
  appendRound: (op) => { const bench = benchOf(op); store.appendRound(bench, op.content, op.prompts); return bench; },
  vote: (op) => {
    const bench = benchOf(op);
    const id = (m) => resolve(`${op.bench}.member${m}`);
    store.castVote(bench, bench.rounds[op.round].id, op.pick === null ? null : id(op.pick), op.judged.map(id));
    return bench;
  },
  clearVote: (op) => { const bench = benchOf(op); store.clearVote(bench, bench.rounds[op.round].id); return bench; },
  reveal: (op) => { const bench = benchOf(op); store.revealBench(bench); return bench; },
  editRound: (op) => {
    const bench = benchOf(op);
    const reverted = store.editBenchRound(bench, op.index, op.content, op.links);
    return { bench, reverted: reverted.map((chat) => chat.id) };
  },
  followBranch: (op) => {
    const bench = benchOf(op);
    const round = bench.rounds[op.round];
    const id = op.which === 'self' ? round.id : op.which === 'previous' ? round.branch.previousId : round.branch.nextId;
    if (!id) return 'none';
    store.followBenchBranch(bench, id);
    return { bench, heads: [0, 1, 2].map((m) => (vars.has(`${op.bench}.member${m}`) ? branchOf(member(op, m)) : null)) };
  },
  retryRound: (op) => { const bench = benchOf(op); store.retryBenchRound(bench, member(op)); return bench; },
  benchTitle: (op) => { const bench = benchOf(op); store.setBenchTitle(bench, op.title); return bench; },
  forkMember: (op) => {
    const source = member(op);
    return remember(op.as, store.forkConversation(source, source.messages.length, source.world).id);
  },
  leaderboard: () => store.leaderboard(),
  voteCount: () => store.voteCount(),
  clearAllVotes: () => { store.clearAllVotes(); return null; },
  fork: (op) => {
    const source = chat(op.chat);
    const world = op.world === 'head' ? source.world : source.checkpoints.get(resolve(op.world.slice('checkpoint:'.length)));
    if (!world) throw new Error('no such checkpoint');
    return remember(op.as, store.forkConversation(source, op.count, world).id);
  },
  images: (op) => remember(op.as, store.saveImages(resolve(op.chat), op.images.map((image) => (image.png !== undefined ? Buffer.from(image.png, 'base64') : image.dataUrl)), op.prefix)),
  attachments: (op) => remember(op.as, store.saveAttachments(resolve(op.chat), op.files.map(file))),
  repeats: (op) => store.repeatsLatestRootPrompt(chat(op.chat), resolve(op.content)),
  reserve: (op) => { storage.reserve(resolve(op.chat), op.bytes); return storage.used(resolve(op.chat)); },
  refund: (op) => { storage.refund(resolve(op.chat), op.bytes); return storage.used(resolve(op.chat)); },
  refusal: (op) => storage.refusal(resolve(op.chat)),
  favorites: (op) => settingsSummary(settings.updateModelFavorites(op.value)),
  settingsUpdate: (op) => settingsSummary(settings.update(op.next)),
  promptsApply: (op) => prompts.apply(op.args, op.by),
  promptsSave: (op) => prompts.save(op.prompts),
  promptsList: () => prompts.list(),
};

const results = ops.map((op, index) => {
  const handler = handlers[op.op];
  if (!handler) throw new Error(`unknown op ${op.op}`);
  try {
    // Written now: a bench object a later op changes is as this op left it.
    return [op.op, JSON.parse(JSON.stringify({ ok: handler(op, index) ?? null }))];
  } catch (error) {
    return [op.op, { error: String(error.message), status: error.statusCode ?? null }];
  }
});
store.close();
fs.mkdirSync(out, { recursive: true });
fs.writeFileSync(path.join(out, 'results.json'), JSON.stringify(results));
