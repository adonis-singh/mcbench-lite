// The live view's renderer alone, as the app runs it: the device, the
// assets, then `live::Draw` with a world shown in a pane of a given size,
// the process's resident memory after each step beside the renderer's own
// count of what it holds (live::Accounting). Also prints the first frame's
// camera and writes the first and the settled frame as PNG.
//
//   pane_memory <data folder> <blueprint> <width> <height> <out folder> [min x,y,z max x,y,z]
//
// The data folder is a draw-test data folder (Paths::of_data); the
// blueprint is the world's (yOffset 64); the bounds default to the
// blueprint's own. MCBENCH_DRAW_SOFTWARE=1 uses the software floor
// (MCBENCH_DRAW_SWIFTSHADER names it on Linux).

use mcbench_draw::assets::{Assets, Paths};
use mcbench_draw::gpu::{Choice, Gpu};
use mcbench_draw::live::{Draw, STAGE};
use mcbench_draw::memory;
use mcbench_render::live::{
    CameraInput, DragButton, Frame, Framing, LiveRenderer, PaneSize, Shown, ViewKey,
};
use mcbench_render::{Clip, Renderer, WorldUpload};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

fn mb(b: u64) -> f64 {
    (b as f64 / 1e5).round() / 10.0
}

fn step(name: &str) -> u64 {
    memory::give_back();
    let r = memory::resident();
    println!("{name:<44} resident {:>8.1} MB", mb(r));
    r
}

fn png(frame: &Frame, path: &PathBuf) {
    let mut rgba = frame.bgra().to_vec();
    for p in rgba.as_chunks_mut::<4>().0 {
        p.swap(0, 2);
    }
    image::save_buffer(
        path,
        &rgba,
        frame.size[0],
        frame.size[1],
        image::ExtendedColorType::Rgba8,
    )
    .unwrap();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let data = PathBuf::from(&args[1]);
    let blueprint = std::fs::read(&args[2]).unwrap();
    let (width, height): (u32, u32) = (args[3].parse().unwrap(), args[4].parse().unwrap());
    let out = PathBuf::from(&args[5]);
    std::fs::create_dir_all(&out).unwrap();
    let software = std::env::var("MCBENCH_DRAW_SOFTWARE").is_ok_and(|v| v == "1");
    let choice = Choice {
        software,
        software_vulkan: std::env::var_os("MCBENCH_DRAW_SWIFTSHADER").map(PathBuf::from),
    };
    let paths = Paths::of_data(&data).unwrap();
    let base = step("start");
    // What Draw::open makes, step by step, then let go: the device, the
    // assets, see_build's pipelines with the block atlas, the live view's
    // pipelines and targets' layouts, Minecraft's night.
    let gpu = Gpu::open(&choice).unwrap();
    let device = step("device opened") - base;
    let assets = Assets::load(&paths).unwrap();
    let assets_bytes = step("assets loaded") - (base + device);
    let shots = mcbench_draw::gpu::Renderer::new(&gpu, &assets.atlas, mcbench_draw::gpu::COLOR);
    gpu.wait().unwrap();
    let before_night = step("see_build's pipelines, block atlas uploaded");
    let night =
        mcbench_draw::live::night::read(&paths.night.clone().unwrap(), &paths.client_jar).unwrap();
    let before_view = step("night read");
    let view = mcbench_draw::live::view::View::new(&gpu, &shots, software, &night);
    gpu.wait().unwrap();
    let after_view = step("live view's pipelines");
    println!(
        "  pipelines and atlas {:.1} MB, night {:.1} MB, live view {:.1} MB",
        mb(before_night - base - device - assets_bytes),
        mb(before_view - before_night),
        mb(after_view - before_view)
    );
    drop((view, night, shots, assets, gpu));
    step("all let go");
    let t = Instant::now();
    let draw = Draw::open(paths, choice, 1 << 30, STAGE).unwrap();
    let opened = step("Draw open");
    println!(
        "  adapter {} ({})",
        draw.report().adapter.name,
        draw.report().adapter.backend
    );
    println!(
        "  accounting {}",
        serde_json::to_string(&draw.accounting()).unwrap()
    );
    let report = draw
        .world(WorldUpload {
            name: "live:c".into(),
            y_offset: 64,
            blueprint,
            entities: None,
            base: None,
        })
        .unwrap();
    let kept = step("world kept");
    println!(
        "  {} blocks, {} quads, {} tiles, bounds {:?} to {:?}",
        report.blocks, report.quads, report.tiles, report.bounds.min, report.bounds.max
    );
    println!(
        "  accounting {}",
        serde_json::to_string(&draw.accounting()).unwrap()
    );
    let bounds = match args.get(6) {
        Some(b) => {
            let v: Vec<i32> = b.split(',').map(|n| n.parse().unwrap()).collect();
            Clip {
                min: [v[0], v[1], v[2]],
                max: [v[3], v[4], v[5]],
            }
        }
        None => report.bounds,
    };
    let pane = draw.live();
    let wakes = Arc::new((Mutex::new(0u64), Condvar::new()));
    let w = wakes.clone();
    pane.set_waker(Box::new(move || {
        *w.0.lock().unwrap() += 1;
        w.1.notify_all();
    }));
    let next = |ms: u64| -> Option<Frame> {
        let deadline = Instant::now() + Duration::from_millis(ms);
        let mut n = wakes.0.lock().unwrap();
        loop {
            if let Some(f) = pane.take() {
                return Some(f);
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            n = wakes.1.wait_timeout(n, deadline - now).unwrap().0;
        }
    };
    pane.resize(PaneSize {
        width,
        height,
        scale: 1.0,
    });
    draw.live_show(Some(Shown {
        world: "live:c".into(),
        view: ViewKey("c".into()),
        revision: 1,
        bounds,
        framing: Framing::AtOnce,
    }));
    let first = next(20_000).expect("a first frame");
    println!("  first frame {:?} camera {:?}", first.size, first.camera);
    png(&first, &out.join("first.png"));
    let mut last = first;
    let mut frames = 1;
    while let Some(f) = next(500) {
        last = f;
        frames += 1;
    }
    png(&last, &out.join("settled.png"));
    println!("  settled after {frames} frames, camera {:?}", last.camera);
    let shown = step("shown, frames drawn");
    let accounting = draw.accounting();
    println!(
        "  accounting {}",
        serde_json::to_string(&accounting).unwrap()
    );
    println!(
        "summary: device {:.1} MB, assets {:.1} MB, Draw open {:.1} MB over start, world kept +{:.1} MB, shown +{:.1} MB, resident {:.1} MB; counted {:.1} MB ({:.1} s)",
        mb(device),
        mb(assets_bytes),
        mb(opened - base),
        mb(kept - opened),
        mb(shown - kept),
        mb(shown),
        mb(accounting.counted),
        t.elapsed().as_secs_f64()
    );
    // Tilted toward the horizon with the right button, as far as the
    // distance allows: the sky above the build.
    for _ in 0..4 {
        pane.camera(CameraInput::PointerDown);
        pane.camera(CameraInput::Drag {
            button: DragButton::Right,
            dx: 0.0,
            dy: -400.0,
        });
    }
    let mut level = next(5_000).expect("a frame after the tilt");
    while let Some(f) = next(500) {
        level = f;
    }
    png(&level, &out.join("tilted.png"));
    println!("  tilted, camera {:?}", level.camera);
    draw.shutdown();
    step("shut down");
}
