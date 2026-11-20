// An edit applied to a kept world (store::patch_scene: the mesher's patch,
// only the tiles it names meshed again) against the edited world loaded
// fresh: every tile's instances (their digest) and the drawn pictures, byte
// for byte, over random edit sequences: blocks set and removed, emitters,
// edits on tile and chunk borders, the bounds growing and shrinking, new
// palette entries. On the synthetic pack, and on a real build where
// Minecraft's data is present (MCBENCH_DRAW_DATA).

mod common;

use mcbench_draw::assets::{Assets, Paths};
use mcbench_draw::gpu::{Gpu, Renderer, COLOR};
use mcbench_draw::store::{self, Kept, Scene, Shot};
use mcbench_draw::synthetic;
use std::collections::BTreeMap;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn range(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

fn kept(blueprint: Vec<u8>, y_offset: i32) -> Kept {
    Kept {
        blueprint,
        y_offset,
        entities: Vec::new(),
        used: 0,
    }
}

fn pictures(
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    scene: &Scene,
    shots: &[Shot],
) -> Vec<Vec<u8>> {
    shots
        .iter()
        .map(|shot| {
            store::draw_shot(assets, gpu, renderer, scene, shot, 2)
                .unwrap()
                .0
        })
        .collect()
}

// Applies each blueprint in turn as an edit of the one before and compares
// with a fresh load. Answers the tiles meshed by the edits and the tiles
// in all.
fn check(
    assets: &Assets,
    gpu: &Gpu,
    renderer: &Renderer,
    blueprints: &[Vec<u8>],
    y_offset: i32,
    shots: &[Shot],
) -> (usize, usize) {
    let (mut scene, _) = store::make_scene(
        assets,
        gpu,
        &kept(blueprints[0].clone(), y_offset),
        None,
        &mut |_| {},
    )
    .unwrap();
    let (mut meshed, mut total) = (0, 0);
    for (step, pair) in blueprints.windows(2).enumerate() {
        let new = kept(pair[1].clone(), y_offset);
        let (patched, _, n) = store::patch_scene(assets, gpu, scene, &pair[0], &new, &mut |_| {})
            .unwrap_or_else(|e| panic!("step {step}: {e:?}"));
        let (fresh, _) = store::make_scene(assets, gpu, &new, None, &mut |_| {}).unwrap();
        assert_eq!(patched.tiles, fresh.tiles, "step {step}: tiles");
        let differ: Vec<_> = patched
            .digests()
            .into_iter()
            .zip(fresh.digests())
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a.0)
            .collect();
        assert!(
            differ.is_empty(),
            "step {step}: tiles {differ:?} differ from a fresh load ({n} meshed)"
        );
        assert_eq!(
            (patched.bounds, patched.blocks_count),
            (fresh.bounds, fresh.blocks_count),
            "step {step}"
        );
        assert_eq!(patched.quads(), fresh.quads(), "step {step}: quad counts");
        assert!(
            pictures(assets, gpu, renderer, &patched, shots)
                == pictures(assets, gpu, renderer, &fresh, shots),
            "step {step}: pictures differ"
        );
        meshed += n;
        total += fresh.tiles.len();
        scene = patched;
    }
    (meshed, total)
}

// A random sequence of block maps, starting from `start`: each step sets or
// removes a few blocks, near tile (32) and chunk (16) borders more often
// than not, sometimes far out (growing the bounds) and sometimes clearing
// what is far out (shrinking them).
fn sequence(
    start: &BTreeMap<[i32; 3], u16>,
    states: &[u16],
    y0: i32,
    seed: u64,
    steps: usize,
) -> Vec<BTreeMap<[i32; 3], u16>> {
    let mut rng = Rng(seed);
    let mut blocks = start.clone();
    let mut out = vec![blocks.clone()];
    for _ in 0..steps {
        for _ in 0..rng.range(1, 6) {
            let border = [
                -33, -32, -31, -17, -16, -15, -1, 0, 1, 15, 16, 17, 29, 30, 31, 32, 33,
            ];
            let axis = |rng: &mut Rng| {
                if rng.next().is_multiple_of(2) {
                    border[(rng.next() % border.len() as u64) as usize]
                } else {
                    rng.range(-12, 12)
                }
            };
            let at = [axis(&mut rng), y0 + rng.range(0, 8), axis(&mut rng)];
            match rng.next() % 4 {
                0 => {
                    blocks.remove(&at);
                }
                _ => {
                    blocks.insert(at, states[(rng.next() % states.len() as u64) as usize]);
                }
            }
        }
        match rng.next() % 6 {
            0 => {
                let far = [
                    rng.range(60, 90),
                    y0 + rng.range(0, 30),
                    rng.range(-90, -60),
                ];
                blocks.insert(far, states[(rng.next() % states.len() as u64) as usize]);
            }
            1 => blocks.retain(|at, _| at[0] < 50),
            _ => {}
        }
        out.push(blocks.clone());
    }
    out
}

