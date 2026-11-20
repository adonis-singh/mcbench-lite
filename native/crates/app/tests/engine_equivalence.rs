//! The in-process engine (`mcbench-engine`) against the released program's
//! protocol loop (`mcbench-lite headless engine`, `core/src/main.rs`
//! compiled in): the same request sequences through both must give the
//! same replies (but for their milliseconds) and the same bytes (blueprints,
//! patches, entities, views, state dumps, `SURVIVE`'s lines and `VERIFY`'s
//! reports but for their timings). `VERIFY` runs in engine children only
//! (`mcbench_engine::ChildEngine`): its side here is a child driven through
//! that API, which checks the files the API writes and reads back.
//!
//! The sequences are the synthetic ones below (placements, ticks, inputs,
//! the player, and the refusals `test/core-engine.test.js` provokes, with the
//! requests after them that a spent world refuses), and, when
//! `MCBENCH_ENGINE_RECORDING` names a folder, the traffic a parity script
//! sent the released program, recorded by the stage-three engine agent's
//! `record.py` (a folder per engine process, its `requests.txt` with input
//! files as `@<n>` in `in/<n>` and output files as `@out`). Needs
//! Minecraft's tables: `MCBENCH_TABLES` names their folder at build time.

use mcbench_engine::{
    Engine, EngineError, EngineProgram, EngineWorld, PlaceCommands, VerifyRequest,
};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use tokio_util::sync::CancellationToken;

fn tables() -> PathBuf {
    PathBuf::from(
        std::env::var("MCBENCH_TABLES").expect(
            "MCBENCH_TABLES names the folder of Minecraft's tables (core/minecraft/1.21.10)",
        ),
    )
}

fn engine() -> Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    *ENGINE.get_or_init(|| Engine::start(&tables()).unwrap())
}

/// One request: its protocol line, with input files as `@<n>` (in
/// `inputs`) and output files as `@out`.
struct Sequence {
    name: String,
    lines: Vec<String>,
    inputs: PathBuf,
}

/// What a request gave: its reply without the milliseconds, and its output
/// files' bytes.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    reply: String,
    files: Vec<(String, Vec<u8>)>,
}

fn without_ms(reply: &str) -> String {
    let words: Vec<&str> = reply.split(' ').collect();
    let done = [
        "MCBENCH_PLACED",
        "MCBENCH_TICKED",
        "MCBENCH_DONE",
        "MCBENCH_VERIFIED",
    ];
    if words.len() == 3 && done.contains(&words[0]) && words[2].bytes().all(|b| b.is_ascii_digit())
    {
        format!("{} {}", words[0], words[1])
    } else {
        reply.to_string()
    }
}

/// A verifier report as `scripts/core-verify-parity.js` compares it: its
/// timings (`"metrics":{...}`) and the byte count of a report that holds
/// them (`,"reportBytes":<n>`) left out.
fn untimed(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find("\"metrics\":{") {
        out.push_str(&rest[..at]);
        out.push_str("\"metrics\":{}");
        let after = &rest[at + "\"metrics\":{".len()..];
        rest = &after[after.find('}').map_or(after.len(), |end| end + 1)..];
    }
    out.push_str(rest);
    if let Some(at) = out.find(",\"reportBytes\":") {
        let digits = out[at + 15..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        out.replace_range(at..at + 15 + digits, "");
    }
    out.into_bytes()
}

/// A `VERIFY`'s output folder as compared: every file but `progress.json`
/// (written at most every 100 ms, so how far it got is a timing), reports
/// untimed.
fn verify_files(files: Vec<(String, Vec<u8>)>) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = files
        .into_iter()
        .filter(|(name, _)| name != "progress.json")
        .map(|(name, bytes)| {
            let bytes = if name.ends_with(".json") {
                untimed(&bytes)
            } else {
                bytes
            };
            (name, bytes)
        })
        .collect();
    files.sort();
    files
}

fn read_folder(folder: &Path) -> Vec<(String, Vec<u8>)> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    entries
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                std::fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

