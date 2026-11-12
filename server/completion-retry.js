'use strict';
const {setTimeout: delay} = require('node:timers/promises');

function transientCompletionError(error) {
  const code = error?.cause?.code || error?.code || '';
  return /^(ECONNRESET|ECONNREFUSED|EPIPE|ETIMEDOUT|UND_ERR_SOCKET|UND_ERR_CONNECT_TIMEOUT|UND_ERR_HEADERS_TIMEOUT|UND_ERR_BODY_TIMEOUT)$/.test(code)
    || /^(terminated|fetch failed|socket hang up)$/i.test(error?.message || '')
    || /HTTP (429|500|502|503|504|529)\b|servers are currently overloaded|overloaded_error|Overloaded|Codex stream ended before response.completed|Codex tool argument stream stalled|Anthropic stream ended before message_stop|Model stream ended without a finish reason/.test(error?.message || '');
}

// A completion has no tool side effects: the caller executes tools only after
// receiving its complete message. Retry this request, never the whole turn.
// Keep the sandbox and native runtime alive while a connection recovers. A
// provider still failing after MAX_ATTEMPTS requests (about 90 s of
// backoff) fails the turn with its last error.
const MAX_ATTEMPTS = 8;
async function completionWithRetry(complete, options, {onRetry = () => {}, wait = delay} = {}) {
  for (let retry = 0; ; retry++) {
    options.signal?.throwIfAborted();
    try { return await complete(options); }
    catch (error) {
      if (options.signal?.aborted || !transientCompletionError(error) || retry + 1 === MAX_ATTEMPTS) throw error;
      const delayMs = Math.min(30000, 1000 * 2 ** Math.min(retry, 5));
      onRetry({attempt: retry + 1, delayMs, error: error.message});
      await wait(delayMs, undefined, {signal: options.signal});
    }
  }
}
module.exports = {completionWithRetry, transientCompletionError, MAX_ATTEMPTS};
