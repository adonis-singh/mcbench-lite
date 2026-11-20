'use strict';

const zlib = require('node:zlib');

// The differential matrix of mcbench-providers: every case is a conversation
// as agent.js apiMessages builds it (system prompt, user text, prompt links,
// images, tool calls and results, seeBuild screenshots, reasoning replay,
// stopped replies, archive stubs), a provider, a model, an effort, tools on
// or off, and the scripted answers the replay server gives for it. All data
// is synthetic. The streams follow the providers' public API documentation
// (OpenAI Chat Completions and Responses streaming, Anthropic Messages
// streaming, Gemini's OpenAI compatibility, QwenCloud's compatible mode,
// OpenRouter's SSE comments and reasoning fields) and today's tests' fakes.
//
//   node cases.js > cases.json
//
// The output is deterministic: both machines generate the same file.

const KEYS = {
  openrouter: 'fixture-openrouter-key-0000000000000000',
  openai: 'fixture-openai-key-0123456789abcdef',
  anthropic: 'fixture-anthropic-key-0123456789',
  groq: 'gsk_FIXTUREgroq0123456789abcdef',
  google: 'fixture-google-key-0123456789abcdef',
  qwencloud: 'fixture-qwencloud-key-0123456789',
};
const BASE = {
  openrouter: '/openrouter.ai/api/v1',
  openai: '/api.openai.com/v1',
  anthropic: '/api.anthropic.com/v1',
  groq: '/api.groq.com/openai/v1',
  google: '/generativelanguage.googleapis.com/v1beta/openai',
  qwencloud: '/dashscope-intl.aliyuncs.com/compatible-mode/v1',
};
const CATALOG = 'GET /openrouter.ai/api/v1/models';
const CODEX = '/chatgpt.com/backend-api/codex';
const REGISTRY = 'GET /registry.npmjs.org/@openai/codex/latest';
const TOKEN = 'POST /auth.openai.com/oauth/token';

// ---- answers ----------------------------------------------------------------

const SSE_HEADERS = [['content-type', 'text/event-stream; charset=utf-8'], ['cache-control', 'no-cache']];
const RATE_HEADERS = [['x-ratelimit-limit-requests', '500'], ['x-ratelimit-remaining-requests', '499'], ['x-ratelimit-reset-requests', '120ms'], ['anthropic-ratelimit-tokens-remaining', '79000']];

// Records as a provider writes them, cut into writes of `cut` characters
// (0: one write per record).
function sse(events, { crlf = false, cut = 0, done = false, named = false, tail = true } = {}) {
  const nl = crlf ? '\r\n' : '\n';
  let text = '';
  for (const event of events) {
    if (typeof event === 'string') { text += event + nl + nl; continue; }
    const head = named && event.type ? `event: ${event.type}${nl}` : '';
    text += `${head}data: ${JSON.stringify(event)}${nl}${nl}`;
  }
  if (done) text += `data: [DONE]${nl}${nl}`;
  if (!tail) text = text.replace(/(\r?\n)+$/, '');
  if (!cut) return [text];
  const chunks = [];
  for (let i = 0; i < text.length; i += cut) chunks.push(text.slice(i, i + cut));
  return chunks;
}
const ok = (chunks, headers = SSE_HEADERS, extra = {}) => ({ status: 200, headers, chunks, ...extra });
const json = (status, value, headers = []) => ({ status, headers: [['content-type', 'application/json'], ...headers], chunks: [typeof value === 'string' ? value : JSON.stringify(value)] });

// ---- conversations ----------------------------------------------------------

const PNG = 'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
const JPEG = 'data:image/jpeg;base64,/9j/4AAQSkZJRgABAQAAAQABAAD/2wBDAP//////////////////////////////////////////////////////////////////////////////////////wgALCAABAAEBAREA/8QAFBABAAAAAAAAAAAAAAAAAAAAAP/aAAgBAQABPxA=';
const SYSTEM = { role: 'system', content: 'You build in Minecraft 1.21.10 with the tools you are given.\nCoordinates are x, y, z; "y" is up.\tTabs, ünïcödé and 🧱 stay as written.' };

const tool = (name, description, parameters) => JSON.stringify({ type: 'function', function: { name, description, parameters } });
const TOOLS = [
  tool('build', 'Run a build program. Returns the diff and the totals.', { type: 'object', properties: { program: { type: 'string', description: 'JavaScript' }, summary: { type: 'string' } }, required: ['program'], additionalProperties: false }),
  tool('bash', 'Run a command in /work. Output over 30000 characters is cut.', { type: 'object', properties: { command: { type: 'string' }, timeout_ms: { type: 'integer', minimum: 1, maximum: 600000 } }, required: ['command'] }),
  tool('see_build', 'Screenshots of the build from 4 angles at 0.5 scale.', { type: 'object', properties: { angles: { type: 'array', items: { type: 'string', enum: ['north', 'east', 'south', 'west'] } }, zoom: { type: 'number', minimum: 0.25, maximum: 4 } } }),
];

const call = (id, name, args, extra) => ({ id, type: 'function', function: { name, arguments: args }, ...(extra ? { extra_content: extra } : {}) });
const result = (id, content) => ({ role: 'tool', tool_call_id: id, content });
const screenshots = (images) => ({ role: 'user', seeBuild: true, content: [{ type: 'text', text: '[see_build] Screenshots of the current build:' }, ...images.map((url) => ({ type: 'image_url', image_url: { url } }))] });
const omitted = { role: 'user', seeBuild: true, content: [{ type: 'text', text: '[Visual tool result omitted: this model does not take images]' }] };

// The reasoning a provider's previous reply carried, as agent.js stored it.
const REPLAY = {
  chat: { reasoning: 'I should lay the floor first.' },
  qwencloud: { reasoning: 'Floor first, then walls.' },
  anthropic: { reasoning_details: [{ type: 'thinking', thinking: 'Plan: floor, walls, roof.', signature: 'EqQBCkYIBxgCKkDsig==' }, { type: 'redacted_thinking', data: 'EmwKAhgBEgy3va==' }] },
  codex: { reasoning: 'Planning the layout', reasoning_details: [{ type: 'reasoning', id: 'rs_0a1', summary: [{ type: 'summary_text', text: 'Planning the layout' }], content: [], encrypted_content: 'gAAAAABencrypted==' }] },
};

