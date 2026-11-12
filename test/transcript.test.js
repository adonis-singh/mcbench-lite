'use strict';

const assert = require('node:assert/strict');
const { test } = require('node:test');

async function projection() {
  return import('../web/transcript.mjs');
}

test('live turns expose speech and reasoning while compacting consecutive tools', async () => {
  const { groupItems } = await projection();
  const tools = Array.from({ length: 5 }, (_, index) => ({
    kind: 'tool',
    callId: `call-${index + 1}`,
    summary: `tool ${index + 1}`,
    status: index === 4 ? 'running' : 'ok',
  }));
  const items = [
    { kind: 'user', text: 'Build a house' },
    { kind: 'reasoning', text: 'Plan the shell' },
    { kind: 'assistant', text: 'Okay, I will build that house for you!', final: false },
    ...tools,
    { kind: 'reasoning', text: 'Inspect the result', live: true },
  ];

  const entries = groupItems(items, true);
  assert.deepStrictEqual(entries.map((entry) => entry.kind), [
    'user', 'reasoning', 'assistant', 'tool-stack', 'reasoning',
  ]);
  assert.equal(entries[2].text, 'Okay, I will build that house for you!');
  assert.equal(entries[3].calls.length, 5);
  assert.equal(entries[3].calls.at(-1).tool.summary, 'tool 5');
});

test('spoken assistant content breaks one tool stack from the next', async () => {
  const { groupItems } = await projection();
  const entries = groupItems([
    { kind: 'user', text: 'Build' },
    { kind: 'tool', summary: 'first' },
    { kind: 'tool', summary: 'second' },
    { kind: 'assistant', text: 'The shell is ready.', final: false },
    { kind: 'tool', summary: 'third' },
  ], true);

  assert.deepStrictEqual(entries.map((entry) => entry.kind), [
    'user', 'tool-stack', 'assistant', 'tool-stack',
  ]);
  assert.equal(entries[1].calls.length, 2);
  assert.equal(entries[3].calls.length, 1);
});

test('live screenshot output stays attached to its latest tool', async () => {
  const { groupItems } = await projection();
  const shot = { kind: 'shots', images: ['/one.jpg'] };
  const entries = groupItems([
    { kind: 'user', text: 'Inspect it' },
    { kind: 'tool', summary: 'see build' },
    shot,
    { kind: 'tool', summary: 'fix roof' },
  ], true);

  assert.equal(entries[1].kind, 'tool-stack');
  assert.deepStrictEqual(entries[1].calls[0].outputs, [shot]);
  assert.deepStrictEqual(entries[1].calls[1].outputs, []);
});

test('settled turns fold all work and expose only the explicit final response', async () => {
  const { groupItems } = await projection();
  const entries = groupItems([
    { kind: 'user', text: 'Build' },
    { kind: 'reasoning', text: 'Plan' },
    { kind: 'assistant', text: 'I am starting the shell.', final: false },
    { kind: 'tool', summary: 'build shell' },
    { kind: 'reasoning', text: 'Check the roof' },
    { kind: 'assistant', text: 'The build is complete!', final: true },
  ], false);

  assert.deepStrictEqual(entries.map((entry) => entry.kind), ['user', 'activity', 'assistant']);
  assert.equal(entries[1].children.length, 4);
  assert.equal(entries[2].text, 'The build is complete!');
});

test('a settled direct reply has no empty activity dropdown', async () => {
  const { groupItems } = await projection();
  const final = { kind: 'assistant', text: 'Hello!', final: true };
  assert.deepStrictEqual(groupItems([
    { kind: 'user', text: 'Hello' },
    final,
  ], false), [
    { kind: 'user', text: 'Hello' },
    final,
  ]);
});

test('a turn begun before its prompt shows leaves the previous turn settled', async () => {
  const { groupItems } = await projection();
  const items = [
    { kind: 'user', text: 'Build' },
    { kind: 'reasoning', text: 'Plan' },
    { kind: 'tool', summary: 'build shell' },
    { kind: 'assistant', text: 'Done!', final: true },
  ];
  assert.deepStrictEqual(groupItems(items, true).map((entry) => entry.kind), groupItems(items, false).map((entry) => entry.kind));
});

test('entry keys hold while a turn grows and change when it settles', async () => {
  const { groupItems, entryKeys } = await projection();
  const first = [{ kind: 'user', text: 'a' }, { kind: 'reasoning', text: 'r' }, { kind: 'assistant', text: 'ok', final: true }];
  const live = [...first, { kind: 'user', text: 'b' }, { kind: 'reasoning', text: 'r2' }];
  const grown = [...live, { kind: 'tool', summary: 't', callId: 'c' }];
  const keys = entryKeys(groupItems(live, true));
  const grownKeys = entryKeys(groupItems(grown, true));
  assert.deepStrictEqual(grownKeys.slice(0, keys.length), keys);
  assert.deepStrictEqual(keys.slice(0, 3), ['1:user', '1:1:activity', '1:2:assistant']);
  const settled = entryKeys(groupItems([...grown, { kind: 'assistant', text: 'done', final: true }], false));
  assert.equal(settled[4], '2:1:activity');
  assert.notEqual(keys[4], settled[4]);
});

