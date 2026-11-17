// mcbench-draw serve
//   the helper (PROTOCOL.md)
// mcbench-draw shot <data folder | assets.json> <build folder> <out folder> [--repeat N] [--software] [--software-vulkan <lib>] [--phases]
//   draws every shot of <build folder>/job.json (the renderer spike's
//   compare/today.js format: yOffset and resolved shots) over
//   <build folder>/blueprint.bin (and entities.json beside it, if there),
//   writes <out>/<i>.png (the read-back RGBA), <i>.jpg and times.json
// mcbench-draw compare <a> <b> [--interior-from <surfaces>]
//   colour statistics and SSIM of two pictures (PNG or JPEG), as JSON
// mcbench-draw synthetic <folder>
//   writes the synthetic pack and its assets (tests/synthetic)

use mcbench_draw::assets::{Assets, Paths};
use mcbench_draw::compare::{colour, Image};
use mcbench_draw::gpu::{Choice, Gpu, Renderer, COLOR};
use mcbench_draw::picture::{self, Font};
use mcbench_draw::store::{self, Kept, Shot};
use mcbench_draw::{round2, serve};
use serde_json::{json, Value};
use std::path::PathBuf;

// On macOS, mimalloc: it returns freed memory to the system, where
// macOS's malloc kept a large world's freed meshing memory charged to the
// process (footprint 2,441 MB idle after a 3 million block world, 297 MB
// with mimalloc). On Linux glibc's malloc did better than mimalloc
// (resident 666 against 888 MB after the same world), so it stays.
#[cfg(target_os = "macos")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("serve") if args.len() == 1 => serve::run(),
        Some("shot") => shot(&args[1..]),
        Some("compare") => compare(&args[1..]),
        Some("synthetic") => args.get(1).ok_or_else(|| "usage: mcbench-draw synthetic <folder>".to_string()).and_then(|f| mcbench_draw::synthetic::write(&PathBuf::from(f))),
        _ => Err("usage: mcbench-draw serve | shot <data> <build> <out> [--repeat N] [--software] [--software-vulkan <lib>] [--phases] | compare <a> <b> | synthetic <folder>".into()),
    };
    if let Err(e) = result {
        serve::log(&e);
        std::process::exit(1);
    }
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn shot(args: &[String]) -> Result<(), String> {
    let [data, build, out, ..] = args else { return Err("usage: mcbench-draw shot <data> <build> <out> [--repeat N] [--software] [--software-vulkan <lib>] [--phases]".into()) };
    let repeat: usize = option(args, "--repeat").map(|r| r.parse().map_err(|_| "--repeat N")).transpose()?.unwrap_or(0);
    let choice = Choice { software: args.iter().any(|a| a == "--software"), software_vulkan: option(args, "--software-vulkan").map(PathBuf::from) };
    // The first pictures' work submitted one part at a time, each timed.
    let phases = args.iter().any(|a| a == "--phases");
    let (build, out) = (PathBuf::from(build), PathBuf::from(out));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let job: Value = serde_json::from_slice(&std::fs::read(build.join("job.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    let t_start = mcbench_draw::now_ms();
    let t = mcbench_draw::now_ms();
    let gpu = Gpu::open(&choice)?;
    let device_ms = mcbench_draw::now_ms() - t;
    let report = gpu.report();
    let t = mcbench_draw::now_ms();
    // A data folder of today's bake, or a JSON file of hello's assets.
    let paths: Paths = if data.ends_with(".json") { serde_json::from_slice(&std::fs::read(data).map_err(|e| format!("{data}: {e}"))?).map_err(|e| format!("{data}: {e}"))? } else { Paths::of_data(&PathBuf::from(data))? };
    let assets = Assets::load(&paths)?;
    let assets_ms = mcbench_draw::now_ms() - t;
    let t = mcbench_draw::now_ms();
    let renderer = Renderer::new(&gpu, &assets.atlas, COLOR);
    gpu.wait().map_err(|e| e.0)?;
    let pipelines_ms = mcbench_draw::now_ms() - t;
    let font = Font::new()?;
    let entities: Vec<Value> = match std::fs::read(build.join("entities.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        Err(_) => Vec::new(),
    };
    let kept = Kept { blueprint: std::fs::read(build.join("blueprint.bin")).map_err(|e| e.to_string())?, y_offset: job["yOffset"].as_i64().ok_or("job.json has no yOffset")? as i32, entities, used: 0 };
    let shots: Vec<(Shot, u32)> = job["shots"]
        .as_array()
        .ok_or("job.json has no shots")?
        .iter()
        .map(|s| {
            let shot = Shot { width: s["width"].as_u64().unwrap_or(0) as u32, height: s["height"].as_u64().unwrap_or(0) as u32, camera: serde_json::from_value(s["camera"].clone()).map_err(|e| e.to_string())?, clip: serde_json::from_value(s["clip"].clone()).map_err(|e| e.to_string())?, grid: serde_json::from_value(s["grid"].clone()).map_err(|e| e.to_string())?, _described: None, _tick: None };
            Ok((shot, s["supersample"].as_u64().unwrap_or(2) as u32))
        })
        .collect::<Result<_, String>>()?;
    let mut scenes: std::collections::HashMap<Option<store::Clip>, (store::Scene, store::SceneTimes)> = Default::default();
    let draw = |i: usize, write: bool, scenes: &mut std::collections::HashMap<Option<store::Clip>, (store::Scene, store::SceneTimes)>| -> Result<Value, String> {
        let (shot, ss) = &shots[i];
        let t0 = mcbench_draw::now_ms();
        let mut scene_ms = 0.0;
        if let std::collections::hash_map::Entry::Vacant(slot) = scenes.entry(shot.clip) {
            let made = store::make_scene(&assets, &gpu, &kept, shot.clip, &mut || {}).map_err(|e| format!("{e:?}"))?;
            scene_ms = made.1.total;
            slot.insert(made);
        }
        let (scene, _) = &scenes[&shot.clip];
        let (rgba, times, phases) = if phases && write {
            let (rgba, phases) = store::draw_shot_phases(&assets, &gpu, &renderer, scene, shot, *ss).map_err(|e| format!("{e:?}"))?;
            (rgba, Default::default(), Some(phases))
        } else {
            let (rgba, times) = store::draw_shot(&assets, &gpu, &renderer, scene, shot, *ss).map_err(|e| format!("{e:?}"))?;
            (rgba, times, None)
        };
        let t = mcbench_draw::now_ms();
        let png = write.then(|| rgba.clone());
        let rgb = picture::finish(rgba, shot.width, shot.height, shot.grid.as_ref(), &font);
        let grid_ms = mcbench_draw::now_ms() - t;
        let t = mcbench_draw::now_ms();
        let bytes = picture::jpeg(&rgb, shot.width, shot.height, 90)?;
        let encode_ms = mcbench_draw::now_ms() - t;
        if write {
            image::save_buffer(out.join(format!("{i}.png")), &png.unwrap(), shot.width, shot.height, image::ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
            std::fs::write(out.join(format!("{i}.jpg")), &bytes).map_err(|e| e.to_string())?;
        }
        Ok(json!({ "shot": i, "phases": phases.map(|p| p.into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<serde_json::Map<String, Value>>()), "scene": round2(scene_ms), "draw": round2(times.draw_ms), "readback": round2(times.readback_ms), "grid": round2(grid_ms), "encode": round2(encode_ms), "total": round2(mcbench_draw::now_ms() - t0), "bytes": bytes.len() }))
    };
    let mut cold = Vec::new();
    for i in 0..shots.len() {
        cold.push(draw(i, true, &mut scenes)?);
    }
    let cold_ms = mcbench_draw::now_ms() - t_start;
    let mut warm = Vec::new();
    for _ in 0..repeat {
        for i in 0..shots.len() {
            warm.push(draw(i, false, &mut scenes)?);
        }
    }
    let first = scenes.get(&None).map(|(s, t)| json!({ "quads": s.quads(), "entityQuads": s.entity_quads, "tiles": s.tiles.len(), "blocks": s.blocks_count, "bytes": s.bytes(), "missing": s.missing(), "times": t }));
    let report = json!({
        "adapter": report,
        "device_ms": round2(device_ms), "assets_ms": round2(assets_ms), "pipelines_ms": round2(pipelines_ms),
        "atlas_bytes": renderer.atlas_bytes(),
        "scene": first,
        "cold": cold, "cold_ms": round2(cold_ms), "warm": warm,
    });
    std::fs::write(out.join("times.json"), serde_json::to_string_pretty(&report).unwrap()).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string(&json!({ "adapter": report["adapter"]["name"], "cold_ms": report["cold_ms"], "scene": report["scene"], "cold": report["cold"] })).unwrap());
    Ok(())
}

fn compare(args: &[String]) -> Result<(), String> {
    let [a, b] = args else { return Err("usage: mcbench-draw compare <a> <b>".into()) };
    let (a, b) = (Image::open(&PathBuf::from(a))?, Image::open(&PathBuf::from(b))?);
    let stats = colour(&a, &b, None)?;
    println!("{}", serde_json::to_string(&json!({ "stats": stats, "same": stats.same(), "acrossBackends": stats.across_backends(), "acrossBackendsJpeg": stats.across_backends_jpeg() })).unwrap());
    Ok(())
}
