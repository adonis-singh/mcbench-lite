'use strict';

// Writes tests/json-errors.jsonl: texts and the message V8's JSON.parse
// gives for them (null when it parses), for src/api/v8json.rs. The texts
// are every error class by hand, then every one-character deletion,
// insertion and replacement of a few provider events, so the cursor
// position of each class is held too.
//
//   node json-errors.js > ../json-errors.jsonl

const seeds = [
  '{"choices":[{"delta":{"content":"ok"},"finish_reason":null}]}',

  '[1,-2.5e+3,true,false,null,"\\u00e9\\n"]',
  '{"a":[{"b":{}},[]],"c":0}',
];
const inserts = ['}', ']', ',', ':', '"', '\\', 'x', '0', '-', '.', 'e', ' ', '\n', '\u0001', 'é'];

const texts = new Set([
  '', ' ', '{', '}', '[', ']', '{"a"', '{"a":', '{"a":1', '{"a":1,', '{"a" 1}', '{a:1}', '{"a":1,}', '{"a":1 "b":2}',
  '[1 2]', '[1,]', '[,1]', '01', '-', '-x', '1.', '1.x', '1e', '1e+', '1E-x', '-01', '0x10', '.5',
  '"abc', '"a\\x"', '"a\\', '"\\u12"', '"\\u12G4"', '"\\u00e9"', '"a\u0001b"', '"tab\there"', '"é\\€"',
  'tru', 'trux', 'nul1', 'fals', 'falsey', 'nan', 'NaN', 'Infinity', 'undefined', '[object Object]',
  'not json at all', 'data: {"a":1}', '{"a":1}x', '{"a":1} ', '{"a":1}\n{"b":2}', '1 2', '"x" "y"',
  '{"content":"broken', '{"line":1,\n"two":2,\r\n"three":x}', '{"a":\r\r\n1 x}', '€', '[€]', '{"🧱":1,x}',
  '{"a":"012345678901234567890123456789"x}', '{"aaaaaaaaaaaaaaaaaaaaaaa":x}', 'x{"aaaaaaaaaaaaaaaaaaaaaaa":1}',
  '{"type":"response.output_text.delta","delta":}',
]);
for (const seed of seeds) {
  for (let i = 0; i <= seed.length; i++) {
    texts.add(seed.slice(0, i));
    if (i < seed.length) texts.add(seed.slice(0, i) + seed.slice(i + 1));
    for (const c of inserts) {
      texts.add(seed.slice(0, i) + c + seed.slice(i));
      if (i < seed.length) texts.add(seed.slice(0, i) + c + seed.slice(i + 1));
    }
  }
}
const lines = [];
for (const text of texts) {
  // A lone surrogate cannot be written to a file as UTF-8 or held in a Rust
  // string, so no text puts an astral character where V8 quotes one code
  // unit of it ("Unexpected token '\ud83e'", which Rust writes as U+FFFD).
  let error = null;
  try { JSON.parse(text); } catch (e) { error = e.message; }
  lines.push(JSON.stringify({ text, error }));
}
process.stdout.write(`${lines.join('\n')}\n`);
