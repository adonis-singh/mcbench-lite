//! The child's tests: today's `build-sandbox.test.js` and `mc-dsl.test.js`,
//! the golden corpus against Node 26.13.1, the `Math` bits, the
//! confinement probes, the process protocol against recorded frames and the
//! parent's answers to forged frames.
//!
//! The child here is the `mcbench-build-child` that `MCBENCH_BUILD_CHILD`
//! names (`native/build-child`, its own workspace; `native/check.sh` builds
//! it and sets the variable), the program the app ships, under the platform
//! sandbox; without the variable the tests that run it are reported
//! ignored. The test modes that need a child of their own run
//! this test binary, selected by an environment variable the spawn passes,
//! so the shipped child holds no test code: with `MCBENCH_BUILD_ESCAPE_PROBE`
//! set, under the platform sandbox, it is the confinement probe
//! (`escape::probe`); with `MCBENCH_BUILD_FORGE=<kind>` a child that answers
//! with forged frames (`forge::child`). Otherwise it runs the tests
//! (`harness = false`); words given select the tests whose names contain one
//! of them.
//!
//! The golden files (`tests/golden/`) are Node 26.13.1's results of the
//! recorded test cases and the adversarial probes, made by
//! `harness/golden.js`; `harness/allow-list.json` lists the cases whose
//! result differs with its cause and pins the Rust result.

mod common;
mod escape;
mod forge;
mod ported;
mod protocol;

use std::panic::AssertUnwindSafe;
use std::time::Instant;

fn main() {
    if std::env::var_os(escape::PROBE).is_some() {
        escape::probe()
    }
    if let Some(kind) = std::env::var_os(forge::FORGE) {
        forge::child(kind.to_str().unwrap())
    }
    if let Some(case) = std::env::var_os(protocol::REPLAY) {
        protocol::replay(case.to_str().unwrap())
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Words select tests; libtest's flags are ignored, and so is the value
    // of `--test-threads` (this binary runs its tests one after another).
    let mut filters: Vec<&String> = Vec::new();
    let mut words = args.iter();
    while let Some(arg) = words.next() {
        if arg == "--test-threads" {
            words.next();
        } else if !arg.starts_with('-') {
            filters.push(arg);
        }
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let mut tests: Vec<common::Test> = Vec::new();
    // These run the child under the platform sandbox. On Windows that is
    // the Linux child inside a WSL2 distro the bash sandbox imported, named
    // for the test by MCBENCH_WSL_DISTRO.
    let mut confined: Vec<common::Test> = Vec::new();
    confined.extend(ported::mc_dsl());
    confined.extend(ported::build_sandbox());
    confined.extend(common::corpus_tests());
    confined.push(common::math_test());
    confined.push(common::math_fixed_test());
    confined.extend(protocol::real_child_tests());
    if cfg!(windows) && std::env::var_os(mcbench_build::sandbox::wsl::DISTRO_VAR).is_none() {
        for test in &confined {
            println!(
                "ignored: {}: needs {}=<a WSL2 distro the bash sandbox imported> and {}=<the Linux x86_64 mcbench-build-child>",
                test.name,
                mcbench_build::sandbox::wsl::DISTRO_VAR,
                mcbench_build::CHILD_PROGRAM_VAR
            );
        }
    } else if std::env::var_os(mcbench_build::CHILD_PROGRAM_VAR).is_none() {
        for test in &confined {
            println!(
                "ignored: {}: needs {}=<the built mcbench-build-child>",
                test.name,
                mcbench_build::CHILD_PROGRAM_VAR
            );
        }
    } else {
        tests.extend(confined);
    }
    tests.extend(common::golden_node_test());
    tests.extend(escape::tests());
    tests.extend(forge::tests());
    let selected: Vec<_> = tests
        .into_iter()
        .filter(|test| filters.is_empty() || filters.iter().any(|f| test.name.contains(f.as_str())))
        .collect();
    println!("\nrunning {} tests", selected.len());
    let started = Instant::now();
    let mut failed = Vec::new();
    for test in &selected {
        let one = Instant::now();
        let outcome = std::panic::catch_unwind(AssertUnwindSafe(|| runtime.block_on((test.run)())));
        let ms = one.elapsed().as_millis();
        match outcome {
            Ok(()) => println!("test {} ... ok ({ms} ms)", test.name),
            Err(_) => {
                println!("test {} ... FAILED ({ms} ms)", test.name);
                failed.push(test.name.clone());
            }
        }
    }
    println!(
        "\ntest result: {}. {} passed; {} failed; finished in {:.1} s",
        if failed.is_empty() { "ok" } else { "FAILED" },
        selected.len() - failed.len(),
        failed.len(),
        started.elapsed().as_secs_f64()
    );
    if !failed.is_empty() {
        for name in &failed {
            println!("    {name}");
        }
        std::process::exit(101);
    }
}
