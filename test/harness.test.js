'use strict';

const { test, after } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');

const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-harness-test-'));
process.env.MCBENCH_DATA_DIR = dataDir;
const harness = require('../server/harness');

after(() => fs.rmSync(dataDir, { recursive: true, force: true }));

const PNG = 'data:image/png;base64,iVBORw0KGgo=';
const transcript = [
  { role: 'system', content: 'You build in Minecraft.' },
  { role: 'user', content: 'Build a hut.' },
  { role: 'assistant', content: 'Starting.', tool_calls: [{ id: 'call-1', type: 'function', function: { name: 'build', arguments: '{"summary":"hut"}' } }] },
  { role: 'tool', tool_call_id: 'call-1', content: '{"ok":true}' },
  { role: 'user', content: [{ type: 'text', text: '[see_build] Screenshots of the current build:' }, { type: 'image_url', image_url: { url: PNG } }] },
  { role: 'assistant', content: 'Done.' },
  { role: 'user', content: [{ type: 'text', text: 'Make the roof dark oak.' }, { type: 'image_url', image_url: { url: PNG } }] },
];

test('a fresh harness session gets the earlier transcript and the request; a resumed one only the request', () => {
  const fresh = harness.harnessPrompt(transcript, { history: true, notes: [] });
  assert.match(fresh.text, /^<conversation>\nUser:\nBuild a hut\./);
  assert.match(fresh.text, /Called build \(call-1\) with \{"summary":"hut"\}/);
  assert.match(fresh.text, /\[Image 1\][\s\S]*<\/conversation>\n\nMake the roof dark oak\.\n\[Image 2\]$/);
  assert.equal(fresh.images.length, 2);
  assert.doesNotMatch(fresh.text, /<instructions>/);
  const resumed = harness.harnessPrompt(transcript, { history: false, notes: [] });
  assert.equal(resumed.text, 'Make the roof dark oak.\n[Image 1]');
  // What a resumed session lacks goes before the request.
  assert.equal(harness.harnessPrompt(transcript, { history: false, notes: ['<note>\nA\n</note>'] }).text, '<note>\nA\n</note>\n\nMake the roof dark oak.\n[Image 1]');
  assert.deepEqual(resumed.images, [{ mimeType: 'image/png', data: 'iVBORw0KGgo=' }]);
  assert.equal(harness.systemText(transcript), 'You build in Minecraft.');
});

test('a stored session continues while everything after its latest request belongs to that turn', () => {
  const user = (id) => ({ id, role: 'user', content: 'x' });
  const assistant = (id) => ({ id, role: 'assistant', content: 'y' });
  const tool = (id) => ({ id, role: 'tool', content: '{}' });
  const screenshots = (id) => ({ id, role: 'user', seeBuild: true, content: [] });
  const record = { sessionId: 's', lastSeen: 'u1' };
  assert.equal(harness.sessionContinues(null, [user('u2')]), false);
  // The previous turn ended with its reply.
  assert.equal(harness.sessionContinues(record, [user('u1'), assistant('a1'), tool('t1'), screenshots('s1'), assistant('a2'), user('u2')]), true);
  // It was stopped or failed partway: the session holds it as far as it got.
  assert.equal(harness.sessionContinues(record, [user('u1'), assistant('a1'), tool('t1'), { id: 'a2', role: 'assistant', content: '(stopped by user)', stopped: true }, user('u2')]), true);
  assert.equal(harness.sessionContinues(record, [user('u1'), assistant('a1'), tool('t1'), { id: 'e1', role: 'error', content: 'Claude Code exited' }, user('u2')]), true);
  // Reverted past the request the session took.
  assert.equal(harness.sessionContinues(record, [user('u0'), user('u2')]), false);
  // A turn in between that the session never took (another provider's).
  assert.equal(harness.sessionContinues(record, [user('u1'), assistant('a1'), user('u9'), assistant('a9'), user('u2')]), false);
  // Mid-turn with no live session: the last message is screenshots, not a request.
  assert.equal(harness.sessionContinues(record, [user('u1'), tool('t1'), screenshots('s1')]), false);
});

