//! `test/minecraft-facts.test.js` (2 tests) as Rust tests: the facts of a
//! table written for the test, and of Minecraft's own table (its path in
//! `MCBENCH_BLOCKS_TXT`; ignored without it, and failing when it is set and
//! the file is missing).

use mcbench_world::Palette;
use mcbench_world::facts::{LastPass, MinecraftFacts};

// The lines of the engine's table MinecraftFacts reads, for a test: blocks
// with their class, properties and states (the default second), an item
// tag, what each state placed alone does and where a full block holds it.
const TEXT: &str = "block\tminecraft:stone\tnet.minecraft.world.level.block.Block\t-\t0\t1\t0\t0.6\t1.0\t1.0\t-\t-\t6.0\n\
state\t0\t-\n\
block\ttest:torch\tnet.minecraft.world.level.block.TorchBlock\t-\t1\t2\t2\t0.6\t1.0\t1.0\t-\tBaseTorchBlock\t0.0\n\
property\tlit\ttrue,false\n\
state\t1\t0\n\
state\t2\t1\n\
itemtag\tminecraft:lectern_books\tminecraft:written_book,minecraft:writable_book\n\
alone\t0,2,0\n\
held\t1:1,2:0";

fn props(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn the_facts_name_states_by_their_properties_defaults_where_none_is_named_and_read_what_each_does_alone()
 {
    let facts = MinecraftFacts::new(TEXT).unwrap();
    assert_eq!(
        [
            facts.state_id("minecraft:stone", &[]).unwrap(),
            facts.state_id("test:torch", &[]).unwrap(),
            facts
                .state_id("test:torch", &props(&[("lit", "true")]))
                .unwrap()
        ],
        [Some(0), Some(2), Some(1)]
    );
    assert!(
        facts
            .state_id("test:torch", &props(&[("lit", "maybe")]))
            .unwrap_err()
            .contains("no block state test:torch[lit=maybe]")
    );
    assert!(
        facts
            .state_id("test:lamp", &[])
            .unwrap_err()
            .contains("no block test:lamp")
    );
    assert_eq!(facts.class_name("test:torch").unwrap(), "TorchBlock");
    assert_eq!(
        [
            facts.is("test:torch", "TorchBlock").unwrap(),
            facts.is("test:torch", "BaseTorchBlock").unwrap(),
            facts.is("test:torch", "Block").unwrap()
        ],
        [true, true, false]
    );
    assert_eq!(
        facts.defaults("test:torch").unwrap(),
        props(&[("lit", "false")])
    );
    assert_eq!(
        facts.item_tag("minecraft:lectern_books").unwrap(),
        ["minecraft:written_book", "minecraft:writable_book"]
    );
    assert_eq!(facts.survives, [1, 0, 0]);
    assert_eq!(
        facts
            .held_by("test:torch", &props(&[("lit", "true")]))
            .unwrap(),
        [[0, -1, 0]]
    );
    assert!(facts.held_by("test:torch", &[]).unwrap().is_empty());
    let palette = Palette::new();
    palette.intern("minecraft:stone", &[]).unwrap();
    palette
        .intern("test:torch", &props(&[("lit", "true")]))
        .unwrap();
    assert_eq!(facts.last_pass(&palette).unwrap(), [0, 0, 1]);
    palette.intern("test:torch", &[]).unwrap();
    assert_eq!(
        facts.last_pass(&palette).unwrap(),
        [0, 0, 1, 0],
        "an entry added later is read then"
    );
}

#[test]
#[ignore = "needs Minecraft's blocks.txt: set MCBENCH_BLOCKS_TXT to its path"]
fn minecrafts_table_a_lectern_takes_written_and_writable_books_beds_are_bed_block_and_three_blocks_are_air()
 {
    let file = std::env::var("MCBENCH_BLOCKS_TXT").expect("MCBENCH_BLOCKS_TXT names blocks.txt");
    let facts = MinecraftFacts::new(
        &std::fs::read_to_string(&file).expect("MCBENCH_BLOCKS_TXT is readable"),
    )
    .unwrap();
    let mut books = facts.item_tag("minecraft:lectern_books").unwrap().to_vec();
    books.sort();
    assert_eq!(books, ["minecraft:writable_book", "minecraft:written_book"]);
    let beds: Vec<&str> = facts
        .blocks()
        .map(|(id, _)| id)
        .filter(|id| facts.class_name(id).unwrap() == "BedBlock")
        .collect();
    assert_eq!(beds.len(), 16);
    assert!(beds.iter().all(|id| id.ends_with("_bed")));
    let air: Vec<&str> = facts
        .blocks()
        .map(|(id, _)| id)
        .filter(|id| facts.is_air(id).unwrap())
        .collect();
    assert_eq!(
        air,
        ["minecraft:air", "minecraft:void_air", "minecraft:cave_air"]
    );
}
