//! `test/live-views.test.js`, ported, with today's fake facts.

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use common::fake_facts::FakeFacts;
use mcbench_world::live_views::{LiveViews, Resolved};
use mcbench_world::{BlockState, World};
use serde_json::{Value, json};

fn build(blocks: &[([i32; 3], &str)]) -> Arc<World> {
    let mut world = World::new();
    for (at, id) in blocks {
        world.set(*at, BlockState::new(id, &[])).unwrap();
    }
    world.finalize();
    Arc::new(world)
}

/// A clock the test moves, so each look sees the build as it is.
fn views<R: FnMut(&str) -> Resolved>(resolve: R) -> (LiveViews<R>, Arc<AtomicU64>) {
    let clock = Arc::new(AtomicU64::new(0));
    let reading = clock.clone();
    (
        LiveViews::new(
            resolve,
            64,
            Box::new(move || reading.load(Ordering::SeqCst) as f64),
            Arc::new(FakeFacts),
        ),
        clock,
    )
}

fn look(clock: &AtomicU64) {
    clock.fetch_add(1000, Ordering::SeqCst);
}

#[test]
fn a_view_has_a_version_that_changes_only_when_its_blocks_do_and_its_descriptor_names_the_build() {
    let world = Rc::new(RefCell::new(build(&[([0, 0, 0], "minecraft:stone")])));
    let source = world.clone();
    let (mut live, clock) = views(move |_| Ok((source.borrow().clone(), Value::Null)));
    let first = live.describe("chat").unwrap();
    assert_eq!(
        first["bounds"],
        json!({ "min": [0, 0, 0], "max": [0, 0, 0] })
    );
    assert_eq!(first["blockCount"], 1);
    look(&clock);
    *world.borrow_mut() = build(&[([0, 0, 0], "minecraft:stone")]);
    assert_eq!(live.describe("chat").unwrap()["version"], first["version"]);
    look(&clock);
    *world.borrow_mut() = build(&[
        ([0, 0, 0], "minecraft:stone"),
        ([1, 0, 0], "minecraft:oak_planks"),
    ]);
    let second = live.describe("chat").unwrap();
    assert!(second["version"].as_u64() > first["version"].as_u64());
    assert_eq!(second["blockCount"], 2);
}

#[test]
fn the_page_gets_the_import_that_takes_the_world_it_holds_to_the_latest_or_a_fresh_one() {
    let world = Rc::new(RefCell::new(build(&[([0, 0, 0], "minecraft:stone")])));
    let source = world.clone();
    let (mut live, clock) = views(move |_| Ok((source.borrow().clone(), Value::Null)));
    let version = live.describe("chat").unwrap()["version"].as_u64().unwrap();
    let fresh = live.imports("chat", None).unwrap();
    assert_eq!(
        (
            fresh.version,
            fresh.fresh,
            fresh.commands.as_str(),
            fresh.block_count
        ),
        (version, true, "setblock 0 64 0 minecraft:stone\n", 1)
    );
    let held = live.imports("chat", Some(version)).unwrap();
    assert_eq!(
        (
            held.version,
            held.fresh,
            held.commands.as_str(),
            held.block_count
        ),
        (version, false, "", 1)
    );
    look(&clock);
    *world.borrow_mut() = build(&[
        ([0, 0, 0], "minecraft:stone"),
        ([1, 0, 0], "minecraft:oak_planks"),
    ]);
    let edit = live.imports("chat", Some(version)).unwrap();
    assert!(!edit.fresh);
    assert_eq!(edit.commands, "setblock 1 64 0 minecraft:oak_planks\n");
    assert!(edit.version > version);
    assert!(live.imports("chat", Some(9999)).unwrap().fresh);
    assert_eq!(
        live.imports("chat", Some(9999))
            .unwrap()
            .commands
            .split('\n')
            .count(),
        3
    );
}

#[test]
fn a_build_too_tall_to_place_keeps_the_version_before_it_and_says_why() {
    let world = Rc::new(RefCell::new(build(&[([0, 0, 0], "minecraft:stone")])));
    let source = world.clone();
    let (mut live, clock) = views(move |_| Ok((source.borrow().clone(), Value::Null)));
    let before = live.describe("chat").unwrap();
    look(&clock);
    *world.borrow_mut() = build(&[
        ([0, 0, 0], "minecraft:stone"),
        ([0, 2000, 0], "minecraft:stone"),
    ]);
    let after = live.describe("chat").unwrap();
    assert_eq!(after["version"], before["version"]);
    assert!(
        after["syncError"].as_str().unwrap().starts_with(
            "Minecraft kept the previous build: This build reaches Minecraft y=64..2064"
        )
    );
    look(&clock);
    *world.borrow_mut() = build(&[([0, 0, 0], "minecraft:stone")]);
    assert_eq!(live.describe("chat").unwrap()["syncError"], Value::Null);
    look(&clock);
    let tall = build(&[([0, 1500, 0], "minecraft:stone")]);
    let (mut fresh, _) = views(move |_| Ok((tall.clone(), Value::Null)));
    assert!(
        fresh.describe("other").unwrap_err().contains("y=1564"),
        "a view that never had a build has nothing to keep"
    );
}

#[test]
fn a_benchs_rounds_share_one_camera_counter_and_a_chat_has_its_own() {
    let (mut live, _) = views(|_| {
        Ok((
            build(&[([0, 0, 0], "minecraft:stone")]),
            json!([{ "id": "a" }]),
        ))
    });
    assert_eq!(live.describe("bench:b1:0").unwrap()["layout"][0]["id"], "a");
    assert_eq!(live.describe("bench:b1:2").unwrap()["frame"], 0);
    live.frame("bench:b1:1");
    live.frame("chat");
    assert_eq!(live.describe("bench:b1:2").unwrap()["frame"], 1);
    assert_eq!(live.describe("bench:b2:0").unwrap()["frame"], 0);
    assert_eq!(live.describe("chat").unwrap()["frame"], 1);
}

#[test]
fn views_not_looked_at_lately_are_let_go_and_a_missing_view_is_the_resolvers_error() {
    let calls = Cell::new(0);
    let (mut live, _) = views(|key| {
        calls.set(calls.get() + 1);
        if key == "gone" {
            return Err("Conversation not found".into());
        }
        Ok((build(&[([0, 0, 0], "minecraft:stone")]), Value::Null))
    });
    assert!(
        live.describe("gone")
            .unwrap_err()
            .contains("Conversation not found")
    );
    for i in 0..100 {
        live.describe(&format!("chat-{i}")).unwrap();
    }
    assert!(live.len() <= 64);
}
