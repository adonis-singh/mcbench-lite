'use strict';

const crypto = require('crypto');
const fs = require('fs');
const http = require('http');
const path = require('path');

require('./dotenv').loadDotEnv();

// The data folder belongs to one server process: `server.pid` in it holds
// the owner's pid (pid-lock.js). It is taken here, before any module that
// changes the folder is loaded (store.js migrates and collects at load), so
// a second server on the folder fails at once and touches nothing. Given
// back when this one has stopped; a dead owner's is taken over.
const { DATA_DIR } = require('./storage');
const { pidLock } = require('./pid-lock');
fs.mkdirSync(DATA_DIR, { recursive: true });
const releaseDataFolder = pidLock(path.join(DATA_DIR, 'server.pid'));

const { getAsset, getBlockRegistry, MC_VERSION, ASSETS } = require('./assets');
const { runTurn } = require('./agent');
const sandbox = require('./sandbox');
const harness = require('./harness');
const store = require('./store');
const { chatCompletion, closeHarnessSession, modelInfo, listModels, PROVIDERS: LLM_PROVIDERS } = require('./llm');
const { blueMap } = require('./bluemap');
const { minecraftLive } = require('./minecraft-live');
const { LiveViews } = require('./live-views');
const coreWasm = require('./core-wasm');
const { minecraftVerifier } = require('./minecraft-verify');
const { writeSchematic } = require('./schematic');
const { BenchComposer, worldAtRound } = require('./bench-world');
const { describeSelection, readSelection } = require('./selection');
const { blockIcon } = require('./block-icon');
const { generatePrompt } = require('./prompt-generation');
const promptLibrary = require('./prompts');
const { RESOLUTIONS } = require('../web/resolution.mjs');
const settings = require('./settings');
const { World } = require('./world');
const { shotWorld, warmShotEngines, stopShotEngines } = require('./shot-world');
const { warmRenderer, closeRenderer } = require('./shot');
const { Shares, shareId } = require('./share');
const shareViewer = require('./share-viewer');
const { TurnStream, Edit, Reservations } = require('./turn-stream');
const { sendFile } = require('./static-file');
const { supportsImageData, supportsImageType } = require('./image-input');
const { BODY_LIMITS, httpError, readJsonBody, requestUrl, localHosts, foreignRequest, securityHeaders, pagePolicy } = require('./http-guard');
const auth = require('./auth');

const PORT = Number(process.env.PORT || 3020);
// The server listens on loopback only (server.listen below): the sandbox
// holds an API key and can run networked commands, so the app is never
// exposed to the LAN.
store.clearExpiredArchive();
// A purge that fails is tried again the next day; it is no reason to stop
// the server (exitOnFatal).
const archiveCleanup = setInterval(() => {
  try {
    store.clearExpiredArchive();
    publishConversations();
  } catch (error) {
    console.error('[archive] purge failed:', error);
  }
}, 24 * 60 * 60 * 1000);
archiveCleanup.unref();
const webDir = path.join(__dirname, '..', 'web');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.gif': 'image/gif',
  '.pdf': 'application/pdf',
  '.txt': 'text/plain; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
  '.csv': 'text/csv; charset=utf-8',
  '.svg': 'image/svg+xml',
};

// What an attachment is served as. A user's file may be anything, so only
// these types, which a browser shows and never runs as the app's script,
// keep their type: images (the chat shows them), PDF, plain text, Markdown
// and CSV. Every other file (a script, a stylesheet, HTML, JSON) is sent as
// bytes to save, so `<script src>` of an uploaded .js file is refused as
// not JavaScript and an uploaded page never opens on the app's origin.
const ATTACHMENT_TYPES = Object.fromEntries(['.png', '.jpg', '.jpeg', '.webp', '.gif', '.svg', '.pdf', '.txt', '.md', '.csv'].map((extension) => [extension, MIME[extension]]));

function sendJson(res, status, body) {
  // Serialized before the head is written, so a failure still gets a 500.
  const payload = JSON.stringify(body);
  res.writeHead(status, { 'content-type': 'application/json' });
  res.end(payload);
}

// What holds each chat (turn-stream.js): one turn or edit at a time, taken
// before anything is checked and changed, and per bench (benchKey) while its
// rounds are edited. Turns run detached from HTTP connections: closing the
// tab does NOT cancel a build — only the explicit stop endpoint does. Any
// number of clients can (re)attach to the live stream.
const reservations = new Reservations();
const benchKey = (benchId) => `bench:${benchId}`;

// Runs `work` holding `keys` as one edit, taken now (before work's first
// await); released however it ends.
async function editing(keys, busy, work) {
  const holds = reservations.take(keys, () => new Edit(), busy);
  try {
    return await work();
  } finally {
    keys.forEach((key, i) => reservations.release(key, holds[i]));
  }
}

// Titles are made beside turns, never as part of one: stopping a turn does
// not stop its title. Shutdown aborts them and waits for them.
// `states` is how a chat or bench (by id) stands on its title. Nothing is
// kept for one that is as stored: 'titled', or 'untitled' while its title is
// the default. { state: 'titling', prompt } is a title under way for that
// prompt (a message or round id), the only one whose title is taken;
// { state: 'failed' } waits for the next prompt, which tries again.
const titles = { controller: new AbortController(), tasks: new Set(), states: new Map() };
const BENCH_TITLE = 'New bench';

// `prompts` are the ids on the active line. A title under way for a prompt
// no longer there will be dropped, so its chat or bench is untitled.
function titleState(id, title, untitled, prompts) {
  const entry = titles.states.get(id);
  if (!entry) return title === untitled ? 'untitled' : 'titled';
  if (entry.state === 'titling' && !prompts.includes(entry.prompt)) return 'untitled';
  return entry.state;
}

function inBackground(task) {
  const promise = task.catch((error) => console.error('[title]', error)).then(() => titles.tasks.delete(promise));
  titles.tasks.add(promise);
}

// Every open page holds one stream of the chat list. It is sent whole on any
// change that the sidebar shows: a turn starting or ending, a title, a chat
// being seen or archived, and how many chats and benches are archived.
const listWatchers = new Set();

// Bench chats also carry how far their run has got: rounds sent, and whether
// a failed or stopped turn holds the rest of their queue.
function conversationList() {
  return store.listConversations().map((row) => ({
    ...row,
    ...(row.benchId ? memberProgress(row.id) : {}),
    runningSince: reservations.turn(row.id)?.startedAt ?? null,
  }));
}

function listPayload() {
  const archive = store.listArchive();
  return {
    conversations: conversationList(),
    benches: store.listBenches(),
    archived: { conversations: archive.conversations.length, benches: archive.benches.length },
  };
}

function publishConversations() {
  if (!listWatchers.size) return;
  const line = `${JSON.stringify(listPayload())}\n`;
  for (const res of listWatchers) if (!res.writableEnded) res.write(line);
}

// The longest title kept, in characters.
const TITLE_MAX = 80;

// `chatModel` ({ provider, model }) is the model of what is titled, which
// titles it at low effort when the user has chosen no title model: a few
// words need no more thinking than that, whatever effort the chat runs at.
async function generateTitle(text, images, chatModel, signal) {
  const { provider, model, effort } = settings.get().titleModel ?? { ...chatModel, effort: 'low' };
  const content = images.length
    ? [
      { type: 'text', text: text || 'Name this Minecraft build from the attached image.' },
      ...images.map((url) => ({ type: 'image_url', image_url: { url } })),
    ]
    : text;
  const message = await chatCompletion({
    provider,
    model,
    effort,
    messages: [
      {
        role: 'system',
        content: 'Name this chat in 2 to 6 words. Return only the title, with no quotes or punctuation.',
      },
      { role: 'user', content },
    ],
    tools: [],
    signal,
  });
  const title = String(message.content || '').trim();
  if (!title) throw new Error('Title model returned an empty title');
  // A reply that is not one short line (the model answered the prompt
  // instead of naming it) is no title: the chat keeps its untitled name.
  if (title.length > TITLE_MAX || /[\r\n]/.test(title)) {
    throw new Error(`Title model returned a reply, not a title of a few words: ${JSON.stringify(title.length > 80 ? `${title.slice(0, 80)}…` : title)}`);
  }
  return title;
}

