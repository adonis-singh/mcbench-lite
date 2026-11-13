'use strict';

// Signing in to a subscription from Settings. The app does what a terminal
// does: it starts the provider's own login (the CLI's login command, or the
// Cursor SDK's login in this process), shows the address and one-time code
// the login prints, and passes a line the person types (Claude Code's pasted
// code) to the CLI's stdin. Anthropic's terms for hosting Claude Code ask for
// exactly this: the unmodified CLI with both of its sign-in methods, each
// person signing in through Anthropic's own pages, and no credential held by
// the app. The line the person types goes to the CLI and nowhere else: it is
// not kept, logged or returned. The login is the CLI's: it writes its own
// files, and the app reads from them only what Settings read before, the
// account and its plan.
//
// Each CLI was run with pipes for stdin and stdout and prints its address
// there, so no pseudo-terminal is needed: Claude Code 2.1.322 prints the
// address, then "Paste code here if prompted > ", and reads "code#state"
// lines from stdin; Codex 0.167.1 prints the address and a one-time code in
// colour, then waits for OpenAI to see the code entered.

const { spawn } = require('node:child_process');
const claudeCode = require('./claude-code');
const codexCli = require('./codex-cli');
const cursor = require('./cursor');
const tools = require('./tools');
const { httpError } = require('./http-guard');

// A sign-in nobody finishes is ended after this long.
const LIMIT_MS = 10 * 60_000;
// Colours and other terminal sequences the CLIs print on a pipe.
const TERMINAL_SEQUENCES = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g;
const LAST_LINES = 4;

// What each login printed that the page shows, read from all its output so
// far (without terminal sequences), or null while its address is not out.
// `message` is the last line the CLI printed after asking for a code, such
// as Claude Code's refusal of a code that is not whole.
function claudeShown(output) {
  const url = /(?:visit|Sign in at): (https:\/\/\S+)\n/.exec(output)?.[1];
  if (!url) return null;
  const prompt = output.lastIndexOf('Paste code here');
  const after = prompt < 0 ? '' : output.slice(output.indexOf('>', prompt) + 1);
  return { url, userCode: null, wantsInput: prompt >= 0, message: lastLines(after, 1) };
}

function codexShown(output) {
  const url = /Open this link[^\n]*\n\s*(https:\/\/\S+)\n/.exec(output)?.[1];
  const userCode = /one-time code[^\n]*\n\s*([A-Z0-9]{4}-[A-Z0-9]{4,})\n/.exec(output)?.[1];
  if (!url || !userCode) return null;
  return { url, userCode, wantsInput: false, message: null };
}

// One login per entry; ChatGPT sign-in and Codex CLI share Codex's login.
// `methods` are the sign-in methods the person picks from, when there is a
// choice: Claude Code's `claude auth login` offers a Claude subscription
// (--claudeai) and the Anthropic Console (--console), and both stay offered.
// `program` is the CLI the login runs, which cli-install.js installs into
// the tools folder when it is missing.
const LOGINS = {
  claude: {
    providers: ['claude-code'],
    methods: ['claudeai', 'console'],
    login: (method) => claudeCode.authCommand(['login', `--${method}`]),
    logout: () => claudeCode.authCommand(['logout']),
    shown: claudeShown,
    stored: claudeCode.storedLogin,
    program: 'claude',
  },
  codex: {
    providers: ['codex', 'codex-cli'],
    login: () => codexCli.loginCommand(['login', '--device-auth']),
    logout: () => codexCli.loginCommand(['logout']),
    shown: codexShown,
    stored: codexCli.storedLogin,
    program: 'codex',
  },
  cursor: {
    providers: ['cursor'],
    sdk: true,
    stored: cursor.storedLogin,
  },
};
const LOGIN_OF = Object.fromEntries(Object.entries(LOGINS).flatMap(([id, entry]) => entry.providers.map((provider) => [provider, id])));

function loginOf(provider) {
  if (!Object.hasOwn(LOGIN_OF, provider)) throw httpError(400, `${provider} has no sign-in`);
  return LOGIN_OF[provider];
}

function lastLines(text, count = LAST_LINES) {
  const lines = text.split('\n').map((line) => line.trim()).filter(Boolean);
  return lines.length ? lines.slice(-count).join('\n') : null;
}

// A sign-in's states: starting (its login runs, its address is not out yet),
// waiting (the address is shown: the login waits for the person), done (the
// login ended well and is stored), failed (it ended any other way, or ran
// past LIMIT_MS) and cancelled (the person ended it). An ended sign-in
// stays until the next one starts, so the page can show how it ended.
const MOVES = {
  starting: ['waiting', 'done', 'failed', 'cancelled'],
  waiting: ['done', 'failed', 'cancelled'],
  done: [],
  failed: [],
  cancelled: [],
};

class SignIn {
  constructor(id) {
    this.id = id;
    this.entry = LOGINS[id];
    this.state = 'starting';
    this.url = null;
    this.userCode = null;
    this.wantsInput = false;
    this.message = null;
    this.error = null;
    this.output = '';
    // The CLI's stdin (null for the SDK's login), and what ends the login.
    this.stdin = null;
    this.end = null;
    this.timer = setTimeout(() => this.stop('failed', `The sign-in was not finished within ${LIMIT_MS / 60_000} minutes, so it was ended. Start it again when you are ready.`), LIMIT_MS);
  }

  to(state) {
    if (!MOVES[this.state].includes(state)) throw new Error(`A sign-in cannot go from ${this.state} to ${state}`);
    this.state = state;
    if (!MOVES[state].length) clearTimeout(this.timer);
  }

