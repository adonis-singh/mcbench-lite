//! `test/world.test.js`, ported.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use common::block_table::{Options, TestState, test_block_states};
use mcbench_world::{BlockBox, BlockState, Diff, MAX_BLOCKS, World, WorldManifest, key_of};

fn state(id: &str) -> BlockState {
    BlockState::new(id, &[])
}

fn area(min: [i32; 3], max: [i32; 3]) -> BlockBox {
    BlockBox { min, max }
}

#[test]
fn clone_copies_only_changed_chunks_and_leaves_the_base_immutable() {
    let mut base = World::new();
    base.fill(area([0, 0, 0], [31, 15, 15]), state("minecraft:stone"))
        .unwrap();
    base.finalize();
    let mut candidate = base.clone_world().unwrap();
    candidate
        .set([1, 1, 1], state("minecraft:oak_planks"))
        .unwrap();
    candidate.set([40, 1, 1], state("minecraft:stone")).unwrap();
    assert_eq!(base.get([1, 1, 1]).unwrap().id, "minecraft:stone");
    assert!(base.get([40, 1, 1]).is_none());
    assert_eq!(
        candidate.diff(&base).unwrap(),
        Diff {
            added: 1,
            removed: 0,
            changed: 1
        }
    );
    assert!(!Arc::ptr_eq(
        candidate.chunk(key_of(0, 0, 0)).unwrap(),
        base.chunk(key_of(0, 0, 0)).unwrap()
    ));
    assert!(Arc::ptr_eq(
        candidate.chunk(key_of(1, 0, 0)).unwrap(),
        base.chunk(key_of(1, 0, 0)).unwrap()
    ));
}

#[test]
fn chunk_manifest_round_trips_and_shares_content_addressed_payloads() {
    let mut world = World::new();
    world
        .fill(area([-20, 0, -20], [20, 3, 20]), state("minecraft:stone"))
        .unwrap();
    let mut files: HashMap<String, Vec<u8>> = HashMap::new();
    let manifest = world
        .persist(&mut |hash, bytes| {
            files.insert(hash.to_string(), bytes.to_vec());
            Ok(())
        })
        .unwrap();
    let restored = World::from_manifest(&manifest, &mut |hash| Ok(files[hash].clone())).unwrap();
    assert_eq!(restored.size(), world.size());
    assert_eq!(restored.bounds(), world.bounds());
    assert_eq!(restored.get([-20, 0, -20]).unwrap().id, "minecraft:stone");
    assert!(restored.get([21, 0, 0]).is_none());
}

#[test]
fn three_million_blocks_fit_exactly_and_the_next_block_fails_fast() {
    let mut world = World::new();
    world
        .fill(area([0, 0, 0], [299, 199, 49]), state("minecraft:stone"))
        .unwrap();
    assert_eq!(world.size(), MAX_BLOCKS);
    let error = world
        .set([300, 0, 0], state("minecraft:stone"))
        .unwrap_err();
    assert!(error.contains("3000000 block limit"), "{error}");
}

#[test]
fn a_positive_coordinate_1920x1080_canvas_fits_inside_world_bounds() {
    // Concrete as a cube with six full faces, written for the test.
    let states = test_block_states(
        &[TestState::cube("minecraft:black_concrete")],
        &Options::default(),
    );
    let mut base = World::new();
    base.finalize();
    let mut world = base.clone_world().unwrap();
    world
        .fill(
            area([0, 0, 0], [1919, 1079, 0]),
            state("minecraft:black_concrete"),
        )
        .unwrap();
    assert_eq!(world.size(), 1920 * 1080);
    assert_eq!(world.bounds(), Some(area([0, 0, 0], [1919, 1079, 0])));
    assert_eq!(world.render_lod(), Some((2, 4)));
    let palette = states.of_palette(world.palette()).unwrap();
    let mut blocks = 0;
    for (key, _) in world.chunks() {
        blocks += world
            .chunk_snapshot(key, &palette, world.render_lod())
            .unwrap()
            .blocks
            .len();
    }
    assert_eq!(blocks, (1920 / 4) * (1080 / 4));
}

#[test]
fn a_world_with_writes_since_its_last_finalize_cannot_be_cloned_since_it_would_write_its_clones_chunks()
 {
    let mut base = World::new();
    base.set([0, 0, 0], state("minecraft:stone")).unwrap();
    assert!(
        base.clone_world()
            .unwrap_err()
            .contains("cannot be cloned: finalize it first")
    );
    base.finalize();
    let clone = base.clone_world().unwrap();
    base.set([1, 0, 0], state("minecraft:stone")).unwrap();
    assert!(clone.get([1, 0, 0]).is_none());
}