// Titles chat or bench `id` for `prompt`, from any state: a title already
// under way for another prompt is dropped when it arrives. This one is
// `taken`, or its failure `lost`, only while `id` is still titling for it.
function makeTitle(id, prompt, text, images, chatModel, taken, lost) {
  titles.states.set(id, { state: 'titling', prompt });
  const titling = () => {
    const entry = titles.states.get(id);
    return entry?.state === 'titling' && entry.prompt === prompt;
  };
  const { signal } = titles.controller;
  inBackground(generateTitle(text, images, chatModel, signal).then((title) => {
    if (!titling()) return;
    titles.states.delete(id);
    taken(title);
  }, (error) => {
    if (signal.aborted || !titling()) return;
    titles.states.set(id, { state: 'failed' });
    return lost(error);
  }));
}

// A chat's title is kept only if its prompt is still on the active line (it
// was not reverted meanwhile). A title that failed is reported in the
// transcript once no turn or edit holds the chat, and tried again from the
// chat's next message; the chat list push tells an open view to reload.
function titleChat(id, prompt, text, images, chatModel) {
  makeTitle(id, prompt, text, images, chatModel, (title) => {
    const conversation = store.getConversation(id);
    if (!conversation?.messages.some((message) => message.id === prompt)) return;
    store.setTitle(conversation, title);
    publishConversations();
  }, async (error) => {
    for (let hold; (hold = reservations.get(id));) await hold.done;
    const conversation = store.getConversation(id);
    if (!conversation) return;
    store.appendMessage(conversation, { role: 'error', content: `Title generation failed: ${String(error.message || error)}` });
    store.save(conversation);
    publishConversations();
  });
}

// A bench is titled from the first round of its line, read as stored. One
// still untitled (its title failed, the server stopped first, or that round
// was edited before the title came) is titled when it is next sent a round
// or one of its models is retried.
function titleBench(id) {
  const bench = store.getBench(id);
  const [first] = bench.rounds;
  const state = titleState(id, bench.title, BENCH_TITLE, bench.rounds.map((round) => round.id));
  if (!first || (state !== 'untitled' && state !== 'failed')) return;
  const parts = typeof first.content === 'string' ? [{ type: 'text', text: first.content }] : first.content;
  const files = parts.filter((part) => part.type === 'attachment').map((part) => part.attachment);
  const text = parts.filter((part) => part.type === 'text').map((part) => part.text).join('\n');
  const images = files.filter((file) => supportsImageType(file.type)).map((file) => `data:${file.type};base64,${store.readAttachment(file.url).toString('base64')}`);
  const [{ provider, model }] = store.benchMembers(id);
  makeTitle(id, first.id, titleSource(text, files), images, { provider, model }, (title) => {
    const stored = store.getBench(id);
    if (stored?.rounds[0]?.id !== first.id) return;
    store.setBenchTitle(stored, title);
    publishConversations();
  }, (error) => console.error('[title]', error));
}

function releaseTurn(id, turn) {
  reservations.release(id, turn);
  publishConversations();
}

// Runs a turn whose prompt is already appended and saved, and records how it
// ended: the model's final answer, a stop, or an error. Bench chats build
// with the tools every model shares; the prompt library is the user's own.
async function executeTurn(id, turn) {
  const emit = (event) => turn.emit(event);
  let conversation = store.getConversation(id);
  const { benchId } = conversation;
  const startBounds = JSON.stringify(conversation.world.bounds());
  try {
    // Inside the try, so a failure here still ends the turn and its hold.
    emit({ type: 'turn_snapshot', messages: store.publicMessages(conversation), messageTree: store.messageTree(conversation) });
    const blockRegistry = await getBlockRegistry();
    await runTurn(conversation, blockRegistry, emit, turn.controller.signal, () => {}, { chat: !benchId });
    store.save(conversation);
    emit({ type: 'done' });
  } catch (err) {
    const stopped = turn.controller.signal.aborted;
    if (!stopped) console.error('[turn]', err);
    const message = stopped ? '(stopped by user)' : String(err && err.message || err);
    const telemetry = err?.telemetry ? { telemetry: err.telemetry } : {};
    try {
      // A failed save dropped the chat (store.js): the outcome goes on what
      // is stored, after a result for each call the turn left unanswered.
      // Persisted (role "error"): resync after the turn rebuilds the UI from
      // stored messages, so an unpersisted error would flash and vanish.
      // apiMessages skips this role.
      conversation = store.getConversation(id);
      // A stopped reply keeps the text the user saw streaming (`reply`): a
      // harness session that dropped it is given it on its next turn
      // (harness.stoppedReplyNote).
      const stored = store.endTurn(conversation, stopped
        ? { role: 'assistant', content: message, stopped: true, ...(err?.reply ? { reply: err.reply } : {}), ...telemetry }
        : { role: 'error', content: message, ...telemetry });
      if (stopped) {
        emit({ type: 'assistant', text: message, final: true });
        emit({ type: 'done' });
      } else {
        emit({ type: 'error', message, messageId: stored.id });
      }
    } catch (error) {
      // The stored turn stays under way: it reads as failed (store.benchProgress)
      // and is ended before its chat takes another prompt (handleChat,
      // advanceBench), or at the next start (store.interruptUnfinished).
      console.error('[turn] saving how the turn ended failed:', error);
      emit({ type: 'error', message: `${message} (saving this failed: ${String(error.message || error)})` });
    }
  } finally {
    // The turn is released even if this fails; otherwise the chat would stay
    // held until the app restarts.
    try {
      store.markFinished(id);
    } finally {
      closeHarnessSession(id);
      releaseTurn(id, turn);
      if (benchId) advanceBench(benchId);
      frameAfterTurn(id, benchId, startBounds);
    }
  }
}

// The viewer moves to what a finished turn built: the chat's build when the
// turn changed its extent, and every model's build once a bench has no turn
// left running.
function frameAfterTurn(id, benchId, startBounds) {
  if (JSON.stringify(store.getConversation(id).world.bounds()) !== startBounds) liveViews.frame(id);
  if (!benchId || store.benchMembers(benchId).some((member) => reservations.has(member.id))) return;
  liveViews.frame(benchViewId(benchId, 0));
}

// Text, attachments and viewer selections of a message body, checked;
// nothing is stored yet.
// A message's or new bench's resolution (web/resolution.mjs): one of the
// stops, sent every time, never assumed.
function readResolution(body) {
  if (!RESOLUTIONS.some((stop) => stop.value === body.resolution)) throw httpError(400, `Resolution must be one of ${RESOLUTIONS.map((stop) => stop.value).join(', ')}`);
  return body.resolution;
}

function readMessage(body) {
  const text = String(body.text || '').trim();
  const selections = (Array.isArray(body.selections) ? body.selections : []).map(readSelection);
  const files = Array.isArray(body.files) ? body.files : [];
  if (files.length > 8) throw httpError(400, 'A message supports at most 8 attachments');
  if (!text && files.length === 0) throw httpError(400, 'Message text or an attachment is required');
  return { text, selections, files };
}

// Saves a checked message's files under `owner`, then runs `write` with them
// and the message's content: it saves the row that names them. The files
// come first, so if `write` throws they are removed and nothing is left.
function storeMessage({ text, selections, files }, owner, write) {
  const saved = store.saveAttachments(owner, files);
  const content = saved.length > 0 || selections.length > 0
    ? [
      { type: 'text', text },
      ...saved.map((file) => ({ type: 'attachment', attachment: file })),
      ...selections.map((selection) => ({ type: 'selection', selection })),
    ]
    : text;
  try {
    return write({ files: saved, content });
  } catch (error) {
    store.discardAttachments(owner, saved);
    throw error;
  }
}