  get running() {
    return this.state === 'starting' || this.state === 'waiting';
  }

  view() {
    return { state: this.state, url: this.url, userCode: this.userCode, wantsInput: this.wantsInput, message: this.message, error: this.error };
  }

  // The address is out: the page can show it.
  show(shown) {
    Object.assign(this, shown);
    if (this.state === 'starting') this.to('waiting');
  }

  // Ends the login for `state` (failed or cancelled); its process's exit
  // that follows changes nothing.
  stop(state, error = null) {
    if (!this.running) return;
    this.to(state);
    this.error = error;
    this.end();
  }

  // The login ended by itself: well only if it says so and its login is stored.
  finish(ok, error) {
    if (!this.running) return;
    if (ok && this.entry.stored()) return this.to('done');
    this.to('failed');
    this.error = error || 'The sign-in ended, but no login was stored. Start it again.';
  }

  runCli({ file, args, env }) {
    const child = spawn(file, args, { env, stdio: ['pipe', 'pipe', 'pipe'] });
    this.stdin = child.stdin;
    this.end = () => child.kill('SIGTERM');
    // A code written as the CLI exits fails with EPIPE, which would otherwise
    // be thrown as an unhandled error; the CLI's exit says how it ended.
    child.stdin.on('error', () => {});
    const take = (chunk) => {
      this.output += chunk.toString('utf8').replace(TERMINAL_SEQUENCES, '').replace(/\r/g, '');
      if (!this.running) return;
      const shown = this.entry.shown(this.output);
      if (shown) this.show(shown);
    };
    child.stdout.on('data', take);
    child.stderr.on('data', take);
    child.on('error', (error) => this.finish(false, `${file} could not be started: ${error.message}`));
    child.on('close', (code, signal) => this.finish(code === 0, lastLines(this.output) || `${file} ended with ${signal || `exit code ${code}`}`));
  }

  runSdk() {
    const controller = new AbortController();
    this.end = () => controller.abort();
    cursor.login({ signal: controller.signal, onLoginUrl: (url) => this.show({ url }) })
      .then(() => this.finish(true), (error) => this.finish(false, error.message));
  }
}

const signIns = new Map(); // login id -> SignIn, the latest one started

function start(provider, method) {
  const id = loginOf(provider);
  const entry = LOGINS[id];
  if (signIns.get(id)?.running) throw httpError(409, 'A sign-in is already under way here. Finish it or cancel it first.');
  if (entry.methods && !entry.methods.includes(method)) throw httpError(400, `Choose a sign-in method: ${entry.methods.join(' or ')}`);
  // Resolved first: a CLI that cannot be found fails the start, not a sign-in.
  const command = entry.sdk ? null : entry.login(method);
  const signIn = new SignIn(id);
  signIns.set(id, signIn);
  if (entry.sdk) signIn.runSdk();
  else signIn.runCli(command);
  return signIn.view();
}

function current(provider) {
  const signIn = signIns.get(loginOf(provider));
  if (!signIn) throw httpError(404, 'No sign-in was started here');
  return signIn;
}

// The latest sign-in of the provider's login, or null when none was started.
const state = (provider) => signIns.get(loginOf(provider))?.view() || null;

// The person's line goes to the CLI's stdin as typed, and nowhere else.
function input(provider, text) {
  const signIn = current(provider);
  if (signIn.state !== 'waiting' || !signIn.wantsInput) throw httpError(409, 'This sign-in is not waiting for a code');
  if (typeof text !== 'string' || !text.trim() || /[\r\n]/.test(text)) throw httpError(400, 'Paste the code as one line');
  signIn.stdin.write(`${text.trim()}\n`);
  return signIn.view();
}

function cancel(provider) {
  const signIn = current(provider);
  signIn.stop('cancelled');
  return signIn.view();
}

// Signs out with the provider's own command: the CLI removes its login, or
// the SDK forgets its key. A sign-in under way is finished or cancelled first.
async function logout(provider) {
  const id = loginOf(provider);
  const entry = LOGINS[id];
  if (signIns.get(id)?.running) throw httpError(409, 'Finish or cancel the sign-in first');
  if (entry.sdk) return cursor.logout();
  const { file, args, env } = entry.logout();
  const { code, output } = await new Promise((resolve, reject) => {
    const child = spawn(file, args, { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let text = '';
    child.stdout.on('data', (chunk) => { text += chunk; });
    child.stderr.on('data', (chunk) => { text += chunk; });
    child.on('error', reject);
    child.on('close', (exit) => resolve({ code: exit, output: text.replace(TERMINAL_SEQUENCES, '') }));
  });
  if (code !== 0) throw new Error(lastLines(output) || `${file} ${args.join(' ')} ended with exit code ${code}`);
}

// What Settings shows for a provider's sign-in: its methods, its CLI (the
// name cli-install.js installs it by, and the pinned release's title and
// about how many MB it downloads), and the latest sign-in, or null.
function describe(provider) {
  const id = LOGIN_OF[provider];
  if (!id) return null;
  const entry = LOGINS[id];
  const program = entry.program && { name: entry.program, title: tools.PINS[entry.program].title, mb: tools.PINS[entry.program].mb };
  return { methods: entry.methods || null, program: program || null, current: signIns.get(id)?.view() || null };
}

// Ends every sign-in under way, when the server stops.
function shutdown() {
  for (const signIn of signIns.values()) signIn.stop('cancelled');
}

module.exports = { start, state, input, cancel, logout, describe, shutdown, LIMIT_MS };