#[test]
fn a_world_keeps_the_ticks_it_was_settled_with_through_its_manifest_and_its_clones() {
    let mut world = World::new();
    world.set([0, 0, 0], state("minecraft:stone")).unwrap();
    world.finalize();
    world.settle_ticks = Some(40);
    let mut chunks: HashMap<String, Vec<u8>> = HashMap::new();
    let manifest = world
        .persist(&mut |hash, bytes| {
            chunks.insert(hash.to_string(), bytes.to_vec());
            Ok(())
        })
        .unwrap();
    let loaded = World::from_manifest(&WorldManifest(manifest.0.clone()), &mut |hash| {
        Ok(chunks[hash].clone())
    })
    .unwrap();
    assert_eq!(loaded.settle_ticks, Some(40));
    assert_eq!(loaded.clone_world().unwrap().settle_ticks, Some(40));
    assert_eq!(loaded.reference().settle_ticks, Some(40));
}

/// What a refused write must leave as it was.
type Contents = (
    usize,
    Option<BlockBox>,
    Vec<(u32, usize, u64)>,
    Vec<u32>,
    Vec<u32>,
    usize,
);

fn contents(world: &World) -> Contents {
    (
        world.size(),
        world.bounds(),
        world
            .chunks()
            .map(|(key, chunk)| (key, chunk.count(), chunk.version()))
            .collect(),
        world.changed_chunks().collect(),
        world.owned_chunks().collect(),
        world.palette().len(),
    )
}

#[test]
fn a_fill_the_block_limit_refuses_leaves_the_world_as_it_was_in_a_chunk_that_did_not_exist_too() {
    let mut world = World::new();
    world
        .fill(area([0, 0, 0], [299, 199, 49]), state("minecraft:stone"))
        .unwrap();
    world.finalize();
    let full = contents(&world);
    // The box starts in a chunk the world does not have.
    assert!(
        world
            .fill(
                area([320, 0, 0], [321, 0, 0]),
                state("minecraft:oak_planks")
            )
            .unwrap_err()
            .contains("3000000 block limit")
    );
    assert_eq!(contents(&world), full);
    assert!(world.chunk(key_of(20, 0, 0)).is_none());
    assert!(
        world
            .set([320, 0, 0], state("minecraft:oak_planks"))
            .unwrap_err()
            .contains("3000000 block limit")
    );
    assert_eq!(contents(&world), full);
    // Blocks that are there already are not added: the same box fills again.
    world
        .fill(
            area([0, 0, 0], [299, 199, 49]),
            state("minecraft:oak_planks"),
        )
        .unwrap();
    assert_eq!(world.size(), MAX_BLOCKS);

    // A box that reaches the limit part-way through writes none of its blocks.
    let mut partial = World::new();
    partial
        .fill(area([0, 0, 0], [299, 199, 49]), state("minecraft:stone"))
        .unwrap();
    partial
        .fill(area([0, 0, 0], [9, 0, 0]), state("minecraft:air"))
        .unwrap();
    partial.finalize();
    let before = contents(&partial);
    assert!(
        partial
            .fill(area([0, 0, 0], [399, 0, 0]), state("minecraft:oak_planks"))
            .unwrap_err()
            .contains("3000000 block limit")
    );
    assert_eq!(contents(&partial), before);
    assert!(partial.get([300, 0, 0]).is_none());
    // Ten blocks of room take ten blocks.
    partial
        .fill(area([0, 0, 0], [20, 0, 0]), state("minecraft:oak_planks"))
        .unwrap();
    assert_eq!(partial.size(), MAX_BLOCKS);
    assert_eq!(partial.get([5, 0, 0]).unwrap().id, "minecraft:oak_planks");
}

#[test]
fn the_block_limit_error_says_how_many_blocks_the_world_has_and_the_call_would_add() {
    let mut world = World::new();
    world
        .fill(area([0, 0, 0], [299, 199, 49]), state("minecraft:stone"))
        .unwrap();
    world
        .fill(area([0, 0, 0], [3, 0, 0]), state("minecraft:air"))
        .unwrap();
    assert_eq!(
        world
            .fill(
                area([296, 0, 0], [305, 0, 0]),
                state("minecraft:oak_planks")
            )
            .unwrap_err(),
        "Build exceeds the 3000000 block limit: the world has 2999996 blocks and this call would add 6"
    );
    world
        .fill(area([0, 0, 0], [3, 0, 0]), state("minecraft:stone"))
        .unwrap();
    assert_eq!(
        world.set([0, 0, 50], state("minecraft:stone")).unwrap_err(),
        "Build exceeds the 3000000 block limit: the world has 3000000 blocks and this call would add 1"
    );
}
