//! Today's tests whose subject is a tool body, ported: `build-verdict.test.js`
//! (3), the build's settle in `settle.test.js` (5 of 7), `settle-ticks.test.js`
//! (5 of 7, its `see_build` case with a renderer that draws names),
//! `settle-flow.test.js` (1 of 7), `storage-limits.test.js`'s three
//! `test_build` tests, `agent-vision.test.js`'s tool rule (1 of 7) and
//! `schematic.test.js` (5). (`agent-verification.test.js`'s tool halves are
//! the verifier's, in `tests/agent_verification_test.rs`.) Each test names
//! its Node test. Today's tests run on a fake engine whose rules they write;
//! these run on the in-process engine with Minecraft's own rules, so where a
//! fake rule was not Minecraft's (a torch on planks removed, water turned to
//! stone) the case uses what Minecraft does (sand falls) and says so; the
//! `test_build` tests use the verifier's fake engine (`tests/common`). One
//! process: the engine starts once, and the build child is the real
//! `mcbench-build-child` that `MCBENCH_BUILD_CHILD` names
//! (`mcbench_build::child_program`; `native/check.sh` builds and names it).
//!
//! Needs `MCBENCH_TABLES=<core/minecraft/1.21.10>`; without it every test is
//! reported ignored, and with it a missing table fails.

#[path = "../common/mod.rs"]
mod verify_common;

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use mcbench_tools::test_build::TestBuild;
use mcbench_tools::testing::{RecordedSandbox, Shared, registry_from_table, services, shared};
use mcbench_tools::verify::VerifierPool;
use mcbench_tools::{Chat, Services, Tools, Workspace, World};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

struct Context {
    shared: Shared,
    tables: PathBuf,
    root: PathBuf,
    registry: Arc<Value>,
    folders: Mutex<usize>,
}

impl Context {
    /// A fresh data folder's services with `limits` and `verifier` (the
    /// engine's when `None`).
    fn services(
        &self,
        limits: mcbench_store::Limits,
        verifier: Option<VerifierPool>,
    ) -> Arc<Services> {
        let n = {
            let mut folders = self.folders.lock().unwrap();
            *folders += 1;
            *folders
        };
        let data = self.root.join(format!("data-{n}"));
        let verifier = match verifier {
            Some(pool) => Arc::new(pool),
            None => self.shared.verifier(data.join("minecraft-verify")).unwrap(),
        };
        services(
            &self.shared,
            &data,
            limits,
            Arc::new(RecordedSandbox::new()),
            verifier,
            self.registry.clone(),
        )
        .unwrap()
    }

    fn default_services(&self) -> Arc<Services> {
        self.services(limits(&[]), None)
    }
}

fn limits(vars: &[(&str, &str)]) -> mcbench_store::Limits {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    mcbench_store::Limits::from_env(|name| {
        vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
    })
    .unwrap()
}

fn chat(services: &Services, id: &str) -> Chat {
    Chat {
        id: id.to_string(),
        model: "fixture".into(),
        world: World::new(),
        workspace: Workspace {
            chat: id.to_string(),
            host: services.store.folder().root.join("sandbox").join(id),
        },
        image_input: true,
        chat: false,
    }
}

async fn call(services: &Arc<Services>, chat: &mut Chat, name: &str, args: Value) -> Value {
    let tools = Tools::new(services.clone());
    tools
        .execute(
            chat,
            name,
            args.as_object().unwrap(),
            &mut |_| {},
            &CancellationToken::new(),
        )
        .await
        .unwrap()
        .result
}

async fn build(services: &Arc<Services>, chat: &mut Chat, program: &str) -> Value {
    call(
        services,
        chat,
        "build",
        json!({"program": format!("module.exports = mc => {{ {program} }}")}),
    )
    .await
}

async fn build_with(
    services: &Arc<Services>,
    chat: &mut Chat,
    program: &str,
    settle_ticks: Value,
) -> Value {
    call(
        services,
        chat,
        "build",
        json!({"program": program, "settle_ticks": settle_ticks}),
    )
    .await
}

type Case = fn(&'static Context) -> Pin<Box<dyn Future<Output = ()> + Send>>;

macro_rules! cases {
    ($($name:ident),* $(,)?) => { &[$((stringify!($name), |c| Box::pin($name(c)))),*] };
}

// ---- build-verdict.test.js ----

/// "a block Minecraft says cannot survive where it is refuses the build, saying where a full block would hold it"
async fn a_block_that_cannot_survive_refuses_the_build(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "verdict-1");
    let floor = build(
        &s,
        &mut chat,
        "mc.set([0, 1, 0], 'torch'); mc.log('placed');",
    )
    .await;
    assert_eq!(floor["ok"], false);
    assert_eq!(
        floor["error"],
        "Minecraft cannot keep a block of this build where it is (Block.canSurvive: a player could not place it there, and the first update that asks again removes it): torch at [0, 1, 0]: a full block at [0, 0, 0] would hold it"
    );
    assert_eq!(
        floor["note"],
        "program aborted — no blocks or files were changed"
    );
    assert_eq!(floor["logs"], json!(["placed"]));
    assert_eq!(chat.world.size(), 0, "nothing was applied");
    let held = build(&s, &mut chat, "mc.set([0, 0, 0], 'stone'); mc.set([0, 1, 0], 'torch'); mc.set([3, 1, 0], 'stone'); mc.set([3, 1, 1], 'wall_torch[facing=south]');").await;
    assert_eq!(held["ok"], true, "{held}");
    assert_eq!(chat.world.size(), 4);
}

/// "a build that takes away a block's support across a chunk border is refused" (the half where a far unsupported block does not refuse a build needs the fake engine's answer and is not ported)
async fn support_taken_across_a_chunk_border_is_refused(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "verdict-2");
    assert_eq!(
        build(
            &s,
            &mut chat,
            "mc.set([15, 0, 0], 'stone'); mc.set([16, 0, 0], 'wall_torch[facing=east]');"
        )
        .await["ok"],
        true
    );
    let removed = build(&s, &mut chat, "mc.replace('stone', 'air');").await;
    assert_eq!(removed["ok"], false);
    let error = removed["error"].as_str().unwrap();
    assert!(
        error.contains(
            "wall_torch[facing=east] at [16, 0, 0]: a full block at [15, 0, 0] would hold it"
        ),
        "{error}"
    );
    assert_eq!(chat.world.size(), 2);
}

