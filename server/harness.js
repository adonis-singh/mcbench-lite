'use strict';

// Shared machinery for vendor harnesses (Claude Code, Codex CLI, Cursor). Each
// runs its own agent loop with mcbench's tools as its only tools; every tool
// call it makes is relayed to runTurn as an ordinary completion tool call, and
// the next completion request delivers runTurn's stored result back to the
// waiting call. One turn loop, one transcript, one tool executor.

const crypto = require('node:crypto');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { spawn } = require('node:child_process');
const readline = require('node:readline');
const { splitResolution, resolutionUpdate } = require('./prompt');

const DATA_DIR = path.resolve(process.env.MCBENCH_DATA_DIR || path.join(__dirname, '..', 'data'));
const HARNESS_DIR = path.join(DATA_DIR, 'harness');

const conversationDir = conversationId => path.join(HARNESS_DIR, conversationId);
const expandHome = value => value.replace(/^~(?=$|\/)/, os.homedir());

function dataImage(url) {
  const match = /^data:([^;,]+);base64,(.+)$/.exec(url);
  if (!match) throw new Error('Harness images must be base64 data URLs');
  return { mimeType: match[1], data: match[2] };
}

function parts(content) {
  return typeof content === 'string' ? [{ type: 'text', text: content }] : content;
}

// Text rendering of message content; images become numbered placeholders and
// their bytes are collected for the harness's own image input.
function promptRenderer() {
  const images = [];
  const render = content => parts(content).map(part => {
    if (part.type === 'text') return part.text;
    if (part.type === 'image_url') {
      images.push(dataImage(part.image_url.url));
      return `[Image ${images.length}]`;
    }
    throw new Error(`Unsupported harness content type: ${part.type}`);
  }).filter(Boolean).join('\n');
  return { images, render };
}

// The system prompt, the earlier transcript and the new request. Trailing user
// messages after a tool result are its screenshots, not a new request.
function splitRequest(messages) {
  const system = messages.filter(m => m.role === 'system').map(m => m.content).join('\n\n');
  const history = messages.filter(m => m.role !== 'system');
  let start = history.length;
  while (start > 0 && history[start - 1].role === 'user') start--;
  const request = start < history.length && history[start - 1]?.role !== 'tool' ? history.at(-1) : null;
  return { system, request, prior: request ? history.slice(0, -1) : history };
}

function renderTranscript(prior, render) {
  const toolNames = new Map();
  return prior.map(message => {
    if (message.role === 'user') return `User:\n${render(message.content)}`;
    if (message.role === 'tool') return `Result of ${toolNames.get(message.tool_call_id)} (${message.tool_call_id}):\n${render(message.content)}`;
    const calls = (message.tool_calls || []).map(call => {
      toolNames.set(call.id, call.function.name);
      return `Called ${call.function.name} (${call.id}) with ${call.function.arguments || '{}'}`;
    });
    return [`Assistant:${message.content ? `\n${message.content}` : ''}`, ...calls].join('\n');
  });
}

// The prompt a CLI harness receives: a new session gets the earlier
// transcript first (`history`), a resumed one the `notes` it lacks
// (stoppedReplyNote, resolutionNote), then the new request. The mcbench
// system prompt travels separately, as the harness's appended instructions.
function harnessPrompt(messages, { history, notes }) {
  const { prior, request } = splitRequest(messages);
  const { images, render } = promptRenderer();
  const sections = [...notes];
  if (history) {
    const transcript = renderTranscript(prior, render);
    if (transcript.length) sections.push(`<conversation>\n${transcript.join('\n\n')}\n</conversation>`);
  }
  sections.push(request ? render(request.content) : 'Continue the current task from the latest tool results above.');
  return { text: sections.join('\n\n'), images };
}

const systemText = messages => splitRequest(messages).system;

// The instructions a session was given, as its record keeps them: a digest of
// the system prompt without its resolution section (`instructions`) and one
// of that section (`resolution`, of '' at Standard). Claude Code and Codex
// both keep the instructions a session started with when it is resumed, and
// ignore new ones (measured on Claude Code 2.1.322 and Codex 0.167.1). So a
// session continues only under the same instructions, and a resolution that
// changed since the session was last told it goes in the request
// (resolutionNote).
const digest = text => crypto.createHash('sha256').update(text).digest('hex');
function sessionTerms(messages) {
  const { base, resolution } = splitResolution(systemText(messages));
  return { instructions: digest(base), resolution: digest(resolution) };
}