const titleSource = (text, files) => text || `Build from ${files.map((file) => file.name).join(', ')}`;
const titleImages = (files) => files.filter((file) => supportsImageData(file?.data)).map((file) => file.data);

// A bench's chats are changed only through the bench's routes, which keep
// its models on the same rounds: a chat of its own takes no prompt, revert,
// branch switch or archive.
function ownChat(id) {
  const conversation = store.getConversation(id);
  if (!conversation) throw httpError(404, 'Conversation not found');
  if (conversation.benchId) throw httpError(409, 'A bench chat is changed only through its bench');
  return conversation;
}

async function handleChat(req, res, id) {
  const conversation = ownChat(id);
  // Held before the first await, so two racing POSTs can't both start a
  // turn, and no revert or branch runs under this one.
  const [turn] = reservations.take([id], () => new TurnStream(), 'A turn is already running for this conversation');
  let title = null;
  try {
    const body = await readJsonBody(req, BODY_LIMITS.message);
    if (reservations.closed) throw httpError(503, 'The server is shutting down');
    // Everything is checked before anything changes.
    const provider = String(body.provider || '').trim();
    const model = String(body.model || '').trim();
    if (!Object.hasOwn(settings.PROVIDERS, provider)) throw httpError(400, 'Choose a configured provider');
    if (!model) throw httpError(400, 'Choose a model');
    const resolution = readResolution(body);
    const message = readMessage(body);
    storeMessage(message, id, ({ files, content }) => {
      sandbox.syncAttachments(id, store.ATTACHMENTS_DIR);
      if (typeof body.effort === 'string') conversation.effort = body.effort.trim() || null;
      // The resolution (web/resolution.mjs) is chosen with each message, as the model is.
      conversation.resolution = resolution;
      conversation.provider = provider;
      conversation.model = model;
      // A new first prompt is titled, and so is a chat still untitled (its
      // title failed, the server stopped first, or the prompt it was being
      // titled for was reverted), from this message.
      const state = titleState(id, conversation.title, store.DEFAULT_TITLE, conversation.messages.map((entry) => entry.id));
      const untitled = state === 'untitled' || state === 'failed'
        || (conversation.messages.length === 0 && !store.repeatsLatestRootPrompt(conversation, content));
      // No prompt goes on a turn still under way: this hold shows none is
      // running, so one left there (its end was not saved) is ended first.
      store.endUnfinishedTurn(conversation, store.UNSAVED);
      const prompt = store.appendMessage(conversation, { role: 'user', content }, conversation.world.reference());
      store.save(conversation);
      if (untitled) title = { prompt: prompt.id, text: titleSource(message.text, files), images: titleImages(message.files), model: { provider, model } };
    });
  } catch (error) {
    releaseTurn(id, turn);
    return sendJson(res, error.statusCode || 500, { error: String(error.message || error) });
  }
  publishConversations();
  turn.attach(res);
  // The title arrives on its own; the turn never waits for it.
  if (title) titleChat(id, title.prompt, title.text, title.images, title.model);
  await executeTurn(id, turn);
}

// Benches. Every round goes to every model unchanged; a model that is still
// working on an earlier round takes the next one as soon as it finishes. A
// model whose turn failed or was stopped waits until it is retried.
const benchComposers = new Map(); // bench id -> BenchComposer

function memberPrompts(member) {
  return member.messages.filter((message) => message.role === 'user' && !message.seeBuild);
}

// Where a bench model's run stands, as stored (store.benchProgress): the
// rounds it was sent, and whether a failed or stopped turn holds its queue.
// A held model is working, so nothing holds its queue.
function memberProgress(id) {
  const { round, halted } = store.benchProgress(id);
  return { round, halted: reservations.has(id) ? null : halted };
}

// A round that cannot start is that model's failed round, stored as a failed
// turn is (executeTurn): its prompt, then the error, which holds the model's
// queue until it is retried.
function startRound(bench, member, index) {
  const [turn] = reservations.take([member.id], () => new TurnStream(), 'This model is already running');
  const round = bench.rounds[index];
  // The bench's own copy of the round, until this model has its files.
  let { content } = round;
  try {
    if (typeof content !== 'string') {
      content = content.map((part) => (
        part.type === 'attachment' ? { ...part, attachment: store.shareAttachment(part.attachment, member.id) } : part
      ));
    }
    sandbox.syncAttachments(member.id, store.ATTACHMENTS_DIR);
    store.appendMessage(member, { role: 'user', content, round: round.id }, member.world.reference());
    store.save(member);
  } catch (error) {
    console.error(`[bench] ${member.model} could not start round ${index + 1}:`, error);
    try {
      // A failed save dropped the chat (store.js): the round goes on what is stored.
      const stored = store.getConversation(member.id);
      store.appendMessage(stored, { role: 'user', content, round: round.id }, stored.world.reference());
      store.endTurn(stored, { role: 'error', content: String(error.message || error) });
      store.markFinished(member.id);
    } finally {
      releaseTurn(member.id, turn);
    }
    return;
  }
  executeTurn(member.id, turn).catch((error) => console.error('[bench]', error));
}

// Starts the next round of every model of the bench that is free for it.
// Once the server is stopping, no round starts. A bench that cannot be loaded
// (one of its chats, at startup) is logged and left as stored.
function advanceBench(benchId) {
  if (reservations.closed) return;
  try {
    startRounds(benchId);
  } catch (error) {
    console.error(`[bench] ${benchId} could not be advanced:`, error);
  }
  publishConversations();
}

function startRounds(benchId) {
  const bench = store.getBench(benchId);
  if (!bench) return;
  for (const member of store.benchMembers(benchId)) {
    if (reservations.has(member.id)) continue;
    try {
      // No round goes on a turn still under way: nothing holds this model,
      // so one left there (its end was not saved) is ended, and holds its queue.
      store.endUnfinishedTurn(member, store.UNSAVED);
      const { round, halted } = store.benchProgress(member.id);
      if (round >= bench.rounds.length || halted) continue;
      startRound(bench, member, round);
    } catch (error) {
      // Nothing could be stored for this model: it is tried again next time.
      console.error(`[bench] ${member.model} could not be advanced:`, error);
    }
  }
}

// Where a model stands on round `index` (from 0) of its bench's line.
function memberRoundState(id, index) {
  const { round, halted } = memberProgress(id);
  if (round > index + 1) return 'done';
  if (round === index + 1) return reservations.has(id) ? 'running' : halted ? 'failed' : 'done';
  return halted ? 'held' : 'queued';
}

// Per round of one model's run: wall time from its prompt to its latest
// message, and the tokens its requests reported (null when none did).
function roundStats(member) {
  const rounds = [];
  for (const message of member.messages) {
    if (message.role === 'user' && !message.seeBuild) {
      rounds.push({ startedAt: Date.parse(message.createdAt), ms: null, inputTokens: null, outputTokens: null });
      continue;
    }
    const round = rounds.at(-1);
    if (!round) continue;
    round.ms = Date.parse(message.createdAt) - round.startedAt;
    const usage = message.telemetry?.usage;
    if (usage?.inputTokens != null) round.inputTokens = (round.inputTokens ?? 0) + usage.inputTokens;
    if (usage?.outputTokens != null) round.outputTokens = (round.outputTokens ?? 0) + usage.outputTokens;
  }
  return rounds.map(({ startedAt, ...round }) => round);
}

const benchViewId = (benchId, round) => `bench:${benchId}:${round}`;

function benchView(benchId, round) {
  const bench = store.getBench(benchId);
  if (!bench) throw httpError(404, 'Bench not found');
  // Round 0 is before the first: every build empty, as after editing the first round.
  if (!Number.isInteger(round) || round < 0 || round > bench.rounds.length) throw httpError(400, 'Unknown round');
  let composer = benchComposers.get(benchId);
  if (!composer) benchComposers.set(benchId, composer = new BenchComposer());
  const { world, layout } = composer.compose(store.benchMembers(benchId)
    .sort((a, b) => bench.order.indexOf(a.id) - bench.order.indexOf(b.id))
    .map((member) => ({ id: member.id, world: worldAtRound(member, round) })));
  return { id: benchViewId(benchId, round), world, layout };
}