/// "a bed needs its other half one block toward its facing, also across a chunk border"
async fn a_bed_needs_its_other_half(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "verdict-3");
    assert_eq!(build(&s, &mut chat, "mc.set([0, 0, 0], 'red_bed[facing=south,part=foot]'); mc.set([0, 0, 1], 'red_bed[facing=south,part=head]');").await["ok"], true);
    let wrong = build(&s, &mut chat, "mc.set([0, 0, -2], 'red_bed[facing=south,part=head]'); mc.set([0, 0, -1], 'red_bed[facing=south,part=foot]');").await;
    assert_eq!(wrong["ok"], false);
    assert_eq!(
        wrong["error"],
        "Incomplete bed at [0, 0, -2]: red_bed[part=head,facing=south] needs its foot at [0, 0, -3] with the same facing (a bed faces from its foot toward its head)"
    );
    let one = build(&s, &mut chat, "mc.set([5, 0, 5], 'red_bed[facing=east]');").await;
    assert!(
        one["error"]
            .as_str()
            .unwrap()
            .contains("red_bed[part=foot,facing=east] needs its head at [6, 0, 5]"),
        "{one}"
    );
    assert_eq!(build(&s, &mut chat, "mc.set([15, 0, 4], 'red_bed[facing=east,part=foot]'); mc.set([16, 0, 4], 'red_bed[facing=east,part=head]');").await["ok"], true);
}

// ---- settle.test.js ----

/// "a build that changes the world carries Minecraft's verdict, and mc.settled reads it back" (sand falls; the fake's torch and water rules are not Minecraft's)
async fn a_build_carries_minecrafts_verdict_and_mc_settled_reads_it(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "settle-1");
    let first = build(
        &s,
        &mut chat,
        "mc.set([0, 0, 0], 'stone'); mc.set([0, 8, 0], 'sand'); mc.log(mc.settled(0, 0, 0));",
    )
    .await;
    assert_eq!(first["ok"], true, "{first}");
    assert_eq!(
        first["logs"],
        json!(["null"]),
        "the world was empty when the program started"
    );
    assert_eq!(first["minecraft"]["ticks"], 210);
    assert_eq!(
        first["minecraft"]["examples"]["removed"],
        json!([{"at": [0, 8, 0], "was": "minecraft:sand"}])
    );
    assert_eq!(
        first["minecraft"]["examples"]["added"],
        json!([{"at": [0, 1, 0], "now": "minecraft:sand"}])
    );
    assert_eq!(first["minecraft"]["blocks"], 2);
    let second = build(&s, &mut chat, "mc.log(mc.settled(0, 1, 0), mc.settled(0, 8, 0), mc.get([0, 8, 0])); mc.translate(0, 1, 0, () => mc.log(mc.settled(0, 0, 0))); mc.set([0, 1, 0], 'stone'); mc.log(mc.settled(0, 1, 0), mc.settled(9, 0, 9));").await;
    assert_eq!(second["ok"], true, "{second}");
    assert_eq!(
        second["logs"],
        json!([
            "{\"id\":\"minecraft:sand\",\"states\":{}} null {\"id\":\"minecraft:sand\",\"states\":{}}",
            "{\"id\":\"minecraft:sand\",\"states\":{}}",
            "{\"id\":\"minecraft:sand\",\"states\":{}} null"
        ])
    );
    assert_eq!(
        second["minecraft"]["examples"]["added"],
        json!([{"at": [0, 2, 0], "now": "minecraft:sand"}])
    );
}

/// "a world is settled once: builds start from the last settle, an unchanged world is not settled again"
async fn a_world_is_settled_once(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "settle-2");
    let base = build(&s, &mut chat, "mc.set([0, 0, 0], 'stone')").await;
    assert_eq!(base["minecraft"]["removed"], 0);
    let read = build(&s, &mut chat, "mc.log(mc.settled(0, 0, 0).id)").await;
    assert_eq!(read["changed"], false);
    assert!(read.get("minecraft").is_none());
    assert_eq!(read["logs"], json!(["minecraft:stone"]));
    let settled = s
        .settler
        .settle(&chat.id, &mut chat.world, &[210], None)
        .await
        .unwrap();
    assert!(settled[&210].cached);
    assert_eq!(
        settled[&210].value.world.get([0, 0, 0]).unwrap().id,
        "minecraft:stone"
    );
    // A world the conversation did not settle (a revert, a reload) is settled when a build starts from it.
    chat.world = chat.world.reference();
    let again = s
        .settler
        .settle(&chat.id, &mut chat.world.clone(), &[210], None)
        .await
        .unwrap();
    assert!(!again[&210].cached);
}

/// "mc.settled takes three integers and rejects anything else"
async fn mc_settled_takes_three_integers(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "settle-5");
    build(&s, &mut chat, "mc.set([0, 0, 0], 'stone')").await;
    for call in [
        "mc.settled([0, 0, 0])",
        "mc.settled(0, 0)",
        "mc.settled(0.5, 0, 0)",
        "mc.settled(0, 0, 99999)",
    ] {
        let result = build(&s, &mut chat, &format!("{call};")).await;
        assert_eq!(result["ok"], false, "{call}");
    }
    let tuple = build(&s, &mut chat, "mc.settled([0, 0, 0])").await;
    assert!(
        tuple["error"]
            .as_str()
            .unwrap()
            .contains("mc.settled takes three integers, mc.settled(x, y, z), not"),
        "{tuple}"
    );
}

