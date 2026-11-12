'use strict';

const path = require('path');
const { chatCompletion, modelInfo } = require('./llm');
const { completionWithRetry } = require('./completion-retry');
const { PROVIDERS } = require('./settings');
const { systemPrompt } = require('./prompt');
const { formatProgramError } = require('./mc-dsl');
const { runProgram } = require('./build-sandbox');
const { resolveShot, renderShots, VIEWS, SIZES } = require('./shot');
const { settle, currentTicks, checkTicks, DEFAULT_SETTLE_TICKS } = require('./shot-world');
const { BUDGETS } = require('./verification-contract');
const store = require('./store');
const sandbox = require('./sandbox');
const storage = require('./storage');
const { contextMessages } = require('./agent-context');
const { supportsImageType } = require('./image-input');
const fs = require('fs');
const { randomUUID } = require('crypto');
const { minecraftVerifier, callTests } = require('./minecraft-verify');
const { TEST_BUILD_TOOL, testBuildTool, MAX_INLINE_BYTES } = require('./verification-contract');
const promptLibrary = require('./prompts');
const DEFAULT_SHOTS = [{ view: 'iso_se' }, { view: 'iso_nw' }];
const MAX_SHOTS = 6;
const MAX_ERROR_CHARS = 8000; // a build error's message and stack, as the model sees it

const BASH_TOOL = {
  type: 'function',
  function: {
    name: 'bash',
    description: 'Run a bash command in your Linux workspace container. The shell session PERSISTS across calls (cwd, env vars, background jobs); it resets if a command times out, floods output, or the workspace sat idle for ~10 minutes — files in /work always survive, so do not rely on env vars or cwd across long gaps. No network by default. User uploads are in /work/attachments/. Python 3 with Pillow and NumPy, ImageMagick, rsvg-convert for SVG, pdftotext, jq, file, and rg are installed. Use it to inspect uploaded files, compute geometry, analyze mc.dump exports, or generate data files for builds to read with mc.readFile.',
    parameters: {
      type: 'object',
      properties: {
        command: {
          type: 'string',
          description: 'The bash command to run. For python, always use a real multi-line heredoc: python3 - <<\'EOF\' (newline) code (newline) EOF — never cram code into python3 -c with \\n escapes. Avoid interactive commands.',
        },
        timeout_ms: {
          type: 'number',
          description: `Maximum run time in milliseconds (default ${sandbox.DEFAULT_TIMEOUT_MS}, max ${sandbox.MAX_TIMEOUT_MS}).`,
        },
        network: {
          type: 'boolean',
          description: 'Run this one command in a separate one-shot shell WITH internet access (persistent session state does not apply there; /work is shared). Use only when needed: pip install --target or venv under /work, downloading a reference. Default false. When downloading, use curl -fsSL (so HTTP errors fail loudly instead of saving an error page), verify with file/identify, and raise timeout_ms beyond the small default. Many sites (e.g. Wikimedia) block non-browser clients; raw.githubusercontent.com works reliably.',
        },
      },
      required: ['command'],
      additionalProperties: false,
    },
  },
};

// Benchmark workers run without network access; the schema then omits the
// one-shot networked mode instead of advertising a parameter that is refused.
function bashTool(networkAccess) {
  if (networkAccess) return BASH_TOOL;
  const tool = structuredClone(BASH_TOOL);
  delete tool.function.parameters.properties.network;
  tool.function.description = tool.function.description.replace('No network by default.', 'No network access.');
  return tool;
}

const COORD = { type: 'array', items: { type: 'number' }, minItems: 3, maxItems: 3 };