test('the page joins a running turn only while it holds no stream and has not seen it end', async () => {
  const { IDLE, turnRunning, turnToJoin } = await projection();
  const row = { id: 'c', runningSince: 100 };
  assert.equal(turnToJoin(IDLE, row), 100);
  assert.equal(turnRunning(IDLE, row), true);
  assert.equal(turnToJoin({ phase: 'reading' }, row), null);
  assert.equal(turnToJoin({ phase: 'waiting' }, row), null);
  assert.equal(turnRunning({ phase: 'sending' }, null), true);
  // The list still shows a turn the page saw end: stale, not joined.
  const ended = { phase: 'idle', ended: 100 };
  assert.equal(turnToJoin(ended, row), null);
  assert.equal(turnRunning(ended, row), false);
  // A later turn in the same chat is.
  assert.equal(turnToJoin(ended, { id: 'c', runningSince: 200 }), 200);
  assert.equal(turnRunning(IDLE, { id: 'c', runningSince: null }), false);
});

test('the turn moves only along its phase table', async () => {
  const { IDLE, turnTo } = await projection();
  const controller = new AbortController();
  const sending = { phase: 'sending', controller, status: null };
  const reading = { phase: 'reading', id: 'c', controller, status: null };
  const detached = { phase: 'detached', id: 'c', startedAt: 100 };
  const waiting = { phase: 'waiting' };
  // A send, read to its end; a status line is reading going on reading.
  assert.equal(turnTo(IDLE, sending), sending);
  assert.equal(turnTo(sending, reading), reading);
  assert.equal(turnTo(reading, { ...reading, status: 'Retrying' }).status, 'Retrying');
  assert.deepStrictEqual(turnTo(reading, { phase: 'idle', ended: 100 }), { phase: 'idle', ended: 100 });
  // A joined turn whose stream breaks, or that the page stops reading.
  assert.equal(turnTo(IDLE, reading), reading);
  assert.equal(turnTo(reading, waiting), waiting);
  assert.equal(turnTo(reading, detached), detached);
  // Leaving the chat goes to idle from every phase.
  for (const turn of [IDLE, sending, reading, waiting, detached]) assert.equal(turnTo(turn, IDLE), IDLE);
  // Nothing is sent or joined while a turn is held, and only a turn being
  // read can lose its stream or be detached from.
  assert.throws(() => turnTo(detached, sending), /cannot go from detached to sending/);
  assert.throws(() => turnTo(detached, reading), /cannot go from detached to reading/);
  assert.throws(() => turnTo(waiting, reading), /cannot go from waiting to reading/);
  assert.throws(() => turnTo(reading, sending), /cannot go from reading to sending/);
  assert.throws(() => turnTo(sending, sending), /cannot go from sending to sending/);
  assert.throws(() => turnTo(IDLE, detached), /cannot go from idle to detached/);
  assert.throws(() => turnTo(sending, detached), /cannot go from sending to detached/);
  assert.throws(() => turnTo(IDLE, waiting), /cannot go from idle to waiting/);
  assert.throws(() => turnTo(IDLE, { phase: 'gone' }), /cannot go from idle to gone/);
  // The phases that hold a stream, and only they, carry its controller.
  assert.throws(() => turnTo(IDLE, { phase: 'reading', id: 'c' }), /holds a stream/);
  assert.throws(() => turnTo(reading, { phase: 'idle', ended: null, controller }), /holds a stream/);
});

test('a detached turn keeps the chat busy until the list stops showing it running', async () => {
  const { turnRunning, turnToJoin, turnOver, IDLE } = await projection();
  const detached = { phase: 'detached', id: 'c', startedAt: 100 };
  const running = { id: 'c', runningSince: 100 };
  assert.equal(turnRunning(detached, running), true);
  assert.equal(turnToJoin(detached, running), null);
  assert.equal(turnOver(detached, running), false);
  // The list says it ended, or shows a later turn of the chat.
  assert.equal(turnOver(detached, { id: 'c', runningSince: null }), true);
  assert.equal(turnOver(detached, { id: 'c', runningSince: 200 }), true);
  assert.equal(turnOver(detached, null), true);
  // A sent turn the list never showed is not waited for.
  assert.equal(turnOver({ phase: 'detached', id: 'c', startedAt: null }, { id: 'c', runningSince: null }), true);
  // Idle again, with that turn seen to end, a list that still shows it is stale.
  const ended = { phase: 'idle', ended: detached.startedAt };
  assert.equal(turnToJoin(ended, running), null);
  assert.equal(turnRunning(ended, running), false);
  assert.equal(turnToJoin(ended, { id: 'c', runningSince: 200 }), 200);
  for (const turn of [IDLE, { phase: 'reading' }, { phase: 'waiting' }, { phase: 'sending' }]) assert.equal(turnOver(turn, null), false);
});