/// "two requests for one world share one engine run, and only the latest conversations stay settled"
async fn two_requests_share_a_run_and_the_latest_conversations_stay(c: &'static Context) {
    let s = c.default_services();
    let mut world = World::new();
    world
        .set(
            [0, 0, 0],
            mcbench_world::BlockState::new("minecraft:stone", &[]),
        )
        .unwrap();
    world.finalize();
    let (mut a, mut b) = (world.clone(), world.clone());
    let (one, two) = tokio::join!(
        s.settler.settle("shared", &mut a, &[100], None),
        s.settler.settle("shared", &mut b, &[100], None)
    );
    let (one, two) = (one.unwrap(), two.unwrap());
    assert!(Arc::ptr_eq(&one[&100].value, &two[&100].value));
    assert_eq!((one[&100].cached, two[&100].cached), (false, true));
    for i in 0..16 {
        s.settler
            .settle(&format!("other-{i}"), &mut world.clone(), &[100], None)
            .await
            .unwrap();
    }
    assert!(
        s.settler
            .settle("other-15", &mut world.clone(), &[100], None)
            .await
            .unwrap()[&100]
            .cached
    );
    assert!(
        !s.settler
            .settle("shared", &mut world.clone(), &[100], None)
            .await
            .unwrap()[&100]
            .cached,
        "the oldest conversation was let go"
    );
}

/// "the changed block ids are those a cell-by-cell comparison finds, in the same order"
async fn changed_ids_are_a_cell_by_cell_comparison(_: &'static Context) {
    let mut state: u32 = 99;
    let mut random = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        f64::from(state) / 2f64.powi(32)
    };
    let names = ["stone", "dirt", "sand", "water", "torch", "glass"];
    for round in 0..5 {
        let mut authored = World::new();
        let mut settled = World::new();
        let order: Vec<&str> = if round % 2 == 1 {
            names.to_vec()
        } else {
            names.iter().rev().copied().collect()
        };
        for name in order {
            settled
                .palette()
                .intern(&format!("minecraft:{name}"), &[])
                .unwrap();
        }
        for _ in 0..4000 {
            let cell = [
                (random() * 40.0).floor() as i32 - 8,
                (random() * 20.0).floor() as i32,
                (random() * 40.0).floor() as i32,
            ];
            let roll = random();
            if roll < 0.5 {
                let name = names[(random() * names.len() as f64).floor() as usize];
                authored
                    .set(
                        cell,
                        mcbench_world::BlockState::new(&format!("minecraft:{name}"), &[]),
                    )
                    .unwrap();
            }
            if roll > 0.35 {
                let name = names[(random() * names.len() as f64).floor() as usize];
                settled
                    .set(
                        cell,
                        mcbench_world::BlockState::new(&format!("minecraft:{name}"), &[]),
                    )
                    .unwrap();
            }
        }
        // The reference: every cell either world holds, in the chunks' order then by index.
        let mut keys = indexmap::IndexSet::<u32>::new();
        keys.extend(authored.chunks().map(|(k, _)| k));
        keys.extend(settled.chunks().map(|(k, _)| k));
        let (mut counts, mut examples) = ([0u64; 3], [Vec::new(), Vec::new(), Vec::new()]);
        for key in keys {
            let [cx, cy, cz] = mcbench_world::coords_of(key);
            for index in 0..4096usize {
                let at = [
                    cx * 16 + (index & 15) as i32,
                    cy * 16 + (index >> 8) as i32,
                    cz * 16 + ((index >> 4) & 15) as i32,
                ];
                let was = authored.get(at).map(|b| b.id.clone());
                let now = settled.get(at).map(|b| b.id.clone());
                if was == now {
                    continue;
                }
                let kind = if now.is_none() {
                    0
                } else if was.is_none() {
                    1
                } else {
                    2
                };
                counts[kind] += 1;
                if examples[kind].len() < 8 {
                    examples[kind].push(match kind {
                        0 => json!({"at": at, "was": was}),
                        1 => json!({"at": at, "now": now}),
                        _ => json!({"at": at, "was": was, "now": now}),
                    });
                }
            }
        }
        let changes = mcbench_tools::settle::id_changes(&authored, &settled);
        assert_eq!([changes.removed, changes.added, changes.replaced], counts);
        assert_eq!(
            changes.examples,
            json!({"removed": examples[0], "added": examples[1], "replaced": examples[2]})
        );
    }
}

// ---- settle-ticks.test.js ----

const TOWER: &str = "module.exports = mc => { mc.fill([0, 0, 0], [4, 0, 4], \"stone\"); mc.fill([2, 12, 2], [2, 14, 2], \"sand\"); };";

fn sand_at(world: &World) -> Vec<i32> {
    let mut found = Vec::new();
    world.for_each(None, |[_, y, _], block| {
        if block.id == "minecraft:sand" {
            found.push(y);
        }
    });
    found.sort();
    found
}

/// "a build settles for 210 ticks unless it asks for another number, and says how many" (the midway case at 5 ticks, where Minecraft's sand is a falling entity, not a block, is left out)
async fn a_build_settles_for_210_ticks_unless_asked(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "ticks-1");
    let first = build_with(&s, &mut chat, TOWER, Value::Null).await;
    assert_eq!(first["ok"], false, "settle_ticks: null is refused");
    let first = call(&s, &mut chat, "build", json!({"program": TOWER})).await;
    assert_eq!(first["minecraft"]["ticks"], 210);
    let settled = s
        .settler
        .settle(&chat.id, &mut chat.world, &[210], None)
        .await
        .unwrap();
    assert_eq!(sand_at(&settled[&210].value.world), vec![1, 2, 3]);
    let mut placed = mcbench_tools::Chat {
        id: "ticks-1b".into(),
        ..chat
    };
    placed.world = World::new();
    let as_placed = build_with(&s, &mut placed, TOWER, json!(0)).await;
    assert_eq!(as_placed["minecraft"]["ticks"], 0);
    assert_eq!(
        (
            as_placed["minecraft"]["removed"].clone(),
            as_placed["minecraft"]["added"].clone()
        ),
        (json!(0), json!(0))
    );
    let settled = s
        .settler
        .settle(&placed.id, &mut placed.world, &[0], None)
        .await
        .unwrap();
    assert_eq!(sand_at(&settled[&0].value.world), vec![12, 13, 14]);
}