const TOOLS = [
  TEST_BUILD_TOOL,
  {
    type: 'function',
    function: {
      name: 'build',
      description: `Run a JavaScript program against the shared voxel world. The program can read and write blocks through the mc API. Applied atomically: if it throws, nothing changes — you may throw deliberately to abandon a program you realize is wrong mid-write, then send a corrected one. mc.log output collected before the throw is still returned. When the program changes the world, the result's minecraft field is Minecraft's verdict on the whole blueprint placed fresh and run settle_ticks ticks (default ${DEFAULT_SETTLE_TICKS}, at most ${BUDGETS.simulationTicks}): how many block ids it removed, added or replaced against your blueprint, with example positions, and the ticks it ran. mc.settled(x, y, z) in a program reads that verdict for one position, for the world as it was when the program started (so not this program's own edits): {id, states}, or null for air, at mc.get's coordinates; mc.settledTicks is the ticks that verdict ran.`,
      parameters: {
        type: 'object',
        properties: {
          summary: {
            type: 'string',
            description: 'Very short human-readable label for this step, e.g. "Build the house shell".',
          },
          program: {
            type: 'string',
            description: 'JavaScript source. Must assign a synchronous function to module.exports, which receives the mc API object.',
          },
          settle_ticks: {
            type: 'integer',
            minimum: 0,
            maximum: BUDGETS.simulationTicks,
            description: `Ticks Minecraft runs on the whole blueprint for the result's minecraft field when the program changes the world (default ${DEFAULT_SETTLE_TICKS}: overworld lava moves a step every 30 ticks, so its 7 steps take 210). 0 is the blueprint as placed. Longer settles take longer to run.`,
          },
        },
        required: ['summary', 'program'],
        additionalProperties: false,
      },
    },
  },
  {
    type: 'function',
    function: {
      name: 'see_build',
      description: `Screenshot the build as Minecraft makes it: the engine places your blocks (shapes, connections, blocks that cannot stay) and runs ticks (water and lava flow, sand falls, scheduled ticks fire), then renders it. A shot's tick is how many ticks the build has run (default: the ticks of the last build's settle; 0 is as placed, at most ${BUDGETS.simulationTicks}); shots at different ticks in one call show several moments of one run. The result says, for each tick shown, which block ids Minecraft removed, added or replaced against your blueprint. Each shot is saved under /work/images (its path is in the result) for bash and view_image. Use test_build for redstone behavior.`,
      parameters: {
        type: 'object',
        properties: {
          shots: {
            type: 'array',
            minItems: 1,
            maxItems: MAX_SHOTS,
            description: `Up to ${MAX_SHOTS} shots (default ${JSON.stringify(DEFAULT_SHOTS)}).`,
            items: {
              type: 'object',
              properties: {
                view: { type: 'string', enum: Object.keys(VIEWS), description: 'Named view of the framed blocks (default iso_se). front looks from the south, back from the north, left from the east, right from the west, top straight down with north up.' },
                eye: { ...COORD, description: 'Your own camera position, with target, instead of view.' },
                target: { ...COORD, description: 'The point eye looks at.' },
                region: { type: 'object', properties: { from: COORD, to: COORD }, required: ['from', 'to'], additionalProperties: false, description: 'Blocks to frame (default: everything drawn). Not with a perspective eye.' },
                clip: { type: 'object', properties: { from: COORD, to: COORD }, required: ['from', 'to'], additionalProperties: false, description: 'Draw only the blocks inside this box: cut away roofs, walls or neighbours to see interiors and sections. To see into a room, end the box inside the roof and inside the walls that face the camera.' },
                projection: { type: 'string', enum: ['perspective', 'orthographic'], description: 'orthographic: parallel projection, true plans and elevations (default perspective).' },
                fov: { type: 'number', description: 'Perspective field of view in degrees, 5 to 120 (default 70). Smaller zooms in.' },
                grid: { type: 'boolean', description: 'Block grid with coordinate labels. Orthographic top, front, back, left or right only.' },
                size: { type: 'string', enum: Object.keys(SIZES), description: `large ${SIZES.large.join('x')} (default) or small ${SIZES.small.join('x')}.` },
                tick: { type: 'integer', minimum: 0, maximum: BUDGETS.simulationTicks, description: 'Ticks the build has run when this shot is taken (default: the last build\'s settle ticks).' },
              },
              additionalProperties: false,
            },
          },
          attach: { type: 'boolean', description: 'false: only save the shots under /work/images, without adding them to your context (default true).' },
        },
        additionalProperties: false,
      },
    },
  },
  BASH_TOOL,
  {
    type: 'function',
    function: {
      name: 'view_image',
      description: 'Look at an image file from the sandbox /work directory. Use after producing or transforming an image with bash to visually inspect the result.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Path relative to /work, e.g. "images/abc123.png" or "palette_preview.png".',
          },
        },
        required: ['path'],
        additionalProperties: false,
      },
    },
  },
];

const VISION_TOOLS = new Set(['see_build', 'view_image']);

// The tools a model is offered; the benchmark's tool contract hashes these.
function toolsFor({ imageInput, placementMode, networkAccess }) {
  return TOOLS.filter((tool) => imageInput || !VISION_TOOLS.has(tool.function.name))
    .map((tool) => tool.function.name === 'test_build' ? testBuildTool(placementMode)
      : tool.function.name === 'bash' ? bashTool(networkAccess) : tool);
}

