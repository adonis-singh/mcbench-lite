//! The fixture guard (`tests/common/fixture.rs`), with no fixture: a folder
//! holding `settings.json`, `auth-token` or `backups` is refused with the
//! guard's sentence; a folder with none of them is accepted.

#[path = "common/fixture.rs"]
mod fixture;

use std::path::PathBuf;

fn folder(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("fixture-guard")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("mcbench.sqlite3"), "").unwrap();
    dir
}

#[test]
fn a_fixture_with_a_key_file_is_refused_and_one_without_is_accepted() {
    for entry in fixture::LEFT_OUT {
        let dir = folder(entry);
        if entry == "backups" {
            std::fs::create_dir(dir.join(entry)).unwrap();
        } else {
            std::fs::write(dir.join(entry), "").unwrap();
        }
        let refused = std::panic::catch_unwind(|| fixture::guard(&dir))
            .expect_err("the guard accepted a fixture holding a key file");
        let sentence = refused.downcast_ref::<String>().unwrap();
        assert_eq!(
            sentence,
            &format!(
                "the fixture {} holds {entry}: hand the tests a fixture cloned without settings.json, auth-token and backups",
                dir.display()
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
    let clean = folder("clean");
    fixture::guard(&clean);
    std::fs::remove_dir_all(&clean).unwrap();
}
