#!/usr/bin/env node
'use strict';
// Writes the text of mcbench-agent's tool contract (the tool schemas, the
// system prompt, the facts that choose between them) from today's server/, by
// running it. Every file beside this script except this script is its output:
// never edit one by hand; change server/ and run this again.
//
//   node generate.js --root <checkout> --bench <folder>            writes the files beside this script
//   node generate.js --root <checkout> --bench <folder> --check    regenerates into a temporary folder and
//                                                 fails unless every file beside this script is identical
//   node generate.js --root <checkout> --bench <folder> --dump     prints, as JSON, what server/ gives under
//                                                 this process's MCBENCH_VERIFY_MAX_* (the Rust tests' Node side)
//
// <checkout> is a checkout whose server/ and node_modules load (server/ needs its
// dependencies); it defaults to the one holding this script. <folder> is the
// private benchmark's folder, required: the contract records the benchmark's
// import order revision and its unrestricted study's budgets, which are its
// modules' (the import order module and the tool contract module of its batch
// runner). Loading server/
// opens a store, so every load runs with MCBENCH_DATA_DIR set to a fresh
// temporary folder, which is removed afterwards.
//
// The budgets. The schemas and the prompt quote the verification budgets
// (BUDGETS, read from MCBENCH_VERIFY_MAX_* when server/ loads), raw or divided
// (`${BUDGETS.wallMs / 1000}`). The files hold them as holes, ⟦key⟧ or
// ⟦key/divisor⟧, which Rust fills with JavaScript's Number::toString of
// BUDGETS[key] / divisor. Where the holes are is found, not written down: the
// server is loaded under sentinel budgets chosen so that every candidate
// expression (each budget, alone or divided by 1000 or by 1 MiB) prints as text
// found nowhere else, each place an expression's text appears becomes its hole,
// and the templates must then reproduce, filled, what the server gives under
// other sentinel budgets, the default budgets and the unrestricted study's
// budgets (LIFTED of the benchmark's tool contract module). A budget quoted any
// other way fails that check.
//
// The files:
//   manifest.json                    the order of the tools, which are vision tools, what each
//                                    varies by, the chat tools, the resolutions, IMPORT_ORDER, LIFTED
//   tools/<name>[.<variant>].json    JSON.stringify of one tool (agent.js toolsFor, prompts.js TOOL)
//   prompt/<image|text>.<network|offline>.txt     systemPrompt at the standard resolution
//   prompt/resolution-<value>.<image|text>.txt    what systemPrompt adds after "\n\n" at another
//   texts/title-system.txt, texts/title-request.txt   the title model's system message and the
//                                    request quoting the chat's first message as ⟦message⟧
//                                    (index.js generateTitle, captured from a run)
//   texts/prompt-generation.txt      Improve with AI's instructions (prompt-generation.js)
//   files.rs                         the table of these files for include_str!

const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { execFileSync } = require('node:child_process');

const HERE = __dirname;
const PLACEMENTS = ['updates', 'strict'];
const DIVISORS = [1, 1000, 1024 * 1024];
const OPEN = '⟦', CLOSE = '⟧';

// The benchmark's folder (--bench), for this process and the ones it starts.
let bench = null;

function parseArgs() {
  const argv = process.argv.slice(2);
  const options = { root: path.resolve(HERE, '../../../..'), bench: null, mode: 'write' };
  for (let i = 0; i < argv.length; i += 1) {
    if (argv[i] === '--root') options.root = path.resolve(argv[++i]);
    else if (argv[i] === '--bench') options.bench = path.resolve(argv[++i]);
    else if (argv[i] === '--check') options.mode = 'check';
    else if (argv[i] === '--dump') options.mode = 'dump';
    else if (argv[i] === '--texts') { options.mode = 'texts'; options.message = argv[++i]; }
    else if (argv[i] === '--out') options.out = path.resolve(argv[++i]);
    else throw new Error(`generate.js: unknown argument ${argv[i]}`);
  }
  if (!options.bench) {
    throw new Error('generate.js needs --bench <folder>: the private benchmark\'s folder, whose import order and tool contract modules give the contract\'s import order revision and the unrestricted study\'s budgets');
  }
  return options;
}

