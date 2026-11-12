'use strict';

// Claude Code in headless print mode as a harness: Claude Code's own loop,
// system prompt and context management, with mcbench's tools as its only tools
// over MCP (`--tools ""` removes every built-in; the init event's tool list is
// checked). It runs on the host under the Claude Code login, with no user,
// project or local settings. Each conversation keeps one Claude Code session
// that follow-up turns resume, a turn that was stopped or failed included.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { execFileSync } = require('node:child_process');
const { serveMcp, BRIDGE } = require('./mcp');
const harness = require('./harness');

const NAME = 'claude-code';
const PREFIX = 'mcp__mcbench__';
// The one working directory of every Claude Code run. Claude Code puts the
// directory, and the git status of any repository around it, in its system
// prompt, so one folder outside any git work tree gives every chat the same
// prompt, whose cache one chat's first request can read from another's. It
// also files sessions by this directory, so a session started in another
// folder cannot be resumed here. Nothing is written in the folder itself. It
// is in the user's home and private to them: a folder under a shared /tmp
// could be made first by another account.
const CWD = path.join(os.homedir(), '.cache', 'mcbench', 'claude-cwd');
// The Claude Code executable and config directory; Settings can point them
// elsewhere (CLAUDE_CONFIG_DIR is the CLI's own setting for the latter).
const runtime = { binary: 'claude', home: null };
function configure({ binary, home }) {
  runtime.binary = binary || 'claude';
  runtime.home = home ? harness.expandHome(home) : null;
}
const configDir = () => runtime.home || process.env.CLAUDE_CONFIG_DIR || os.homedir();

const version = () => execFileSync(runtime.binary, ['--version'], { encoding: 'utf8', timeout: 5000 }).trim();

// `claude auth login` records the account in the CLI's config file.
let login = { file: null, mtimeMs: null, account: null };
function oauthAccount() {
  const file = path.join(configDir(), '.claude.json');
  if (!fs.existsSync(file)) return null;
  const { mtimeMs } = fs.statSync(file);
  if (file !== login.file || mtimeMs !== login.mtimeMs) login = { file, mtimeMs, account: JSON.parse(fs.readFileSync(file, 'utf8')).oauthAccount || null };
  return login.account;
}
const storedLogin = () => Boolean(oauthAccount());

// Who is signed in and on what plan, as the CLI records it: organizationType
// "claude_max" with rate-limit tier "default_claude_max_20x" reads Claude Max 20x.
function account() {
  const entry = oauthAccount();
  if (!entry) return null;
  const tier = /^claude_(\w+)$/.exec(entry.organizationType || '')?.[1];
  const multiplier = /_(\d+x)$/.exec(entry.organizationRateLimitTier || '')?.[1];
  const plan = tier ? ['Claude', tier[0].toUpperCase() + tier.slice(1), multiplier].filter(Boolean).join(' ') : null;
  return { email: entry.emailAddress || null, plan };
}

// `session` is { id, resume } for a persistent chat session; the batch harness
// passes null for a run that is never saved, its prompt as plain text, with
// Claude Code's own system prompt and the mcbench one appended.
// `oneShot` is the app's completion with no conversation (a title, a prompt
// rewrite): streamed and read as a chat turn is, never saved, and its system
// prompt replaces Claude Code's own. Appended to Claude Code's agent prompt,
// it loses: the model takes the user's text as a task and answers or acts on
// it (a whole build, or tool-call markup) instead of doing what was asked.
// `builtins` are Claude Code tools left on: none for benchmark attempts; its
// task tools in chats, so its task list reaches the UI. They only record a list.
function claudeArgs({ model, effort, systemPrompt, allowedTools, socket, session, oneShot = false, builtins = [] }) {
  return ['-p', '--output-format', 'stream-json', '--verbose', '--model', model,
    ...(effort ? ['--effort', effort] : []),
    // Print mode leaves thinking text empty unless asked for a summary.
    '--thinking-display', 'summarized',
    '--tools', builtins.join(','), '--strict-mcp-config',
    '--mcp-config', JSON.stringify({ mcpServers: { mcbench: { type: 'stdio', command: 'python3', args: [BRIDGE, socket] } } }),
    '--allowedTools', [...allowedTools, ...builtins].join(','), '--permission-mode', 'dontAsk',
    '--setting-sources', '', '--disable-slash-commands',
    ...(session || oneShot ? ['--include-partial-messages', '--input-format', 'stream-json'] : []),
    ...(session ? [session.resume ? '--resume' : '--session-id', session.id] : ['--no-session-persistence']),
    oneShot ? '--system-prompt' : '--append-system-prompt', systemPrompt];
}

