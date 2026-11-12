'use strict';

// Codex CLI (`codex exec`) as a chat harness and as the batch harness's
// runtime. Codex keeps built-ins that no feature flag removes (apply_patch, MCP
// resource listing, a clock). On Linux it therefore runs inside a container
// with --network none in the sandbox image, with only an empty workspace, its
// own CODEX_HOME and the ChatGPT login mounted; its only route out is the host
// egress proxy, which admits the model API hosts, and mcbench tools arrive over
// the mounted MCP socket. macOS cannot share Unix sockets with Docker Desktop
// containers or run the host's Mach-O binary in one, so there Codex runs on
// the host like Claude Code: each chat's own CODEX_HOME linking to the login,
// user config, rules and AGENTS.md ignored, every setting passed as an override, writes
// denied by Codex's read-only sandbox, and the MCP bridge on a host socket.
// Each conversation keeps one Codex thread that follow-up turns resume, a
// turn that was stopped or failed included.
// A completed turn's context reading comes from the thread's rollout file in
// the run's CODEX_HOME, since stdout reports only the turn's summed usage.

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const crypto = require('node:crypto');
const { spawn, execFile, execFileSync } = require('node:child_process');
const { promisify } = require('node:util');
const sandbox = require('./sandbox');
const { startEgressProxy } = require('./egress-proxy');
const { serveMcp, BRIDGE } = require('./mcp');
const harness = require('./harness');

const NAME = 'codex-cli';
// The Codex executable and CODEX_HOME; Settings can point them elsewhere.
const runtime = { binary: null, home: null };
function configure({ binary, home }) {
  runtime.binary = binary || null;
  runtime.home = home ? harness.expandHome(home) : null;
}
const codexHome = () => runtime.home || process.env.CODEX_HOME || path.join(os.homedir(), '.codex');
const SHIM = path.join(__dirname, 'container-shim.py');
// The ChatGPT-login model API and its token refresh endpoint.
const ALLOWED_HOSTS = ['chatgpt.com', 'auth.openai.com'];
const DISABLED_FEATURES = ['shell_tool', 'unified_exec', 'view_image', 'apps', 'browser_use', 'browser_use_external', 'computer_use',
  'image_generation', 'multi_agent', 'plugins', 'memories', 'goals', 'in_app_browser', 'tool_suggest', 'skill_search', 'remote_plugin',
  'shell_snapshot', 'skill_mcp_dependency_install', 'workspace_dependencies', 'sleep_tool', 'multi_agent_v2', 'code_mode_host'];
// The items a turn may report: the model's text, its reasoning, its plan,
// Codex's warnings, and calls to mcbench's tools. Any other item is a tool
// mcbench did not give it (a shell command, a file change, a web search, a
// subagent, another MCP server, or a kind added in a later Codex).
const ALLOWED_ITEMS = new Set(['agent_message', 'reasoning', 'todo_list', 'error', 'mcp_tool_call']);
const IMAGE_EXTENSIONS = { 'image/png': 'png', 'image/jpeg': 'jpg', 'image/webp': 'webp', 'image/gif': 'gif' };
const CONTAINER_BRIDGE = { command: 'python3', args: ['/run/mcbench/mcp-bridge.py', '/run/mcbench/mcp.sock'] };

const codexBinary = () => {
  const binary = runtime.binary || 'codex';
  return fs.realpathSync(binary.includes('/') ? binary : execFileSync('which', [binary], { encoding: 'utf8' }).trim());
};
const version = () => execFileSync(codexBinary(), ['--version'], { encoding: 'utf8', timeout: 5000 }).trim();
const authFile = () => path.join(codexHome(), 'auth.json');
const storedLogin = () => fs.existsSync(authFile());

// Who is signed in and on what plan: the ChatGPT login's ID token carries the
// email and `chatgpt_plan_type` ("pro" reads ChatGPT Pro). Only those two
// claims leave this function; the tokens never do.
let login = { file: null, mtimeMs: null, account: null };
function account() {
  if (!storedLogin()) return null;
  const file = authFile();
  const { mtimeMs } = fs.statSync(file);
  if (file !== login.file || mtimeMs !== login.mtimeMs) {
    const token = JSON.parse(fs.readFileSync(file, 'utf8')).tokens?.id_token;
    const claims = token ? JSON.parse(Buffer.from(token.split('.')[1], 'base64url').toString('utf8')) : {};
    const plan = claims['https://api.openai.com/auth']?.chatgpt_plan_type;
    login = { file, mtimeMs, account: { email: claims.email || null, plan: plan ? `ChatGPT ${plan[0].toUpperCase()}${plan.slice(1)}` : null } };
  }
  return login.account;
}
const toml = value => JSON.stringify(value);