test('a failed transcript read carries why, and only it does', async () => {
  const { LOADING, SHOWN, readFailed } = await projection();
  assert.deepStrictEqual(LOADING, { phase: 'loading', error: null });
  assert.deepStrictEqual(SHOWN, { phase: 'shown', error: null });
  assert.deepStrictEqual(readFailed(new Error('HTTP 500')), { phase: 'failed', error: 'HTTP 500' });
});

test('tool events land on the call they name', async () => {
  const { applyEvent } = await projection();
  let items = [{ kind: 'user', text: 'Build' }];
  for (const event of [
    { type: 'tool_call_delta', index: 0, id: 'a', name: 'build', args: '{"summary":"one"' },
    { type: 'tool_call_delta', index: 1, id: 'b', name: 'bash', args: '{"command":"ls"' },
    { type: 'tool_call_delta', index: 0, id: 'a', args: '}' },
    { type: 'tool_start', callId: 'b', name: 'bash', summary: '$ ls', program: 'ls' },
  ]) items = applyEvent(items, event);
  assert.deepStrictEqual(items.slice(1).map((item) => [item.callId, item.status]), [['a', 'writing'], ['b', 'running']]);
  items = applyEvent(items, { type: 'tool_result', callId: 'b', name: 'bash', ok: true, output: 'x' });
  assert.equal(items[2].status, 'ok');
  assert.equal(items[2].output, 'x');
  assert.throws(() => applyEvent(items, { type: 'tool_result', callId: 'a', name: 'build', ok: true }), /without running/);
  assert.throws(() => applyEvent(items, { type: 'tool_start', callId: 'z', name: 'build', summary: 's' }), /without being written/);
  // Another response's call of the same number is another call.
  items = applyEvent(items, { type: 'tool_call_delta', index: 0, id: 'c', name: 'build', args: '{' });
  assert.deepStrictEqual(items.slice(1).map((item) => item.callId), ['a', 'b', 'c']);
  assert.throws(() => applyEvent(items, { type: 'surprise' }), /Unknown turn event/);
});

test('a call written in parts, with another call\'s result between them, is one call', async () => {
  const { applyEvent } = await projection();
  let items = [{ kind: 'user', text: 'Build' }];
  for (const event of [
    { type: 'tool_call_delta', index: 0, id: 'a', name: 'see_build', args: '{}' },
    { type: 'tool_call_delta', index: 1, id: 'b', name: 'build', args: '{"summary":"ro' },
    { type: 'tool_start', callId: 'a', name: 'see_build', summary: 'Look at build' },
    { type: 'tool_result', callId: 'a', name: 'see_build', ok: true, images: ['/shot.png'] },
    // A harness sends the rest of b's text once the next completion waits.
    { type: 'tool_call_delta', index: 1, id: 'b', name: 'build', args: 'of"}' },
    { type: 'tool_start', callId: 'b', name: 'build', summary: 'roof' },
  ]) items = applyEvent(items, event);
  assert.deepStrictEqual(items.slice(1).map((item) => [item.kind, item.callId, item.status]), [
    ['tool', 'a', 'ok'], ['tool', 'b', 'running'], ['shots', undefined, undefined],
  ]);
  assert.equal(items[2].argsRaw, '{"summary":"roof"}');
  assert.throws(() => applyEvent(items, { type: 'tool_call_delta', index: 0, name: 'build', args: '{' }), /without its id/);
});

test('deltas stream into one live item that the full event seals', async () => {
  const { applyEvent } = await projection();
  let items = [];
  items = applyEvent(items, { type: 'assistant_delta', text: 'Hel' });
  items = applyEvent(items, { type: 'assistant_delta', text: 'lo' });
  assert.deepStrictEqual(items, [{ kind: 'assistant', text: 'Hello', live: true }]);
  items = applyEvent(items, { type: 'assistant', text: 'Hello!', final: true });
  assert.deepStrictEqual(items, [{ kind: 'assistant', text: 'Hello!', live: false, final: true }]);
  items = applyEvent([...items, { kind: 'reasoning', text: 'r', live: true }], { type: 'completion_retry' });
  assert.deepStrictEqual(items.map((item) => item.kind), ['assistant']);
});

test('an error dismissed in the page is dismissed in the fetched transcript', async () => {
  const { dismissError } = await projection();
  const fetched = {
    messages: [
      { id: 1, role: 'user', content: 'Build' },
      { id: 2, role: 'error', content: 'Rate limited' },
      { id: 3, role: 'error', content: 'Out of credits' },
    ],
    messageTree: { headId: 3, messages: [] },
  };
  const known = dismissError(fetched, 2);
  assert.deepStrictEqual(known.messages.map((message) => Boolean(message.dismissed)), [false, true, false]);
  assert.equal(known.messageTree, fetched.messageTree);
  // The fetched copy itself is not changed: it may be on screen.
  assert.equal(fetched.messages[1].dismissed, undefined);
});
