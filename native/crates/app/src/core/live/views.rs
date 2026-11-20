//! What a view key shows (`server/index.js` `viewOf` and `benchView`): a
//! chat's world, or a bench round's builds side by side
//! (`mcbench_world::bench`), behind `mcbench_world::live_views` as the
//! server keeps them.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::Value;

use mcbench_store::Message;
use mcbench_store::Store;
use mcbench_world::bench::{BenchComposer, RoundMessage, world_at_round};
use mcbench_world::facts::MinecraftFacts;
use mcbench_world::live_views::{LiveViews, Resolved};

use super::clock::Clock;

/// The views of the live world.
pub type Views = LiveViews<Box<dyn FnMut(&str) -> Resolved + Send>>;

/// The views over the store's chats and benches, timed by `clock`.
pub fn views(store: Arc<Store>, clock: Arc<dyn Clock>, facts: Arc<MinecraftFacts>) -> Views {
    let mut composers: HashMap<String, BenchComposer> = HashMap::new();
    LiveViews::new(
        Box::new(move |key: &str| resolve(&store, &mut composers, key)),
        mcbench_world::dimension::y_offset(),
        Box::new(move || clock.now().as_secs_f64() * 1000.0),
        facts,
    )
}

/// `viewOf(key)`.
fn resolve(store: &Store, composers: &mut HashMap<String, BenchComposer>, key: &str) -> Resolved {
    if key.starts_with("bench:") {
        let mut parts = key.split(':').skip(1);
        let bench_id = parts.next().unwrap_or("");
        return bench_view(store, composers, bench_id, parts.next());
    }
    let live = store
        .conversation(key)
        .map_err(|e| e.message)?
        .ok_or("Conversation not found")?;
    let world = live.lock().expect("a chat's lock").world.clone();
    Ok((Arc::new(world), Value::Null))
}

/// `benchView(benchId, Number(round))`.
fn bench_view(
    store: &Store,
    composers: &mut HashMap<String, BenchComposer>,
    bench_id: &str,
    round: Option<&str>,
) -> Resolved {
    let bench = store
        .bench(bench_id)
        .map_err(|e| e.message)?
        .ok_or("Bench not found")?;
    // Round 0 is before the first: every build empty.
    let round = match round.and_then(|r| r.parse::<usize>().ok()) {
        Some(round) if round <= bench.rounds.len() => round,
        _ => return Err("Unknown round".into()),
    };
    let order: Vec<&str> = bench
        .order
        .as_array()
        .ok_or("A bench's order is a list")?
        .iter()
        .map(|id| id.as_str().ok_or("A bench's order holds member ids"))
        .collect::<Result<_, _>>()?;
    let index = |id: &str| order.iter().position(|o| *o == id).map_or(-1, |i| i as i64);
    let mut members: Vec<(String, mcbench_world::World)> = Vec::new();
    for member in store.bench_members(bench_id).map_err(|e| e.message)? {
        let member = member.lock().expect("a chat's lock");
        let messages: Vec<RoundMessage> = member
            .messages
            .iter()
            .map(|m| {
                let (role, see_build) = match &m.message {
                    Message::User(user) => ("user", user.see_build),
                    Message::Assistant(_) => ("assistant", false),
                    Message::Tool(_) => ("tool", false),
                    Message::Error(_) => ("error", false),
                };
                RoundMessage {
                    id: m.id.0.clone(),
                    role: role.into(),
                    see_build,
                }
            })
            .collect();
        let checkpoints: HashMap<String, mcbench_world::World> = member
            .checkpoints
            .iter()
            .map(|(id, world)| (id.0.clone(), world.clone()))
            .collect();
        let world = world_at_round(&messages, &checkpoints, &member.world, round)
            .ok_or("A member's round has no checkpoint")?
            .clone();
        members.push((member.id.0.clone(), world));
    }
    members.sort_by_key(|(id, _)| index(id));
    let entries: Vec<(String, &mcbench_world::World)> = members
        .iter()
        .map(|(id, world)| (id.clone(), world))
        .collect();
    let composer = composers.entry(bench_id.to_string()).or_default();
    let (world, layout) = composer.compose(&entries)?;
    Ok((
        Arc::new(world),
        serde_json::to_value(&layout).expect("a layout is JSON"),
    ))
}