function resolutionNote(record, messages) {
  const { resolution } = splitResolution(systemText(messages));
  return record.resolution === digest(resolution) ? [] : [resolutionUpdate(resolution)];
}

// A reply the user stopped while it streamed, in the session's latest turn:
// Claude Code drops the request it was cut from (measured on 2.1.322), so a
// resumed session is given the text the user saw. The app keeps it on the
// stopped message as `reply`. `conversationMessages` are the stored
// transcript, which the record's `lastSeen` names a request of.
function stoppedReplyNote(record, conversationMessages) {
  const index = conversationMessages.findIndex(m => m.id === record.lastSeen);
  const stopped = conversationMessages.slice(index + 1, -1).find(m => m.stopped && m.reply);
  return stopped ? [`<stopped-reply>\nThe user stopped your previous reply while you were writing it, and this session does not hold it. This is the text you had written, as the user saw it:\n\nAssistant:\n${stopped.reply}\n</stopped-reply>`] : [];
}

// Tool results for the pending call: its tool message plus any screenshots
// runTurn attached right after it, as MCP content blocks.
function pendingResult(messages, callId) {
  const index = messages.findIndex(m => m.role === 'tool' && m.tool_call_id === callId);
  if (index < 0 || messages.slice(index + 1).some(m => m.role !== 'user')) return null;
  return messages.slice(index).flatMap(m => parts(m.content)).map(part => {
    if (part.type === 'text') return { type: 'text', text: part.text };
    if (part.type === 'image_url') return { type: 'image', ...dataImage(part.image_url.url) };
    throw new Error(`Unsupported harness content type: ${part.type}`);
  });
}

// Each harness keeps its own session per conversation (Claude Code session,
// Codex thread) so follow-up turns resume it. The record's `lastSeen` is the
// request of the latest turn the session took.
const sessionFile = (conversationId, harness) => path.join(conversationDir(conversationId), `${harness}.json`);

function readSession(conversationId, harness) {
  const file = sessionFile(conversationId, harness);
  return fs.existsSync(file) ? JSON.parse(fs.readFileSync(file, 'utf8')) : null;
}

// Written whole then renamed into place: a record cut short by a crash would
// fail to parse at the start of every later turn of its chat.
function writeSession(conversationId, harness, record) {
  fs.mkdirSync(conversationDir(conversationId), { recursive: true });
  const file = sessionFile(conversationId, harness);
  const temporary = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temporary, JSON.stringify(record, null, 2));
  fs.renameSync(temporary, file);
}

// A stored session continues the transcript when the request of its latest
// turn is still on the active branch and everything between it and the new
// request belongs to that turn: replies, tool results, screenshots and how
// the turn ended, a stop or an error included (the session holds that turn as
// far as it got). A revert or a branch switch takes that request off the
// active branch, and a user prompt in between is a turn the session never
// took (another provider's); then the harness starts over from the rendered
// transcript.
const userPrompt = m => m.role === 'user' && !m.seeBuild;
function sessionContinues(record, messages) {
  if (!record) return false;
  const index = messages.findIndex(m => m.id === record.lastSeen);
  if (index < 0 || !userPrompt(messages.at(-1))) return false;
  return !messages.slice(index + 1, -1).some(userPrompt);
}

// The environment a vendor CLI starts with: what a process needs to run
// (path, home, locale, proxies, certificates) and nothing else. Keys in the
// app's .env and anything the app itself inherited, such as a parent Claude
// Code's session variables or ANTHROPIC_BASE_URL, stay out.
const CLI_ENV = new Set(['PATH', 'HOME', 'USER', 'LOGNAME', 'SHELL', 'TMPDIR', 'LANG', 'TERM', 'TZ',
  'HTTP_PROXY', 'HTTPS_PROXY', 'NO_PROXY', 'ALL_PROXY', 'http_proxy', 'https_proxy', 'no_proxy', 'all_proxy',
  'SSL_CERT_FILE', 'SSL_CERT_DIR', 'NODE_EXTRA_CA_CERTS']);

