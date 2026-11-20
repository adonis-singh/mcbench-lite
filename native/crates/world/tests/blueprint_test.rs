//! `test/blueprint.test.js`, ported, and the one assertion of
//! `test/live-patch.test.js` whose subject is this crate (a blueprint of
//! one lever holds one block).

use std::sync::Arc;

use mcbench_world::blueprint::{
    blocks_blueprint, blueprint_world, encode_blueprint, for_each_blueprint_block, read_blueprint,
    world_blueprint,
};
use mcbench_world::{BlockState, CHUNK_VOLUME, Palette, World};

#[test]
fn a_voxel_worlds_blueprint_names_its_blocks_at_minecraft_height_properties_sorted() {
    let mut world = World::new();
    world
        .set(
            [1, 0, -3],
            BlockState::new(
                "minecraft:oak_stairs",
                &[("half", "top"), ("facing", "east")],
            ),
        )
        .unwrap();
    world
        .set([1, 1, -3], BlockState::new("minecraft:stone", &[]))
        .unwrap();
    world.finalize();
    let read: Vec<(String, String)> = read_blueprint(&world_blueprint(&world, 64).unwrap())
        .unwrap()
        .into_iter()
        .collect();
    assert_eq!(
        read,
        vec![
            (
                "1,64,-3".to_string(),
                "minecraft:oak_stairs[facing=east,half=top]".to_string()
            ),
            ("1,65,-3".to_string(), "minecraft:stone".to_string())
        ]
    );
    let empty = blocks_blueprint(&[]).unwrap();
    assert_eq!(
        i32::from_be_bytes(empty[0..4].try_into().unwrap()),
        1,
        "air alone"
    );
    assert_eq!(
        i32::from_be_bytes(empty[empty.len() - 4..].try_into().unwrap()),
        0,
        "no sections"
    );
}

/// Setting every block of a blueprint into a new World, one at a time.
fn per_block_world(bytes: &[u8], y_offset: i32) -> Result<World, String> {
    let mut world = World::new();
    let mut failure = Ok(());
    for_each_blueprint_block(bytes, |x, y, z, (id, properties, _)| {
        if failure.is_ok() {
            failure = world.set_block([x, y - y_offset, z], id, properties);
        }
    })?;
    failure.map(|_| world)
}

/// Everything observable about a World: palette order, chunk order, how each
/// chunk stores its blocks.
type Shape = (
    Vec<(String, Vec<(String, String)>)>,
    usize,
    Vec<(u32, usize, bool, Vec<(u16, u16)>)>,
);

fn shape(world: &World) -> Shape {
    let palette = world
        .palette()
        .entries()
        .map(|e| (e.id.clone(), e.properties.clone()))
        .collect();
    let chunks = world
        .chunks()
        .map(|(key, chunk)| {
            let cells = match chunk.cells() {
                mcbench_world::chunk::Cells::Dense(dense) => dense
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i as u16, *v))
                    .collect(),
                mcbench_world::chunk::Cells::Sparse(sparse) => {
                    sparse.iter().map(|(i, v)| (*i, *v)).collect()
                }
            };
            (key, chunk.count(), chunk.is_dense(), cells)
        })
        .collect();
    (palette, world.size(), chunks)
}

fn random_blueprint(seed: u32) -> Vec<u8> {
    let mut state = seed;
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        state as f64 / 2f64.powi(32)
    };
    let names = [
        "minecraft:stone",
        "minecraft:oak_stairs[facing=north,half=bottom]",
        "minecraft:water[level=3]",
        "minecraft:torch",
        "minecraft:oak_fence[east=true,west=false]",
    ];
    let palette: Vec<String> = std::iter::once("minecraft:air")
        .chain(names)
        .map(str::to_string)
        .collect();
    let mut sections = Vec::new();
    for origin in [[0, 0, 0], [0, 1, 0], [-1, 0, 2], [3, -2, -4], [0, 5, 1]] {
        let density = if random() < 0.5 { 0.9 } else { 0.02 };
        let mut indices = Box::new([0u16; CHUNK_VOLUME]);
        for slot in indices.iter_mut() {
            if random() < density {
                *slot = 1 + (random() * names.len() as f64).floor() as u16;
            }
        }
        sections.push((origin, indices));
    }
    let refs: Vec<([i32; 3], &[u16; CHUNK_VOLUME])> =
        sections.iter().map(|(o, s)| (*o, &**s)).collect();
    encode_blueprint(&palette, &refs, [0; 6]).unwrap()
}

fn blocks(world: &World) -> Vec<String> {
    let mut out = Vec::new();
    world.for_each(None, |p, e| {
        out.push(format!(
            "{},{},{} {}{:?}",
            p[0], p[1], p[2], e.id, e.properties
        ))
    });
    out.sort();
    out
}

#[test]
fn the_blueprint_of_the_engines_world_becomes_the_world_setting_its_blocks_one_by_one_would_make() {
    for seed in [1, 2, 3, 4] {
        let bytes = random_blueprint(seed);
        assert_eq!(
            shape(&blueprint_world(&bytes, 64, None).unwrap()),
            shape(&per_block_world(&bytes, 64).unwrap())
        );
        let shifted = blueprint_world(&bytes, 5, None).unwrap();
        let expected = per_block_world(&bytes, 5).unwrap();
        assert_eq!(blocks(&shifted), blocks(&expected));
        assert_eq!(shifted.size(), expected.size());
    }
    let shared: Arc<Palette> = per_block_world(&random_blueprint(1), 64)
        .unwrap()
        .palette()
        .clone();
    let world = blueprint_world(&random_blueprint(1), 64, Some(shared.clone())).unwrap();
    assert!(Arc::ptr_eq(world.palette(), &shared));
    let length = shared.len();
    assert_eq!(
        blueprint_world(&random_blueprint(1), 64, Some(shared.clone()))
            .unwrap()
            .palette()
            .len(),
        length
    );
    assert_eq!(
        blueprint_world(
            &encode_blueprint(&["minecraft:air".to_string()], &[], [0; 6]).unwrap(),
            64,
            None
        )
        .unwrap()
        .size(),
        0
    );
}

#[test]
fn a_blueprint_block_beyond_the_world_limits_is_refused_as_setting_it_would_be() {
    let indices = Box::new([1u16; CHUNK_VOLUME]);
    let bytes = encode_blueprint(
        &["minecraft:air".to_string(), "minecraft:stone".to_string()],
        &[([257, 0, 0], &indices)],
        [0; 6],
    )
    .unwrap();
    assert!(
        blueprint_world(&bytes, 0, None)
            .unwrap_err()
            .contains("out of range")
    );
    assert!(
        per_block_world(&bytes, 0)
            .unwrap_err()
            .contains("out of range")
    );
}

#[test]
fn a_blueprint_of_one_lever_holds_one_block() {
    // live-patch.test.js: the page's reader of this blueprint finds one block.
    let bytes = blocks_blueprint(&[(
        1,
        64,
        1,
        "minecraft:lever[face=floor,facing=north,powered=true]".to_string(),
    )])
    .unwrap();
    let mut count = 0;
    for_each_blueprint_block(&bytes, |_, _, _, _| count += 1).unwrap();
    assert_eq!(count, 1);
}