function toolsForImageInput(imageInput) {
  return toolsFor({ imageInput, placementMode: minecraftVerifier.placementMode, networkAccess: sandbox.networkAccess() });
}

// What Minecraft made of a build (shot-world.js settle), as the model reads
// it: the block ids it removed, added and replaced.
function minecraftReport({ world, changes, ticks }) {
  return { ticks, blocks: world.size(), removed: changes.removed, added: changes.added, replaced: changes.replaced, examples: changes.examples };
}

// The program runs in an isolated process (server/build-sandbox.js). Minecraft
// settles the world it starts from (kept from the last build's settle) for
// mc.settled, and a program that changes the world is settled before it is
// applied, so a build is applied only with Minecraft's verdict on it.
async function executeBuild(conversation, args, blockRegistry) {
  const program = String(args.program || '');
  if (!program.trim()) {
    return { ok: false, error: 'build tool called with an empty program' };
  }
  let settleTicks;
  try {
    settleTicks = args.settle_ticks === undefined ? DEFAULT_SETTLE_TICKS : checkTicks(args.settle_ticks, 'settle_ticks');
  } catch (err) {
    return { ok: false, error: String(err.message || err), note: 'program not run — no blocks or files were changed' };
  }
  const candidate = conversation.world.clone();
  const before = conversation.world;
  const startTicks = currentTicks(before);
  let settledBefore;
  try {
    settledBefore = (await settle(conversation.id, before, [startTicks], { primary: startTicks })).get(startTicks);
  } catch (err) {
    return { ok: false, error: `Minecraft could not settle the current world: ${String(err && err.message || err)}`, note: 'program not run — no blocks or files were changed' };
  }
  let logs;
  let files;
  try {
    ({ logs, files } = await runProgram(program, candidate, blockRegistry, sandbox.makeReadFile(conversation.id), { world: settledBefore.world, ticks: startTicks }));
  } catch (err) {
    const error = formatProgramError(err);
    return {
      ok: false,
      // A huge thrown message would otherwise fill the context for good.
      error: error.length > MAX_ERROR_CHARS ? `${error.slice(0, MAX_ERROR_CHARS)}\n[error message cut at ${MAX_ERROR_CHARS} characters]` : error,
      ...(err?.logs?.length ? { logs: err.logs } : {}),
      note: 'program aborted — no blocks or files were changed',
    };
  }
  // mc.writeFile / mc.dump land in /work only now, same atomicity as blocks:
  // every path is checked before anything is written or applied.
  try {
    for (const file of files) sandbox.resolveWorkPath(conversation.id, file.path, { write: true });
  } catch (err) {
    return { ok: false, error: String(err.message || err), note: 'program aborted — no blocks or files were changed' };
  }
  const diff = candidate.diff(before);
  const changed = diff.added + diff.removed + diff.changed > 0;
  let minecraft;
  if (changed) {
    candidate.finalize();
    try {
      const settled = (await settle(conversation.id, candidate, [settleTicks], { primary: settleTicks })).get(settleTicks);
      minecraft = minecraftReport(settled);
      console.log(`[build] ${candidate.size()} blocks, settle ${Object.entries(settled.timings).map(([name, value]) => `${name} ${typeof value === 'number' ? `${Math.round(value)} ms` : value}`).join(', ')}`);
    } catch (err) {
      return { ok: false, error: `Minecraft could not settle this build: ${String(err && err.message || err)}`, note: 'program aborted — no blocks or files were changed' };
    }
  }
  // What the build writes counts against the chat's storage budget before any
  // of it is applied: its /work files, then the chunk files of its world.
  const fileBytes = files.reduce((sum, file) => sum + Buffer.byteLength(file.text), 0);
  try {
    if (fileBytes) storage.reserve(conversation.id, fileBytes);
    if (changed) {
      try {
        store.persistWorld(conversation.id, candidate);
      } catch (err) {
        if (fileBytes) storage.refund(conversation.id, fileBytes);
        throw err;
      }
    }
  } catch (err) {
    return { ok: false, error: String(err.message || err), note: 'program aborted — no blocks or files were changed' };
  }
  for (const file of files) {
    sandbox.writeWorkFile(conversation.id, file.path, file.text);
  }
  if (changed) conversation.world = candidate;
  return {
    ok: true,
    worldRevision: conversation.world.revision,
    changed,
    diff,
    totalBlocks: candidate.size(),
    bounds: candidate.bounds(),
    ...(minecraft ? { minecraft } : {}),
    logs,
    ...(files.length ? { wrote: files.map((f) => `/work/${f.path.replace(/^\/?work(?:\/|$)/, '')}`) } : {}),
  };
}