// The builds viewers are shown (live-views.js): a conversation, or a round
// of a bench with every model's build side by side.
function viewOf(key) {
  if (key.startsWith('bench:')) {
    const [, benchId, round] = key.split(':');
    const { world, layout } = benchView(benchId, Number(round));
    return { world, layout };
  }
  const conversation = store.getConversation(key);
  if (!conversation) throw httpError(404, 'Conversation not found');
  return { world: conversation.world, layout: conversation.layout || null };
}

const liveViews = new LiveViews({ resolve: viewOf, yOffset: minecraftLive.yOffset });

// Builds shared to the GitHub Pages repository named in Settings (share.js).
const shares = new Shares({ viewer: shareViewer });
const SHARE_REPO_NEEDED = 'Set the GitHub repository for shared builds in Settings';
const SHARE_ID = /^[0-9a-f]{12}$/;

// A publish's states, streamed to `res` as NDJSON, one line per move.
const publishStream = (res) => (event) => {
  if (!res.headersSent) res.writeHead(200, { 'content-type': 'application/x-ndjson', 'cache-control': 'no-cache', 'x-accel-buffering': 'no' });
  if (!res.destroyed) res.write(`${JSON.stringify(event)}\n`);
};

async function handleApi(req, res, url) {
  const parts = url.pathname.split('/').filter(Boolean); // ['api', ...]

  // The live world is the page's (web/live-world.mjs); the server tells it
  // what to show.
  //
  // What a viewer needs: the viewer's assets, the engine module, and the
  // view's latest version (live-views.js). An unknown view is a 404.
  if (req.method === 'GET' && url.pathname === '/api/minecraft/status') {
    const key = url.searchParams.get('view');
    const core = coreWasm.state();
    return sendJson(res, 200, {
      ...minecraftLive.status(),
      core: core.url ?? null,
      coreError: core.error ?? null,
      view: key ? liveViews.describe(key) : null,
    });
  }

  // What takes the world the page holds (version `since`, if any) to the
  // view's latest version.
  if (req.method === 'GET' && url.pathname === '/api/minecraft/import') {
    const key = url.searchParams.get('view');
    const since = url.searchParams.get('since');
    if (!key) return sendJson(res, 400, { error: 'A view is needed' });
    return sendJson(res, 200, liveViews.imports(key, since === null ? null : Number(since)));
  }


  if (req.method === 'GET' && url.pathname === '/api/config') {
    return sendJson(res, 200, {
      providers: settings.publicSettings().providers,
      modelFavorites: settings.get().modelFavorites,
      mcVersion: MC_VERSION,
    });
  }

  if (req.method === 'PUT' && url.pathname === '/api/model-favorites') {
    const body = await readJsonBody(req);
    return sendJson(res, 200, { favorites: settings.updateModelFavorites(body.favorites) });
  }

  if (req.method === 'GET' && url.pathname === '/api/models') {
    const provider = String(url.searchParams.get('provider') || '').trim();
    if (!Object.hasOwn(settings.PROVIDERS, provider)) return sendJson(res, 400, { error: 'Unknown provider' });
    return sendJson(res, 200, { provider, models: await listModels(provider) });
  }

  // Live capabilities for an exact provider/model pair.
  if (req.method === 'GET' && url.pathname === '/api/model-info') {
    const provider = String(url.searchParams.get('provider') || '').trim();
    const model = String(url.searchParams.get('model') || '').trim();
    if (!Object.hasOwn(settings.PROVIDERS, provider)) return sendJson(res, 400, { error: 'provider query parameter is required' });
    if (!model) return sendJson(res, 400, { error: 'model query parameter is required' });
    return sendJson(res, 200, await modelInfo(provider, model));
  }

  if (req.method === 'POST' && url.pathname === '/api/prompts/generate') {
    const body = await readJsonBody(req);
    const provider = String(body.provider || '').trim();
    if (!Object.hasOwn(settings.PROVIDERS, provider)) return sendJson(res, 400, { error: 'Choose a configured provider' });
    if (!settings.publicSettings().providers[provider].connected) {
      return sendJson(res, 400, { error: `${settings.PROVIDERS[provider].label} is not configured` });
    }
    return sendJson(res, 200, { text: await generatePrompt({ provider, model: body.model, effort: body.effort, text: body.text }) });
  }

  // The prompt library, shared by Bench and the builders' prompt_library tool.
  if (url.pathname === '/api/prompts') {
    if (req.method === 'GET') return sendJson(res, 200, { prompts: promptLibrary.list() });
    if (req.method === 'PUT') {
      const body = await readJsonBody(req, BODY_LIMITS.message);
      if (!Array.isArray(body.prompts)) return sendJson(res, 400, { error: 'prompts must be an array' });
      return sendJson(res, 200, { prompts: promptLibrary.save(body.prompts) });
    }
  }

  if (url.pathname === '/api/settings') {
    if (req.method === 'GET') {
      const details = settings.details({ refresh: url.searchParams.get('refresh') === '1' });
      // Where each provider's requests go and in what wire format.
      const PROTOCOLS = { openai: 'Chat Completions', responses: 'Responses', anthropic: 'Messages', cursor: 'Cursor SDK agent', 'claude-code': 'Claude Code CLI over MCP', 'codex-cli': 'Codex CLI over MCP' };
      for (const [id, entry] of Object.entries(details.providers)) {
        entry.endpoint = LLM_PROVIDERS[id].baseUrl || null;
        entry.protocol = PROTOCOLS[LLM_PROVIDERS[id].protocol];
      }
      return sendJson(res, 200, details);
    }
    if (req.method === 'PUT') {
      const body = await readJsonBody(req);
      settings.update(body);
      return sendJson(res, 200, { ok: true, ...settings.publicSettings() });
    }
  }

  if (url.pathname.startsWith('/api/assets/')) {
    const name = parts[2];
    if (!Object.hasOwn(ASSETS, name)) return sendJson(res, 404, { error: 'Unknown asset' });
    const buf = await getAsset(name);
    res.writeHead(200, {
      'content-type': name.endsWith('.png') ? 'image/png' : 'application/json',
      'cache-control': 'private, max-age=86400',
    });
    return res.end(buf);
  }

  if (req.method === 'GET' && parts[1] === 'blocks' && parts[3] === 'icon.png' && parts.length === 4) {
    const png = await blockIcon(parts[2]);
    if (!png) return sendJson(res, 404, { error: 'Unknown block' });
    res.writeHead(200, { 'content-type': 'image/png', 'cache-control': 'private, max-age=31536000, immutable' });
    return res.end(png);
  }

  // A chat's images and attachments are the user's: every load is checked
  // with the server ('no-cache'; the ETag makes an unchanged file a 304), so
  // a page on another site that frames or embeds one never gets it from the
  // browser's cache past the access check, and no shared cache keeps them.
  if (req.method === 'GET' && url.pathname.startsWith('/api/images/') && parts.length === 4) {
    const file = store.imageFile(url.pathname);
    if (!fs.existsSync(file)) return sendJson(res, 404, { error: 'Image not found' });
    return sendFile(req, res, file, {
      'content-type': file.endsWith('.jpg') ? 'image/jpeg' : `image/${file.split('.').pop()}`,
      'cache-control': 'private, no-cache',
    });
  }

  if (req.method === 'GET' && url.pathname.startsWith('/api/attachments/') && parts.length === 4) {
    const file = store.attachmentFile(url.pathname);
    if (!fs.existsSync(file)) return sendJson(res, 404, { error: 'Attachment not found' });
    const extension = path.extname(file).toLowerCase();
    const shown = Object.hasOwn(ATTACHMENT_TYPES, extension);
    // User files are shown, never run: an uploaded SVG gets no script, and
    // no page may frame one. Set on the response, so a 304 carries it too.
    res.setHeader('content-security-policy', "sandbox; frame-ancestors 'none'");
    return sendFile(req, res, file, {
      'content-type': shown ? ATTACHMENT_TYPES[extension] : 'application/octet-stream',
      'content-disposition': `${shown ? 'inline' : 'attachment'}; filename="${path.basename(file).replace(/"/g, '')}"`,
      'cache-control': 'private, no-cache',
    });
  }

  if (req.method === 'GET' && url.pathname === '/api/conversations/stream') {
    res.writeHead(200, { 'content-type': 'application/x-ndjson', 'cache-control': 'no-cache', 'x-accel-buffering': 'no' });
    res.write(`${JSON.stringify(listPayload())}\n`);
    listWatchers.add(res);
    res.once('close', () => listWatchers.delete(res));
    return;
  }

  if (req.method === 'POST' && url.pathname === '/api/conversations') {
    const body = await readJsonBody(req);
    const provider = String(body.provider || '').trim();
    const model = String(body.model || '').trim();
    if (!Object.hasOwn(settings.PROVIDERS, provider) || !model) {
      return sendJson(res, 400, { error: 'Provider and model are required' });
    }
    const conversation = store.createConversation(provider, model);
    return sendJson(res, 200, {
      id: conversation.id,
      provider: conversation.provider,
      model: conversation.model,
    });
  }

  if (req.method === 'GET' && url.pathname === '/api/search') {
    const scope = url.searchParams.get('scope');
    if (scope !== 'build' && scope !== 'bench') return sendJson(res, 400, { error: 'scope must be build or bench' });
    return sendJson(res, 200, { results: store.search(scope, String(url.searchParams.get('q') || '')) });
  }

  if (req.method === 'GET' && url.pathname === '/api/leaderboard') {
    return sendJson(res, 200, { entries: store.leaderboard(), votes: store.voteCount() });
  }
  // Every vote, blind or not; benches and their builds stay.
  if (req.method === 'DELETE' && url.pathname === '/api/votes') {
    store.clearAllVotes();
    return sendJson(res, 200, { ok: true });
  }

  // Checked whole, then written whole (store.createBench).
  if (req.method === 'POST' && url.pathname === '/api/benches') {
    const body = await readJsonBody(req, BODY_LIMITS.message);
    const members = Array.isArray(body.members) ? body.members.map((entry) => ({
      provider: String(entry?.provider || '').trim(),
      model: String(entry?.model || '').trim(),
      effort: String(entry?.effort || '').trim() || null,
    })) : [];
    if (members.length < 2 || members.length > 8) return sendJson(res, 400, { error: 'A bench runs 2 to 8 models' });
    if (members.some((entry) => !Object.hasOwn(settings.PROVIDERS, entry.provider) || !entry.model)) return sendJson(res, 400, { error: 'Every model needs a configured provider' });
    if (body.selections?.length) return sendJson(res, 400, { error: 'Selections belong to one build, not a bench' });
    const resolution = readResolution(body);
    const id = crypto.randomUUID();
    storeMessage(readMessage(body), id, ({ content }) => store.createBench(id, { title: BENCH_TITLE, blind: body.blind === true, resolution, content, members }));
    advanceBench(id);
    titleBench(id);
    return sendJson(res, 200, { id });
  }

  // What is archived, for its 7 days: each can be restored as it was, or
  // deleted for good now (store.js). A bench's model is neither on its own.
  if (req.method === 'GET' && url.pathname === '/api/archive') {
    return sendJson(res, 200, store.listArchive());
  }
  if (req.method === 'POST' && parts[1] === 'conversations' && parts[3] === 'restore' && parts.length === 4) {
    store.restoreConversation(parts[2]);
    publishConversations();
    return sendJson(res, 200, { ok: true });
  }
  // A bench carries on with the rounds it had queued, and publishes the list.
  if (req.method === 'POST' && parts[1] === 'benches' && parts[3] === 'restore' && parts.length === 4) {
    store.restoreBench(parts[2]);
    advanceBench(parts[2]);
    return sendJson(res, 200, { ok: true });
  }
  if (req.method === 'DELETE' && parts[1] === 'conversations' && parts.length === 3) {
    store.deleteConversation(parts[2]);
    publishConversations();
    return sendJson(res, 200, { ok: true });
  }
  if (req.method === 'DELETE' && parts[1] === 'benches' && parts.length === 3) {
    store.deleteBench(parts[2]);
    publishConversations();
    return sendJson(res, 200, { ok: true });
  }

  // Every share on the shared site, newest first, with the chat or bench it
  // came from (null once that is deleted for good, and then no `archived`),
  // and the removal of any of them.
  if (req.method === 'GET' && url.pathname === '/api/shares') {
    const repo = settings.get().shareRepo;
    if (!repo) return sendJson(res, 200, { repo: null, builds: [] });
    const sources = {
      chat: new Map(store.listConversationIds().map((chat) => [shareId(chat.id), chat])),
      bench: new Map(store.listBenchIds().map((bench) => [shareId(bench.id), bench])),
    };
    const builds = (await shares.list(repo)).map((build) => {
      const source = sources[build.kind].get(build.shareId);
      const ids = {
        conversationId: build.kind === 'chat' ? source?.id ?? null : null,
        benchId: build.kind === 'bench' ? source?.id ?? null : null,
      };
      return source ? { ...build, ...ids, archived: source.archived } : { ...build, ...ids };
    });
    return sendJson(res, 200, { repo, builds });
  }
  if (req.method === 'DELETE' && parts[1] === 'shares' && parts.length === 3) {
    if (!SHARE_ID.test(parts[2])) return sendJson(res, 404, { error: 'This build is not shared' });
    const repo = settings.get().shareRepo;
    if (!repo) return sendJson(res, 400, { error: SHARE_REPO_NEEDED });
    if (!(await shares.remove(repo, parts[2]))) return sendJson(res, 404, { error: 'This build is not shared' });
    return sendJson(res, 200, { ok: true });
  }

  // A bench's request body is read before the bench is, so nothing below
  // acts on a bench as it was before an await.
  if (parts[0] === 'api' && parts[1] === 'benches' && parts[2]) {
    const body = req.method === 'POST' ? await readJsonBody(req, parts[3] === 'rounds' ? BODY_LIMITS.message : undefined) : null;
    const bench = store.getBench(parts[2]);
    if (!bench) return sendJson(res, 404, { error: 'Bench not found' });
    if (req.method === 'GET' && parts.length === 3) {
      const stats = Object.fromEntries(store.benchMembers(bench.id).map((member) => [member.id, roundStats(member)]));
      const { nodes, ...shown } = bench;
      return sendJson(res, 200, { ...shown, stats });
    }
    // The best build of one round, or a tie, among the models that finished
    // it. `pick` null takes the vote back.
    if (req.method === 'POST' && parts[3] === 'vote') {
      // A vote is a blind judgement: once the names show, the votes cast stay as they are.
      if (!bench.blind || bench.revealed) return sendJson(res, 409, { error: 'Votes are cast while the models\' names are hidden' });
      const { roundId, pick } = body;
      const index = bench.rounds.findIndex((round) => round.id === roundId);
      if (index < 0) return sendJson(res, 400, { error: 'Unknown round' });
      const members = store.benchMembers(bench.id);
      const states = members.map((member) => memberRoundState(member.id, index));
      if (states.some((state) => state === 'running' || state === 'queued')) return sendJson(res, 409, { error: 'Vote once every model has finished this round' });
      const judged = members.filter((_, i) => states[i] === 'done').map((member) => member.id);
      if (judged.length < 2) return sendJson(res, 409, { error: 'A vote needs at least two finished builds' });
      if (pick === null) store.clearVote(bench, roundId);
      else if (pick === 'tie') store.castVote(bench, roundId, null, judged);
      else if (judged.includes(pick)) store.castVote(bench, roundId, pick, judged);
      else return sendJson(res, 400, { error: 'Pick a model that finished this round' });
      return sendJson(res, 200, { votes: bench.votes });
    }
    // Shows every model's name for good; the bench takes no more votes.
    if (req.method === 'POST' && parts[3] === 'reveal') {
      store.revealBench(bench);
      publishConversations();
      return sendJson(res, 200, { ok: true });
    }
    // Editing a round goes back to before it: every model's chat returns to
    // its world then, and the next round sent branches off there. The rounds
    // after it stay stored, one branch switch away. The bench and its models
    // stay held until their sandboxes are reset.
    if (req.method === 'POST' && parts[3] === 'revert') {
      const index = bench.rounds.findIndex((round) => round.id === body.roundId);
      if (index < 0) return sendJson(res, 400, { error: 'Unknown round' });
      const { content } = bench.rounds[index];
      const members = store.benchMembers(bench.id);
      await editing([benchKey(bench.id), ...members.map((member) => member.id)], 'Stop the running models before editing a round', async () => {
        const reverted = store.revertBenchRound(bench, index);
        await Promise.all(reverted.map((member) => sandbox.reset(member.id)));
      });
      publishConversations();
      const parts = typeof content === 'string' ? [{ type: 'text', text: content }] : content;
      return sendJson(res, 200, {
        rounds: bench.rounds.length,
        text: parts.filter((part) => part.type === 'text').map((part) => part.text).join('\n'),
        attachments: parts.filter((part) => part.type === 'attachment').map((part) => part.attachment),
      });
    }
    // Another version of a round: the bench follows its latest line, and each
    // model's chat the branch that answered it.
    if (req.method === 'POST' && parts[3] === 'branch') {
      const { roundId } = body;
      if (!bench.nodes.some((node) => node.id === roundId)) return sendJson(res, 400, { error: 'Unknown round' });
      const members = store.benchMembers(bench.id);
      const round = await editing([benchKey(bench.id), ...members.map((member) => member.id)], 'Stop the running models before switching branches', async () => {
        store.followBenchBranch(bench, roundId);
        await Promise.all(members.map((member) => sandbox.reset(member.id)));
        return bench.rounds.findIndex((entry) => entry.id === roundId) + 1;
      });
      liveViews.frame(benchViewId(bench.id, round));
      advanceBench(bench.id);
      return sendJson(res, 200, { round });
    }
    if (req.method === 'POST' && parts[3] === 'rounds') {
      if (body.selections?.length) return sendJson(res, 400, { error: 'Selections belong to one build, not a bench' });
      if (reservations.has(benchKey(bench.id))) return sendJson(res, 409, { error: 'Wait for the round edit to finish' });
      storeMessage(readMessage(body), bench.id, ({ content }) => store.appendRound(bench, content));
      advanceBench(bench.id);
      titleBench(bench.id);
      return sendJson(res, 200, { rounds: bench.rounds.length });
    }
    // A model whose turn failed or was stopped runs that round again, once
    // its sandbox is reset; it stays held until then.
    if (req.method === 'POST' && parts[3] === 'retry') {
      const member = store.benchMembers(bench.id).find((entry) => entry.id === body.memberId);
      if (!member) return sendJson(res, 404, { error: 'This model is not in the bench' });
      if (!memberProgress(member.id).halted) return sendJson(res, 409, { error: 'Only a failed or stopped model can be retried' });
      await editing([member.id], 'Only a failed or stopped model can be retried', async () => {
        store.retryBenchRound(bench, member);
        await sandbox.reset(member.id);
      });
      advanceBench(bench.id);
      titleBench(bench.id);
      return sendJson(res, 200, { ok: true });
    }
    // The bench's builds on the shared site, as a chat's are (below): its
    // page's address, a publish streamed as NDJSON, or its removal.
    if (parts[3] === 'share' && parts.length === 4) {
      const repo = settings.get().shareRepo;
      if (req.method === 'GET') return sendJson(res, 200, { url: repo ? await shares.url(repo, shareId(bench.id)) : null });
      if (req.method === 'POST') {
        if (!repo) return sendJson(res, 400, { error: SHARE_REPO_NEEDED });
        const members = store.benchMembers(bench.id);
        if (members.every((member) => member.world.size() === 0)) return sendJson(res, 400, { error: 'No model in this bench has built anything yet' });
        if ([benchKey(bench.id), ...members.map((member) => member.id)].some((key) => reservations.has(key))) {
          return sendJson(res, 409, { error: 'Wait for the bench\'s running turns before sharing' });
        }
        await shares.publish({ repo, sourceId: bench.id, build: () => shares.benchShare(bench, members), onState: publishStream(res) });
        return res.end();
      }
      if (req.method === 'DELETE') {
        if (!repo || !(await shares.remove(repo, shareId(bench.id)))) return sendJson(res, 404, { error: 'This build is not shared' });
        return sendJson(res, 200, {});
      }
    }
    if (req.method === 'POST' && parts[3] === 'archive') {
      const members = store.benchMembers(bench.id);
      if ([benchKey(bench.id), ...members.map((member) => member.id)].some((key) => reservations.has(key))) {
        return sendJson(res, 409, { error: 'Stop the running models before archiving' });
      }
      // Only the containers go; everything on disk stays until the purge.
      await Promise.all(store.archiveBench(bench.id).map((id) => sandbox.reset(id)));
      benchComposers.delete(bench.id);
      publishConversations();
      return sendJson(res, 200, { ok: true });
    }
  }

  if (parts[0] === 'api' && parts[1] === 'conversations' && parts[2]) {
    const id = parts[2];
    if (req.method === 'GET' && parts.length === 3) {
      const conversation = store.getConversation(id);
      if (!conversation) return sendJson(res, 404, { error: 'Conversation not found' });
      return sendJson(res, 200, {
        id: conversation.id,
        title: conversation.title,
        provider: conversation.provider,
        model: conversation.model,
        effort: conversation.effort || null,
        messages: store.publicMessages(conversation),
        messageTree: store.messageTree(conversation),
        runningSince: reservations.turn(id)?.startedAt ?? null,
      });
    }
    if (req.method === 'GET' && parts[3] === 'schematic' && parts.length === 4) {
      const conversation = store.getConversation(id);
      if (!conversation) return sendJson(res, 404, { error: 'Conversation not found' });
      const name = (conversation.title || 'build').replace(/[^\w .-]+/g, '').trim() || 'build';
      res.writeHead(200, {
        'content-type': 'application/octet-stream',
        'content-disposition': `attachment; filename="${name}.schem"`,
        'cache-control': 'no-store',
      });
      return res.end(writeSchematic(conversation.world));
    }
    // A chat's build on the shared site: its page's address (null when it is
    // not shared there), a publish streamed as NDJSON, one line per move of
    // the publish (share.js), or its removal. A publish goes on if the page
    // that asked for it goes.
    if (parts[3] === 'share' && parts.length === 4) {
      const conversation = store.getConversation(id);
      if (!conversation) return sendJson(res, 404, { error: 'Conversation not found' });
      const repo = settings.get().shareRepo;
      if (req.method === 'GET') return sendJson(res, 200, { url: repo ? await shares.url(repo, shareId(id)) : null });
      if (req.method === 'POST') {
        if (!repo) return sendJson(res, 400, { error: SHARE_REPO_NEEDED });
        if (conversation.world.size() === 0) return sendJson(res, 400, { error: 'The build is empty: there is nothing to share' });
        if (reservations.has(id)) return sendJson(res, 409, { error: 'Wait for the running turn before sharing' });
        await shares.publish({ repo, sourceId: id, build: () => shares.chatShare(conversation), onState: publishStream(res) });
        return res.end();
      }
      if (req.method === 'DELETE') {
        if (!repo || !(await shares.remove(repo, shareId(id)))) return sendJson(res, 404, { error: 'This build is not shared' });
        return sendJson(res, 200, {});
      }
    }
    if (req.method === 'POST' && parts[3] === 'selection') {
      const conversation = store.getConversation(id);
      if (!conversation) return sendJson(res, 404, { error: 'Conversation not found' });
      const { min, max } = await readJsonBody(req);
      return sendJson(res, 200, describeSelection(conversation.world, min, max));
    }
    if (req.method === 'POST' && parts[3] === 'seen') {
      store.markSeen(id);
      publishConversations();
      return sendJson(res, 200, { ok: true });
    }
    // Streams the render snapshot as NDJSON. The client sends the chunk
    // versions it already holds; only chunks whose version differs (plus their
    // face neighbours, whose culling and connections depend on them) and the
    // keys it must drop are sent. Chunk versions are process-unique stamps,
    // so equal (key, version) means equal content across branches.
    if (req.method === 'POST' && parts[3] === 'world') {
      const conversation = store.getConversation(id);
      if (!conversation) return sendJson(res, 404, { error: 'Conversation not found' });
      const body = await readJsonBody(req, BODY_LIMITS.world);
      // see_build's page draws the engine's world (shot-world.js), or the
      // part of it inside a clip box, whose states are Minecraft's own: no
      // connections are worked out here.
      const world = body.engine ? shotWorld(conversation, body.clip, body.tick) : conversation.world;
      if (!world) return sendJson(res, 409, { error: 'No engine world placed for this conversation' });
      const renderLod = world.renderLod();
      const known = JSON.stringify(body.renderLod ?? null) === JSON.stringify(renderLod) ? body.known : {};
      const removed = [];
      const send = new Set();
      for (const keyText of Object.keys(known)) {
        if (!world.chunks.has(Number(keyText))) removed.push(Number(keyText));
      }
      for (const [key, chunk] of world.chunks) {
        if (known[key] !== chunk.version) send.add(key);
      }
      // An authored chunk's connections depend on its neighbours, so they
      // are sent again; the engine's states are final.
      if (!body.engine) for (const key of [...send, ...removed]) {
        const [cx, cy, cz] = World.coordsOf(key);
        for (const neighbor of [
          World.keyOf(cx + 1, cy, cz), World.keyOf(cx - 1, cy, cz), World.keyOf(cx, cy + 1, cz),
          World.keyOf(cx, cy - 1, cz), World.keyOf(cx, cy, cz + 1), World.keyOf(cx, cy, cz - 1),
        ]) if (world.chunks.has(neighbor)) send.add(neighbor);
      }
      res.writeHead(200, {
        'content-type': 'application/x-ndjson',
        'cache-control': 'no-cache',
        'x-accel-buffering': 'no',
      });
      const writeEvent = async (event) => {
        if (res.write(`${JSON.stringify(event)}\n`)) return;
        await new Promise((resolve) => {
          const done = () => {
            res.off('drain', done);
            res.off('close', done);
            resolve();
          };
          res.once('drain', done);
          res.once('close', done);
        });
      };
      await writeEvent({
        type: 'meta',
        revision: world.revision,
        blockCount: world.size(),
        bounds: world.bounds(),
        renderLod,
        removed,
      });
      for (const key of send) {
        await writeEvent({ type: 'chunk', chunk: world.chunkSnapshot(key, !body.engine, true, renderLod) });
        if (res.destroyed) return;
      }
      return res.end();
    }
    if (req.method === 'POST' && parts[3] === 'dismiss-error') {
      const body = await readJsonBody(req);
      const conversation = store.getConversation(id);
      if (!conversation) return sendJson(res, 404, { error: 'Conversation not found' });
      // It saves the chat: under a running turn that would store the turn's
      // unfinished work, and a save that failed would drop the chat from under it.
      if (reservations.has(id)) return sendJson(res, 409, { error: 'Wait for the running turn before dismissing an error' });
      store.dismissError(conversation, body.messageId);
      return sendJson(res, 200, { ok: true });
    }
    if (req.method === 'POST' && parts[3] === 'archive') {
      ownChat(id);
      if (reservations.has(id)) return sendJson(res, 409, { error: 'Stop the running turn before archiving' });
      store.archiveConversation(id);
      await sandbox.reset(id);
      publishConversations();
      return sendJson(res, 200, { ok: true });
    }
    // A bench chat continues in Build as a new chat, from the end of a round.
    if (req.method === 'POST' && parts[3] === 'fork') {
      const { round } = await readJsonBody(req);
      const source = store.getConversation(id);
      if (!source?.benchId) return sendJson(res, 404, { error: 'Bench chat not found' });
      const prompts = memberPrompts(source);
      if (!Number.isInteger(round) || round < 1 || round > prompts.length) return sendJson(res, 400, { error: 'Unknown round' });
      const next = prompts[round];
      if (!next && reservations.has(id)) return sendJson(res, 409, { error: 'Wait for this round to finish before forking it' });
      // A fork is made whole or not at all: its workspace is copied, then its
      // files and rows are stored (store.forkConversation). One that fails
      // leaves no workspace either.
      const forkId = crypto.randomUUID();
      const work = sandbox.workDir(forkId);
      let fork;
      try {
        fs.cpSync(sandbox.workDir(id), work, { recursive: true, force: true, mode: fs.constants.COPYFILE_FICLONE });
        fork = next
          ? store.forkConversation(source, source.messages.indexOf(next), source.checkpoints.get(next.id), forkId)
          : store.forkConversation(source, source.messages.length, source.world, forkId);
      } catch (error) {
        fs.rmSync(work, { recursive: true, force: true });
        throw error;
      }
      // Build shows the fork's model, so it reveals a blind bench: for good,
      // so only once the fork is made.
      const bench = store.getBench(source.benchId);
      if (bench.blind && !bench.revealed) store.revealBench(bench);
      publishConversations();
      return sendJson(res, 200, conversationList().find((row) => row.id === fork.id));
    }
    if (req.method === 'POST' && parts[3] === 'chat') {
      return handleChat(req, res, id);
    }
    // Re-attach to a running turn's event stream (e.g. after a page reload).
    if (req.method === 'GET' && parts[3] === 'stream') {
      const turn = reservations.turn(id);
      if (!turn) return sendJson(res, 404, { error: 'No turn is running' });
      return turn.attach(res);
    }
    if (req.method === 'POST' && parts[3] === 'stop') {
      reservations.turn(id)?.controller.abort();
      return sendJson(res, 200, { ok: true });
    }
    // Both hold the chat until its sandbox is reset, so no turn starts on a
    // shell that is about to be killed.
    if (req.method === 'POST' && parts[3] === 'branch') {
      const body = await readJsonBody(req);
      const conversation = ownChat(id);
      await editing([id], 'Stop the running turn before switching branches', async () => {
        store.activateBranch(conversation, String(body.messageId || ''));
        await sandbox.reset(id);
      });
      return sendJson(res, 200, {
        messages: store.publicMessages(conversation),
        messageTree: store.messageTree(conversation),
      });
    }
    // Move the active history head to just before a user message and restore
    // its world checkpoint. The old branch remains stored for future history UI.
    if (req.method === 'POST' && parts[3] === 'revert') {
      const body = await readJsonBody(req);
      const conversation = ownChat(id);
      const target = await editing([id], 'Stop the running turn before reverting', async () => {
        const reverted = store.revert(conversation, String(body.messageId || ''));
        await sandbox.reset(id);
        return reverted;
      });
      const content = Array.isArray(target.content) ? target.content : [{ type: 'text', text: target.content }];
      return sendJson(res, 200, {
        messages: store.publicMessages(conversation),
        messageTree: store.messageTree(conversation),
        title: conversation.title,
        text: content.filter((p) => p.type === 'text').map((p) => p.text).join('\n'),
        attachments: content.filter((p) => p.type === 'attachment').map((p) => p.attachment),
        selections: content.filter((p) => p.type === 'selection').map((p) => p.selection),
      });
    }
  }

  return sendJson(res, 404, { error: 'Not found' });
}

