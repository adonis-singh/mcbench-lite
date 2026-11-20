//! One run of the store timings through this crate, on a clone named by
//! `MCBENCH_BENCH_DATA`: the same calls as `tests/node/bench.js`. Prints one
//! JSON line of milliseconds. Run in release mode; ignored otherwise.

mod common;
#[path = "../../agent/tests/common/fixture.rs"]
mod fixture;

use std::sync::Arc;
use std::time::Instant;

use mcbench_store::{Content, NewMessage, SearchScope, SystemHost};

#[test]
#[ignore = "needs MCBENCH_BENCH_DATA (a clone of a data folder); a timing, run with --release"]
fn store_timings() {
    let root = std::path::PathBuf::from(
        std::env::var("MCBENCH_BENCH_DATA").expect("MCBENCH_BENCH_DATA must be set"),
    );
    fixture::guard(&root);
    let probe = rusqlite::Connection::open_with_flags(
        root.join("mcbench.sqlite3"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let most: String = probe
        .query_row("SELECT conversation_id AS id FROM messages m JOIN conversations c ON c.id = m.conversation_id WHERE c.archived_at IS NULL GROUP BY conversation_id ORDER BY count(*) DESC LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let largest: String = probe
        .query_row("SELECT id FROM conversations WHERE archived_at IS NULL ORDER BY length(world_manifest_json) DESC LIMIT 1", [], |r| r.get(0))
        .unwrap();
    drop(probe);
    let ms = |start: Instant| start.elapsed().as_secs_f64() * 1000.0;
    let start = Instant::now();
    let store = common::open(
        &root,
        Arc::new(SystemHost),
        mcbench_store::Limits::from_env(|name| std::env::var(name).ok()).unwrap(),
    )
    .unwrap();
    let open = ms(start);
    let start = Instant::now();
    store.list_conversations().unwrap();
    let list = ms(start);
    let start = Instant::now();
    store.conversation(&most).unwrap().unwrap();
    let load_most = ms(start);
    let start = Instant::now();
    store.conversation(&largest).unwrap().unwrap();
    let load_largest = ms(start);
    let start = Instant::now();
    store.search(SearchScope::Build, "tower").unwrap();
    let search = ms(start);
    let live = store.conversation(&most).unwrap().unwrap();
    let mut chat = live.lock().unwrap();
    let start = Instant::now();
    let world = chat.world.clone();
    store
        .append_message(
            &mut chat,
            NewMessage::prompt(&Content::Text("Timing prompt".into()), &[]),
            Some(world),
        )
        .unwrap();
    store.save(&mut chat).unwrap();
    let append_save = ms(start);
    println!(
        r#"{{"open":{open},"list":{list},"loadMost":{load_most},"loadLargestWorld":{load_largest},"search":{search},"appendSave":{append_save}}}"#
    );
}
