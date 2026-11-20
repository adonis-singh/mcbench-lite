//! What the live world holds in memory without a renderer: a world's
//! engine blueprint read into the stored world (as the store keeps a
//! chat's), its fresh placement in an in-process engine world, the core's
//! index of its blocks (`core::live::blocks::LiveBlocks`) and `ticks`
//! ticks applied to it, with the process's resident memory after each step
//! (`/proc/self/status`, Linux).
//!
//! Usage: `live_memory <tables folder> <blueprint at Minecraft height> <ticks>`

use std::path::PathBuf;

use mcbench_engine::{Engine, EngineWorld};
use mcbench_lite::core::live::blocks::LiveBlocks;

fn memory(step: &str) {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    let field = |name: &str| {
        status
            .lines()
            .find(|line| line.starts_with(name))
            .map_or("?".into(), |line| line[name.len()..].trim().to_string())
    };
    println!(
        "{step}: VmRSS {} VmHWM {}",
        field("VmRSS:"),
        field("VmHWM:")
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [tables, blueprint, ticks] = args.as_slice() else {
        panic!("usage: live_memory <tables folder> <blueprint> <ticks>");
    };
    let (tables, ticks) = (PathBuf::from(tables), ticks.parse::<u32>().expect("ticks"));
    let bytes = std::fs::read(blueprint).expect("the blueprint");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("a runtime");
    runtime.block_on(async move {
        memory("start");
        let engine = Engine::start(&tables).expect("the engine");
        let facts = mcbench_world::facts::MinecraftFacts::new(
            &std::fs::read_to_string(tables.join("blocks.txt")).expect("blocks.txt"),
        )
        .expect("the facts");
        let y = mcbench_world::dimension::y_offset();
        let world = mcbench_world::blueprint::blueprint_world(&bytes, y, None).expect("the world");
        drop(bytes);
        println!("blocks: {}", world.size());
        memory("stored world read");
        let commands = world
            .place_commands(None, y, None, &facts)
            .expect("the import");
        memory("import made");
        let live = EngineWorld::new(engine).expect("an engine world");
        let started = std::time::Instant::now();
        let placed = live.place(&commands, true).await.expect("the placement");
        println!(
            "placed in {:?}, blueprint {} bytes",
            started.elapsed(),
            placed.0.len()
        );
        drop(commands);
        let inputs = live.base_inputs().await.expect("the inputs");
        let mut blocks = LiveBlocks::load(&placed.0, &inputs).expect("the index");
        drop(placed);
        memory("placed, the import and blueprint let go");
        let started = std::time::Instant::now();
        for _ in 0..ticks {
            let ticked = live.tick(1, true).await.expect("a tick");
            blocks
                .apply(&ticked.changes, &ticked.inputs)
                .expect("a patch");
        }
        println!("{ticks} ticks in {:?}", started.elapsed());
        memory("ticked");
        live.stop().await;
        drop(world);
    });
}