// The run's configuration as dotted TOML keys: a config.toml for the container
// run, `-c` overrides for the host run. `sandbox` is the policy for Codex's
// own command sandbox, which only apply_patch can still reach.
function configEntries({ model, effort, systemPrompt, sandbox: sandboxMode, bridge }) {
  return [
    ['model', toml(model)],
    ...(effort ? [['model_reasoning_effort', toml(effort)]] : []),
    ['approval_policy', toml('never')],
    ['sandbox_mode', toml(sandboxMode)],
    ['web_search', toml('disabled')],
    ['check_for_update_on_startup', 'false'],
    ['tool_output_token_limit', '200000'],
    ['developer_instructions', toml(systemPrompt)],
    // No AGENTS.md from the working directory or the repository around it.
    ['project_doc_max_bytes', '0'],
    ...DISABLED_FEATURES.map(feature => [`features.${feature}`, 'false']),
    ['mcp_servers.mcbench.command', toml(bridge.command)],
    ['mcp_servers.mcbench.args', `[${bridge.args.map(toml).join(', ')}]`],
    ['mcp_servers.mcbench.startup_timeout_sec', '120'],
    ['mcp_servers.mcbench.tool_timeout_sec', '2000000'],
    // Outside full-access sandboxes Codex asks before MCP tool calls; with no
    // one to ask, mcbench's tools are pre-approved.
    ['mcp_servers.mcbench.default_tools_approval_mode', toml('approve')],
  ];
}

function configToml({ model, effort, systemPrompt }) {
  return configEntries({ model, effort, systemPrompt, sandbox: 'danger-full-access', bridge: CONTAINER_BRIDGE }).map(([key, value]) => `${key} = ${value}`).join('\n') + '\n';
}

// The harness container: loopback-only networking, host UID, and only the
// listed mounts. The shim relays 127.0.0.1:3128 to the egress proxy socket.
function containerArgs({ name, runtimeDir, home, workspace, command }) {
  return ['run', '--rm', '-i', '--name', name, '--network', 'none', '--cap-drop', 'all', '--security-opt', 'no-new-privileges',
    '--user', `${process.getuid()}:${process.getgid()}`, '--memory', '4g', '--pids-limit', '512', '--cpus', '4',
    '-v', `${path.dirname(path.dirname(codexBinary()))}:/opt/codex:ro`, '-v', `${home}:/codex-home`, '-v', `${authFile()}:/codex-home/auth.json`,
    '-v', `${workspace}:/workspace`, '-v', `${path.join(runtimeDir, 'sock')}:/run/mcbench`,
    '-e', 'CODEX_HOME=/codex-home', '-e', 'HOME=/codex-home', '-e', 'HTTPS_PROXY=http://127.0.0.1:3128', '-e', 'HTTP_PROXY=http://127.0.0.1:3128',
    '-e', 'ALL_PROXY=http://127.0.0.1:3128', '-e', 'SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt', '-w', '/workspace',
    sandbox.IMAGE, 'python3', '/run/mcbench/container-shim.py', ...command];
}

// The host run: the login's CODEX_HOME, no user config or rules, settings as
// overrides. The ChatGPT login pays, not an API key from the app's .env.
function hostArgs({ model, effort, systemPrompt, socket, resume, threadId, images }) {
  const entries = configEntries({ model, effort, systemPrompt, sandbox: 'read-only', bridge: { command: 'python3', args: [BRIDGE, socket] } });
  return ['exec', ...(resume ? ['resume', threadId] : []), '--json', '--skip-git-repo-check', '--ignore-user-config', '--ignore-rules',
    ...entries.flatMap(([key, value]) => ['-c', `${key}=${value}`]), ...images.flatMap(file => ['-i', file]), '-'];
}

function hostEnv(home = codexHome()) {
  return harness.cliEnv({ CODEX_HOME: home });
}