// Pinned browser libraries, served from the installed packages.
const modulesDir = path.join(__dirname, '..', 'node_modules');
const VENDOR = {
  '/vendor/preact-htm.mjs': path.join(modulesDir, 'htm', 'preact', 'standalone.module.js'),
  '/vendor/deepslate.js': path.join(modulesDir, 'deepslate', 'dist', 'deepslate.umd.cjs'),
};
const GL_MATRIX = '/vendor/gl-matrix/';

// Pages and scripts are revalidated on every load and sent again only when
// they changed.
function serveStatic(req, res, pathname) {
  let file;
  if (VENDOR[pathname]) file = VENDOR[pathname];
  else if (pathname.startsWith(GL_MATRIX)) {
    const root = path.join(modulesDir, 'gl-matrix', 'esm');
    file = path.normalize(path.join(root, pathname.slice(GL_MATRIX.length)));
    if (!file.startsWith(`${root}${path.sep}`)) file = null;
  } else {
    file = path.normalize(path.join(webDir, pathname === '/' ? 'index.html' : pathname.slice(1)));
    if (!file.startsWith(webDir)) file = null;
  }
  if (!file || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
    res.writeHead(404, { 'content-type': 'text/plain' });
    return res.end('Not found');
  }
  const type = file.endsWith('.cjs') ? 'text/javascript; charset=utf-8' : MIME[path.extname(file)] || 'application/octet-stream';
  return sendFile(req, res, file, { 'content-type': type, 'cache-control': 'no-cache' });
}