test('a resumed session is given the reply the user stopped in its latest turn, and a resolution that changed since it was told one', () => {
  const { systemPrompt } = require('../server/prompt');
  const at = (resolution) => [{ role: 'system', content: systemPrompt({ resolution, networkAccess: true }) }, { role: 'user', content: 'Go on.' }];
  const standard = harness.sessionTerms(at(1));
  const large = harness.sessionTerms(at(5));
  // The resolution section is apart from the rest of the instructions.
  assert.equal(standard.instructions, large.instructions);
  assert.notEqual(standard.resolution, large.resolution);
  assert.deepEqual(harness.resolutionNote({ ...standard }, at(1)), []);
  const [toLarge] = harness.resolutionNote({ ...standard }, at(5));
  assert.match(toLarge, /^<resolution-change>\nThe chat's resolution has changed\. From this request on, this section replaces the Resolution section of your instructions and any earlier resolution change:\n\n## Resolution\nThis chat's resolution is /);
  assert.ok(toLarge.endsWith('\n</resolution-change>'));
  const [toStandard] = harness.resolutionNote({ ...large }, at(1));
  assert.match(toStandard, /changed to Standard\. From this request on, the Resolution section of your instructions and any earlier resolution change no longer apply/);

  const record = { sessionId: 's', lastSeen: 'u1' };
  const stopped = [{ id: 'u0', role: 'user', content: 'x' }, { id: 'a0', role: 'assistant', content: '(stopped by user)', stopped: true, reply: 'Old turn' },
    { id: 'u1', role: 'user', content: 'Plan a hut.' }, { id: 'a1', role: 'assistant', content: '(stopped by user)', stopped: true, reply: 'First the floor, then' }, { id: 'u2', role: 'user', content: 'continue' }];
  assert.deepEqual(harness.stoppedReplyNote(record, stopped), ['<stopped-reply>\nThe user stopped your previous reply while you were writing it, and this session does not hold it. This is the text you had written, as the user saw it:\n\nAssistant:\nFirst the floor, then\n</stopped-reply>']);
  // A stop with nothing streamed (during a tool), and a turn that ended, give nothing.
  assert.deepEqual(harness.stoppedReplyNote(record, [...stopped.slice(0, 3), { id: 'a1', role: 'assistant', content: '(stopped by user)', stopped: true }, stopped[4]]), []);
  assert.deepEqual(harness.stoppedReplyNote(record, [...stopped.slice(0, 3), { id: 'a1', role: 'assistant', content: 'Done.' }, stopped[4]]), []);
});

test('session records live under the data directory per conversation and harness', () => {
  assert.equal(harness.readSession('conv', 'claude-code'), null);
  harness.writeSession('conv', 'claude-code', { sessionId: 'abc', lastSeen: 'm1' });
  assert.deepEqual(harness.readSession('conv', 'claude-code'), { sessionId: 'abc', lastSeen: 'm1' });
  assert.equal(path.dirname(harness.conversationDir('conv')), path.join(dataDir, 'harness'));
  harness.destroy('conv');
  assert.equal(harness.readSession('conv', 'claude-code'), null);
});

test('the relay surfaces harness tool calls one at a time and rejects the rest when the turn ends', async () => {
  const session = new harness.RelaySession('Test');
  const deltas = [];
  const first = session.next((delta) => deltas.push(delta));
  session.update({ text: 'Placing.' });
  const calls = [session.call('c1', 'build', { summary: 'a' }), session.call('c2', 'bash', { command: 'ls' })];
  const message = await first;
  assert.equal(message.content, 'Placing.');
  assert.deepEqual(message.tool_calls, [{ id: 'c1', type: 'function', function: { name: 'build', arguments: '{"summary":"a"}' } }]);
  assert.deepEqual(deltas.map((d) => Object.keys(d)[0]), ['content', 'toolCall']);
  session.deliver([{ type: 'text', text: 'ok' }]);
  assert.deepEqual(await calls[0], [{ type: 'text', text: 'ok' }]);
  const second = await session.next();
  assert.equal(second.tool_calls[0].id, 'c2');
  session.deliver([{ type: 'text', text: 'listing' }]);
  await calls[1];
  const last = session.next();
  session.update({ text: 'Finished.', reasoning: 'checked', usage: { input_tokens: 3, output_tokens: 1 } });
  session.finish(null);
  const final = await last;
  assert.deepEqual(final, { role: 'assistant', content: 'Finished.', reasoning: 'checked', finish_reason: 'stop', usage: { input_tokens: 3, output_tokens: 1 } });
  assert.equal(session.state, 'closed');

  const stopped = new harness.RelaySession('Test');
  const waiting = stopped.call('c3', 'build', {});
  stopped.close();
  await assert.rejects(waiting, /turn ended before this tool ran/);
  await assert.rejects(stopped.next(), /Test run closed/);
});

