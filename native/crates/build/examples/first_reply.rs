//! The build child's start as the runner sees it, on this platform's
//! sandbox (on Windows: through `wsl.exe` into the app's WSL2 distro):
//!
//! | Field | Clock |
//! |---|---|
//! | `coldMs` | Windows: after `wsl --terminate` of the distro; elsewhere the first child of the process: `spawn` to the first byte of the child's answer to a one-block program |
//! | `warmMs` | the same with the distro (or the machine) already running and the sandbox made (on Windows the program already copied into the distro), `runs` times, 300 ms apart |
//! | `spareMs` | `V8Runner::run_encoded` of the same program on a runner whose spare process is already running, `runs` times, 1.5 s apart: the call to its answer |
//!
//! `cargo run --example first_reply -- <mcbench-build-child> <runs>`; prints
//! JSON (each list sorted, and its median).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mcbench_build::{BuildRequest, EncodedHost, V8Runner};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

const EMPTY_WORLD: [u8; 10] = [2, 0, 0, 0, b'[', b']', 0, 0, 0, 0];
const ONE_BLOCK: &str = r#"module.exports = (mc) => mc.set([0, 60, 0], "gold_block")"#;

struct Host;

#[async_trait]
impl EncodedHost for Host {
    async fn read_file(&mut self, _: &str) -> Result<String, String> {
        unreachable!("the program reads no file")
    }
    async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
        unreachable!("the program reads no settled world")
    }
}

fn registry() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../tests/golden/registries/c899b5826e2e1be4.json"
    ))
    .unwrap()
}

/// `spawn` to the first byte the child writes, in ms.
async fn first_reply(sandbox: &dyn mcbench_build::ChildSandbox, program: &std::path::Path) -> f64 {
    let request = serde_json::to_vec(&json!({
        "source": ONE_BLOCK, "registry": registry(), "readFile": false, "settledTicks": null,
    }))
    .unwrap();
    let mut bytes = (request.len() as u32).to_le_bytes().to_vec();
    bytes.extend_from_slice(&request);
    bytes.extend_from_slice(&(EMPTY_WORLD.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&EMPTY_WORLD);
    let started = Instant::now();
    let mut child = sandbox.spawn(program, &[]).unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&bytes).await.unwrap();
    let mut first = [0u8; 1];
    child
        .stdout
        .take()
        .unwrap()
        .read_exact(&mut first)
        .await
        .unwrap();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    child.kill().await.unwrap();
    ms
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    values[values.len() / 2]
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, runs] = args.as_slice() else {
        eprintln!("usage: first_reply <mcbench-build-child> <runs>");
        std::process::exit(2)
    };
    let program = PathBuf::from(program);
    let runs: usize = runs.parse().unwrap();
    #[cfg(windows)]
    {
        let distro = mcbench_build::sandbox::wsl::distro();
        let status = mcbench_build::sandbox::wsl::manage()
            .unwrap()
            .args(["--terminate", &distro])
            .status()
            .unwrap();
        assert!(status.success(), "wsl --terminate {distro}");
    }
    let sandbox = mcbench_build::sandbox::platform().unwrap();
    let cold = first_reply(&*sandbox, &program).await;
    let mut warm = Vec::new();
    for _ in 0..runs {
        tokio::time::sleep(Duration::from_millis(300)).await;
        warm.push(first_reply(&*sandbox, &program).await);
    }
    let runner = V8Runner::new(program, mcbench_build::sandbox::platform().unwrap());
    let request = BuildRequest {
        source: ONE_BLOCK.to_string(),
        registry: registry(),
        read_file: false,
        settled_ticks: None,
    };
    let mut spare = Vec::new();
    for _ in 0..runs {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let started = Instant::now();
        runner
            .run_encoded(&request, &EMPTY_WORLD, &mut Host, &CancellationToken::new())
            .await
            .unwrap();
        spare.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    let warm_median = median(&mut warm);
    let spare_median = median(&mut spare);
    println!(
        "{}",
        json!({
            "coldMs": cold,
            "warmMs": warm, "warmMedianMs": warm_median,
            "spareMs": spare, "spareMedianMs": spare_median,
        })
    );
}
