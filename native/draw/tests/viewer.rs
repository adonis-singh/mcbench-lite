// What the deleted page's drawing tests checked (test/viewer-draw,
// viewer-block-entities, viewer-entities, viewer-reset), checked of the
// native drawing with Minecraft's own tables: tints, glass, water,
// waterlogged blocks, corner shading, block entities, entities, and a world
// drawn after others were dropped. Needs Minecraft's baked data
// (MCBENCH_DRAW_DATA); the reset tests run on the synthetic pack.

mod common;

use mcbench_draw::assets::{Assets, Paths};
use mcbench_draw::scene::{light_curve, Draw, Quad};
use mcbench_renderer::tile;
use mcbench_renderer::world::{Mask, World};

const Y: i32 = 64;
const EVERYWHERE: Mask = Mask {
    min_x: -1000,
    max_x: 1000,
    min_z: -1000,
    max_z: 1000,
};

fn assets(test: &str) -> Option<Assets> {
    let data = common::data(test)?;
    Some(Assets::load(&Paths::of_data(&data).unwrap()).unwrap())
}

// The quads of named blocks at page heights (Minecraft's less 64).
fn quads(assets: &Assets, blocks: &[(&str, [i32; 3])]) -> Vec<Quad> {
    let lifted: Vec<(&str, [i32; 3])> = blocks
        .iter()
        .map(|(n, p)| (*n, [p[0], p[1] + Y, p[2]]))
        .collect();
    let world = World::read(&assets.pack, &common::blueprint(&lifted), EVERYWHERE).unwrap();
    Draw {
        pack: &assets.pack,
        facts: &assets.facts,
        y_offset: Y,
    }
    .quads(&world)
}

fn of(quads: &[Quad], block: [i32; 3]) -> Vec<&Quad> {
    quads.iter().filter(|q| q.block == block).collect()
}

// A face on the plane just inside a block's east side.
fn toward_east(q: &Quad) -> bool {
    q.corners.iter().all(|c| (c[0] - 0.999).abs() < 1e-3)
}

fn hollow_box(name: &'static str, lo: [i32; 3], hi: [i32; 3]) -> Vec<(&'static str, [i32; 3])> {
    let mut out = Vec::new();
    for x in lo[0]..=hi[0] {
        for y in lo[1]..=hi[1] {
            for z in lo[2]..=hi[2] {
                if [x, y, z]
                    .iter()
                    .zip(lo.iter().zip(&hi))
                    .any(|(v, (l, h))| v == l || v == h)
                {
                    out.push((name, [x, y, z]));
                }
            }
        }
    }
    out
}

// viewer-draw: a tinted face takes Minecraft's colour for its tint index;
// water takes the fluid's tint; an untinted block stays white.
#[test]
fn tints_follow_the_tint_index_and_the_fluid() {
    let Some(a) = assets("tints_follow_the_tint_index_and_the_fluid") else {
        return;
    };
    let q = quads(
        &a,
        &[
            ("minecraft:grass_block[snowy=false]", [0, 0, 0]),
            ("minecraft:water[level=0]", [3, 0, 0]),
            ("minecraft:stone", [6, 0, 0]),
        ],
    );
    let up = |b: [i32; 3]| {
        of(&q, b)
            .into_iter()
            .find(|q| q.normal[1] > 0.99)
            .unwrap()
            .clone()
    };
    let grass = up([0, 0, 0]);
    assert!(
        grass.tint[1] > grass.tint[0] && grass.tint[1] < 1.0,
        "grass top {:?}",
        grass.tint
    );
    let water = up([3, 0, 0]);
    assert_eq!(water.kind, tile::FLUID);
    assert!(
        water.tint[2] > water.tint[0] && water.tint != [1.0; 3],
        "water {:?}",
        water.tint
    );
    assert_eq!(up([6, 0, 0]).tint, [1.0; 3]);
}