// A chat's own CODEX_HOME on the host: the login (linked, so token refreshes
// reach the real one) and the chat's sessions. The real CODEX_HOME's
// AGENTS.md, skills and memories would otherwise enter the model's prompt.
function linkLogin(home) {
  fs.mkdirSync(home, { recursive: true });
  const auth = path.join(home, 'auth.json');
  if (!fs.lstatSync(auth, { throwIfNoEntry: false })) fs.symlinkSync(authFile(), auth);
}

// A CODEX_HOME holding only what the run writes, plus the mount target for the login.
function prepareHome(home) {
  fs.mkdirSync(home, { recursive: true });
  const auth = path.join(home, 'auth.json');
  if (!fs.existsSync(auth)) fs.writeFileSync(auth, '');
}

// The mounted socket directory with the bridge and shim, and the egress proxy.
async function prepareRuntime({ runtimeDir, logFile }) {
  const sock = path.join(runtimeDir, 'sock');
  fs.mkdirSync(sock, { recursive: true });
  fs.copyFileSync(BRIDGE, path.join(sock, 'mcp-bridge.py'));
  fs.copyFileSync(SHIM, path.join(sock, 'container-shim.py'));
  return startEgressProxy(path.join(sock, 'egress.sock'), ALLOWED_HOSTS, logFile);
}

// An item outside ALLOWED_ITEMS, or an MCP call to another server, ends the run.
function checkItem(item) {
  if (!ALLOWED_ITEMS.has(item.type) || (item.type === 'mcp_tool_call' && item.server !== 'mcbench')) {
    throw new Error(`Codex used a tool outside mcbench: ${item.type}${item.server ? ` ${item.server}` : ''}`);
  }
}

function usageOf(usage) {
  return { input_tokens: usage.input_tokens, output_tokens: usage.output_tokens,
    input_tokens_details: { cached_tokens: usage.cached_input_tokens }, output_tokens_details: { reasoning_tokens: usage.reasoning_output_tokens } };
}

// How full the model's context was: the whole input (fresh plus cached) of
// the turn's last model request. `codex exec --json` prints only the turn's
// usage summed over all its requests, so the number comes from the thread's
// rollout file, `<home>/sessions/YYYY/MM/DD/rollout-<time>-<threadId>.jsonl`,
// where each request adds a `token_count` event whose `last_token_usage` is
// that request's usage. Events with `info: null` carry only rate limits.
function contextTokensOf(home, threadId) {
  const sessions = path.join(home, 'sessions');
  const suffix = `-${threadId}.jsonl`;
  const file = fs.readdirSync(sessions, { recursive: true }).find(name => path.basename(name).startsWith('rollout-') && name.endsWith(suffix));
  if (!file) throw new Error(`No Codex rollout file for thread ${threadId} under ${sessions}`);
  const events = fs.readFileSync(path.join(sessions, file), 'utf8').split('\n').filter(Boolean).map(line => JSON.parse(line));
  const last = events.findLast(event => event.type === 'event_msg' && event.payload.type === 'token_count' && event.payload.info);
  if (!last) throw new Error(`No token_count event with usage in ${path.join(sessions, file)}`);
  return last.payload.info.last_token_usage.input_tokens;
}

// The models the signed-in account may use, as Codex itself resolves them
// (`codex debug models` fetches the live catalogue). Kept for five minutes;
// Settings' re-check asks again.
const CATALOG_TTL_MS = 5 * 60 * 1000;
// Callers while it is fetched share the one fetch.
let catalog = { at: 0, models: null };
let loading = null;
async function listModels() {
  if (catalog.models && Date.now() - catalog.at <= CATALOG_TTL_MS) return catalog.models;
  loading ||= promisify(execFile)(codexBinary(), ['debug', 'models'], { env: hostEnv(), maxBuffer: 64 * 1024 * 1024, timeout: 60_000 })
    .then(({ stdout }) => {
      catalog = { at: Date.now(), models: modelsFromCache(JSON.parse(stdout)) };
      return catalog.models;
    })
    .finally(() => { loading = null; });
  return loading;
}
const forgetModels = () => { catalog = { at: 0, models: null }; };

