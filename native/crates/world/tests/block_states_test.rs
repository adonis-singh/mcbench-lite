//! `test/block-states.test.js`, ported. `read` is the page's reading of the
//! bytes (`web/viewer.js` `loadResources`), as the test reads them.

mod common;

use common::block_table::{Options, TestState, block_table, render_layers, test_block_states};
use mcbench_world::block_states::BlockStates;
use mcbench_world::{BlockBox, BlockState, World, key_of};

fn cube(name: &'static str) -> TestState {
    TestState::cube(name)
}

/// The states of the first tests: a cube, a lamp, a clear block, a glow
/// block and a slab whose halves close a face only beside the other half.
fn states() -> BlockStates {
    test_block_states(
        &[
            cube("test:cube"),
            cube("test:lamp[lit=false]"),
            TestState {
                emission: 15,
                ..cube("test:lamp[lit=true]")
            },
            TestState::new("test:clear"),
            TestState {
                emission: 9,
                ..TestState::new("test:glow[level=9,wet=false]")
            },
            TestState {
                emission: 9,
                light_block: 1,
                ..TestState::new("test:glow[level=9,wet=true]")
            },
            TestState {
                emission: 3,
                ..TestState::new("test:glow[level=3,wet=false]")
            },
            TestState {
                emission: 3,
                light_block: 1,
                ..TestState::new("test:glow[level=3,wet=true]")
            },
            TestState {
                full: 1,
                faces: [1, 0, 2, 2, 2, 2],
                cull: Some([1, 0, 2, 2, 2, 2]),
                ..TestState::new("test:slab[type=bottom]")
            },
            TestState {
                full: 2,
                faces: [0, 1, 3, 3, 3, 3],
                cull: Some([0, 1, 3, 3, 3, 3]),
                ..TestState::new("test:slab[type=top]")
            },
        ],
        &Options {
            shapes: 4,
            pairs: vec![(2, 3), (3, 2)],
            cull_shapes: 4,
            hide_pairs: vec![(2, 2), (3, 3)],
            ..Options::default()
        },
    )
}