function cliEnv(extra) {
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => CLI_ENV.has(key) || key.startsWith('LC_')));
  return { ...env, ...extra };
}

// A turn that starts forgets the stored session until the harness records it
// again with this turn's request: the CLI's own session file takes the new
// request either way, and the old record would let a later revert of that
// request resume a session holding the reverted work.
function forgetSession(conversationId, harness) {
  fs.rmSync(sessionFile(conversationId, harness), { force: true });
}

// Harness processes that outlive their turn (Claude Code's warm processes).
// Each has a `conversationId` and a `kill()` that resolves once it has exited;
// archiving a conversation and shutting the server down end them.
const processes = new Set();
const track = proc => processes.add(proc);
const untrack = proc => processes.delete(proc);

// What a harness keeps of a conversation outside its harness folder (Claude
// Code's session transcript), removed by `cleanup(record)` with the
// conversation's session record, by harness name.
const cleanups = new Map();
const onDestroy = (name, cleanup) => cleanups.set(name, cleanup);

// Ends the conversation's harness processes and removes its harness files.
// What a harness keeps elsewhere is removed once its processes have exited,
// so none of them writes it again; the promise resolves then. The chat is
// already gone by then, so a removal that fails is logged.
function destroy(conversationId) {
  const removals = [...cleanups].flatMap(([name, cleanup]) => {
    const record = readSession(conversationId, name);
    return record ? [() => cleanup(record)] : [];
  });
  const exits = [...processes].filter(proc => proc.conversationId === conversationId).map(proc => proc.kill());
  fs.rmSync(conversationDir(conversationId), { recursive: true, force: true });
  return Promise.all(exits).then(() => { for (const remove of removals) remove(); })
    .catch(error => console.error(`[harness] removing what chat ${conversationId} kept outside its folder failed:`, error));
}

// Kills every harness process; resolves once all have exited.
function shutdownHarnesses() {
  return Promise.all([...processes].map(proc => proc.kill()));
}

// Relays one harness run to runTurn. The harness side calls `call` for each
// tool call and `update`/`finish` as it streams; runTurn's side awaits `next`
// for the following completion and `deliver`s the stored tool result.
//
// States: running (the harness works; its tool calls queue and surface one at
// a time as `pending`), ended (the harness reported its outcome, which the
// next completion collects) and closed (final: the turn is over or was
// stopped). Harness events outside running are dropped, and a session cannot
// be created on an aborted signal.
class RelaySession {
  constructor(name, signal) {
    signal?.throwIfAborted();
    this.name = name;
    this.state = 'running';
    this.calls = [];
    this.pending = null;
    this.waiter = null;
    this.text = '';
    this.reasoning = '';
    this.usage = null;
    // What the vendor agent reports besides text: notices (fallbacks,
    // compaction, warnings), its task list, usage-limit state and how much
    // context its latest request used. Each rides on the next completion.
    this.notices = [];
    this.plan = null;
    this.rateLimit = null;
    this.contextTokens = null;
    // Tool calls whose arguments the model is still writing, by call id: the
    // text streams to the UI as it arrives, and a call that streamed is not
    // sent again whole when it runs.
    this.drafts = new Map();
    this.outcome = null;
    this.onClose = () => {};
    this.abort = () => this.close();
    signal?.addEventListener('abort', this.abort, { once: true });
    this.signal = signal;
  }

  closeReason() {
    return this.signal?.reason || new Error(`${this.name} run closed`);
  }

  throwIfClosed() {
    if (this.state === 'closed') throw this.closeReason();
  }

  // A tool call from the harness. `signal` (the MCP request's) cancels it: a
  // queued call never runs, and a running one's result is dropped.
  call(id, name, args, signal) {
    return new Promise((resolve, reject) => {
      if (this.state !== 'running') return reject(new Error('The mcbench turn ended before this tool ran.'));
      signal?.throwIfAborted();
      const call = { id, name, args, resolve, reject };
      signal?.addEventListener('abort', () => {
        this.calls = this.calls.filter(queued => queued !== call);
        this.drafts.delete(id);
        reject(signal.reason);
      }, { once: true });
      this.calls.push(call);
      this.pump();
    });
  }