/// The sequence through `mcbench-lite headless engine`.
fn through_binary(sequence: &Sequence, scratch: &Path) -> Vec<Outcome> {
    let folder = scratch.join("binary");
    std::fs::create_dir_all(&folder).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcbench-lite"))
        .args(["headless", "engine"])
        .arg(tables())
        .current_dir(&folder)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    assert_eq!(lines.next().unwrap().unwrap(), "MCBENCH_ENGINE_READY");
    let mut outcomes = Vec::new();
    for (index, line) in sequence.lines.iter().enumerate() {
        let mut outputs = Vec::new();
        let words: Vec<String> = line
            .split(' ')
            .enumerate()
            .map(|(i, word)| {
                if word == "@out" {
                    let path = folder.join(format!("out-{index}-{i}"));
                    outputs.push(path.clone());
                    path.display().to_string()
                } else if let Some(n) = word.strip_prefix('@') {
                    sequence.inputs.join(n).display().to_string()
                } else {
                    word.to_string()
                }
            })
            .collect();
        writeln!(child.stdin.as_mut().unwrap(), "{}", words.join(" ")).unwrap();
        let reply = without_ms(&lines.next().unwrap().unwrap());
        let verify = words[0] == "VERIFY";
        let mut files = Vec::new();
        for path in outputs {
            if verify {
                files.extend(verify_files(read_folder(&path)));
                let _ = std::fs::remove_dir_all(&path);
            } else if let Ok(bytes) = std::fs::read(&path) {
                files.push((String::new(), bytes));
                std::fs::remove_file(&path).unwrap();
            }
        }
        outcomes.push(Outcome { reply, files });
    }
    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    outcomes
}

fn failed(fail: &str, id: &str, error: EngineError) -> String {
    match error {
        EngineError::Failed(reason) => format!("{fail} {id} {reason}"),
        other => panic!("not an answer of the engine: {other}"),
    }
}

/// The sequence through `EngineWorld`, its `VERIFY`s through an engine
/// child (`ChildEngine::verify`, which writes the request's files and reads
/// the report back as the verifier uses it).
async fn in_process(sequence: &Sequence) -> Vec<Outcome> {
    let engine = engine();
    let world = EngineWorld::new(engine).unwrap();
    let child = EngineProgram::new(env!("CARGO_BIN_EXE_mcbench-lite"), tables())
        .spawn()
        .unwrap();
    let mut outcomes = Vec::new();
    for line in &sequence.lines {
        let words: Vec<&str> = line.split(' ').collect();
        let id = words[1];
        let input =
            |i: usize| std::fs::read_to_string(sequence.inputs.join(&words[i][1..])).unwrap();
        let (ok, fail) = match words[0] {
            "PLACE" => ("MCBENCH_PLACED", "MCBENCH_PLACE_FAILED"),
            "TICK" | "PTICK" => ("MCBENCH_TICKED", "MCBENCH_TICK_FAILED"),
            "VERIFY" => ("MCBENCH_VERIFIED", "MCBENCH_VERIFY_FAILED"),
            _ => ("MCBENCH_DONE", "MCBENCH_FAILED"),
        };
        let done = format!("{ok} {id}");
        let outcome = |result: Result<Vec<Vec<u8>>, EngineError>| match result {
            Ok(files) => Outcome {
                reply: done.clone(),
                files: files
                    .into_iter()
                    .map(|bytes| (String::new(), bytes))
                    .collect(),
            },
            Err(error) => Outcome {
                reply: failed(fail, id, error),
                files: Vec::new(),
            },
        };
        let outcome = match words[0] {
            "PLACE" => {
                let fresh = words[4] == "fresh";
                outcome(
                    world
                        .place(&PlaceCommands(input(2)), fresh)
                        .await
                        .map(|b| vec![b.0]),
                )
            }
            "TICK" => {
                let light = words.get(5).is_none();
                outcome(
                    world
                        .tick(words[2].parse().unwrap(), light)
                        .await
                        .map(|t| vec![t.changes, t.entities]),
                )
            }
            "PTICK" => outcome(
                world
                    .player_tick(
                        words[2].parse().unwrap(),
                        words[3].parse().unwrap(),
                        words[4].parse().unwrap(),
                    )
                    .await
                    .map(|(t, view)| {
                        vec![
                            t.changes,
                            t.entities,
                            view.iter().flat_map(|v| v.to_le_bytes()).collect(),
                        ]
                    }),
            ),
            "PSPAWN" => outcome(
                world
                    .spawn_player(
                        [
                            words[2].parse().unwrap(),
                            words[3].parse().unwrap(),
                            words[4].parse().unwrap(),
                        ],
                        words[5].parse().unwrap(),
                        words[6].parse().unwrap(),
                        words[7].parse::<u8>().unwrap() == 1,
                    )
                    .await
                    .map(|()| vec![]),
            ),
            "PREMOVE" => outcome(world.remove_player().await.map(|()| vec![])),
            "INPUT" => outcome(
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
                    .map(|()| vec![]),
            ),
            "STATE" => outcome(world.state().await.map(|s| vec![s.into_bytes()])),
            "SURVIVE" => outcome(world.unsupported().await.map(|s| vec![s.into_bytes()])),
            "VERIFY" => {
                let request = VerifyRequest {
                    request: input(2),
                    commands: PlaceCommands(input(3)),
                };
                match child
                    .verify(&request, |_| {}, &CancellationToken::new())
                    .await
                {
                    Ok(report) => {
                        let mut files = report.files;
                        files.push(("result.json".into(), report.result.into_bytes()));
                        Outcome {
                            reply: done.clone(),
                            files: verify_files(files),
                        }
                    }
                    Err(error) => Outcome {
                        reply: failed(fail, id, error),
                        files: Vec::new(),
                    },
                }
            }
            other => panic!("no such request {other}"),
        };
        outcomes.push(outcome);
    }
    world.stop().await;
    child.stop().await;
    outcomes
}

