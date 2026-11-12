// mcbench-core as a program: the Java engine's protocol (McbenchEngine),
//
//   PLACE <id> <commands file> <blueprint file> <fresh|continue>
//   TICK <id> <ticks> <patch file> <entities file> [nolight]
//   INPUT <id> <toggle|press|use> <x> <y> <z> <yaw>
//   VERIFY <id> <request file> <commands file> <output folder>
//
// and, for the Rust engine only (the Java engine has no player), the
// creative player of web/core-engine.mjs:
//
//   PSPAWN <id> <x> <y> <z> <yaw> <pitch> <flying 0|1>
//   PTICK <id> <keys> <yaw> <pitch> <patch file> <entities file> <view file>
//   PREMOVE <id>
//
// PTICK answers as TICK does and writes the player's view (28 doubles, little
// endian, Session::player_view) to its last file.
//
// with the same replies and the same files, byte for byte (and STATE, for
// the parity scripts). The engine itself is the library (lib.rs); this only
// moves its bytes through files. A VERIFY is the exception to the refusal
// rule: it builds a level of its own, so a refusal leaves the live world as
// it was. A request with an unknown verb answers MCBENCH_FAILED <id>, and one
// with a missing or malformed word fails with its verb's failure reply; neither
// is a refusal.

use mcbench_core::session::Session;
use mcbench_core::{boot, failure, is_refusal, probe, verify};
use std::io::{BufRead, Write};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::time::Instant;

fn main() {
    // Block code recurses as deep as Minecraft's (512 nested block sets).
    std::thread::Builder::new().stack_size(1 << 30).spawn(serve).unwrap().join().unwrap();
}

// A file written whole or not at all.
fn save(target: &str, bytes: &[u8]) {
    let target = Path::new(target);
    let partial = target.with_file_name(format!("{}.part", target.file_name().unwrap().to_str().unwrap()));
    std::fs::File::create(&partial).and_then(|mut f| f.write_all(bytes)).expect("write");
    std::fs::rename(&partial, target).expect("rename");
}