  update({ text, reasoning, usage }) {
    if (this.state !== 'running') return;
    if (text) {
      this.text += text;
      this.waiter?.onDelta?.({ content: text });
    }
    if (reasoning) {
      this.reasoning += reasoning;
      this.waiter?.onDelta?.({ reasoning });
    }
    if (usage) this.usage = usage;
  }

  draftToolCall(id, index, name) {
    if (this.state !== 'running') return;
    this.drafts.set(id, { index, name, args: '', sent: null });
    this.flushDrafts();
  }

  draftArgs(id, text) {
    if (this.state !== 'running') return;
    this.drafts.get(id).args += text;
    this.flushDrafts();
  }

  // Argument text written while no completion was waiting (another call was
  // running) goes out once the next one is.
  flushDrafts() {
    const onDelta = this.waiter?.onDelta;
    if (!onDelta) return;
    for (const [id, draft] of this.drafts) {
      if (draft.sent === draft.args.length) continue;
      onDelta({ toolCall: { index: draft.index, id, name: draft.name, args: draft.args.slice(draft.sent ?? 0) } });
      draft.sent = draft.args.length;
    }
  }

  // A transient activity line ("Retrying…", "Compacting context") for the UI.
  status(text) {
    if (this.state !== 'running') return;
    this.waiter?.onDelta?.({ status: text });
  }

  notice(level, text) {
    if (this.state !== 'running' || !text) return;
    const entry = { level, text };
    this.notices.push(entry);
    this.waiter?.onDelta?.({ notice: entry });
  }

  setPlan(items) {
    if (this.state !== 'running') return;
    this.plan = items;
    this.waiter?.onDelta?.({ plan: items });
  }

  setRateLimit(info) {
    if (this.state === 'running') this.rateLimit = info;
  }

  setContextTokens(tokens) {
    if (this.state === 'running') this.contextTokens = tokens;
  }

  // The harness run ended. `save` records its session for the next turn and
  // runs only for a turn that completed normally: still running, no tool call
  // left unanswered. A save that throws fails the turn.
  finish(error, save) {
    if (this.state !== 'running') return;
    let failure = error;
    if (!failure && (this.pending || this.calls.length)) failure = new Error(`${this.name} ended with a tool call unanswered`);
    if (!failure && save) {
      try { save(); } catch (saveError) { failure = saveError; }
    }
    this.state = 'ended';
    this.outcome = { error: failure || null };
    // The harness is gone: its calls get no answer. A pending call stays
    // known so that delivering its result reports how the run ended.
    for (const call of [this.pending, ...this.calls].filter(Boolean)) call.reject(new Error('The mcbench turn ended before this tool ran.'));
    this.calls = [];
    this.pump();
  }

  deliver(content) {
    if (this.state !== 'running') {
      const error = this.state === 'ended' ? this.outcome.error : this.closeReason();
      this.close();
      throw error;
    }
    const call = this.pending;
    this.pending = null;
    call.resolve(content);
  }

  next(onDelta) {
    return new Promise((resolve, reject) => {
      this.waiter = { resolve, reject, onDelta };
      this.pump();
    });
  }

  // Answers the waiting completion, if the state has something for it.
  pump() {
    const waiter = this.waiter;
    if (!waiter) return;
    if (this.state === 'closed') {
      this.waiter = null;
      return waiter.reject(this.closeReason());
    }
    const message = () => {
      const entry = {
        role: 'assistant',
        content: this.text,
        ...(this.reasoning ? { reasoning: this.reasoning } : {}),
        ...(this.notices.length ? { notices: this.notices } : {}),
        ...(this.plan ? { plan: this.plan } : {}),
        ...(this.rateLimit ? { rateLimit: this.rateLimit } : {}),
        ...(this.contextTokens !== null ? { contextTokens: this.contextTokens } : {}),
      };
      this.text = '';
      this.reasoning = '';
      this.notices = [];
      this.plan = null;
      return entry;
    };
    this.flushDrafts();
    if (this.state === 'running') {
      if (!this.calls.length) return;
      const call = this.pending = this.calls.shift();
      const args = JSON.stringify(call.args);
      this.waiter = null;
      if (!this.drafts.delete(call.id)) waiter.onDelta?.({ toolCall: { index: 0, id: call.id, name: call.name, args } });
      return waiter.resolve({ ...message(), finish_reason: 'tool_calls', tool_calls: [{ id: call.id, type: 'function', function: { name: call.name, arguments: args } }] });
    }
    this.waiter = null;
    const { error } = this.outcome;
    const final = { ...message(), finish_reason: 'stop', ...(this.usage ? { usage: this.usage } : {}) };
    this.close();
    if (error) return waiter.reject(error);
    waiter.resolve(final);
  }