// The Claude Code login pays for the run, not an API key from the app's .env.
// Auto memory is off: it would give every chat the notes Claude Code kept
// from earlier ones, in a folder outside the app.
// Fine-grained tool streaming makes tool arguments stream as the model writes
// them; without it they arrive in one burst once the call is complete.
// Claude Code records how it was started (CLAUDE_CODE_ENTRYPOINT); the app
// names itself there instead of the SDK label Claude Code gives a -p run.
function claudeEnv() {
  return harness.cliEnv({ ...(runtime.home ? { CLAUDE_CONFIG_DIR: runtime.home } : {}), MCP_TOOL_TIMEOUT: '2147483647', MCP_TIMEOUT: '120000',
    MAX_MCP_OUTPUT_TOKENS: '200000', CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: '1', DISABLE_AUTOUPDATER: '1',
    CLAUDE_CODE_ENABLE_FINE_GRAINED_TOOL_STREAMING: '1', CLAUDE_CODE_ENTRYPOINT: 'mcbench-lite', CLAUDE_CODE_DISABLE_AUTO_MEMORY: '1' });
}

// Claude Code files a session's transcript in its config folder (the one the
// run is given, else ~/.claude), under projects/ and a folder named after the
// working directory, CWD, with every character but a letter or digit made a
// dash.
function transcriptFile(sessionId) {
  const folder = path.join(runtime.home || path.join(os.homedir(), '.claude'), 'projects', CWD.replace(/[^a-zA-Z0-9]/g, '-'));
  return path.join(folder, `${sessionId}.jsonl`);
}
// A deleted chat takes its Claude Code session's transcript with it: that one
// file, named by the session id in the chat's record, and nothing else in the
// folder.
harness.onDestroy(NAME, (record) => fs.rmSync(transcriptFile(record.sessionId), { force: true }));

// The init event must list only mcbench's tools, served by a connected server.
function checkInit(event, allowedTools, builtins = []) {
  const extra = event.tools.filter(t => !allowedTools.includes(t) && !builtins.includes(t));
  if (extra.length) throw new Error(`Claude Code exposed tools outside mcbench: ${extra.join(', ')}`);
  const server = event.mcp_servers.find(s => s.name === 'mcbench');
  if (server?.status !== 'connected') throw new Error(`mcbench MCP server status: ${server?.status}`);
}

const CHAT_BUILTINS = ['TaskCreate', 'TaskUpdate', 'TaskList', 'TaskGet'];
const TASK_STATUS = { pending: 'pending', in_progress: 'active', completed: 'done' };

// The context one API request used, from the usage its assistant event
// carries: its fresh, cache-written and cache-read input.
function contextTokensOf(usage) {
  return usage.input_tokens + usage.cache_creation_input_tokens + usage.cache_read_input_tokens;
}

