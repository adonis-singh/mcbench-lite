//! `test/bench-world.test.js`, ported.

use std::collections::HashMap;
use std::sync::Arc;

use mcbench_world::bench::{BenchComposer, RoundMessage, world_at_round};
use mcbench_world::{BlockState, World};

fn build(blocks: &[([i32; 3], &str)]) -> World {
    let mut world = World::new();
    for (at, id) in blocks {
        world.set(*at, BlockState::new(id, &[])).unwrap();
    }
    world.finalize();
    world
}

#[test]
fn builds_sit_side_by_side_each_keeping_its_blocks_and_their_heights() {
    let a = build(&[
        ([100, 5, 40], "minecraft:stone"),
        ([110, 9, 44], "minecraft:oak_planks"),
    ]);
    let b = build(&[([-300, 2, -300], "minecraft:glass")]);
    let (world, layout) = BenchComposer::new()
        .compose(&[("a".into(), &a), ("b".into(), &b)])
        .unwrap();
    assert_eq!(world.size(), 3);
    let (left, right) = (&layout[0], &layout[1]);
    assert!(left.max[0] < right.min[0], "the builds do not overlap");
    assert!(right.min[0] - left.max[0] > 16, "a gap separates them");
    assert_eq!([left.min[1], left.max[1], right.min[1]], [5, 9, 2]);
    assert_eq!(
        world.get([left.min[0], 5, left.min[2]]).unwrap().id,
        "minecraft:stone"
    );
    assert_eq!(
        world.get([left.max[0], 9, left.max[2]]).unwrap().id,
        "minecraft:oak_planks"
    );
    assert_eq!(
        world.get([right.min[0], 2, right.min[2]]).unwrap().id,
        "minecraft:glass"
    );
}

#[test]
fn an_unchanged_build_keeps_its_chunk_objects_across_updates() {
    let mut composer = BenchComposer::new();
    let a = build(&[([0, 0, 0], "minecraft:stone")]);
    let b = build(&[([0, 0, 0], "minecraft:dirt")]);
    let (before, _) = composer
        .compose(&[("a".into(), &a), ("b".into(), &b)])
        .unwrap();
    let mut b = b.clone_world().unwrap();
    b.set([1, 0, 0], BlockState::new("minecraft:dirt", &[]))
        .unwrap();
    b.finalize();
    let (after, _) = composer
        .compose(&[("a".into(), &a), ("b".into(), &b)])
        .unwrap();
    let key_of = |world: &World, id: &str| {
        world.chunks().map(|(key, _)| key).find(|&key| {
            let mut found = false;
            world.for_each_in_chunk(key, |_, entry| found |= entry.id == id);
            found
        })
    };
    let stone = key_of(&before, "minecraft:stone").expect("a stone chunk");
    assert!(Arc::ptr_eq(
        after.chunk(stone).unwrap(),
        before.chunk(stone).unwrap()
    ));
    let dirt_after = after
        .chunk(key_of(&after, "minecraft:dirt").unwrap())
        .unwrap();
    let dirt_before = before
        .chunk(key_of(&before, "minecraft:dirt").unwrap())
        .unwrap();
    assert!(!Arc::ptr_eq(dirt_after, dirt_before));
    assert!(after.revision() > before.revision());
}

#[test]
fn a_round_shows_each_model_as_it_was_when_that_round_ended() {
    let first = build(&[([0, 0, 0], "minecraft:stone")]);
    let first_uid = first.uid();
    let messages = ["u1:user", "a1:assistant", "u2:user"]
        .map(|m| {
            let (id, role) = m.split_once(':').unwrap();
            RoundMessage {
                id: id.into(),
                role: role.into(),
                see_build: false,
            }
        })
        .to_vec();
    let checkpoints = HashMap::from([("u2".to_string(), first)]);
    let world = build(&[
        ([0, 0, 0], "minecraft:stone"),
        ([0, 1, 0], "minecraft:stone"),
    ]);
    assert_eq!(
        world_at_round(&messages, &checkpoints, &world, 1)
            .unwrap()
            .uid(),
        first_uid
    );
    assert_eq!(
        world_at_round(&messages, &checkpoints, &world, 2)
            .unwrap()
            .uid(),
        world.uid()
    );
}