fn props(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn fact(states: &BlockStates, id: &str, properties: &[(&str, &str)]) -> (u8, u8, u8) {
    let s = states.id_of(id, &props(properties)).unwrap() as usize;
    (
        states.emission[s],
        states.light_block[s],
        states.full_faces[s],
    )
}

#[test]
fn a_state_is_found_by_its_block_and_properties_its_blocks_default_filling_the_properties_it_leaves_out()
 {
    let states = states();
    assert_eq!(states.id_of("minecraft:air", &[]).unwrap(), 0);
    assert_eq!(fact(&states, "test:glow", &[]), (9, 0, 0));
    assert_eq!(fact(&states, "test:glow", &[("wet", "true")]), (9, 1, 0));
    assert_eq!(
        fact(&states, "test:glow", &[("wet", "true"), ("level", "3")]),
        (3, 1, 0)
    );
    assert_eq!(
        [
            fact(&states, "test:lamp", &[]),
            fact(&states, "test:lamp", &[("lit", "true")])
        ],
        [(0, 15, 63), (15, 15, 63)]
    );
    assert!(
        states
            .id_of("test:glow", &props(&[("level", "16")]))
            .unwrap_err()
            .contains("no block state test:glow[level=16,wet=false]")
    );
    assert!(
        states
            .id_of("test:nothing", &[])
            .unwrap_err()
            .contains("no block test:nothing")
    );
}

#[test]
fn every_block_is_named_by_the_name_the_table_gives_it() {
    let states = states();
    assert_eq!(states.names["test:glow"], "Test test:glow");
    assert_eq!(states.names["minecraft:air"], "Test minecraft:air");
}

/// What the page reads of the bytes, each part on four bytes.
struct Page {
    count: usize,
    shapes: usize,
    cull_shapes: usize,
    fluid: Vec<u16>,
    skip_row: Vec<u16>,
    skip_column: Vec<u16>,
    skip_start: Vec<u32>,
    skip_first: Vec<u16>,
    skip_length: Vec<u16>,
    emission: Vec<u8>,
    light_block: Vec<u8>,
    full_faces: Vec<u8>,
    draw: Vec<u8>,
    faces: Vec<u8>,
    cull: Vec<u8>,
    occludes: Vec<u8>,
    hides: Vec<u8>,
    skip_masks: Vec<u8>,
    fluid_names: serde_json::Value,
}

fn read(bytes: &[u8]) -> Page {
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let (count, shapes, cull_shapes, rows, mask_bytes, name_bytes) = (
        u32_at(0),
        u32_at(4),
        u32_at(8),
        u32_at(12),
        u32_at(16),
        u32_at(20),
    );
    let mut at = 24;
    let mut take = |width: usize, length: usize| {
        let part = bytes[at..at + width * length].to_vec();
        at += (width * length).div_ceil(4) * 4;
        part
    };
    let u16s = |b: Vec<u8>| {
        b.chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect::<Vec<_>>()
    };
    let u32s = |b: Vec<u8>| {
        b.chunks(4)
            .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
            .collect::<Vec<_>>()
    };
    let page = Page {
        count,
        shapes,
        cull_shapes,
        fluid: u16s(take(2, count)),
        skip_row: u16s(take(2, count)),
        skip_column: u16s(take(2, count)),
        skip_start: u32s(take(4, rows)),
        skip_first: u16s(take(2, rows)),
        skip_length: u16s(take(2, rows)),
        emission: take(1, count),
        light_block: take(1, count),
        full_faces: take(1, count),
        draw: take(1, count),
        faces: take(1, count * 6),
        cull: take(1, count * 6),
        occludes: take(1, shapes * shapes),
        hides: take(1, cull_shapes * cull_shapes),
        skip_masks: take(1, mask_bytes),
        fluid_names: serde_json::from_slice(&take(1, name_bytes)).unwrap(),
    };
    assert_eq!(at, bytes.len(), "the page reads every byte");
    page
}

#[test]
fn the_bytes_the_page_reads_hold_every_states_facts_and_the_face_shape_pairs() {
    let states = states();
    let page = read(&states.bytes());
    assert_eq!((page.count, page.shapes), (states.count, 4));
    let bottom = states
        .id_of("test:slab", &props(&[("type", "bottom")]))
        .unwrap() as usize;
    let top = states
        .id_of("test:slab", &props(&[("type", "top")]))
        .unwrap() as usize;
    let glow = states.id_of("test:glow", &[]).unwrap() as usize;
    assert_eq!(
        (
            page.emission[glow],
            page.light_block[glow],
            page.full_faces[bottom]
        ),
        (9, 0, 1)
    );
    let east = |s: usize| page.faces[s * 6 + 5] as usize;
    let west = |s: usize| page.faces[s * 6 + 4] as usize;
    assert_eq!(page.occludes[east(bottom) * page.shapes + west(top)], 1);
    assert_eq!(page.occludes[east(bottom) * page.shapes + west(bottom)], 0);
    assert_eq!(
        page.occludes[page.faces[bottom * 6] as usize * page.shapes],
        1,
        "its full bottom closes the face below it alone"
    );
    let page_hides = |a: usize, b: usize, d: usize| {
        page.hides
            [page.cull[a * 6 + d] as usize * page.cull_shapes + page.cull[b * 6 + (d ^ 1)] as usize]
            == 1
    };
    let cube = states.id_of("test:cube", &[]).unwrap() as usize;
    for (a, b, d, hidden) in [
        (bottom, bottom, 5, true),
        (bottom, top, 5, false),
        (top, top, 4, true),
        (bottom, cube, 2, true),
        (bottom, 0, 2, false),
        (cube, bottom, 5, false),
        (cube, bottom, 1, true),
    ] {
        assert_eq!(states.hides_face(a, b, d), hidden, "{a} beside {b} to {d}");
        assert_eq!(
            page_hides(a, b, d),
            hidden,
            "the page: {a} beside {b} to {d}"
        );
    }
}

const ALL: u8 = 63;

fn drawn() -> BlockStates {
    test_block_states(
        &[
            TestState::new("test:pane[joined=false]"),
            TestState::new("test:pane[joined=true]"),
            TestState {
                model: false,
                fluid: Some("test:water[level=0]"),
                fluid_blended: true,
                ..TestState::new("test:water[level=0]")
            },
            TestState {
                model: false,
                fluid: Some("test:water[level=1]"),
                fluid_blended: true,
                ..TestState::new("test:water[level=1]")
            },
            TestState::new("test:stair[waterlogged=false]"),
            TestState {
                fluid: Some("test:water[level=0]"),
                fluid_blended: true,
                ..TestState::new("test:stair[waterlogged=true]")
            },
            TestState {
                blended: true,
                ..TestState::new("test:glass")
            },
            TestState {
                renderer: "test:banner",
                ..TestState::new("test:banner")
            },
        ],
        &Options {
            skips: vec![
                (
                    "test:pane[joined=true]",
                    "test:pane[joined=true]",
                    1 | 2 | 16 | 32,
                ),
                ("test:pane[joined=false]", "test:pane[joined=false]", 1 | 2),
                ("test:pane[joined=false]", "test:pane[joined=true]", 1 | 2),
                ("test:water[level=0]", "test:water[level=0]", ALL),
                ("test:water[level=0]", "test:water[level=1]", ALL),
                ("test:water[level=0]", "test:stair[waterlogged=true]", ALL),
                ("test:water[level=1]", "test:water[level=0]", ALL),
                ("test:water[level=1]", "test:water[level=1]", ALL),
                ("test:water[level=1]", "test:stair[waterlogged=true]", ALL),
            ],
            ..Options::default()
        },
    )
}

#[test]
fn skip_rendering_is_answered_for_each_ordered_pair_of_states_and_direction_as_the_table_holds_it_and_the_page_reads_the_same()
 {
    let drawn = drawn();
    let id = |name: &str| drawn.ids[name] as usize;
    let (joined, alone, water, stair, dry) = (
        id("test:pane[joined=true]"),
        id("test:pane[joined=false]"),
        id("test:water[level=0]"),
        id("test:stair[waterlogged=true]"),
        id("test:stair[waterlogged=false]"),
    );
    let water1 = id("test:water[level=1]");
    let expected = |a: usize, b: usize, d: usize| {
        if a == joined && b == joined {
            return [0, 1, 4, 5].contains(&d);
        }
        if a == alone && (b == alone || b == joined) {
            return d <= 1;
        }
        if a == water || a == water1 {
            return b == water || b == water1 || b == stair;
        }
        false
    };
    let page = read(&drawn.bytes());
    let page_skips = |a: usize, b: usize, d: usize| {
        let row = page.skip_row[a] as usize;
        let at = page.skip_column[b] as i64 - page.skip_first[row] as i64;
        at >= 0
            && at < page.skip_length[row] as i64
            && (page.skip_masks[page.skip_start[row] as usize + at as usize] >> d) & 1 == 1
    };
    for a in 0..drawn.count {
        for b in 0..drawn.count {
            for d in 0..6 {
                assert_eq!(
                    drawn.skips(a, b, d),
                    expected(a, b, d),
                    "{a} beside {b} to {d}"
                );
                assert_eq!(
                    page_skips(a, b, d),
                    expected(a, b, d),
                    "the page: {a} beside {b} to {d}"
                );
            }
        }
    }
    assert!(
        !drawn.skips(joined, alone, 4),
        "a joined pane does not skip beside one that is not"
    );
    assert!(
        !drawn.skips(stair, water, 1) && !drawn.skips(dry, water, 1),
        "a stair does not skip its faces beside water"
    );
}

#[test]
fn a_states_fluid_whether_its_model_is_drawn_the_layers_they_are_drawn_in_and_whether_a_block_entity_renderer_draws_it_reach_the_page()
 {
    let drawn = drawn();
    let id = |name: &str| drawn.ids[name] as usize;
    let page = read(&drawn.bytes());
    let water = id("test:water[level=0]");
    let water1 = id("test:water[level=1]");
    assert_eq!(
        [
            page.fluid[id("test:stair[waterlogged=true]")],
            page.fluid[water],
            page.fluid[water1],
            page.fluid[id("test:stair[waterlogged=false]")],
            page.fluid[0]
        ],
        [water as u16, water as u16, water1 as u16, 0, 0]
    );
    assert_eq!(
        page.fluid_names,
        serde_json::json!({ water.to_string(): "test:water[level=0]", water1.to_string(): "test:water[level=1]" })
    );
    assert_eq!(
        [
            page.draw[0],
            page.draw[water],
            page.draw[id("test:stair[waterlogged=true]")],
            page.draw[id("test:stair[waterlogged=false]")],
            page.draw[id("test:glass")],
            page.draw[id("test:banner")]
        ],
        [0, 4, 1 | 4, 1, 1 | 2, 1 | 16]
    );
}

#[test]
fn render_layers_that_name_another_state_than_the_block_table_are_refused() {
    let table = [
        TestState {
            model: false,
            ..TestState::new("minecraft:air")
        },
        TestState::new("test:a"),
        TestState::new("test:b"),
    ];
    let text = block_table(&table, &Options::default());
    let error = BlockStates::new(
        &text,
        &render_layers(&[table[0].clone(), table[2].clone(), table[1].clone()]),
    )
    .unwrap_err();
    assert!(
        error.contains("render layers name state 1 test:b, the block table test:a"),
        "{error}"
    );
    let error = BlockStates::new(&text, &render_layers(&table[..2])).unwrap_err();
    assert!(
        error.contains("render layers have 2 states, the block table 3"),
        "{error}"
    );
}

/// The blocks of a world's chunk as see_build's page is sent them.
fn shown(world: &World, states: &BlockStates, key: u32) -> Vec<String> {
    let palette = states.of_palette(world.palette()).unwrap();
    let chunk = world.chunk_snapshot(key, &palette, None).unwrap();
    let mut out: Vec<String> = chunk
        .blocks
        .iter()
        .map(|b| {
            format!(
                "{},{},{} {}",
                b.at[0], b.at[1], b.at[2], chunk.palette[b.palette].0.id
            )
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_block_is_left_out_only_where_it_and_the_six_blocks_around_it_have_six_full_faces() {
    let states = states();
    let mut world = World::new();
    world
        .fill(
            BlockBox {
                min: [0, 0, 0],
                max: [2, 2, 2],
            },
            BlockState::new("test:cube", &[]),
        )
        .unwrap();
    let key = key_of(0, 0, 0);
    assert_eq!(shown(&world, &states, key).len(), 26);
    assert!(!shown(&world, &states, key).contains(&"1,1,1 test:cube".to_string()));
    for (id, properties) in [
        ("test:glow", vec![]),
        ("test:clear", vec![]),
        ("test:slab", vec![("type", "bottom")]),
    ] {
        world
            .set([1, 2, 1], BlockState::new(id, &properties))
            .unwrap();
        assert!(
            shown(&world, &states, key).contains(&"1,1,1 test:cube".to_string()),
            "{id}"
        );
    }
    world
        .set([1, 2, 1], BlockState::new("test:cube", &[]))
        .unwrap();
    world
        .set([1, 1, 1], BlockState::new("test:lamp", &[("lit", "true")]))
        .unwrap();
    assert!(
        !shown(&world, &states, key)
            .iter()
            .any(|b| b.starts_with("1,1,1 "))
    );
    let palette = states.of_palette(world.palette()).unwrap();
    let chunk = world.chunk_snapshot(key, &palette, None).unwrap();
    let ids: Vec<(String, u16)> = chunk
        .palette
        .iter()
        .map(|(entry, state)| (entry.id.clone(), *state))
        .collect();
    assert_eq!(
        ids,
        vec![(
            "test:cube".to_string(),
            states.id_of("test:cube", &[]).unwrap()
        )]
    );
}

#[test]
fn a_chunks_version_changes_where_a_block_across_its_faces_hides_or_shows_its_edge_and_only_there()
{
    let states = states();
    let mut world = World::new();
    world
        .fill(
            BlockBox {
                min: [14, 0, 0],
                max: [17, 2, 2],
            },
            BlockState::new("test:cube", &[]),
        )
        .unwrap();
    let (left, right) = (key_of(0, 0, 0), key_of(1, 0, 0));
    let version =
        |world: &World| world.shown_version(left, &states.of_palette(world.palette()).unwrap());
    let before = version(&world);
    assert!(!shown(&world, &states, left).contains(&"15,1,1 test:cube".to_string()));
    world
        .set([17, 1, 1], BlockState::new("test:clear", &[]))
        .unwrap();
    assert_eq!(version(&world), before);
    world
        .set([16, 1, 1], BlockState::new("test:clear", &[]))
        .unwrap();
    let after = version(&world);
    assert_ne!(after, before);
    assert!(shown(&world, &states, left).contains(&"15,1,1 test:cube".to_string()));
    assert_eq!(
        world
            .chunk_snapshot(left, &states.of_palette(world.palette()).unwrap(), None)
            .unwrap()
            .version,
        after
    );
    world
        .set([16, 1, 1], BlockState::new("test:cube", &[]))
        .unwrap();
    assert_eq!(version(&world), before);
    for x in 16..=17 {
        for y in 0..=2 {
            for z in 0..=2 {
                world
                    .set([x, y, z], BlockState::new("minecraft:air", &[]))
                    .unwrap();
            }
        }
    }
    assert!(world.chunk(right).is_none_or(|c| c.count() == 0));
    assert_ne!(version(&world), before);
    assert!(shown(&world, &states, left).contains(&"15,1,1 test:cube".to_string()));
}