// viewer-draw: stained glass hides its face toward the same glass and is
// drawn blended.
#[test]
fn glass_culls_toward_glass_and_blends() {
    let Some(a) = assets("glass_culls_toward_glass_and_blends") else {
        return;
    };
    let q = quads(
        &a,
        &[
            ("minecraft:white_stained_glass", [0, 0, 0]),
            ("minecraft:white_stained_glass", [1, 0, 0]),
        ],
    );
    assert_eq!(q.len(), 10, "two cubes less the two faces between them");
    assert!(q.iter().all(|q| q.transparent));
    assert!(
        !q.iter().any(|q| q.corners.iter().all(|c| c[0] == 1.0)),
        "no face on the plane between them"
    );
}

// viewer-draw: water alone is a blended fluid; it hides its faces toward
// water and toward a block that holds water.
#[test]
fn water_blends_and_hides_faces_toward_water() {
    let Some(a) = assets("water_blends_and_hides_faces_toward_water") else {
        return;
    };
    let alone = quads(&a, &[("minecraft:water[level=0]", [0, 0, 0])]);
    assert!(!alone.is_empty() && alone.iter().all(|q| q.kind == tile::FLUID && q.transparent));
    let pair = quads(
        &a,
        &[
            ("minecraft:water[level=0]", [0, 0, 0]),
            ("minecraft:water[level=0]", [1, 0, 0]),
        ],
    );
    // A side face lies just inside the block (x 0.999) and is drawn from
    // both sides: none at all on the side toward the water.
    assert!(
        of(&alone, [0, 0, 0])
            .iter()
            .filter(|q| toward_east(q))
            .count()
            == 2,
        "alone: the east side, from both sides"
    );
    assert!(
        !of(&pair, [0, 0, 0]).iter().any(|q| toward_east(q)),
        "no water face toward water"
    );
    let held = quads(
        &a,
        &[
            ("minecraft:water[level=0]", [0, 0, 0]),
            (
                "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]",
                [1, 0, 0],
            ),
        ],
    );
    assert!(
        !of(&held, [0, 0, 0]).iter().any(|q| toward_east(q)),
        "no water face toward a waterlogged stair"
    );
}

// viewer-draw: a waterlogged stair is its model, opaque, and water, blended.
#[test]
fn a_waterlogged_stair_is_its_model_and_water() {
    let Some(a) = assets("a_waterlogged_stair_is_its_model_and_water") else {
        return;
    };
    let q = quads(
        &a,
        &[(
            "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]",
            [0, 0, 0],
        )],
    );
    assert!(q.iter().any(|q| q.kind == tile::MODEL && !q.transparent));
    assert!(q.iter().any(|q| q.kind == tile::FLUID && q.transparent));
}

// viewer-draw: a corner beside a block that shades is darker than the
// face's others; an emitter's faces are not shaded.
#[test]
fn corners_shade_by_shading_blocks_and_not_on_emitters() {
    let Some(a) = assets("corners_shade_by_shading_blocks_and_not_on_emitters") else {
        return;
    };
    let mut blocks: Vec<(&str, [i32; 3])> = Vec::new();
    for x in -1..=1 {
        for z in -1..=1 {
            blocks.push(("minecraft:stone", [x, 0, z]));
        }
    }
    blocks.push(("minecraft:stone", [0, 1, 0]));
    blocks.push(("minecraft:glowstone", [5, 0, 0]));
    let q = quads(&a, &blocks);
    let top = of(&q, [1, 0, 0])
        .into_iter()
        .find(|q| q.normal[1] > 0.99)
        .unwrap()
        .clone();
    let near = top
        .corners
        .iter()
        .zip(top.shade)
        .filter(|(c, _)| c[0] == 1.0)
        .map(|(_, s)| s)
        .fold(f32::MAX, f32::min);
    let far = top
        .corners
        .iter()
        .zip(top.shade)
        .filter(|(c, _)| c[0] == 2.0)
        .map(|(_, s)| s)
        .fold(f32::MIN, f32::max);
    assert!(
        near < far,
        "the corners beside the block above are darker: {:?}",
        top.shade
    );
    assert!(
        of(&q, [5, 0, 0])
            .iter()
            .all(|q| q.emissive && q.shade == [1.15; 4]),
        "an emitter is drawn at 1.15 unshaded"
    );
}

