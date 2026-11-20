//! No engine child outlives what started it, in every exit path: a test
//! process that starts children (settles running, a warm one free, an
//! experiment's child) and then panics, or aborts, mid-run leaves none of
//! them running. The harness is a test of this file run in a process of its
//! own (this test binary again); it records its children's process ids in a
//! file before it ends. A panic unwinds and drops every child (each is
//! killed on drop); an abort drops nothing, and the children see their
//! parent go (`mcbench-lite headless engine` watches it on Unix) and exit,
//! or on Windows the system closes the dead process's job handles, which
//! kills every child.
//!
//! Needs Minecraft's tables and the children's program (`MCBENCH_TABLES` and
//! `MCBENCH_ENGINE_PROGRAM` at build time).

mod common;

use mcbench_engine::{
    ChildEngine, Lease, PlaceCommands, SettlePool, VerifyEngines, settle_on, sweep_children,
};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// `count` observer clocks on stone: a settle of them keeps its child busy.
fn clocks(count: i32) -> PlaceCommands {
    let mut commands = String::new();
    for i in 0..count {
        let (x, z) = (4 * (i % 32), 2 * (i / 32));
        commands.push_str(&format!(
            "setblock {x} 64 {z} minecraft:stone\nsetblock {} 64 {z} minecraft:stone\nsetblock {x} 65 {z} minecraft:observer[facing=east]\nsetblock {} 65 {z} minecraft:observer[facing=west]\n",
            x + 1,
            x + 1
        ));
    }
    PlaceCommands(commands)
}

// The harness: run by the test below, which names the file for its
// children's pids (MCBENCH_HARNESS_PIDS) and how it ends
// (MCBENCH_HARNESS_END, panic or abort). Run without them (a run of the
// ignored tests) it is a harness with no one to report to and does nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "the harness of no_child_outlives_a_test_that_panics_or_aborts_mid_run, run by it in a process of its own"]
async fn harness_starts_children_and_ends_mid_run() {
    let Ok(pids_file) = std::env::var("MCBENCH_HARNESS_PIDS") else {
        return;
    };
    let end = std::env::var("MCBENCH_HARNESS_END").expect("run by the test below");
    let program = common::program();
    let mut pids = Vec::new();
    // Two settles running, on a pool of two.
    let pool = SettlePool::new(program.clone(), 2, Duration::from_secs(600));
    let mut settles = Vec::new();
    for key in ["a", "b"] {
        let (pid, told) = tokio::sync::oneshot::channel();
        settles.push(tokio::spawn(pool.run(
            key,
            move |lease: Arc<Lease<ChildEngine>>| async move {
                let _ = pid.send(lease.member().pid());
                settle_on(
                    lease.member(),
                    &clocks(1024),
                    &[70000],
                    &CancellationToken::new(),
                )
                .await
            },
        )));
        pids.push(told.await.unwrap());
    }
    // A warm child, free.
    let engines = VerifyEngines::new(program.clone());
    engines.warm(1).unwrap();
    pids.extend(engines.pids());
    // A child of its own, ready.
    let child = program.spawn().unwrap();
    child.ready(&CancellationToken::new()).await.unwrap();
    pids.push(child.pid());
    std::fs::write(
        &pids_file,
        pids.iter()
            .map(|pid| format!("{pid}\n"))
            .collect::<String>(),
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    match end.as_str() {
        "panic" => panic!("the harness panics mid-run"),
        "abort" => std::process::abort(),
        other => unreachable!("no end {other}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn no_child_outlives_a_test_that_panics_or_aborts_mid_run() {
    let folder = common::scratch("leftover");
    for end in ["panic", "abort"] {
        let pids_file = folder.join(format!("pids-{end}"));
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "harness_starts_children_and_ends_mid_run",
                "--include-ignored",
                "--test-threads",
                "1",
            ])
            .env("MCBENCH_HARNESS_PIDS", &pids_file)
            .env("MCBENCH_HARNESS_END", end)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(!status.success(), "the harness ended with {end}");
        let pids: Vec<u32> = std::fs::read_to_string(&pids_file)
            .unwrap()
            .lines()
            .map(|pid| pid.parse().unwrap())
            .collect();
        assert_eq!(pids.len(), 4, "{end}: {pids:?}");
        for pid in pids {
            assert!(
                common::gone_within(pid, common::AT_ONCE).await,
                "{end}: the child {pid} outlived the harness"
            );
        }
    }
    // The folders the aborted harness's children left go with the sweep.
    sweep_children(&std::env::temp_dir(), 0.0);
    std::fs::remove_dir_all(&folder).unwrap();
}