/// "a number of ticks beyond test_build's budget, or not a whole number, is refused before anything runs"
async fn ticks_beyond_the_budget_are_refused(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "ticks-2");
    let max = s.budgets.simulation_ticks;
    for ticks in [json!(max + 1), json!(-1), json!(2.5), json!("40")] {
        let result = build_with(&s, &mut chat, TOWER, ticks.clone()).await;
        assert_eq!(result["ok"], false, "{ticks}");
        assert!(result["error"].as_str().unwrap().contains("settle_ticks"));
        assert_eq!(
            result["note"],
            "program not run — no blocks or files were changed"
        );
    }
    let over = build_with(&s, &mut chat, TOWER, json!(max + 1)).await;
    assert!(over["error"].as_str().unwrap().contains(&format!("the budget of simulated ticks, the same as test_build's, is {max} (MCBENCH_VERIFY_MAX_SIMULATION_TICKS)")));
    assert_eq!(chat.world.size(), 0);
    let edge = build_with(
        &s,
        &mut chat,
        "module.exports = mc => mc.set([0, 0, 0], \"stone\")",
        json!(3),
    )
    .await;
    assert_eq!(edge["ok"], true);
}

/// "mc.settled reads the settle the world has whatever its ticks, and mc.settledTicks says which" (ticks 0 and 60 in place of 4 and 60, where the sand is a block)
async fn mc_settled_reads_the_worlds_settle_and_its_ticks(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "ticks-3");
    let empty = build(
        &s,
        &mut chat,
        "mc.log(mc.settledTicks); mc.set([0, 0, 0], 'stone');",
    )
    .await;
    assert_eq!(
        empty["logs"],
        json!(["210"]),
        "a world nothing has settled starts from the default"
    );
    build_with(&s, &mut chat, TOWER, json!(0)).await;
    let read = build_with(&s, &mut chat, "module.exports = mc => { mc.log(mc.settledTicks, mc.settled(2, 12, 2)?.id, mc.settled(2, 1, 2)?.id ?? \"air\"); mc.set([6, 0, 6], \"stone\"); }", json!(60)).await;
    assert_eq!(read["logs"], json!(["0 minecraft:sand air"]));
    assert_eq!(read["minecraft"]["ticks"], 60);
    let next = build(
        &s,
        &mut chat,
        "mc.log(mc.settledTicks, mc.settled(2, 1, 2)?.id, mc.settled(2, 12, 2)?.id ?? 'air')",
    )
    .await;
    assert_eq!(next["logs"], json!(["60 minecraft:sand air"]));
    assert_eq!(mcbench_tools::settle::current_ticks(&chat.world), 60);
    // A reloaded conversation keeps the ticks its last build asked.
    let manifest = mcbench_tools::testing::manifest_of(&chat.world);
    let chunks: std::collections::HashMap<String, Vec<u8>> = {
        let mut out = std::collections::HashMap::new();
        chat.world
            .persist(&mut |hash: &str, bytes: &[u8]| {
                out.insert(hash.to_string(), bytes.to_vec());
                Ok(())
            })
            .unwrap();
        for (_, chunk) in chat.world.chunks() {
            let payload = mcbench_world::world::chunk_payload(&chunk.pairs());
            out.insert(
                mcbench_world::world::chunk_hash(&payload),
                payload.into_bytes(),
            );
        }
        out
    };
    chat.world = World::from_manifest(
        &mcbench_world::WorldManifest(manifest),
        &mut |hash: &str| Ok(chunks[hash].clone()),
    )
    .unwrap();
    let reloaded = build(&s, &mut chat, "mc.log(mc.settledTicks)").await;
    assert_eq!(reloaded["logs"], json!(["60"]));
}

/// "settles are kept by conversation, world, revision and ticks"
async fn settles_are_kept_by_conversation_world_revision_and_ticks(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "ticks-4");
    build_with(&s, &mut chat, TOWER, json!(3)).await;
    let settler = &s.settler;
    assert!(
        settler
            .settle(&chat.id, &mut chat.world, &[3], None)
            .await
            .unwrap()[&3]
            .cached
    );
    let more = settler
        .settle(&chat.id, &mut chat.world, &[3, 9], None)
        .await
        .unwrap();
    assert_eq!((more[&3].cached, more[&9].cached), (true, false));
    assert!(
        settler
            .settle(&chat.id, &mut chat.world, &[9], None)
            .await
            .unwrap()[&9]
            .cached
    );
    assert!(
        !settler
            .settle("ticks-4-other", &mut chat.world, &[3], None)
            .await
            .unwrap()[&3]
            .cached,
        "another conversation is its own settle"
    );
    let mut copy = chat.world.reference();
    assert!(
        !settler
            .settle(&chat.id, &mut copy, &[3], None)
            .await
            .unwrap()[&3]
            .cached
    );
    assert!(
        settler
            .settle(&chat.id, &mut chat.world, &[3], None)
            .await
            .unwrap()[&3]
            .cached,
        "the world before it is still kept"
    );
    let mut next = chat.world.clone_world().unwrap();
    next.finalize();
    assert!(
        !settler
            .settle(&chat.id, &mut next, &[3], None)
            .await
            .unwrap()[&3]
            .cached,
        "a revision is a world"
    );
}

/// A renderer that keeps nothing it is not given, draws each shot as a
/// JPEG-headed name, and notes each `shots` request's world and size.
#[derive(Default)]
struct Pictures {
    drawn: Mutex<Vec<(String, usize)>>,
}

impl mcbench_render::live::LiveRenderer for Pictures {
    fn live(&self) -> Arc<dyn mcbench_render::live::LivePane> {
        unimplemented!("see_build draws no live view")
    }
    fn live_show(&self, _: Option<mcbench_render::live::Shown>) {}
    fn live_patch(&self, _: mcbench_render::live::LivePatch) {}
}

