'use strict';

// What the differential test compares of a completion's result: the message
// as agent.js runTurn stores it (server/agent.js 662-673, in that key order,
// with `role` first and every other key a provider returned refused), and the
// telemetry with its three clock fields fixed, since the two runs take
// different times.

const STORED = ['content', 'reasoning', 'reasoning_details', 'finish_reason', 'telemetry', 'notices', 'plan', 'rateLimit', 'tool_calls'];

function normalizeTelemetry(telemetry) {
  return { ...telemetry, startedAt: '<startedAt>', wallMs: 0, firstDeltaMs: telemetry.firstDeltaMs === null ? null : 0 };
}

function storedMessage(message) {
  const extra = Object.keys(message).filter((key) => key !== 'role' && !STORED.includes(key));
  if (extra.length) throw new Error(`the message has keys agent.js does not store: ${extra.join(', ')}`);
  const toolCalls = message.tool_calls || [];
  return {
    role: message.role,
    content: message.content || '',
    ...(message.reasoning ? { reasoning: message.reasoning } : {}),
    ...(message.reasoning_details ? { reasoning_details: message.reasoning_details } : {}),
    ...(message.finish_reason ? { finish_reason: message.finish_reason } : {}),
    ...(message.telemetry ? { telemetry: normalizeTelemetry(message.telemetry) } : {}),
    ...(message.notices ? { notices: message.notices } : {}),
    ...(message.plan ? { plan: message.plan } : {}),
    ...(message.rateLimit ? { rateLimit: message.rateLimit } : {}),
    ...(toolCalls.length ? { tool_calls: toolCalls } : {}),
  };
}

module.exports = { normalizeTelemetry, storedMessage };