test('streamed tool arguments reach the UI as written, even while another call runs, and are not resent whole', async () => {
  const session = new harness.RelaySession('Test');
  const firstDeltas = [];
  const first = session.next((delta) => firstDeltas.push(delta));
  session.draftToolCall('t1', 1, 'build');
  session.draftArgs('t1', '{"summary":');
  session.draftArgs('t1', '"a"}');
  session.draftToolCall('t2', 2, 'build');
  session.draftArgs('t2', '{"summary":');
  const calls = [session.call('t1', 'build', { summary: 'a' })];
  await first;
  assert.deepEqual(firstDeltas.map((d) => d.toolCall), [
    { index: 1, id: 't1', name: 'build', args: '' },
    { index: 1, id: 't1', name: 'build', args: '{"summary":' },
    { index: 1, id: 't1', name: 'build', args: '"a"}' },
    { index: 2, id: 't2', name: 'build', args: '' },
    { index: 2, id: 't2', name: 'build', args: '{"summary":' },
  ]);
  // t2 keeps writing while t1 runs and no completion is waiting.
  session.draftArgs('t2', '"b"}');
  calls.push(session.call('t2', 'build', { summary: 'b' }));
  session.deliver([{ type: 'text', text: 'ok' }]);
  const secondDeltas = [];
  const second = await session.next((delta) => secondDeltas.push(delta));
  assert.equal(second.tool_calls[0].id, 't2');
  assert.deepEqual(secondDeltas.map((d) => d.toolCall), [{ index: 2, id: 't2', name: 'build', args: '"b"}' }]);
  session.close();
  await assert.rejects(calls[1], /turn ended before this tool ran/);
});

test('runJsonLines parses stdout lines, stops the process when a line is refused, and reports the exit', async () => {
  const script = path.join(dataDir, 'emit.js');
  fs.writeFileSync(script, `
    let input = ''; process.stdin.on('data', d => { input += d; }).on('end', () => {
      console.log(JSON.stringify({ type: 'echo', input }));
      console.log(JSON.stringify({ type: 'done' }));
      console.error('warned');
      process.exit(process.env.EXIT_CODE ? Number(process.env.EXIT_CODE) : 0);
    });`);
  const seen = [];
  const run = harness.runJsonLines(process.execPath, [script], { env: { ...process.env, EXIT_CODE: '3' }, input: 'hello', onLine: (event) => seen.push(event) });
  const exit = await run.done;
  assert.deepEqual(seen, [{ type: 'echo', input: 'hello' }, { type: 'done' }]);
  assert.equal(exit.code, 3);
  assert.match(exit.stderr, /warned/);

  const hang = path.join(dataDir, 'hang.js');
  fs.writeFileSync(hang, "console.log(JSON.stringify({ type: 'bad' })); setInterval(() => {}, 1000);");
  const refused = harness.runJsonLines(process.execPath, [hang], { env: process.env, input: '', onLine: (event) => { if (event.type === 'bad') throw new Error('refused'); } });
  await assert.rejects(refused.done, /refused/);
});

test('a relay session cannot start on an aborted signal, and a closed one refuses calls and drops harness events', async () => {
  const aborted = new AbortController();
  aborted.abort(new Error('stopped first'));
  assert.throws(() => new harness.RelaySession('Test', aborted.signal), /stopped first/);

  const session = new harness.RelaySession('Test');
  session.draftToolCall('t1', 1, 'build');
  session.close();
  assert.equal(session.state, 'closed');
  // A call after close settles at once instead of waiting forever.
  await assert.rejects(session.call('c1', 'build', {}), /turn ended before this tool ran/);
  // Late output from a stopped harness is dropped, not applied to cleared drafts.
  session.draftArgs('t1', '{"summary":');
  session.update({ text: 'late' });
  assert.equal(session.text, '');
  assert.throws(() => session.deliver([]), /Test run closed/);
});

test('a harness that dies while its tool runs fails the delivery at once, and its unanswered calls never run', async () => {
  const session = new harness.RelaySession('Test');
  const first = session.call('c1', 'build', {});
  assert.equal((await session.next()).tool_calls[0].id, 'c1');
  const queued = session.call('c2', 'build', {});
  session.finish(new Error('Test exited 1'));
  assert.equal(session.state, 'ended');
  await assert.rejects(first, /turn ended before this tool ran/);
  await assert.rejects(queued, /turn ended before this tool ran/);
  assert.throws(() => session.deliver([{ type: 'text', text: 'ok' }]), /Test exited 1/);
  assert.equal(session.state, 'closed');
});

test('a session record is saved only for a turn that completed normally', async () => {
  let saves = 0;
  const save = () => { saves++; };
  const done = new harness.RelaySession('Test');
  done.finish(null, save);
  assert.equal((await done.next()).finish_reason, 'stop');
  assert.equal(saves, 1);

  // Stopped, then the harness exits cleanly: nothing is saved.
  const stopped = new harness.RelaySession('Test');
  stopped.close();
  stopped.finish(null, save);
  assert.equal(saves, 1);

  // A clean exit with a tool call left unanswered fails instead.
  const cut = new harness.RelaySession('Test');
  cut.call('c1', 'build', {}).catch(() => {});
  cut.finish(null, save);
  await assert.rejects(cut.next(), /ended with a tool call unanswered/);
  assert.equal(saves, 1);

  // A save that throws fails the turn.
  const broken = new harness.RelaySession('Test');
  broken.finish(null, () => { throw new Error('disk full'); });
  await assert.rejects(broken.next(), /disk full/);
});

