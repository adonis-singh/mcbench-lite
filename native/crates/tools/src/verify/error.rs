//! Why an experiment gave no report, worded as today's verifier words it:
//! `Error`s with their message, `VerificationBudgetError` and
//! `WallBudgetError` with their `code` and `budget`, and the abort reasons.

use serde_json::{Map, Value, json};

use super::budgets::Limits;
use super::js;

/// What a test that fails is answered with; its `Display` is the error's
/// `message`, which models read.
#[derive(Clone, Debug, PartialEq)]
pub enum VerifyError {
    /// An `Error` with this message.
    Message(String),
    /// `VerificationBudgetError` (or `WallBudgetError`, whose `budget` holds
    /// three more keys): `code: 'VERIFICATION_BUDGET_EXCEEDED'` and `budget`.
    Budget {
        /// The error's message.
        message: String,
        /// `error.budget`, its keys in today's order.
        budget: Map<String, Value>,
    },
}

/// `AbortController.abort()`'s reason, a `DOMException` whose message is
/// this: what a chat's cancel rejects a test with while it waits or before
/// its engine runs.
pub const ABORTED: &str = "This operation was aborted";

/// The verifier's shutdown reason (`stop()`).
pub const SHUTTING_DOWN: &str = "Minecraft verification is shutting down";

/// What a test cancelled while the engine ran it is answered with: today
/// the cancel stopped the engine process with SIGTERM and the experiment
/// failed with the process's exit (`engine.js`: `The engine stopped
/// (<signal>): <the tail of its stderr>`, which is empty unless the program
/// printed a panic before). The engine child is stopped the same way; the
/// verifier answers at once, with the empty tail, without waiting for the
/// child's exit.
pub const ENGINE_STOPPED: &str = "The engine stopped (SIGTERM): ";

impl VerifyError {
    /// An `Error` with `message`.
    pub fn message(message: impl Into<String>) -> VerifyError {
        VerifyError::Message(message.into())
    }

    /// `new VerificationBudgetError(resource, requested, allowed)`, `allowed`
    /// being `budgets[resource]` (absent from a grader's budgets: today's
    /// `undefined`).
    pub fn budget(resource: &str, requested: Option<f64>, allowed: Option<f64>) -> VerifyError {
        let setting = Limits::setting(resource);
        let message = format!(
            "Minecraft test needs {} {resource}; configured budget is {} ({})",
            requested.map_or("undefined".to_string(), js::text),
            allowed.map_or("undefined".to_string(), js::text),
            setting.unwrap_or("undefined"),
        );
        let mut budget = Map::new();
        budget.insert("resource".into(), Value::from(resource));
        if let Some(requested) = requested {
            budget.insert("requested".into(), js::number(requested));
        }
        if let Some(allowed) = allowed {
            budget.insert("allowed".into(), js::number(allowed));
        }
        if let Some(setting) = setting {
            budget.insert("setting".into(), Value::from(setting));
        }
        VerifyError::Budget { message, budget }
    }

    /// `new WallBudgetError(elapsedMs, allowedMs, { reached })`: `reached` is
    /// the last progress the engine reported (`{phase, tick, totalTicks}`).
    pub fn wall(elapsed_ms: f64, allowed_ms: f64, reached: Option<&Value>) -> VerifyError {
        let VerifyError::Budget { mut budget, .. } =
            VerifyError::budget("wallMs", Some(elapsed_ms), Some(allowed_ms))
        else {
            unreachable!("a budget error")
        };
        let field = |name: &str| reached.and_then(|r| r.get(name));
        let phase = field("phase");
        let tick = field("tick");
        let total = field("totalTicks");
        let shown = |value: Option<&Value>| match value {
            None => "undefined".to_string(),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Number(n)) => js::text(n.as_f64().expect("a number")),
            Some(other) => mcbench_json::stringify(other),
        };
        let where_ = if reached.is_none() || phase == Some(&json!("activating")) {
            "had not started ticking yet".to_string()
        } else if phase == Some(&json!("settling")) {
            format!(
                "was still settling, at tick {} of {} settling ticks",
                shown(tick),
                shown(total)
            )
        } else {
            format!(
                "reached tick {} of {} of its timeline, after its settling ticks",
                shown(tick),
                shown(total)
            )
        };
        let message = format!(
            "The test was too dense to finish within its {}-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS) and was stopped: it {where_}. Shorten it (fewer ticks, a smaller area, less active circuitry) or split it into several tests; this is a limit of the harness, not a result about the build.",
            js::text(js::round(allowed_ms / 1000.0))
        );
        // `reached?.phase ?? null`, `reached?.tick ?? 0`, `reached?.totalTicks ?? null`.
        let or = |value: Option<&Value>, fallback: Value| match value {
            None | Some(Value::Null) => fallback,
            Some(value) => value.clone(),
        };
        budget.insert("phase".into(), or(phase, Value::Null));
        budget.insert("ticksReached".into(), or(tick, Value::from(0)));
        budget.insert("totalTicks".into(), or(total, Value::Null));
        VerifyError::Budget { message, budget }
    }

    /// The `budget` of a budget error.
    pub fn budget_object(&self) -> Option<&Map<String, Value>> {
        match self {
            VerifyError::Budget { budget, .. } => Some(budget),
            VerifyError::Message(_) => None,
        }
    }
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::Message(message) | VerifyError::Budget { message, .. } => {
                f.write_str(message)
            }
        }
    }
}

impl std::error::Error for VerifyError {}