/// A failed VERIFY leaves result.json in its folder for the protocol; the
/// in-process call answers the reason alone, so only the replies compare.
fn comparable(line: &str, binary: &Outcome) -> bool {
    !(line.starts_with("VERIFY") && binary.reply.starts_with("MCBENCH_VERIFY_FAILED"))
}

async fn compare(sequence: &Sequence) -> (usize, usize, Vec<String>) {
    let scratch = std::env::temp_dir().join(format!(
        "mcbench-equivalence-{}-{}",
        std::process::id(),
        sequence.name
    ));
    let _ = std::fs::remove_dir_all(&scratch);
    let binary = through_binary(sequence, &scratch);
    let inside = in_process(sequence).await;
    let mut failures = 0;
    for (index, line) in sequence.lines.iter().enumerate() {
        let (b, i) = (&binary[index], &inside[index]);
        let same = b.reply == i.reply && (!comparable(line, b) || b.files == i.files);
        if !same {
            failures += 1;
            if failures <= 5 {
                eprintln!(
                    "{} request {index} {line}: binary {:?} ({} files), in-process {:?} ({} files)",
                    sequence.name,
                    b.reply,
                    b.files.len(),
                    i.reply,
                    i.files.len()
                );
            }
        }
    }
    let _ = std::fs::remove_dir_all(&scratch);
    let refusals = binary
        .iter()
        .map(|o| o.reply.clone())
        .filter(|reply| reply.contains(" unported: "))
        .collect();
    (sequence.lines.len(), failures, refusals)
}

const COMMANDS: &str = "fill 0 64 0 5 64 5 minecraft:stone\nsetblock 1 65 1 minecraft:lever[face=floor,facing=north,powered=false]\nsetblock 2 65 1 minecraft:redstone_wire\nsetblock 3 65 1 minecraft:redstone_lamp\n";
const INFESTED: &str = "setblock 4 65 4 minecraft:infested_stone\nsetblock 5 65 4 minecraft:tnt[unstable=false]\nsetblock 5 65 3 minecraft:redstone_block\n";
const RESIN: &str = "setblock 0 65 0 minecraft:resin_clump[down=true]\n";
const PORTAL: &str = "fill 0 64 0 3 64 0 minecraft:obsidian\nfill 0 65 0 0 67 0 minecraft:obsidian\nfill 3 65 0 3 67 0 minecraft:obsidian\nfill 1 65 0 2 67 0 minecraft:nether_portal[axis=x]\nsetblock 1 72 0 minecraft:sand\n";