// Maps Claude Code's stream events onto the relay: activity, notices, its
// task list and usage-limit state.
function relayEvent(session, event) {
  if (event.type === 'system') {
    // Any other status (a new request) clears the line back to the elapsed time.
    if (event.subtype === 'status') session.status(event.status === 'compacting' ? 'Compacting context' : null);
    else if (event.subtype === 'api_retry') session.status(`Retrying${event.error_status ? ` after HTTP ${event.error_status}` : ''} (attempt ${event.attempt} of ${event.max_retries})`);
    else if (event.subtype === 'compact_boundary') session.notice('info', 'Claude Code compacted the conversation to fit its context window');
    else if (['model_fallback', 'model_refusal_fallback', 'informational'].includes(event.subtype)) session.notice(event.level === 'warning' ? 'warning' : 'info', event.content);
    else if (event.subtype === 'notification') session.notice(event.color === 'error' ? 'error' : 'warning', event.text);
  } else if (event.type === 'rate_limit_event') {
    session.setRateLimit(event.rate_limit_info);
  } else if (event.type === 'assistant') {
    // Each request's events overwrite the reading, so a turn ends on its last request's.
    session.setContextTokens(contextTokensOf(event.message.usage));
    // Task updates apply once Claude Code confirms them (the user event below).
    for (const block of event.message?.content || []) {
      if (block.type === 'tool_use' && block.name === 'TaskUpdate') (session.pendingTaskUpdates ||= new Map()).set(block.id, block.input);
    }
  } else if (event.type === 'user' && event.tool_use_result) {
    const tasks = session.tasks ||= new Map();
    const result = event.tool_use_result;
    const callId = event.message?.content?.find?.(block => block.type === 'tool_result')?.tool_use_id;
    if (result.task?.id) tasks.set(result.task.id, { text: result.task.subject, status: 'pending' });
    const update = callId && session.pendingTaskUpdates?.get(callId);
    const applies = update && result.success && tasks.has(update.taskId);
    if (applies) {
      if (update.status === 'deleted') tasks.delete(update.taskId);
      else tasks.set(update.taskId, { text: update.subject || tasks.get(update.taskId).text, status: TASK_STATUS[update.status] || tasks.get(update.taskId).status });
    }
    if (result.task?.id || applies) session.setPlan([...tasks.values()]);
  }
}

// Claude Code reports thinking tokens where the app's telemetry reads reasoning tokens.
function usageOf(result) {
  const { output_tokens_details, ...usage } = result.usage;
  return { ...usage, ...(output_tokens_details?.thinking_tokens !== undefined ? { output_tokens_details: { reasoning_tokens: output_tokens_details.thinking_tokens } } : {}) };
}

// One stream-json user message: the prompt text and any images.
function streamInput({ text, images }) {
  const content = [{ type: 'text', text }, ...images.map(image => ({ type: 'image', source: { type: 'base64', media_type: image.mimeType, data: image.data } }))];
  return JSON.stringify({ type: 'user', message: { role: 'user', content } }) + '\n';
}

// One Claude Code process per conversation outlives its turn: the next turn's
// request is written to the same process (its stream-json input stays open),
// which skips the CLI's startup of about two seconds. A process is replaced
// when its turn failed or was stopped (the next process resumes the session),
// when the turn cannot continue the session (a revert, a branch switch) or
// runs with other settings, and it exits after sitting idle. A one-shot run
// (no conversation) ends after its result.
const IDLE_MS = 10 * 60_000;

// The conversation's session record: Claude Code's session id, the folder it
// is filed under, the instructions it was given (harness.sessionTerms), the
// request of its latest turn and the task list.
function recordSession(conversationId, sessionId, session) {
  harness.writeSession(conversationId, NAME, { sessionId, cwd: CWD, ...session.terms, lastSeen: session.request, tasks: [...(session.tasks || [])] });
}
const processes = new Map(); // conversationId -> ClaudeProcess

// A process's states: spawning (its MCP server starting, a turn attached),
// busy (running a turn), idle (between turns, its idle timer set), dead
// (killed, not yet exited) and exited (gone, its files removed; also one
// whose CLI could not be spawned). Output reaches the attached turn only
// while busy. MOVES holds every legal change of state; `to` makes it.
const MOVES = {
  spawning: ['busy', 'dead', 'exited'],
  busy: ['idle', 'dead', 'exited'],
  idle: ['busy', 'dead', 'exited'],
  dead: ['exited'],
  exited: [],
};

class ClaudeProcess {
  constructor({ conversationId, key }) {
    this.conversationId = conversationId;
    this.key = key;
    this.state = 'spawning';
    this.session = null; // the ClaudeCodeSession whose turn is running
    this.sessionId = null; // Claude Code's session, once a result names it
    this.idle = null;
    this.exited = Promise.withResolvers();
    this.toolUses = new Map(); // tool_use id -> { promise, resolve }, resolved once stdout shows the block
    this.blocks = new Map(); // content block index -> tool_use id, for the message being streamed
    if (conversationId) processes.set(conversationId, this);
    harness.track(this);
  }

  to(state) {
    if (!MOVES[this.state].includes(state)) throw new Error(`A Claude Code process that is ${this.state} cannot become ${state}`);
    this.state = state;
  }

