//! `see_build`'s pictures through the built binary's headless families
//! (`PROTOCOL.md`, families `shots` and `turn`): `headless shots` serves a
//! request for a world given as files, and a `see_build` call through
//! `headless turn` answers its result with the pictures saved in the
//! chat's images. Each process starts its own renderer
//! (`mcbench_draw::live::Draw` on the hardware adapter) from the shipped
//! data at `MCBENCH_TABLES` and the textures at `MCBENCH_TEXTURES`.
//!
//! The two drawing tests need a graphics adapter (a GPU, or the software
//! floor the drawing crate is given) and the renderer's files: they are
//! ignored without `MCBENCH_DRAW_DATA` (a shipped data folder,
//! `minecraft/<id>`) and `MCBENCH_DRAW_TEXTURES` (`textures/<id>`), and
//! fail when those are set and something is missing. The turn's build also
//! needs the build child `MCBENCH_BUILD_CHILD` names. A process without
//! the textures named refuses at once, which needs nothing.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

fn folder(name: &str) -> PathBuf {
    let folder = PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("set {name}")));
    assert!(
        folder.is_dir(),
        "{name} names {}, not a folder",
        folder.display()
    );
    folder
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("mcbench-headless-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `headless <family>` with the renderer's files, its stdin and stdout piped.
fn family(family: &str) -> Child {
    Command::new(env!("CARGO_BIN_EXE_mcbench-lite"))
        .args(["headless", family])
        .env("MCBENCH_TABLES", folder("MCBENCH_DRAW_DATA"))
        .env("MCBENCH_TEXTURES", folder("MCBENCH_DRAW_TEXTURES"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap()
}

/// Sends each request and reads one reply line for each (and a `call`'s
/// event lines before it), then closes stdin and waits for status 0.
fn serve(mut child: Child, requests: &[Value]) -> Vec<Value> {
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut replies = Vec::new();
    for request in requests {
        writeln!(stdin, "{}", mcbench_json::stringify(request)).unwrap();
        stdin.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                out.read_line(&mut line).unwrap() > 0,
                "a reply to {request}"
            );
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply.get("event").is_none() {
                replies.push(reply);
                break;
            }
        }
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    replies
}

fn is_jpeg(file: &std::path::Path) -> bool {
    std::fs::read(file)
        .unwrap()
        .starts_with(&[0xff, 0xd8, 0xff])
}

#[test]
fn shots_without_the_textures_named_refuses_at_once() {
    let output = Command::new(env!("CARGO_BIN_EXE_mcbench-lite"))
        .args(["headless", "shots"])
        .env("MCBENCH_TABLES", std::env::temp_dir())
        .env_remove("MCBENCH_TEXTURES")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "mcbench-lite headless shots: MCBENCH_TEXTURES names no folder: see_build draws with the baked textures (textures/<id> of a data folder)\n"
    );
}

#[test]
#[ignore = "needs a graphics adapter, MCBENCH_DRAW_DATA and MCBENCH_DRAW_TEXTURES (the renderer's files) and MCBENCH_BUILD_CHILD"]
fn shots_draws_a_world_given_as_files_needs_a_graphics_adapter() {
    let dir = scratch("shots");
    let mut world = mcbench_world::World::new();
    for x in 0..6 {
        for z in 0..4 {
            world.set_block([x, 0, z], "minecraft:stone", &[]).unwrap();
        }
    }
    world.set_block([2, 1, 1], "minecraft:glass", &[]).unwrap();
    world.finalize();
    let chunks = dir.join("chunks");
    std::fs::create_dir_all(&chunks).unwrap();
    let manifest = world
        .persist(&mut |hash: &str, bytes: &[u8]| {
            std::fs::write(chunks.join(format!("{hash}.json")), bytes).map_err(|e| e.to_string())
        })
        .unwrap();
    std::fs::write(dir.join("manifest.json"), &manifest.0).unwrap();
    let out = dir.join("out");
    let replies = serve(
        family("shots"),
        &[
            json!({"id": 1, "world": {"manifest": dir.join("manifest.json"), "chunks": chunks}, "arguments": {}, "out": out}),
            json!({"id": 2, "world": {"manifest": dir.join("manifest.json"), "chunks": chunks}, "arguments": {"shots": []}, "out": out}),
        ],
    );
    let result = &replies[0]["result"];
    assert_eq!(replies[0]["ok"], true, "{}", replies[0]);
    assert_eq!(result["ok"], true, "{result}");
    let shots = result["shots"].as_array().unwrap();
    assert_eq!(shots.len(), 2, "the two default views");
    for (index, shot) in shots.iter().enumerate() {
        let file = out.join(format!("{}.jpg", index + 1));
        assert_eq!(shot["image"], file.display().to_string());
        assert!(is_jpeg(&file));
    }
    assert_eq!(result["minecraft"][0]["ticks"], 210);
    assert_eq!(
        replies[1]["result"],
        json!({"ok": false, "error": "shots must be a list of 1 to 6 shots"})
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[ignore = "needs a graphics adapter, MCBENCH_DRAW_DATA and MCBENCH_DRAW_TEXTURES (the renderer's files) and MCBENCH_BUILD_CHILD"]
fn a_see_build_call_through_turn_returns_pictures_needs_a_graphics_adapter() {
    let data = scratch("turn");
    std::fs::create_dir_all(data.join("assets/1.21.10")).unwrap();
    std::fs::copy(
        folder("MCBENCH_DRAW_DATA").join("blocks.json"),
        data.join("assets/1.21.10/blocks.json"),
    )
    .unwrap();
    let created = serve(
        family("turn"),
        &[
            json!({"id": 1, "op": "create", "data": data, "provider": "openrouter", "model": "chat/model"}),
        ],
    );
    let chat = created[0]["chat"].as_str().unwrap().to_string();
    let call = |id: u32, name: &str, arguments: Value| json!({"id": id, "op": "call", "data": data, "chat": chat, "name": name, "arguments": arguments, "imageInput": true, "turnChat": false});
    let replies = serve(
        family("turn"),
        &[
            call(
                2,
                "build",
                json!({"program": "module.exports = mc => { mc.fill([0, 0, 0], [5, 0, 3], 'stone'); mc.set([2, 1, 1], 'glass'); }"}),
            ),
            call(3, "see_build", json!({})),
        ],
    );
    assert_eq!(replies[0]["result"]["ok"], true, "{}", replies[0]);
    let seen = &replies[1];
    assert_eq!(seen["result"]["ok"], true, "{seen}");
    assert_eq!(
        seen["screenshots"]["text"],
        "[see_build] Screenshots of the current build:"
    );
    let images = seen["screenshots"]["images"].as_array().unwrap();
    assert_eq!(images.len(), 2);
    for url in images {
        let parts: Vec<&str> = url.as_str().unwrap().split('/').collect();
        assert!(is_jpeg(&data.join("images").join(parts[3]).join(parts[4])));
    }
    std::fs::remove_dir_all(&data).unwrap();
}

/// A turn that never calls `see_build` needs no baked textures: without
/// `MCBENCH_TEXTURES` a build call works as before `see_build` was drawn,
/// and a `see_build` call answers why it cannot draw.
#[test]
#[ignore = "needs MCBENCH_DRAW_DATA (the shipped data's folder) and MCBENCH_BUILD_CHILD"]
fn a_turn_without_the_textures_named_builds_and_see_build_says_why() {
    let data = scratch("untextured");
    std::fs::create_dir_all(data.join("assets/1.21.10")).unwrap();
    std::fs::copy(
        folder("MCBENCH_DRAW_DATA").join("blocks.json"),
        data.join("assets/1.21.10/blocks.json"),
    )
    .unwrap();
    let untextured = || {
        Command::new(env!("CARGO_BIN_EXE_mcbench-lite"))
            .args(["headless", "turn"])
            .env("MCBENCH_TABLES", folder("MCBENCH_DRAW_DATA"))
            .env_remove("MCBENCH_TEXTURES")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let created = serve(
        untextured(),
        &[
            json!({"id": 1, "op": "create", "data": data, "provider": "openrouter", "model": "chat/model"}),
        ],
    );
    let chat = created[0]["chat"].as_str().unwrap().to_string();
    let call = |id: u32, name: &str, arguments: Value| json!({"id": id, "op": "call", "data": data, "chat": chat, "name": name, "arguments": arguments, "imageInput": true, "turnChat": false});
    let replies = serve(
        untextured(),
        &[
            call(
                2,
                "build",
                json!({"program": "module.exports = mc => { mc.fill([0, 0, 0], [5, 0, 3], 'stone'); }"}),
            ),
            call(3, "see_build", json!({})),
        ],
    );
    assert_eq!(replies[0]["result"]["ok"], true, "{}", replies[0]);
    assert_eq!(
        replies[1]["result"],
        json!({"ok": false, "error": "Screenshot initialization: MCBENCH_TEXTURES names no folder: see_build draws with the baked textures (textures/<id> of a data folder)"}),
        "{}",
        replies[1]
    );
    std::fs::remove_dir_all(&data).unwrap();
}
