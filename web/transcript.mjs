import { mergeToolCallDelta } from './tool-stream.mjs';

// The chat turn on screen, from this page's side. Whether the server runs a
// turn is its list row's `runningSince`; this is what the page does about it:
//   idle     no stream held. `ended` is a turn start the page saw end, so a
//            list that still shows it running is stale.
//   sending  a message on its way (the chat being created, or the POST before
//            the server takes it), with the controller that aborts it.
//   reading  the page reads the turn's events; `status` is the harness's
//            activity line ("Retrying (attempt 2 of 10)").
//   waiting  the stream broke mid-turn; the page joins again shortly.
//   detached the server runs the turn (of chat `id`, begun at `startedAt`)
//            and the page has stopped reading it: an event it could not
//            apply, or a stream it was refused. The chat stays busy until
//            the list no longer shows that turn running.
export const IDLE = Object.freeze({ phase: 'idle', ended: null });

// The phases each phase can go to. Leaving the chat goes to idle from any;
// a status line is a reading turn going on reading.
const NEXT = {
  idle: ['idle', 'sending', 'reading'],
  sending: ['reading', 'idle'],
  reading: ['reading', 'waiting', 'detached', 'idle'],
  waiting: ['idle'],
  detached: ['idle'],
};

// The turn after `turn`: `next`, if it can follow it.
export function turnTo(turn, next) {
  if (!NEXT[turn.phase].includes(next.phase)) throw new Error(`The turn cannot go from ${turn.phase} to ${next.phase}`);
  if ((next.phase === 'sending' || next.phase === 'reading') !== Boolean(next.controller)) {
    throw new Error('A turn being sent or read, and only one, holds a stream');
  }
  return next;
}

export function turnRunning(turn, row) {
  return turn.phase !== 'idle' || Boolean(row?.runningSince && row.runningSince !== turn.ended);
}

// A detached turn is over once the list stops showing it running.
export function turnOver(turn, row) {
  return turn.phase === 'detached' && !(row?.runningSince && row.runningSince === turn.startedAt);
}

// The start of a turn the server runs that this page should join, or null.
export function turnToJoin(turn, row) {
  return turn.phase === 'idle' && row?.runningSince && row.runningSince !== turn.ended ? row.runningSince : null;
}

// The transcript of the place on screen (a chat, a new chat, a bench view):
//   loading  not on screen yet, so nothing claims the chat is empty
//   shown    the items on screen are the place's (none, for a new chat)
//   failed   reading it failed (`error` says why): what is on screen may be
//            old, and it can be read again
// Any of them can follow any other; only a failed read has an error.
export const LOADING = Object.freeze({ phase: 'loading', error: null });
export const SHOWN = Object.freeze({ phase: 'shown', error: null });
export const readFailed = (error) => ({ phase: 'failed', error: String(error.message || error) });

// A fetched transcript after one of its stored errors was dismissed in the
// page: what the page knows of the chat until it is read again.
export function dismissError(transcript, messageId) {
  return {
    ...transcript,
    messages: transcript.messages.map((message) => (message.id === messageId ? { ...message, dismissed: true } : message)),
  };
}

// A turn's live events, applied to the transcript's items. Deltas stream into
// a trailing live item of their kind; the full event seals it. A tool call's
// deltas, start and result name it by id; a start or result that matches no
// call an earlier event made is a bug, and throws.
export function applyEvent(items, event) {
  switch (event.type) {
    case 'completion_retry':
      // Drop only the interrupted response's uncommitted stream. Completed
      // tool results and messages remain; no tool is executed by a retry.
      return items.filter((item) => !item.live && !(item.kind === 'tool' && item.status === 'writing'));
    case 'assistant_delta':
      return appendDelta(items, 'assistant', event.text);
    case 'reasoning_delta':
      return appendDelta(items, 'reasoning', event.text);
    case 'assistant':
      return finalize(items, 'assistant', event.text, { final: event.final });
    case 'reasoning':
      return finalize(items, 'reasoning', event.text);
    case 'tool_call_delta':
      return toolCallDelta(items, event);
    case 'tool_start': {
      const index = items.findIndex((item) => item.kind === 'tool' && item.status === 'writing' && item.callId === event.callId);
      if (index < 0) throw new Error(`Tool call ${event.callId} started without being written`);
      return replace(items, index, {
        ...items[index], status: 'running', summary: event.summary, tool: event.name, program: event.program ?? items[index].program,
      });
    }
    case 'tool_progress':
      return items.map((item) => (item.kind === 'tool' && item.status === 'running' && item.callId === event.callId
        ? { ...item, progress: event.message } : item));
    case 'tool_result': {
      const index = items.findIndex((item) => item.kind === 'tool' && item.status === 'running' && item.callId === event.callId);
      if (index < 0) throw new Error(`Tool call ${event.callId} finished without running`);
      const item = items[index];
      const next = replace(items, index, {
        ...item,
        status: event.cancelled ? 'cancelled' : event.ok ? 'ok' : 'failed',
        diff: event.ok ? event.diff : undefined,
        output: typeof event.output === 'string' ? event.output : item.output,
        detail: event.ok ? '' : (event.error || (event.output ? '' : 'failed')),
      });
      return event.images?.length ? [...next, { kind: 'shots', images: event.images }] : next;
    }
    case 'error':
      return [...items, { kind: 'error', text: event.message, messageId: event.messageId }];
    case 'notice':
      return [...items, { kind: 'notice', level: event.level, text: event.text }];
    case 'plan': {
      // One live task list per turn, updated in place.
      const lastUser = items.map((item) => item.kind).lastIndexOf('user');
      const index = items.findIndex((item, i) => i > lastUser && item.kind === 'plan');
      return index < 0 ? [...items, { kind: 'plan', items: event.items }] : replace(items, index, { kind: 'plan', items: event.items });
    }
    default:
      throw new Error(`Unknown turn event: ${event.type}`);
  }
}