  // Claude Code writes a tool_use block to stdout before it calls the tool
  // over MCP, but the two channels are read independently. A call waits for
  // its block, so the stream events before it (its reasoning) reach the turn
  // first.
  toolUse(id) {
    if (!id) throw new Error('Claude Code called an mcbench tool without its tool_use id');
    if (!this.toolUses.has(id)) this.toolUses.set(id, Promise.withResolvers());
    return this.toolUses.get(id);
  }

  // Starts the process for `session`'s turn. A kill while the MCP server
  // starts ends it here, before the CLI is spawned; so does a spawn that
  // throws, since no process is left to report an exit.
  async spawn(session, { args, cwd, tools, input }) {
    this.session = session;
    let socket;
    try {
      this.runtimeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcb-'));
      socket = path.join(this.runtimeDir, 'mcp.sock');
      // A call is known by its tool_use id, the id its streamed arguments
      // carried. The MCP request's signal cancels it.
      this.server = await serveMcp(socket, { tools, call: async (name, callArgs, { toolUseId, signal }) => {
        await this.toolUse(toolUseId).promise;
        this.toolUses.delete(toolUseId);
        if (this.state !== 'busy') throw new Error('No mcbench turn is running in this Claude Code process');
        return this.session.call(toolUseId, name, callArgs, signal);
      } });
      if (this.state === 'dead') throw new Error('Claude Code was stopped while starting');
      this.to('busy');
      this.run = harness.runJsonLines(runtime.binary, args(socket), {
        cwd, env: claudeEnv(), input, keepOpen: true, onLine: (event) => this.line(event),
      });
    } catch (error) {
      this.exit().catch((failure) => console.error('[claude-code]', failure));
      throw error;
    }
    this.run.done.then(
      (exit) => new Error(`Claude Code exited ${exit.code ?? exit.signal} without a result: ${exit.stderr}`),
      (error) => error,
    ).then((error) => {
      // Only a process that dies mid-turn fails it; a killed one's turn has
      // already ended.
      const turn = this.state === 'busy' ? this.session : null;
      turn?.finish(error);
      return this.exit();
    }).catch((error) => console.error('[claude-code]', error));
  }

  line(event) {
    if (this.state !== 'busy') return;
    const session = this.session;
    if (event.type === 'system' && event.subtype === 'init') {
      checkInit(event, session.allowed, session.builtins);
      // Claude Code has taken this turn's request into its session, so the
      // next turn resumes the session however this one ends. Tested on
      // Claude Code 2.1.322: a session killed in the middle of a tool call
      // resumes with that call answered as interrupted (the tool does not run
      // again), and one killed while text streamed resumes without that
      // request's partial reply.
      if (session.conversation) recordSession(this.conversationId, event.session_id, session);
    }
    if (event.type === 'assistant') {
      for (const block of event.message.content) if (block.type === 'tool_use' && block.name.startsWith(PREFIX)) this.toolUse(block.id).resolve();
    }
    relayEvent(session, event);
    if (event.type === 'stream_event') {
      const { type, index, content_block: block, delta } = event.event;
      if (type === 'message_start') this.blocks.clear();
      if (type === 'content_block_start' && block.type === 'tool_use' && block.name.startsWith(PREFIX)) {
        this.blocks.set(index, block.id);
        session.draftToolCall(block.id, index, block.name.slice(PREFIX.length));
      }
      if (delta?.type === 'text_delta') session.update({ text: delta.text });
      if (delta?.type === 'thinking_delta') session.update({ reasoning: delta.thinking });
      if (delta?.type === 'input_json_delta' && this.blocks.has(index)) session.draftArgs(this.blocks.get(index), delta.partial_json);
    }
    if (event.type === 'result') this.result(session, event);
  }

  // Starts a follow-up turn on an idle process.
  begin(session, input) {
    this.to('busy');
    clearTimeout(this.idle);
    this.session = session;
    // Blocks from the last turn whose MCP call never came.
    this.toolUses.clear();
    this.blocks.clear();
    this.run.write(input);
  }