function modelsFromCache(cache) {
  return cache.models.filter(m => m.visibility === 'list').sort((a, b) => a.priority - b.priority).map(m => {
    const efforts = (m.supported_reasoning_levels || []).map(level => level.effort);
    // A "fast" speed tier is the account's fast mode for this model.
    const fast = (m.additional_speed_tiers || []).includes('fast');
    return { id: m.slug, name: m.display_name, created: 0, known: true, imageInput: (m.input_modalities || []).includes('image'), imageOutput: false,
      tools: true, reasoning: efforts.length > 0, efforts, defaultEffort: m.default_reasoning_level || null, mandatoryReasoning: efforts.length > 0,
      contextWindow: m.context_window || null,
      fast, fastNote: fast ? (m.service_tiers || []).find(tier => tier.name === 'Fast')?.description || null : null };
  });
}

async function modelInfo(id) {
  const entry = (await listModels()).find(m => m.id === id);
  if (!entry) return null;
  const { id: _id, name, created, ...info } = entry;
  return info;
}

function writeImages(dir, images) {
  return images.map((image, index) => {
    const file = path.join(dir, `image-${index + 1}.${IMAGE_EXTENSIONS[image.mimeType] || 'png'}`);
    fs.writeFileSync(file, Buffer.from(image.data, 'base64'));
    return file;
  });
}

// How the CLI is launched on this platform; each returns the spawn command and
// what to release afterwards.
const LAUNCHERS = {
  async linux({ dir, runtimeDir, socket, model, effort, systemPrompt, resume, threadId, prompt }) {
    const home = path.join(dir, 'codex-home');
    const workspace = path.join(dir, 'workspace');
    prepareHome(home);
    fs.mkdirSync(workspace, { recursive: true });
    fs.writeFileSync(path.join(home, 'config.toml'), configToml({ model, effort, systemPrompt }));
    await sandbox.ensureImage();
    const images = writeImages(path.join(runtimeDir, 'sock'), prompt.images).map(file => `/run/mcbench/${path.basename(file)}`);
    const proxy = await prepareRuntime({ runtimeDir, logFile: path.join(dir, 'egress.ndjson') });
    const server = await serveMcp(socket, { tools: prompt.tools, call: prompt.call }).catch(async (error) => {
      await new Promise(resolve => proxy.close(resolve));
      throw error;
    });
    const command = ['/opt/codex/bin/codex', 'exec', ...(resume ? ['resume', threadId] : []), '--json', '--skip-git-repo-check', ...images.flatMap(file => ['-i', file]), '-'];
    const name = `mcbench-codex-${crypto.randomUUID().slice(0, 12)}`;
    return {
      command: 'docker', args: containerArgs({ name, runtimeDir, home, workspace, command }), cwd: workspace, env: process.env,
      onStop: () => spawn('docker', ['rm', '-f', name], { stdio: 'ignore' }),
      release: async () => { await new Promise(resolve => proxy.close(resolve)); await server.shutdown(); },
    };
  },
  async darwin({ dir, runtimeDir, socket, model, effort, systemPrompt, resume, threadId, prompt }) {
    const home = path.join(dir, 'codex-home');
    const workspace = path.join(dir, 'workspace');
    linkLogin(home);
    fs.mkdirSync(workspace, { recursive: true });
    const images = writeImages(runtimeDir, prompt.images);
    const server = await serveMcp(socket, { tools: prompt.tools, call: prompt.call });
    return {
      command: codexBinary(), args: hostArgs({ model, effort, systemPrompt, socket, resume, threadId, images }), cwd: workspace, env: hostEnv(home),
      onStop: () => {}, release: () => server.shutdown(),
    };
  },
};

class CodexSession extends harness.RelaySession {
  constructor(signal) {
    super('Codex', signal);
    this.run = null;
  }

