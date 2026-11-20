//! `test/mc-dsl.test.js` (18) and `test/build-sandbox.test.js` (19) through
//! the real child. Every run is the program, world, registry, `/work`
//! answers and settled world today's test gave `runProgram` (recorded by
//! `harness/record.js` under the test's name), and its result must be Node
//! 26.13.1's (the golden files): logs, written files, the touched count, the
//! error text and the change set's bytes, so the world an applied run leaves
//! is Node's block for block. Each test adds the assertion of today's test
//! on what the program printed or failed with. What a test checks of the
//! parent's `World` (copy-on-write sharing, `diff`, the untouched base) is
//! `mcbench_world`'s transfer codec's, named where it occurs.

use regex::Regex;
use serde_json::json;

use crate::common::{Ran, Test, cases_of, check, run, test};

async fn runs(prefix: &str, name: &str) -> Vec<Ran> {
    let mut out = Vec::new();
    for case in cases_of(prefix, name) {
        out.push(check(case).await);
    }
    out
}

fn logs(ran: &Ran) -> Vec<&str> {
    ran.outcome().logs.iter().map(String::as_str).collect()
}

fn assert_matches(text: &str, pattern: &str) {
    assert!(
        Regex::new(pattern).unwrap().is_match(text),
        "{text:?} does not match /{pattern}/"
    );
}

const DSL: &str = "recmcdsl-";

