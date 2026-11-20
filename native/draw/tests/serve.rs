// The helper as the server runs it: `mcbench-draw serve` spoken to over
// stdin and stdout with PROTOCOL.md's frames, on the synthetic pack.

mod common;

use mcbench_draw::compare::{self, Image};
use mcbench_draw::serve::{read_frame, write_frame, Frame};
use mcbench_draw::synthetic;
use serde_json::{json, Value};
use std::io::{BufReader, BufWriter, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct Helper {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Helper {
    fn start() -> Helper {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcbench-draw"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let input = BufWriter::new(child.stdin.take().unwrap());
        let output = BufReader::new(child.stdout.take().unwrap());
        Helper {
            child,
            input,
            output,
        }
    }

    fn send(&mut self, header: Value, payloads: &[&[u8]]) {
        write_frame(&mut self.input, header, payloads).unwrap();
        self.input.flush().unwrap();
    }

    fn reply(&mut self) -> Frame {
        read_frame(&mut self.output).unwrap().expect("a reply")
    }

    fn ask(&mut self, header: Value, payloads: &[&[u8]]) -> Frame {
        self.send(header, payloads);
        self.reply()
    }
}

fn hello(h: &mut Helper) -> Value {
    let mut p = synthetic::paths(&common::crate_dir().join("tests/synthetic"));
    p.software_vulkan = common::software_vulkan();
    let mut header =
        json!({ "id": 1, "type": "hello", "assets": serde_json::to_value(&p).unwrap() });
    if common::software() {
        header["adapter"] = json!("software");
    }
    h.ask(header, &[]).header
}

#[test]
fn hello_world_shots_drop_and_errors() {
    let mut h = Helper::start();
    // Before hello.
    let r = h
        .ask(
            json!({ "id": "a", "type": "world", "world": "w", "yOffset": 64 }),
            &[b"x"],
        )
        .header;
    assert_eq!(
        r["error"],
        "The helper has not been given its assets (hello) yet"
    );
    let r = hello(&mut h);
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["id"], 1);
    assert!(r["adapter"]["name"].is_string());
    // Forced, the floor; otherwise whatever the machine has (WARP in a VM).
    if common::software() {
        assert_eq!(r["adapter"]["software"], true, "{r}");
    }
    assert!(r["adapter"]["software"].is_boolean());
    assert_eq!(
        r["limits"]["worldsKept"],
        if r["adapter"]["software"] == true {
            2
        } else {
            4
        }
    );
    assert_eq!(
        r["limits"]["memoryLimit"],
        if r["adapter"]["software"] == true {
            768u64 << 20
        } else {
            1u64 << 30
        }
    );
    assert_eq!(
        h.ask(json!({ "id": 2, "type": "hello", "assets": {} }), &[])
            .header["error"],
        "hello was already given"
    );
    // A world.
    let blueprint = synthetic::blueprint(&synthetic::world());
    let r = h
        .ask(
            json!({ "id": 3, "type": "world", "world": "chat-1@5", "yOffset": 64 }),
            &[&blueprint],
        )
        .header;
    assert_eq!(r["ok"], true, "{r}");
    assert!(r["quads"].as_u64().unwrap() > 100);
    assert_eq!(
        r["bounds"],
        json!({ "min": [-10, 0, -10], "max": [10, 4, 10] })
    );
    // Its shots, as the reference pictures.
    let shots = synthetic::shots();
    let r = h.ask(json!({ "id": 4, "type": "shots", "world": "chat-1@5", "shots": shots, "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]);
    assert_eq!(r.header["ok"], true, "{}", r.header);
    assert_eq!(r.payloads.len(), 5);
    for (i, jpeg) in r.payloads.iter().enumerate() {
        let ours = Image::decode(jpeg).unwrap();
        let reference =
            Image::open(&common::crate_dir().join(format!("tests/synthetic/reference/{i}.png")))
                .unwrap();
        let stats = compare::colour(&reference, &ours, None).unwrap();
        assert!(stats.mean_abs < 3.0, "shot {i}: {stats:?}");
        assert_eq!(r.header["shots"][i]["bytes"], jpeg.len());
    }
    // Unknown fields and worlds.
    let r = h.ask(json!({ "id": 5, "type": "shots", "world": "nope", "shots": [], "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]).header;
    assert_eq!(r["error"], "No world named nope is kept");
    assert_eq!(r["unknownWorld"], true);
    let r = h.ask(json!({ "id": 6, "type": "shots", "world": "chat-1@5", "shots": [], "supersample": 2, "format": { "type": "gif" } }), &[]).header;
    assert!(
        r["error"].as_str().unwrap().starts_with("format must be"),
        "{r}"
    );
    let r = h
        .ask(
            json!({ "id": 7, "type": "world", "world": "w", "yOffset": 64, "colour": 1 }),
            &[&blueprint],
        )
        .header;
    assert_eq!(r["error"], "A world request takes no field colour");
    let r = h.ask(json!({ "id": 8, "type": "paint" }), &[]).header;
    assert_eq!(r["error"], "Unknown request type paint");
    // Cancel of nothing in flight.
    let r = h
        .ask(json!({ "id": 9, "type": "cancel", "job": 4 }), &[])
        .header;
    assert_eq!(r["found"], false);
    // Drop.
    assert_eq!(
        h.ask(
            json!({ "id": 10, "type": "drop", "world": "chat-1@5" }),
            &[]
        )
        .header["dropped"],
        true
    );
    assert_eq!(
        h.ask(
            json!({ "id": 11, "type": "drop", "world": "chat-1@5" }),
            &[]
        )
        .header["dropped"],
        false
    );
    drop(h.input);
    assert!(h.child.wait().unwrap().success());
}

#[test]
fn a_cancelled_shots_request_answers_cancelled() {
    let mut h = Helper::start();
    assert_eq!(hello(&mut h)["ok"], true);
    let blueprint = synthetic::blueprint(&synthetic::world());
    assert_eq!(
        h.ask(
            json!({ "id": 1, "type": "world", "world": "w", "yOffset": 64 }),
            &[&blueprint]
        )
        .header["ok"],
        true
    );
    // Many shots, then a cancel at once: the cancel is answered first.
    let one = synthetic::shots()[0].clone();
    let shots: Vec<Value> = (0..60).map(|_| one.clone()).collect();
    h.send(json!({ "id": "job", "type": "shots", "world": "w", "shots": shots, "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]);
    h.send(json!({ "id": "stop", "type": "cancel", "job": "job" }), &[]);
    let first = h.reply().header;
    assert_eq!(first["id"], "stop");
    assert_eq!(first["found"], true);
    let second = h.reply().header;
    assert_eq!(second["id"], "job");
    assert_eq!(second["error"], "cancelled");
    drop(h.input);
    assert!(h.child.wait().unwrap().success());
}

#[test]
fn montage_places_each_picture_at_its_cell() {
    let mut h = Helper::start();
    let tile = |rgb: [u8; 3], w: u32, h: u32| {
        mcbench_draw::picture::jpeg(&rgb.repeat((w * h) as usize), w, h, 95).unwrap()
    };
    let (a, b) = (tile([200, 30, 30], 64, 48), tile([30, 30, 200], 64, 48));
    let r = h.ask(json!({ "id": 1, "type": "montage", "size": [128, 48], "cells": [{ "x": 0, "y": 0, "width": 64, "height": 48 }, { "x": 64, "y": 0, "width": 64, "height": 48 }], "quality": 90 }), &[&a, &b]);
    assert_eq!(r.header["ok"], true, "{}", r.header);
    let card = Image::decode(&r.payloads[0]).unwrap();
    assert_eq!((card.w, card.h), (128, 48));
    let at = |x: usize, y: usize| card.rgb[y * 128 + x];
    assert!(at(20, 20)[0] > 150.0 && at(20, 20)[2] < 80.0);
    assert!(at(100, 20)[2] > 150.0 && at(100, 20)[0] < 80.0);
    let r = h.ask(json!({ "id": 2, "type": "montage", "size": [128, 48], "cells": [{ "x": 0, "y": 0, "width": 64, "height": 48 }], "quality": 90 }), &[&a, &b]).header;
    assert_eq!(r["error"], "A card of 1 cells was given 2 pictures");
    drop(h.input);
    assert!(h.child.wait().unwrap().success());
}

// Linux without a shipped software renderer (arm64 in this stage): asked
// for the floor, hello says so in one sentence and searches for nothing.
#[cfg(target_os = "linux")]
#[test]
fn linux_without_a_shipped_floor_says_so() {
    let mut h = Helper::start();
    let p = synthetic::paths(&common::crate_dir().join("tests/synthetic"));
    let r = h.ask(json!({ "id": 1, "type": "hello", "assets": serde_json::to_value(&p).unwrap(), "adapter": "software" }), &[]).header;
    let expected = format!("This machine has no graphics adapter, and this build for Linux {} ships no software renderer", std::env::consts::ARCH);
    assert_eq!(r["error"], expected.as_str(), "{r}");
    // It stays up answering only that.
    let r = h
        .ask(
            json!({ "id": 2, "type": "world", "world": "w", "yOffset": 64 }),
            &[b"x"],
        )
        .header;
    assert_eq!(r["error"], expected.as_str());
    drop(h.input);
    assert!(h.child.wait().unwrap().success());
}

#[test]
fn a_world_named_as_an_edit_of_its_base_is_patched() {
    let mut h = Helper::start();
    assert_eq!(hello(&mut h)["ok"], true);
    let mut blocks = synthetic::world();
    let first = synthetic::blueprint(&blocks);
    assert_eq!(
        h.ask(
            json!({ "id": 1, "type": "world", "world": "c@1", "yOffset": 64 }),
            &[&first]
        )
        .header["ok"],
        true
    );
    blocks.push(([9, 65, 9], 3));
    let second = synthetic::blueprint(&blocks);
    let r = h
        .ask(
            json!({ "id": 2, "type": "world", "world": "c@2", "yOffset": 64, "base": "c@1" }),
            &[&second],
        )
        .header;
    assert_eq!(r["patched"], true, "{r}");
    assert!(r["tilesMeshed"].as_u64().unwrap() < r["tiles"].as_u64().unwrap());
    // The base is dropped; asked again it is unknown.
    let r = h
        .ask(
            json!({ "id": 3, "type": "world", "world": "c@3", "yOffset": 64, "base": "c@1" }),
            &[&second],
        )
        .header;
    assert_eq!(r["unknownWorld"], true);
    // keepBase: read fresh, the base stays.
    let r = h.ask(json!({ "id": 4, "type": "world", "world": "c@3", "yOffset": 64, "base": "c@2", "keepBase": true }), &[&first]).header;
    assert!(r["patched"].is_null(), "{r}");
    assert_eq!(
        h.ask(json!({ "id": 5, "type": "drop", "world": "c@2" }), &[])
            .header["dropped"],
        true
    );
    drop(h.input);
    assert!(h.child.wait().unwrap().success());
}

// viewer-reset: a world drawn after other worlds were drawn and dropped is
// drawn as a fresh helper draws it.
#[test]
fn a_world_after_dropped_worlds_draws_as_on_a_fresh_helper() {
    let shots = synthetic::shots();
    let format = json!({ "type": "jpeg", "quality": 90 });
    let mut other = synthetic::world();
    other.retain(|(at, _)| at[2] < 6);
    let mut used = Helper::start();
    assert_eq!(hello(&mut used)["ok"], true);
    for (i, name) in ["x", "y"].iter().enumerate() {
        assert_eq!(
            used.ask(
                json!({ "id": i, "type": "world", "world": name, "yOffset": 64 }),
                &[&synthetic::blueprint(&other)]
            )
            .header["ok"],
            true
        );
        assert_eq!(used.ask(json!({ "id": 10 + i, "type": "shots", "world": name, "shots": shots, "supersample": 2, "format": format }), &[]).header["ok"], true);
        assert_eq!(
            used.ask(json!({ "id": 20 + i, "type": "drop", "world": name }), &[])
                .header["dropped"],
            true
        );
    }
    let blueprint = synthetic::blueprint(&synthetic::world());
    let draw = |h: &mut Helper| {
        assert_eq!(
            h.ask(
                json!({ "id": 30, "type": "world", "world": "w", "yOffset": 64 }),
                &[&blueprint]
            )
            .header["ok"],
            true
        );
        h.ask(json!({ "id": 31, "type": "shots", "world": "w", "shots": shots, "supersample": 2, "format": format }), &[]).payloads
    };
    let after = draw(&mut used);
    let mut fresh = Helper::start();
    assert_eq!(hello(&mut fresh)["ok"], true);
    assert!(after == draw(&mut fresh), "the same JPEG bytes");
    for h in [used, fresh] {
        drop(h.input);
        let mut child = h.child;
        assert!(child.wait().unwrap().success());
    }
}

// The bound is the process's own memory (PROTOCOL.md, What is kept): after a
// world whose scene alone passes the limit is drawn, the helper's resident
// memory is back within the limit (plus the stated overhead), and its peak
// within the limit plus that scene's quads, its picture targets and the
// meshing's scratch. Read from /proc/<pid>/status (Linux).
#[test]
fn resident_memory_stays_within_the_bound() {
    if !cfg!(target_os = "linux") {
        eprintln!("resident_memory_stays_within_the_bound: skipped: it reads /proc (Linux)");
        return;
    }
    const LIMIT: u64 = 512 << 20;
    const OVERHEAD: u64 = 32 << 20;
    let mut h = Helper::start();
    let mut p = synthetic::paths(&common::crate_dir().join("tests/synthetic"));
    p.software_vulkan = common::software_vulkan();
    let mut header = json!({ "id": 1, "type": "hello", "assets": serde_json::to_value(&p).unwrap(), "memoryLimit": LIMIT });
    if common::software() {
        header["adapter"] = json!("software");
    }
    let r = h.ask(header, &[]).header;
    assert_eq!(r["ok"], true, "{r}");
    let d = std::f64::consts::PI / 180.0;
    let shots = json!([
        { "width": 960, "height": 720, "clip": null, "grid": null, "camera": { "yaw": -45.0 * d, "pitch": 30.0 * d, "focus": [64.0, 32.0, 64.0], "focusDist": 220.0, "fov": 70 } },
        { "width": 960, "height": 720, "clip": null, "grid": null, "camera": { "yaw": 135.0 * d, "pitch": 20.0 * d, "focus": [64.0, 32.0, 64.0], "focusDist": 220.0, "fov": 70 } },
    ]);
    // A small world first: kept, then let go for the large one.
    let small = synthetic::blueprint(&synthetic::world());
    assert_eq!(
        h.ask(
            json!({ "id": 2, "type": "world", "world": "small", "yOffset": 64 }),
            &[&small]
        )
        .header["ok"],
        true
    );
    assert_eq!(h.ask(json!({ "id": 3, "type": "shots", "world": "small", "shots": synthetic::shots(), "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]).header["ok"], true);
    let large = checkerboard();
    let w = h
        .ask(
            json!({ "id": 4, "type": "world", "world": "large", "yOffset": 64 }),
            &[&large],
        )
        .header;
    assert_eq!(w["ok"], true, "{w}");
    let scene = w["bytes"].as_u64().unwrap();
    assert!(
        scene > LIMIT / 2,
        "the large world's quads are {scene} bytes"
    );
    let s = h.ask(json!({ "id": 5, "type": "shots", "world": "large", "shots": shots, "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]);
    assert_eq!(s.header["ok"], true, "{}", s.header);
    // Freed memory is given back after the reply, while no request waits:
    // given up to 2 s (it takes milliseconds).
    let read = || {
        let status = std::fs::read_to_string(format!("/proc/{}/status", h.child.id())).unwrap();
        let kb = |name: &str| {
            status.lines().find(|l| l.starts_with(name)).unwrap()[name.len()..]
                .trim()
                .trim_end_matches(" kB")
                .trim()
                .parse::<u64>()
                .unwrap()
                * 1024
        };
        (kb("VmRSS:"), kb("VmHWM:"))
    };
    let start = std::time::Instant::now();
    let (mut resident, mut peak) = read();
    while resident > LIMIT + OVERHEAD && start.elapsed().as_secs_f64() < 2.0 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        (resident, peak) = read();
    }
    eprintln!(
        "trimmed {:.0} ms after the reply",
        start.elapsed().as_secs_f64() * 1e3
    );
    // The picture targets of a 960 x 720 shot at supersample 2: the resolved
    // picture, multisampled colour and depth (4 samples), the picture and
    // its readback.
    let targets = 1920 * 1440 * (4 + 4 * 8) + 2 * 960 * 720 * 4;
    let scratch = 256 << 20;
    eprintln!(
        "resident {} MB, peak {} MB (limit {} MB, scene {} MB)",
        resident >> 20,
        peak >> 20,
        LIMIT >> 20,
        scene >> 20
    );
    assert!(
        resident <= LIMIT + OVERHEAD,
        "resident {resident} after the call, over the limit {LIMIT} plus {OVERHEAD}"
    );
    assert!(peak <= LIMIT + scene + targets + scratch, "peak {peak} over the limit, the scene's {scene}, the targets' {targets} and {scratch} of scratch");
    // The large world is still kept (its blueprint): drawn again on asking.
    let s = h.ask(json!({ "id": 6, "type": "shots", "world": "large", "shots": [shots[0].clone()], "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]);
    assert_eq!(s.header["ok"], true, "{}", s.header);
}

// Stone and glass alternating over 128 x 64 x 128: a million blocks, every
// stone's six faces drawn, 3 million quads (300 MB).
fn checkerboard() -> Vec<u8> {
    let mut blocks = Vec::with_capacity(128 * 64 * 128);
    for x in 0..128 {
        for y in 64..128 {
            for z in 0..128 {
                blocks.push((
                    if (x + y + z) % 2 == 0 {
                        "test:stone"
                    } else {
                        "test:glass"
                    },
                    [x, y, z],
                ));
            }
        }
    }
    common::blueprint(&blocks)
}

fn hello_with_limit(h: &mut Helper, limit: u64) {
    let mut p = synthetic::paths(&common::crate_dir().join("tests/synthetic"));
    p.software_vulkan = common::software_vulkan();
    let mut header = json!({ "id": 1, "type": "hello", "assets": serde_json::to_value(&p).unwrap(), "memoryLimit": limit });
    if common::software() {
        header["adapter"] = json!("software");
    }
    assert_eq!(h.ask(header, &[]).header["ok"], true);
}

// Five calls drawing the held world `name`: each call's time spent making
// scenes (its shots' reply timings `scene`, ms), 0 for a call that drew a
// scene it held.
fn held_scenes(h: &mut Helper, name: &str, shots: &Value) -> Vec<f64> {
    (0..5)
        .map(|i| {
            let r = h.ask(json!({ "id": format!("{name}-{i}"), "type": "shots", "world": name, "shots": shots, "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]);
            assert_eq!(r.header["ok"], true, "{}", r.header);
            r.header["shots"].as_array().unwrap().iter().map(|s| s["timings"]["scene"].as_f64().unwrap()).sum()
        })
        .collect()
}

// After a large world (one that passes the limit where the GPU's memory is
// counted), a small held world's scene is made once and then kept: its calls make no scene but, at most, the first (its
// scene may have been let go to make room for the large one) and then draw
// the scene they hold, as every call on a fresh helper does (PROTOCOL.md,
// What is kept). It counts the scenes made (the shots' reply timings
// `scene`), not time; the store's decisions rest on its own counts, so it
// holds on any load.
#[test]
fn a_small_worlds_scene_is_made_once_and_kept_after_a_large_world() {
    // The large world's scene, its picture targets and the helper's own
    // pass it where the GPU's memory is counted; the small world fits.
    const LIMIT: u64 = 512 << 20;
    let small = synthetic::blueprint(&synthetic::world());
    let shots = json!([synthetic::shots()[0].clone()]);
    let fresh = {
        let mut h = Helper::start();
        hello_with_limit(&mut h, LIMIT);
        assert_eq!(
            h.ask(
                json!({ "id": 2, "type": "world", "world": "small", "yOffset": 64 }),
                &[&small]
            )
            .header["ok"],
            true
        );
        held_scenes(&mut h, "small", &shots)
    };
    assert!(
        fresh.iter().all(|&s| s == 0.0),
        "a fresh helper's calls draw the scene it holds: {fresh:?}"
    );
    let mut h = Helper::start();
    hello_with_limit(&mut h, LIMIT);
    assert_eq!(
        h.ask(
            json!({ "id": 2, "type": "world", "world": "small", "yOffset": 64 }),
            &[&small]
        )
        .header["ok"],
        true
    );
    held_scenes(&mut h, "small", &shots);
    let d = std::f64::consts::PI / 180.0;
    let large_shots = json!([{ "width": 960, "height": 720, "clip": null, "grid": null, "camera": { "yaw": -45.0 * d, "pitch": 30.0 * d, "focus": [64.0, 32.0, 64.0], "focusDist": 220.0, "fov": 70 } }]);
    assert_eq!(
        h.ask(
            json!({ "id": 3, "type": "world", "world": "large", "yOffset": 64 }),
            &[&checkerboard()]
        )
        .header["ok"],
        true
    );
    assert_eq!(h.ask(json!({ "id": 4, "type": "shots", "world": "large", "shots": large_shots, "supersample": 2, "format": { "type": "jpeg", "quality": 90 } }), &[]).header["ok"], true);
    let after = held_scenes(&mut h, "small", &shots);
    eprintln!("the small world's calls' scene times: {fresh:?} on a fresh helper, {after:?} after the large world");
    assert!(
        after[1..].iter().all(|&s| s == 0.0),
        "after its first call the small world draws the scene it holds: {after:?}"
    );
}
