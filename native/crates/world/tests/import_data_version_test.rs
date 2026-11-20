//! `test/import-data-version.test.js`, the half that is this crate's: the
//! live import orders a world by the palette the engine places
//! (`importCommands(world, null, 64, placed)`) and names its blocks as
//! written. The data fixer that makes `placed` (`engine.js` `fixedPalette`)
//! and the verifier are `mcbench-engine`'s; here `placed` holds the fixer's
//! answers the same test asserts (the data fixer's own cases). Needs
//! Minecraft's tables: `MCBENCH_TABLES=<core/minecraft/1.21.10>`.

mod common;

use mcbench_world::facts::MinecraftFacts;
use mcbench_world::live_placement::import_commands;
use mcbench_world::{BlockState, Palette, World};

type Case = (
    u32,
    Vec<(
        &'static str,
        Vec<(&'static str, &'static str)>,
        &'static str,
    )>,
);

fn cases() -> Vec<Case> {
    vec![
        (
            2566,
            vec![
                ("minecraft:grass_path", vec![], "minecraft:dirt_path"),
                ("minecraft:grass", vec![], "minecraft:short_grass"),
                (
                    "minecraft:chain",
                    vec![("axis", "x")],
                    "minecraft:iron_chain",
                ),
            ],
        ),
        (
            3218,
            vec![
                ("minecraft:grass", vec![], "minecraft:short_grass"),
                (
                    "minecraft:chain",
                    vec![("axis", "x")],
                    "minecraft:iron_chain",
                ),
            ],
        ),
        (
            3465,
            vec![
                ("minecraft:grass", vec![], "minecraft:short_grass"),
                (
                    "minecraft:chain",
                    vec![("axis", "y"), ("waterlogged", "false")],
                    "minecraft:iron_chain",
                ),
            ],
        ),
        (
            4189,
            vec![
                ("minecraft:short_grass", vec![], "minecraft:short_grass"),
                (
                    "minecraft:chain",
                    vec![("axis", "z")],
                    "minecraft:iron_chain",
                ),
            ],
        ),
    ]
}

fn world_of(blocks: &[(&str, Vec<(&str, &str)>)]) -> World {
    let mut world = World::new();
    for (i, (id, states)) in blocks.iter().enumerate() {
        world
            .set([3 * i as i32, 0, 0], BlockState::new("minecraft:dirt", &[]))
            .unwrap();
        world
            .set([3 * i as i32, 1, 0], BlockState::new(id, states))
            .unwrap();
    }
    world.finalize();
    world
}

fn facts() -> Option<MinecraftFacts> {
    let file = common::tables()?;
    Some(MinecraftFacts::new(&std::fs::read_to_string(file).unwrap()).unwrap())
}

#[test]
#[ignore = "needs MCBENCH_TABLES (Minecraft's tables)"]
fn a_world_written_at_an_older_version_is_ordered_by_its_blocks_as_the_data_fixer_brings_them_forward_and_keeps_them_as_written()
 {
    let facts = facts().expect("set MCBENCH_TABLES");
    for (version, blocks) in cases() {
        let world = world_of(
            &blocks
                .iter()
                .map(|(id, s, _)| (*id, s.clone()))
                .collect::<Vec<_>>(),
        );
        // The palette the engine places, index for index.
        let placed = Palette::new();
        for entry in world.palette().entries() {
            let now = blocks
                .iter()
                .find(|(id, _, _)| *id == entry.id)
                .map_or(entry.id.as_str(), |(_, _, now)| now);
            placed.intern(now, &entry.properties).unwrap();
        }
        assert_eq!(placed.len(), world.palette().len());
        let (_, commands) = import_commands(&world, None, 64, Some(&placed), &facts).unwrap();
        let grass = blocks
            .iter()
            .find(|(_, _, now)| *now == "minecraft:short_grass")
            .unwrap()
            .0;
        let at = |id: &str| {
            commands
                .iter()
                .position(|c| c.ends_with(&format!(" {id}")) || c.contains(&format!(" {id}[")))
                .map_or(-1, |p| p as i64)
        };
        assert!(
            blocks.iter().all(|(id, _, _)| at(id) >= 0),
            "{version}: {}",
            commands.join(" | ")
        );
        for (id, _, now) in &blocks {
            if *now != "minecraft:short_grass" {
                assert!(at(id) < at(grass), "{version}: {id} before {grass}");
            }
        }
        assert!(at("minecraft:dirt") < at(grass));
    }
    // An old name read as the pinned version's is no block Minecraft has.
    let world = world_of(&[("minecraft:grass", vec![])]);
    assert!(
        import_commands(&world, None, 64, None, &facts)
            .unwrap_err()
            .contains("Minecraft has no block minecraft:grass")
    );
}
