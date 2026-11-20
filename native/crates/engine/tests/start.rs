//! `test/engine.test.js`'s start cases, in process: a test binary of their
//! own, as the tables are given once per process.

mod common;

use mcbench_engine::Engine;

// engine.test.js: "an engine that cannot start reports why", and "a failed
// build reaches the caller and the next warm builds again". Nothing is
// built in process: a start that cannot read a table fails with the
// released program's sentence naming it, leaves nothing set, and the next
// start tries again; once one worked, the tables cannot be given again.
#[test]
#[cfg_attr(
    not(mcbench_tables),
    ignore = "needs Minecraft's tables: set MCBENCH_TABLES at build time"
)]
fn an_engine_that_cannot_start_reports_why_and_the_next_start_tries_again() {
    let folder = common::scratch("start");
    for name in ["blocks.txt", "loot.json"] {
        std::fs::copy(common::tables().join(name), folder.join(name)).unwrap();
    }
    let expected = format!(
        "mcbench-core: cannot read Minecraft's table {}: ",
        folder.join("datafix.txt").display()
    );
    for _ in 0..2 {
        let error = Engine::start(&folder).err().unwrap();
        assert!(error.starts_with(&expected), "{error}");
    }
    let engine = Engine::start(&common::tables()).unwrap();
    assert!(engine.world().unwrap().alive());
    assert_eq!(
        Engine::start(&common::tables()).err().unwrap(),
        "The engine was started already: Minecraft's tables are given once per process"
    );
    std::fs::remove_dir_all(&folder).unwrap();
}