// The shapes; `family` picks the reasoning replayed and whether Gemini
// signatures ride on the calls.
function conversations(family, vision) {
  const replay = REPLAY[family] || REPLAY.chat;
  const sig = family === 'google' ? { google: { thought_signature: 'Cv8BAdHtim9sig==' } } : undefined;
  const shot = vision ? screenshots([JPEG, PNG]) : omitted;
  return [
    ['text', [SYSTEM, { role: 'user', content: 'Build a small oak house.' }]],
    ['no-system', [{ role: 'user', content: 'Name this build in three words.' }]],
    ['prompt-links', [SYSTEM, { role: 'user', content: 'Build the castle from the library.', prompts: [{ id: 'p-castle', title: 'Castle' }] }]],
    ['bench-prompt', [SYSTEM, { role: 'user', prompts: [{ id: 'p-well', title: 'A well' }], content: 'A stone well with a roof.' }]],
    ['upload', [SYSTEM, { role: 'user', content: [{ type: 'text', text: 'Copy this picture.' }, { type: 'text', text: '[Uploaded file: ref.png | image/png | 68 bytes | /work/uploads/ref.png]' }, ...(vision ? [{ type: 'image_url', image_url: { url: PNG } }] : [{ type: 'text', text: '[Image content omitted: this model does not take images]' }])] }]],
    ['selection', [SYSTEM, { role: 'user', content: [{ type: 'text', text: 'Make these walls glass.' }, { type: 'text', text: '[The user picked 2 regions of the world; each is named below.]' }, { type: 'text', text: '[Selection 1 of 2 (A): x 0..4, y 64..67, z 0..0; 20 blocks]' }, { type: 'text', text: '[Selection 2 of 2 (B): x 0..0, y 64..67, z 0..4; 20 blocks]' }] }]],
    ['tool-round', [SYSTEM, { role: 'user', content: 'Build a 3x3 platform.' },
      { role: 'assistant', content: 'Placing the platform.', finish_reason: 'tool_calls', tool_calls: [call('call_a1', 'build', '{"program":"for (let x = 0; x < 3; x++) mc.set([x, 64, 0], \\"stone\\")","summary":"platform"}', sig)] },
      result('call_a1', '{"ok":true,"worldRevision":2,"changed":3,"diff":"+3 stone","totalBlocks":3}')]],
    ['parallel-calls', [SYSTEM, { role: 'user', content: 'Check the folder and build.' },
      { role: 'assistant', content: '', finish_reason: 'tool_calls', tool_calls: [call('call_b1', 'bash', '{"command":"ls -la /work"}', sig), call('call_b2', 'build', '{"program":"mc.set([0,64,0], \\"glass\\")"}')] },
      result('call_b1', 'total 0\ndrwxr-xr-x 2 user user 40 .\n'), result('call_b2', '{"ok":true,"changed":1}')]],
    ['see-build', [SYSTEM, { role: 'user', content: 'Build and look at it.' },
      { role: 'assistant', content: '', finish_reason: 'tool_calls', tool_calls: [call('call_c1', 'see_build', '{}', sig)] },
      result('call_c1', '{"ok":true,"worldRevision":3,"minecraft":"1.21.10","shots":4,"note":"Screenshots follow."}'), shot]],
    ['reasoning-replay', [SYSTEM, { role: 'user', content: 'Build a tower.' },
      { role: 'assistant', content: 'Starting with the base.', ...replay, finish_reason: 'tool_calls', tool_calls: [call('call_d1', 'build', '{"program":"mc.fill([0,64,0],[2,64,2],\\"cobblestone\\")"}', sig)] },
      result('call_d1', '{"ok":true,"changed":9}')]],
    ['stopped', [SYSTEM, { role: 'user', content: 'Build a bridge.' }, { role: 'assistant', content: 'First the ', stopped: true }, { role: 'user', content: 'Go on.' }]],
    ['archived', [SYSTEM, { role: 'user', content: 'Keep building.' }, { role: 'assistant', content: '[Earlier tool calls and results were archived to /work/context/0001.json]' }, { role: 'user', content: 'Now the roof.' }]],
    ['bad-arguments', [SYSTEM, { role: 'user', content: 'Build.' },
      { role: 'assistant', content: '', finish_reason: 'tool_calls', tool_calls: [call('call_e1', 'build', '{"program": "mc.set(', sig)] },
      result('call_e1', '{"ok":false,"error":"Invalid tool arguments JSON: Unterminated string in JSON at position 20"}')]],
    ['long-history', [SYSTEM, { role: 'user', content: 'Build a village.' },
      { role: 'assistant', content: 'House one.', finish_reason: 'tool_calls', tool_calls: [call('call_f1', 'build', '{"program":"house(0)"}', sig)] }, result('call_f1', '{"ok":true}'),
      { role: 'assistant', content: 'House two.', ...replay, finish_reason: 'tool_calls', tool_calls: [call('call_f2', 'build', '{"program":"house(8)"}', sig)] }, result('call_f2', '{"ok":true}'),
      { role: 'assistant', content: 'Two houses stand.', finish_reason: 'stop' }, { role: 'user', content: 'Add a well, with "quotes" and a \\ backslash.' }]],
  ];
}

// ---- streams per protocol ---------------------------------------------------

const chunk = (delta, extra = {}) => ({ id: 'gen-1', object: 'chat.completion.chunk', created: 1763000000, model: 'm', choices: [{ index: 0, delta, ...extra }] });
const usageChunk = (usage) => ({ id: 'gen-1', object: 'chat.completion.chunk', created: 1763000000, model: 'm', choices: [], usage });
const finish = (reason, delta = {}) => chunk(delta, { finish_reason: reason });