  async start({ conversation, conversationId, messages, tools, model, effort }) {
    const launcher = LAUNCHERS[process.platform];
    if (!launcher) throw new Error(`The Codex CLI harness runs on Linux (in a container) or macOS (on the host), not ${process.platform}`);
    const record = conversationId ? harness.readSession(conversationId, NAME) : null;
    const terms = harness.sessionTerms(messages);
    // Threads live in the chat's own CODEX_HOME; older records point
    // elsewhere. A thread given other instructions than the resolution
    // (harness.sessionTerms) is not resumed either: the chat starts a new
    // thread with its history.
    const resume = Boolean(conversation) && record?.home === 'chat' && record.instructions === terms.instructions
      && harness.sessionContinues(record, conversation.messages);
    if (conversationId) harness.forgetSession(conversationId, NAME);
    // The transcript's head is this turn's request, which the record names.
    const request = conversation?.messages.at(-1).id;
    const dir = conversationId ? harness.conversationDir(conversationId) : fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-codex-'));
    const runtimeDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcb-'));
    fs.mkdirSync(path.join(runtimeDir, 'sock'));
    const prefix = crypto.randomUUID().slice(0, 8);
    // Codex streams no partial text (an agent message reaches the app once it
    // is complete, and the thread holds it), so a resumed thread lacks only a
    // changed resolution.
    const { text, images } = harness.harnessPrompt(messages, resume
      ? { history: false, notes: harness.resolutionNote(record, messages) }
      : { history: true, notes: [] });
    const cleanup = () => {
      fs.rmSync(runtimeDir, { recursive: true, force: true });
      // A one-shot run (a title, a prompt rewrite) keeps nothing.
      if (!conversationId) fs.rmSync(dir, { recursive: true, force: true });
    };
    let launch;
    try {
      launch = await launcher({
        dir, runtimeDir, socket: path.join(runtimeDir, 'sock', 'mcp.sock'), model, effort, systemPrompt: harness.systemText(messages),
        resume, threadId: record?.threadId,
        prompt: { images, tools, call: (name, args, { callId, signal }) => this.call(`${prefix}:${callId}`, name, args, signal) },
      });
    } catch (error) {
      cleanup();
      throw error;
    }
    // Stopped while launching: nothing was spawned.
    if (this.state === 'closed') {
      await launch.release();
      cleanup();
      throw this.closeReason();
    }
    const state = { threadId: resume ? record.threadId : null, failure: null, completed: false };
    this.run = harness.runJsonLines(launch.command, launch.args, {
      cwd: launch.cwd, env: launch.env, input: text, onStop: launch.onStop,
      onLine: event => {
        const item = event.item;
        if (item) checkItem(item);
        // Codex names the thread as the turn starts, and the thread holds the
        // turn however it ends, so the next turn resumes it. Tested on Codex
        // 0.167.1: a thread killed in the middle of a tool call resumes with
        // that call answered as aborted, and the tool does not run again.
        if (event.type === 'thread.started') {
          state.threadId = event.thread_id;
          if (conversation) harness.writeSession(conversationId, NAME, { threadId: state.threadId, home: 'chat', ...terms, lastSeen: request });
        }
        if (event.type === 'item.completed' && item.type === 'agent_message') this.update({ text: (this.text ? '\n\n' : '') + item.text });
        if (event.type === 'item.completed' && item.type === 'reasoning') this.update({ reasoning: (this.reasoning ? '\n\n' : '') + item.text });
        if (event.type === 'turn.completed') {
          state.completed = true;
          this.update({ usage: usageOf(event.usage) });
        }
        if (item?.type === 'todo_list') this.setPlan(item.items.map(entry => ({ text: entry.text, status: entry.completed ? 'done' : 'pending' })));
        if (event.type === 'item.completed' && item.type === 'error') this.notice('warning', item.message);
        if (event.type === 'turn.failed') state.failure = event.error?.message || JSON.stringify(event.error);
        if (event.type === 'error') state.failure = event.message || JSON.stringify(event);
      },
    });
    // A stopped turn's exit, even with code 0, finishes nothing: the session
    // is closed.
    this.run.done.then(exit => {
      if (state.failure) throw new Error(`Codex turn failed: ${state.failure}`);
      if (exit.code !== 0) throw new Error(`codex exec exited ${exit.code ?? exit.signal}: ${exit.stderr}`);
      // Codex writes the rollout file from a background writer and flushes it
      // before exiting, so it is complete only now that the process has exited.
      if (state.completed && this.state === 'running') this.setContextTokens(contextTokensOf(path.join(dir, 'codex-home'), state.threadId));
      this.finish(null);
    }).catch(error => this.finish(error)).finally(async () => {
      try { await launch.release(); } finally { cleanup(); }
    }).catch(error => console.error('[codex-cli]', error));
  }

  // No run yet: stopped while launching, which start handles.
  stop() { this.run?.stop(); }
}

const { completion, closeSession } = harness.sessionRelay(CodexSession);

module.exports = {
  completion, closeSession, listModels, modelInfo, forgetModels, storedLogin, account, version, configToml, containerArgs, hostArgs, hostEnv, prepareHome, prepareRuntime,
  checkItem, usageOf, contextTokensOf, modelsFromCache, configure, codexHome, ALLOWED_HOSTS,
};
module.exports._internal = { LAUNCHERS };