impl mcbench_render::Renderer for Pictures {
    fn start(
        _: &mcbench_render::Assets,
        _: mcbench_render::AdapterChoice,
        _: u64,
    ) -> Result<Self, mcbench_render::RenderError> {
        unimplemented!("given started")
    }
    fn report(&self) -> &mcbench_render::AdapterReport {
        unimplemented!("never read")
    }
    fn world(
        &self,
        _: mcbench_render::WorldUpload,
    ) -> Result<mcbench_render::WorldReport, mcbench_render::RenderError> {
        unimplemented!("every world is kept")
    }
    fn shots(
        &self,
        world: &str,
        shots: &[mcbench_render::Shot],
        _: u32,
        _: mcbench_render::PictureFormat,
        _: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<mcbench_render::Picture>, mcbench_render::RenderError> {
        self.drawn
            .lock()
            .unwrap()
            .push((world.to_string(), shots.len()));
        Ok(shots
            .iter()
            .map(|_| mcbench_render::Picture {
                width: 1,
                height: 1,
                bytes: vec![0xff, 0xd8, 0xff, 0],
            })
            .collect())
    }
    fn montage(
        &self,
        _: [u32; 2],
        _: &[mcbench_render::Cell],
        _: &[&[u8]],
        _: u8,
    ) -> Result<Vec<u8>, mcbench_render::RenderError> {
        unimplemented!("see_build makes no card")
    }
    fn drop_world(&self, _: &str) -> Result<bool, mcbench_render::RenderError> {
        unimplemented!("see_build drops no world")
    }
    fn shutdown(&self) {
        unimplemented!("see_build never shuts the renderer down")
    }
}

/// "see_build shows several ticks of one run, reports each, and defaults to the last build's ticks" (the counts of blocks changed are Minecraft's sand's, not the fake engine's, and are not compared; that the ticks asked were settled in the call is read from the settles kept)
async fn see_build_shows_several_ticks_of_one_run(c: &'static Context) {
    let s = c.default_services();
    let pictures = Arc::new(Pictures::default());
    s.renderer.set(Ok(
        Arc::clone(&pictures) as Arc<dyn mcbench_render::Renderer>
    ));
    let mut chat = chat(&s, "ticks-see");
    assert_eq!(build_with(&s, &mut chat, TOWER, json!(6)).await["ok"], true);
    let result = call(
        &s,
        &mut chat,
        "see_build",
        json!({"shots": [{"view": "front"}, {"view": "front", "tick": 0}, {"view": "top", "tick": 2}, {"view": "iso_se", "tick": 6}, {"view": "iso_se", "tick": 40}]}),
    )
    .await;
    assert_eq!(result["ok"], true, "{result}");
    let ticks = |of: &Value, key: &str, field: &str| -> Vec<i64> {
        of[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|one| one[field].as_i64().unwrap())
            .collect()
    };
    assert_eq!(ticks(&result, "shots", "tick"), [6, 0, 2, 6, 40]);
    assert_eq!(ticks(&result, "minecraft", "ticks"), [0, 2, 6, 40]);
    // The shots of one settle are one request, in the order of their first shot.
    let drawn: Vec<(String, usize)> = pictures
        .drawn
        .lock()
        .unwrap()
        .iter()
        .map(|(world, n)| (world.split('/').nth(1).unwrap().to_string(), *n))
        .collect();
    assert_eq!(
        drawn,
        [
            ("tick 6".to_string(), 2),
            ("tick 0".to_string(), 1),
            ("tick 2".to_string(), 1),
            ("tick 40".to_string(), 1)
        ]
    );
    let kept = s
        .settler
        .settle(&chat.id, &mut chat.world, &[0, 2, 6, 40], None)
        .await
        .unwrap();
    assert!(
        kept.values().all(|settle| settle.cached),
        "the call settled every tick"
    );
    let again = call(
        &s,
        &mut chat,
        "see_build",
        json!({"shots": [{"view": "front", "tick": 2}]}),
    )
    .await;
    assert_eq!(ticks(&again, "minecraft", "ticks"), [2]);
    let max = s.budgets.simulation_ticks;
    let refused = call(
        &s,
        &mut chat,
        "see_build",
        json!({"shots": [{"view": "front"}, {"tick": max + 1}]}),
    )
    .await;
    assert_eq!(refused["ok"], false);
    assert_eq!(
        refused["error"],
        format!(
            "shot 2: tick is {} ticks; the budget of simulated ticks, the same as test_build's, is {max} (MCBENCH_VERIFY_MAX_SIMULATION_TICKS)",
            max + 1
        )
    );
}

// ---- settle-flow.test.js ----

/// "a candidate settled but not applied leaves the conversation's own settle, ticks included"
async fn a_refused_candidate_leaves_the_conversations_settle(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "flow-3");
    build_with(
        &s,
        &mut chat,
        "module.exports = mc => mc.set([0, 0, 0], \"stone\")",
        json!(30),
    )
    .await;
    let refused = build_with(
        &s,
        &mut chat,
        "module.exports = mc => mc.set([9, 5, 9], \"torch\")",
        json!(80),
    )
    .await;
    assert_eq!(refused["ok"], false);
    assert_eq!(mcbench_tools::settle::current_ticks(&chat.world), 30);
    assert!(
        s.settler
            .settle(&chat.id, &mut chat.world, &[30], None)
            .await
            .unwrap()[&30]
            .cached
    );
    assert_eq!(
        build(&s, &mut chat, "mc.log(mc.settledTicks)").await["logs"],
        json!(["30"])
    );
}

// ---- agent-vision.test.js ----

/// "text-only models do not receive vision tools" (its tool rule: a vision tool called anyway is refused, and build still runs)
async fn a_text_only_model_is_refused_the_vision_tools(c: &'static Context) {
    let s = c.default_services();
    let mut chat = chat(&s, "vision-1");
    chat.image_input = false;
    for name in ["see_build", "view_image"] {
        assert_eq!(
            call(&s, &mut chat, name, json!({"path": "x.png"})).await,
            json!({"ok": false, "error": "The selected model does not support image input"})
        );
    }
    assert_eq!(
        build(&s, &mut chat, "mc.set([0, 0, 0], 'stone')").await["ok"],
        true
    );
}