const combo = (placement, image, network) => `${placement}/${image ? 'image' : 'text'}/${network ? 'network' : 'offline'}`;

// What server/ gives in this process, whose environment holds the budgets.
function dump(root) {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-contract-'));
  process.env.MCBENCH_DATA_DIR = dataDir;
  try {
    let loaded;
    try {
      loaded = {
        agent: require(path.join(root, 'server/agent')),
        prompt: require(path.join(root, 'server/prompt')),
        contract: require(path.join(root, 'server/verification-contract')),
        prompts: require(path.join(root, 'server/prompts')),
      };
    } catch (error) {
      // readBudgets refuses a budget while server/ loads.
      return { ok: false, error: error.message };
    }
    const { agent, prompt, contract, prompts } = loaded;
    const { RESOLUTIONS } = require(path.join(root, 'web/resolution.mjs'));
    const { IMPORT_ORDER } = require(path.join(bench, 'import-order'));
    const { LIFTED } = require(path.join(bench, 'batch/tool-contract'));
    const out = {
      ok: true, budgets: JSON.stringify(contract.BUDGETS), importOrder: IMPORT_ORDER, lifted: LIFTED,
      resolutions: RESOLUTIONS.map(stop => stop.value), promptLibrary: JSON.stringify(prompts.TOOL),
      toolTexts: {}, tools: {}, prompts: {},
    };
    for (const placement of PLACEMENTS) for (const image of [true, false]) for (const network of [true, false]) {
      const tools = agent.toolsFor({ imageInput: image, placementMode: placement, networkAccess: network });
      const key = combo(placement, image, network);
      out.toolTexts[key] = tools.map(tool => [tool.function.name, JSON.stringify(tool)]);
      out.tools[key] = JSON.stringify(tools);
      out.tools[`${key}/chat`] = JSON.stringify([...tools, prompts.TOOL]);
    }
    for (const image of [true, false]) for (const network of [true, false]) for (const resolution of out.resolutions) {
      out.prompts[`${image ? 'image' : 'text'}/${network ? 'network' : 'offline'}/${resolution}`] = prompt.systemPrompt({ imageInput: image, resolution, networkAccess: network });
    }
    try {
      prompt.systemPrompt({ imageInput: true, resolution: 3, networkAccess: false });
      throw new Error('generate.js: systemPrompt took resolution 3');
    } catch (error) {
      out.resolutionError = error.message;
    }
    return out;
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true });
  }
}

// The other texts models read, from a run of server/: the title request
// (index.js generateTitle, captured from the completion a chat's first
// message starts, with `message` as that message) and Improve with AI's
// instructions (prompt-generation.js).
async function texts(root, message) {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-contract-texts-'));
  process.env.MCBENCH_DATA_DIR = dataDir;
  process.env.MCBENCH_SETTINGS_FILE = path.join(dataDir, 'settings.json');
  const { Readable } = require('node:stream');
  const { EventEmitter } = require('node:events');
  const llm = require(path.join(root, 'server/llm'));
  let titled;
  const seen = new Promise((resolve) => { titled = resolve; });
  llm.chatCompletion = async (options) => {
    if (options.tools.length) return { content: 'Done.' };
    titled(options);
    return { content: 'A title' };
  };
  llm.modelInfo = async () => ({ imageInput: true });
  require(path.join(root, 'server/assets')).getBlockRegistry = async () => ({});
  const sandbox = require(path.join(root, 'server/sandbox'));
  sandbox.reset = async () => {};
  sandbox.syncAttachments = () => {};
  const app = require(path.join(root, 'server/index'));
  const call = (method, pathname, body) => {
    const req = Readable.from(body === undefined ? [] : [Buffer.from(JSON.stringify(body))]);
    Object.assign(req, { method, url: pathname, headers: {} });
    const res = new EventEmitter();
    res.writeHead = () => {};
    res.write = () => true;
    res.end = (payload) => { if (payload) res.body = JSON.parse(payload); res.emit('close'); };
    return app.handleApi(req, res, new URL(pathname, 'http://app.invalid')).then(() => res);
  };
  const { body } = await call('POST', '/api/conversations', { provider: 'openrouter', model: 'm' });
  await call('POST', `/api/conversations/${body.id}/chat`, { provider: 'openrouter', model: 'm', resolution: 1, text: message });
  const options = await seen;
  const out = {
    titleSystem: options.messages[0].content,
    titleRequest: options.messages[1].content,
    promptGeneration: require(path.join(root, 'server/prompt-generation')).INSTRUCTIONS,
  };
  fs.rmSync(dataDir, { recursive: true, force: true });
  return out;
}