// Each is [name, answer for /chat/completions]; `provider` adjusts details.
function chatStreams(provider, key) {
  const usage = { prompt_tokens: 1234, completion_tokens: 56, total_tokens: 1290, prompt_tokens_details: { cached_tokens: 1024 }, completion_tokens_details: { reasoning_tokens: 12 } };
  const reasoningField = provider === 'qwencloud' || provider === 'groq' ? 'reasoning_content' : 'reasoning';
  const geminiCalls = [
    chunk({ role: 'assistant', tool_calls: [{ extra_content: { google: { thought_signature: 'sig-one' } }, function: { arguments: '{"command":"pwd"}', name: 'bash' }, id: 'call-one', type: 'function' }] }),
    chunk({ role: 'assistant', tool_calls: [{ function: { arguments: '{"program":"x"}', name: 'build' }, id: 'call-two', type: 'function' }] }),
    { choices: [{ delta: { role: 'assistant' }, finish_reason: 'stop', index: 0 }], usage: { completion_tokens: 32, prompt_tokens: 60, total_tokens: 92 } },
  ];
  return [
    ['text', ok(sse([chunk({ role: 'assistant', content: '' }), chunk({ content: 'A small ' }), chunk({ content: 'oak house — done.' }), finish('stop'), usageChunk(usage)], { done: true }))],
    ['text-split', ok(sse([chunk({ content: 'Split ' }), chunk({ content: 'across "writes" ünï 🧱' }), finish('stop')], { cut: 7, done: true }))],
    ['crlf', ok(sse([chunk({ content: 'CRLF records' }), finish('stop'), usageChunk({ prompt_tokens: 10, completion_tokens: 2 })], { crlf: true, cut: 11 }))],
    ['reasoning', ok(sse([chunk({ [reasoningField]: 'Think about ' }), chunk({ [reasoningField]: 'the roof.', reasoning_details: [{ type: 'reasoning.text', text: 'the roof.' }] }), chunk({ content: 'Roof planned.' }), finish('stop'), usageChunk(usage)], { done: true }))],
    ['tool-calls', ok(sse([
      chunk({ role: 'assistant', content: null, tool_calls: [{ index: 0, id: 'call_x1', type: 'function', function: { name: 'build', arguments: '' } }] }),
      chunk({ tool_calls: [{ index: 0, function: { arguments: '{"program":' } }] }),
      chunk({ tool_calls: [{ index: 0, function: { arguments: '"mc.set([0,64,0], \\"stone\\")"}' } }] }),
      chunk({ tool_calls: [{ index: 1, id: 'call_x2', type: 'function', function: { name: 'bash', arguments: '{"command":' } }] }),
      chunk({ tool_calls: [{ index: 1, function: { arguments: '"ls"}' } }] }),
      finish('tool_calls'), usageChunk(usage)], { done: true, cut: 23 }))],
    ['tool-and-text', ok(sse([chunk({ content: 'Building now.' }), chunk({ tool_calls: [{ index: 0, id: 'call_y1', type: 'function', function: { name: 'see_build', arguments: '{}' } }] }), finish('tool_calls')], { done: true }))],
    ['gemini-calls', ok(sse(geminiCalls))],
    ['comments', ok(sse([': OPENROUTER PROCESSING', chunk({ content: 'after a comment' }), ': OPENROUTER PROCESSING', finish('stop')], { done: true }))],
    ['usage-with-finish', ok(sse([chunk({ content: 'one chunk' }), { ...finish('stop'), usage: { prompt_tokens: 7, completion_tokens: 3, prompt_tokens_details: { cached_tokens: 0 } } }]))],
    ['no-tail', ok(sse([chunk({ content: 'last record has no blank line' }), finish('stop')], { tail: false }))],
    ['refusal', ok(sse([chunk({ refusal: 'I cannot help with that.' }), chunk({ content: 'I cannot help with that.' }), finish('stop')], { done: true }))],
    ['length', ok(sse([chunk({ content: 'cut off' }), finish('length')], { done: true }))],
    ['content-filter', ok(sse([chunk({ content: 'filtered' }), finish('content_filter')], { done: true }))],
    ['no-finish', ok(sse([chunk({ content: 'First the ' }), chunk({ content: 'floor' })]))],
    ['no-finish-tools', ok(sse([chunk({ tool_calls: [{ index: 0, id: 'unfinished', type: 'function', function: { name: 'build', arguments: '{}' } }] })]))],
    ['error-midstream', ok(sse([chunk({ content: 'Partial ' }), { error: { message: 'Upstream provider timed out', code: 502, metadata: { provider_name: 'Fixture' } } }]))],
    ['error-midstream-nomessage', ok(sse([{ error: { code: 'server_error' } }]))],
    ['malformed', ok([`data: ${JSON.stringify(chunk({ content: 'ok ' }))}\n\n`, 'data: {"choices":[{"delta":{"content":"broken\n\n'])],
    ['malformed-token', ok([`data: ${JSON.stringify(chunk({ content: 'ok ' }))}\n\n`, 'data: not json at all\n\n'])],
    ['cut-connection', ok([...sse([chunk({ content: 'Half a ' }), chunk({ content: 'reply' })]), { destroy: true }])],
    ['multi-data-lines', ok(['data: {"choices":[{"delta":\ndata: {"content":"two data lines"}}]}\n\n', `data: ${JSON.stringify(finish('stop'))}\n\n`])],
    ['gzip', ok(sse([chunk({ content: 'compressed ' }), chunk({ content: 'stream' }), finish('stop')], { done: true }), [...SSE_HEADERS, ['content-encoding', 'gzip']], { gzip: true })],
    ['rate-headers', ok(sse([chunk({ content: 'with rate limit headers' }), finish('stop')], { done: true }), [...SSE_HEADERS, ...RATE_HEADERS])],
    ['utf8-split', ok([`data: {"choices":[{"delta":{"content":"brick `, { bytes: Buffer.from('🧱').subarray(0, 2).toString('base64') }, { bytes: Buffer.from('🧱').subarray(2).toString('base64') }, `"}}]}\n\n`, `data: ${JSON.stringify(finish('stop'))}\n\n`])],
    ['http-400', json(400, { error: { message: 'Invalid schema for function \'build\': "additionalProperties" is required', type: 'invalid_request_error', param: 'tools[0]', code: null } })],
    ['http-401-key', json(401, { error: { message: `Incorrect API key provided: ${key}. You can find your API key at https://platform.openai.com/account/api-keys. (${key})`, type: 'invalid_request_error', code: 'invalid_api_key' } })],
    ['http-429', json(429, { error: { message: 'Rate limit reached for requests', type: 'requests', code: 'rate_limit_exceeded' } }, [['retry-after', '2'], ...RATE_HEADERS])],
    ['http-500-text', { status: 500, headers: [['content-type', 'text/html']], chunks: [`<html><body>${'Internal Server Error '.repeat(30)}${key}</body></html>`] }],
    ['http-503-gemini', json(503, [{ error: { code: 503, message: 'The model is overloaded. Please try again later.', status: 'UNAVAILABLE' } }])],
    ['http-529-nested', json(529, { error: { error: { message: 'servers are currently overloaded' } } })],
    ['http-502-message', json(502, { message: 'Bad gateway from upstream' })],
    ['http-404-empty', { status: 404, headers: [], chunks: [] }],
    ...compressedStreams(),
  ];
}

// Compressed bodies that end in every way fetch tells apart, from fixed
// bytes (zlib's output differs between builds, so it is not made here):
// gzip, zlib and raw deflate of the same stream, then cut, corrupted,
// doubled, padded, and sent chunked, with a content-length, or
// close-delimited, and cut by a reset connection.
const GZIP = 'H4sIAAAAAAACE83QsU7DQAwG4D1PYXk+qkYIkG5DghUeoK2q652bO5LYIecIUJVnYWRE7Gx9IR4BBaTyBrQeLev3ZwenzsIOU0CLFfFZiQZl80Be0aKPTmde2q4hTcIzHweu0aDvySkFtOXV5fn8pwy2EqhBi+00ECV5ymgXO0wc6Bnt3GCgRh3aHXphJZ42XMMU31POFADHcTUWRTiyqaeueQGNTqEfOEMjXAGxDFUEFdgQ+EEhcU6BIGmGQNvGKcGmEV+bUznjKWmE/Sfv3w18vb59gOMAS3wcRCkv8WScGonBwTZxynF2TNRo8Fex7sllYbSYVTr8H9HK4JBdRdN3ul7aTtcqNXFGW5YG/8IP7Yvx4Frc3N/droriGzOopADQAwAA';
const ZLIB = 'eNrN0LFOw0AMBuA9T2F5PqpGCJBuQ4IVHqCtquudmzuS2CHnCFCVZ2FkROxsfSEeAQWk8ga0Hi3r92cHp87CDlNAixXxWYkGZfNAXtGij05nXtquIU3CMx8HrtGg78kpBbTl1eX5/KcMthKoQYvtNBAlecpoFztMHOgZ7dxgoEYd2h16YSWeNlzDFN9TzhQAx3E1FkU4sqmnrnkBjU6hHzhDI1wBsQxVBBXYEPhBIXFOgSBphkDbxinBphFfm1M54ylphP0n798NfL2+fYDjAEt8HEQpL/FknBqJwcE2ccpxdkzUaPBXse7JZWG0mFU6/B/RyuCQXUXTd7pe2k7XKjVxRluWBv/CD+2L8eBa3Nzf3a6K4hvg9DrU';
const RAW = 'zdCxTsNADAbgPU9heT6qRgiQbkOCFR6grarrnZs7ktgh5whQlWdhZETsbH0hHgEFpPIGtB4t6/dnB6fOwg5TQIsV8VmJBmXzQF7Roo9OZ17ariFNwjMfB67RoO/JKQW05dXl+fynDLYSqEGL7TQQJXnKaBc7TBzoGe3cYKBGHdodemElnjZcwxTfU84UAMdxNRZFOLKpp655AY1OoR84QyNcAbEMVQQV2BD4QSFxToEgaYZA28YpwaYRX5tTOeMpaYT9J+/fDXy9vn2A4wBLfBxEKS/xZJwaicHBNnHKcXZM1GjwV7HuyWVhtJhVOvwf0crgkF1F03e6XtpO1yo1cUZblgb/wg/ti/HgWtzc392uiuIb';

