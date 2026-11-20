//! The differential test: the cases of `tests/diff/cases.json` through
//! today's Docker sandbox (`tests/diff/today.js` on `server/sandbox.js`)
//! and through this crate's Linux sandbox, compared: transcript text, exit
//! codes, storage messages, error sentences, `view_image`'s PNG. Wall times
//! and curl's milliseconds are masked. Every difference must be one listed
//! in [`EXPLAINED`] with its cause; the report (a table) is written to
//! `MCBENCH_SANDBOX_DIFF_REPORT` when set.
//!
//! Needs `MCBENCH_SANDBOX_DOCKER_DIFF=1` (Docker running with today's image
//! built, Node on `PATH`) beside the rootfs variables, and the internet. Run
//! with `cargo test -p mcbench-sandbox --test docker_diff -- --ignored`.

#![cfg(target_os = "linux")]

mod support;

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;
use support::diff::*;
use support::*;

const MIB: u64 = 1024 * 1024;

/// Today's stdout and stderr ordering.
const ORDER: &str = "today's `docker exec` gives stdout and stderr two pipes that Node reads as they arrive; the sentinel on stdout overtakes stderr just written, so today's transcript reorders stderr or carries it into the next command's; here both are one pipe, in the order the shell wrote them (a change of what a model sees: the owner's decision)";

/// The differences a model can see, by case, with their cause.
const EXPLAINED: &[(&str, &str)] = &[
    ("stderr", ORDER),
    ("interleave", ORDER),
    ("interleave-python", ORDER),
    ("many-lines-mixed", ORDER),
    ("net-stderr", ORDER),
    (
        "emoji-truncated",
        "the middle cut splits a surrogate pair: today's string keeps a lone surrogate (JSON writes it as \\udXXX), Rust's String holds U+FFFD there; the text is otherwise the same",
    ),
    (
        "id",
        "kernel: an unprivileged user namespace cannot drop the host user's supplementary groups (setgroups is denied), so they show as 65534 (nogroup); Docker sets them as root",
    ),
    (
        "signal-segv",
        "the box's own limits: core dumps are off in every box (`linux::keeper::RLIMITS`), so a crash prints no \"(core dumped)\"; today's container took the machine's, and adipc pipes cores to systemd-coredump, so bash added it there",
    ),
    (
        "unshare",
        "containment: a new user namespace fails with ENOSPC (bubblewrap's --disable-userns sets user.max_user_namespaces to 0); Docker's seccomp profile answers EPERM",
    ),
];

fn node(cases: &Path, scratch: &Path, chat: &str, which: &str, max: Option<u64>) -> Vec<Value> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/diff/today.js");
    let mut command = std::process::Command::new("node");
    command
        .arg(script)
        .arg(cases)
        .arg(scratch)
        .arg(chat)
        .arg(which);
    if let Some(max) = max {
        command.env("MCBENCH_CHAT_MAX_BYTES", max.to_string());
    }
    let out = command.output().expect("node");
    assert!(
        out.status.success(),
        "today.js: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_DOCKER_DIFF=1 (Docker with today's image, Node), MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_same_commands_give_the_same_transcripts_as_todays_docker_sandbox() {
    // Today's names are fixed in server/sandbox.js (IMAGE, containerName):
    // this test must never run on the owner's machines. Its last results
    // are in tests/diff/RESULTS.md.
    assert_eq!(
        std::env::var("MCBENCH_I_OWN_THIS_DOCKER").as_deref(),
        Ok("1"),
        "refused: it runs today's server/sandbox.js on Docker, which makes containers named as the app's own (mcb-v3-..., mcb-net-...) from the image localhost/mcbench-sandbox:<hash>, names that cannot be changed without editing server/; it is for a machine that is not the owner's (CI): set MCBENCH_I_OWN_THIS_DOCKER=1 there"
    );
    assert_eq!(
        std::env::var("MCBENCH_SANDBOX_DOCKER_DIFF").as_deref(),
        Ok("1"),
        "MCBENCH_SANDBOX_DOCKER_DIFF=1 is needed"
    );
    let cases_file = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/diff/cases.json");
    let cases: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(&cases_file).unwrap()).unwrap();
    let node_scratch = Scratch::new("diff-node");
    let mut today = node(&cases_file, &node_scratch.0, "diff-main", "main", None);
    today.extend(node(
        &cases_file,
        &node_scratch.0,
        "diff-tight",
        "tight",
        Some(MIB),
    ));
    let scratch = Scratch::new("diff-ours");
    let mut native = ours(&cases, &scratch, "diff-main", "main", 4 * 1024 * MIB).await;
    native.extend(ours(&cases, &scratch, "diff-tight", "tight", MIB).await);
    let by_id = |lines: &[Value]| -> BTreeMap<String, BTreeMap<String, String>> {
        lines
            .iter()
            .map(|l| (l["id"].as_str().unwrap().to_string(), normalised(l)))
            .collect()
    };
    let (today, native) = (by_id(&today), by_id(&native));
    let explained: BTreeMap<&str, &str> = EXPLAINED.iter().copied().collect();
    let mut report = vec![
        "| Case | Same | Today (Docker) | Native (bubblewrap) | Cause |".to_string(),
        "|---|---|---|---|---|".to_string(),
    ];
    let mut unexplained = Vec::new();
    let mut same = 0;
    for case in &cases {
        let id = case["id"].as_str().unwrap();
        let (a, b) = (&today[id], &native[id]);
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
        let cause = explained.get(id).copied().unwrap_or("UNEXPLAINED");
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
        "unexplained differences: {unexplained:?}"
    );
}
