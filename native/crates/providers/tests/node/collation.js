'use strict';

// Writes tests/collation.json: names and the sign of a.localeCompare(b) for
// every pair, as Node 24 (full ICU, its default locale) answers, for
// src/catalog.rs's model-list order (llm.js sorts ties of `created` by
// name.localeCompare).
//
//   node collation.js > ../collation.json

const names = [
  '', ' ', 'a', 'A', 'b', 'B', 'ab', 'a b', 'a-b', 'a_b', 'a.b', 'a,b', 'a/b', 'a(b', 'a:b',
  'e', 'é', 'E', 'É', 'f', 'ß', 'ss', 'SS', 'Æther', 'aether', 'ﬁ', 'fi', 'ı', 'i', 'İ',
  '1', '9', '10', '1.5', '01', '½', '²',
  'GPT-5.6', 'gpt-5.6', 'GPT 5.6', 'GPT-5.10', 'GPT-5.6 Mini', 'GPT-5.6-mini', 'GPT‑5.6',
  'Claude Opus 5.5', 'claude-opus-5-5', 'Claude Sonnet 4.6', 'Anthropic: Claude Opus 5.5',
  'Gemini 2.5 Flash', 'Gemini 2.5 Flash-Lite', 'Gemini 3.8 Flash', 'Nano Banana 2',
  'Qwen3.8 Max', 'Qwen3.8 Max (0902)', 'Qwen3.8 2.4T A95B', 'o3', 'O3', 'o3-pro',
  '_under', '(paren)', '#hash', '@at', '~tilde', '🧱 Brick', 'Brick', '通义千问', 'こんにちは', 'Яндекс', 'Ёлка', 'Елка',
  'عربي', 'עברית', 'ไทย', 'Ω', 'ω', 'Zeta', 'zeta', 'Ångström', 'Angstrom', 'naïve', 'naive', 'Ꭰ', '́a', 'á', 'á',
];
const signs = names.map((a) => names.map((b) => '-0+'[Math.sign(a.localeCompare(b)) + 1]).join(''));
process.stdout.write(`${JSON.stringify({ locale: Intl.DateTimeFormat().resolvedOptions().locale, icu: process.versions.icu, names, signs }, null, 1)}\n`);
