#!/usr/bin/env node
'use strict';

// A stand-in for `codex` (codex-cli 0.167.1) in tests. `login --device-auth`
// prints what the real CLI prints on a pipe, colours included (copied from
// it, with a made-up one-time code), and then waits as the real one does
// while it asks OpenAI whether the code was entered: here, until the test
// writes fake-approve (the login file is written and it exits 0, as the real
// one does) or fake-deny (it fails) in CODEX_HOME. The BROWSER it was given
// is written beside it for the test to read.

const fs = require('node:fs');
const path = require('node:path');

const home = process.env.CODEX_HOME;
const args = process.argv.slice(2).join(' ');
const auth = path.join(home, 'auth.json');

if (args === '--version') {
  process.stdout.write('codex-cli 0.167.1\n');
} else if (args === 'logout') {
  if (fs.existsSync(auth)) {
    fs.rmSync(auth);
    process.stdout.write('Successfully logged out\n');
  } else {
    process.stderr.write('Not logged in\n');
  }
} else if (args === 'login --device-auth') {
  fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(path.join(home, 'fake-browser'), process.env.BROWSER || '');
  process.stdout.write('\nWelcome to Codex [v\u001b[90m0.167.1\u001b[0m]\n\u001b[90mOpenAI\'s command-line coding agent\u001b[0m\n\n'
    + 'Follow these steps to sign in with ChatGPT using device code authorization:\n\n'
    + '1. Open this link in your browser and sign in to your account\n   \u001b[94mhttps://auth.openai.com/codex/device\u001b[0m\n\n'
    + '2. Enter this one-time code \u001b[90m(expires in 15 minutes)\u001b[0m\n   \u001b[94mFAKE-C0DE1\u001b[0m\n\n'
    + '\u001b[90mContinue only if you started this login in Codex. If a website or another person gave you this code, cancel.\u001b[0m\n\n');
  const timer = setInterval(() => {
    if (fs.existsSync(path.join(home, 'fake-approve'))) {
      const claims = { email: 'person@example.com', 'https://api.openai.com/auth': { chatgpt_plan_type: 'pro' } };
      const idToken = ['e30', Buffer.from(JSON.stringify(claims)).toString('base64url'), 'sig'].join('.');
      fs.writeFileSync(auth, JSON.stringify({ tokens: { id_token: idToken } }));
      process.stdout.write('Successfully logged in\n');
      clearInterval(timer);
    } else if (fs.existsSync(path.join(home, 'fake-deny'))) {
      process.stderr.write('Error logging in: device code exchange failed: the code was denied\n');
      process.exit(1);
    }
  }, 20);
} else {
  process.stderr.write(`fake codex: unexpected arguments ${args}\n`);
  process.exit(2);
}
