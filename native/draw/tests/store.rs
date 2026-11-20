// The store's keep-or-evict decisions in the app's own process (as
// live::Draw makes them: Store::in_process), on the synthetic pack: they
// rest on what the store counts of its worlds, scenes and picture targets,
// so memory the process holds for anything else lets nothing go.

mod common;

use mcbench_draw::assets::Assets;
use mcbench_draw::gpu::{Gpu, Renderer, COLOR};
use mcbench_draw::picture::Font;
use mcbench_draw::store::{self, Limits, Room, Shot, Store, Upload};
use mcbench_draw::synthetic;
use std::sync::Arc;

#[test]
fn an_in_process_store_keeps_its_worlds_beside_2_gib_of_the_process_s_own() {
    let gpu = Gpu::open(&common::choice()).unwrap();
    let mut paths = synthetic::paths(&common::crate_dir().join("tests/synthetic"));
    paths.software_vulkan = common::software_vulkan();
    let assets = Assets::load(&paths).unwrap();
    let renderer = Renderer::new(&gpu, &assets.atlas, COLOR);
    let font = Arc::new(Font::new().unwrap());
    let mut store = Store::new(Limits::new(Some(1 << 30), gpu.report().software));
    store.in_process(&gpu);
    // The app's own memory: 2 GiB written, more than the limit itself.
    let other = vec![1u8; 2 << 30];
    // The whole worlds' shots, two worlds: within the counts a software
    // adapter keeps (2 worlds, 3 scenes).
    let all: Vec<Shot> = serde_json::from_value(synthetic::shots()).unwrap();
    let shots: Vec<Shot> = all.into_iter().filter(|s| s.clip.is_none()).collect();
    let mut blocks = synthetic::world();
    let names = ["a", "b"];
    for name in names {
        blocks.push(([9, 65, 9], 3));
        store::keep_world(
            &mut store,
            &assets,
            &gpu,
            &renderer,
            Upload {
                name: name.into(),
                y_offset: synthetic::Y_OFFSET,
                blueprint: synthetic::blueprint(&blocks),
                entities: Vec::new(),
                base: None,
                keep_base: false,
            },
            &mut |_| {},
        )
        .unwrap();
    }
    // Every world drawn twice, each call followed by the store's decisions
    // after a reply and the giving back once no request waits, as Draw does.
    for round in 0..2 {
        for name in names {
            let (timings, _, used) = store::draw_shots(
                &mut store,
                &assets,
                &gpu,
                &renderer,
                &font,
                name,
                &shots,
                2,
                Some(90),
                &|| false,
                &mut |_| {},
            )
            .unwrap();
            let room = Room {
                gpu: &gpu,
                renderer: &renderer,
                extra: 0,
            };
            store.shed(used, true, &room);
            store.release(&room).unwrap();
            let made: f64 = timings
                .iter()
                .map(|t| t["timings"]["scene"].as_f64().unwrap())
                .sum();
            assert_eq!(
                made, 0.0,
                "round {round}: {name}'s shots draw the scene it holds"
            );
        }
    }
    let room = Room {
        gpu: &gpu,
        renderer: &renderer,
        extra: 0,
    };
    assert!(
        store.counted(&room) < 1 << 30,
        "the store counts {} bytes",
        store.counted(&room)
    );
    for name in names {
        assert!(store.worlds.contains_key(name), "{name} is kept");
        assert!(
            store.scenes.contains_key(&(name.to_string(), None)),
            "{name}'s scene is kept"
        );
    }
    assert_eq!(std::hint::black_box(&other)[(1 << 30) + 7], 1);
}