async function executeSeeBuild(conversation, args, signal) {
  if (conversation.world.size() === 0) {
    return { ok: false, error: 'The world is empty — nothing to render yet' };
  }
  const specs = args.shots ?? DEFAULT_SHOTS;
  if (!Array.isArray(specs) || specs.length === 0 || specs.length > MAX_SHOTS) return { ok: false, error: `shots must be a list of 1 to ${MAX_SHOTS} shots` };
  if (args.attach !== undefined && typeof args.attach !== 'boolean') return { ok: false, error: 'attach must be true or false' };
  let settled;
  let shots;
  let buffers;
  const started = process.hrtime.bigint();
  const timings = {};
  try {
    // The build as Minecraft makes it (shot-world.js) at each tick asked: the
    // settles the last build made, unless the world changed since, and one
    // run of the engine to the largest tick for the rest.
    const base = currentTicks(conversation.world);
    const cameras = specs.map((spec, index) => {
      if (!spec || typeof spec !== 'object' || Array.isArray(spec)) throw new Error(`shot ${index + 1} must be an object`);
      const { tick, ...camera } = spec;
      try {
        return { camera, tick: tick === undefined ? base : checkTicks(tick, 'tick') };
      } catch (error) {
        throw new Error(`shot ${index + 1}: ${error.message}`);
      }
    });
    settled = await settle(conversation.id, conversation.world, cameras.map((c) => c.tick), { primary: base });
    const made = [...settled.values()].find((result) => !result.cached);
    Object.assign(timings, made ? made.timings : { settle: 'cached' });
    shots = cameras.map(({ camera, tick }, index) => {
      const { world } = settled.get(tick);
      if (world.size() === 0) throw new Error(`shot ${index + 1}: Minecraft keeps none of the blocks of this build at tick ${tick}`);
      try {
        return { ...resolveShot(camera, world), tick };
      } catch (error) {
        throw new Error(`shot ${index + 1}: ${error.message}`);
      }
    });
    buffers = await renderShots(conversation.id, shots, signal, timings);
  } catch (err) {
    return { ok: false, ...(signal?.aborted ? { cancelled: true } : {}), error: String(err && err.message || err) };
  }
  timings.total = Number(process.hrtime.bigint() - started) / 1e6;
  console.log(`[see_build] ${conversation.world.size()} blocks, ${shots.length} shots: ${Object.entries(timings).map(([name, value]) => `${name} ${typeof value === 'number' ? `${Math.round(value)} ms` : value}`).join(', ')}`);
  let images;
  try {
    images = store.saveImages(conversation.id, buffers.map((buffer) => `data:image/jpeg;base64,${buffer.toString('base64')}`)).map((url) => ({ url }));
  } catch (err) {
    return { ok: false, error: String(err.message || err) };
  }
  return {
    ok: true,
    worldRevision: conversation.world.revision,
    attach: args.attach !== false,
    minecraft: [...settled.values()].map(minecraftReport),
    shots: shots.map((shot, i) => ({ image: `/work/images/${path.basename(images[i].url)}`, tick: shot.tick, ...shot.described })),
    images,
  };
}

// A non-zero exit code is information, not a failure: the transcript goes to
// the model as plain terminal text (not JSON-escaped), and only sandbox
// infrastructure problems count as errors.
async function executeBash(conversation, args, signal) {
  const command = String(args.command || '');
  if (!command.trim()) return { ok: false, error: 'bash tool called with an empty command' };
  try {
    sandbox.syncImages(conversation.id, store.IMAGES_DIR);
    sandbox.syncAttachments(conversation.id, store.ATTACHMENTS_DIR);
    const { formatted, storage: full } = await sandbox.exec(conversation.id, command, args.timeout_ms, { network: args.network === true, signal });
    return full ? { ok: false, error: full, output: formatted } : { ok: true, text: formatted };
  } catch (err) {
    return { ok: false, ...(signal?.aborted ? { cancelled: true } : {}), error: String(err && err.message || err) };
  }
}

async function executeViewImage(conversation, args) {
  try {
    const png = await sandbox.viewImage(conversation.id, String(args.path || ''));
    // 'view-' prefix keeps these out of the /work/images mirror: the source
    // file already lives in /work, a normalized copy would just be clutter.
    const [url] = store.saveImages(conversation.id, [png], 'view-');
    return { ok: true, images: [{ angle: String(args.path), url }] };
  } catch (err) {
    return { ok: false, error: String(err && err.message || err) };
  }
}

