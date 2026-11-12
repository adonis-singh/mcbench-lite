'use strict';

// Images go as the Codex CLI sends them: with detail "high", and a tool
// result's screenshots inside that result (below). In three chat turns that
// sent a new screenshot without detail, the request that added it read 0
// tokens from the cache, the cached prefix before it included; with detail
// "high" in the function output it read 6,912 of 8,349 (the screenshot cost
// the same input tokens either way). A user message that ends with an image
// (an upload) gets IMAGES_END after it, in every request, so the message is
// the same whether or not it is the last: of two identical requests ending
// with an image the second read 0 of 4,610 tokens from the cache, and with a
// text part after the image 4,224 of 4,615. A tool result's screenshots
// (`closed: false`) are followed by later items.
const IMAGES_END = { type: 'input_text', text: '[End of images]' };
function contentParts(parts, { closed }) {
  const content = parts.map((part) => {
    if (part.type === 'text') return { type: 'input_text', text: part.text };
    if (part.type === 'image_url') return { type: 'input_image', image_url: part.image_url.url, detail: 'high' };
    throw new Error(`Unsupported Codex content type: ${part.type}`);
  });
  return closed && parts.at(-1)?.type === 'image_url' ? [...content, IMAGES_END] : content;
}

// Use the OAuth bridge's native Responses route. Its Chat Completions adapter
// drops Astra reasoning settings and cannot forward embedded screenshots.
// A chat's requests carry its id where the Codex CLI puts its session id:
// the prompt_cache_key field, and the session-id and thread-id headers that
// codexSessionFetch adds. A one-shot (a title, a prompt rewrite) has no chat
// and no history to share.
function codexRequest({ model, messages, tools = [], effort, conversationId }) {
  const input = [];
  for (const message of messages) {
    if (message.role === 'system') continue;
    if (message.role === 'tool') {
      input.push({ type: 'function_call_output', call_id: message.tool_call_id, output: message.content });
      continue;
    }
    // Screenshots a tool result showed go inside that result, after its text.
    if (message.seeBuild) {
      const result = input.at(-1);
      if (result?.type !== 'function_call_output') throw new Error('Screenshots must follow a tool result');
      result.output = [...(typeof result.output === 'string' ? [{ type: 'input_text', text: result.output }] : result.output), ...contentParts(message.content, { closed: false })];
      continue;
    }
    if (message.role === 'assistant') {
      input.push(...(message.reasoning_details || []).filter((item) => item.type === 'reasoning'));
    }
    const content = typeof message.content === 'string' ? message.content : contentParts(message.content, { closed: true });
    if (content.length) input.push({ role: message.role, content });
    for (const call of message.tool_calls || []) {
      input.push({ type: 'function_call', call_id: call.id, name: call.function.name, arguments: call.function.arguments });
    }
  }
  return {
    model,
    instructions: messages.filter((message) => message.role === 'system').map((message) => message.content).join('\n\n'),
    input,
    tools: tools.map((tool) => ({ type: 'function', ...tool.function, strict: false })),
    stream: true,
    store: false,
    include: ['reasoning.encrypted_content'],
    ...(conversationId ? { prompt_cache_key: conversationId } : {}),
    ...(effort ? { reasoning: { effort, summary: 'auto' } } : {}),
  };
}

// Repeated formatting whitespace outside JSON strings carries no tool content.
// Recover a degenerate stream without bounding valid argument size, code inside
// strings, reasoning time, model turns, or tool calls.
function toolArgumentProgress() {
  let inString = false;
  let escaped = false;
  let whitespace = 0;
  return (chunk) => {
    for (const char of chunk || '') {
      if (inString) {
        whitespace = 0;
        if (escaped) escaped = false;
        else if (char === '\\') escaped = true;
        else if (char === '"') inString = false;
      } else if (char === '"') {
        inString = true;
        whitespace = 0;
      } else if (char === ' ' || char === '\n' || char === '\r' || char === '\t') {
        if (++whitespace > 16384) throw new Error('Codex tool argument stream stalled: repeated JSON whitespace without progress');
      } else {
        whitespace = 0;
      }
    }
  };
}

// The fetch the OAuth bridge sends its upstream requests with. The bridge
// passes on none of the headers of the request it is given, so the session
// headers go on here, from the prompt_cache_key the body carries. The backend
// reads the prompt cache only for requests with them (measured: of two
// identical requests, the second read 0 of 3,512 input tokens from the cache
// with the key alone and 3,328 with the headers).
function codexSessionFetch(url, init) {
  if (!url.endsWith('/responses')) return fetch(url, init);
  const key = JSON.parse(init.body).prompt_cache_key;
  if (key === undefined) return fetch(url, init);
  const headers = new Headers(init.headers);
  headers.set('session-id', key);
  headers.set('thread-id', key);
  return fetch(url, { ...init, headers });
}

async function codexCompletion(provider, options, request, readSse) {
  const response = await request(provider, codexRequest(options), options.signal);
  const calls = new Map();
  const progress = new Map();
  const output = [];
  let completed;
  let reasoningPart;
  await readSse(response, (event) => {
    switch (event.type) {
      case 'response.output_text.delta':
        options.onDelta?.({ content: event.delta });
        break;
      case 'response.reasoning_summary_text.delta': {
        const part = `${event.item_id}:${event.summary_index}`;
        const separator = reasoningPart !== undefined && part !== reasoningPart ? '\n\n' : '';
        options.onDelta?.({ reasoning: separator + event.delta });
        reasoningPart = part;
        break;
      }
      case 'response.output_item.added':
        if (event.item.type === 'function_call') {
          const call = { index: calls.size, id: event.item.call_id, name: event.item.name };
          calls.set(event.output_index, call);
          const guard = toolArgumentProgress();
          progress.set(event.output_index, guard);
          guard(event.item.arguments);
          options.onDelta?.({ toolCall: { ...call, args: event.item.arguments } });
        }
        break;
      case 'response.function_call_arguments.delta':
        progress.get(event.output_index)?.(event.delta);
        options.onDelta?.({ toolCall: { ...calls.get(event.output_index), args: event.delta } });
        break;
      case 'response.output_item.done':
        output.push(event.item);
        break;
      case 'response.completed':
        completed = event.response;
        break;
      case 'error':
      case 'response.failed':
        throw new Error(event.response?.error?.message || event.error?.message || event.message || 'Codex response failed');
      case 'response.incomplete':
        throw new Error(`Codex response incomplete: ${event.response.incomplete_details.reason}`);
    }
  });
  if (!completed) throw new Error('Codex stream ended before response.completed');
  const message = { role: 'assistant', content: '', finish_reason: 'stop', ...(completed.usage ? { usage: completed.usage } : {}) };
  // Codex sends complete items here and leaves response.completed.output empty.
  for (const item of output) {
    if (item.type === 'message') {
      for (const part of item.content) {
        if (part.type === 'output_text') message.content += part.text;
        if (part.type === 'refusal') message.content += part.refusal;
      }
    } else if (item.type === 'function_call') {
      (message.tool_calls ||= []).push({
        id: item.call_id, type: 'function', function: { name: item.name, arguments: item.arguments },
      });
      message.finish_reason = 'tool_calls';
    } else if (item.type === 'reasoning') {
      (message.reasoning_details ||= []).push(item);
    }
  }
  if (message.reasoning_details) {
    message.reasoning = message.reasoning_details.flatMap((item) => item.summary.map((part) => part.text)).join('\n\n');
  }
  return message;
}

module.exports = { codexRequest, codexSessionFetch, codexCompletion, toolArgumentProgress };
