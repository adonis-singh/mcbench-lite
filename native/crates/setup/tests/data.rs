//! The shipped data's bundle unpacked into a data folder: once, whole, only
//! when it is the lock's.

mod common;

use common::{names, scratch, sha256};
use mcbench_setup::Data;
use mcbench_setup::bundle::{FILES, Lock, tar};

fn bundle() -> (Vec<u8>, Lock) {
    let files: Vec<(&str, Vec<u8>)> = FILES
        .iter()
        .map(|n| (*n, format!("the file {n}\n").repeat(100).into_bytes()))
        .collect();
    let list: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let archive = tar(&list).unwrap();
    let bundle = ruzstd::encoding::compress_to_vec(
        &archive[..],
        ruzstd::encoding::CompressionLevel::Fastest,
    );
    let lock = Lock {
        minecraft: "1.21.10".into(),
        sha256: sha256(&bundle),
        size: bundle.len() as u64,
        url: "https://example.invalid/bundle".into(),
        zstd: "test".into(),
        files: files
            .iter()
            .map(|(n, b)| (n.to_string(), sha256(b), b.len() as u64))
            .collect(),
    };
    (bundle, lock)
}

#[test]
fn the_bundle_is_unpacked_once_into_a_folder_named_by_its_version_and_sha256() {
    let (bundle, lock) = bundle();
    let folder = scratch("data-unpack");
    std::fs::create_dir_all(folder.join("minecraft/1.21.9-0123456789abcdef")).unwrap();
    let data = Data::unpack(&bundle, &lock, &folder).unwrap();
    let id = format!("1.21.10-{}", &lock.sha256[..16]);
    assert_eq!(data.folder(), folder.join("minecraft").join(&id));
    assert_eq!(
        names(&folder.join("minecraft")),
        [id.as_str()],
        "another version's folder is removed"
    );
    let mut want: Vec<String> = FILES.iter().map(|n| n.to_string()).collect();
    want.sort();
    assert_eq!(names(data.folder()), want);
    assert_eq!(
        data.text("blocks.txt").unwrap(),
        "the file blocks.txt\n".repeat(100)
    );
    // A later start finds the folder and reads nothing of the bundle.
    assert_eq!(Data::unpack(b"not a bundle", &lock, &folder).unwrap(), data);
}

#[test]
fn a_bundle_that_is_not_the_locks_is_refused_and_nothing_is_written() {
    let (mut bundle, lock) = bundle();
    let last = bundle.len() - 1;
    bundle[last] ^= 1;
    let folder = scratch("data-refused");
    assert!(
        Data::unpack(&bundle, &lock, &folder)
            .unwrap_err()
            .starts_with("The Minecraft data bundle has sha256")
    );
    assert!(!folder.join("minecraft").exists());
}

#[test]
fn the_tracked_lock_file_names_every_shipped_file() {
    let lock = Lock::parse(mcbench_setup::bundle::LOCK).unwrap();
    assert_eq!(lock.minecraft, "1.21.10");
    assert_eq!(lock.files.len(), FILES.len());
}

// The shipped bundle itself (`--features shipped-data`): unpacked, its
// tables are the engine's.
#[cfg(feature = "shipped-data")]
#[test]
fn the_shipped_bundle_unpacks_to_the_files_the_lock_names() {
    let folder = scratch("data-shipped");
    let data = Data::shipped(&folder).unwrap();
    let lock = Lock::parse(mcbench_setup::bundle::LOCK).unwrap();
    for (name, sha, size) in &lock.files {
        let bytes = data.read(name).unwrap();
        assert_eq!(
            (&sha256(&bytes), bytes.len() as u64),
            (sha, *size),
            "{name}"
        );
    }
    data.manifest().unwrap();
}