// A chat keeps at most storage.REPORTS_KEPT test_build reports in
// /work/verification. Before a call adds more, the oldest past that number
// are deleted, but never one a stored message names: those (every report a
// saved tool result returned) stay, within the chat's byte budget.
const REPORT_NAME = /^[0-9a-f-]{36}\.json$/;
// The model can make /work/verification a file, or anything else, so the
// listing's file errors are told in /work terms (sandbox.workError).
async function pruneReports(conversationId) {
  if (!sandbox.workFileExists(conversationId, 'verification')) return;
  const dir = sandbox.resolveWorkPath(conversationId, 'verification');
  let doomed;
  try {
    const names = fs.readdirSync(dir).filter((name) => REPORT_NAME.test(name));
    if (names.length <= storage.REPORTS_KEPT) return;
    doomed = storage.expendable(names, store.reportReferences(conversationId), storage.REPORTS_KEPT,
      (name) => fs.lstatSync(path.join(dir, name)).mtimeMs);
  } catch (error) {
    throw sandbox.workError(error, 'verification');
  }
  if (doomed.length) await sandbox.removeWorkFiles(conversationId, doomed.map((name) => `verification/${name}`));
}

// A test_build call: its tests run on the verifier's lanes side by side (each
// is a level of its own, so a test's result is the same alone or beside
// others), and the results come back in the order the tests were given.
async function executeTestBuild(conversation, args, signal, verifier = minecraftVerifier, onUpdate = () => {}) {
  const failure = (error) => ({ ok: false, ...(signal?.aborted ? { cancelled: true } : {}),
    ...(error.budget ? { code: error.code, budget: error.budget } : {}),
    error: String(error.message || error), note: 'No completed Minecraft verification is available from this call.' });
  let tests;
  try {
    tests = callTests(args);
    // A chat over its storage limits runs no experiments.
    const full = storage.refusal(conversation.id);
    if (full) throw new Error(full);
    await pruneReports(conversation.id);
  } catch (error) {
    return failure(error);
  }
  const messages = tests.map(() => 'Waiting for the Minecraft verifier');
  const results = await Promise.all(tests.map(async (test, index) => {
    try {
      const file = `verification/${randomUUID()}.json`;
      const { reportFile, ...report } = await verifier.run(conversation.world, test, { signal, onUpdate: (update) => {
        messages[index] = update.message;
        onUpdate({ ...update, message: messages.map((message, i) => `Test ${i + 1}: ${message}`).join('; ') });
      } });
      // The report lives on in /work, within the chat's budget; the
      // verifier's archived copy is not kept.
      try {
        storage.reserve(conversation.id, fs.statSync(reportFile).size);
        sandbox.copyIntoWork(conversation.id, file, reportFile);
      } finally { fs.rmSync(reportFile, { force: true }); }
      return { ...report, report: `/work/${file}` };
    } catch (error) {
      return failure(error);
    }
  }));
  const failed = results.flatMap((result, index) => result.ok ? [] : [`test ${index + 1}: ${result.error}`]);
  const call = (tests) => ({ ok: !failed.length, ...(signal?.aborted ? { cancelled: true } : {}), ...(failed.length ? { error: failed.join('\n') } : {}), tests });
  // Past MAX_INLINE_BYTES the observations stay in the files, then the preview and the imports too.
  const trimmed = results.map(({ initial, observations, probes, actions, inputs, sampleTicks, ...summary }) => (
    summary.ok ? { ...summary, observationsInFile: true, probesInFile: true, inputsInFile: true } : summary));
  const summaries = trimmed.map(({ preview, blockStateMigrations, importedEntities, importedBlockEntities, ...rest }) => (
    rest.ok ? { ...rest, previewInFile: true } : rest));
  return [results, trimmed, summaries].map(call).find((output, i, all) => i === all.length - 1 || Buffer.byteLength(JSON.stringify(output)) <= MAX_INLINE_BYTES);
}

