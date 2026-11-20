//! The shell step's files as a shipped build fetches them, into the data
//! folder named by the one argument: this checkout's lock, the address the
//! build was given (`MCBENCH_SANDBOX_ASSETS_URL` at build time) or the
//! lock's, and this machine's architecture; no environment variable is read
//! for the files. Prints the shell step's state, each kept file, and how
//! long the step took and how long a sha256 of the kept files takes.

use std::sync::Arc;
use std::time::Instant;

use mcbench_setup::shell::{BUILD_ADDRESS, Build, LOCK, files_of};
use mcbench_setup::steps::{Steps, native_steps};
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), String> {
    let folder = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: shell_assets <data folder>")?,
    );
    let steps = Arc::new(Steps::new(native_steps(), Box::new(|_| {})));
    let build = Build {
        shipped: true,
        lock: LOCK,
        address: BUILD_ADDRESS,
        arch: std::env::consts::ARCH,
    };
    let started = Instant::now();
    let files = steps
        .track(
            "shell",
            files_of(
                &build,
                &|name| panic!("the shipped path read {name}"),
                steps.clone(),
                folder,
                mcbench_setup::fetch::client(),
                CancellationToken::new(),
            ),
        )
        .await;
    let took = started.elapsed();
    println!("address: {BUILD_ADDRESS:?}");
    println!("shell step: {:?}", steps.view().1[2].state);
    println!("fetch and check: {:.2} s", took.as_secs_f64());
    let files = files?;
    let started = Instant::now();
    let bytes = std::fs::read(&files.rootfs.file).map_err(|e| e.to_string())?;
    assert_eq!(mcbench_setup::bundle::sha256(&bytes), files.rootfs.sha256);
    println!(
        "rootfs: {} ({} bytes), sha256 again in {:.2} s",
        files.rootfs.file.display(),
        bytes.len(),
        started.elapsed().as_secs_f64()
    );
    Ok(())
}
