//! One program on an empty world in the V8 child under the platform
//! sandbox; its logs go to stdout and the files it wrote to `<out-dir>`
//! (for `harness/math-probe.js`).
//!
//! `cargo run --release --example run_program -- <mcbench-build-child> <program.js> <out-dir>`

use async_trait::async_trait;
use mcbench_build::{BuildRequest, EncodedHost, V8Runner};
use tokio_util::sync::CancellationToken;

/// `encodeWorld(new World())`.
const EMPTY_WORLD: [u8; 10] = [2, 0, 0, 0, b'[', b']', 0, 0, 0, 0];

struct NoHost;

#[async_trait]
impl EncodedHost for NoHost {
    async fn read_file(&mut self, _: &str) -> Result<String, String> {
        unreachable!("readFile is not offered")
    }
    async fn settled_world(&mut self) -> Result<Vec<u8>, String> {
        unreachable!("mc.settled is not offered")
    }
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, source, out] = args.as_slice() else {
        panic!("usage: run_program <mcbench-build-child> <program.js> <out-dir>");
    };
    let runner = V8Runner::new(
        std::fs::canonicalize(program).unwrap(),
        mcbench_build::sandbox::platform().unwrap(),
    );
    let request = BuildRequest {
        source: std::fs::read_to_string(source).unwrap(),
        registry: serde_json::json!({ "stone": [{}, {}] }),
        read_file: false,
        settled_ticks: None,
    };
    let outcome = runner
        .run_encoded(
            &request,
            &EMPTY_WORLD,
            &mut NoHost,
            &CancellationToken::new(),
        )
        .await
        .unwrap_or_else(|failure| panic!("{}", failure.message));
    std::fs::create_dir_all(out).unwrap();
    for file in &outcome.files {
        std::fs::write(std::path::Path::new(out).join(&file.path), &file.text).unwrap();
    }
    for line in &outcome.logs {
        println!("{line}");
    }
}