// ---- storage-limits.test.js ----

fn small_budget() -> mcbench_store::Limits {
    limits(&[
        ("MCBENCH_CHAT_MAX_BYTES", "1048576"),
        ("MCBENCH_REPORTS_KEPT", "3"),
    ])
}

fn listing(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The verifier's fake engine answering every experiment with a report of
/// `bytes` bytes, counting the experiments it ran.
fn padded(bytes: usize, ran: Arc<Mutex<usize>>) -> VerifierPool {
    let engine = verify_common::FakeEngine::new(move |request| {
        *ran.lock().unwrap() += 1;
        let report = mcbench_json::stringify(
            &json!({"initial": {}, "observations": [], "padding": "x".repeat(bytes)}),
        );
        Box::pin(async move { Ok(verify_common::complete_with(&request, &report, |_| {})) })
    });
    verify_common::verifier(engine, verify_common::budgets()).0
}

/// A chat whose world holds a block and the arguments of `count` tests
/// that probe it (today's test's fake verifier read no request; this one
/// checks it as the verifier does).
fn probed(services: &Services, id: &str, count: usize) -> (Chat, serde_json::Map<String, Value>) {
    let mut chat = chat(services, id);
    chat.world
        .set(
            [0, 0, 0],
            mcbench_world::BlockState::new("minecraft:stone", &[]),
        )
        .unwrap();
    chat.world.finalize();
    let tests: Vec<Value> = (0..count).map(|_| json!({"probes": [[0, 0, 0]]})).collect();
    (chat, json!({ "tests": tests }).as_object().unwrap().clone())
}

/// "a test_build report that does not fit fails its test and is not kept"
async fn a_report_that_does_not_fit_fails_its_test(c: &'static Context) {
    let s = c.services(small_budget(), Some(padded(700 * 1024, Arc::default())));
    let (chat, args) = probed(&s, "limits-1", 2);
    let result = TestBuild(s.clone())
        .run(&chat, &args, &mut |_| {}, &CancellationToken::new())
        .await;
    let tests = result["tests"].as_array().unwrap();
    assert_eq!(
        tests.iter().filter(|t| t["ok"] == true).count(),
        1,
        "{result}"
    );
    let failed = tests.iter().find(|t| t["ok"] != true).unwrap();
    assert!(
        failed["error"]
            .as_str()
            .unwrap()
            .contains("This chat's storage is full"),
        "{failed}"
    );
    assert_eq!(listing(&chat.workspace.host.join("verification")).len(), 1);
}

/// "a chat over its budget runs no test_build experiments"
async fn a_chat_over_its_budget_runs_no_experiments(c: &'static Context) {
    let ran = Arc::new(Mutex::new(0));
    let s = c.services(small_budget(), Some(padded(10, ran.clone())));
    let (chat, args) = probed(&s, "limits-2", 1);
    std::fs::create_dir_all(&chat.workspace.host).unwrap();
    std::fs::write(
        chat.workspace.host.join("filled.bin"),
        vec![0u8; 1100 * 1024],
    )
    .unwrap();
    s.store.storage().measure(&chat.id).wait().unwrap();
    let result = TestBuild(s.clone())
        .run(&chat, &args, &mut |_| {}, &CancellationToken::new())
        .await;
    assert_eq!(result["ok"], false);
    let error = result["error"].as_str().unwrap();
    assert!(
        error.starts_with("This chat's storage is full: ")
            && error.contains("hold 1.")
            && error.contains("MiB of its 1.0 MiB budget"),
        "{error}"
    );
    assert_eq!(*ran.lock().unwrap(), 0);
}

/// "a test_build call prunes the oldest reports past the cap, never one a stored message names"
async fn a_call_prunes_the_oldest_reports(c: &'static Context) {
    let s = c.services(small_budget(), Some(padded(10, Arc::default())));
    let conversation = s
        .store
        .create_conversation("openai", "fixture", Default::default())
        .map_err(|e| e.message)
        .unwrap();
    let id = conversation.lock().unwrap().id.0.clone();
    let (chat, args) = probed(&s, &id, 1);
    let dir = chat.workspace.host.join("verification");
    std::fs::create_dir_all(&dir).unwrap();
    let names: Vec<String> = (0..5)
        .map(|_| format!("{}.json", s.store.host().uuid()))
        .collect();
    for (index, name) in names.iter().enumerate() {
        let file = dir.join(name);
        std::fs::write(&file, "{}").unwrap();
        let time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000 + index as u64);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(time)
            .unwrap();
    }
    std::fs::write(dir.join("notes.json"), "{}").unwrap();
    // The oldest report is named by a saved tool result.
    {
        let mut conversation = conversation.lock().unwrap();
        s.store
            .append_message(
                &mut conversation,
                mcbench_store::NewMessage::prompt(
                    &mcbench_store::Content::Text("test it".into()),
                    &[],
                ),
                None,
            )
            .unwrap();
        let telemetry = mcbench_store::ToolTelemetry {
            name: "test_build".into(),
            started_at: "2026-11-17T00:00:00.000Z".into(),
            wall_ms: 1,
            ok: true,
            cancelled: false,
        };
        let content = mcbench_json::stringify(
            &json!({"ok": true, "tests": [{"report": format!("/work/verification/{}", names[0])}]}),
        );
        s.store
            .append_message(
                &mut conversation,
                mcbench_store::NewMessage::tool_result("call", &telemetry, &content),
                None,
            )
            .unwrap();
        s.store.save(&mut conversation).unwrap();
    }
    let result = TestBuild(s.clone())
        .run(&chat, &args, &mut |_| {}, &CancellationToken::new())
        .await;
    assert_eq!(result["ok"], true, "{result}");
    let report = result["tests"][0]["report"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let mut expected = vec![
        names[0].clone(),
        names[3].clone(),
        names[4].clone(),
        "notes.json".into(),
        report,
    ];
    expected.sort();
    assert_eq!(listing(&dir), expected);
}

// ---- schematic.test.js ----

/// An NBT reader independent of the writer (the Node test reads with
/// nbtlib): the schematic's size, palette and the block at each position.
fn read_back(bytes: &[u8], positions: &[[usize; 3]]) -> Value {
    use std::io::Read;
    let mut nbt = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut nbt)
        .unwrap();
    struct R<'a>(&'a [u8], usize);
    impl R<'_> {
        fn take(&mut self, n: usize) -> &[u8] {
            let s = &self.0[self.1..self.1 + n];
            self.1 += n;
            s
        }
        fn u16(&mut self) -> usize {
            u16::from_be_bytes(self.take(2).try_into().unwrap()) as usize
        }
        fn i32(&mut self) -> i32 {
            i32::from_be_bytes(self.take(4).try_into().unwrap())
        }
        fn name(&mut self) -> String {
            let n = self.u16();
            String::from_utf8(self.take(n).to_vec()).unwrap()
        }
        fn payload(&mut self, kind: u8) -> Value {
            match kind {
                2 => json!(self.u16()),
                3 => json!(self.i32()),
                7 => {
                    let n = self.i32() as usize;
                    json!(self.take(n).to_vec())
                }
                8 => json!(self.name()),
                11 => {
                    let n = self.i32() as usize;
                    json!((0..n).map(|_| self.i32()).collect::<Vec<_>>())
                }
                10 => {
                    let mut map = Map::new();
                    loop {
                        let kind = self.take(1)[0];
                        if kind == 0 {
                            break;
                        }
                        let name = self.name();
                        map.insert(name, self.payload(kind));
                    }
                    Value::Object(map)
                }
                other => panic!("tag {other}"),
            }
        }
    }
    let mut r = R(&nbt, 0);
    assert_eq!(r.take(1)[0], 10);
    assert_eq!(r.name(), "");
    let root = r.payload(10);
    let s = &root["Schematic"];
    let (w, h, l) = (
        s["Width"].as_u64().unwrap() as usize,
        s["Height"].as_u64().unwrap() as usize,
        s["Length"].as_u64().unwrap() as usize,
    );
    let palette: std::collections::HashMap<i64, String> = s["Blocks"]["Palette"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (v.as_i64().unwrap(), k.clone()))
        .collect();
    let mut cells = Vec::new();
    let (mut value, mut shift) = (0i64, 0);
    for b in s["Blocks"]["Data"].as_array().unwrap() {
        let b = b.as_u64().unwrap() as i64;
        value |= (b & 0x7f) << shift;
        shift += 7;
        if b & 0x80 == 0 {
            cells.push(value);
            value = 0;
            shift = 0;
        }
    }
    let blocks: Vec<String> = positions
        .iter()
        .map(|[x, y, z]| palette[&cells[x + z * w + y * w * l]].clone())
        .collect();
    json!({"version": s["Version"], "dataVersion": s["DataVersion"], "size": [w, h, l], "cells": cells.len(), "palette": palette.len(), "blocks": blocks})
}