function compressedStreams() {
  const gzip = Buffer.from(GZIP, 'base64');
  const zlibbed = Buffer.from(ZLIB, 'base64');
  const raw = Buffer.from(RAW, 'base64');
  // The fixed bytes are what they say they are.
  const text = zlib.gunzipSync(gzip).toString();
  if (zlib.inflateSync(zlibbed).toString() !== text || zlib.inflateRawSync(raw).toString() !== text) throw new Error('the compressed fixtures disagree');
  const bytes = (b) => ({ bytes: b.toString('base64') });
  // Two writes, split inside the deflate data.
  const split = (b) => [bytes(b.subarray(0, 100)), bytes(b.subarray(100))];
  const answer = (encoding, chunks, extra = {}, headers = []) => ({ status: 200, headers: [...SSE_HEADERS, ['content-encoding', encoding], ...headers], chunks, ...extra });
  const badCrc = Buffer.from(gzip);
  badCrc[badCrc.length - 8] ^= 1;
  const badAdler = Buffer.from(zlibbed);
  badAdler[badAdler.length - 1] ^= 1;
  return [
    ['gzip-whole-split', answer('gzip', split(gzip))],
    ['gzip-no-trailer', answer('gzip', split(gzip.subarray(0, gzip.length - 8)))],
    ['gzip-half-trailer', answer('gzip', split(gzip.subarray(0, gzip.length - 4)))],
    ['gzip-mid-block', answer('gzip', [bytes(gzip.subarray(0, 200))])],
    ['gzip-header-only', answer('gzip', [bytes(gzip.subarray(0, 10))])],
    ['gzip-bad-crc', answer('gzip', split(badCrc))],
    ['gzip-two-members', answer('gzip', [bytes(gzip.subarray(0, gzip.length - 40)), bytes(Buffer.concat([gzip.subarray(gzip.length - 40), gzip]))])],
    ['gzip-trailing-zeros', answer('gzip', [bytes(Buffer.concat([gzip, Buffer.alloc(4)]))])],
    ['gzip-trailing-garbage', answer('gzip', [bytes(Buffer.concat([gzip, Buffer.from('xyz')]))])],
    ['gzip-not-gzip', answer('gzip', ['data: {"choices":[]}\n\n'])],
    ['gzip-length', answer('gzip', split(gzip), {}, [['content-length', String(gzip.length)]])],
    ['gzip-length-no-trailer', answer('gzip', [bytes(gzip.subarray(0, gzip.length - 8))], {}, [['content-length', String(gzip.length - 8)]])],
    ['gzip-close', answer('gzip', split(gzip), { close: true })],
    ['gzip-close-no-trailer', answer('gzip', [bytes(gzip.subarray(0, gzip.length - 8))], { close: true })],
    ['gzip-reset-before-trailer', answer('gzip', [bytes(gzip.subarray(0, gzip.length - 8)), { destroy: true }])],
    ['gzip-reset-mid-block', answer('gzip', [bytes(gzip.subarray(0, 200)), { destroy: true }])],
    ['x-gzip', answer('x-gzip', split(gzip))],
    ['gzip-twice-named', answer('GZIP , identity', split(gzip))],
    ['deflate-zlib', answer('deflate', split(zlibbed))],
    ['deflate-raw', answer('deflate', split(raw))],
    ['deflate-no-adler', answer('deflate', [bytes(zlibbed.subarray(0, zlibbed.length - 4))])],
    ['deflate-bad-adler', answer('deflate', [bytes(badAdler)])],
    ['identity', answer('identity', sse([chunk({ content: 'not compressed' }), finish('stop')]))],
  ];
}

