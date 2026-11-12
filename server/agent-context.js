'use strict';

const crypto = require('crypto');
const sandbox = require('./sandbox');

const MAX_CONTEXT_BYTES = Number(process.env.MCBENCH_CONTEXT_MAX_BYTES || 512_000);
if (!Number.isSafeInteger(MAX_CONTEXT_BYTES) || MAX_CONTEXT_BYTES < 16_000) {
  throw new Error('MCBENCH_CONTEXT_MAX_BYTES must be an integer of at least 16000');
}

// What a message costs in the budget: its JSON, and for each image about the
// text a provider charges for one screenshot.
const IMAGE_BYTES = 6_000;
function messageBytes(message) {
  const images = Array.isArray(message.content)
    ? message.content.filter(part => part.type === 'image_url' || (part.type === 'attachment' && part.attachment.type.startsWith('image/'))).length
    : 0;
  return Buffer.byteLength(JSON.stringify(message)) + images * IMAGE_BYTES;
}

// Complete tool exchanges, oldest first: the call, its results, and the
// screenshots they showed. Each knows the stub that archives it.
function toolExchanges(conversation, messages) {
  const exchanges = [];
  for (let i = 0; i < messages.length; i++) {
    const first = messages[i];
    if (!first.tool_calls?.length) continue;
    const ids = new Set(first.tool_calls.map(call => call.id));
    let end = i + 1;
    while (end < messages.length && messages[end].role === 'tool' && ids.has(messages[end].tool_call_id)) end++;
    if (end - i - 1 !== ids.size) continue;
    while (end < messages.length && messages[end].seeBuild) end++;
    const original = messages.slice(i, end);
    const text = JSON.stringify(original);
    const file = `context/${crypto.createHash('sha256').update(text).digest('hex').slice(0, 24)}.json`;
    const outcomes = original.filter(m => m.role === 'tool').map(m => {
      const call = first.tool_calls.find(c => c.id === m.tool_call_id);
      let result;
      try { result = JSON.parse(m.content); } catch { /* plain terminal output */ }
      return { tool: call.function.name, ...(result ? {
        ok: result.ok, worldRevision: result.worldRevision, report: result.report, reports: result.tests?.map(test => test.report),
        error: typeof result.error === 'string' ? result.error.slice(0, 400) : undefined,
      } : {}) };
    });
    const replacement = { role: 'assistant', content:
      `${typeof first.content === 'string' ? first.content : ''}\n[Earlier tool exchange archived at /work/${file}. Outcomes: ${JSON.stringify(outcomes)}. Read that file with bash if the original program, observations, or output is needed.]` };
    const bytes = original.reduce((sum, m) => sum + messageBytes(m), 0);
    // An already compact exchange is kept as it is.
    const saves = Math.max(0, bytes - messageBytes(replacement));
    exchanges.push({ start: i, end, text, file, replacement, saves });
    i = end - 1;
  }
  return exchanges;
}

// The history a request sends. Over the budget, the oldest complete tool
// exchanges are archived to /work (the UI and database keep the original)
// until half the budget is left, never user instructions, the newest
// `keepExchanges` exchanges, or half of a call and its results. What is sent
// only ever grows between two archivings: each request in the history is
// replayed (one before every assistant message, and this one), so an
// exchange archived once is archived at every later request. Signed thinking
// is bound to everything before it: the thinking of turns kept across an
// archiving goes, and the thinking since stays valid.
function contextMessages(conversation, { maxBytes = MAX_CONTEXT_BYTES, keepExchanges = 6 } = {}) {
  const messages = conversation.messages.filter(m => m.role !== 'error');
  const sizes = messages.map(messageBytes);
  const exchanges = toolExchanges(conversation, messages);
  const requests = [...messages.flatMap((m, i) => (m.role === 'assistant' ? [i] : [])), messages.length];
  let sent = 0;
  let next = 0;
  let saved = 0;
  let archived = 0;
  let cut = 0;
  for (const at of requests) {
    while (next < at) sent += sizes[next++];
    if (sent - saved <= maxBytes) continue;
    const complete = exchanges.filter(exchange => exchange.end <= at).length;
    while (archived < complete - keepExchanges && sent - saved > maxBytes / 2) saved += exchanges[archived++].saves;
    cut = at;
  }
  if (sent - saved > maxBytes) {
    throw new Error(`Protected user messages and recent tool exchanges exceed MCBENCH_CONTEXT_MAX_BYTES (${maxBytes}). Increase that setting or continue in a new chat; history was not silently discarded.`);
  }
  const output = [];
  let index = 0;
  for (const exchange of exchanges.slice(0, archived).filter(e => e.saves > 0)) {
    output.push(...messages.slice(index, exchange.start));
    sandbox.writeWorkFile(conversation.id, exchange.file, exchange.text);
    output.push(exchange.replacement);
    index = exchange.end;
  }
  output.push(...messages.slice(index));
  const kept = new Set(messages.slice(0, cut));
  return output.map((message) => {
    if (!message.reasoning_details || !kept.has(message)) return message;
    const { reasoning_details, ...rest } = message;
    return rest;
  });
}

module.exports = { contextMessages };
