//! The differential test between platforms: the cases of
//! `tests/diff/cases.json` through this platform's sandbox (Linux's
//! bubblewrap on the host kernel, or the macOS VM), compared with what the
//! Linux implementation gave on a Linux host. Transcript text, exit codes,
//! storage messages, error sentences and `view_image`'s PNG are compared
//! after the masks of `support::diff` (wall times, curl's and dd's timings,
//! bash's job pids). A difference must be one of [`MACHINE`], which name
//! the machine and nothing else, or the test fails naming the case.
//!
//! `MCBENCH_SANDBOX_DIFF_OUT=<file>` writes this run's answers (one JSON
//! object: case id to its normalised fields); with
//! `MCBENCH_SANDBOX_DIFF_REFERENCE=<file>` (such a file from the Linux
//! implementation on a Linux host) they are compared, and the table is
//! written to `MCBENCH_SANDBOX_DIFF_REPORT` when set. Needs the rootfs
//! variables (and on macOS the guest base and the signed binary, as
//! `tests/macos_vm.rs` says) and the internet. Run with
//! `cargo test -p mcbench-sandbox --test native_diff -- --ignored`.

#![cfg(any(target_os = "linux", target_os = "macos", windows))]

mod support;

use std::collections::BTreeMap;

use support::diff::*;
use support::*;

const MIB: u64 = 1024 * 1024;

/// The differences that come from the machine and nothing else, by case:
/// the reference's machine (adipc: x86_64, its login user, 32 CPUs)
/// against the macOS VM (aarch64, the agent's user, 2 CPUs) or the Windows
/// WSL2 distro (x86_64, the agent's user, the CPUs WSL gives its VM). The
/// open-file limit and core dumps are the box's own
/// (`linux::keeper::RLIMITS`), so `ulimit-n` and `signal-segv` are the same
/// on all three.
const MACHINE: &[(&str, &str)] = &[
    (
        "id",
        "the user's supplementary groups: a Linux host's login user has groups the box cannot map (shown as 65534, nogroup); the VM's and the distro's agent user has its own group only",
    ),
    (
        "nproc",
        "the CPUs the machine gives the box: the host's 32 on adipc, the VM's 2, the WSL2 VM's (all of Windows', 4 in the Windows test VM)",
    ),
    (
        "ls-root",
        "the architecture: Debian's x86_64 root filesystem has /lib64 (the x86_64 loader's path), the aarch64 one has none",
    ),
    (
        "proc-count",
        "the machine's timing: `ls -d /proc/[0-9]* | wc -l` counts its own pipeline, and whether bash has forked `wc` when `ls`'s glob is expanded depends on the machine's speed (counted 2026-11-18: the same seven processes listed on Linux and in WSL, 9 against 8)",
    ),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256 (on macOS the guest base too) and the internet"]
async fn the_same_commands_give_the_same_transcripts_as_the_linux_implementation() {
    let cases = cases();
    let scratch = Scratch::new("native-diff");
    let mut lines = ours(&cases, &scratch, "diff-main", "main", 4 * 1024 * MIB).await;
    lines.extend(ours(&cases, &scratch, "diff-tight", "tight", MIB).await);
    let answers = by_id(&lines);
    if let Some(file) = std::env::var_os("MCBENCH_SANDBOX_DIFF_OUT") {
        std::fs::write(file, serde_json::to_string_pretty(&answers).unwrap()).unwrap();
    }
    let Some(reference) = std::env::var_os("MCBENCH_SANDBOX_DIFF_REFERENCE") else {
        eprintln!(
            "{} cases run; MCBENCH_SANDBOX_DIFF_REFERENCE is not set, nothing compared",
            answers.len()
        );
        return;
    };
    let reference: BTreeMap<String, BTreeMap<String, String>> =
        serde_json::from_str(&std::fs::read_to_string(reference).unwrap()).unwrap();
    let machine: BTreeMap<&str, &str> = MACHINE.iter().copied().collect();
    let mut report = vec![
        "| Case | Same | Linux host (reference) | This machine | Cause |".to_string(),
        "|---|---|---|---|---|".to_string(),
    ];
    let (mut same, mut unexplained) = (0, Vec::new());
    for case in &cases {
        let id = case["id"].as_str().unwrap();
        let (a, b) = (&reference[id], &answers[id]);
        if a == b {
            same += 1;
            report.push(format!("| {id} | yes | | | |"));
            continue;
        }
        let keys: Vec<&String> = a
            .keys()
            .chain(b.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter(|k| a.get(*k) != b.get(*k))
            .collect();
        let show = |m: &BTreeMap<String, String>| {
            keys.iter()
                .map(|k| format!("{k}: {}", m.get(*k).map(String::as_str).unwrap_or("-")))
                .collect::<Vec<_>>()
                .join(" ; ")
        };
        let cause = machine.get(id).copied().unwrap_or("UNEXPLAINED");
        if cause == "UNEXPLAINED" {
            unexplained.push(id.to_string());
        }
        report.push(format!(
            "| {id} | no | {} | {} | {} |",
            cell(&show(a)),
            cell(&show(b)),
            cause
        ));
    }
    report.push(String::new());
    report.push(format!("{same} of {} cases identical", cases.len()));
    let text = report.join("\n");
    if let Some(file) = std::env::var_os("MCBENCH_SANDBOX_DIFF_REPORT") {
        std::fs::write(file, &text).unwrap();
    }
    eprintln!("{text}");
    assert!(
        unexplained.is_empty(),
        "differences that do not only name the machine: {unexplained:?}"
    );
}
