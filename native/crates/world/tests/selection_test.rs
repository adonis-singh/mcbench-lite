//! `test/selection.test.js`, ported.

use indexmap::IndexMap;
use mcbench_world::selection::{SelectionDescription, describe_selection, read_selection};
use mcbench_world::{BlockBox, BlockState, World};
use serde_json::{Value, json};

/// Minecraft's names of the blocks below, as the block table gives them.
fn names() -> IndexMap<String, String> {
    [
        ("minecraft:stone_bricks", "Stone Bricks"),
        ("minecraft:dark_oak_stairs", "Dark Oak Stairs"),
        ("minecraft:glass", "Glass"),
        ("minecraft:stone", "Stone"),
        ("minecraft:dirt", "Dirt"),
        ("minecraft:oak_planks", "Oak Planks"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

fn describe(world: &World, min: Value, max: Value) -> Result<SelectionDescription, String> {
    describe_selection(world, &min, &max, &names()).map_err(|e| e.0)
}

fn described(blocks: u64, block: Option<&str>, label: &str) -> SelectionDescription {
    SelectionDescription {
        blocks,
        block: block.map(str::to_string),
        label: label.into(),
    }
}

fn house() -> World {
    let mut world = World::new();
    world
        .fill(
            BlockBox {
                min: [0, 0, 0],
                max: [9, 5, 9],
            },
            BlockState::new("minecraft:stone_bricks", &[]),
        )
        .unwrap();
    world
        .fill(
            BlockBox {
                min: [0, 6, 0],
                max: [9, 8, 9],
            },
            BlockState::new("minecraft:dark_oak_stairs", &[("facing", "north")]),
        )
        .unwrap();
    world
}

#[test]
fn a_selection_is_named_by_minecrafts_name_of_its_most_common_block_and_where_it_sits() {
    let world = house();
    assert_eq!(
        describe(&world, json!([0, 6, 0]), json!([9, 8, 9])).unwrap(),
        described(
            300,
            Some("minecraft:dark_oak_stairs"),
            "Dark Oak Stairs, top"
        )
    );
    assert_eq!(
        describe(&world, json!([0, 0, 0]), json!([2, 1, 2])).unwrap(),
        described(
            18,
            Some("minecraft:stone_bricks"),
            "Stone Bricks, base, north-west side"
        )
    );
    assert_eq!(
        describe(&world, json!([4, 3, 4]), json!([5, 3, 5])).unwrap(),
        described(4, Some("minecraft:stone_bricks"), "Stone Bricks, middle")
    );
    assert_eq!(
        describe(&world, json!([20, 0, 20]), json!([21, 1, 21])).unwrap(),
        described(0, None, "Empty space")
    );
}

#[test]
fn malformed_selections_are_refused() {
    assert!(
        describe(&house(), json!([0, 0]), json!([1, 1, 1]))
            .unwrap_err()
            .contains("integer corners")
    );
    assert!(
        describe(&house(), json!([3, 0, 0]), json!([1, 1, 1]))
            .unwrap_err()
            .contains("min at most max")
    );
    assert!(read_selection(&json!({ "letter": "AA", "label": "x", "min": [0, 0, 0], "max": [0, 0, 0], "blocks": 1, "block": null })).unwrap_err().0.contains("letter"));
    let read = read_selection(&json!({ "letter": "B", "label": "Glass, top", "min": [0, 0, 0], "max": [1, 1, 1], "blocks": 8, "block": "minecraft:glass" })).unwrap();
    assert_eq!(
        serde_json::to_value(read).unwrap(),
        json!({ "letter": "B", "label": "Glass, top", "min": [0, 0, 0], "max": [1, 1, 1], "blocks": 8, "block": "minecraft:glass" })
    );
    assert!(read_selection(&json!({ "letter": "B", "label": "x", "min": [0, 0, 0], "max": [0, 0, 0], "blocks": 1, "block": "../x" })).unwrap_err().0.contains("main block"));
}

#[test]
fn a_selection_costs_what_the_build_holds_not_the_size_of_its_box() {
    let world = house();
    let started = std::time::Instant::now();
    let whole = describe(
        &world,
        json!([-4096, -4096, -4096]),
        json!([4096, 4096, 4096]),
    )
    .unwrap();
    assert!(
        // Scanning the box would take hours; a bound a starved machine
        // meets tells the two apart.
        started.elapsed().as_secs() < 10,
        "the whole world took {:?}",
        started.elapsed()
    );
    assert_eq!(
        (whole.blocks, whole.block.as_deref()),
        (900, Some("minecraft:stone_bricks"))
    );
    for (min, max) in [
        (json!([0, 0, 0]), json!([4097, 0, 0])),
        (json!([-(2i64.pow(31)), 0, 0]), json!([2i64.pow(31), 0, 0])),
    ] {
        assert!(
            describe(&world, min, max)
                .unwrap_err()
                .contains("within +/-4096")
        );
    }
}

#[test]
fn a_tie_is_named_by_the_block_in_the_lowest_chunk() {
    let mut world = World::new();
    world
        .set([16, 0, 0], BlockState::new("minecraft:glass", &[]))
        .unwrap();
    world
        .set([0, 16, 0], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    world
        .set([0, 0, 16], BlockState::new("minecraft:dirt", &[]))
        .unwrap();
    assert_eq!(
        describe(&world, json!([0, 0, 0]), json!([16, 16, 16]))
            .unwrap()
            .block
            .as_deref(),
        Some("minecraft:dirt")
    );
    world
        .set([0, 0, 0], BlockState::new("minecraft:oak_planks", &[]))
        .unwrap();
    assert_eq!(
        describe(&world, json!([0, 0, 0]), json!([16, 16, 16]))
            .unwrap()
            .block
            .as_deref(),
        Some("minecraft:oak_planks")
    );
}
