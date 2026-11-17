// The synthetic pack (tests/synthetic, made by src/synthetic.rs): pictures
// on whatever adapter the machine has against the committed reference
// pictures (drawn on the Mac's Metal) at the across-backends thresholds,
// and its quads against the committed quads as multisets. No Minecraft
// data is needed. MCBENCH_DRAW_WRITE_REFERENCES=1 writes the references
// instead (on the Mac, after a deliberate change to the pictures).

mod common;

use mcbench_draw::assets::Assets;
use mcbench_draw::compare::{self, Image};
use mcbench_draw::gpu::{Gpu, Renderer, COLOR};
use mcbench_draw::picture::{self, Font};
use mcbench_draw::scene::{Draw, Quad};
use mcbench_draw::store::{self, Kept, Shot};
use mcbench_draw::synthetic;
use mcbench_renderer::world::{Mask, World};

fn folder() -> std::path::PathBuf {
    common::crate_dir().join("tests/synthetic")
}

fn writing() -> bool {
    std::env::var("MCBENCH_DRAW_WRITE_REFERENCES").is_ok_and(|v| v == "1")
}

#[test]
fn committed_files_are_what_the_generator_writes() {
    for (name, bytes) in synthetic::files() {
        let path = folder().join(name);
        if writing() {
            std::fs::create_dir_all(folder()).unwrap();
            std::fs::write(&path, &bytes).unwrap();
        }
        assert_eq!(std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())), bytes, "{name} differs from what src/synthetic.rs writes (mcbench-draw synthetic tests/synthetic)");
    }
}

#[test]
fn quads_are_the_committed_multiset() {
    let assets = Assets::load(&synthetic::paths(&folder())).unwrap();
    let world = World::read(&assets.pack, &synthetic::blueprint(&synthetic::world()), Mask { min_x: -1000, max_x: 1000, min_z: -1000, max_z: 1000 }).unwrap();
    let draw = Draw { pack: &assets.pack, facts: &assets.facts, y_offset: synthetic::Y_OFFSET };
    let quads = draw.quads(&world);
    let path = folder().join("reference/quads.json");
    if writing() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&quads).unwrap()).unwrap();
    }
    let reference: Vec<Quad> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let g = compare::geometry(&quads, &assets.names, &reference, &assets.names);
    assert!(g.equal(), "{g:?}");
    // What the world is made to show: faces between the floor's stones are
    // culled, the sign is a block entity quad, the lamps and torches emit.
    assert!(quads.iter().any(|q| q.kind == mcbench_renderer::tile::ENTITY));
    assert!(quads.iter().any(|q| q.emissive && q.shade == [1.15; 4]));
    assert!(quads.iter().any(|q| q.transparent));
    assert!(quads.len() < 21 * 21 * 6);
    // Fluids as Minecraft draws them: side faces from both sides (a face and
    // its reverse at the same place, so back-face culling draws one of the
    // two from any side and nothing fights for depth), and the overlay
    // texture where water meets glass.
    let fluid: Vec<&Quad> = quads.iter().filter(|q| q.kind == mcbench_renderer::tile::FLUID).collect();
    assert!(fluid.iter().all(|q| q.transparent || assets.names[q.texture as usize].contains("lava")));
    let reversed = |a: &Quad, b: &Quad| {
        let mut ca: Vec<[i64; 3]> = a.corners.iter().map(|c| c.map(|v| (v * 4096.0).round() as i64)).collect();
        let mut cb: Vec<[i64; 3]> = b.corners.iter().map(|c| c.map(|v| (v * 4096.0).round() as i64)).collect();
        ca.sort();
        cb.sort();
        ca == cb && (0..3).all(|i| (a.normal[i] + b.normal[i]).abs() < 1e-4)
    };
    let pairs = fluid.iter().filter(|a| fluid.iter().any(|b| reversed(a, b))).count();
    assert!(pairs > 0 && pairs % 2 == 0, "fluid faces drawn from both sides: {pairs}");
    assert!(fluid.iter().any(|q| assets.names[q.texture as usize] == "test:block/water_overlay"), "the overlay beside glass");
    assert!(fluid.iter().filter(|q| assets.names[q.texture as usize] == "test:block/water_overlay").all(|a| !fluid.iter().any(|b| reversed(a, b))), "an overlay face is drawn from one side");
}

#[test]
fn pictures_match_the_references() {
    let assets = Assets::load(&synthetic::paths(&folder())).unwrap();
    let gpu = Gpu::open(&common::choice()).unwrap();
    let renderer = Renderer::new(&gpu, &assets.atlas, COLOR);
    let font = Font::new().unwrap();
    let kept = Kept { blueprint: synthetic::blueprint(&synthetic::world()), y_offset: synthetic::Y_OFFSET, entities: Vec::new(), used: 0 };
    let shots: Vec<Shot> = serde_json::from_value(synthetic::shots()).unwrap();
    let report = gpu.report();
    for (i, shot) in shots.iter().enumerate() {
        let (scene, _) = store::make_scene(&assets, &gpu, &kept, shot.clip, &mut || {}).unwrap();
        let (rgba, _) = store::draw_shot(&assets, &gpu, &renderer, &scene, shot, 2).unwrap();
        let rgb = picture::finish(rgba, shot.width, shot.height, shot.grid.as_ref(), &font);
        let path = folder().join(format!("reference/{i}.png"));
        if writing() {
            image::save_buffer(&path, &rgb, shot.width, shot.height, image::ExtendedColorType::Rgb8).unwrap();
        }
        let ours = Image::from_rgb(&rgb, shot.width, shot.height);
        let reference = Image::open(&path).unwrap();
        let stats = compare::colour(&reference, &ours, None).unwrap();
        eprintln!("shot {i} on {} ({}): {stats:?}", report.name, report.backend);
        assert!(stats.across_backends(), "shot {i} on {} ({}): {stats:?}", report.name, report.backend);
        // The JPEG holds the picture.
        let jpeg = picture::jpeg(&rgb, shot.width, shot.height, 90).unwrap();
        let decoded = Image::decode(&jpeg).unwrap();
        assert!(compare::colour(&ours, &decoded, None).unwrap().mean_abs < 3.0);
    }
}
