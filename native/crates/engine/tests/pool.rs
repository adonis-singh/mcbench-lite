//! `test/settle-pool.test.js` (14) and `test/settle-eviction.test.js` (2):
//! the pool over fake members (as the first file's own `EnginePool` cases
//! make them), the settles on real engine children (where today's file uses
//! its fake engines, which only move sand), and what is kept of each
//! conversation with a fake run (as the eviction file's fake engines do);
//! and what the children are for: a child killed from outside mid-settle,
//! one past the wall budget, and one past a memory limit. The cases on
//! children need Minecraft's tables and the children's program
//! (`MCBENCH_TABLES` and `MCBENCH_ENGINE_PROGRAM` at build time).

mod common;

use mcbench_engine::{
    ChildEngine, EngineError, KEPT_CONVERSATIONS, Lease, Member, PlaceCommands, Pool, SettlePool,
    Settled, Settles, SettlesKey, pool_size, settle_on,
};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// A member as the pool cases make them: alive until told otherwise, its
/// stop recorded, and (when given one) a stop that waits to be let go.
struct Fake {
    slot: usize,
    died: AtomicBool,
    stopped: Arc<Mutex<Vec<usize>>>,
    exit: Option<Arc<Notify>>,
}

impl Member for Fake {
    fn alive(&self) -> bool {
        !self.died.load(Ordering::SeqCst)
    }

    fn stop(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if let Some(exit) = &self.exit {
                exit.notified().await;
            }
            self.stopped.lock().unwrap().push(self.slot);
        })
    }
}

struct Fakes {
    made: Arc<Mutex<Vec<usize>>>,
    stopped: Arc<Mutex<Vec<usize>>>,
}

fn fake_pool(size: usize, wall: Duration) -> (Pool<Fake>, Fakes) {
    let made = Arc::new(Mutex::new(Vec::new()));
    let stopped = Arc::new(Mutex::new(Vec::new()));
    let (m, s) = (made.clone(), stopped.clone());
    let pool = Pool::new(size, wall, move |slot| {
        m.lock().unwrap().push(slot);
        Ok(Fake {
            slot,
            died: AtomicBool::new(false),
            stopped: s.clone(),
            exit: None,
        })
    });
    (pool, Fakes { made, stopped })
}

const MINUTE: Duration = Duration::from_secs(60);

