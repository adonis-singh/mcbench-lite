#!/usr/bin/env node
'use strict';

// A stand-in for `claude` (Claude Code 2.1.322) in tests. `auth login` prints
// what the real CLI prints when its stdin and stdout are pipes (copied from
// it), then reads pasted codes from stdin as the real one does: a line that
// is not "code#state" is refused with the real CLI's message and it waits on.
// The code "good" signs in and records the account where the real CLI does
// (.claude.json in CLAUDE_CONFIG_DIR); any other code fails. The line it got
// and the BROWSER it was given are written beside it for the test to read.

const fs = require('node:fs');
const path = require('node:path');
const readline = require('node:readline');

const dir = process.env.CLAUDE_CONFIG_DIR;
const [command, sub, flag] = process.argv.slice(2);
const config = path.join(dir, '.claude.json');

if (command === '--version') {
  process.stdout.write('2.1.322 (Claude Code)\n');
} else if (command === 'auth' && sub === 'logout') {
  fs.rmSync(config, { force: true });
  process.stdout.write('Successfully logged out from your Anthropic account.\n');
} else if (command === 'auth' && sub === 'login') {
  fs.writeFileSync(path.join(dir, 'fake-browser'), process.env.BROWSER || '');
  const authorize = flag === '--console' ? 'https://platform.claude.com/oauth/authorize' : 'https://claude.com/cai/oauth/authorize';
  process.stdout.write('Opening browser to sign in…\n');
  process.stdout.write(`If the browser didn't open, visit: ${authorize}?code=true&client_id=fake-client&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=fake&code_challenge_method=S256&state=fake-state\n`);
  process.stdout.write('Paste code here if prompted > ');
  readline.createInterface({ input: process.stdin }).on('line', (line) => {
    const [code, state] = line.trim().split('#');
    if (!code || !state) {
      process.stderr.write('Invalid code. Please make sure the full code was copied.\n');
      return;
    }
    fs.writeFileSync(path.join(dir, 'fake-received'), line);
    if (code !== 'good') {
      process.stderr.write('Login failed: Request failed with status code 400\n');
      process.exit(1);
    }
    fs.writeFileSync(config, JSON.stringify({ oauthAccount: { emailAddress: 'person@example.com', organizationType: 'claude_max', organizationRateLimitTier: 'default_claude_max_20x' } }));
    process.stdout.write('Login successful.\n');
    process.exit(0);
  });
} else {
  process.stderr.write(`fake claude: unexpected arguments ${process.argv.slice(2).join(' ')}\n`);
  process.exit(2);
}