  // The turn's result always finishes its session; the process then idles
  // only if the turn completed. Its record is written again with the task
  // list as the turn left it.
  result(session, result) {
    this.session = null;
    try {
      if (result.is_error) throw new Error(`Claude Code ${result.subtype}: ${result.result ?? ''}`);
      session.update({ usage: usageOf(result) });
      this.sessionId = result.session_id;
    } catch (error) {
      session.finish(error);
      return this.kill();
    }
    session.finish(null, session.conversation && (() => recordSession(this.conversationId, result.session_id, session)));
    if (!this.conversationId || session.outcome.error) return this.kill();
    this.to('idle');
    this.idle = setTimeout(() => this.kill(), IDLE_MS);
    this.idle.unref();
  }

  // Resolves once the process has exited and its files are gone.
  kill() {
    if (this.state === 'dead' || this.state === 'exited') return this.exited.promise;
    const started = this.state !== 'spawning';
    this.to('dead');
    clearTimeout(this.idle);
    if (processes.get(this.conversationId) === this) processes.delete(this.conversationId);
    if (started) this.run.stop();
    return this.exited.promise;
  }

  async exit() {
    this.to('exited');
    clearTimeout(this.idle);
    if (processes.get(this.conversationId) === this) processes.delete(this.conversationId);
    harness.untrack(this);
    // The process is gone whatever its cleanup does, so kill() and shutdown
    // never wait on a cleanup that failed.
    try {
      await this.server?.shutdown();
      if (this.runtimeDir) fs.rmSync(this.runtimeDir, { recursive: true, force: true });
    } finally {
      this.exited.resolve();
    }
  }
}

class ClaudeCodeSession extends harness.RelaySession {
  constructor(signal) {
    super('Claude Code', signal);
    this.process = null;
  }

  async start({ conversation, conversationId, messages, tools, model, effort, chat }) {
    this.allowed = tools.map(t => PREFIX + t.function.name);
    this.builtins = chat ? CHAT_BUILTINS : [];
    this.conversation = conversation;
    // The transcript's head is this turn's request.
    this.request = conversation?.messages.at(-1).id;
    const systemPrompt = harness.systemText(messages);
    this.terms = harness.sessionTerms(messages);
    const record = conversationId ? harness.readSession(conversationId, NAME) : null;
    // A session filed under another folder than CWD (a chat's own folder)
    // cannot be resumed, nor one given other instructions than the resolution
    // (harness.sessionTerms): the chat starts a new session with its history.
    const resume = Boolean(conversation) && record?.cwd === CWD && record.instructions === this.terms.instructions
      && harness.sessionContinues(record, conversation.messages);
    if (conversationId) harness.forgetSession(conversationId, NAME);
    // Rendered before anything is opened, so a bad message leaks nothing.
    const input = streamInput(harness.harnessPrompt(messages, resume
      ? { history: false, notes: [...harness.stoppedReplyNote(record, conversation.messages), ...harness.resolutionNote(record, messages)] }
      : { history: true, notes: [] }));
    // A resumed session keeps its task ids, so its task list carries over.
    if (resume) this.tasks = new Map(record.tasks);
    // The instructions, not the whole system prompt: a resumed session keeps
    // its own, so a changed resolution needs no other process.
    const key = JSON.stringify([model, effort, this.terms.instructions, this.allowed, this.builtins]);
    const warm = conversationId && processes.get(conversationId);
    if (warm?.state === 'idle' && resume && warm.key === key && warm.sessionId === record.sessionId) {
      this.process = warm;
      warm.begin(this, input);
      return;
    }
    warm?.kill();
    fs.mkdirSync(CWD, { recursive: true, mode: 0o700 });
    const session = conversationId ? { id: resume ? record.sessionId : crypto.randomUUID(), resume } : null;
    this.process = new ClaudeProcess({ conversationId, key });
    await this.process.spawn(this, {
      args: (socket) => claudeArgs({ model, effort, systemPrompt, allowedTools: this.allowed, socket, session, oneShot: !conversationId, builtins: this.builtins }),
      cwd: CWD, tools, input,
    });
  }

  // A turn stopped before its result takes its process with it; one that
  // finished leaves it for the next turn. No process yet: start failed early.
  stop() { this.process?.kill(); }
}

const { completion, closeSession } = harness.sessionRelay(ClaudeCodeSession);

module.exports = { completion, closeSession, storedLogin, account, version, configure, configDir, runtime, relayEvent, contextTokensOf, claudeArgs, claudeEnv, checkInit, usageOf, PREFIX };
module.exports._internal = { ClaudeCodeSession, streamInput, processes, CWD, transcriptFile };