const LOCAL_HOSTS = localHosts(PORT, auth.hostName());

async function route(req, res) {
  securityHeaders(res);
  const url = requestUrl(req);
  pagePolicy(res, url);
  if (foreignRequest(req, url, LOCAL_HOSTS)) {
    res.writeHead(403, { 'content-type': 'text/plain' });
    return res.end('Forbidden: requests must come from this app on this machine');
  }
  if (url.searchParams.has('token')) return auth.signIn(req, res, url, PORT);
  if (!auth.authorized(req, PORT)) return auth.refuse(req, res);
  if (blueMap.serve(req, res, url.pathname)) return;
  if (url.pathname.startsWith('/api/')) return handleApi(req, res, url);
  if (await coreWasm.serve(req, res, url.pathname)) return;
  return serveStatic(req, res, url.pathname);
}

// Every failure, thrown or rejected, ends its own request and never the server.
const server = http.createServer((req, res) => {
  route(req, res).catch((err) => {
    // A refusal (4xx) is the client's answer, not a server failure.
    if (!(err?.statusCode < 500)) console.error('[http]', req.method, req.url, err);
    if (!res.headersSent) sendJson(res, err?.statusCode || 500, { error: String(err && err.message || err) });
    else res.end();
  });
});
// A client that sends its headers or its body slowly is cut off, so it cannot
// hold a connection open forever. Both limits are on receiving the request:
// a turn's response streams for as long as the turn runs. The largest body,
// a message with 200 MiB of attachments (BODY_LIMITS.message), arrives over
// loopback in seconds.
server.headersTimeout = 10_000;
server.requestTimeout = 120_000;