const a = (event) => event;
function anthropicStreams(key) {
  const start = (usage = { input_tokens: 10, cache_creation_input_tokens: 20, cache_read_input_tokens: 30, output_tokens: 1 }) => a({ type: 'message_start', message: { id: 'msg_01', type: 'message', role: 'assistant', model: 'claude-fixture', content: [], stop_reason: null, stop_sequence: null, usage } });
  const text = (index, parts) => [a({ type: 'content_block_start', index, content_block: { type: 'text', text: '' } }), ...parts.map((t) => a({ type: 'content_block_delta', index, delta: { type: 'text_delta', text: t } })), a({ type: 'content_block_stop', index })];
  const thinking = (index, parts, signature) => [a({ type: 'content_block_start', index, content_block: { type: 'thinking', thinking: '', signature: '' } }), ...parts.map((t) => a({ type: 'content_block_delta', index, delta: { type: 'thinking_delta', thinking: t } })), a({ type: 'content_block_delta', index, delta: { type: 'signature_delta', signature } }), a({ type: 'content_block_stop', index })];
  const toolUse = (index, id, name, parts) => [a({ type: 'content_block_start', index, content_block: { type: 'tool_use', id, name, input: {} } }), ...parts.map((p) => a({ type: 'content_block_delta', index, delta: { type: 'input_json_delta', partial_json: p } })), a({ type: 'content_block_stop', index })];
  const end = (reason, usage = { output_tokens: 42 }) => [a({ type: 'message_delta', delta: { stop_reason: reason, stop_sequence: null }, usage }), a({ type: 'message_stop' })];
  const opts = { named: true };
  return [
    ['text', ok(sse([start(), a({ type: 'ping' }), ...text(0, ['A small ', 'oak house.']), ...end('end_turn')], opts))],
    ['text-split', ok(sse([start(), ...text(0, ['split ', 'ünï 🧱 "q"']), ...end('end_turn')], { ...opts, cut: 9 }))],
    ['thinking', ok(sse([start(), ...thinking(0, ['Plan the ', 'roof.'], 'EqQBsig=='), ...text(1, ['Roof first.']), ...end('end_turn')], opts))],
    ['thinking-tools', ok(sse([start(), ...thinking(0, ['Two calls.'], 'sigA=='), ...text(1, ['Working.']), ...toolUse(2, 'toolu_01', 'build', ['{"program":', ' "mc.set([0,64,0], \\"stone\\")"}']), ...toolUse(3, 'toolu_02', 'bash', ['', '{"command": "ls"}']), ...end('tool_use')], { ...opts, cut: 40 }))],
    ['redacted', ok(sse([start(), a({ type: 'content_block_start', index: 0, content_block: { type: 'redacted_thinking', data: 'EmwKAhgBEgy3va==' } }), a({ type: 'content_block_stop', index: 0 }), ...thinking(1, ['visible'], 'sigB=='), ...text(2, ['Done.']), ...end('end_turn')], opts))],
    ['tool-only', ok(sse([start({ input_tokens: 5, output_tokens: 1 }), ...toolUse(0, 'toolu_09', 'see_build', ['{}']), ...end('tool_use', { output_tokens: 7 })], opts))],
    ['no-cache-fields', ok(sse([start({ input_tokens: 99, output_tokens: 0 }), ...text(0, ['plain usage']), ...end('end_turn', { output_tokens: 3 })]))],
    ['delta-usage-cache', ok(sse([start({ input_tokens: 1, output_tokens: 1 }), ...text(0, ['late cache']), ...end('end_turn', { output_tokens: 3, cache_read_input_tokens: 500, cache_creation_input_tokens: 0 })]))],
    ['refusal', ok(sse([start(), ...text(0, ['I can\'t help with that.']), ...end('refusal')], opts))],
    ['pause-turn', ok(sse([start(), ...text(0, ['paused']), ...end('pause_turn')], opts))],
    ['max-tokens', ok(sse([start(), ...text(0, ['cut']), ...end('max_tokens')], opts))],
    ['no-stop', ok(sse([start(), ...text(0, ['no stop']), a({ type: 'message_delta', delta: { stop_reason: 'end_turn' }, usage: { output_tokens: 2 } })], opts))],
    ['error-event', ok(sse([start(), ...text(0, ['Partial']), a({ type: 'error', error: { type: 'overloaded_error', message: 'Overloaded' } })], opts))],
    ['error-event-api', ok(sse([a({ type: 'error', error: { type: 'api_error', message: 'Internal server error' } })], opts))],
    ['malformed', ok([`event: message_start\ndata: ${JSON.stringify(start())}\n\n`, 'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}\n\n'])],
    ['cut-connection', ok([...sse([start(), ...text(0, ['Half'])], opts), { destroy: true }])],
    ['crlf', ok(sse([start(), ...text(0, ['crlf']), ...end('end_turn')], { ...opts, crlf: true, cut: 13 }))],
    ['gzip', ok(sse([start(), ...text(0, ['compressed']), ...end('end_turn')], opts), [...SSE_HEADERS, ['content-encoding', 'gzip']], { gzip: true })],
    ['rate-headers', ok(sse([start(), ...text(0, ['headers']), ...end('end_turn')], opts), [...SSE_HEADERS, ...RATE_HEADERS])],
    ['http-400', json(400, { type: 'error', error: { type: 'invalid_request_error', message: 'messages.1.content.0: thinking block signature is invalid' } })],
    ['http-401-key', json(401, { type: 'error', error: { type: 'authentication_error', message: `invalid x-api-key ${key}` } })],
    ['http-429', json(429, { type: 'error', error: { type: 'rate_limit_error', message: 'Number of request tokens has exceeded your per-minute rate limit' } }, [['retry-after', '7'], ['anthropic-ratelimit-requests-remaining', '0']])],
    ['http-529', json(529, { type: 'error', error: { type: 'overloaded_error', message: 'Overloaded' } })],
    ['http-500-text', { status: 500, headers: [['content-type', 'text/plain']], chunks: ['upstream connect error or disconnect/reset before headers'] }],
  ];
}

function codexStreams() {
  const completed = (usage = { input_tokens: 2048, input_tokens_details: { cached_tokens: 1536 }, output_tokens: 300, output_tokens_details: { reasoning_tokens: 128 }, total_tokens: 2348 }) => ({ type: 'response.completed', sequence_number: 99, response: { id: 'resp_1', object: 'response', status: 'completed', output: [], usage } });
  const created = { type: 'response.created', sequence_number: 0, response: { id: 'resp_1', object: 'response', status: 'in_progress', output: [] } };
  const message = (index, text) => [
    { type: 'response.output_item.added', output_index: index, item: { id: `msg_${index}`, type: 'message', status: 'in_progress', role: 'assistant', content: [] } },
    { type: 'response.content_part.added', output_index: index, item_id: `msg_${index}`, content_index: 0, part: { type: 'output_text', text: '' } },
    ...text.map((delta) => ({ type: 'response.output_text.delta', output_index: index, item_id: `msg_${index}`, content_index: 0, delta })),
    { type: 'response.output_text.done', output_index: index, item_id: `msg_${index}`, content_index: 0, text: text.join('') },
    { type: 'response.output_item.done', output_index: index, item: { id: `msg_${index}`, type: 'message', status: 'completed', role: 'assistant', content: [{ type: 'output_text', text: text.join(''), annotations: [] }] } },
  ];
  const reasoning = (index, id, parts) => {
    const events = [{ type: 'response.output_item.added', output_index: index, item: { id, type: 'reasoning', summary: [] } }];
    parts.forEach((text, summary_index) => {
      events.push({ type: 'response.reasoning_summary_part.added', item_id: id, output_index: index, summary_index, part: { type: 'summary_text', text: '' } });
      for (const delta of [text.slice(0, 5), text.slice(5)]) events.push({ type: 'response.reasoning_summary_text.delta', item_id: id, output_index: index, summary_index, delta });
      events.push({ type: 'response.reasoning_summary_text.done', item_id: id, output_index: index, summary_index, text });
    });
    events.push({ type: 'response.output_item.done', output_index: index, item: { id, type: 'reasoning', summary: parts.map((text) => ({ type: 'summary_text', text })), encrypted_content: `enc-${id}` } });
    return events;
  };
  const fn = (index, callId, name, parts) => [
    { type: 'response.output_item.added', output_index: index, item: { id: `fc_${index}`, type: 'function_call', status: 'in_progress', call_id: callId, name, arguments: '' } },
    ...parts.map((delta) => ({ type: 'response.function_call_arguments.delta', output_index: index, item_id: `fc_${index}`, delta })),
    { type: 'response.function_call_arguments.done', output_index: index, item_id: `fc_${index}`, arguments: parts.join('') },
    { type: 'response.output_item.done', output_index: index, item: { id: `fc_${index}`, type: 'function_call', status: 'completed', call_id: callId, name, arguments: parts.join('') } },
  ];
  return [
    ['text', ok(sse([created, ...message(0, ['A small ', 'oak house.']), completed()]))],
    ['text-split', ok(sse([created, ...message(0, ['ünï ', '🧱 "split"']), completed()], { cut: 17 }))],
    ['reasoning-text', ok(sse([created, ...reasoning(0, 'rs_1', ['**Planning the layout**', '**Sizing the canvas**']), ...message(1, ['Planned.']), completed()]))],
    ['reasoning-tools', ok(sse([created, ...reasoning(0, 'rs_2', ['**Choosing materials**']), ...fn(1, 'call_1', 'build', ['{"program":', '"mc.set([0,64,0], \\"stone\\")"}']), ...fn(2, 'call_2', 'bash', ['{"command":"ls"}']), completed()], { cut: 50 }))],
    ['two-reasoning-items', ok(sse([created, ...reasoning(0, 'rs_3', ['**First**']), ...message(1, ['Between.']), ...reasoning(2, 'rs_4', ['**Second**', '**Third**']), ...fn(3, 'call_3', 'see_build', ['{}']), completed()]))],
    ['tools-first', ok(sse([created, ...fn(0, 'call_4', 'build', ['{}']), ...reasoning(1, 'rs_5', ['**After the call**']), completed({ input_tokens: 5, output_tokens: 1 })]))],
    ['refusal', ok(sse([created, { type: 'response.output_item.done', output_index: 0, item: { id: 'msg_r', type: 'message', role: 'assistant', content: [{ type: 'refusal', refusal: 'I can\'t help with that.' }] } }, completed()]))],
    ['no-usage', ok(sse([created, ...message(0, ['no usage']), { type: 'response.completed', response: { output: [] } }]))],
    ['failed', ok(sse([created, ...message(0, ['Part']), { type: 'response.failed', response: { error: { code: 'rate_limit_exceeded', message: 'Quota exceeded' } } }]))],
    ['error-event', ok(sse([{ type: 'error', code: 'server_error', message: 'The server had an error while processing your request.' }]))],
    ['error-event-nested', ok(sse([{ type: 'error', error: { message: 'nested error message' } }]))],
    ['failed-bare', ok(sse([{ type: 'response.failed', response: {} }]))],
    ['incomplete', ok(sse([created, ...message(0, ['cut']), { type: 'response.incomplete', response: { incomplete_details: { reason: 'max_output_tokens' } } }]))],
    ['truncated', ok(sse([created, ...message(0, ['Partial answer'])]))],
    ['stalled', ok([...sse([created, { type: 'response.output_item.added', output_index: 0, item: { type: 'function_call', call_id: 'call_s', name: 'build', arguments: '' } }, { type: 'response.function_call_arguments.delta', output_index: 0, delta: '{"program":[' }]), ...Array.from({ length: 4 }, () => sse([{ type: 'response.function_call_arguments.delta', output_index: 0, delta: ' '.repeat(5000) }])[0])])],
    ['malformed', ok([`data: ${JSON.stringify(created)}\n\n`, 'data: {"type":"response.output_text.delta","delta":}\n\n'])],
    ['cut-connection', ok([...sse([created, ...message(0, ['Half'])]), { destroy: true }])],
    ['done-marker', ok(sse([created, ...message(0, ['with done']), completed()], { done: true }))],
    ['http-400', json(400, { detail: 'Unsupported parameter: reasoning.summary' })],
    ['http-429', json(429, { error: { type: 'usage_limit_reached', message: 'The usage limit has been reached', resets_in_seconds: 3600 } })],
    ['http-500', { status: 500, headers: [['content-type', 'text/plain']], chunks: ['internal error'] }],
  ];
}