function textsUnder(root, message) {
  return JSON.parse(execFileSync(process.execPath, [__filename, '--texts', message, '--root', root, '--bench', bench], { encoding: 'utf8', maxBuffer: 1 << 26 }));
}

function dumpUnder(root, budgets) {
  const env = { ...process.env };
  for (const name of Object.keys(env)) if (name.startsWith('MCBENCH_VERIFY_MAX_')) delete env[name];
  Object.assign(env, budgets);
  const out = JSON.parse(execFileSync(process.execPath, [__filename, '--dump', '--root', root, '--bench', bench], { encoding: 'utf8', maxBuffer: 1 << 26, env }));
  if (!out.ok) throw new Error(`generate.js: server/ refused the budgets ${JSON.stringify(budgets)}: ${out.error}`);
  return out;
}

// The budget environment variables and their keys, from server/.
function budgetNames(root) {
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-contract-'));
  process.env.MCBENCH_DATA_DIR = dataDir;
  try {
    return require(path.join(root, 'server/verification-contract')).BUDGET_ENV;
  } finally {
    fs.rmSync(dataDir, { recursive: true, force: true });
  }
}

// sfc32: sentinel budgets are the same on every run.
function rng(seed) {
  let a = 0x9e3779b9, b = 0x243f6a88, c = 0xb7e15162, d = seed >>> 0;
  return () => {
    a >>>= 0; b >>>= 0; c >>>= 0; d >>>= 0;
    const t = (a + b | 0) + d | 0;
    d = d + 1 | 0; a = b ^ b >>> 9; b = c + (c << 3) | 0; c = c << 21 | c >>> 11; c = c + t | 0;
    return t >>> 0;
  };
}

const candidates = values => Object.entries(values).flatMap(([key, value]) => DIVISORS.map(divisor => ({
  hole: divisor === 1 ? `${OPEN}${key}${CLOSE}` : `${OPEN}${key}/${divisor}${CLOSE}`, text: String(value / divisor),
})));

// Sentinel budgets: every candidate's text distinct, none inside another,
// none in `texts` (what server/ says under the default budgets).
function sentinels(names, seed, texts) {
  const next = rng(seed);
  for (let attempt = 0; attempt < 1000; attempt += 1) {
    const values = Object.fromEntries(Object.keys(names).map(key => [key, 1_000_000_007 + next() % 1_000_000_000]));
    const list = candidates(values).map(c => c.text);
    const clash = list.some((t, i) => list.some((u, j) => i !== j && u.includes(t)) || texts.some(s => s.includes(t)));
    if (!clash) return values;
  }
  throw new Error('generate.js: no sentinel budgets found');
}

const envOf = (names, values) => Object.fromEntries(Object.entries(values).map(([key, value]) => [names[key], String(value)]));

// `text` with each candidate's text replaced by its hole.
function templatize(text, values) {
  if (text.includes(OPEN) || text.includes(CLOSE)) throw new Error(`generate.js: server/'s text holds ${OPEN} or ${CLOSE}`);
  const found = [];
  for (const { hole, text: needle } of candidates(values)) {
    for (let at = text.indexOf(needle); at >= 0; at = text.indexOf(needle, at + 1)) {
      const before = text[at - 1], after = text[at + needle.length];
      if (/[0-9.]/.test(before ?? '') || /[0-9.]/.test(after ?? '')) throw new Error(`generate.js: ${needle} (${hole}) appears inside a longer number`);
      found.push({ at, end: at + needle.length, hole });
    }
  }
  found.sort((x, y) => x.at - y.at);
  let out = '', last = 0;
  for (const { at, end, hole } of found) {
    if (at < last) throw new Error(`generate.js: holes overlap at ${at}`);
    out += text.slice(last, at) + hole;
    last = end;
  }
  return out + text.slice(last);
}