// Executes one parsed tool call. Visual tools return a stub result for the
// tool message plus the screenshots that follow it. Benchmark harnesses call
// this same function, so their tool results match the agent loop exactly.
// `chat` offers the user's prompt library too.
async function executeToolCall(conversation, name, args, { blockRegistry, imageInput, signal, onProgress, chat = false }) {
  if (signal?.aborted) return { result: { ok: false, cancelled: true, error: 'Cancelled by user' } };
  if (name === 'prompt_library' && chat) {
    try {
      return { result: { ok: true, prompts: promptLibrary.apply(args, { chat: conversation.id, model: conversation.model }) } };
    } catch (error) {
      return { result: { ok: false, error: error.message } };
    }
  }
  if (name === 'build') return { result: await executeBuild(conversation, args, blockRegistry) };
  if (name === 'test_build') return { result: await executeTestBuild(conversation, args, signal, minecraftVerifier, onProgress) };
  if (!imageInput && VISION_TOOLS.has(name)) return { result: { ok: false, error: 'The selected model does not support image input' } };
  if (name === 'see_build') {
    const { images, attach, ...result } = await executeSeeBuild(conversation, args, signal);
    if (!result.ok) return { result };
    result.note = attach ? `${images.length} screenshot(s) attached in the next message, in shot order` : 'Shots saved under /work/images only (attach: false)';
    return { result, ...(attach ? { screenshots: { text: '[see_build] Screenshots of the current build:', images } } : { images }) };
  }
  if (name === 'bash') return { result: await executeBash(conversation, args, signal) };
  if (name === 'view_image') {
    const full = await executeViewImage(conversation, args);
    if (!full.ok) return { result: full };
    return {
      result: { ok: true, note: 'image attached in the next message' },
      screenshots: { text: `[view_image] /work/${String(args.path || '')}:`, images: full.images },
    };
  }
  return { result: { ok: false, error: `Unknown tool: ${name}` } };
}

// bash transcripts go through as plain text — a JSON envelope would
// turn every newline into \n and make tracebacks unreadable.
function toolMessageContent(name, result) {
  return name === 'bash' && result.ok ? result.text : JSON.stringify(result);
}

// Messages reference images by URL; base64 is inlined only here, per API call.
// Screenshots stay until their tool exchange is archived (agent-context.js):
// what a request sends only grows, so signed thinking and the prompt cache
// stay valid.
function apiMessages(conversation, { imageInput = true } = {}) {
  const messages = [{ role: 'system', content: systemPrompt({ imageInput, resolution: conversation.resolution }) }];
  const history = contextMessages(conversation);
  const inlined = [];
  for (let i = history.length - 1; i >= 0; i -= 1) {
    const msg = history[i];
    if (msg.role === 'error') continue; // infrastructure failures are for the user, not the model
    if (!Array.isArray(msg.content)) {
      inlined.push(msg);
      continue;
    }
    if (!imageInput && msg.seeBuild) {
      inlined.push({
        role: msg.role,
        seeBuild: true,
        content: [{
          type: 'text',
          text: '[Visual tool result omitted because the selected model accepts text input only.]',
        }],
      });
      continue;
    }
    const picked = msg.content.filter((part) => part.type === 'selection').length;
    let order = 0;
    inlined.push({
      role: msg.role,
      ...(msg.seeBuild ? { seeBuild: true } : {}),
      content: msg.content.flatMap((part) => {
        // Regions the user picked in the viewer, in build coordinates,
        // numbered in the order they were picked.
        if (part.type === 'selection') {
          order += 1;
          const { label, min, max, blocks } = part.selection;
          const region = {
            type: 'text',
            text: `[Selection ${order}${picked > 1 ? ` of ${picked}` : ''} (${label}): x ${min[0]}..${max[0]}, y ${min[1]}..${max[1]}, z ${min[2]}..${max[2]}; ${blocks} blocks]`,
          };
          return order === 1 && picked > 1
            ? [{ type: 'text', text: `[The user picked ${picked} regions in the viewer, numbered in the order picked]` }, region]
            : [region];
        }
        if (part.type === 'attachment') {
          const file = part.attachment;
          const label = {
            type: 'text',
            text: `[Uploaded file: ${file.name} | ${file.type} | ${file.size} bytes | ${file.path}]`,
          };
          if (!supportsImageType(file.type) || !imageInput) return [label];
          const image = {
            type: 'image_url',
            image_url: {
              url: `data:${file.type};base64,${store.readAttachment(file.url).toString('base64')}`,
            },
          };
          return [label, image];
        }
        if (part.type === 'image_url' && !imageInput) {
          return [{
            type: 'text',
            text: '[Image content omitted because the selected model accepts text input only.]',
          }];
        }
        // Generated screenshots stay in the image store and are inlined only
        // when constructing the provider request.
        if (part.type !== 'image_url' || !part.image_url.url.startsWith('/api/images/')) return [part];
        const url = part.image_url.url;
        const name = url.split('/').pop();
        const mime = url.endsWith('.jpg') ? 'image/jpeg' : `image/${url.split('.').pop()}`;
        const inlinedPart = { type: 'image_url', image_url: { url: `data:${mime};base64,${store.readImage(url).toString('base64')}` } };
        return name.startsWith('view-') ? [inlinedPart] : [{ type: 'text', text: `[/work/images/${name}]` }, inlinedPart];
      }),
    });
  }
  inlined.reverse();
  for (const msg of inlined) {
    // UI-only fields never reach the provider; `seeBuild` does, to mark a tool
    // result's screenshots.
    const { telemetry, createdAt, notices, plan, rateLimit, round, reply, ...rest } = store.messagePayload(msg);
    messages.push(rest);
  }
  return messages;
}

