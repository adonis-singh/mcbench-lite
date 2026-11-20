//! `test/minecraft-dimension.test.js`, ported for what is this crate's: the
//! world, the render mask, the data pack and the height check. Today's
//! tests run the fill and the edits through `mc-dsl.js`'s `runProgram`,
//! which stays JavaScript in the build child; here they are the world's own
//! calls that `runProgram` makes, and the messages are the ones the model
//! reads.

use mcbench_world::dimension::{
    MAX_MC_Y, MIN_MC_Y, WORLD_HEIGHT, validate_world_height, write_dimension_pack,
};
use mcbench_world::live_views::render_mask;
use mcbench_world::{BlockBox, BlockState, World};

#[test]
fn the_expanded_dimension_and_the_viewer_include_the_388_block_tall_monitor() {
    let mut world = World::new();
    world
        .fill(
            BlockBox {
                min: [0, 0, 0],
                max: [0, 387, 0],
            },
            BlockState::new("minecraft:stone", &[]),
        )
        .unwrap();
    assert_eq!(world.size(), 388);
    let mask = render_mask(world.bounds());
    assert!(mask.get("minY").is_none() && mask.get("maxY").is_none());
    let root = std::env::temp_dir().join(format!("mcbench-dimension-{}", std::process::id()));
    write_dimension_pack(&root).unwrap();
    let pack = root.join("world/datapacks/mcbench");
    let kind: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(pack.join("data/minecraft/dimension_type/overworld.json"))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(kind["height"], WORLD_HEIGHT);
    assert_eq!(kind["min_y"], MIN_MC_Y);
    assert_eq!(
        kind["min_y"].as_i64().unwrap() + kind["height"].as_i64().unwrap() - 1,
        MAX_MC_Y as i64
    );
    assert!(
        kind["min_y"].as_i64().unwrap() + kind["height"].as_i64().unwrap()
            > (world.bounds().unwrap().max[1] + 64) as i64
    );
    let meta: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(pack.join("pack.mcmeta")).unwrap()).unwrap();
    assert_eq!(meta["pack"]["min_format"], serde_json::json!([88, 0]));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn out_of_range_edits_fail_before_the_candidate_world_can_be_committed() {
    let mut original = World::new();
    original
        .set([0, 0, 0], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    original.finalize();
    let mut candidate = original.clone_world().unwrap();
    candidate
        .set([1, 1, 1], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    candidate
        .set([0, 896, 0], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    assert!(
        validate_world_height(&candidate, 64)
            .unwrap_err()
            .contains("Keep build y within -128..895")
    );
    assert_eq!(original.size(), 1);
    assert_eq!(
        original.bounds(),
        Some(BlockBox {
            min: [0, 0, 0],
            max: [0, 0, 0]
        })
    );
    let mut other = original.clone_world().unwrap();
    other
        .set([0, -129, 0], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    assert!(
        validate_world_height(&other, 64)
            .unwrap_err()
            .contains("y=-65..64")
    );
}
