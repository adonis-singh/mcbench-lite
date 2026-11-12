'use strict';

// One terminal line when a step of preparing the app starts to take a while,
// and one when it is done: a step done within a second says nothing, and a
// step that fails says nothing here (its caller reports the error).

const QUIET_MS = 1000;

// Runs `work()` and resolves with its result. `what` says what it does
// ("[engine] building the Rust engine"); after a second it is printed, and
// once the work is done, `${done} (N s)`.
async function step(what, done, work) {
  const started = Date.now();
  let told = false;
  const timer = setTimeout(() => {
    told = true;
    console.log(what);
  }, QUIET_MS);
  try {
    const result = await work();
    if (told) console.log(`${done} (${Math.round((Date.now() - started) / 1000)} s)`);
    return result;
  } finally {
    clearTimeout(timer);
  }
}

module.exports = { step };