/// "schematic keeps every authored block state at its position"
async fn schematic_keeps_every_state(_: &'static Context) {
    let mut world = World::new();
    world
        .set(
            [-2, -1, 3],
            mcbench_world::BlockState::new("minecraft:stone", &[]),
        )
        .unwrap();
    world
        .set(
            [1, 4, 5],
            mcbench_world::BlockState::new(
                "minecraft:oak_stairs",
                &[("half", "top"), ("facing", "east")],
            ),
        )
        .unwrap();
    world
        .fill(
            mcbench_world::BlockBox {
                min: [0, 0, 3],
                max: [1, 0, 3],
            },
            mcbench_world::BlockState::new("minecraft:oak_fence", &[]),
        )
        .unwrap();
    let result = read_back(
        &mcbench_tools::export::schematic(&world).unwrap(),
        &[[0, 0, 0], [3, 5, 2], [2, 1, 0], [3, 1, 0], [1, 2, 1]],
    );
    assert_eq!(result["version"], 3);
    assert_eq!(result["dataVersion"], 4556);
    assert_eq!(result["size"], json!([4, 6, 3]));
    assert_eq!(result["cells"], 4 * 6 * 3);
    assert_eq!(
        result["blocks"],
        json!([
            "minecraft:stone",
            "minecraft:oak_stairs[facing=east,half=top]",
            "minecraft:oak_fence",
            "minecraft:oak_fence",
            "minecraft:air"
        ])
    );
}

/// "palettes past 128 states use multi-byte indexes"
async fn large_palettes_use_multi_byte_indexes(_: &'static Context) {
    let mut world = World::new();
    for i in 0..300 {
        world
            .set(
                [i, 0, 0],
                mcbench_world::BlockState::new(
                    "minecraft:stone_bricks",
                    &[("variant", &i.to_string())],
                ),
            )
            .unwrap();
    }
    let result = read_back(
        &mcbench_tools::export::schematic(&world).unwrap(),
        &[[0, 0, 0], [127, 0, 0], [299, 0, 0]],
    );
    assert_eq!(result["palette"], 301);
    assert_eq!(result["cells"], 300);
    assert_eq!(
        result["blocks"],
        json!([
            "minecraft:stone_bricks[variant=0]",
            "minecraft:stone_bricks[variant=127]",
            "minecraft:stone_bricks[variant=299]"
        ])
    );
}

/// "an empty build has nothing to export"
async fn an_empty_build_has_nothing_to_export(_: &'static Context) {
    assert!(
        mcbench_tools::export::schematic(&World::new())
            .unwrap_err()
            .contains("empty")
    );
}

