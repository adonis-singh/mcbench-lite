//! The speed table's `bash` row for the native sandbox (driven by
//! `native/bench/bash-tool.mjs --target native`):
//!
//!   bash_bench <sandbox folder> <rootfs.tar.zst> <sha256> <agent> <runs>
//!
//! The clock is the row's: from the call of `exec` with `{ command: 'true' }`
//! to its answer, on a new chat each run: the first call (its box made),
//! then 5 more; then the networked command's first call (its box made) and
//! 5 more. One JSON line per run.

#[cfg(target_os = "linux")]
#[tokio::main]
async fn main() {
    use std::sync::Arc;
    use std::time::Instant;

    use mcbench_sandbox::lifecycle::Sandboxes;
    use mcbench_sandbox::linux::Bubblewrap;
    use mcbench_sandbox::testing::TestBudget;
    use mcbench_sandbox::{BashCommand, Sandbox, Workspace};
    use tokio_util::sync::CancellationToken;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, rootfs, sha, agent, runs] = args.as_slice() else {
        eprintln!("usage: bash_bench <sandbox folder> <rootfs.tar.zst> <sha256> <agent> <runs>");
        std::process::exit(2)
    };
    let root = std::path::PathBuf::from(root);
    let budget = Arc::new(TestBudget::new(root.clone(), 4 << 30));
    let sandbox = Sandboxes::new(
        Bubblewrap::new(
            root.clone(),
            (rootfs.into(), sha.clone()),
            vec![agent.into()],
            mcbench_sandbox::linux::Delegation::UserScope,
        ),
        budget,
    );
    sandbox.prepare().await.expect("prepare");
    for run in 0..runs.parse::<usize>().expect("runs") {
        let ws = Workspace {
            chat: format!("bench-{}-{run}", std::process::id()),
            host: root.join(format!("bench-{}-{run}", std::process::id())),
        };
        let time = |network: bool| {
            let sandbox = &sandbox;
            let ws = ws.clone();
            async move {
                let command = BashCommand {
                    command: "true".into(),
                    timeout_ms: None,
                    network,
                };
                let t = Instant::now();
                let answer = sandbox
                    .exec(&ws, &command, &CancellationToken::new())
                    .await
                    .expect("bash");
                assert_eq!(answer.exit_code, 0, "{}", answer.formatted);
                t.elapsed().as_secs_f64() * 1000.0
            }
        };
        let first = time(false).await;
        let mut later = Vec::new();
        for _ in 0..5 {
            later.push(time(false).await);
        }
        let net_first = time(true).await;
        let mut net_later = Vec::new();
        for _ in 0..5 {
            net_later.push(time(true).await);
        }
        println!(
            "{}",
            serde_json::json!({ "firstMs": first, "laterMs": later, "netFirstMs": net_first, "netLaterMs": net_later })
        );
        sandbox.destroy(&ws).await.expect("destroy");
    }
    sandbox.shutdown().await;
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("bash_bench measures the Linux sandbox");
    std::process::exit(2)
}
