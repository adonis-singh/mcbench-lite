'use strict';

// What the first start prepares, one step per thing, for GET /api/setup
// (index.js) and the page's setup screen. Each step is marked by the code
// that does its work (track), so a later start, which finds everything in
// place, marks every step done within seconds. The terminal's lines are
// progress.js's.
//
// A step's `title` names what it makes and `doing` says what its work does,
// in the words the page shows while it runs.
//
// A step's `state` is one of
//   waiting      its work has not started (it may wait for another step)
//   running      its work runs: `detail` may say what it does now, and
//                `progress` ({ done, total } bytes, total null when the
//                server did not say) how far its download is
//   done         its work is done (`seconds`, how long it took)
//   failed       its work failed (`error`): the work is tried again when
//                it is next needed
//   unavailable  it cannot be done on this machine as it is (`error` says
//                what is missing): only an optional step is ever this

const { httpError } = require('./http-guard');

const STEPS = [
  { id: 'jdk', title: 'Java', doing: 'Downloading Java', optional: false },
  { id: 'rust', title: 'Rust', doing: 'Installing Rust', optional: false },
  { id: 'minecraft', title: 'Minecraft server', doing: 'Downloading the Minecraft server', optional: false },
  { id: 'tables', title: "Minecraft's block tables", doing: "Reading Minecraft's block tables", optional: false },
  { id: 'blocks', title: 'Block data for the page', doing: 'Downloading block data for the page', optional: false },
  { id: 'viewer', title: '3D viewer', doing: 'Preparing the 3D viewer', optional: false },
  { id: 'engine', title: 'Minecraft engine', doing: 'Compiling the Minecraft engine', optional: false },
  { id: 'wasm', title: "The page's engine", doing: "Compiling the page's engine", optional: false },
  { id: 'browser', title: 'Screenshot browser', doing: 'Downloading the screenshot browser', optional: false },
  { id: 'shell', title: "Model's shell", doing: "Building the model's shell image", optional: true },
];

let steps = new Map();
// The work of every step (start), or null before the server gave it.
let prepare = null;
function reset() {
  prepare = null;
  steps = new Map(STEPS.map((step) => [step.id, { ...step, state: 'waiting' }]));
}
reset();

function get(id) {
  const step = steps.get(id);
  if (!step) throw new Error(`No setup step ${id}`);
  return step;
}

// Sets fields of a running step: its `detail` or `progress`.
function report(id, fields) {
  const step = get(id);
  if (step.state === 'running') Object.assign(step, fields);
}

// A download's progress callback (tools.js download) for step `id`.
function progress(id) {
  return (bytes) => report(id, { progress: bytes });
}

// Runs `work()` as step `id` and resolves or rejects as it does. It is
// called at once, before this returns, as a caller that keeps its promise
// expects. A step that is done stays done: work that finds everything in
// place, asked for again, changes nothing here. A failure that
// `unavailable(error)` says is about the machine makes the step
// unavailable instead of failed.
function track(id, work, { unavailable = () => false } = {}) {
  const step = get(id);
  const run = () => {
    try {
      return Promise.resolve(work());
    } catch (error) {
      return Promise.reject(error);
    }
  };
  if (step.state === 'done') return run();
  const started = Date.now();
  Object.assign(step, { state: 'running', detail: undefined, progress: undefined, seconds: undefined, error: undefined });
  return run().then((value) => {
    Object.assign(step, { state: 'done', detail: undefined, progress: undefined, seconds: Math.round((Date.now() - started) / 100) / 10 });
    return value;
  }, (error) => {
    const message = String(error.stderr || error.message || error).trim().slice(-2000);
    Object.assign(step, { state: unavailable(error) ? 'unavailable' : 'failed', progress: undefined, error: message });
    throw error;
  });
}

// The work of every step, given once by the server (index.js prepare),
// which runs it at its start. retry() runs it again after a step failed
// (POST /api/setup/retry): each step's work is kept once it worked, so what
// is done stays done, and a failed step runs again, with the steps that
// waited on it. Answers the steps as they are then.
function start(work) {
  if (prepare) throw new Error('The setup steps were started already');
  prepare = work;
  work();
}

function retry() {
  if (!view().steps.some((step) => step.state === 'failed')) throw httpError(409, 'No setup step failed');
  prepare();
  return view();
}

// The document GET /api/setup answers: every step in order, and `ready`,
// true once every step that is not optional is done.
function view() {
  const list = [...steps.values()].map((step) => JSON.parse(JSON.stringify(step)));
  return { ready: list.every((step) => step.optional || step.state === 'done'), steps: list };
}

module.exports = { track, report, progress, view, start, retry, reset, STEPS };
