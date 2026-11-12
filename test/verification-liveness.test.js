'use strict';
const assert = require('node:assert/strict');
const { test } = require('node:test');
const { runtimeLivenessCheck } = require('../server/verification-liveness');

const running = () => ({ exitCode: null, signalCode: null });

test('a running Minecraft process passes the liveness check', () => {
  runtimeLivenessCheck({ server: running() })();
});

test('a process that exited, was killed, or was stopped fails the verification as an infrastructure failure', () => {
  for (const server of [{ exitCode: 1, signalCode: null }, { exitCode: 143, signalCode: null }, { exitCode: null, signalCode: 'SIGKILL' }, null]) {
    assert.throws(runtimeLivenessCheck({ server }), /runtime exited or became unavailable.*infrastructure failure/);
  }
});

test('the check follows the live server as it changes', () => {
  const live = { server: running() };
  const check = runtimeLivenessCheck(live);
  check();
  live.server.exitCode = 0;
  assert.throws(check, /infrastructure failure/);
});