// A template with its holes filled from `budgets` (JSON.stringify(BUDGETS)),
// as the Rust side fills it; null when it holds a hole the budgets do not.
function fill(template, budgets) {
  const values = JSON.parse(budgets);
  return template.replace(new RegExp(`${OPEN}([A-Za-z]+)(?:/([0-9]+))?${CLOSE}`, 'g'), (_, key, divisor) => {
    if (!(key in values)) throw new Error(`generate.js: no budget ${key}`);
    return String(values[key] / Number(divisor ?? 1));
  });
}

function generate(root, outDir) {
  const names = budgetNames(root);
  const standard = dumpUnder(root, {});
  const defaultTexts = [...Object.values(standard.tools), ...Object.values(standard.prompts)];
  const a = sentinels(names, 1, defaultTexts);
  const b = sentinels(names, 2, defaultTexts);
  const dumpA = dumpUnder(root, envOf(names, a));
  const checks = [standard, dumpUnder(root, envOf(names, b)), dumpUnder(root, standard.lifted)];
  const files = new Map();
  const put = (name, template, of) => {
    for (const other of checks) {
      if (fill(template, other.budgets) !== of(other)) throw new Error(`generate.js: ${name} filled with ${other.budgets} is not what server/ gives`);
    }
    if (files.has(name) && files.get(name) !== template) throw new Error(`generate.js: two texts for ${name}`);
    files.set(name, template);
  };

  // The tools: the order, the vision tools, and what each varies by.
  const keys = Object.keys(dumpA.toolTexts);
  const order = dumpA.toolTexts[combo('updates', true, false)].map(([name]) => name);
  // The parts of a combination's key a tool's text may vary by (image input
  // only chooses whether a vision tool is offered).
  const axes = { placement: 0, network: 2 };
  const part = (k, axis) => k.split('/')[axes[axis]];
  const manifestTools = order.map(name => {
    const where = keys.filter(k => dumpA.toolTexts[k].some(([n]) => n === name));
    const vision = where.length !== keys.length;
    if (vision && where.some(k => k.split('/')[1] !== 'image')) throw new Error(`generate.js: ${name} is offered with and without image input only in part`);
    const textOf = (d, k) => d.toolTexts[k].find(([n]) => n === name)[1];
    // An axis the text changes with; put() then requires the same text for
    // every combination that agrees on these axes.
    const onlyIn = (k, j, part) => k.split('/').every((v, i) => (i === part) !== (v === j.split('/')[i]));
    const by = Object.keys(axes).filter(axis => where.some(k => where.some(j => onlyIn(k, j, axes[axis])
      && textOf(dumpA, k) !== textOf(dumpA, j))));
    for (const k of where) {
      const file = `tools/${[name, ...by.map(axis => part(k, axis))].join('.')}.json`;
      put(file, templatize(textOf(dumpA, k), a), d => textOf(d, k));
    }
    return { name, vision, by };
  });
  for (const k of keys) {
    const expected = order.filter(name => dumpA.toolTexts[k].some(([n]) => n === name));
    if (dumpA.toolTexts[k].map(([n]) => n).join() !== expected.join()) throw new Error(`generate.js: the tools of ${k} are not in toolsFor's order`);
    for (const d of [dumpA, ...checks]) {
      if (d.tools[k] !== `[${d.toolTexts[k].map(([, t]) => t).join(',')}]`) throw new Error(`generate.js: the tools of ${k} are not their texts joined`);
      if (d.tools[`${k}/chat`] !== `[${[...d.toolTexts[k].map(([, t]) => t), d.promptLibrary].join(',')}]`) throw new Error(`generate.js: the chat tools of ${k} are not the tools and prompt_library`);
    }
  }
  put('tools/prompt_library.json', templatize(dumpA.promptLibrary, a), d => d.promptLibrary);

  // The system prompt: the standard one per image input and network access,
  // and the section another resolution adds, per image input.
  const [standardResolution, ...others] = dumpA.resolutions;
  for (const image of ['image', 'text']) for (const network of ['network', 'offline']) {
    const base = `${image}/${network}/${standardResolution}`;
    put(`prompt/${image}.${network}.txt`, templatize(dumpA.prompts[base], a), d => d.prompts[base]);
    for (const resolution of others) {
      const key = `${image}/${network}/${resolution}`;
      const sectionOf = d => {
        const head = `${d.prompts[base]}\n\n`;
        if (!d.prompts[key].startsWith(head)) throw new Error(`generate.js: the prompt at resolution ${resolution} does not start with the standard prompt`);
        return d.prompts[key].slice(head.length);
      };
      put(`prompt/resolution-${resolution}.${image}.txt`, templatize(sectionOf(dumpA), a), sectionOf);
    }
  }

  // The other texts. The title request quotes the chat's first message,
  // held as the hole ⟦message⟧, which Rust fills with the message's text;
  // the template must give what server/ gives for a second message.
  const MESSAGE = `${OPEN}message${CLOSE}`;
  const first = textsUnder(root, MESSAGE);
  const second = textsUnder(root, 'Two lines\nof a message');
  if (first.titleRequest.split(MESSAGE).join('Two lines\nof a message') !== second.titleRequest
    || first.titleSystem !== second.titleSystem || first.promptGeneration !== second.promptGeneration) {
    throw new Error('generate.js: the title request does not quote the message where its hole is');
  }
  files.set('texts/title-system.txt', first.titleSystem);
  files.set('texts/title-request.txt', first.titleRequest);
  files.set('texts/prompt-generation.txt', first.promptGeneration);

  const manifest = {
    importOrder: dumpA.importOrder, lifted: dumpA.lifted, budgetEnv: names, tools: manifestTools, chatTools: ['prompt_library'],
    resolutions: dumpA.resolutions, standardResolution, resolutionError: standard.resolutionError,
  };
  files.set('manifest.json', `${JSON.stringify(manifest, null, 2)}\n`);
  const table = [...files.keys(), 'manifest.json'].filter((n, i, all) => all.indexOf(n) === i).sort();
  files.set('files.rs', `// Generated by generate.js from server/; do not edit.\n/// Every generated file of the contract, by its name.\npub(crate) const FILES: &[(&str, &str)] = &[\n${table.map(n => `    (${JSON.stringify(n)}, include_str!(${JSON.stringify(n)})),`).join('\n')}\n];\n`);

  for (const [name, content] of files) {
    fs.mkdirSync(path.dirname(path.join(outDir, name)), { recursive: true });
    fs.writeFileSync(path.join(outDir, name), content);
  }
  return [...files.keys()].sort();
}

