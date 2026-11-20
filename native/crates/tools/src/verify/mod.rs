//! `test_build`'s engine verifier: `server/minecraft-verify.js`'s
//! `EngineVerifier` and `VerifierPool` with what of that file the engine
//! path uses, `verification-contract.js`'s budgets and checks, and
//! `server/agent.js` `executeTestBuild`.
//!
//! | Module | Today's code |
//! |---|---|
//! | `budgets` ([`Budgets`], [`Limits`]) | `verification-contract.js` `readBudgets`, `BUDGETS`, `BUDGET_ENV` |
//! | `request` ([`validate_request`], [`call_tests`], [`Experiment`]) | `callTests`, `validateRequest`, `entityCoordinates`, `validateEntities`, `Experiments.request` and `transport` |
//! | `error` ([`VerifyError`]) | `VerificationBudgetError`, `WallBudgetError`, `verificationFailure`, `START_REJECTIONS` |
//! | `verifier` ([`VerifierPool`]) | `Experiments.run`, `EngineVerifier.execute`, `checkComplete`, `finishedResult`, `progressUpdate`, `placedPalette`, `sweepRuns`, `VerifierPool` |
//! | `call` ([`execute_test_build`]) | `executeTestBuild` |
//! | `system` | `os.totalmem()`, `crypto.randomUUID()` |
//!
//! Every text a model reads (each error sentence, the result's keys in
//! their order, its numbers) is today's: the differential harness
//! (`tests/differential.rs` with `tests/node/`) runs the same calls
//! through today's Node verifier on the released engine and through this
//! module on the app's engine children and compares the results byte for
//! byte, and the files written.
//!
//! ## What the native app does not have
//!
//! The Paper verifier (`MinecraftVerifier`, `compilePlugin`, `seedCache`,
//! `verification-liveness.js`) stays in the development-time parity tools.
//! Where today's code branched on the verifier kind, only the engine's
//! branch is here: the queued-runtime reuse and idle timer, the plugin's
//! preparation and `prepared.json`, `physicsGuard`, the runtime's liveness
//! check, the Paper note and `reusedRuntime` timing, and the broker's
//! `timings.broker` (a sweep's shared engine pool; here the engine
//! children are the pool's own).
//!
//! ## Cancelling an experiment the engine runs
//!
//! A `VERIFY` runs in an engine child of the app's binary
//! (`mcbench_engine::VerifyEngines`). A cancel, the shutdown or the
//! wall-clock budget answers the caller at once (a cancel with today's
//! answer, `The engine stopped (SIGTERM): `, the wall budget with today's
//! `WallBudgetError` and the ticks the progress reached) and stops the
//! child, so the lane is free at once and its next experiment runs on a
//! fresh child.

mod budgets;
mod call;
mod error;
mod js;
mod request;
mod system;
mod verifier;

pub use budgets::{Budgets, Limits};
pub use call::{FAILURE_NOTE, Work, WorkFolder, execute_test_build, failure};
pub use error::{
    ABORTED, ENGINE_STOPPED, SHUTTING_DOWN, START_REJECTIONS, VerifyError, verification_failure,
};
pub use request::{
    DEFAULT_SAMPLES, DEFAULT_SETTLE_TICKS, Encoding, Experiment, ItemTags, Placement, Settings,
    call_tests, entity_coordinates, validate_entities, validate_request,
};
pub use system::{random_uuid, set_uuid_source, total_memory};
pub use verifier::{
    LANE_BYTES, MAX_INLINE_BYTES, MAX_LANES, Minecraft, NOTE, RESERVED_BYTES, VerifierPool,
    VerifyEngine, sweep_runs,
};