fn serve() {
    std::panic::set_hook(Box::new(|info| {
        if !is_refusal(info.payload()) {
            eprintln!("{info}");
        }
    }));
    let (reg, blocks) = boot();
    if std::env::args().any(|a| a == "--probe") {
        return probe::run(reg, blocks);
    }
    // The player's movement for the cases of a file (scripts/movement-parity.js).
    // Saved data upgraded by the data fixer, a line each (scripts/datafix-parity.js).
    if std::env::args().any(|a| a == "--datafix") {
        let mut input = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut input).expect("read the lines");
        print!("{}", mcbench_core::datafix::run_lines(&input));
        return;
    }
    if let Some(at) = std::env::args().position(|a| a == "--movement") {
        let file = std::env::args().nth(at + 1).expect("a cases file");
        print!("{}", mcbench_core::player::script::run(reg, blocks, &std::fs::read_to_string(file).expect("cases")));
        return;
    }
    let mut session = Session::new(reg, blocks);
    // A refusal leaves a level the server would not have made.
    let mut refused = false;
    let mut out = std::io::stdout();
    writeln!(out, "MCBENCH_ENGINE_READY").unwrap();
    out.flush().unwrap();
    for line in std::io::stdin().lock().lines() {
        let line = line.unwrap();
        let request: Vec<&str> = line.split(' ').collect();
        let start = Instant::now();
        let id = request.get(1).copied().unwrap_or("?");
        let (ok, fail) = match request[0] {
            "PLACE" => ("MCBENCH_PLACED", "MCBENCH_PLACE_FAILED"),
            "TICK" | "PTICK" => ("MCBENCH_TICKED", "MCBENCH_TICK_FAILED"),
            "INPUT" | "STATE" | "PSPAWN" | "PREMOVE" => ("MCBENCH_DONE", "MCBENCH_FAILED"),
            "VERIFY" => ("MCBENCH_VERIFIED", "MCBENCH_VERIFY_FAILED"),
            other => {
                writeln!(out, "MCBENCH_FAILED {id} unknown request {other}").unwrap();
                out.flush().unwrap();
                continue;
            }
        };
        // A request's words; one missing or malformed fails the request
        // before it reaches the world.
        let word = |i: usize| request.get(i).copied().ok_or_else(|| format!("malformed request: {line}"));
        fn number<T: std::str::FromStr>(word: Result<&str, String>, line: &str) -> Result<T, String> {
            word?.parse().map_err(|_| format!("malformed request: {line}"))
        }
        let read = |file: &str| std::fs::read_to_string(file).map_err(|error| format!("{file}: {error}"));
        // A VERIFY runs on a level of its own: a refusal or a failure leaves
        // the live world as it was.
        if request[0] == "VERIFY" {
            let result = catch_unwind(AssertUnwindSafe(|| verify::verify(reg, blocks, Path::new(word(2)?), Path::new(word(3)?), Path::new(word(4)?))));
            let reply = match result {
                Ok(Ok(())) => format!("{ok} {id} {}", start.elapsed().as_millis()),
                Ok(Err(reason)) => format!("{fail} {id} {reason}"),
                Err(payload) => format!("{fail} {id} {}", failure(&*payload)),
            };
            writeln!(out, "{reply}").unwrap();
            out.flush().unwrap();
            continue;
        }
        if request[0] == "PLACE" && request.get(4) == Some(&"fresh") {
            refused = false;
        }
        let result: Result<Result<(), String>, Box<dyn std::any::Any + Send>> = if refused {
            Ok(Err("unported: an earlier request reached what this port does not model".into()))
        } else {
            catch_unwind(AssertUnwindSafe(|| match request[0] {
                "PLACE" => {
                    let fresh = match word(4)? {
                        "fresh" => true,
                        "continue" => false,
                        _ => return Err(format!("malformed request: {line}")),
                    };
                    if fresh {
                        session.renew();
                    }
                    let blueprint = session.place(&read(word(2)?)?, false)?;
                    save(word(3)?, &blueprint);
                    Ok(())
                }
                "TICK" => {
                    let light = match request.get(5) {
                        None => true,
                        Some(&"nolight") => false,
                        Some(_) => return Err(format!("malformed request: {line}")),
                    };
                    let (patch, entities) = (word(3)?, word(4)?);
                    let ticked = session.tick(number(word(2), &line)?, light)?;
                    save(entities, &ticked.entities);
                    save(patch, &ticked.changes);
                    Ok(())
                }
                "PTICK" => {
                    let (patch, entities, view_file) = (word(5)?, word(6)?, word(7)?);
                    let (ticked, view) = session.player_tick(number(word(2), &line)?, number(word(3), &line)?, number(word(4), &line)?)?;
                    save(entities, &ticked.entities);
                    save(patch, &ticked.changes);
                    save(view_file, &view.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
                    Ok(())
                }
                "PSPAWN" => {
                    let flying = number::<u8>(word(7), &line)? == 1;
                    session.spawn_player([number(word(2), &line)?, number(word(3), &line)?, number(word(4), &line)?], number(word(5), &line)?, number(word(6), &line)?, flying);
                    Ok(())
                }
                "PREMOVE" => {
                    session.remove_player();
                    Ok(())
                }
                "STATE" => {
                    let file = word(2)?;
                    std::fs::write(file, session.state()).expect("write");
                    Ok(())
                }
                "INPUT" => {
                    let kind = word(2)?;
                    session.input(kind, number(word(3), &line)?, number(word(4), &line)?, number(word(5), &line)?, number(word(6), &line)?)?;
                    Ok(())
                }
                _ => unreachable!(),
            }))
        };
        let reply = match result {
            Ok(Ok(())) => format!("{ok} {id} {}", start.elapsed().as_millis()),
            Ok(Err(reason)) => format!("{fail} {id} {reason}"),
            Err(payload) => {
                refused = true;
                format!("{fail} {id} {}", failure(&*payload))
            }
        };
        writeln!(out, "{reply}").unwrap();
        out.flush().unwrap();
    }
}
