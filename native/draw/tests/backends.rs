// The same picture across adapters, on the synthetic pack, before the JPEG
// and on the path models get (JPEG at quality 90, decoded): the machine's
// hardware adapter against its software floor (forced). Linux needs
// MCBENCH_DRAW_SWIFTSHADER; a machine with no software floor (macOS), or
// whose only adapter is the floor (the Windows VM's WARP), has nothing to
// compare and says so.

mod common;

use mcbench_draw::assets::Assets;
use mcbench_draw::compare::{self, Image};
use mcbench_draw::gpu::{Choice, Gpu, Renderer, COLOR};
use mcbench_draw::picture::{self, Font};
use mcbench_draw::store::{self, Kept, Shot};
use mcbench_draw::synthetic;

fn pictures(choice: &Choice, assets: &Assets, shots: &[Shot]) -> (String, Vec<Vec<u8>>) {
    let gpu = Gpu::open(choice).unwrap();
    let renderer = Renderer::new(&gpu, &assets.atlas, COLOR);
    let font = Font::new().unwrap();
    let kept = Kept { blueprint: synthetic::blueprint(&synthetic::world()), y_offset: synthetic::Y_OFFSET, entities: Vec::new(), used: 0 };
    let out = shots
        .iter()
        .map(|shot| {
            let (scene, _) = store::make_scene(assets, &gpu, &kept, shot.clip, &mut || {}).unwrap();
            let (rgba, _) = store::draw_shot(assets, &gpu, &renderer, &scene, shot, 2).unwrap();
            picture::finish(rgba, shot.width, shot.height, shot.grid.as_ref(), &font)
        })
        .collect();
    (gpu.report().name, out)
}

#[test]
fn hardware_and_software_adapters_draw_the_same_pictures_and_jpegs() {
    if cfg!(target_os = "macos") {
        eprintln!("hardware_and_software_adapters_draw_the_same_pictures_and_jpegs: skipped: macOS has no software floor (Metal or nothing)");
        return;
    }
    let software_vulkan = common::software_vulkan();
    if cfg!(target_os = "linux") && software_vulkan.is_none() {
        eprintln!("hardware_and_software_adapters_draw_the_same_pictures_and_jpegs: skipped: MCBENCH_DRAW_SWIFTSHADER does not name the software floor");
        return;
    }
    let hardware = Gpu::open(&Choice { software: false, software_vulkan: software_vulkan.clone() }).unwrap();
    if hardware.report().software {
        eprintln!("hardware_and_software_adapters_draw_the_same_pictures_and_jpegs: skipped: the only adapter is the software floor ({})", hardware.report().name);
        return;
    }
    drop(hardware);
    let assets = Assets::load(&synthetic::paths(&common::crate_dir().join("tests/synthetic"))).unwrap();
    let shots: Vec<Shot> = serde_json::from_value(synthetic::shots()).unwrap();
    let (hw, a) = pictures(&Choice { software: false, software_vulkan: software_vulkan.clone() }, &assets, &shots);
    let (sw, b) = pictures(&Choice { software: true, software_vulkan }, &assets, &shots);
    for (i, ((x, y), shot)) in a.iter().zip(&b).zip(&shots).enumerate() {
        let (ix, iy) = (Image::from_rgb(x, shot.width, shot.height), Image::from_rgb(y, shot.width, shot.height));
        let before = compare::colour(&ix, &iy, None).unwrap();
        assert!(before.across_backends(), "shot {i}, {hw} against {sw}, before the JPEG: {before:?}");
        let jx = Image::decode(&picture::jpeg(x, shot.width, shot.height, 90).unwrap()).unwrap();
        let jy = Image::decode(&picture::jpeg(y, shot.width, shot.height, 90).unwrap()).unwrap();
        let after = compare::colour(&jx, &jy, None).unwrap();
        eprintln!("shot {i}, {hw} against {sw}: before the JPEG {before:?}; decoded JPEGs {after:?}");
        assert!(after.across_backends_jpeg(), "shot {i}, {hw} against {sw}, decoded JPEGs: {after:?}");
    }
}