pub fn mc_dsl() -> Vec<Test> {
    vec![
        test("mc-dsl: build places blocks and reports logs", || async {
            let r = runs(DSL, "build places blocks and reports logs").await;
            assert_eq!(logs(&r[0]), ["n 17"]);
        }),
        test("mc-dsl: mc.find reads only the region asked", || async {
            let r = runs(DSL, "mc.find reads only the region asked").await;
            assert_eq!(logs(&r[0]), ["[[15,0,0],[16,0,0],[17,0,0]]", "42 42 1"]);
        }),
        test(
            "mc-dsl: block state validation rejects unknown states",
            || async {
                let r = runs(DSL, "block state validation rejects unknown states").await;
                assert_matches(&r[0].failure().message, "facing=up is invalid");
            },
        ),
        test(
            "mc-dsl: a throwing program leaves the passed world untouched (caller swaps clones)",
            || async {
                // In the child a failed program returns no change set at all.
                let r = runs(
                    DSL,
                    "a throwing program leaves the passed world untouched (caller swaps clones)",
                )
                .await;
                assert_matches(&r[0].failure().message, "abort");
            },
        ),
        test(
            "mc-dsl: Math.random is deterministic across runs (seeded from source)",
            || async {
                let r = runs(
                    DSL,
                    "Math.random is deterministic across runs (seeded from source)",
                )
                .await;
                assert_eq!(r.len(), 2);
                assert_eq!(logs(&r[0]), logs(&r[1]));
            },
        ),
        test(
            "mc-dsl: sandbox escape via host function constructor is blocked",
            || async {
                let r = runs(
                    DSL,
                    "sandbox escape via host function constructor is blocked",
                )
                .await;
                assert_matches(&r[0].failure().message, "process is not defined");
            },
        ),
        test(
            "mc-dsl: returned objects carry only the in-realm constructor",
            || async {
                let r = runs(DSL, "returned objects carry only the in-realm constructor").await;
                assert_eq!(logs(&r[0]), ["undefined"]);
            },
        ),
        test("mc-dsl: async programs are rejected", || async {
            let r = runs(DSL, "async programs are rejected").await;
            assert_matches(&r[0].failure().message, "synchronous");
        }),
        test(
            "mc-dsl: replace to the same id terminates and rewrites states past the find cap",
            || async {
                let r = runs(
                    DSL,
                    "replace to the same id terminates and rewrites states past the find cap",
                )
                .await;
                assert_eq!(logs(&r[0]), ["600"]);
            },
        ),
        test("mc-dsl: replace honors the region", || async {
            let r = runs(DSL, "replace honors the region").await;
            assert_eq!(logs(&r[0]), ["5"]);
        }),
        test(
            "mc-dsl: bulk replacements preserve checkpoints, carry compatible states, and count no-ops",
            || async {
                // Chunk sharing between base and clone is mcbench_world's.
                let r = runs(DSL, "bulk replacements preserve checkpoints, carry compatible states, and count no-ops").await;
                assert_eq!(r.len(), 3);
                assert_eq!(logs(&r[0]), ["162"]);
                assert_eq!(logs(&r[1]), ["900"]);
                r[2].outcome();
            },
        ),
        test(
            "mc-dsl: outline matches edge geometry for volumes, planes, lines, and single blocks",
            || async {
                let r = runs(
                    DSL,
                    "outline matches edge geometry for volumes, planes, lines, and single blocks",
                )
                .await;
                let dimensions = [
                    [35, 19, 7],
                    [1, 5, 6],
                    [4, 1, 6],
                    [4, 5, 1],
                    [1, 1, 6],
                    [1, 1, 1],
                    [2, 3, 2],
                ];
                assert_eq!(r.len(), dimensions.len());
                for (ran, [nx, ny, nz]) in r.iter().zip(dimensions) {
                    let mut edges = 0i64;
                    for x in 0..nx {
                        for y in 0..ny {
                            for z in 0..nz {
                                let on = |v: i32, n: i32| i32::from(v == 0 || v == n - 1);
                                if on(x, nx) + on(y, ny) + on(z, nz) >= 2 {
                                    edges += 1;
                                }
                            }
                        }
                    }
                    assert_eq!(ran.outcome().touched, 2 * i64::from(nx * ny * nz) - edges);
                }
            },
        ),
        test(
            "mc-dsl: hollow and outline leave the world block for block as filling the box and carving it did",
            || async {
                // 432 runs; each change set is Node's byte for byte.
                let r = runs(DSL, "hollow and outline leave the world block for block as filling the box and carving it did").await;
                assert_eq!(r.len(), 2 * 6 * 6 * 6);
            },
        ),
        test(
            "mc-dsl: the block limit counts what hollow and outline leave, not the whole box",
            || async {
                let r = runs(
                    DSL,
                    "the block limit counts what hollow and outline leave, not the whole box",
                )
                .await;
                assert_eq!(r.len(), 2);
                r[0].outcome();
                r[1].outcome();
            },
        ),
        test(
            "mc-dsl: a hollow fill the block limit refuses says how many blocks it would add and writes nothing",
            || async {
                let r = runs(DSL, "a hollow fill the block limit refuses says how many blocks it would add and writes nothing").await;
                assert_matches(
                    &r[0].failure().message,
                    "(?m)Build exceeds the 3000000 block limit: the world has 2999999 blocks and this call would add 25$",
                );
            },
        ),
        test(
            "mc-dsl: find throws past the result cap instead of silently truncating",
            || async {
                let r = runs(
                    DSL,
                    "find throws past the result cap instead of silently truncating",
                )
                .await;
                assert_matches(&r[0].failure().message, r#"more than 500 "stone" matches"#);
            },
        ),
        test(
            "mc-dsl: failed programs carry their logs and a build.js stack frame",
            || async {
                let r = runs(
                    DSL,
                    "failed programs carry their logs and a build.js stack frame",
                )
                .await;
                let failure = r[0].failure();
                assert_eq!(failure.logs, ["phase 1 done"]);
                assert_matches(&failure.message, "Cannot read properties of undefined");
                assert_matches(&failure.message, r"build\.js:4");
                assert!(
                    !Regex::new(r"node:vm|bootstrap\.js")
                        .unwrap()
                        .is_match(&failure.message)
                );
            },
        ),
        test(
            "mc-dsl: mc.settled reads the settled world at mc.get coordinates, offsets included",
            || async {
                let r = runs(
                    DSL,
                    "mc.settled reads the settled world at mc.get coordinates, offsets included",
                )
                .await;
                assert_eq!(r.len(), 4);
                assert_eq!(
                    logs(&r[0]),
                    [
                        r#"{"id":"minecraft:oak_stairs","states":{"facing":"north","half":"bottom"}} null"#,
                        r#"{"id":"minecraft:oak_stairs","states":{"facing":"north","half":"bottom"}}"#,
                        "north 50",
                    ]
                );
                // The settled world crosses to the child once, however often the
                // program reads it (in process the test counts 5 loads).
                assert_eq!(r[0].settled_calls, 1);
                assert_matches(
                    &r[1].failure().message,
                    "no Minecraft settle for this program",
                );
                assert_matches(&r[2].failure().message, r"mc\.settled takes three integers");
                assert_matches(&r[3].failure().message, "out of range");
            },
        ),
    ]
}

const SANDBOX: &str = "recbuildsandbox-";

/// The program of today's tests that reaches the host realm through a
/// custom `map` (build-sandbox.test.js `escapeToHostRealm`): in the V8 child
/// that realm is the prelude's, whose `process` is `{ env, exit }`.
pub fn build_sandbox() -> Vec<Test> {
    vec![
        test(
            "build-sandbox: the isolated run matches the in-process run block for block",
            || async {
                // The in-process run and the isolated run of today's test, recorded
                // as two cases of the same program.
                let r = runs(
                    SANDBOX,
                    "the isolated run matches the in-process run block for block",
                )
                .await;
                assert_eq!(r.len(), 2);
                // Each change set is Node's for its own recorded clone; what
                // the program reports is the same for both.
                let (a, b) = (r[0].outcome(), r[1].outcome());
                assert_eq!(
                    (&a.logs, &a.files, a.touched),
                    (&b.logs, &b.files, b.touched)
                );
                assert_eq!(a.files.len(), 1);
            },
        ),
        test(
            "build-sandbox: a cloned world keeps copy-on-write sharing and an untouched base after an isolated run",
            || async {
                // The sharing and the untouched base are mcbench_world's applyChanges.
                let r = runs(SANDBOX, "a cloned world keeps copy-on-write sharing and an untouched base after an isolated run").await;
                assert_eq!(r[0].outcome().touched, 1);
            },
        ),
        test(
            "build-sandbox: Math.random stays deterministic and matches the in-process seed",
            || async {
                let r = runs(
                    SANDBOX,
                    "Math.random stays deterministic and matches the in-process seed",
                )
                .await;
                assert_eq!(r.len(), 3);
                assert_eq!(logs(&r[0]), logs(&r[1]));
                assert_eq!(logs(&r[0]), logs(&r[2]));
            },
        ),
        test(
            "build-sandbox: mc.readFile is served over the channel and its errors reach the program",
            || async {
                let r = runs(
                    SANDBOX,
                    "mc.readFile is served over the channel and its errors reach the program",
                )
                .await;
                assert_eq!(r[0].reads, ["grid.txt"]);
                assert_eq!(logs(&r[0]), ["payload-from-host"]);
                assert_matches(&r[1].failure().message, r"No such file in /work: missing");
            },
        ),
        test(
            "build-sandbox: validation and error formatting are identical to the in-process runtime",
            || async {
                let r = runs(
                    SANDBOX,
                    "validation and error formatting are identical to the in-process runtime",
                )
                .await;
                assert_matches(&r[0].failure().message, "facing=up is invalid");
                assert_matches(&r[1].failure().message, "synchronous");
                let failure = r[2].failure();
                assert_eq!(failure.logs, ["phase 1 done"]);
                assert_matches(&failure.message, "Cannot read properties of undefined");
                assert_matches(&failure.message, r"build\.js:4");
                assert!(
                    !Regex::new(r"node:vm|bootstrap\.js|build-child\.js")
                        .unwrap()
                        .is_match(&failure.message)
                );
            },
        ),
        test(
            "build-sandbox: the normVec3 map vector reaches the host realm of an empty, namespaced process",
            || async {
                // The vector still reaches the child's main realm; there `process`
                // has no pid, no permission model and no modules, so the program
                // fails reading `process.permission.has`.
                let r = runs(SANDBOX, "the normVec3 map vector reaches the host realm of an empty, namespaced process").await;
                assert_matches(
                    &r[0].failure().message,
                    r"Cannot read properties of undefined \(reading 'has'\)",
                );
            },
        ),
        test(
            "build-sandbox: an escaped program cannot read a canary outside /work",
            || async {
                let r = runs(
                    SANDBOX,
                    "an escaped program cannot read a canary outside /work",
                )
                .await;
                // The realm has no getBuiltinModule, so the program's fs is null.
                assert_eq!(
                    logs(&r[0]),
                    ["blocked:Cannot read properties of null (reading 'readFileSync')"]
                );
                assert!(!logs(&r[0])[0].contains("SECRET"));
            },
        ),
        test(
            "build-sandbox: an escaped program cannot open a network connection",
            || async {
                // The child's realms have no network module at all.
                let r = runs(
                    SANDBOX,
                    "an escaped program cannot open a network connection",
                )
                .await;
                assert_matches(
                    &r[0].failure().message,
                    r"process\.getBuiltinModule is not a function",
                );
            },
        ),
        test(
            "build-sandbox: an escaped program cannot spawn a child process",
            || async {
                let r = runs(SANDBOX, "an escaped program cannot spawn a child process").await;
                assert_eq!(
                    logs(&r[0]),
                    ["blocked:process.getBuiltinModule is not a function"]
                );
            },
        ),
        test(
            "build-sandbox: every known vector either stays in the vm realm or lands in the powerless process",
            || async {
                let r = runs(SANDBOX, "every known vector either stays in the vm realm or lands in the powerless process").await;
                assert_eq!(r.len(), 10);
                for ran in &r {
                    let (text, lines) = match &ran.result {
                        Ok(outcome) => (outcome.logs.join("\n"), outcome.logs.clone()),
                        Err(failure) => (
                            format!("{}\n{}", failure.message, failure.logs.join("\n")),
                            failure.logs.clone(),
                        ),
                    };
                    assert!(!text.contains("SECRET"), "{text}");
                    for line in lines.iter().filter(|line| line.contains("pid=")) {
                        // The host realm's process has no pid and no modules
                        // (mc.log prints the undefined read as nothing).
                        assert_matches(line, "pid=undefined $");
                    }
                }
                let custom = &r[0].outcome().logs;
                assert_eq!(custom, &["custom map callback pid=undefined "]);
                assert_matches(&r[8].failure().message, "synchronous");
                assert_eq!(
                    logs(&r[9]),
                    ["floating promise and timers undefined undefined undefined undefined"]
                );
            },
        ),
        test(
            "build-sandbox: an infinite loop is stopped at the script timeout, not the wall clock",
            || async {
                let started = std::time::Instant::now();
                let r = runs(
                    SANDBOX,
                    "an infinite loop is stopped at the script timeout, not the wall clock",
                )
                .await;
                assert_matches(&r[0].failure().message, "(?i)timed out");
                assert!(started.elapsed() < mcbench_build::WALL_TIMEOUT);
            },
        ),
        test(
            "build-sandbox: a memory bomb is reported as a program error and the server survives",
            || async {
                // The bomb is stopped by whichever of the program's two
                // limits it reaches first: the 2,048 MB heap cap (Node's
                // recorded result, and the child's on a machine whose page
                // faults are fast) or the 5 s script limit (in the Windows
                // VM, where nested virtualisation makes each fault slow). The
                // test holds that one of the two stopped it, as a program
                // error, and that the server survives it; not which came
                // first.
                let cases = cases_of(
                    SANDBOX,
                    "a memory bomb is reported as a program error and the server survives",
                );
                assert_eq!(cases.len(), 2);
                let bomb = crate::common::run_case(cases[0]).await.value();
                let cap = crate::common::wanted(cases[0]);
                let time_limit = json!({
                    "ok": false,
                    "error": "Error: Script execution timed out after 5000ms",
                    "logs": null,
                });
                assert!(
                    bomb == cap || bomb == time_limit,
                    "neither limit stopped the bomb: {bomb}"
                );
                assert_eq!(check(cases[1]).await.outcome().touched, 1);
            },
        ),
        test(
            "build-sandbox: a huge off-heap allocation fails at the data limit instead of filling host memory",
            || async {
                // Today's test runs on Linux only (RLIMIT_DATA); the child's
                // allocator counts array-buffer bytes, so it holds on every platform.
                let registry = json!({ "stone": [{}, {}] });
                let started = std::time::Instant::now();
                let r = run(
                "module.exports = mc => { const b = new Uint8Array(16 * 1024 ** 3); b.fill(1); mc.log(b.length); }",
                &registry,
                &crate::common::EMPTY_WORLD,
                None,
                None,
            )
            .await;
                assert_matches(&r.failure().message, "Array buffer allocation failed");
                assert!(started.elapsed() < crate::common::AT_ONCE);
            },
        ),
        test(
            "build-sandbox: mc.settled crosses to the process once, on the first call, and matches the in-process read",
            || async {
                let r = runs(SANDBOX, "mc.settled crosses to the process once, on the first call, and matches the in-process read").await;
                assert_eq!(r.len(), 4);
                assert_eq!(r[0].value(), r[1].value());
                assert_eq!(
                    logs(&r[1]),
                    [
                        r#"{"id":"minecraft:oak_stairs","states":{"facing":"west","half":"top"}} null minecraft:stone"#,
                        "west",
                        "210",
                    ]
                );
                assert_eq!(r[1].settled_calls, 1);
                assert_matches(
                    &r[2].failure().message,
                    "no Minecraft settle for this program",
                );
                assert_eq!(r[2].settled_calls, 0);
                assert_eq!(logs(&r[3]), ["null"]);
                assert_eq!(r[3].settled_calls, 0);
            },
        ),
    ]
}