// Runs one user turn. Emits events through `emit(event)`, calls
// `onWorldChange(conversation)` after every applied build, and mutates the
// conversation in place (messages, world). `signal` aborts the loop between
// steps by throwing its abort; partial work stays applied. Throws on
// unrecoverable errors; the caller ends the turn (store.endTurn).
// `chat` marks a turn in the chat app: only there do builders also get the
// user's prompt library (and harnesses their task-list tools). Benchmark
// attempts leave it off and offer exactly the build tools.
async function runTurn(conversation, blockRegistry, emit, signal, onWorldChange, { chat = false } = {}) {
  let capabilities = null;
  try {
    capabilities = await modelInfo(conversation.provider, conversation.model);
  } catch (error) {
    console.warn(`[model-info] ${conversation.provider}/${conversation.model}: ${error.message}`);
  }
  // Unknown models keep the previous vision-enabled behavior. A known
  // text-only model is gated before its first completion request.
  const imageInput = capabilities?.imageInput !== false;
  const tools = [...toolsForImageInput(imageInput), ...(chat ? [promptLibrary.TOOL] : [])];
  // A harness provider (Claude Code, Codex CLI, Cursor) is a whole agent
  // session that retries its own API requests. Its completion runs once: a
  // second attempt would start the session over and replay the turn.
  const complete = PROVIDERS[conversation.provider].harness
    ? chatCompletion
    : (options) => completionWithRetry(chatCompletion, options, { onRetry: retry => emit({ type: 'completion_retry', ...retry }) });

  for (;;) {
    // The caller records the stop, as for one during a completion.
    signal.throwIfAborted();

    const message = await complete({
      conversationId: conversation.id,
      conversation, // harness providers resume their own session from the transcript's head
      chat,
      provider: conversation.provider,
      model: conversation.model,
      effort: conversation.effort || undefined,
      messages: apiMessages(conversation, { imageInput }),
      tools,
      signal,
      onDelta: (delta) => {
        if (delta.content) emit({ type: 'assistant_delta', text: delta.content });
        if (delta.reasoning) emit({ type: 'reasoning_delta', text: delta.reasoning });
        if (delta.toolCall) emit({ type: 'tool_call_delta', ...delta.toolCall });
        // Vendor harnesses also report activity, notices and a task list.
        if ('status' in delta) emit({ type: 'harness_status', text: delta.status });
        if (delta.notice) emit({ type: 'notice', ...delta.notice });
        if (delta.plan) emit({ type: 'plan', items: delta.plan });
      },
    });

    const toolCalls = message.tool_calls || [];
    store.appendMessage(conversation, {
      role: 'assistant',
      content: message.content || '',
      ...(message.reasoning ? { reasoning: message.reasoning } : {}),
      ...(message.reasoning_details ? { reasoning_details: message.reasoning_details } : {}),
      ...(message.finish_reason ? { finish_reason: message.finish_reason } : {}),
      ...(message.telemetry ? { telemetry: message.telemetry } : {}),
      ...(message.notices ? { notices: message.notices } : {}),
      ...(message.plan ? { plan: message.plan } : {}),
      ...(message.rateLimit ? { rateLimit: message.rateLimit } : {}),
      ...(toolCalls.length ? { tool_calls: toolCalls } : {}),
    });

    if (message.reasoning) {
      emit({ type: 'reasoning', text: message.reasoning });
    }

    if (toolCalls.length === 0) {
      emit({ type: 'assistant', text: message.content || '', final: true });
      return;
    }

    if (message.content) {
      emit({ type: 'assistant', text: message.content, final: false });
    }

    store.save(conversation);
    const screenshotBatches = [];
    let answered = 0; // calls of this batch with a result, in call order
    const runCall = async (call) => {
      const toolStarted = Date.now();
      const name = call.function && call.function.name;
      let args = null;
      let result = null;
      try {
        args = JSON.parse(call.function.arguments || '{}');
        if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('Tool arguments must be an object');
      } catch (err) {
        result = { ok: false, error: `Invalid tool arguments JSON: ${err.message}` };
      }

      // Every call gets a full step lifecycle (start -> result), including
      // failures — otherwise the step is stuck "writing" in the UI and its
      // error surfaces only at end-of-turn resync.
      const summary = result ? `${name || 'tool'} (invalid arguments)`
        : name === 'build' ? String(args.summary || 'build')
          : name === 'test_build' ? `Test build in Minecraft${Array.isArray(args.tests) && args.tests.length > 1 ? ` (${args.tests.length} tests)` : ''}`
          : name === 'see_build' ? `Look at build (${(Array.isArray(args.shots) ? args.shots : DEFAULT_SHOTS).map((shot) => shot?.view || (shot?.eye ? 'camera' : 'iso_se')).join(', ')})`
            : name === 'bash' ? `$ ${String(args.command || '').split('\n')[0].slice(0, 80)}`
              : name === 'view_image' ? `Look at /work/${String(args.path || '')}`
                : name === 'prompt_library' ? (args.save || args.remove ? 'Update prompt library' : 'Read prompt library')
                  : String(name || 'tool');
      emit({
        type: 'tool_start',
        callId: call.id,
        name,
        summary,
        program: args && (name === 'test_build' ? JSON.stringify(args, null, 2) : args.program ?? args.command),
      });

      let shownImages;
      if (!result) {
        const executed = await executeToolCall(conversation, name, args, { blockRegistry, imageInput, signal, chat,
          onProgress: progress => emit({ type: 'tool_progress', callId: call.id, name, ...progress }) });
        result = executed.result;
        if (executed.screenshots) screenshotBatches.push(executed.screenshots);
        shownImages = executed.screenshots?.images || executed.images;
      }

      emit({
        type: 'tool_result',
        callId: call.id,
        name,
        ok: result.ok,
        cancelled: result.cancelled,
        error: result.error,
        diff: result.diff,
        totalBlocks: result.totalBlocks,
        output: name === 'bash' ? result.text ?? result.output : name === 'test_build' || name === 'prompt_library' ? JSON.stringify(result, null, 2) : undefined,
        images: (name === 'see_build' || name === 'view_image') && result.ok
          ? shownImages.map((img) => img.url)
          : undefined,
      });
      if (name === 'build' && result.ok && result.changed) onWorldChange(conversation);

      store.appendMessage(conversation, {
        role: 'tool',
        tool_call_id: call.id,
        telemetry: { name, startedAt: new Date(toolStarted).toISOString(), wallMs: Date.now() - toolStarted, ok: result.ok, cancelled: result.cancelled === true },
        content: toolMessageContent(name, result),
      });
      answered += 1;
      store.save(conversation);
    };

    // A call that throws fails the turn, and the caller stores how
    // (store.endTurn). The batch is completed first, in the order every
    // provider needs: a result for each call left unanswered, then the
    // screenshots that results already stored say are attached in the next
    // message.
    let failure = null;
    try {
      for (const call of toolCalls) await runCall(call);
    } catch (error) {
      failure = error;
      const unanswered = store.unansweredResult(signal.aborted, String(error && error.message || error));
      for (const call of toolCalls.slice(answered)) {
        store.appendMessage(conversation, { role: 'tool', tool_call_id: call.id, content: JSON.stringify(unanswered) });
      }
    }

    for (const batch of screenshotBatches) {
      store.appendMessage(conversation, {
        role: 'user',
        seeBuild: true,
        content: [
          { type: 'text', text: batch.text },
          ...batch.images.map((img) => ({ type: 'image_url', image_url: { url: img.url } })),
        ],
      });
    }
    // Not saved here: endTurn saves these with the error, and a save that
    // threw has dropped the chat, so its error would be lost to another.
    if (failure) throw failure;

    // Persist after every step. Build side effects (world mutations, /work
    // files, saved screenshots) already landed on disk; a mid-turn crash must
    // not lose the messages that reference them. The turn survives page
    // reloads, so this is also what a reattaching client resyncs against.
    store.save(conversation);
  }
}

module.exports = {
  runTurn,
  executeToolCall,
  toolMessageContent,
  toolsForImageInput,
  toolsFor,
  _internal: { apiMessages, toolsForImageInput, executeTestBuild, executeBuild, executeSeeBuild, executeBash, pruneReports },
};
