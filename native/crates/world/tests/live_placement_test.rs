//! `test/live-placement.test.js`, ported, with today's fake facts.

mod common;

use std::collections::BTreeMap;

use common::fake_facts::FakeFacts;
use mcbench_world::dimension::validate_world_height;
use mcbench_world::live_placement::{ChunkPlan, block_spec, changed_keys, compile_chunk};
use mcbench_world::{BlockBox, BlockState, World, key_of};

fn last(world: &World) -> Vec<u8> {
    use mcbench_world::facts::LastPass;
    FakeFacts.last_pass(world.palette()).unwrap()
}

fn plan(world: &World, key: u32, previous: Option<&World>) -> ChunkPlan {
    compile_chunk(world, key, 64, previous, &last(world)).unwrap()
}

fn s(id: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState::new(id, properties)
}

fn fill(world: &mut World, min: [i32; 3], max: [i32; 3], state: BlockState) {
    world.fill(BlockBox { min, max }, state).unwrap();
}

/// Each command's cells: position at browser height to its state.
fn cells(commands: &[String], mut each: impl FnMut(String, String)) {
    for command in commands {
        let tokens: Vec<&str> = command.split(' ').collect();
        let from: Vec<i32> = tokens[1..4].iter().map(|t| t.parse().unwrap()).collect();
        let to: Vec<i32> = if tokens[0] == "fill" {
            tokens[4..7].iter().map(|t| t.parse().unwrap()).collect()
        } else {
            from.clone()
        };
        let spec = tokens.last().unwrap().to_string();
        for x in from[0]..=to[0] {
            for y in from[1]..=to[1] {
                for z in from[2]..=to[2] {
                    each(format!("{x},{},{z}", y - 64), spec.clone());
                }
            }
        }
    }
}

fn expected(world: &World) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    world.for_each(None, |p, e| {
        out.insert(
            format!("{},{},{}", p[0], p[1], p[2]),
            block_spec(e).unwrap(),
        );
    });
    out
}

#[test]
fn solid_chunks_compile_to_one_direct_fill_without_a_datapack_reload() {
    let mut world = World::new();
    fill(
        &mut world,
        [0, 0, 0],
        [15, 15, 15],
        s("minecraft:stone", &[]),
    );
    let plan = plan(&world, key_of(0, 0, 0), None);
    assert_eq!(plan.alone, vec!["fill 0 64 0 15 79 15 minecraft:stone"]);
    assert!(plan.supported.is_empty());
}

#[test]
fn compiled_cuboids_reproduce_mixed_blocks_and_states_exactly_in_a_negative_chunk() {
    let mut world = World::new();
    fill(
        &mut world,
        [-16, -16, -16],
        [-1, -1, -1],
        s("minecraft:stone", &[]),
    );
    fill(
        &mut world,
        [-13, -12, -11],
        [-4, -3, -2],
        s("minecraft:air", &[]),
    );
    fill(
        &mut world,
        [-12, -12, -12],
        [-6, -12, -12],
        s(
            "minecraft:oak_stairs",
            &[("facing", "west"), ("half", "top")],
        ),
    );
    world
        .set([-8, -11, -12], s("minecraft:torch", &[]))
        .unwrap();
    world
        .set(
            [-16, -15, -16],
            s(
                "minecraft:oak_stairs",
                &[("half", "bottom"), ("facing", "south")],
            ),
        )
        .unwrap();
    let plan = plan(&world, key_of(-1, -1, -1), None);
    let mut placed = BTreeMap::new();
    let commands: Vec<String> = plan.alone.iter().chain(&plan.supported).cloned().collect();
    cells(&commands, |position, spec| {
        assert!(
            placed.insert(position.clone(), spec).is_none(),
            "duplicate placement: {position}"
        );
    });
    assert_eq!(placed, expected(&world));
    assert_eq!(plan.supported, vec!["setblock -8 53 -12 minecraft:torch"]);
}

#[test]
fn height_validation_explains_builds_that_cannot_fit_in_a_real_world() {
    let mut world = World::new();
    world.set([0, 896, 0], s("minecraft:stone", &[])).unwrap();
    assert!(
        validate_world_height(&world, 64)
            .unwrap_err()
            .contains("y=960..960")
    );
}