#[test]
fn edits_equal_fresh_loads_on_the_synthetic_pack() {
    let assets = Assets::load(&synthetic::paths(
        &common::crate_dir().join("tests/synthetic"),
    ))
    .unwrap();
    let gpu = Gpu::open(&common::choice()).unwrap();
    let renderer = Renderer::new(&gpu, &assets.atlas, COLOR);
    let shots: Vec<Shot> = serde_json::from_value(synthetic::shots()).unwrap();
    // The iso view and the view of the fluids.
    let shots: Vec<Shot> = vec![shots[0].clone(), shots[shots.len() - 1].clone()];
    let start: BTreeMap<[i32; 3], u16> = synthetic::world().into_iter().collect();
    // Every state but air: stone, glass, lamp (emits), leaves, slabs, torch
    // (emits), sign (a block entity), water at every level, lava, a slab
    // holding water.
    let states: Vec<u16> = (1..19).collect();
    for seed in [7u64, 1234567, 99991] {
        let maps = sequence(&start, &states, 64, seed, 12);
        // Compact palettes: an edit adds palette entries the base lacked.
        let blueprints: Vec<Vec<u8>> = maps
            .iter()
            .map(|m| {
                synthetic::blueprint_compact(&m.iter().map(|(a, s)| (*a, *s)).collect::<Vec<_>>())
            })
            .collect();
        let (meshed, total) = check(
            &assets,
            &gpu,
            &renderer,
            &blueprints,
            synthetic::Y_OFFSET,
            &shots,
        );
        eprintln!("seed {seed}: {meshed} tiles meshed by 12 edits, of {total}");
        assert!(meshed < total, "edits mesh fewer tiles than fresh loads");
    }
}

// A real build: blocks of Minecraft's states set and removed in it, by the
// engine's own blueprint format (its palette entries are reused).
#[test]
fn edits_equal_fresh_loads_on_a_real_build() {
    let Some(data) = common::data("edits_equal_fresh_loads_on_a_real_build") else {
        return;
    };
    let Some(build) = std::env::var_os("MCBENCH_DRAW_BUILD") else {
        eprintln!("edits_equal_fresh_loads_on_a_real_build: skipped: MCBENCH_DRAW_BUILD does not name a build folder (blueprint.bin, job.json)");
        return;
    };
    let build = std::path::PathBuf::from(build);
    let assets = Assets::load(&Paths::of_data(&data).unwrap()).unwrap();
    let gpu = Gpu::open(&common::choice()).unwrap();
    let renderer = Renderer::new(&gpu, &assets.atlas, COLOR);
    let job: serde_json::Value =
        serde_json::from_slice(&std::fs::read(build.join("job.json")).unwrap()).unwrap();
    let y_offset = job["yOffset"].as_i64().unwrap() as i32;
    let shots: Vec<Shot> = job["shots"]
        .as_array()
        .unwrap()
        .iter()
        .take(2)
        .map(|s| Shot {
            width: s["width"].as_u64().unwrap() as u32 / 2,
            height: s["height"].as_u64().unwrap() as u32 / 2,
            camera: serde_json::from_value(s["camera"].clone()).unwrap(),
            clip: None,
            grid: None,
            _described: None,
            _tick: None,
        })
        .collect();
    let base = std::fs::read(build.join("blueprint.bin")).unwrap();
    let blueprints = mcbench_draw::blueprint::edits_for_tests(&base, 0x5eed, 8);
    let (meshed, total) = check(&assets, &gpu, &renderer, &blueprints, y_offset, &shots);
    eprintln!(
        "{}: {meshed} tiles meshed by 8 edits, of {total}",
        build.display()
    );
}

// gpu-mesh: a scene keeps its quads on the GPU and only their counts and
// digests on the CPU; the counts hold through a tile replaced and a tile
// removed (its last block cleared).
#[test]
fn a_scene_keeps_counts_through_replacement_and_removal() {
    let assets = Assets::load(&synthetic::paths(
        &common::crate_dir().join("tests/synthetic"),
    ))
    .unwrap();
    let gpu = Gpu::open(&common::choice()).unwrap();
    let mut blocks = synthetic::world();
    blocks.push(([200, 64, 200], 1));
    let first = synthetic::blueprint(&blocks);
    let (scene, _) =
        store::make_scene(&assets, &gpu, &kept(first.clone(), 64), None, &mut |_| {}).unwrap();
    let far = scene.tiles.len();
    blocks.pop();
    blocks.push(([0, 66, 0], 3));
    let second = synthetic::blueprint(&blocks);
    let (patched, _, meshed) = store::patch_scene(
        &assets,
        &gpu,
        scene,
        &first,
        &kept(second.clone(), 64),
        &mut |_| {},
    )
    .unwrap();
    let (fresh, _) =
        store::make_scene(&assets, &gpu, &kept(second, 64), None, &mut |_| {}).unwrap();
    assert!(patched.tiles.len() < far, "the far block's tiles are gone");
    assert!(meshed > 0 && meshed <= patched.tiles.len());
    assert_eq!(patched.quads(), fresh.quads());
    assert_eq!(patched.bytes(), fresh.bytes());
    assert_eq!(patched.digests(), fresh.digests());
}