function replace(items, index, item) {
  const next = [...items];
  next[index] = item;
  return next;
}

function appendDelta(items, kind, text) {
  const last = items[items.length - 1];
  if (last?.kind === kind && last.live) return replace(items, items.length - 1, { ...last, text: last.text + text });
  return [...items, { kind, text, live: true }];
}

function finalize(items, kind, text, fields = {}) {
  for (let i = items.length - 1; i >= 0; i -= 1) {
    if (items[i].kind === kind && items[i].live) return replace(items, i, { ...items[i], ...fields, text, live: false });
  }
  return text ? [...items, { kind, text, ...fields }] : items;
}

// Argument text of a call being written, by the call's id, which every
// delta carries. A call's text can arrive in parts with other events between
// them: a harness streams the next call while one runs.
function toolCallDelta(items, event) {
  if (!event.id) throw new Error(`Tool call ${event.index} streamed without its id`);
  const index = items.findIndex((item) => item.kind === 'tool' && item.status === 'writing' && item.callId === event.id);
  if (index >= 0) return replace(items, index, mergeToolCallDelta(items[index], event));
  return [...items, mergeToolCallDelta({
    kind: 'tool', callId: event.id, tool: event.name || 'build', summary: '', status: 'writing', argsRaw: '', detail: '',
  }, event)];
}

const ACTIVITY_KINDS = new Set(['reasoning', 'tool', 'shots', 'assistant', 'notice', 'plan']);

function liveEntries(children) {
  const entries = [];
  let calls = null;

  const flushCalls = () => {
    if (!calls) return;
    entries.push({ kind: 'tool-stack', calls });
    calls = null;
  };

  for (const item of children) {
    if (item.kind === 'tool') {
      if (!calls) calls = [];
      calls.push({ tool: item, outputs: [] });
      continue;
    }
    if (item.kind === 'shots' && calls?.length) {
      calls[calls.length - 1].outputs.push(item);
      continue;
    }
    flushCalls();
    entries.push(item);
  }
  flushCalls();
  return entries;
}

function settledEntries(children) {
  const finalIndex = children.findLastIndex((item) => item.kind === 'assistant' && item.final === true);
  const final = finalIndex >= 0 ? children[finalIndex] : null;
  const work = finalIndex >= 0
    ? children.filter((_, index) => index !== finalIndex)
    : children;
  // The task list and any warnings stay visible after the turn settles; the
  // rest of the work folds away.
  const surfaced = (item) => item.kind === 'plan' || (item.kind === 'notice' && item.level !== 'info');
  const activity = work.filter((item) => !surfaced(item));
  const entries = [];
  if (activity.length) entries.push({ kind: 'activity', children: activity });
  entries.push(...work.filter(surfaced));
  if (final) entries.push(final);
  return entries;
}

// A turn's task list is reported as snapshots; only the latest one shows.
function latestPlanOnly(children) {
  const last = children.map((item) => item.kind).lastIndexOf('plan');
  return children.filter((item, index) => item.kind !== 'plan' || index === last);
}

// Turns have two deliberately different projections. While the current turn
// runs, spoken assistant text stays visible, reasoning gets its own fold, and
// uninterrupted tool calls compact into a latest-call stack. Once settled,
// all work except the explicit final response folds into one activity entry.
// Work that already has its final response is settled, so a turn starting
// before its prompt shows leaves the previous turn's fold as it is.
export function groupItems(items, busy) {
  const grouped = [];
  let activity = null;

  for (const item of items) {
    if (ACTIVITY_KINDS.has(item.kind)) {
      if (!activity) {
        activity = { kind: 'raw-activity', children: [] };
        grouped.push(activity);
      }
      activity.children.push(item);
      continue;
    }
    activity = null;
    grouped.push(item);
  }

  return grouped.flatMap((entry, index) => {
    if (entry.kind !== 'raw-activity') return [entry];
    const children = latestPlanOnly(entry.children);
    const live = busy && index === grouped.length - 1 && !children.some((item) => item.kind === 'assistant' && item.final === true);
    return live ? liveEntries(children) : settledEntries(children);
  });
}

// A key per entry, stable while the entries around it change, so a fold
// keeps its open state: a turn's entries count from its prompt, and a turn's
// settled fold is not the live entry that stood in its place.
export function entryKeys(entries) {
  let turn = 0;
  let ordinal = 0;
  return entries.map((entry) => {
    if (entry.kind === 'user') {
      turn += 1;
      ordinal = 0;
      return `${turn}:user`;
    }
    ordinal += 1;
    return `${turn}:${ordinal}:${entry.kind}`;
  });
}
