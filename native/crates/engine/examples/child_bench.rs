//! What an engine child costs, on recorded work (a recording as
//! `replay_bench` reads it, `requests.txt` with input files `@<n>` in
//! `in/<n>`):
//!
//!   child_bench <program> <tables> <recording>
//!
//! `<program>` is a built `mcbench-lite`. Printed, one JSON line: `start`,
//! the milliseconds from spawning a child to its `MCBENCH_ENGINE_READY`
//! (each of 10 children, sequentially); `verify`, the milliseconds of each
//! of the recording's `VERIFY`s on one warm child (the request written, the
//! reply read, the report read back); `settle`, the milliseconds of a
//! settle (`SettlePool::settle` at the default 210 ticks) of each of the
//! recording's fresh placements on a warm pool of one child.

use mcbench_engine::{EngineProgram, PlaceCommands, SettlePool, VerifyRequest};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn requests(root: &Path) -> Vec<(Vec<String>, PathBuf)> {
    let mut found = Vec::new();
    let mut walk = vec![root.to_path_buf()];
    while let Some(folder) = walk.pop() {
        if folder.join("requests.txt").is_file() {
            let lines = std::fs::read_to_string(folder.join("requests.txt"))
                .unwrap()
                .lines()
                .map(String::from)
                .collect();
            found.push((lines, folder.join("in")));
        } else {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk.push(path);
                }
            }
        }
    }
    found
}

fn list(ms: &[f64]) -> String {
    let words: Vec<String> = ms.iter().map(|ms| format!("{ms:.3}")).collect();
    format!("[{}]", words.join(","))
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [program, tables, recording] = args.as_slice() else {
        eprintln!("usage: child_bench <program> <tables> <recording>");
        std::process::exit(2);
    };
    let program = EngineProgram::new(program, tables);
    let never = CancellationToken::new();
    let mut start = Vec::new();
    for _ in 0..10 {
        let began = Instant::now();
        let child = program.spawn().unwrap();
        child.ready(&never).await.unwrap();
        start.push(began.elapsed().as_secs_f64() * 1000.0);
        child.stop().await;
    }
    let mut verifies = Vec::new();
    let mut places = Vec::new();
    for (lines, inputs) in requests(Path::new(recording)) {
        for line in lines {
            let words: Vec<&str> = line.split(' ').collect();
            let input = |i: usize| std::fs::read_to_string(inputs.join(&words[i][1..])).unwrap();
            match words[0] {
                "VERIFY" => verifies.push(VerifyRequest {
                    request: input(2),
                    commands: PlaceCommands(input(3)),
                }),
                "PLACE" if words[4] == "fresh" => places.push(PlaceCommands(input(2))),
                _ => {}
            }
        }
    }
    let child = program.spawn().unwrap();
    child.ready(&never).await.unwrap();
    // As the in-process counterpart does, one VERIFY before the clock.
    child.verify(&verifies[0], |_| {}, &never).await.unwrap();
    let mut verify = Vec::new();
    for request in &verifies {
        let began = Instant::now();
        child.verify(request, |_| {}, &never).await.unwrap();
        verify.push(began.elapsed().as_secs_f64() * 1000.0);
    }
    child.stop().await;
    let pool = SettlePool::new(program, 1, Duration::from_secs(600));
    pool.warm().await.unwrap();
    let mut settle = Vec::new();
    for commands in places {
        let began = Instant::now();
        pool.settle("bench", commands, vec![210]).await.unwrap();
        settle.push(began.elapsed().as_secs_f64() * 1000.0);
    }
    pool.stop().await;
    println!(
        "{{\"start\":{},\"verify\":{},\"settle\":{}}}",
        list(&start),
        list(&verify),
        list(&settle)
    );
}