// The server process: 'starting' until it listens, then 'listening';
// 'closing' from the first shutdown and 'stopped' once that is done, when
// `stopped` (the one shutdown, from its start) resolves. No other step exists.
const lifecycle = {
  state: 'starting',
  stopped: null,
  to(state, from) {
    if (!from.includes(this.state)) throw new Error(`The server is ${this.state}: it cannot become ${state}`);
    this.state = state;
  },
};

async function startHttpServer() {
  // Turns the last run of the server stopped in the middle of end as errors.
  for (const id of store.interruptUnfinished()) console.log(`[startup] ended the turn the server stopped in ${id}`);
  // The viewer's assets, the engines and the page's engine module get ready
  // before the first build opens. A failure is printed once, however many of
  // them it stops (a missing rustup stops three), and one last line says
  // whether all of them are ready.
  const told = new Set();
  const tell = (tag) => (error) => {
    const text = String(error.stderr || error.message || error);
    if (!told.has(text)) console.error(tag, text);
    told.add(text);
    throw error;
  };
  // The engines are built from Minecraft's tables, made from the server jar
  // first (the page's engine module waits for them itself).
  const tables = minecraftLive.minecraftTables();
  Promise.allSettled([
    minecraftLive.warm(),
    coreWasm.prepare().catch(tell('[core-wasm]')),
    tables.then(() => minecraftVerifier.warm()).catch(tell('[verify]')),
    // Every build is settled on these engines (shot-world.js).
    tables.then(() => warmShotEngines()).catch(tell('[settle]')),
  ]).then((results) => console.log(results.every((result) => result.status === 'fulfilled')
    ? '[startup] ready: the live world, the 3D viewer and the engines are prepared'
    : '[startup] not everything could be prepared: see the errors above'));
  server.listen(PORT, '127.0.0.1', () => {
    // A signal came before the server listened: that shutdown, under way or
    // already done, had nothing to close.
    if (lifecycle.state !== 'starting') return server.close();
    lifecycle.to('listening', ['starting']);
    console.log(`mcbench-lite chat builder on ${auth.link(PORT)}`);
    // see_build's page is this server's own: it is opened once the server listens.
    warmRenderer().catch((error) => console.error('[see_build]', error.message));
    getBlockRegistry()
      .then((registry) => console.log(`[assets] block registry ready (${Object.keys(registry).length} blocks, MC ${MC_VERSION})`))
      .catch((err) => console.error('[assets] warm-up failed:', err.message));
    // Benches carry on with the rounds they had queued.
    for (const bench of store.listBenches()) advanceBench(bench.id);
  });
}

