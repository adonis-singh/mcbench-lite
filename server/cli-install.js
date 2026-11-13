'use strict';

// Installing the Claude Code or Codex CLI from Connect an AI, for a person
// who does not have it: the pinned release (tools.js PINS.claude and
// PINS.codex) is downloaded into the tools folder, and the app runs it from
// there (settings.js applyRuntimes), which signing in from Settings
// (provider-login.js) then uses too. The page names only which CLI; where it
// goes is the tools folder's, never the browser's or a setting's. A CLI that
// is on PATH, or named by MCBENCH_CLAUDE_BINARY or MCBENCH_CODEX_BINARY, is
// the one the app runs, and is not installed; neither is one the tools
// folder has.
//
// An install's `state` is running (`progress`, { done, total } bytes of its
// download), done or failed (`error`). The latest install of each CLI stays
// until the next one starts, so the page can show how it ended.

const fs = require('fs');
const tools = require('./tools');
const settings = require('./settings');
const { httpError } = require('./http-guard');

const PROGRAMS = { claude: 'MCBENCH_CLAUDE_BINARY', codex: 'MCBENCH_CODEX_BINARY' };

const installs = new Map(); // program -> its latest install

function view(install) {
  return { state: install.state, progress: install.progress, error: install.error };
}

function program(name) {
  if (!Object.hasOwn(PROGRAMS, name)) throw httpError(400, `No CLI named ${name} can be installed: claude or codex`);
  return name;
}

function start(name) {
  program(name);
  const running = installs.get(name);
  if (running?.state === 'running') return view(running);
  if (process.env[PROGRAMS[name]] || tools.onPath(name) || fs.existsSync(tools.installed(name).folder)) throw httpError(409, `${name} is installed already, and the app runs that one`);
  const install = { state: 'running', progress: null, error: null };
  installs.set(name, install);
  console.log(`[tools] installing ${tools.PINS[name].title} (about ${tools.PINS[name].mb} MB) into ${tools.TOOLS_DIR}`);
  tools.install(name, { onProgress: (bytes) => { install.progress = bytes; } }).then(() => {
    settings.refreshRuntimes();
    Object.assign(install, { state: 'done', progress: null });
    console.log(`[tools] ${tools.PINS[name].title} installed`);
  }, (error) => {
    Object.assign(install, { state: 'failed', progress: null, error: error.message });
    console.error(`[tools] ${tools.PINS[name].title} was not installed:`, error.message);
  });
  return view(install);
}

// The latest install of the CLI, or null when none was started.
function state(name) {
  const install = installs.get(program(name));
  return install ? view(install) : null;
}

module.exports = { start, state, PROGRAMS };