/// The synthetic sequences: `test/core-engine.test.js`'s worlds and
/// refusals, with every kind of request before and after a refusal.
fn synthetic(scratch: &Path) -> Vec<Sequence> {
    let inputs = scratch.join("inputs");
    std::fs::create_dir_all(&inputs).unwrap();
    for (name, text) in [
        ("commands", COMMANDS.to_string()),
        ("infested", INFESTED.to_string()),
        ("resin", RESIN.to_string()),
        ("portal", PORTAL.to_string()),
        ("sand", "setblock 2 72 0 minecraft:sand\n".to_string()),
        ("none", String::new()),
        ("both", format!("{COMMANDS}{RESIN}")),
    ] {
        std::fs::write(inputs.join(name), text).unwrap();
    }
    let sequence = |name: &str, lines: &[&str]| Sequence {
        name: name.into(),
        lines: lines.iter().map(|line| line.to_string()).collect(),
        inputs: inputs.clone(),
    };
    let mut portal = vec!["PLACE 1 @portal @out fresh".to_string()];
    for i in 0..20 {
        portal.push(format!("TICK {} 1 @out @out", i + 2));
    }
    portal.extend(
        [
            "PLACE 30 @sand @out continue",
            "TICK 31 3 @out @out",
            "TICK 32 1 @out @out",
            "STATE 33 @out",
            "PLACE 34 @portal @out fresh",
            "TICK 35 2 @out @out",
            "STATE 36 @out",
        ]
        .map(String::from),
    );
    vec![
        sequence(
            "refusal",
            &[
                "STATE 1 @out",
                "TICK 2 5 @out @out",
                "PLACE 3 @commands @out fresh",
                "TICK 4 1 @out @out",
                "INPUT 5 toggle 1 65 1 0",
                "INPUT 6 toggle 0 64 0 0",
                "STATE 7 @out",
                "TICK 8 2 @out @out nolight",
                "SURVIVE 9 @out",
                "PLACE 10 @infested @out continue",
                "TICK 11 80 @out @out",
                "TICK 12 1 @out @out",
                "STATE 13 @out",
                "SURVIVE 14 @out",
                "PSPAWN 15 0.5 65 0.5 0 0 1",
                "PLACE 16 @none @out continue",
                "INPUT 17 toggle 1 65 1 0",
                "PLACE 18 @commands @out fresh",
                "TICK 19 1 @out @out",
                "PSPAWN 20 0.5 65 0.5 0 0 1",
                "PTICK 21 0 0 0 @out @out @out",
                "PTICK 22 1 90 10 @out @out @out",
                "PTICK 23 17 45.5 -20 @out @out @out",
                "PREMOVE 24",
                "PTICK 25 0 0 0 @out @out @out",
                "STATE 26 @out",
                "SURVIVE 27 @out",
            ],
        ),
        sequence(
            "resin",
            &[
                "PLACE 1 @commands @out fresh",
                "PLACE 2 @resin @out continue",
                "TICK 3 1 @out @out",
                "INPUT 4 toggle 1 65 1 0",
                "PREMOVE 5",
                "PLACE 6 @both @out fresh",
                "STATE 7 @out",
                "PLACE 8 @commands @out fresh",
                "TICK 9 1 @out @out",
            ],
        ),
        Sequence {
            name: "portal".into(),
            lines: portal,
            inputs: inputs.clone(),
        },
    ]
}

/// The recorded sequences of `MCBENCH_ENGINE_RECORDING`, if it is set.
fn recorded() -> Vec<Sequence> {
    let Ok(root) = std::env::var("MCBENCH_ENGINE_RECORDING") else {
        return Vec::new();
    };
    let mut sequences = Vec::new();
    let mut walk = vec![PathBuf::from(root)];
    while let Some(folder) = walk.pop() {
        let requests = folder.join("requests.txt");
        if requests.is_file() {
            sequences.push(Sequence {
                name: folder.file_name().unwrap().to_string_lossy().into_owned(),
                lines: std::fs::read_to_string(&requests)
                    .unwrap()
                    .lines()
                    .map(String::from)
                    .collect(),
                inputs: folder.join("in"),
            });
        } else {
            for entry in std::fs::read_dir(&folder).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk.push(path);
                }
            }
        }
    }
    assert!(
        !sequences.is_empty(),
        "MCBENCH_ENGINE_RECORDING holds no requests.txt"
    );
    sequences.sort_by(|a, b| a.name.cmp(&b.name));
    sequences
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(mcbench_tables),
    ignore = "needs Minecraft's tables: set MCBENCH_TABLES at build time"
)]
async fn the_in_process_engine_gives_the_released_loops_replies_and_bytes() {
    let scratch = std::env::temp_dir().join(format!("mcbench-equivalence-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    let mut sequences = synthetic(&scratch);
    sequences.extend(recorded());
    let (mut requests, mut failures, mut refusals) = (0, 0, Vec::new());
    for sequence in &sequences {
        let (asked, failed, refused) = compare(sequence).await;
        requests += asked;
        failures += failed;
        refusals.extend(refused);
    }
    let spent = refusals
        .iter()
        .filter(|reply| reply.ends_with(mcbench_engine::SPENT))
        .count();
    eprintln!(
        "{} sequences, {requests} requests, {failures} differed; {} answered unported, {spent} of them on a spent world",
        sequences.len(),
        refusals.len()
    );
    // The synthetic sequences reach the refusals they were written for.
    for expected in [
        "MCBENCH_TICK_FAILED 11 unported: InfestedBlock.spawnAfterBreak",
        "MCBENCH_TICK_FAILED 12 unported: an earlier request",
        "MCBENCH_FAILED 15 unported: an earlier request",
        "MCBENCH_TICK_FAILED 31 unported: a draw of the entities' shared random",
    ] {
        assert!(
            refusals.iter().any(|reply| reply.starts_with(expected)),
            "no reply {expected}"
        );
    }
    let _ = std::fs::remove_dir_all(&scratch);
    assert_eq!(failures, 0, "{failures} of {requests} requests differed");
}