  // Stops the harness while it runs; subclasses override.
  stop() {}

  // Releases what the run holds, however it ended; subclasses override.
  release() {}

  close() {
    if (this.state === 'closed') return;
    const running = this.state === 'running';
    this.state = 'closed';
    this.signal?.removeEventListener('abort', this.abort);
    for (const call of [this.pending, ...this.calls].filter(Boolean)) {
      call.reject(new Error('The mcbench turn ended before this tool ran.'));
    }
    this.pending = null;
    this.calls = [];
    this.drafts.clear();
    // A stop that throws still leaves the run released, forgotten and its
    // waiter answered; its error reaches the caller after that.
    try {
      if (running) this.stop();
    } finally {
      this.release();
      this.onClose();
      this.pump();
    }
  }
}

// One run per conversation for a harness whose Session class starts its run
// in `start(options, provider)`. A completion either delivers the stored
// result of the run's pending tool call or ends any previous run and starts a
// new one; a run that fails to start is closed.
function sessionRelay(Session) {
  const sessions = new Map();
  async function completion(provider, options) {
    const key = options.conversationId;
    let session = key && sessions.get(key);
    const result = session?.pending && pendingResult(options.messages, session.pending.id);
    if (result) {
      session.deliver(result);
    } else {
      session?.close();
      session = new Session(options.signal);
      if (key) {
        sessions.set(key, session);
        session.onClose = () => { if (sessions.get(key) === session) sessions.delete(key); };
      }
      try {
        await session.start(options, provider);
      } catch (error) {
        session.close();
        throw error;
      }
    }
    return session.next(options.onDelta);
  }
  // Ends the conversation's run, if any (the turn is over, however it ended).
  const closeSession = conversationId => sessions.get(conversationId)?.close();
  return { completion, closeSession };
}

// Runs a CLI whose stdout is newline-delimited JSON. `done` resolves with the
// exit status and the tail of stderr; `onLine` receives each parsed stdout
// line, and an exception from it stops the process and rejects. `stop` ends
// the process early (SIGTERM, then SIGKILL) and does nothing once it is
// stopping or has exited. With `keepOpen`, stdin stays open after `input` for
// later `write`s.
function runJsonLines(command, args, { cwd, env, input, onLine, onStop = () => {}, keepOpen = false }) {
  const child = spawn(command, args, { cwd, env, stdio: ['pipe', 'pipe', 'pipe'] });
  let state = 'running'; // running, stopping, exited
  let stderr = '';
  let failure;
  let force;
  const stop = () => {
    if (state !== 'running') return;
    state = 'stopping';
    onStop();
    child.kill('SIGTERM');
    force = setTimeout(() => child.kill('SIGKILL'), 10_000);
  };
  const done = new Promise((resolve, reject) => {
    child.stderr.on('data', chunk => { stderr = (stderr + chunk).slice(-4000); });
    readline.createInterface({ input: child.stdout }).on('line', line => {
      if (failure || !line.trim()) return;
      try { onLine(JSON.parse(line)); } catch (error) { failure = error; stop(); }
    });
    child.stdin.on('error', () => {});
    if (keepOpen) child.stdin.write(input);
    else child.stdin.end(input);
    child.once('error', reject);
    child.once('close', (code, killed) => {
      state = 'exited';
      clearTimeout(force);
      if (failure) return reject(failure);
      resolve({ code, signal: killed, stderr });
    });
  });
  return { done, stop, write: (line) => child.stdin.write(line) };
}

module.exports = {
  RelaySession, sessionRelay, runJsonLines, harnessPrompt, systemText, sessionTerms, resolutionNote, stoppedReplyNote, pendingResult, track, untrack, shutdownHarnesses,
  readSession, forgetSession, writeSession, cliEnv, sessionContinues, destroy, onDestroy, conversationDir, expandHome, DATA_DIR,
};