// viewer-block-entities: what a block entity renderer draws is drawn,
// unshaded, in the light of its own cell.
#[test]
fn block_entities_are_drawn_unshaded_in_their_cells_light() {
    let Some(a) = assets("block_entities_are_drawn_unshaded_in_their_cells_light") else {
        return;
    };
    let open = quads(
        &a,
        &[(
            "minecraft:chest[facing=north,type=single,waterlogged=false]",
            [0, 0, 0],
        )],
    );
    let entity: Vec<&Quad> = open.iter().filter(|q| q.kind == tile::ENTITY).collect();
    assert!(!entity.is_empty());
    assert!(
        entity.iter().all(|q| q.shade == [light_curve(15.0); 4]),
        "open sky: full light, no corner shading"
    );
    let mut dark = hollow_box("minecraft:stone", [-2, -2, -2], [2, 2, 2]);
    dark.push((
        "minecraft:chest[facing=north,type=single,waterlogged=false]",
        [0, 0, 0],
    ));
    let closed = quads(&a, &dark);
    let inside: Vec<&Quad> = closed.iter().filter(|q| q.kind == tile::ENTITY).collect();
    assert_eq!(inside.len(), entity.len());
    assert!(
        inside.iter().all(|q| q.shade == [light_curve(0.0); 4]),
        "a sealed box: no light"
    );
}

// viewer-block-entities: block entity quads are not culled by the blocks
// around them (Minecraft draws a block entity whole).
#[test]
fn block_entity_quads_are_not_culled_by_neighbours() {
    let Some(a) = assets("block_entity_quads_are_not_culled_by_neighbours") else {
        return;
    };
    let chest = "minecraft:chest[facing=north,type=single,waterlogged=false]";
    let alone = quads(&a, &[(chest, [0, 0, 0])])
        .into_iter()
        .filter(|q| q.kind == tile::ENTITY)
        .count();
    let mut around = vec![(chest, [0, 0, 0])];
    for d in [
        [1, 0, 0],
        [-1, 0, 0],
        [0, 1, 0],
        [0, -1, 0],
        [0, 0, 1],
        [0, 0, -1],
    ] {
        around.push(("minecraft:stone", d));
    }
    let surrounded = quads(&a, &around)
        .into_iter()
        .filter(|q| q.kind == tile::ENTITY)
        .count();
    assert_eq!(alone, surrounded);
}

// viewer-block-entities: a block that a block entity renderer draws but
// whose model is not the renderer's (a chest) draws no model quads.
#[test]
fn a_chest_is_its_block_entity_quads() {
    let Some(a) = assets("a_chest_is_its_block_entity_quads") else {
        return;
    };
    let q = quads(
        &a,
        &[(
            "minecraft:chest[facing=north,type=single,waterlogged=false]",
            [0, 0, 0],
        )],
    );
    assert!(
        q.iter().all(|q| q.kind == tile::ENTITY),
        "{:?}",
        q.iter().map(|q| q.kind).collect::<Vec<_>>()
    );
}

fn scene_with(
    a: &Assets,
    blocks: &[(&str, [i32; 3])],
    entities: Vec<serde_json::Value>,
) -> mcbench_draw::store::Scene {
    let gpu = mcbench_draw::gpu::Gpu::open(&common::choice()).unwrap();
    let lifted: Vec<(&str, [i32; 3])> = blocks
        .iter()
        .map(|(n, p)| (*n, [p[0], p[1] + Y, p[2]]))
        .collect();
    let kept = mcbench_draw::store::Kept {
        blueprint: common::blueprint(&lifted),
        y_offset: Y,
        entities,
        used: 0,
    };
    mcbench_draw::store::make_scene(a, &gpu, &kept, None, &mut |_| {})
        .unwrap()
        .0
}