/// "a chat's schematic is the world the engine holds after placing the build, kept for the next export" (Minecraft joins the panes itself)
async fn a_schematic_is_the_placed_world_and_is_kept(c: &'static Context) {
    let s = c.default_services();
    let mut world = World::new();
    world
        .fill(
            mcbench_world::BlockBox {
                min: [0, 0, 0],
                max: [2, 0, 0],
            },
            mcbench_world::BlockState::new("minecraft:glass_pane", &[]),
        )
        .unwrap();
    world.finalize();
    let bytes = mcbench_tools::export::build_schematic(&s.settler, "schematic-chat", &mut world)
        .await
        .unwrap();
    let result = read_back(&bytes, &[[0, 0, 0], [1, 0, 0], [2, 0, 0]]);
    assert_eq!(result["size"], json!([3, 1, 1]));
    assert_eq!(
        result["blocks"],
        json!([
            "minecraft:glass_pane[east=true,north=false,south=false,waterlogged=false,west=false]",
            "minecraft:glass_pane[east=true,north=false,south=false,waterlogged=false,west=true]",
            "minecraft:glass_pane[east=false,north=false,south=false,waterlogged=false,west=true]",
        ])
    );
    assert!(
        s.settler
            .settle("schematic-chat", &mut world, &[0], None)
            .await
            .unwrap()[&0]
            .cached,
        "the second export reads the kept world"
    );
}

/// "a build Minecraft keeps none of has no schematic" (a torch alone, which Minecraft drops at once)
async fn a_build_minecraft_keeps_none_of_has_no_schematic(c: &'static Context) {
    let s = c.default_services();
    let mut world = World::new();
    world
        .set(
            [0, 0, 0],
            mcbench_world::BlockState::new("minecraft:sand", &[]),
        )
        .unwrap();
    world.finalize();
    let settled = s
        .settler
        .settle("gone", &mut world, &[0], None)
        .await
        .unwrap();
    if settled[&0].value.world.size() == 0 {
        let error = mcbench_tools::export::build_schematic(&s.settler, "gone", &mut world)
            .await
            .unwrap_err();
        assert!(error.contains("keeps none"));
    } else {
        // Minecraft keeps a lone block placed at tick 0: the empty settle is checked as the tool words it.
        let mut empty = World::new();
        empty.finalize();
        let error = mcbench_tools::export::build_schematic(&s.settler, "gone-empty", &mut empty)
            .await
            .unwrap_err();
        assert_eq!(
            error,
            "Minecraft keeps none of the blocks of this build: there is nothing to export"
        );
    }
}

const CASES: &[(&str, Case)] = cases![
    a_block_that_cannot_survive_refuses_the_build,
    support_taken_across_a_chunk_border_is_refused,
    a_bed_needs_its_other_half,
    a_build_carries_minecrafts_verdict_and_mc_settled_reads_it,
    a_world_is_settled_once,
    mc_settled_takes_three_integers,
    two_requests_share_a_run_and_the_latest_conversations_stay,
    changed_ids_are_a_cell_by_cell_comparison,
    a_build_settles_for_210_ticks_unless_asked,
    ticks_beyond_the_budget_are_refused,
    mc_settled_reads_the_worlds_settle_and_its_ticks,
    settles_are_kept_by_conversation_world_revision_and_ticks,
    see_build_shows_several_ticks_of_one_run,
    a_refused_candidate_leaves_the_conversations_settle,
    a_text_only_model_is_refused_the_vision_tools,
    a_report_that_does_not_fit_fails_its_test,
    a_chat_over_its_budget_runs_no_experiments,
    a_call_prunes_the_oldest_reports,
    schematic_keeps_every_state,
    large_palettes_use_multi_byte_indexes,
    an_empty_build_has_nothing_to_export,
    a_schematic_is_the_placed_world_and_is_kept,
    a_build_minecraft_keeps_none_of_has_no_schematic,
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // A name to run, as libtest takes one; a flag's value (`--test-threads 2`) is not one.
    let filter: Option<&String> = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(at, a)| !a.starts_with('-') && !args[at - 1].starts_with("--test-threads"))
        .map(|(_, a)| a);
    let Some(tables) = std::env::var_os("MCBENCH_TABLES").map(PathBuf::from) else {
        for (name, _) in CASES {
            println!("test {name} ... ignored, needs MCBENCH_TABLES=<core/minecraft/1.21.10>");
        }
        println!(
            "\ntest result: ok. 0 passed; 0 failed; {} ignored",
            CASES.len()
        );
        return;
    };
    // The cases share tools whose build runner needs the platform's build
    // sandbox, which on Windows is the Linux child inside a WSL2 distro the
    // bash sandbox imported, named for the test by MCBENCH_WSL_DISTRO.
    if cfg!(windows) && std::env::var_os(mcbench_build::sandbox::wsl::DISTRO_VAR).is_none() {
        for (name, _) in CASES {
            println!(
                "test {name} ... ignored, needs {}=<a WSL2 distro the bash sandbox imported> and MCBENCH_BUILD_CHILD=<the Linux x86_64 mcbench-build-child>",
                mcbench_build::sandbox::wsl::DISTRO_VAR
            );
        }
        println!(
            "\ntest result: ok. 0 passed; 0 failed; {} ignored",
            CASES.len()
        );
        return;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    let root = std::env::temp_dir().join(format!("mcbench-tools-ported-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let context: &'static Context = runtime.block_on(async {
        let shared = shared(&tables).unwrap();
        Box::leak(Box::new(Context {
            registry: Arc::new(registry_from_table(&tables).unwrap()),
            shared,
            tables: tables.clone(),
            root: root.clone(),
            folders: Mutex::new(0),
        }))
    });
    let _ = &context.tables;
    let (mut passed, mut failed) = (0, Vec::new());
    for (name, case) in CASES {
        if filter.is_some_and(|f| !name.contains(f.as_str())) {
            continue;
        }
        let outcome = runtime.block_on(runtime.spawn(case(context)));
        match outcome {
            Ok(()) => {
                println!("test {name} ... ok");
                passed += 1;
            }
            Err(_) => {
                println!("test {name} ... FAILED");
                failed.push(*name);
            }
        }
    }
    let _ = std::fs::remove_dir_all(&root);
    println!(
        "\ntest result: {}. {passed} passed; {} failed",
        if failed.is_empty() { "ok" } else { "FAILED" },
        failed.len()
    );
    if !failed.is_empty() {
        std::process::exit(1);
    }
}
