//! The engine's speed on recorded work, three ways: a program speaking the
//! released protocol (the released `mcbench-core`, or `mcbench-lite
//! headless engine`) through files, or the in-process binding with no files
//! but `VERIFY`'s, which runs in an engine child of `<program>` (a built
//! `mcbench-lite`), as the app runs it.
//!
//!   replay_bench <tables> <recording folder> in-process <program>
//!   replay_bench <tables> <recording folder> program <program> [<argument>...]
//!
//! A recording is what the stage-three engine agent's `record.py` keeps of a
//! script's traffic (a folder per engine process with `requests.txt`, input
//! files `@<n>` in `in/<n>`, output files `@out`), or one made the same way.
//! The clock of a request starts when it is sent (the line written, or the
//! call made) and stops when its answer is read (the reply line, or the
//! call's result). Printed: one JSON line with the milliseconds summed by
//! phase: `place` (fresh placements), `tick` (ticks, player ticks, inputs
//! and continuing placements), `verify` (`VERIFY`), `other`, the request
//! counts, and how many failed (a `..._FAILED` reply, or an `Err`).

use mcbench_engine::{ChildEngine, Engine, EngineWorld, PlaceCommands, VerifyRequest};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use tokio_util::sync::CancellationToken;

struct Sequence {
    lines: Vec<String>,
    inputs: PathBuf,
}

fn sequences(root: &Path) -> Vec<Sequence> {
    let mut found = Vec::new();
    let mut walk = vec![root.to_path_buf()];
    while let Some(folder) = walk.pop() {
        if folder.join("requests.txt").is_file() {
            found.push((
                folder.clone(),
                Sequence {
                    lines: std::fs::read_to_string(folder.join("requests.txt"))
                        .unwrap()
                        .lines()
                        .map(String::from)
                        .collect(),
                    inputs: folder.join("in"),
                },
            ));
        } else {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk.push(path);
                }
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.into_iter().map(|(_, sequence)| sequence).collect()
}

fn words_verb(line: &str) -> &str {
    line.split(' ').next().unwrap()
}

fn phase(line: &str) -> &'static str {
    let words: Vec<&str> = line.split(' ').collect();
    match words[0] {
        "PLACE" if words.get(4) == Some(&"fresh") => "place",
        "PLACE" | "TICK" | "PTICK" | "INPUT" => "tick",
        "VERIFY" => "verify",
        _ => "other",
    }
}

#[derive(Default)]
struct Sums {
    ms: BTreeMap<&'static str, f64>,
    count: BTreeMap<&'static str, usize>,
    failed: BTreeMap<&'static str, usize>,
}

impl Sums {
    fn add(&mut self, line: &str, started: Instant, failed: bool) {
        let phase = phase(line);
        if failed {
            *self.failed.entry(phase).or_default() += 1;
        }
        *self.ms.entry(phase).or_default() += started.elapsed().as_secs_f64() * 1000.0;
        *self.count.entry(phase).or_default() += 1;
    }
}

fn through_program(
    tables: &Path,
    sequence: &Sequence,
    program: &[String],
    scratch: &Path,
    sums: &mut Sums,
) {
    let mut child = Command::new(&program[0])
        .args(&program[1..])
        .arg(tables)
        .current_dir(scratch)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "MCBENCH_ENGINE_READY");
    for (index, line) in sequence.lines.iter().enumerate() {
        let words: Vec<String> = line
            .split(' ')
            .enumerate()
            .map(|(i, word)| {
                if word == "@out" {
                    // A VERIFY's output is a folder, which may not be a file
                    // another request wrote.
                    let kind = if words_verb(line) == "VERIFY" {
                        "verify"
                    } else {
                        "out"
                    };
                    scratch
                        .join(format!("{kind}-{}-{i}", index % 4))
                        .display()
                        .to_string()
                } else if let Some(n) = word.strip_prefix('@') {
                    sequence.inputs.join(n).display().to_string()
                } else {
                    word.to_string()
                }
            })
            .collect();
        let started = Instant::now();
        writeln!(stdin, "{}", words.join(" ")).unwrap();
        stdin.flush().unwrap();
        let reply = lines.next().unwrap().unwrap();
        let failed = reply.split(' ').next().unwrap().ends_with("FAILED");
        sums.add(line, started, failed);
    }
    drop(stdin);
    child.wait().unwrap();
}