// viewer-entities: a world without entities loads no entity capture.
#[test]
fn a_world_without_entities_loads_no_capture() {
    let Some(a) = assets("a_world_without_entities_loads_no_capture") else {
        return;
    };
    let scene = scene_with(&a, &[("minecraft:stone", [0, 0, 0])], Vec::new());
    assert!(scene.sorted.solid.is_empty());
    assert!(!a.entity_loaded());
}

// viewer-entities: an entity's quads are placed by its frame and lit by the
// light of its cell (through its kind's light).
#[test]
fn entity_quads_are_placed_by_frame_and_lit_by_cell() {
    let Some(a) = assets("entity_quads_are_placed_by_frame_and_lit_by_cell") else {
        return;
    };
    let cart = |x: f64| serde_json::json!({ "id": 3, "type": "minecraft:minecart", "pos": [x, 0.0, 0.5], "yaw": 0.0, "pitch": 0.0, "width": 0.98, "height": 0.7, "ticks": 5, "block": "minecraft:air", "offset": 6 });
    let mut blocks = hollow_box("minecraft:stone", [-2, -2, -2], [2, 2, 2]);
    blocks.push(("minecraft:stone", [20, 0, 0]));
    let scene = scene_with(&a, &blocks, vec![cart(0.5), cart(30.5)]);
    assert!(a.entity_loaded());
    let (inside, outside): (Vec<_>, Vec<_>) =
        scene.sorted.solid.iter().partition(|p| p.entity == 0);
    assert!(!inside.is_empty() && !outside.is_empty());
    // In the sealed box: no light; in open sky: full light.
    assert!(
        inside.iter().all(|p| p.quad.light == [0.0, 0.0]),
        "{:?}",
        inside[0].quad.light
    );
    assert!(outside.iter().all(|p| p.quad.light == [15.0, 15.0]));
    // Placed by the frame: the same quads 30 blocks apart.
    for (i, o) in inside.iter().zip(&outside) {
        for (ci, co) in i.corners.iter().zip(&o.corners) {
            assert!(
                (co[0] - ci[0] - 30.0).abs() < 1e-9
                    && (co[1] - ci[1]).abs() < 1e-9
                    && (co[2] - ci[2]).abs() < 1e-9
            );
        }
    }
}

// viewer-reset: dropping a world frees its scenes and their GPU geometry.
#[test]
fn dropping_a_world_frees_its_scenes() {
    let a = Assets::load(&mcbench_draw::synthetic::paths(
        &common::crate_dir().join("tests/synthetic"),
    ))
    .unwrap();
    let gpu = mcbench_draw::gpu::Gpu::open(&common::choice()).unwrap();
    let kept = mcbench_draw::store::Kept {
        blueprint: mcbench_draw::synthetic::blueprint(&mcbench_draw::synthetic::world()),
        y_offset: 64,
        entities: Vec::new(),
        used: 0,
    };
    let (scene, _) = mcbench_draw::store::make_scene(&a, &gpu, &kept, None, &mut |_| {}).unwrap();
    let (clip, _) = mcbench_draw::store::make_scene(
        &a,
        &gpu,
        &kept,
        Some(mcbench_draw::store::Clip {
            min: [2, 1, 2],
            max: [4, 3, 4],
        }),
        &mut |_| {},
    )
    .unwrap();
    let mut store = mcbench_draw::store::Store::new(mcbench_draw::store::Limits::new(None, false));
    store.put_world("a", kept);
    store.put_scene(("a".into(), None), scene);
    store.put_scene(
        (
            "a".into(),
            Some(mcbench_draw::store::Clip {
                min: [2, 1, 2],
                max: [4, 3, 4],
            }),
        ),
        clip,
    );
    assert!(store.scenes.values().map(|s| s.bytes()).sum::<u64>() > 0);
    assert!(store.drop_world("a"));
    assert!(store.scenes.is_empty() && store.worlds.is_empty());
}