// The Codex catalogue the OAuth bridge reads for each request's model
// defaults (GET /models?client_version=...), and the npm version it asks for.
const CODEX_MODELS = { models: [
  { slug: 'gpt-7-nova', visibility: 'list', supported_in_api: true, default_reasoning_level: 'medium', support_verbosity: true, default_verbosity: 'low' },
  { slug: 'gpt-6-astra', visibility: 'list', supported_in_api: true, default_reasoning_level: 'high' },
  { slug: 'gpt-6-lite', visibility: 'list', supported_in_api: true, use_responses_lite: true, default_reasoning_level: 'low' },
  { slug: 'gpt-6-mini', visibility: 'list', supported_in_api: true },
] };

// ---- tokens -----------------------------------------------------------------

const b64url = (value) => Buffer.from(JSON.stringify(value)).toString('base64url');
const jwt = (claims) => `${b64url({ alg: 'RS256', typ: 'JWT' })}.${b64url(claims)}.fixture-signature`;
const FAR = 4102444800; // 2100-01-01
const ACCOUNT = { 'https://api.openai.com/auth': { chatgpt_account_id: 'acct-fixture-0001', chatgpt_plan_type: 'pro' }, email: 'person@example.com' };
const auth = (accessClaims, extra = {}) => JSON.stringify({ OPENAI_API_KEY: null, tokens: { id_token: jwt({ ...ACCOUNT, exp: FAR }), access_token: accessClaims ? jwt(accessClaims) : undefined, refresh_token: 'rt-fixture-0001', account_id: 'acct-fixture-0001' }, last_refresh: '2100-01-01T00:00:00.000Z', ...extra }, null, 2);
const FRESH_AUTH = auth({ ...ACCOUNT, exp: FAR });

// ---- the matrix -------------------------------------------------------------