function listFiles(dir) {
  const out = [];
  const walk = sub => {
    for (const entry of fs.readdirSync(path.join(dir, sub), { withFileTypes: true })) {
      const name = sub ? `${sub}/${entry.name}` : entry.name;
      if (entry.isDirectory()) walk(name);
      else if (name !== 'generate.js') out.push(name);
    }
  };
  walk('');
  return out.sort();
}

function main() {
  const options = parseArgs();
  bench = options.bench;
  if (options.mode === 'dump') {
    process.stdout.write(JSON.stringify(dump(options.root)));
    return;
  }
  if (options.mode === 'texts') {
    // server/index.js leaves its timers and engines behind: the process ends here.
    texts(options.root, options.message).then((out) => process.stdout.write(JSON.stringify(out), () => process.exit(0)),
      (error) => { console.error(error); process.exit(1); });
    return;
  }
  if (options.mode === 'write') {
    const written = generate(options.root, options.out ?? HERE);
    console.log(`generate.js: wrote ${written.length} files`);
    return;
  }
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'mcbench-contract-check-'));
  try {
    generate(options.root, tmp);
    const ours = listFiles(HERE), fresh = listFiles(tmp);
    const differ = [...new Set([...ours, ...fresh])].filter(name => !ours.includes(name) || !fresh.includes(name)
      || !fs.readFileSync(path.join(HERE, name)).equals(fs.readFileSync(path.join(tmp, name))));
    if (differ.length) {
      console.error(`generate.js --check: these files differ from what server/ gives: ${differ.join(', ')}`);
      process.exitCode = 1;
    } else {
      console.log(`generate.js --check: ${ours.length} files identical`);
    }
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

main();