async fn pause(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pool_runs_jobs_of_different_keys_at_once_on_as_many_members_as_it_has_and_a_keys_jobs_in_order()
 {
    let (pool, fakes) = fake_pool(3, MINUTE);
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let job = |name: &'static str, ms: u64| {
        let (running, peak, log) = (running.clone(), peak.clone(), log.clone());
        move |lease: Arc<Lease<Fake>>| async move {
            let now = running.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            log.lock()
                .unwrap()
                .push(format!("{name} start on {}", lease.slot()));
            pause(ms).await;
            log.lock().unwrap().push(format!("{name} end"));
            running.fetch_sub(1, Ordering::SeqCst);
            Ok(())
        }
    };
    let jobs = vec![
        tokio::spawn(pool.run("a", job("a1", 40))),
        tokio::spawn(pool.run("a", job("a2", 5))),
        tokio::spawn(pool.run("a", job("a3", 5))),
        tokio::spawn(pool.run("b", job("b1", 20))),
        tokio::spawn(pool.run("c", job("c1", 20))),
        tokio::spawn(pool.run("d", job("d1", 5))),
    ];
    for job in jobs {
        job.await.unwrap().unwrap();
    }
    assert_eq!(peak.load(Ordering::SeqCst), 3);
    let mut made = fakes.made.lock().unwrap().clone();
    made.sort();
    assert_eq!(
        made,
        [0, 1, 2],
        "members are made when jobs wait for one, never more than the size"
    );
    let log = log.lock().unwrap().clone();
    let a: Vec<&String> = log.iter().filter(|line| line.starts_with('a')).collect();
    assert_eq!(a[0], "a1 start on 0");
    assert_eq!(a[1], "a1 end");
    assert!(a[2].starts_with("a2 start on "));
    assert_eq!(a[3], "a2 end");
    assert!(a[4].starts_with("a3 start on "));
    assert_eq!(a[5], "a3 end");
    assert!(log.iter().any(|line| line == "d1 end"));
    pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_pool_with_one_job_at_a_time_makes_one_member_and_a_dropped_member_is_replaced_for_the_jobs_waiting()
 {
    let (pool, fakes) = fake_pool(1, MINUTE);
    let failing = pool.run("a", |lease: Arc<Lease<Fake>>| async move {
        lease.drop_member();
        Err::<usize, _>(EngineError::Stopped("did not start".into()))
    });
    let waiting = pool.run(
        "b",
        |lease: Arc<Lease<Fake>>| async move { Ok(lease.slot()) },
    );
    let (failing, waiting) = (tokio::spawn(failing), tokio::spawn(waiting));
    assert_eq!(
        failing.await.unwrap().unwrap_err().to_string(),
        "did not start"
    );
    assert_eq!(waiting.await.unwrap().unwrap(), 0, "the slot is reused");
    assert_eq!(*fakes.made.lock().unwrap(), [0, 0]);
    assert_eq!(*fakes.stopped.lock().unwrap(), [0]);
    assert_eq!(
        pool.run(
            "c",
            |lease: Arc<Lease<Fake>>| async move { Ok(lease.slot()) }
        )
        .await
        .unwrap(),
        0
    );
    assert_eq!(*fakes.made.lock().unwrap(), [0, 0]);
    // A job that needs no member is still run in its key's order.
    assert_eq!(
        pool.run_without_member("x", async { Ok("no member") })
            .await
            .unwrap(),
        "no member"
    );
    pool.stop().await;
}

// A settle with no end in sight used to hold its engine until the server
// stopped; eight of them held the whole pool.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_job_past_the_wall_clock_budget_fails_and_its_member_is_stopped_and_replaced() {
    let (pool, fakes) = fake_pool(1, Duration::from_millis(50));
    let stuck = tokio::spawn(pool.run("a", |_lease: Arc<Lease<Fake>>| {
        std::future::pending::<Result<(), EngineError>>()
    }));
    let next = tokio::spawn(pool.run(
        "b",
        |lease: Arc<Lease<Fake>>| async move { Ok(lease.slot()) },
    ));
    let error = stuck.await.unwrap().unwrap_err().to_string();
    assert!(
        error.contains("ran past its 0.05-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS)"),
        "{error}"
    );
    assert_eq!(
        error,
        "The settle ran past its 0.05-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS) and was stopped: the build is too active to settle in time. Ask for fewer ticks or make the build less active."
    );
    assert_eq!(next.await.unwrap().unwrap(), 0);
    assert_eq!(*fakes.made.lock().unwrap(), [0, 0]);
    assert!(fakes.stopped.lock().unwrap().contains(&0));
    pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stopped_pool_fails_the_jobs_waiting_for_a_member_and_makes_none_for_a_job_that_ends_after_it()
 {
    let (pool, fakes) = fake_pool(1, MINUTE);
    let finish = Arc::new(Notify::new());
    let started = Arc::new(Notify::new());
    let (f, s) = (finish.clone(), started.clone());
    let running = tokio::spawn(pool.run("a", move |_lease: Arc<Lease<Fake>>| async move {
        s.notify_one();
        f.notified().await;
        Ok("done")
    }));
    let waiting = tokio::spawn(pool.run(
        "b",
        |lease: Arc<Lease<Fake>>| async move { Ok(lease.slot()) },
    ));
    started.notified().await;
    pool.stop().await;
    assert_eq!(
        waiting.await.unwrap().unwrap_err().to_string(),
        "The engine pool was stopped"
    );
    finish.notify_one();
    assert_eq!(running.await.unwrap().unwrap(), "done");
    assert_eq!(
        pool.run("c", |_lease: Arc<Lease<Fake>>| async { Ok(()) })
            .await
            .unwrap_err()
            .to_string(),
        "The engine pool was stopped"
    );
    assert_eq!(*fakes.made.lock().unwrap(), [0]);
    assert_eq!(pool.held(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_slot_is_not_given_a_new_engine_until_the_engine_dropped_from_it_has_exited() {
    let made = Arc::new(Mutex::new(Vec::new()));
    let exit = Arc::new(Notify::new());
    let (m, e) = (made.clone(), exit.clone());
    let pool = Pool::new(1, Duration::from_millis(30), move |slot| {
        let mut made = m.lock().unwrap();
        made.push(slot);
        // The first engine takes its time to exit after it is told to.
        let first = made.len() == 1;
        Ok(Fake {
            slot,
            died: AtomicBool::new(false),
            stopped: Arc::new(Mutex::new(Vec::new())),
            exit: first.then(|| e.clone()),
        })
    });
    let stuck = tokio::spawn(pool.run("a", |_lease: Arc<Lease<Fake>>| {
        std::future::pending::<Result<(), EngineError>>()
    }));
    let next = tokio::spawn(pool.run(
        "b",
        |lease: Arc<Lease<Fake>>| async move { Ok(lease.slot()) },
    ));
    assert!(
        stuck
            .await
            .unwrap()
            .unwrap_err()
            .to_string()
            .contains("wall-clock budget")
    );
    pause(20).await;
    assert_eq!(
        *made.lock().unwrap(),
        [0],
        "nothing new while the old engine runs"
    );
    exit.notify_one();
    assert_eq!(next.await.unwrap().unwrap(), 0);
    assert_eq!(*made.lock().unwrap(), [0, 0]);
    pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_free_member_whose_engine_died_is_replaced_not_handed_out() {
    let (pool, fakes) = fake_pool(1, MINUTE);
    let first = pool
        .run("a", |lease: Arc<Lease<Fake>>| async move { Ok(lease) })
        .await
        .unwrap();
    first.member().died.store(true, Ordering::SeqCst);
    let second = pool
        .run("b", |lease: Arc<Lease<Fake>>| async move { Ok(lease) })
        .await
        .unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(fakes.made.lock().unwrap().len(), 2);
    pool.stop().await;
}

#[test]
fn the_pool_is_sized_from_the_cores_and_memory_or_by_mcbench_settle_engines() {
    let size = pool_size(&HashMap::new()).unwrap();
    let cores = std::thread::available_parallelism().unwrap().get();
    assert!((1..=8).contains(&size) && size <= 1.max(cores - 1));
    let env =
        |value: &str| HashMap::from([("MCBENCH_SETTLE_ENGINES".to_string(), value.to_string())]);
    assert_eq!(pool_size(&env("3")).unwrap(), 3);
    assert_eq!(
        pool_size(&env("0")).unwrap_err(),
        "MCBENCH_SETTLE_ENGINES must be an integer of at least 1, not 0"
    );
}

/// tower(seed) of settle-pool.test.js at the engine's heights: a stone floor
/// reaching x = 3 + seed and three sand blocks over it, which fall.
fn tower(seed: i32) -> PlaceCommands {
    PlaceCommands(format!(
        "fill 0 64 0 {} 64 3 minecraft:stone\nfill 1 {} 1 1 {} 1 minecraft:sand\n",
        3 + seed,
        74 + seed,
        76 + seed
    ))
}

/// What is kept of the settles, and the settle (`shot-world.js` `settle`
/// over `runSettle`) with each run counted, the child's folder noted, and
/// held `delay` long before it reaches its child, as the fake engines' delay
/// holds them.
struct Settling {
    pool: SettlePool,
    settles: Settles<Settled>,
    running: Arc<AtomicUsize>,
    peak: Arc<AtomicUsize>,
    places: Arc<Mutex<Vec<String>>>,
    folders: Arc<Mutex<std::collections::HashSet<std::path::PathBuf>>>,
    delay: Duration,
}

impl Settling {
    fn new(members: usize, delay_ms: u64) -> Settling {
        Settling {
            pool: SettlePool::new(common::program(), members, MINUTE),
            settles: Settles::new(),
            running: Arc::new(AtomicUsize::new(0)),
            peak: Arc::new(AtomicUsize::new(0)),
            places: Arc::new(Mutex::new(Vec::new())),
            folders: Arc::new(Mutex::new(std::collections::HashSet::new())),
            delay: Duration::from_millis(delay_ms),
        }
    }

    fn settle(
        &self,
        conversation: &str,
        world: u64,
        commands: PlaceCommands,
        ticks: &[i32],
    ) -> impl Future<Output = Result<Vec<mcbench_engine::KeptSettle<Settled>>, EngineError>>
    + Send
    + 'static {
        let (pool, running, peak, places, folders, delay) = (
            self.pool.clone(),
            self.running.clone(),
            self.peak.clone(),
            self.places.clone(),
            self.folders.clone(),
            self.delay,
        );
        let conversation_key = conversation.to_string();
        self.settles.settle(
            conversation,
            SettlesKey { world, revision: 0 },
            ticks,
            move |_latest, missing| {
                let run = pool.run(&conversation_key, move |lease| async move {
                    let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    places.lock().unwrap().push(commands.0.clone());
                    folders
                        .lock()
                        .unwrap()
                        .insert(lease.member().folder().to_path_buf());
                    tokio::time::sleep(delay).await;
                    let settled = settle_on(
                        lease.member(),
                        &commands,
                        &missing,
                        &CancellationToken::new(),
                    )
                    .await;
                    running.fetch_sub(1, Ordering::SeqCst);
                    settled
                });
                async move {
                    Ok(run
                        .await?
                        .into_iter()
                        .map(|settled| (settled.ticks, settled))
                        .collect())
                }
            },
        )
    }
}

/// Starts `count` engines of `pool` side by side (each job holds its member
/// until every one has its own), so they are ready before a case's settles.
/// Today's fake engines are ready at once; a real member is a child process
/// that takes about 20 ms to start (longer in a debug build, on one CPU or
/// on Windows), which is longer than a case's settles take, so a settle that
/// needed a new member would start only after the others had ended.
async fn start_worlds(pool: &SettlePool, count: usize) {
    let all = Arc::new(tokio::sync::Barrier::new(count));
    let started: Vec<_> = (0..count)
        .map(|i| {
            let all = all.clone();
            tokio::spawn(pool.run(&format!("start-{i}"), move |_engine| async move {
                all.wait().await;
                Ok(())
            }))
        })
        .collect();
    for started in started {
        started.await.unwrap().unwrap();
    }
}

// An engine that died used to stay in the pool: every later settle on it
// failed until the server restarted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn an_engine_that_stopped_under_a_settle_is_replaced_one_that_refused_a_request_is_kept() {
    let pool = SettlePool::new(common::program(), 1, MINUTE);
    let resin = PlaceCommands("setblock 0 65 0 minecraft:resin_clump[down=true]\n".into());
    let refused = pool.settle("refused", resin, vec![0]).await.unwrap_err();
    assert!(
        refused
            .to_string()
            .starts_with("The engine failed: unported: "),
        "{refused}"
    );
    assert!(
        !pool.settle("refused", tower(1), vec![0]).await.unwrap()[0]
            .blueprint
            .0
            .is_empty()
    );
    assert_eq!(pool.engines_started(), 1);
    // The child dies (killed from outside) under the next settle's request.
    let died = pool
        .run("died", |lease: Arc<Lease<ChildEngine>>| async move {
            common::kill(lease.member().pid());
            lease.member().state(&CancellationToken::new()).await
        })
        .await
        .unwrap_err();
    assert_eq!(
        died.to_string(),
        format!("The engine stopped ({}): ", common::KILLED_FROM_OUTSIDE)
    );
    assert!(
        !pool.settle("died", tower(3), vec![0]).await.unwrap()[0]
            .blueprint
            .0
            .is_empty()
    );
    assert_eq!(pool.engines_started(), 2);
    pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn settles_of_different_conversations_run_at_once_each_on_a_world_of_its_own_and_give_the_results_one_queue_gives()
 {
    async fn run(members: usize) -> (Vec<Vec<Settled>>, usize, usize, usize) {
        let settling = Settling::new(members, 15);
        start_worlds(&settling.pool, members).await;
        let asked: Vec<_> = (0..5)
            .map(|i| {
                tokio::spawn(settling.settle(
                    &format!("pool-{members}-{i}"),
                    i as u64,
                    tower(i),
                    &[0, 3, 40],
                ))
            })
            .collect();
        let mut results = Vec::new();
        for asked in asked {
            results.push(
                asked
                    .await
                    .unwrap()
                    .unwrap()
                    .into_iter()
                    .map(|kept| kept.value)
                    .collect(),
            );
        }
        let (peak, made, folders) = (
            settling.peak.load(Ordering::SeqCst),
            settling.pool.engines_started(),
            settling.folders.lock().unwrap().len(),
        );
        settling.pool.stop().await;
        (results, peak, made, folders)
    }
    let (queued, queued_peak, queued_made, queued_folders) = run(1).await;
    let (pooled, pooled_peak, pooled_made, pooled_folders) = run(3).await;
    assert_eq!([queued_peak, queued_made, queued_folders], [1, 1, 1]);
    // Each child in a folder of its own.
    assert_eq!([pooled_peak, pooled_made, pooled_folders], [3, 3, 3]);
    assert_eq!(
        pooled, queued,
        "the same worlds, entities and SURVIVE lines"
    );
    // The sand fell: the settle after 40 ticks is not the placement.
    assert_ne!(pooled[0][2].blueprint, pooled[0][0].blueprint);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn one_conversations_settles_keep_their_order_and_its_state_is_not_shared_with_the_others() {
    let settling = Settling::new(3, 10);
    start_worlds(&settling.pool, 2).await;
    let asked = [
        tokio::spawn(settling.settle("ordered", 0, tower(0), &[5])),
        tokio::spawn(settling.settle("ordered", 1, tower(1), &[5])),
        tokio::spawn(settling.settle("ordered", 0, tower(0), &[9])),
        tokio::spawn(settling.settle("neighbour", 2, tower(2), &[5])),
    ];
    let mut results = Vec::new();
    for asked in asked {
        results.push(asked.await.unwrap().unwrap());
    }
    assert_eq!(
        settling.peak.load(Ordering::SeqCst),
        2,
        "the three of one conversation ran one at a time, beside the other conversation"
    );
    // The stone floor of tower(seed) reaches x = 3 + seed.
    let reach: Vec<i32> = settling
        .places
        .lock()
        .unwrap()
        .iter()
        .map(|commands| commands.split(' ').nth(4).unwrap().parse().unwrap())
        .filter(|&x| x != 5)
        .collect();
    assert_eq!(reach, [3, 4, 3]);
    assert!(!results[0][0].cached);
    assert!(
        settling.settle("ordered", 0, tower(0), &[9]).await.unwrap()[0].cached,
        "the last world asked is the one kept"
    );
    settling.pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_engine_that_does_not_start_is_dropped_and_the_next_settle_starts_another() {
    let made = Arc::new(Mutex::new(Vec::new()));
    let m = made.clone();
    let pool: Pool<Fake> = Pool::new(1, MINUTE, move |slot| {
        let mut made = m.lock().unwrap();
        made.push(slot);
        if made.len() == 1 {
            return Err(EngineError::Stopped(
                "The engine stopped (code 1): no registries".into(),
            ));
        }
        Ok(Fake {
            slot,
            died: AtomicBool::new(false),
            stopped: Arc::new(Mutex::new(Vec::new())),
            exit: None,
        })
    });
    let failed = pool
        .run("warming", |_lease: Arc<Lease<Fake>>| async { Ok(()) })
        .await
        .unwrap_err();
    assert!(failed.to_string().starts_with("The engine stopped"));
    assert!(
        pool.run("warming", |_lease: Arc<Lease<Fake>>| async { Ok(()) })
            .await
            .is_ok()
    );
    assert_eq!(made.lock().unwrap().len(), 2);
    pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn the_engine_world_shown_is_the_conversations_current_world_not_a_candidate_settled_after_it()
 {
    let settling = Settling::new(1, 0);
    let world = SettlesKey {
        world: 0,
        revision: 0,
    };
    let shown = settling.settle("shown", 0, tower(0), &[5]).await.unwrap()[0]
        .value
        .clone();
    // A build's candidate, settled and then not applied.
    settling.settle("shown", 1, tower(1), &[7]).await.unwrap();
    assert_eq!(
        settling.settles.done("shown", world, 5),
        Some(shown.clone())
    );
    assert_eq!(settling.settles.done("shown", world, 7), None);
    // A tick that fails keeps the world's settles that worked.
    let failing = settling
        .settles
        .settle("shown", world, &[9], |_latest, _missing| async {
            Err::<Vec<(i32, Settled)>, _>(EngineError::Failed("fixture".into()))
        });
    assert_eq!(
        failing.await.unwrap_err().to_string(),
        "The engine failed: fixture"
    );
    assert!(settling.settle("shown", 0, tower(0), &[5]).await.unwrap()[0].cached);
    assert_eq!(settling.settles.done("shown", world, 5), Some(shown));
    settling.pool.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn a_free_pool_member_whose_world_ended_is_replaced_at_the_next_settle() {
    let pool = SettlePool::new(common::program(), 1, MINUTE);
    pool.settle("restart", tower(4), vec![0]).await.unwrap();
    // The free member's child ends while it waits.
    let lease = pool.pool().acquire().await.unwrap();
    lease.member().stop().await;
    pool.pool().release(lease);
    assert!(
        !pool.settle("restart-2", tower(5), vec![0]).await.unwrap()[0]
            .blueprint
            .0
            .is_empty()
    );
    // The dead member was let go at take time and a new one made in its slot.
    assert_eq!(pool.engines_started(), 2);
    pool.stop().await;
}

// A warm-up the app's shutdown overtakes: its world goes back to the
// stopped pool, and nothing starts it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn a_warm_up_stopped_part_way_leaves_no_world_running_and_no_pool_behind() {
    let pool = SettlePool::new(common::program(), 1, MINUTE);
    let warming = tokio::spawn({
        let pool = pool.clone();
        async move { pool.warm().await }
    });
    pool.stop().await;
    let _ = warming.await.unwrap();
    assert_eq!(
        pool.pool().held(),
        0,
        "the stopped world was not started again"
    );
    assert_eq!(
        pool.settle("after-stop", tower(6), vec![0])
            .await
            .unwrap_err()
            .to_string(),
        "The engine pool was stopped"
    );
    // The next settle runs on a new pool of its own.
    let next = SettlePool::new(common::program(), 1, MINUTE);
    assert!(next.settle("after-stop", tower(6), vec![0]).await.is_ok());
    assert_eq!(next.engines_started(), 1);
    next.stop().await;
}

/// A fake settle of the eviction file: the value is the conversation's
/// "palette", taken from its latest settled value or made new, as
/// `runSettle` takes the palette from `chat.latest`.
type FakeSettle = Pin<Box<dyn Future<Output = Result<Vec<(i32, usize)>, EngineError>> + Send>>;

fn fake_run(
    failing: bool,
    next_palette: Arc<AtomicUsize>,
) -> impl FnOnce(Arc<Mutex<Option<usize>>>, Vec<i32>) -> FakeSettle {
    move |latest, missing| {
        Box::pin(async move {
            pause(2).await;
            if failing {
                return Err(EngineError::Failed("fixture".into()));
            }
            let mut latest = latest.lock().unwrap();
            let palette = latest.unwrap_or_else(|| next_palette.fetch_add(1, Ordering::SeqCst));
            *latest = Some(palette);
            Ok(missing.into_iter().map(|tick| (tick, palette)).collect())
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_settle_that_ends_after_its_conversation_was_dropped_leaves_no_engine_world_of_it_behind()
{
    let settles = Settles::<usize>::new();
    let palettes = Arc::new(AtomicUsize::new(0));
    let world = |seed| SettlesKey {
        world: seed,
        revision: 0,
    };
    // Asked first, and dropped by the sixteen asked next, before its engine ran.
    let dropped = settles.settle("dropped", world(0), &[0], fake_run(false, palettes.clone()));
    let others: Vec<_> = (0..KEPT_CONVERSATIONS)
        .map(|i| {
            settles.settle(
                &format!("other-{i}"),
                world(1),
                &[0],
                fake_run(false, palettes.clone()),
            )
        })
        .collect();
    let before = dropped.await.unwrap()[0].value;
    let mut last = 0;
    for other in others {
        last = other.await.unwrap()[0].value;
    }
    assert_eq!(
        settles.done("dropped", world(0), 0),
        None,
        "the conversation was dropped"
    );
    // A kept conversation's next settle takes its palette from its latest settle.
    let again = settles.settle(
        &format!("other-{}", KEPT_CONVERSATIONS - 1),
        world(2),
        &[0],
        fake_run(false, palettes.clone()),
    );
    assert_eq!(again.await.unwrap()[0].value, last);
    let next = settles
        .settle("dropped", world(2), &[0], fake_run(false, palettes.clone()))
        .await
        .unwrap()[0]
        .value;
    assert_ne!(
        next, before,
        "the dropped conversation has no latest settle"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_settle_that_fails_after_its_conversation_was_dropped_does_not_take_the_place_of_a_kept_one()
 {
    let settles = Settles::<usize>::new();
    let palettes = Arc::new(AtomicUsize::new(0));
    let world = |seed| SettlesKey {
        world: seed,
        revision: 0,
    };
    let failing = settles.settle(
        "failing",
        world(100),
        &[0],
        fake_run(true, palettes.clone()),
    );
    let others: Vec<_> = (0..KEPT_CONVERSATIONS as u64)
        .map(|i| {
            settles.settle(
                &format!("kept-{i}"),
                world(i),
                &[0],
                fake_run(false, palettes.clone()),
            )
        })
        .collect();
    assert_eq!(
        failing.await.unwrap_err().to_string(),
        "The engine failed: fixture"
    );
    for other in others {
        other.await.unwrap();
    }
    // Sixteen are kept: settling one again drops none of them.
    assert!(
        settles
            .settle("kept-1", world(1), &[0], fake_run(false, palettes.clone()))
            .await
            .unwrap()[0]
            .cached
    );
    assert!(
        settles
            .settle("kept-0", world(0), &[0], fake_run(false, palettes.clone()))
            .await
            .unwrap()[0]
            .cached,
        "the oldest kept conversation still has its settle"
    );
}

/// `count` observer clocks on stone, every one firing each tick for good:
/// a world that keeps a settle or an experiment busy.
fn clocks(count: i32) -> PlaceCommands {
    let mut commands = String::new();
    for i in 0..count {
        let (x, z) = (4 * (i % 32), 2 * (i / 32));
        commands.push_str(&format!(
            "setblock {x} 64 {z} minecraft:stone\nsetblock {} 64 {z} minecraft:stone\nsetblock {x} 65 {z} minecraft:observer[facing=east]\nsetblock {} 65 {z} minecraft:observer[facing=west]\n",
            x + 1,
            x + 1
        ));
    }
    PlaceCommands(commands)
}

/// A settle of a busy world on `pool` whose child's pid is sent before it
/// runs, as the settle's own job runs it.
fn busy_settle(
    pool: &SettlePool,
    key: &str,
) -> (
    tokio::task::JoinHandle<Result<Vec<Settled>, EngineError>>,
    tokio::sync::oneshot::Receiver<u32>,
) {
    let (pid, told) = tokio::sync::oneshot::channel();
    let job = pool.run(key, move |lease: Arc<Lease<ChildEngine>>| async move {
        let _ = pid.send(lease.member().pid());
        settle_on(
            lease.member(),
            &clocks(1024),
            &[70000],
            &CancellationToken::new(),
        )
        .await
    });
    (tokio::spawn(job), told)
}

// What the children are for. A child killed from outside in the middle of a
// settle's request (the system's out-of-memory killer, a user) fails the
// settle with today's sentence, `The engine stopped (SIGKILL): ` and its
// stderr's tail (on Windows `code 1`, `taskkill /F`'s), and the pool
// replaces it for the next settle.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn a_child_killed_from_outside_mid_settle_fails_it_plainly_and_is_replaced() {
    let pool = SettlePool::new(common::program(), 1, MINUTE);
    let (settling, told) = busy_settle(&pool, "killed");
    let pid = told.await.unwrap();
    // Well inside its ticks.
    pause(500).await;
    assert!(!settling.is_finished(), "the settle ended before the kill");
    common::kill(pid);
    let error = settling.await.unwrap().unwrap_err();
    assert_eq!(
        error.to_string(),
        format!("The engine stopped ({}): ", common::KILLED_FROM_OUTSIDE)
    );
    assert!(common::gone_within(pid, common::AT_ONCE).await);
    assert!(
        !pool.settle("killed", tower(1), vec![0]).await.unwrap()[0]
            .blueprint
            .0
            .is_empty()
    );
    assert_eq!((pool.engines_started(), pool.pool().held()), (2, 1));
    pool.stop().await;
}

// A settle past the pool's wall-clock budget fails at once with today's
// sentence, its child is stopped (gone at once, where an in-process
// world ran on to the end of its ticks), and the next settle runs on a new
// child at once.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn a_settle_past_the_wall_budget_stops_its_child_at_once_and_the_next_runs_on_a_new_one() {
    let pool = SettlePool::new(common::program(), 1, Duration::from_millis(1500));
    let (settling, told) = busy_settle(&pool, "wall");
    let pid = told.await.unwrap();
    let error = settling.await.unwrap().unwrap_err();
    let ended = std::time::Instant::now();
    assert_eq!(
        error.to_string(),
        "The settle ran past its 1.5-second wall-clock budget (MCBENCH_VERIFY_MAX_WALL_MS) and was stopped: the build is too active to settle in time. Ask for fewer ticks or make the build less active."
    );
    assert!(
        common::gone_within(pid, common::AT_ONCE).await,
        "the child {pid} still runs"
    );
    assert!(
        !pool.settle("wall", tower(2), vec![0]).await.unwrap()[0]
            .blueprint
            .0
            .is_empty()
    );
    // At once, where the stopped settle's many ticks would hold the member:
    // the next settle's new child starts and settles inside a bound a
    // starved machine meets.
    assert!(ended.elapsed() < Duration::from_secs(30));
    assert_eq!(pool.engines_started(), 2);
    pool.stop().await;
}

// A child that runs out of memory (here a limit of its address space set on
// it, `ulimit -v`, below what a large placement needs) dies alone: the
// settle fails with the child's exit, this process goes on, and the next
// settle runs on a new child.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[cfg_attr(
    not(all(mcbench_tables, mcbench_program)),
    ignore = "needs Minecraft's tables and the children's program: set MCBENCH_TABLES and MCBENCH_ENGINE_PROGRAM at build time"
)]
async fn a_child_past_its_memory_limit_dies_alone_and_is_replaced() {
    let folder = common::scratch("memory");
    let real = common::program();
    let limited = common::fake(
        &folder,
        "limited.sh",
        &format!(
            "ulimit -v {}\nexec {} \"$@\"\n",
            MEMORY_LIMIT_KIB,
            real.binary().display()
        ),
    );
    let limited = mcbench_engine::EngineProgram::new(limited.binary(), real.tables());
    // The script is started once first, past a moment it may be busy.
    common::spawn(&limited).stop().await;
    let pool = SettlePool::new(limited, 1, MINUTE);
    // A small world fits.
    assert!(pool.settle("memory", tower(0), vec![0]).await.is_ok());
    // A large one does not: 16 million blocks.
    let large = PlaceCommands(
        "fill 0 0 0 255 255 255 minecraft:stone\nfill 256 0 0 511 255 255 minecraft:stone\nfill 0 0 256 255 255 511 minecraft:stone\nfill 256 0 256 511 255 511 minecraft:stone\n".into(),
    );
    let error = pool.settle("memory", large, vec![0]).await.unwrap_err();
    eprintln!("the child past its memory limit: {error}");
    assert!(
        error.to_string().starts_with("The engine stopped ("),
        "{error}"
    );
    // This process lives on, and so does the pool.
    assert!(pool.settle("memory", tower(1), vec![0]).await.is_ok());
    assert_eq!(pool.engines_started(), 2);
    pool.stop().await;
    std::fs::remove_dir_all(&folder).unwrap();
}

/// The address space a child of the memory test may use, in KiB: the
/// engine's 1 GiB stack (reserved, not used) and its tables fit, a world of
/// 16 million blocks does not.
#[cfg(target_os = "linux")]
const MEMORY_LIMIT_KIB: u64 = 2 * 1024 * 1024;