/// An experiment the engine rejected, worded as the Paper plugin words it
/// (`verificationFailure`): a rejection while the test's data is imported
/// is "could not start: MCBENCH_ERROR \<message\>"; any other failure is
/// "failed: \<exception\>". A fault of the engine itself keeps its own
/// words.
pub fn verification_failure(message: &str) -> VerifyError {
    let Some(rejected) = message.strip_prefix("The engine failed: ") else {
        return VerifyError::message(message);
    };
    // The engine appends the exception's causes (" <- ...") and where it
    // arose (" @ frame"); the plugin reports the exception alone.
    let cut = [rejected.find(" <- "), rejected.find(" @ ")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(rejected.len());
    let exception = &rejected[..cut];
    let message = strip_exception_class(exception);
    if START_REJECTIONS
        .iter()
        .any(|prefix| message.starts_with(prefix))
    {
        VerifyError::message(format!(
            "Minecraft verification could not start: MCBENCH_ERROR {message}\n"
        ))
    } else {
        VerifyError::message(format!("Minecraft verification failed: {exception}"))
    }
}

/// `exception.replace(/^(?:[\w$]+\.)+[\w$]*(?:Exception|Error): /, '')`.
fn strip_exception_class(exception: &str) -> &str {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_' || c == '$';
    // The match holds no ':' before its ": ", so only the first ": " can end
    // it. Before it: one or more nonempty `word.` groups, then a last
    // segment of word characters ending in Exception or Error.
    let Some(found) = exception.find(": ") else {
        return exception;
    };
    let head = &exception[..found];
    let segments: Vec<&str> = head.split('.').collect();
    let (last, groups) = segments.split_last().expect("split gives one segment");
    let matches = !groups.is_empty()
        && groups.iter().all(|s| !s.is_empty() && s.chars().all(word))
        && last.chars().all(word)
        && (last.ends_with("Exception") || last.ends_with("Error"));
    if matches {
        &exception[found + 2..]
    } else {
        exception
    }
}

/// What `McbenchExperiment.start` throws while it imports a test's block
/// entities, scheduled ticks and entities (`START_REJECTIONS`, copied from
/// `server/minecraft-verify.js`; `test/start-rejections.test.js` keeps that
/// list equal to the Java source's).
pub const START_REJECTIONS: &[&str] = &[
    "No block entity at ",
    "Cannot import future block-entity data version ",
    "Block entity ID mismatch at ",
    "Invalid block entity at ",
    "Saved tick restoration requires frozen strict placement",
    "Future scheduled ticks cannot be downgraded",
    "Scheduled tick outside saved bounds",
    "Saved tick order leaves no room for subsequent native updates",
    "Unknown saved block tick: ",
    "Unknown saved fluid tick: ",
    "Unknown saved tick kind",
    "Missing restored block tick",
    "Native queue changed saved block tick timing or order",
    "Missing restored fluid tick",
    "Native queue changed saved fluid tick timing or order",
    "Cannot import future entity data version ",
    "Native entity import currently supports boats, rafts, dropped items, experience orbs, item frames, paintings, armor stands and ordinary container/furnace/TNT minecarts only: ",
    "Entity leash relationships require a faithful relationship importer",
    "Invalid passenger list",
    "Unsupported passengers: only a single original dolphin riding an ordinary minecart is supported",
    "Invalid dolphin passenger",
    "Unsupported passenger family or relationship",
    "Passenger requires original UUID",
    "Invalid passenger UUID",
    "Passenger requires original Pos",
    "Invalid passenger Pos",
    "Invalid passenger coordinate",
    "Passenger is outside the mounted-entity import margin",
    "Entity needs Pos",
    "Entity needs three Pos coordinates and its source UUID",
    "Entity needs a native UUID int array",
    "Entity UUID must contain four ints",
    "Invalid entity Pos",
    "Entity Pos does not match declared at",
    "Item frame needs native attachment block_pos",
    "Invalid frame attachment coordinates",
    "Invalid native entity ",
    "Native frame loader rejected the source attachment position",
    "Frame attachment/facing does not match source Pos",
    "Native import changed source entity UUID",
    "Native import lost passenger relationship",
    "Native import changed dolphin passenger identity",
    "Native import changed saved passenger position",
    "Failed native entity insertion (possibly duplicate UUID): ",
    "Could not audit imported entity ",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_failures_are_worded_as_the_plugin_words_them() {
        assert_eq!(
            verification_failure("The engine failed: java.lang.IllegalStateException: No block entity at [0,0,0] <- cause @ frame").to_string(),
            "Minecraft verification could not start: MCBENCH_ERROR No block entity at [0,0,0]\n"
        );
        assert_eq!(
            verification_failure(
                "The engine failed: unported: LiquidBlock.getDrops (minecraft:water[level=1])"
            )
            .to_string(),
            "Minecraft verification failed: unported: LiquidBlock.getDrops (minecraft:water[level=1])"
        );
        assert_eq!(
            verification_failure("The engine failed: java.lang.NullPointerException @ x")
                .to_string(),
            "Minecraft verification failed: java.lang.NullPointerException"
        );
        assert_eq!(
            verification_failure("The engine stopped (SIGTERM): ").to_string(),
            "The engine stopped (SIGTERM): "
        );
        assert_eq!(strip_exception_class("a.b.FooError: x: y"), "x: y");
        assert_eq!(strip_exception_class("FooError: x"), "FooError: x");
        assert_eq!(strip_exception_class(".FooError: x"), ".FooError: x");
    }
}