#[test]
fn one_block_edit_emits_one_placement_without_clearing_or_rebuilding_its_neighbors() {
    let mut before = World::new();
    for x in 0..16 {
        for y in 0..16 {
            for z in 0..16 {
                before
                    .set(
                        [x, y, z],
                        s(
                            if (x + y + z) % 2 == 1 {
                                "minecraft:stone"
                            } else {
                                "minecraft:oak_planks"
                            },
                            &[],
                        ),
                    )
                    .unwrap();
            }
        }
    }
    before.finalize();
    let mut next = before.clone_world().unwrap();
    next.set([8, 8, 8], s("minecraft:gold_block", &[])).unwrap();
    next.finalize();
    let origin = key_of(0, 0, 0);
    assert_eq!(
        plan(&next, origin, Some(&before)),
        ChunkPlan {
            key: origin,
            removed: vec![],
            alone: vec!["setblock 8 72 8 minecraft:gold_block".into()],
            supported: vec![]
        }
    );
}

#[test]
fn incremental_cuboids_reconstruct_additions_removals_and_states_across_independent_palettes() {
    for seed in 1..=12 {
        let mut before = World::new();
        let mut after = World::new();
        before.set([-16, 0, 0], s("minecraft:stone", &[])).unwrap();
        after
            .set([-16, 0, 0], s("minecraft:oak_planks", &[]))
            .unwrap();
        for x in -16..16 {
            for z in 0..16 {
                let old = (x * x + z + seed) % 5;
                let next = (x * x + z * 3 + seed) % 7;
                if old != 0 {
                    before
                        .set(
                            [x, 0, z],
                            if old % 2 == 1 {
                                s("minecraft:stone", &[])
                            } else {
                                s("minecraft:oak_stairs", &[("facing", "west")])
                            },
                        )
                        .unwrap();
                }
                if next != 0 {
                    after
                        .set(
                            [x, 0, z],
                            if next % 2 == 1 {
                                s("minecraft:stone", &[])
                            } else {
                                s("minecraft:oak_stairs", &[("facing", "east")])
                            },
                        )
                        .unwrap();
                }
            }
        }
        let mut placed = expected(&before);
        for key in changed_keys(Some(&before), &after).unwrap() {
            let plan = plan(&after, key, Some(&before));
            let commands: Vec<String> = plan
                .removed
                .iter()
                .chain(&plan.alone)
                .chain(&plan.supported)
                .cloned()
                .collect();
            cells(&commands, |position, spec| {
                if spec == "minecraft:air" {
                    placed.remove(&position);
                } else {
                    placed.insert(position, spec);
                }
            });
        }
        assert_eq!(placed, expected(&after));
        let empty = World::new();
        for (key, _) in after.chunks() {
            let plan = compile_chunk(&empty, key, 64, Some(&after), &last(&empty)).unwrap();
            assert!(!plan.removed.is_empty());
            assert_eq!(plan.alone.len() + plan.supported.len(), 0);
        }
    }
}

#[test]
fn fresh_sibling_branches_skip_chunks_with_equal_persisted_content_and_palettes() {
    let origin = key_of(0, 0, 0);
    let world = |blocks: &[([i32; 3], &str)], hash: &str| {
        let mut world = World::new();
        for (at, id) in blocks {
            world.set(*at, s(id, &[])).unwrap();
        }
        world.finalize();
        world
            .chunk(origin)
            .unwrap()
            .set_persisted(Some(hash.into()));
        world
    };
    let first = world(&[([0, 0, 0], "minecraft:stone")], "equal-chunk");
    let sibling = world(&[([0, 0, 0], "minecraft:stone")], "equal-chunk");
    assert!(changed_keys(Some(&first), &sibling).unwrap().is_empty());
    let collision = world(&[([0, 0, 0], "minecraft:diamond_block")], "equal-chunk");
    assert_eq!(
        changed_keys(Some(&first), &collision).unwrap(),
        vec![origin]
    );
    let reordered = world(
        &[
            ([1, 0, 0], "minecraft:stone"),
            ([0, 0, 0], "minecraft:oak_planks"),
            ([1, 0, 0], "minecraft:air"),
        ],
        "left-order",
    );
    let other = world(&[([0, 0, 0], "minecraft:oak_planks")], "right-order");
    assert!(changed_keys(Some(&reordered), &other).unwrap().is_empty());
}
