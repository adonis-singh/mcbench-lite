//! `test/storage.test.js` (9 tests), the store's half of
//! `test/storage-limits.test.js` (the attachment, round attachment, image
//! and world limits and the disk reserve; its three `test_build` tests are
//! `mcbench-agent`'s, and its build test's agent half too) and
//! `test/store-missing-database.test.js` (2), ported.

mod common;

use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex};

use common::{SmallDisk, TestWorld};
use mcbench_store::json::Json;
use mcbench_store::{
    Host, ImageData, Limits, NewConversation, Storage, SystemHost, expendable, object,
};

const MIB: u64 = 1024 * 1024;
const KIB: u64 = 1024;

fn write_file(root: &Path, relative: &str, bytes: usize) {
    let file = root.join(relative);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(file, vec![1u8; bytes]).unwrap();
}

fn storage(root: &Path, chat_max_bytes: u64) -> (Arc<Storage>, Arc<SmallDisk>) {
    let host = Arc::new(SmallDisk(Mutex::new(None)));
    (
        Arc::new(Storage::new(
            root.to_path_buf(),
            common::limits(chat_max_bytes),
            host.clone(),
        )),
        host,
    )
}

#[test]
fn a_chat_holds_what_du_counts_in_its_four_folders_plus_what_was_reserved_since() {
    let root = common::scratch("du");
    let (storage, _) = storage(&root, 4 * MIB);
    let id = "measured-chat";
    write_file(&root, &format!("sandbox/{id}/data.bin"), MIB as usize);
    write_file(&root, &format!("images/{id}/shot.jpg"), MIB as usize / 2);
    write_file(&root, "sandbox/other-chat/data.bin", MIB as usize);
    let first = storage.used(id).unwrap();
    assert!(
        first >= (1.5 * MIB as f64) as i64 && first < (1.6 * MIB as f64) as i64,
        "measured {first}"
    );
    storage.reserve(id, MIB / 4, true).unwrap();
    assert_eq!(storage.used(id).unwrap(), first + MIB as i64 / 4);
    std::fs::remove_file(root.join(format!("sandbox/{id}/data.bin"))).unwrap();
    storage.measure(id).wait().unwrap();
    let second = storage.used(id).unwrap();
    assert!(
        second >= MIB as i64 / 2 && second < (0.6 * MIB as f64) as i64,
        "measured {second}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_reservation_made_while_a_measurement_runs_stays_counted() {
    let root = common::scratch("racing");
    let (storage, _) = storage(&root, 4 * MIB);
    let id = "racing-chat";
    write_file(&root, &format!("attachments/{id}/file.bin"), MIB as usize);
    let running = storage.measure(id);
    storage.reserve(id, 100_000, true).unwrap();
    running.wait().unwrap();
    let held = storage.used(id).unwrap();
    assert!(
        held >= MIB as i64 + 100_000 && held < (1.1 * MIB as f64) as i64 + 100_000,
        "measured {held}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn after_a_measurement_fails_the_next_write_measures_again_and_goes_on() {
    use std::os::unix::fs::PermissionsExt;
    let root = common::scratch("failing");
    let (storage, _) = storage(&root, 4 * MIB);
    let id = "failing-chat";
    write_file(&root, &format!("worlds/{id}/chunk.json"), MIB as usize);
    storage.used(id).unwrap();
    // A folder the walk cannot read stands for today's failing du.
    let locked = root.join(format!("worlds/{id}/locked"));
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let error = storage.measure(id).wait().unwrap_err();
    assert!(error.contains("du did not measure"), "{error}");
    let error = storage.reserve(id, 10, true).unwrap_err();
    assert!(
        error.message.contains("du did not measure"),
        "{}",
        error.message
    );
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
    storage.reserve(id, 10, true).unwrap();
    let held = storage.used(id).unwrap();
    assert!(
        held >= MIB as i64 + 10 && held < (1.1 * MIB as f64) as i64,
        "measured {held}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

/// A host that holds each background job until the test runs it. Today's
/// test refunds after one `setImmediate`, while du runs; here the refund
/// lands between the measurement's start and its walk whatever the machine's
/// load, where a thread could finish the walk first.
struct HeldJobs(Mutex<Vec<Box<dyn FnOnce() + Send>>>);

impl Host for HeldJobs {
    fn now_ms(&self) -> i64 {
        SystemHost.now_ms()
    }
    fn uuid(&self) -> String {
        SystemHost.uuid()
    }
    fn random_int(&self, below: u32) -> u32 {
        SystemHost.random_int(below)
    }
    fn free_bytes(&self, folder: &Path) -> Result<u64, String> {
        SystemHost.free_bytes(folder)
    }
    fn background(&self, job: Box<dyn FnOnce() + Send>) {
        self.0.lock().unwrap().push(job);
    }
}

#[test]
fn a_refund_while_a_measurement_runs_does_not_take_off_what_du_never_saw() {
    let root = common::scratch("refunded");
    let host = Arc::new(HeldJobs(Mutex::new(Vec::new())));
    let storage = Arc::new(Storage::new(
        root.clone(),
        common::limits(4 * MIB),
        host.clone(),
    ));
    let id = "refunded-chat";
    write_file(&root, &format!("images/{id}/shot.jpg"), MIB as usize);
    storage.reserve(id, 100_000, true).unwrap();
    let running = storage.measure(id);
    // The 100 000 bytes were never written.
    storage.refund(id, 100_000);
    let held = std::mem::take(&mut *host.0.lock().unwrap());
    assert_eq!(held.len(), 1, "one measurement was started");
    for job in held {
        job();
    }
    running.wait().unwrap();
    assert!(storage.used(id).unwrap() >= MIB as i64);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_refund_after_the_chat_was_forgotten_counts_nothing() {
    let root = common::scratch("forgotten");
    let (storage, _) = storage(&root, 4 * MIB);
    let id = "forgotten-chat";
    write_file(&root, &format!("images/{id}/shot.jpg"), MIB as usize);
    storage.reserve(id, 100_000, true).unwrap();
    storage.forget(id);
    storage.refund(id, 100_000);
    let held = storage.used(id).unwrap();
    assert!(
        held >= MIB as i64 && held < (1.1 * MIB as f64) as i64,
        "measured {held}"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_write_past_the_chats_budget_is_refused_with_507_and_nothing_is_counted() {
    let root = common::scratch("full");
    let (storage, _) = storage(&root, 4 * MIB);
    let id = "full-chat";
    storage.reserve(id, 3 * MIB, true).unwrap();
    let error = storage.reserve(id, 2 * MIB, true).unwrap_err();
    assert_eq!(error.status, Some(507));
    assert!(error.message.contains("This chat's storage is full: its attachments, images, world and /work hold 3.0 MiB of its 4.0 MiB budget (MCBENCH_CHAT_MAX_BYTES) and this write needs 2.0 MiB"), "{}", error.message);
    assert_eq!(storage.used(id).unwrap(), 3 * MIB as i64);
    storage.reserve(id, MIB, true).unwrap();
    assert_eq!(
        storage.refusal(id).unwrap(),
        None,
        "exactly at the budget is within it"
    );
    storage.refund(id, 4 * MIB);
    write_file(&root, &format!("sandbox/{id}/big.bin"), 5 * MIB as usize);
    assert_eq!(storage.refusal(id).unwrap(), None);
    storage.measure(id).wait().unwrap();
    let refusal = storage.refusal(id).unwrap().unwrap();
    assert!(
        regex::Regex::new(r"storage is full: .* hold 5\.0 MiB of its 4\.0 MiB budget")
            .unwrap()
            .is_match(&refusal),
        "{refusal}"
    );
    assert!(
        storage
            .reserve(id, 1, true)
            .unwrap_err()
            .message
            .contains("storage is full")
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn under_the_disks_reserve_nothing_is_written_whatever_the_chat_holds() {
    let root = common::scratch("small-disk");
    let (storage, host) = storage(&root, 4 * MIB);
    let id = "small-chat";
    *host.0.lock().unwrap() = Some(1000 * 4096);
    storage.drop_reading();
    let error = storage.reserve(id, 10, true).unwrap_err();
    assert_eq!(error.status, Some(507));
    assert_eq!(
        error.message,
        "The disk holding the data folder has 3.9 MiB free and this write needs 1 KiB; mcbench-lite keeps 2.00 GiB free (MCBENCH_DISK_RESERVE_BYTES) and saves nothing new until space is freed."
    );
    assert!(
        storage
            .refusal(id)
            .unwrap()
            .unwrap()
            .contains("MCBENCH_DISK_RESERVE_BYTES")
    );
    storage.reserve(id, 10, false).unwrap();
    *host.0.lock().unwrap() = None;
    storage.drop_reading();
    assert_eq!(storage.refusal(id).unwrap(), None);
    assert!(storage.free_bytes().unwrap() > storage.limits().disk_reserve_bytes as i64);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn expendable_files_go_oldest_first_then_by_name_never_a_referenced_one_down_to_the_number_kept() {
    let ages = |name: &str| match name {
        "a" => 5.0,
        "b" | "c" => 1.0,
        "d" => 3.0,
        _ => 9.0,
    };
    let names = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let set = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<HashSet<_>>();
    assert_eq!(
        expendable(&names(&["a", "b", "c", "d", "e"]), &set(&[]), 2, ages),
        names(&["b", "c", "d"])
    );
    assert_eq!(
        expendable(&names(&["e", "d", "c", "b", "a"]), &set(&["b"]), 2, ages),
        names(&["c", "d", "a"])
    );
    assert!(expendable(&names(&["a", "b", "c"]), &set(&["a", "b", "c"]), 1, ages).is_empty());
    assert!(expendable(&names(&["a", "b"]), &set(&[]), 2, ages).is_empty());
    assert_eq!(
        expendable(&names(&["a", "b", "c"]), &set(&["b"]), 0, |_| panic!(
            "keeping none needs no ages"
        )),
        names(&["a", "c"])
    );
}

#[test]
fn the_limits_are_operator_settings_and_refuse_a_value_that_is_not_an_integer() {
    for (name, value) in [
        ("MCBENCH_CHAT_MAX_BYTES", "4GB"),
        ("MCBENCH_DISK_RESERVE_BYTES", "-1"),
        ("MCBENCH_REPORTS_KEPT", "1.5"),
    ] {
        let error = Limits::from_env(|key| (key == name).then(|| value.to_string())).unwrap_err();
        assert!(
            error.starts_with(&format!("{name} must be an integer of at least")),
            "{error}"
        );
    }
    let limits =
        Limits::from_env(|key| (key == "MCBENCH_CHAT_MAX_BYTES").then(|| (4 * MIB).to_string()))
            .unwrap();
    assert_eq!(
        limits,
        Limits {
            chat_max_bytes: 4 * MIB,
            disk_reserve_bytes: 2 * 1024 * MIB,
            reports_kept: 100
        }
    );
}

// ---- storage-limits.test.js (the store's half) ------------------------------

fn limited(name: &str) -> (std::path::PathBuf, mcbench_store::Store, Arc<SmallDisk>) {
    let root = common::scratch(name);
    let host = Arc::new(SmallDisk(Mutex::new(None)));
    let store = common::open(
        &root,
        host.clone(),
        Limits {
            chat_max_bytes: MIB,
            disk_reserve_bytes: 2 * 1024 * MIB,
            reports_kept: 3,
        },
    )
    .unwrap();
    (root, store, host)
}

fn file(name: &str, bytes: u64) -> Json {
    use base64::Engine;
    object! { "name" => name, "data" => format!("data:application/octet-stream;base64,{}", base64::engine::general_purpose::STANDARD.encode(vec![7u8; bytes as usize])) }
}

fn full(error: &mcbench_store::Error) -> bool {
    error.status == Some(507)
        && regex::Regex::new(r"This chat's storage is full: .*MCBENCH_CHAT_MAX_BYTES")
            .unwrap()
            .is_match(&error.message)
}

fn listing(root: &Path, parts: &[&str]) -> usize {
    let dir = parts
        .iter()
        .fold(root.to_path_buf(), |dir, part| dir.join(part));
    if dir.exists() {
        std::fs::read_dir(dir).unwrap().count()
    } else {
        0
    }
}

fn chat(store: &mcbench_store::Store) -> String {
    store
        .create_conversation("openrouter", "test/model", NewConversation::default())
        .unwrap()
        .lock()
        .unwrap()
        .id
        .0
        .clone()
}

#[test]
fn attachments_that_do_not_fit_the_chats_budget_are_refused_with_507_and_none_is_written() {
    let (root, store, _) = limited("limits-attachments");
    let id = chat(&store);
    store
        .save_attachments(&id, &[file("first.bin", 400 * KIB)])
        .unwrap();
    assert!(full(
        &store
            .save_attachments(
                &id,
                &[file("second.bin", 300 * KIB), file("third.bin", 400 * KIB)]
            )
            .unwrap_err()
    ));
    assert_eq!(listing(&root, &["attachments", &id]), 1);
    store
        .save_attachments(&id, &[file("fits.bin", 300 * KIB)])
        .unwrap();
    assert_eq!(listing(&root, &["attachments", &id]), 2);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_bench_rounds_attachment_is_refused_for_a_chat_it_does_not_fit() {
    let (root, store, _) = limited("limits-round");
    let attachment = store
        .save_attachments(&SystemHost.uuid_text(), &[file("round.bin", 700 * KIB)])
        .unwrap()
        .remove(0);
    let member = chat(&store);
    store
        .save_attachments(&member, &[file("own.bin", 400 * KIB)])
        .unwrap();
    assert!(full(
        &store.share_attachment(&attachment, &member).unwrap_err()
    ));
    assert_eq!(listing(&root, &["attachments", &member]), 1);
    assert_eq!(
        store
            .share_attachment(&attachment, &chat(&store))
            .unwrap()
            .size,
        700 * KIB
    );
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

trait UuidText {
    fn uuid_text(&self) -> String;
}
impl UuidText for SystemHost {
    fn uuid_text(&self) -> String {
        mcbench_store::Host::uuid(self)
    }
}

#[test]
fn images_that_do_not_fit_are_refused_before_any_is_written() {
    let (root, store, _) = limited("limits-images");
    let id = chat(&store);
    let random =
        |bytes: u64| -> Vec<u8> { (0..bytes).map(|i| (i * 2654435761 % 251) as u8).collect() };
    let urls = store
        .save_images(&id, &[ImageData::Png(random(500 * KIB))], "")
        .unwrap();
    assert!(
        regex::Regex::new(&format!("^/api/images/{id}/.*\\.png$"))
            .unwrap()
            .is_match(&urls[0])
    );
    assert!(full(
        &store
            .save_images(
                &id,
                &[
                    ImageData::Png(random(200 * KIB)),
                    ImageData::Png(random(400 * KIB))
                ],
                ""
            )
            .unwrap_err()
    ));
    assert_eq!(listing(&root, &["images", &id]), 1);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_build_whose_world_does_not_fit_fails_and_changes_nothing() {
    let (root, store, _) = limited("limits-world");
    let id = chat(&store);
    // 36 dense chunks, each different: about 1.6 MiB.
    let mut world = TestWorld::default();
    for c in 0..36 {
        for i in 0..4096 {
            let id = if i == 0 {
                format!("minecraft:block_{c}")
            } else {
                "minecraft:stone".to_string()
            };
            world
                .blocks
                .insert((c * 16 + (i % 16), i / 256, (i / 16) % 16), id);
        }
    }
    let error = store.persist_world(&id, &world.into_chat()).unwrap_err();
    assert!(
        error.message.contains("This chat's storage is full"),
        "{}",
        error.message
    );
    assert_eq!(
        listing(&root, &["worlds", &id, "chunks"]),
        0,
        "the chunks written before the refusal are removed"
    );
    assert!(
        store.storage().used(&id).unwrap() < 64 * KIB as i64,
        "and refunded"
    );
    let small = TestWorld::default()
        .with(0, 0, 0, "minecraft:stone")
        .into_chat();
    store.persist_world(&id, &small).unwrap();
    assert_eq!(
        listing(&root, &["worlds", &id, "chunks"]),
        1,
        "a build that fits is written before it is applied"
    );
    let live = store.conversation(&id).unwrap().unwrap();
    let mut conversation = live.lock().unwrap();
    conversation.world = small;
    store.save(&mut conversation).unwrap();
    assert_eq!(
        listing(&root, &["worlds", &id, "chunks"]),
        1,
        "and its save writes no chunk again"
    );
    drop(conversation);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn under_the_disks_reserve_attachments_images_and_world_saves_are_refused() {
    let (root, store, host) = limited("limits-disk");
    let id = chat(&store);
    *host.0.lock().unwrap() = Some(100 * 4096);
    store.storage().drop_reading();
    let disk = |error: mcbench_store::Error| {
        error.status == Some(507) && error.message.contains("MCBENCH_DISK_RESERVE_BYTES")
    };
    assert!(disk(
        store
            .save_attachments(&id, &[file("small.bin", 10)])
            .unwrap_err()
    ));
    assert!(disk(
        store
            .save_images(&id, &[ImageData::Png(b"png".to_vec())], "")
            .unwrap_err()
    ));
    let candidate = TestWorld::default()
        .with(0, 0, 0, "minecraft:stone")
        .into_chat();
    assert!(disk(store.persist_world(&id, &candidate).unwrap_err()));
    assert_eq!(listing(&root, &["worlds", &id, "chunks"]), 0);
    *host.0.lock().unwrap() = None;
    store.storage().drop_reading();
    store.persist_world(&id, &candidate).unwrap();
    assert_eq!(listing(&root, &["worlds", &id, "chunks"]), 1);
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}

// ---- store-missing-database.test.js ------------------------------------------

#[test]
fn a_missing_database_beside_worlds_fails_before_it_is_created_and_the_worlds_are_kept() {
    let root = common::scratch("missing");
    let chunk = root
        .join("worlds")
        .join("aaaaaaaa-0000-0000-0000-000000000000")
        .join("chunks")
        .join("c.json");
    std::fs::create_dir_all(chunk.parent().unwrap()).unwrap();
    std::fs::write(&chunk, "[]").unwrap();
    let error = common::open(&root, Arc::new(SystemHost), common::limits(MIB))
        .map(|_| ())
        .unwrap_err();
    assert_eq!(
        error.message,
        format!(
            "The database {} is missing, but {} holds worlds. Restore the database first: starting without it would create an empty one and delete those worlds.",
            root.join("mcbench.sqlite3").display(),
            root.join("worlds").display()
        )
    );
    assert!(!root.join("mcbench.sqlite3").exists());
    assert_eq!(std::fs::read_to_string(&chunk).unwrap(), "[]");

    // A data folder whose worlds folder is empty gets a new database.
    std::fs::remove_dir_all(chunk.parent().unwrap().parent().unwrap()).unwrap();
    let store = common::open(&root, Arc::new(SystemHost), common::limits(MIB)).unwrap();
    assert!(root.join("mcbench.sqlite3").exists());
    assert!(store.list_conversations().unwrap().is_empty());
    drop(store);
    std::fs::remove_dir_all(root).unwrap();
}