test('a tool call cancelled by its MCP request never surfaces, and a running one drops its result', async () => {
  const session = new harness.RelaySession('Test');
  const running = new AbortController();
  const first = session.call('c1', 'build', {}, running.signal);
  assert.equal((await session.next()).tool_calls[0].id, 'c1');
  const queued = new AbortController();
  const second = session.call('c2', 'build', {}, queued.signal);
  queued.abort(new Error('Cancelled by harness'));
  await assert.rejects(second, /Cancelled by harness/);
  running.abort(new Error('MCP client disconnected'));
  await assert.rejects(first, /MCP client disconnected/);
  session.deliver([{ type: 'text', text: 'ignored' }]);
  const next = session.next();
  session.finish(null);
  assert.equal((await next).finish_reason, 'stop');
});

test('a run that fails to start is closed and forgotten', async () => {
  class Failing extends harness.RelaySession {
    constructor(signal) { super('Failing', signal); }
    async start() { throw new Error('no binary'); }
    stop() { this.stopped = true; }
  }
  let made;
  const relay = harness.sessionRelay(class extends Failing { constructor(signal) { super(signal); made = this; } });
  await assert.rejects(relay.completion({}, { conversationId: 'c', messages: [] }), /no binary/);
  assert.equal(made.state, 'closed');
  assert.equal(made.stopped, true);
  const first = made;
  await assert.rejects(relay.completion({}, { conversationId: 'c', messages: [] }), /no binary/);
  assert.notEqual(made, first);
});

test('destroy kills the conversation\'s harness processes and shutdownHarnesses kills them all', async () => {
  const killed = [];
  const fake = (conversationId) => {
    const proc = { conversationId, kill: async () => { killed.push(conversationId); harness.untrack(proc); } };
    harness.track(proc);
    return proc;
  };
  fake('a'); fake('b'); fake('b');
  harness.destroy('b');
  assert.deepEqual(killed, ['b', 'b']);
  await harness.shutdownHarnesses();
  assert.deepEqual(killed, ['b', 'b', 'a']);
});

test('runJsonLines stop does nothing once the process has exited', async () => {
  const script = path.join(dataDir, 'quick.js');
  fs.writeFileSync(script, "console.log(JSON.stringify({ type: 'done' }));");
  let stops = 0;
  const run = harness.runJsonLines(process.execPath, [script], { env: process.env, input: '', onLine: () => {}, onStop: () => { stops++; } });
  assert.equal((await run.done).code, 0);
  run.stop();
  assert.equal(stops, 0);
});

test('a run whose stop throws is still released, forgotten and its waiter answered, and the error surfaces', async () => {
  let made;
  let forgotten = false;
  const relay = harness.sessionRelay(class extends harness.RelaySession {
    constructor(signal) { super('Stuck', signal); made = this; }
    async start() {}
    stop() { throw new Error('stop failed'); }
    release() { this.released = true; }
  });
  const waiting = relay.completion({}, { conversationId: 'stuck', messages: [] });
  waiting.catch(() => {});
  await new Promise(setImmediate);
  const forget = made.onClose;
  made.onClose = () => { forgotten = true; forget(); };
  assert.throws(() => relay.closeSession('stuck'), /stop failed/);
  assert.equal(made.state, 'closed');
  assert.equal(made.released, true);
  assert.equal(forgotten, true);
  await assert.rejects(waiting, /Stuck run closed/);
});

test('a session record whose write is cut short leaves no record that fails to parse', () => {
  const write = fs.writeFileSync;
  // The disk fills partway through the write.
  fs.writeFileSync = (file, data, ...rest) => { write(file, String(data).slice(0, 12), ...rest); throw new Error('ENOSPC'); };
  try {
    assert.throws(() => harness.writeSession('conv-cut', 'claude-code', { sessionId: 'abc', lastSeen: 'm1' }), /ENOSPC/);
  } finally {
    fs.writeFileSync = write;
  }
  assert.equal(harness.readSession('conv-cut', 'claude-code'), null);
  harness.writeSession('conv-cut', 'claude-code', { sessionId: 'abc', lastSeen: 'm2' });
  assert.deepEqual(harness.readSession('conv-cut', 'claude-code'), { sessionId: 'abc', lastSeen: 'm2' });
});