async fn in_process(engine: Engine, child: &ChildEngine, sequence: &Sequence, sums: &mut Sums) {
    let world = EngineWorld::new(engine).unwrap();
    for line in &sequence.lines {
        let words: Vec<&str> = line.split(' ').collect();
        let input =
            |i: usize| std::fs::read_to_string(sequence.inputs.join(&words[i][1..])).unwrap();
        // The commands are read before the clock starts, as a caller holds
        // them in memory.
        let commands = (words[0] == "PLACE").then(|| PlaceCommands(input(2)));
        let verify = (words[0] == "VERIFY").then(|| VerifyRequest {
            request: input(2),
            commands: PlaceCommands(input(3)),
        });
        let started = Instant::now();
        let answer = match words[0] {
            "PLACE" => world
                .place(commands.as_ref().unwrap(), words[4] == "fresh")
                .await
                .map(drop),
            "TICK" => world
                .tick(words[2].parse().unwrap(), words.get(5).is_none())
                .await
                .map(drop),
            "PTICK" => world
                .player_tick(
                    words[2].parse().unwrap(),
                    words[3].parse().unwrap(),
                    words[4].parse().unwrap(),
                )
                .await
                .map(drop),
            "PSPAWN" => {
                world
                    .spawn_player(
                        [
                            words[2].parse().unwrap(),
                            words[3].parse().unwrap(),
                            words[4].parse().unwrap(),
                        ],
                        words[5].parse().unwrap(),
                        words[6].parse().unwrap(),
                        words[7] == "1",
                    )
                    .await
            }
            "PREMOVE" => world.remove_player().await,
            "INPUT" => {
                world
                    .input(
                        words[2],
                        [
                            words[3].parse().unwrap(),
                            words[4].parse().unwrap(),
                            words[5].parse().unwrap(),
                        ],
                        words[6].parse().unwrap(),
                    )
                    .await
            }
            "STATE" => world.state().await.map(drop),
            "SURVIVE" => world.unsupported().await.map(drop),
            "VERIFY" => child
                .verify(verify.as_ref().unwrap(), |_| {}, &CancellationToken::new())
                .await
                .map(drop),
            other => panic!("no such request {other}"),
        };
        sums.add(line, started, answer.is_err());
    }
    world.stop().await;
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [tables, recording, mode, rest @ ..] = args.as_slice() else {
        eprintln!(
            "usage: replay_bench <tables> <recording> in-process <program> | program <program> [<argument>...]"
        );
        std::process::exit(2);
    };
    let (tables, recording) = (PathBuf::from(tables), PathBuf::from(recording));
    let sequences = sequences(&recording);
    let mut sums = Sums::default();
    match mode.as_str() {
        "in-process" => {
            let engine = Engine::start(&tables).unwrap();
            let child = mcbench_engine::EngineProgram::new(&rest[0], &tables)
                .spawn()
                .unwrap();
            child.ready(&CancellationToken::new()).await.unwrap();
            for sequence in &sequences {
                in_process(engine, &child, sequence, &mut sums).await;
            }
            child.stop().await;
        }
        "program" => {
            let scratch =
                std::env::temp_dir().join(format!("mcbench-replay-bench-{}", std::process::id()));
            std::fs::create_dir_all(&scratch).unwrap();
            for sequence in &sequences {
                through_program(&tables, sequence, rest, &scratch, &mut sums);
            }
            std::fs::remove_dir_all(&scratch).unwrap();
        }
        other => panic!("no mode {other}"),
    }
    let ms: Vec<String> = sums
        .ms
        .iter()
        .map(|(phase, ms)| format!("\"{phase}\":{ms:.3}"))
        .collect();
    let count: Vec<String> = sums
        .count
        .iter()
        .map(|(phase, n)| format!("\"{phase}\":{n}"))
        .collect();
    let failed: Vec<String> = sums
        .failed
        .iter()
        .map(|(phase, n)| format!("\"{phase}\":{n}"))
        .collect();
    println!(
        "{{\"ms\":{{{}}},\"requests\":{{{}}},\"failed\":{{{}}}}}",
        ms.join(","),
        count.join(","),
        failed.join(",")
    );
}