// Once closed, no turn, edit or bench round starts (a 503); every one under
// way is stopped and waited for, then the harnesses and engines stop. There
// is one shutdown: every call, during it or after, gets the same one.
function shutdown() {
  lifecycle.stopped ??= stop();
  return lifecycle.stopped;
}

async function stop() {
  lifecycle.to('closing', ['starting', 'listening']);
  reservations.close();
  if (server.listening) server.close();
  for (const res of listWatchers) res.end();
  listWatchers.clear();
  for (const turn of reservations.turns()) turn.controller.abort();
  titles.controller.abort();
  await Promise.all([reservations.idle(), ...titles.tasks]);
  await Promise.allSettled([
    sandbox.shutdown(),
    harness.shutdownHarnesses(),
    minecraftLive.stop(),
    minecraftVerifier.stop(),
    stopShotEngines(),
    closeRenderer(),
  ]);
  store.close();
  releaseDataFolder();
  lifecycle.to('stopped', ['closing']);
}

// Every signal, the first and any after it, waits for the one shutdown; the
// process exits when that is done, never before.
function exitOnSignals(proc) {
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    proc.on(signal, () => shutdown().finally(() => proc.exit(0)));
  }
}

// A failure outside any request leaves the process in a state nothing
// checked, so it stops serving: the one shutdown saves the turns under way
// and ends containers, browsers and CLIs, then the process exits 1. A
// shutdown that hangs is given up on at the deadline.
const FATAL_SHUTDOWN_MS = 10_000;
function exitOnFatal(proc) {
  const fatal = (error) => {
    console.error('[fatal]', error);
    const deadline = new Promise((resolve) => setTimeout(resolve, FATAL_SHUTDOWN_MS));
    Promise.race([shutdown(), deadline])
      .catch((failure) => console.error('[fatal] shutdown failed:', failure))
      .finally(() => proc.exit(1));
  };
  proc.on('uncaughtException', fatal);
  proc.on('unhandledRejection', fatal);
}

// Run as the app; tests require it and drive handleApi themselves.
if (require.main === module) {
  exitOnSignals(process);
  exitOnFatal(process);
  startHttpServer().catch((error) => {
    console.error('[startup]', error);
    process.exitCode = 1;
  });
}

module.exports = { handleApi, route, server, advanceBench, startHttpServer, shutdown, exitOnSignals, exitOnFatal, FATAL_SHUTDOWN_MS, reservations, shares };