const MODELS = {
  openrouter: ['openai/gpt-5.6-luna', 'anthropic/claude-opus-5.5', 'qwen/qwen3.8-max'],
  openai: ['gpt-5.6', 'gpt-5.6-mini'],
  groq: ['openai/gpt-oss-120b', 'moonshotai/kimi-k3'],
  google: ['gemini-3.8-flash', 'gemini-3.8-pro'],
  qwencloud: ['qwen3.8-max', 'qwen3.7-plus', 'qwen3.8-2.4t-a95b', 'qwen3.8-27b', 'qwen-unlisted'],
  anthropic: ['claude-opus-5-5', 'claude-sonnet-4-6-20261001', 'claude-haiku-legacy'],
  codex: ['gpt-7-nova', 'gpt-6-astra', 'gpt-6-lite', 'gpt-6-mini'],
};
const EFFORTS = [null, 'none', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'];
const VISION = { qwencloud: (m) => !['qwen3.8-2.4t-a95b'].includes(m), codex: (m) => m !== 'gpt-6-astra' };

function catalogAnswer() {
  return json(200, { data: [
    { id: 'anthropic/claude-opus-5.5', name: 'Anthropic: Claude Opus 5.5', created: 1760000000, context_length: 1000000, architecture: { modality: 'text+image->text', input_modalities: ['text', 'image'], output_modalities: ['text'] }, supported_parameters: ['tools', 'reasoning'], reasoning: { mandatory: false, default_effort: 'high', supported_efforts: ['max', 'low', 'medium', 'high', 'xhigh'] } },
    { id: 'anthropic/claude-sonnet-4.6', name: 'Anthropic: Claude Sonnet 4.6', created: 1750000000, context_length: 200000, architecture: { modality: 'text+image->text' }, supported_parameters: ['tools'], reasoning: { supported_efforts: ['low', 'high'] } },
    { id: 'openai/gpt-5.6-luna', name: 'OpenAI: GPT-5.6 Luna', created: 1761000000, architecture: { input_modalities: ['text', 'image'], output_modalities: ['text'] }, supported_parameters: ['tools', 'reasoning'], reasoning: { supported_efforts: ['minimal', 'low', 'medium', 'high'], default_effort: 'medium' } },
    { id: 'openai/gpt-5.6', name: 'OpenAI: GPT-5.6', created: 1761000001, architecture: { input_modalities: ['text', 'image'], output_modalities: ['text'] }, supported_parameters: ['tools'] },
    { id: 'openai/gpt-image-2', name: 'OpenAI: GPT Image 2', created: 1761000002, architecture: { input_modalities: ['text', 'image'], output_modalities: ['image'] } },
    { id: 'openai/gpt-oss-120b', name: 'OpenAI: gpt-oss-120b', created: 1740000000, architecture: { modality: 'text->text' }, supported_parameters: ['tools', 'reasoning'], reasoning: { supported_efforts: ['high', 'low', 'medium'] } },
    { id: 'google/gemini-3.8-flash', name: 'Google: Gemini 3.8 Flash', created: 1762000000, context_length: 1048576, architecture: { input_modalities: ['text', 'image'], output_modalities: ['text'] }, supported_parameters: ['tools', 'reasoning'], reasoning: { mandatory: true, supported_efforts: ['high', 'medium', 'low'], default_effort: 'medium' } },
    { id: 'google/gemini-3.1-flash-image', name: 'Nano Banana 2', created: 1761500000, architecture: { input_modalities: ['text', 'image'], output_modalities: ['image', 'text'] } },
    { id: 'qwen/qwen3.8-max', name: 'Qwen: Qwen3.8 Max', created: 1759000000, architecture: { modality: 'text+image->text' }, supported_parameters: ['tools'] },
    { id: 'moonshotai/kimi-k3', name: 'MoonshotAI: Kimi K3', architecture: { modality: 'text->text' }, supported_parameters: ['tools'] },
    { id: 'anthropic/claude-opus-5.5:batch', name: 'Anthropic: Claude Opus 5.5 (batch)', created: 1760000000 },
  ] });
}

const cases = [];
const add = (entry) => cases.push({ cancelAfterDeltas: null, conversationId: null, codexAuth: null, env: {}, ...entry, id: `${String(cases.length).padStart(3, '0')}-${entry.id}` });

// Anthropic first: the first request reads the catalog for the model's
// effort levels; the catalog is down, so it fails, and the lookup is retried
// on the next request (llm.js anthropicAdaptive).
add({ id: 'anthropic-catalog-down', kind: 'complete', provider: 'anthropic', model: 'claude-opus-5-5', effort: 'high', messages: conversations('anthropic', true)[0][1], tools: TOOLS, routes: { [CATALOG]: [{ status: 503, headers: [['content-type', 'text/plain']], chunks: ['unavailable'] }] } });

let streamTurn = 0;
function matrix(provider, family, streams, path) {
  const models = MODELS[provider];
  let n = 0;
  // Every stream once, then every conversation shape once more, so each
  // protocol runs every event type and every message shape.
  const shapesFor = (model) => conversations(family, (VISION[provider] || (() => true))(model));
  const total = Math.max(streams.length, 14) + 18;
  for (let i = 0; i < total; i++) {
    const model = models[i % models.length];
    const shapes = shapesFor(model);
    const [shapeName, messages] = shapes[(i * 5 + streamTurn) % shapes.length];
    const [streamName, answer] = streams[i % streams.length];
    const effort = EFFORTS[(i * 3 + n) % EFFORTS.length];
    const toolsOn = i % 4 !== 3;
    const routes = { [path]: [JSON.parse(JSON.stringify(answer))] };
    const entry = { id: `${provider}-${shapeName}-${streamName}`, kind: 'complete', provider, model, effort, messages, tools: toolsOn ? TOOLS : [], routes };
    if (i % 3 === 1) entry.conversationId = `6f1c2a40-1111-4c3e-9d2b-${String(1000 + i).padStart(12, '0')}`;
    add(entry);
    n++;
  }
  // Stopped by the user after the first delta.
  const [, split] = streams.find(([name]) => name === 'text-split');
  add({ id: `${provider}-cancelled`, kind: 'complete', provider, model: models[0], effort: null, messages: shapesFor(models[0])[0][1], tools: TOOLS, routes: { [path]: [JSON.parse(JSON.stringify(split))] }, cancelAfterDeltas: 1 });
  streamTurn++;
}

const routeOf = (provider) => `POST ${BASE[provider]}/chat/completions`;
for (const provider of ['openrouter', 'openai', 'groq', 'google', 'qwencloud']) {
  matrix(provider, provider === 'qwencloud' ? 'qwencloud' : provider === 'google' ? 'google' : 'chat', chatStreams(provider, KEYS[provider]), routeOf(provider));
}

// Anthropic: the catalog answers now and is read once.
const firstAnthropic = cases.length;
matrix('anthropic', 'anthropic', anthropicStreams(KEYS.anthropic), 'POST /api.anthropic.com/v1/messages');
cases[firstAnthropic].routes[CATALOG] = [catalogAnswer()];
// The model not in the catalog was looked up once already (no effort levels):
// a data URL that is not base64 and a content part Anthropic cannot take fail.
add({ id: 'anthropic-not-base64', kind: 'complete', provider: 'anthropic', model: 'claude-opus-5-5', effort: 'low', messages: [{ role: 'user', content: [{ type: 'image_url', image_url: { url: 'https://example.com/a.png' } }] }], tools: [], routes: {} });
add({ id: 'anthropic-unsupported-part', kind: 'complete', provider: 'anthropic', model: 'claude-opus-5-5', effort: null, messages: [{ role: 'user', content: [{ type: 'input_audio', input_audio: { data: 'AAAA', format: 'wav' } }] }], tools: [], routes: {} });

// Codex: the first request asks npm for the Codex version and the backend for
// its catalogue (each cached), then sends the request.
const codexRoute = `POST ${CODEX}/responses`;
const codexStreamList = codexStreams();
const firstCodex = cases.length;
matrix('codex', 'codex', codexStreamList, codexRoute);
cases[firstCodex].routes[REGISTRY] = [json(200, { name: '@openai/codex', version: '0.169.0' })];
cases[firstCodex].routes[`GET ${CODEX}/models?client_version=0.169.0`] = [json(200, CODEX_MODELS)];
cases[firstCodex].codexAuth = FRESH_AUTH;
// A seeBuild message that does not follow a tool result fails.
add({ id: 'codex-stray-screenshots', kind: 'complete', provider: 'codex', model: 'gpt-7-nova', effort: null, messages: [{ role: 'user', content: 'look' }, screenshots([PNG])], tools: [], routes: {} });
// An expired access token is refreshed first and written back to auth.json.
add({ id: 'codex-refresh', kind: 'complete', provider: 'codex', model: 'gpt-6-astra', effort: 'high', messages: conversations('codex', false)[0][1], tools: TOOLS,
  codexAuth: auth({ ...ACCOUNT, exp: 1700000000 }, { auth_mode: 'chatgpt', extra_field: [1, 2] }),
  routes: { [TOKEN]: [json(200, { access_token: jwt({ ...ACCOUNT, exp: FAR, iat: 1 }), refresh_token: 'rt-fixture-0002', id_token: jwt({ ...ACCOUNT, exp: FAR, iat: 2 }), expires_in: 864000 })], [codexRoute]: [codexStreamList[0][1]] } });
add({ id: 'codex-refresh-refused', kind: 'complete', provider: 'codex', model: 'gpt-6-astra', effort: 'high', messages: conversations('codex', false)[1][1], tools: [],
  codexAuth: auth({ ...ACCOUNT, exp: 1700000000 }),
  routes: { [TOKEN]: [json(401, { error: 'invalid_grant', error_description: 'The refresh token has been revoked.' })] } });
add({ id: 'codex-no-token', kind: 'complete', provider: 'codex', model: 'gpt-6-astra', effort: null, messages: conversations('codex', false)[1][1], tools: [], codexAuth: JSON.stringify({ tokens: { id_token: jwt({ ...ACCOUNT, exp: FAR }) } }), routes: {} });
add({ id: 'codex-no-account', kind: 'complete', provider: 'codex', model: 'gpt-6-astra', effort: null, messages: conversations('codex', false)[1][1], tools: [], codexAuth: JSON.stringify({ tokens: { access_token: jwt({ exp: FAR }) } }), routes: {} });
add({ id: 'codex-fedramp', kind: 'complete', provider: 'codex', model: 'gpt-6-mini', effort: 'low', messages: conversations('codex', true)[0][1], tools: [], conversationId: 'chat-fedramp',
  codexAuth: JSON.stringify({ tokens: { access_token: jwt({ exp: FAR, 'https://api.openai.com/auth': { chatgpt_account_id: 'acct-fed', chatgpt_account_is_fedramp: true } }), account_id: 'acct-fed' } }),
  routes: { [`GET ${CODEX}/models?client_version=0.169.0`]: [json(500, { detail: 'catalogue down' })], [codexRoute]: [codexStreamList[0][1]] } });
add({ id: 'codex-signed-in-again', kind: 'complete', provider: 'codex', model: 'gpt-6-lite', effort: null, messages: conversations('codex', true)[6][1], tools: TOOLS, conversationId: 'chat-lite', codexAuth: FRESH_AUTH, routes: { [codexRoute]: [codexStreamList[3][1]] } });

// Settings: a provider without a key, and ChatGPT without its login.
add({ id: 'openai-no-key', kind: 'complete', provider: 'openai', model: 'gpt-5.6', effort: null, messages: conversations('chat', true)[0][1], tools: [], env: { OPENAI_API_KEY: '' }, routes: {} });

// Model lists and model facts, each provider's own way.
add({ id: 'models-openrouter', kind: 'listModels', provider: 'openrouter', routes: {} });
add({ id: 'models-openai', kind: 'listModels', provider: 'openai', routes: { [`GET ${BASE.openai}/models`]: [json(200, { object: 'list', data: [{ id: 'gpt-5.6', object: 'model', created: 1761000001, owned_by: 'openai' }, { id: 'gpt-image-2', object: 'model', created: 1761000002 }, { id: 'gpt-5.6-mini', object: 'model', created: 1761000001 }, { id: 'o9', object: 'model' }] })] } });
add({ id: 'models-openai-cached', kind: 'listModels', provider: 'openai', routes: {} });
add({ id: 'models-groq-error', kind: 'listModels', provider: 'groq', routes: { [`GET ${BASE.groq}/models`]: [json(401, { error: { message: `Invalid API Key ${KEYS.groq}`, type: 'invalid_request_error', code: 'invalid_api_key' } })] } });
add({ id: 'models-groq', kind: 'listModels', provider: 'groq', routes: { [`GET ${BASE.groq}/models`]: [json(200, { object: 'list', data: [{ id: 'openai/gpt-oss-120b', object: 'model', created: 1740000000, owned_by: 'OpenAI' }, { id: 'moonshotai/kimi-k3', created: 1740000000 }, { id: 'whisper-large-v4', created: 1 }] })] } });
add({ id: 'models-anthropic', kind: 'listModels', provider: 'anthropic', routes: {
  [`GET ${BASE.anthropic}/models?limit=100`]: [json(200, { data: [{ type: 'model', id: 'claude-opus-5-5', display_name: 'Claude Opus 5.5', created_at: '2026-09-01T00:00:00Z' }], has_more: true, first_id: 'claude-opus-5-5', last_id: 'claude-opus-5-5' })],
  [`GET ${BASE.anthropic}/models?limit=100&after_id=claude-opus-5-5`]: [json(200, { data: [{ type: 'model', id: 'claude-sonnet-4-6-20261001', display_name: 'Claude Sonnet 4.6', created_at: '2026-10-01T00:00:00Z' }, { type: 'model', id: 'claude-mystery', created_at: 'not a date' }], has_more: false, last_id: 'claude-mystery' })] } });
add({ id: 'models-google', kind: 'listModels', provider: 'google', routes: {
  'GET /generativelanguage.googleapis.com/v1beta/models?pageSize=1000': [json(200, { models: [{ name: 'models/gemini-2.5-flash', displayName: 'Gemini 2.5 Flash', supportedGenerationMethods: ['generateContent', 'countTokens'] }, { name: 'models/gemini-embedding-2', displayName: 'Gemini Embedding 2', supportedGenerationMethods: ['embedContent'] }], nextPageToken: 'page 2/=' })],
  'GET /generativelanguage.googleapis.com/v1beta/models?pageSize=1000&pageToken=page%202%2F%3D': [json(200, { models: [{ name: 'models/gemini-3.8-flash', displayName: 'Gemini 3.8 Flash', supportedGenerationMethods: ['generateContent'] }, { name: 'models/gemini-3.1-flash-image', displayName: 'Nano Banana 2', supportedGenerationMethods: ['generateContent'] }] })] } });
add({ id: 'models-qwencloud', kind: 'listModels', provider: 'qwencloud', routes: { [`GET ${BASE.qwencloud}/models`]: [json(200, { object: 'list', data: [{ id: 'qwen3.7-plus', created: 1756000000 }, { id: 'qwen3.8-max', created: 1758000000 }, { id: 'qwen-legacy', created: 1 }, { id: 'qwen3.8-2.4t-a95b' }] })] } });
add({ id: 'models-codex', kind: 'listModels', provider: 'codex', routes: {} });
for (const [provider, model] of [['openrouter', 'openai/gpt-5.6-luna'], ['openrouter', 'not/listed'], ['openai', 'gpt-5.6'], ['groq', 'openai/gpt-oss-120b'], ['google', 'gemini-3.8-flash'], ['anthropic', 'claude-sonnet-4-6-20261001'], ['qwencloud', 'qwen3.8-2.4t-a95b'], ['qwencloud', 'qwen3.7-max'], ['qwencloud', 'unknown'], ['codex', 'gpt-7-nova'], ['codex', 'nope']]) {
  add({ id: `info-${provider}-${model.replace(/\W/g, '_')}`, kind: 'modelInfo', provider, model, routes: {} });
}

process.stdout.write(`${JSON.stringify({ keys: KEYS, cases }, null, 1)}\n`);
