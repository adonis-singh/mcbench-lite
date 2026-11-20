#!/usr/bin/env node
'use strict';

// A stand-in `codex` for the differential test: `debug models` prints a
// synthetic catalogue in the shape codex-cli 0.169.0 prints (the fields
// codex-cli.js modelsFromCache reads), anything else fails.

const args = process.argv.slice(2).join(' ');
if (args !== 'debug models') {
  process.stderr.write(`fake codex: unexpected arguments ${args}\n`);
  process.exit(2);
}
process.stdout.write(JSON.stringify({ models: [
  { slug: 'gpt-7-nova', display_name: 'GPT-7 Nova', visibility: 'list', priority: 2, supported_reasoning_levels: [{ effort: 'low' }, { effort: 'medium' }, { effort: 'high' }, { effort: 'xhigh' }], default_reasoning_level: 'medium', input_modalities: ['text', 'image'], context_window: 400000, additional_speed_tiers: ['fast'], service_tiers: [{ name: 'Fast', description: 'Twice the speed, twice the usage' }] },
  { slug: 'gpt-6-astra', display_name: 'GPT-6 Astra', visibility: 'list', priority: 1, supported_reasoning_levels: [{ effort: 'high' }], default_reasoning_level: 'high', input_modalities: ['text'], context_window: 272000 },
  { slug: 'gpt-6-mini', display_name: 'GPT-6 Mini', visibility: 'list', priority: 3, input_modalities: ['text', 'image'] },
  { slug: 'codex-internal', display_name: 'Internal', visibility: 'hide', priority: 0 },
] }));
