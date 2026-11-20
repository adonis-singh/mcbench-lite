//! The update check, download and install against a local file server, with
//! key pairs made at test time by the `minisign` crate (pure Rust).

mod common;

use std::collections::HashMap;
use std::io::Cursor;
use std::path::Path;

use common::{STALL, client, serve};

use mcbench_updater::{
    Feed, Installed, Start, Target, UpdateKey, UpdateState, check, check_once, download,
    hold_install_at, install, start_with,
};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

struct Key {
    pair: minisign::KeyPair,
}

impl Key {
    fn new() -> Key {
        Key {
            pair: minisign::KeyPair::generate_unencrypted_keypair().unwrap(),
        }
    }
    fn base64(&self) -> String {
        self.pair.pk.to_base64()
    }
    fn trusted(&self) -> UpdateKey {
        UpdateKey::from_base64(&self.base64()).unwrap()
    }
    fn sign(&self, data: &[u8], comment: &str) -> String {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(data),
            Some(comment),
            None,
        )
        .unwrap()
        .into_string()
    }
    /// The key-chain entry in which this key signs `next`.
    fn signs_key(&self, next: &Key) -> serde_json::Value {
        serde_json::json!({
            "public_key": next.base64(),
            "signature": self.sign(next.base64().as_bytes(), "mcbench-lite update key"),
        })
    }
}

fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn comment(version: &str, target: &str, name: &str, data: &[u8]) -> String {
    format!(
        "version:{version}\ttarget:{target}\tfile:{name}\tsize:{}\tsha256:{}",
        data.len(),
        sha256(data)
    )
}

/// A `latest.json` asset entry for `data`, signed by `key`.
fn entry(key: &Key, version: &str, target: &str, name: &str, data: &[u8]) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "size": data.len(),
        "sha256": sha256(data),
        "signature": key.sign(data, &comment(version, target, name, data)),
    })
}

const TARGET: Target = Target::LinuxX86_64Appimage;
const NAME: &str = "mcbench-lite-x86_64.AppImage";
const NEW: &[u8] = b"the new AppImage";

/// A release server with `latest` at `/download/latest.json` and `files`
/// beside it.
async fn release(latest: serde_json::Value, files: &[(&str, &[u8])]) -> String {
    let mut map: HashMap<String, Vec<u8>> = files
        .iter()
        .map(|(n, d)| (format!("/download/{n}"), d.to_vec()))
        .collect();
    map.insert(
        "/download/latest.json".into(),
        serde_json::to_vec(&latest).unwrap(),
    );
    format!("{}/download/latest.json", serve(map).await)
}

fn latest(
    version: &str,
    keys: Vec<serde_json::Value>,
    assets: serde_json::Value,
) -> serde_json::Value {
    serde_json::json!({ "version": version, "keys": keys, "assets": assets })
}

/// The check and download of `url` for `TARGET` from version 1.0.0,
/// trusting `trusted`, into a scratch folder; the error sentence or the
/// downloaded bytes.
async fn fetch(url: &str, trusted: &Key) -> Result<Vec<u8>, String> {
    let dir = tempfile::tempdir().unwrap();
    let cancel = CancellationToken::new();
    let release = check(&client(), url, "1.0.0", TARGET, &trusted.trusted(), &cancel)
        .await?
        .expect("a newer release")
        .0;
    let target = dir.path().join("mcbench-lite.AppImage");
    let downloaded = download(&client(), &release, &target, &cancel).await?;
    Ok(std::fs::read(downloaded.path()).unwrap())
}

#[tokio::test]
async fn an_update_downloads_verifies_and_replaces_the_appimage() {
    let key = Key::new();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ TARGET.name(): entry(&key, "1.1.0", TARGET.name(), NAME, NEW) }),
        ),
        &[(NAME, NEW)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("mcbench-lite.AppImage");
    std::fs::write(&image, "the old AppImage").unwrap();
    let installed = Installed::locate(
        Path::new("/tmp/.mount/usr/bin/mcbench-lite"),
        TARGET,
        Some(image.clone()),
    )
    .unwrap();
    let cancel = CancellationToken::new();
    let (release, _) = check(&client(), &url, "1.0.0", TARGET, &key.trusted(), &cancel)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (
            release.version.as_str(),
            release.name.as_str(),
            release.size
        ),
        ("1.1.0", NAME, NEW.len() as u64)
    );
    let downloaded = download(&client(), &release, &installed.path, &cancel)
        .await
        .unwrap();
    assert_eq!(
        downloaded.path(),
        dir.path().join(".mcbench-lite.AppImage.update")
    );
    assert_eq!(std::fs::read(&image).unwrap(), b"the old AppImage");
    install(&downloaded, &installed).unwrap();
    drop(downloaded);
    assert_eq!(std::fs::read(&image).unwrap(), NEW);
    assert_eq!(
        std::fs::read(dir.path().join("mcbench-lite.AppImage.old")).unwrap(),
        b"the old AppImage"
    );
    mcbench_updater::remove_leftover(&installed).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[tokio::test]
async fn the_same_version_is_current() {
    let key = Key::new();
    let url = release(latest("1.0.0", vec![], serde_json::json!({})), &[]).await;
    let got = check(
        &client(),
        &url,
        "1.0.0",
        TARGET,
        &key.trusted(),
        &CancellationToken::new(),
    )
    .await;
    assert!(matches!(got, Ok(None)));
}

#[tokio::test]
async fn a_macos_bundle_update_end_to_end() {
    let key = Key::new();
    let mut archive = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut archive, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        for name in [
            "mcbench-lite.app/Contents/MacOS/mcbench-lite",
            "mcbench-lite.app/Contents/Resources/icon.icns",
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_mode(0o755);
            tar.append_data(&mut header, name, &b"new"[..]).unwrap();
        }
        let mut header = tar::Header::new_gnu();
        header.set_size(6);
        header.set_mode(0o644);
        tar.append_data(
            &mut header,
            "mcbench-lite.app/Contents/Resources/version",
            &b"2.0.0\n"[..],
        )
        .unwrap();
        tar.into_inner().unwrap().finish().unwrap();
    }
    let target = Target::MacosAarch64;
    let name = target.asset();
    let url = release(
        latest(
            "2.0.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "2.0.0", target.name(), name, &archive) }),
        ),
        &[(name, &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let exe = dir
        .path()
        .join("mcbench-lite.app/Contents/MacOS/mcbench-lite");
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::create_dir_all(dir.path().join("mcbench-lite.app/Contents/Resources")).unwrap();
    std::fs::write(&exe, "old").unwrap();
    let installed = Installed::locate(&exe, target, None).unwrap();
    let source = installed.install_source();
    std::fs::write(&source, "script\n").unwrap();
    let cancel = CancellationToken::new();
    let (release, _) = check(&client(), &url, "1.9.0", target, &key.trusted(), &cancel)
        .await
        .unwrap()
        .unwrap();
    let downloaded = download(&client(), &release, &installed.path, &cancel)
        .await
        .unwrap();
    install(&downloaded, &installed).unwrap();
    drop(downloaded);
    assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
    assert_eq!(std::fs::read_to_string(&source).unwrap(), "script\n");
    mcbench_updater::remove_leftover(&installed).unwrap();
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["mcbench-lite.app"]);
}

#[tokio::test]
async fn key_rotation_follows_the_chain() {
    let (a, b, c) = (Key::new(), Key::new(), Key::new());
    let chain = vec![a.signs_key(&b), b.signs_key(&c)];
    let url = release(
        latest(
            "1.1.0",
            chain,
            serde_json::json!({ TARGET.name(): entry(&c, "1.1.0", TARGET.name(), NAME, NEW) }),
        ),
        &[(NAME, NEW)],
    )
    .await;
    // An app on the first key, on the second, and on the newest.
    for trusted in [&a, &b, &c] {
        assert_eq!(fetch(&url, trusted).await.unwrap(), NEW);
    }
    // An app on a key outside the chain.
    let stranger = Key::new();
    let err = fetch(&url, &stranger).await.unwrap_err();
    assert_eq!(
        err,
        format!(
            "latest.json's key {} is signed by a key this build does not trust (it trusts {}).",
            b.base64(),
            stranger.base64()
        )
    );
}

#[tokio::test]
async fn a_chain_entry_signed_by_another_key_is_refused() {
    let (a, b, x) = (Key::new(), Key::new(), Key::new());
    let url = release(
        latest(
            "1.1.0",
            vec![x.signs_key(&b)],
            serde_json::json!({ TARGET.name(): entry(&b, "1.1.0", TARGET.name(), NAME, NEW) }),
        ),
        &[(NAME, NEW)],
    )
    .await;
    assert_eq!(
        fetch(&url, &a).await.unwrap_err(),
        format!(
            "latest.json's key {} is signed by a key this build does not trust (it trusts {}).",
            b.base64(),
            a.base64()
        )
    );
}

/// Each refusal and its sentence.
#[tokio::test]
async fn refusals() {
    let key = Key::new();
    let other = Key::new();
    let t = TARGET.name();
    let good = entry(&key, "1.1.0", t, NAME, NEW);
    let with = |field: &str, value: serde_json::Value| {
        let mut e = good.clone();
        e[field] = value;
        e
    };
    let tampered: &[u8] = b"the bad AppImage";
    let longer: &[u8] = b"the new AppImage and more";
    // What, latest.json, the files beside it, the sentence (empty: a 404).
    type Case<'a> = (&'a str, serde_json::Value, Vec<(&'a str, &'a [u8])>, String);
    let cases: Vec<Case> = vec![
        (
            "tampered latest.json (sha256)",
            latest("1.1.0", vec![], serde_json::json!({ t: with("sha256", sha256(tampered).into()) })),
            vec![(NAME, tampered)],
            format!(
                "latest.json's entry for {t} does not match its signed comment: latest.json gives {:?}, the signature {:?}.",
                format!("version:1.1.0\ttarget:{t}\tfile:{NAME}\tsize:16\tsha256:{}", sha256(tampered)),
                comment("1.1.0", t, NAME, NEW)
            ),
        ),
        (
            "tampered latest.json (version)",
            latest("9.0.0", vec![], serde_json::json!({ t: good.clone() })),
            vec![(NAME, NEW)],
            format!(
                "latest.json's entry for {t} does not match its signed comment: latest.json gives {:?}, the signature {:?}.",
                comment("9.0.0", t, NAME, NEW),
                comment("1.1.0", t, NAME, NEW)
            ),
        ),
        (
            "tampered asset",
            latest("1.1.0", vec![], serde_json::json!({ t: good.clone() })),
            vec![(NAME, tampered)],
            format!(
                "The download of {NAME} has sha256 {} where latest.json gives {}: it is not the released file.",
                sha256(tampered),
                sha256(NEW)
            ),
        ),
        (
            "asset longer than latest.json says",
            latest("1.1.0", vec![], serde_json::json!({ t: good.clone() })),
            vec![(NAME, longer)],
            format!("The download of {NAME} is larger than the 16 bytes latest.json gives: it is not the released file."),
        ),
        (
            "wrong signature (another file's, with this file's comment)",
            latest(
                "1.1.0",
                vec![],
                serde_json::json!({ t: with("signature", key.sign(tampered, &comment("1.1.0", t, NAME, NEW)).into()) }),
            ),
            vec![(NAME, NEW)],
            format!(
                "The signature of {NAME} does not verify against {}: the file or its signed comment was changed after signing.",
                key.base64()
            ),
        ),
        (
            "signature by an unknown key",
            latest("1.1.0", vec![], serde_json::json!({ t: entry(&other, "1.1.0", t, NAME, NEW) })),
            vec![(NAME, NEW)],
            format!("{NAME} is signed by a key this build does not trust (it trusts {}).", key.base64()),
        ),
        (
            "downgrade",
            latest("0.9.9", vec![], serde_json::json!({ t: entry(&key, "0.9.9", t, NAME, NEW) })),
            vec![(NAME, NEW)],
            "latest.json offers version 0.9.9, older than this app's 1.0.0: a downgrade is refused.".into(),
        ),
        (
            "an asset for another target",
            latest(
                "1.1.0",
                vec![],
                serde_json::json!({ t: entry(&key, "1.1.0", "windows-x86_64", "mcbench-lite.exe", NEW) }),
            ),
            vec![("mcbench-lite.exe", NEW)],
            format!("The asset latest.json lists for {t} is signed as the asset for windows-x86_64."),
        ),
        (
            "no asset for this target",
            latest("1.1.0", vec![], serde_json::json!({ "windows-x86_64": good.clone() })),
            vec![(NAME, NEW)],
            format!("latest.json has no asset for {t}."),
        ),
        (
            "an asset under another name",
            latest(
                "1.1.0",
                vec![],
                serde_json::json!({ t: entry(&key, "1.1.0", t, "mcbench-lite.AppImage", NEW) }),
            ),
            vec![("mcbench-lite.AppImage", NEW)],
            format!("latest.json names the asset for {t} \"mcbench-lite.AppImage\", where its file is always {NAME}."),
        ),
        (
            "an asset larger than 512 MiB",
            latest("1.1.0", vec![], serde_json::json!({ t: with("size", (512u64 * 1024 * 1024 + 1).into()) })),
            vec![(NAME, NEW)],
            format!("latest.json gives {NAME} 536870913 bytes, more than the 536870912 bytes (512 MiB) a release asset may be."),
        ),
        (
            "asset missing from the release",
            latest("1.1.0", vec![], serde_json::json!({ t: good.clone() })),
            vec![],
            String::new(),
        ),
    ];
    for (what, json, files, want) in cases {
        let url = release(json, &files).await;
        let err = fetch(&url, &key).await.unwrap_err();
        if want.is_empty() {
            let asset = url.replace("latest.json", NAME);
            assert_eq!(
                err,
                format!("Fetching {asset} answered 404 Not Found."),
                "{what}"
            );
        } else {
            assert_eq!(err, want, "{what}");
        }
    }
}

#[tokio::test]
async fn an_unknown_field_or_a_bad_version_is_refused() {
    let key = Key::new();
    let mut json = latest("1.1.0", vec![], serde_json::json!({}));
    json["channel"] = "beta".into();
    let url = release(json, &[]).await;
    let err = fetch(&url, &key).await.unwrap_err();
    assert!(
        err.starts_with(&format!(
            "latest.json at {url} is not in its format (LATEST.md): unknown field `channel`"
        )),
        "{err}"
    );
    let url = release(latest("1.1", vec![], serde_json::json!({})), &[]).await;
    assert_eq!(
        fetch(&url, &key).await.unwrap_err(),
        "latest.json's version \"1.1\" is not MAJOR.MINOR.PATCH."
    );
}

#[tokio::test]
async fn a_check_stops_on_cancel() {
    let key = Key::new();
    let base = serve(HashMap::new()).await;
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        stop.cancel();
    });
    let err = check(
        &client(),
        &format!("{base}/hang"),
        "1.0.0",
        TARGET,
        &key.trusted(),
        &cancel,
    )
    .await
    .unwrap_err();
    assert_eq!(err, "The update was cancelled.");
}

/// A Linux `script` install under `root` (`lib/mcbench-lite/` with the
/// program, the sandbox agent and `install-source`), its logs folder, and
/// a release archive of version `version` with the new files.
fn linux_install(root: &Path) -> (Installed, std::path::PathBuf) {
    let payload = root.join("lib/mcbench-lite");
    std::fs::create_dir_all(&payload).unwrap();
    for name in ["mcbench-lite", "mcbench-sandbox-agent"] {
        std::fs::write(payload.join(name), "old").unwrap();
    }
    std::fs::write(payload.join("install-source"), "script\n").unwrap();
    std::fs::write(payload.join("version"), "1.0.0\n").unwrap();
    let logs = root.join("logs");
    std::fs::create_dir(&logs).unwrap();
    let installed =
        Installed::locate(&payload.join("mcbench-lite"), Target::LinuxX86_64, None).unwrap();
    (installed, logs)
}

fn linux_archive() -> Vec<u8> {
    let mut archive = Vec::new();
    {
        let gz = flate2::write::GzEncoder::new(&mut archive, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        for (name, data) in [
            ("mcbench-lite/mcbench-lite", &b"new"[..]),
            ("mcbench-lite/mcbench-sandbox-agent", b"new"),
            ("mcbench-lite/version", b"1.1.0\n"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o755);
            tar.append_data(&mut header, name, data).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }
    archive
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

/// The background check leaves a verified download waiting; the next
/// start checks it again without the network and swaps it in.
#[tokio::test]
async fn a_waiting_download_is_swapped_in_at_the_next_start() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url: url.clone(),
        key: key.trusted(),
    };
    assert_eq!(
        check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await,
        UpdateState::Ready("1.1.0".into())
    );
    let lib = dir.path().join("lib");
    let served = client()
        .get(&url)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(lib.join(".mcbench-lite.update.json")).unwrap(),
        served
    );
    assert_eq!(read(&installed.program), "old");

    // The same version again is not fetched: a feed without the asset
    // still answers Ready.
    let bare = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[],
    )
    .await;
    let feed_bare = Feed {
        url: bare,
        key: key.trusted(),
    };
    assert_eq!(
        check_once(&client(), &feed_bare, &installed, "1.0.0", None, &logs).await,
        UpdateState::Ready("1.1.0".into())
    );

    // The next start: no network needed (the feed's address is not served).
    let offline = Feed {
        url: "http://127.0.0.1:9/download/latest.json".into(),
        key: key.trusted(),
    };
    assert_eq!(
        start_with(&offline, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart {
            version: "1.1.0".into()
        }
    );
    assert_eq!(read(&installed.program), "new");
    assert_eq!(read(&installed.path.join("mcbench-sandbox-agent")), "new");
    assert_eq!(read(&installed.path.join("install-source")), "script\n");
    let mut left: Vec<_> = std::fs::read_dir(&lib)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [
            ".mcbench-lite.gate",
            ".mcbench-lite.lock",
            "mcbench-lite",
            "mcbench-lite.old"
        ]
    );
    // The new program's start keeps the previous version until its core
    // has started; the background thread then removes it.
    assert_eq!(
        start_with(&offline, &installed, "1.1.0", &logs).unwrap().0,
        Start::AsIs
    );
    assert!(lib.join("mcbench-lite.old").is_dir());
    mcbench_updater::remove_leftover(&installed).unwrap();
    let mut left: Vec<_> = std::fs::read_dir(&lib)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [".mcbench-lite.gate", ".mcbench-lite.lock", "mcbench-lite"]
    );
    assert!(!logs.join(mcbench_updater::LOG_FILE).exists());
}

/// A waiting download changed after it was verified is deleted at start,
/// the sentence is logged, and the app starts as it is.
#[tokio::test]
async fn a_tampered_waiting_download_is_deleted() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    let lib = dir.path().join("lib");
    let waiting = lib.join(".mcbench-lite.update");
    let mut bytes = std::fs::read(&waiting).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    std::fs::write(&waiting, &bytes).unwrap();
    assert_eq!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::AsIs
    );
    assert_eq!(read(&installed.program), "old");
    let mut left: Vec<_> = std::fs::read_dir(&lib)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [".mcbench-lite.gate", ".mcbench-lite.lock", "mcbench-lite"]
    );
    let log = read(&logs.join(mcbench_updater::LOG_FILE));
    assert!(
        log.contains(&format!(
            " The waiting update was deleted and the app started as it is: The download of {} has sha256 ",
            target.asset()
        )),
        "{log}"
    );
}

/// A swap that fails leaves the waiting download for the next start and
/// logs why; here the installed `install-source` is missing, so it cannot
/// be carried into the new payload (a failure that needs no permissions
/// and works as root).
#[tokio::test]
async fn a_failed_swap_leaves_the_download_waiting() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    let lib = dir.path().join("lib");
    let source = installed.install_source();
    std::fs::remove_file(&source).unwrap();
    let Start::Held(held) = start_with(&feed, &installed, "1.0.0", &logs).unwrap().0 else {
        panic!("the swap fails")
    };
    assert_eq!(held.version, "1.1.0");
    assert_eq!(read(&installed.program), "old");
    assert!(lib.join(".mcbench-lite.update").is_file());
    assert!(lib.join(".mcbench-lite.update.json").is_file());
    let log = read(&logs.join(mcbench_updater::LOG_FILE));
    assert!(
        log.contains(" The update could not copy the installed install-source to ")
            && log.ends_with(
                "; the app was not changed. The update is tried again at the next start.\n"
            ),
        "{log}"
    );
    assert!(log.ends_with(&format!("{}\n", held.sentence)));
    // This process does not take that version again: a feed without the
    // asset answers the swap's failure, not a fetch's.
    let bare = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[],
    )
    .await;
    let feed_bare = Feed {
        url: bare,
        key: key.trusted(),
    };
    assert_eq!(
        check_once(
            &client(),
            &feed_bare,
            &installed,
            "1.0.0",
            Some(&held),
            &logs
        )
        .await,
        UpdateState::Failed(held.sentence.clone())
    );
    // The next start, with install-source back, swaps it in.
    std::fs::write(&source, "script\n").unwrap();
    assert_eq!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart {
            version: "1.1.0".into()
        }
    );
    assert_eq!(read(&installed.program), "new");
}

/// A process running from the install (a headless run's shared hold, here
/// two of them beside an app's) makes the start skip the swap: everything
/// stays as it was, the download keeps waiting, and one line is logged.
#[tokio::test]
async fn a_process_holding_the_install_makes_the_swap_wait() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    let lib = dir.path().join("lib");
    let listing = || {
        let mut names: Vec<_> = std::fs::read_dir(&lib)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    };
    // A headless run and another holder, both shared at once.
    let headless = hold_install_at(&installed, "1.0.0").unwrap();
    let other = hold_install_at(&installed, "1.0.0").unwrap();
    let before = listing();
    let (start, app) = start_with(&feed, &installed, "1.0.0", &logs).unwrap();
    assert_eq!(start, Start::AsIs);
    assert_eq!(listing(), before);
    assert_eq!(read(&installed.program), "old");
    let log = read(&logs.join(mcbench_updater::LOG_FILE));
    assert!(
        log.ends_with(&format!(
            " Another process runs from {}, so the waiting update was not swapped in; it is tried again at the next start.\n",
            installed.path.display()
        )),
        "{log}"
    );
    assert_eq!(log.lines().count(), 1);
    // With every hold gone, the next start swaps it in.
    drop((headless, other, app));
    assert_eq!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart {
            version: "1.1.0".into()
        }
    );
    assert_eq!(read(&installed.program), "new");
}

/// A new program that cannot be started: the previous version goes back,
/// the sentence is logged, and the version is held for this process.
#[tokio::test]
async fn a_new_program_that_cannot_start_is_reversed() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    assert!(matches!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart { .. }
    ));
    assert_eq!(read(&installed.program), "new");
    let held = mcbench_updater::restart_failed(
        &installed,
        "1.1.0".into(),
        "The updated app at X cannot be started: injected",
        &logs,
    )
    .unwrap();
    assert_eq!(read(&installed.program), "old");
    assert_eq!(read(&installed.path.join("mcbench-sandbox-agent")), "old");
    assert!(!dir.path().join("lib/mcbench-lite.old").exists());
    assert_eq!(
        held.sentence,
        "The updated app at X cannot be started: injected; the previous version was put back, and version 1.1.0 is not taken again until a newer version is offered."
    );
    assert!(read(&logs.join(mcbench_updater::LOG_FILE)).ends_with(&format!("{}\n", held.sentence)));

    // A later process: the held version is read back from beside the
    // payload. It is not fetched again, and a download of it waiting (made
    // here with no held version) is not swapped in.
    assert_eq!(
        mcbench_updater::read_held(&installed, "1.0.0"),
        Some(held.clone())
    );
    assert_eq!(
        check_once(&client(), &feed, &installed, "1.0.0", Some(&held), &logs).await,
        UpdateState::Failed(held.sentence.clone())
    );
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    let lib = dir.path().join("lib");
    assert!(lib.join(".mcbench-lite.update").is_file());
    assert_eq!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::Held(held.clone())
    );
    assert_eq!(read(&installed.program), "old");
    assert!(!lib.join(".mcbench-lite.update").exists());
    // A newer version offered: the held one is forgotten.
    let newer = release(
        latest(
            "1.2.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.2.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let feed_newer = Feed {
        url: newer,
        key: key.trusted(),
    };
    assert_eq!(
        check_once(
            &client(),
            &feed_newer,
            &installed,
            "1.0.0",
            Some(&held),
            &logs
        )
        .await,
        UpdateState::Ready("1.2.0".into())
    );
    assert_eq!(mcbench_updater::read_held(&installed, "1.0.0"), None);
}

/// N1: where the presence file cannot be made (here a dangling symlink in
/// its place), an app-style start and a headless-style hold both go on
/// with no hold and no error, and nothing is swapped.
#[cfg(unix)]
#[tokio::test]
async fn no_hold_is_possible_and_everything_still_runs() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    let lock = installed.lock_file();
    std::os::unix::fs::symlink("/nonexistent/folder/lock", &lock).unwrap();
    let hold = hold_install_at(&installed, "1.0.0").unwrap();
    assert!(!hold.holds());
    let (start, hold) = start_with(&feed, &installed, "1.0.0", &logs).unwrap();
    assert!(!hold.holds());
    let Start::Unable(why) = start else {
        panic!("{start:?}")
    };
    assert_eq!(
        why.sentence(),
        format!(
            "This install cannot update itself, because its lock file {} cannot be opened: No such file or directory (os error 2).",
            lock.display()
        )
    );
    assert_eq!(read(&installed.program), "old");
    assert!(dir.path().join("lib/.mcbench-lite.update").is_file());
}

/// F1: two starters at once. The first holds the gate while it swaps (here
/// the payload's version file is what changes); the second, having waited,
/// finds the installed payload not its own: an app start answers Changed
/// (it runs the installed program instead), a headless hold refuses. The
/// gate wait of more than five seconds is written to update.log.
#[test]
fn a_starter_that_waited_finds_the_payload_replaced() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url: "http://127.0.0.1:9/download/latest.json".into(),
        key: Key::new().trusted(),
    };
    let version = installed.version_file().unwrap();
    for headless in [false, true] {
        std::fs::write(&version, "1.0.0\n").unwrap();
        let gate = installed.path.with_file_name(".mcbench-lite.gate");
        if !gate.exists() {
            std::fs::write(&gate, "").unwrap();
        }
        let first = std::fs::File::open(&gate).unwrap();
        first.lock().unwrap();
        let second = {
            let (installed, logs, feed) = (installed.clone(), logs.clone(), feed.clone());
            std::thread::spawn(move || {
                if headless {
                    hold_install_at(&installed, "1.0.0")
                        .map(|_| Start::AsIs)
                        .unwrap_or(Start::Changed)
                } else {
                    start_with(&feed, &installed, "1.0.0", &logs).unwrap().0
                }
            })
        };
        if headless {
            std::thread::sleep(std::time::Duration::from_millis(200));
        } else {
            // The app start writes its line once it has waited five
            // seconds; until then it waits at the gate.
            let log = logs.join(mcbench_updater::LOG_FILE);
            let since = std::time::Instant::now();
            while !std::fs::read_to_string(&log)
                .is_ok_and(|t| t.contains(" this start waits for it."))
            {
                assert!(since.elapsed().as_secs() < 120, "no line about the wait");
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
        std::fs::write(&version, "1.1.0\n").unwrap();
        drop(first);
        assert_eq!(
            second.join().unwrap(),
            Start::Changed,
            "headless {headless}"
        );
    }
    let log = read(&logs.join(mcbench_updater::LOG_FILE));
    assert!(
        log.ends_with(&format!(
            " Another start of this install has held {} for 5 seconds; this start waits for it.\n",
            installed
                .path
                .with_file_name(".mcbench-lite.gate")
                .display()
        )),
        "{log}"
    );
}

/// F1: every start compares, waited or not. The same version goes on; a
/// payload of another version (replaced since this process was loaded, for
/// example by the install script) is Changed at once.
#[test]
fn every_start_compares_the_payloads_version() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url: "http://127.0.0.1:9/download/latest.json".into(),
        key: Key::new().trusted(),
    };
    let (start, hold) = start_with(&feed, &installed, "1.0.0", &logs).unwrap();
    assert_eq!(start, Start::AsIs);
    assert!(hold.holds());
    drop(hold);
    let (start, _) = start_with(&feed, &installed, "0.9.0", &logs).unwrap();
    assert_eq!(start, Start::Changed);
    assert_eq!(
        hold_install_at(&installed, "0.9.0").err(),
        Some(mcbench_updater::NotThisProgram::Replaced)
    );
    // W4: a version file that cannot be read is reported as itself.
    let version = installed.version_file().unwrap();
    std::fs::remove_file(&version).unwrap();
    let unreadable = start_with(&feed, &installed, "1.0.0", &logs).err().unwrap();
    assert!(
        unreadable.starts_with(&format!(
            "The version file at {} cannot be read (",
            version.display()
        )),
        "{unreadable}"
    );
    assert_eq!(
        hold_install_at(&installed, "1.0.0").err(),
        Some(mcbench_updater::NotThisProgram::Unreadable(unreadable))
    );
}

/// F2: a program loaded from the payload a swap moved aside (here a renamed
/// folder) is not an install of its own: a headless start prints the
/// sentence and exits with status 2. Linux, whose `current_exe()` follows
/// the rename (and whose install layout this test builds).
#[cfg(target_os = "linux")]
#[test]
fn a_program_in_a_moved_payload_is_not_its_install() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = linux_install(dir.path());
    let old = dir.path().join("lib/mcbench-lite.old");
    std::fs::create_dir(&old).unwrap();
    for name in ["mcbench-lite", "install-source", "version"] {
        std::fs::copy(installed.path.join(name), old.join(name)).unwrap();
    }
    assert_eq!(
        mcbench_updater::moved_from(&old.join("mcbench-lite")),
        Some(installed.program.clone())
    );
    // The child that would print the sentence and exit with status 2.
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "headless_hold_from_a_moved_payload",
            "--ignored",
            "--nocapture",
        ])
        .env("UPDATER_TEST_EXE", old.join("mcbench-lite"))
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(2), "{status:?}");
    assert!(
        String::from_utf8_lossy(&status.stderr).contains(mcbench_updater::CHANGED_WHILE_STARTING),
        "{status:?}"
    );
}

/// Run by `a_program_in_a_moved_payload_is_not_its_install` in a child
/// process: the headless hold of the program it names exits.
#[cfg(target_os = "linux")]
#[test]
#[ignore = "run as a child of a_program_in_a_moved_payload_is_not_its_install"]
fn headless_hold_from_a_moved_payload() {
    let exe = std::env::var_os("UPDATER_TEST_EXE").expect("run by its parent test");
    mcbench_updater::hold_install_for(Path::new(&exe), None, "1.0.0");
    unreachable!("the hold exits for a moved payload");
}

/// N4: a program outside an install (no install-source beside it) runs
/// every headless family with no hold, and makes no file.
#[test]
fn a_program_outside_an_install_holds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir.path().join("mcbench-lite");
    std::fs::write(&exe, "").unwrap();
    let hold = mcbench_updater::hold_install_for(&exe, None, "1.0.0");
    assert!(!hold.holds());
    let left: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(left, ["mcbench-lite"]);
}

/// A self-updating install of this system's layout under `root` at version
/// 1.0.0, and its logs folder: on macOS the bundle
/// `mcbench-lite.app/Contents/MacOS/mcbench-lite` (route `script`), on
/// Windows the payload folder `mcbench-lite/` with `mcbench-lite.exe`
/// (route `direct`), elsewhere [`linux_install`]'s `script` folder.
fn this_systems_install(root: &Path) -> (Installed, std::path::PathBuf) {
    if !cfg!(any(target_os = "macos", windows)) {
        return linux_install(root);
    }
    let (exe, resources, source) = if cfg!(target_os = "macos") {
        let contents = root.join("mcbench-lite.app").join("Contents");
        (
            contents.join("MacOS").join("mcbench-lite"),
            contents.join("Resources"),
            "script",
        )
    } else {
        let payload = root.join("mcbench-lite");
        (payload.join("mcbench-lite.exe"), payload, "direct")
    };
    std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
    std::fs::create_dir_all(&resources).unwrap();
    std::fs::write(&exe, "old").unwrap();
    std::fs::write(resources.join("install-source"), format!("{source}\n")).unwrap();
    std::fs::write(resources.join("version"), "1.0.0\n").unwrap();
    let logs = root.join("logs");
    std::fs::create_dir(&logs).unwrap();
    let target = mcbench_updater::install_source_at(&exe)
        .and_then(Target::of_this_build)
        .unwrap();
    (Installed::locate(&exe, target, None).unwrap(), logs)
}

/// `from`'s files and folders copied into `to`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.is_dir() {
            copy_tree(&path, &to.join(entry.file_name()));
        } else {
            std::fs::copy(&path, to.join(entry.file_name())).unwrap();
        }
    }
}

/// The path a process loaded from `installed`'s payload just before a swap
/// moved it aside finds itself at, with the files there: inside
/// `<payload>.old` on Linux and macOS (a copy of the payload), the program
/// renamed to `mcbench-lite.exe.old` in the payload folder on Windows.
fn moved_program(installed: &Installed) -> std::path::PathBuf {
    if cfg!(windows) {
        let old = installed.program.with_file_name("mcbench-lite.exe.old");
        std::fs::copy(&installed.program, &old).unwrap();
        return old;
    }
    let mut name = installed.path.file_name().unwrap().to_os_string();
    name.push(".old");
    let old = installed.path.with_file_name(name);
    copy_tree(&installed.path, &old);
    old.join(installed.program.strip_prefix(&installed.path).unwrap())
}

/// Whether another process of the install holds its presence file (an
/// exclusive try on a handle of its own fails); false where no hold has
/// made the file.
fn presence_held(installed: &Installed) -> bool {
    let file = match std::fs::File::open(installed.lock_file()) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return false,
        Err(e) => panic!("{e}"),
    };
    match file.try_lock() {
        Ok(()) => false,
        Err(std::fs::TryLockError::WouldBlock) => true,
        Err(std::fs::TryLockError::Error(e)) => panic!("{e}"),
    }
}

/// The error screen's quiet hold, taken while the payload is replaced
/// under it: the second process waits at the gate a first one holds, the
/// payload changes (`change`), the gate is released. Returns the hold.
fn quiet_hold_while(installed: &Installed, change: impl FnOnce()) -> mcbench_updater::InstallHold {
    let gate = installed.lock_file().with_extension("gate");
    if !gate.exists() {
        std::fs::write(&gate, "").unwrap();
    }
    let first = std::fs::File::open(&gate).unwrap();
    first.lock().unwrap();
    let second = {
        let program = installed.program.clone();
        std::thread::spawn(move || {
            mcbench_updater::hold_install_quietly_for(&program, None, "1.0.0")
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(200));
    change();
    drop(first);
    second.join().unwrap()
}

/// The error screen's hold is the shared hold a headless run takes: while
/// it is held, a starting app leaves the waiting download for the next
/// start (the method of `a_process_holding_the_install_makes_the_swap_wait`);
/// once it is dropped, the next start swaps.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn the_error_screens_hold_puts_a_swap_off() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    let screen = mcbench_updater::hold_install_quietly_for(&installed.program, None, "1.0.0");
    assert!(screen.holds());
    let (start, app) = start_with(&feed, &installed, "1.0.0", &logs).unwrap();
    assert_eq!(start, Start::AsIs);
    assert_eq!(read(&installed.program), "old");
    let log = read(&logs.join(mcbench_updater::LOG_FILE));
    assert!(
        log.ends_with(&format!(
            " Another process runs from {}, so the waiting update was not swapped in; it is tried again at the next start.\n",
            installed.path.display()
        )),
        "{log}"
    );
    drop((screen, app));
    assert_eq!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart {
            version: "1.1.0".into()
        }
    );
    assert_eq!(read(&installed.program), "new");
}

/// On this system's layout the quiet hold holds the presence file shared
/// for as long as it is kept, and releases it when dropped.
#[test]
fn the_error_screens_hold_holds_the_install() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = this_systems_install(dir.path());
    let screen = mcbench_updater::hold_install_quietly_for(&installed.program, None, "1.0.0");
    assert!(screen.holds());
    assert!(presence_held(&installed));
    drop(screen);
    assert!(!presence_held(&installed));
}

/// A payload of another version, found at once or after waiting at the
/// gate: no hold, nothing printed, and the process goes on (this test
/// process would end with status 2 if it exited).
#[test]
fn the_error_screens_hold_on_a_changed_payload_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = this_systems_install(dir.path());
    let version = installed.version_file().unwrap();
    std::fs::write(&version, "1.1.0\n").unwrap();
    let screen = mcbench_updater::hold_install_quietly_for(&installed.program, None, "1.0.0");
    assert!(!screen.holds());
    assert!(!presence_held(&installed));
    std::fs::write(&version, "1.0.0\n").unwrap();
    let screen = quiet_hold_while(&installed, || std::fs::write(&version, "1.1.0\n").unwrap());
    assert!(!screen.holds());
    assert!(!presence_held(&installed));
}

/// A `version` file that cannot be read, before the hold or once it is
/// taken: no hold, and the process goes on.
#[test]
fn the_error_screens_hold_with_an_unreadable_version_is_none() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = this_systems_install(dir.path());
    let version = installed.version_file().unwrap();
    let screen = quiet_hold_while(&installed, || std::fs::remove_file(&version).unwrap());
    assert!(!screen.holds());
    assert!(!presence_held(&installed));
    let screen = mcbench_updater::hold_install_quietly_for(&installed.program, None, "1.0.0");
    assert!(!screen.holds());
    assert!(!presence_held(&installed));
}

/// A process loaded from the payload a swap moved aside: its quiet hold
/// holds nothing and it goes on, and its restart executes the installed
/// program at the install's real path.
#[test]
fn the_error_screen_from_a_moved_payload_restarts_the_installed_program() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = this_systems_install(dir.path());
    let moved = moved_program(&installed);
    assert_ne!(moved, installed.program);
    let screen = mcbench_updater::hold_install_quietly_for(&moved, None, "1.0.0");
    assert!(!screen.holds());
    assert!(!presence_held(&installed));
    assert_eq!(
        mcbench_updater::installed_program_for(&moved, None),
        installed.program
    );
    // Linux and macOS: a program loaded from the unpack folder, which the
    // swap renamed into place, belongs there too.
    if !cfg!(windows) {
        let rest = installed.program.strip_prefix(&installed.path).unwrap();
        let mut unpack = std::ffi::OsString::from(".");
        unpack.push(installed.path.file_name().unwrap());
        unpack.push(".update.d");
        let unpacked = installed
            .path
            .with_file_name(unpack)
            .join(installed.path.file_name().unwrap())
            .join(rest);
        assert_eq!(
            mcbench_updater::installed_program_for(&unpacked, None),
            installed.program
        );
    }
}

/// What a restart executes for each layout: the program in its payload
/// for this system's self-updating install, the program itself for a
/// package manager's route and outside an install, and the AppImage file
/// for the AppImage.
#[test]
fn the_installed_program_of_each_layout() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = this_systems_install(dir.path());
    let want = if cfg!(target_os = "macos") {
        ["mcbench-lite.app", "Contents", "MacOS", "mcbench-lite"].as_slice()
    } else if cfg!(windows) {
        ["mcbench-lite", "mcbench-lite.exe"].as_slice()
    } else {
        ["lib", "mcbench-lite", "mcbench-lite"].as_slice()
    };
    let want = want.iter().fold(dir.path().to_path_buf(), |p, c| p.join(c));
    assert_eq!(installed.program, want);
    assert_eq!(
        mcbench_updater::installed_program_for(&installed.program, None),
        want
    );
    // A package manager's route: the program itself.
    std::fs::write(installed.install_source(), "apt\n").unwrap();
    assert_eq!(
        mcbench_updater::installed_program_for(&installed.program, None),
        want
    );
    // Outside an install.
    let outside = dir.path().join("loose").join("mcbench-lite");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, "").unwrap();
    assert_eq!(
        mcbench_updater::installed_program_for(&outside, None),
        outside
    );
    // The AppImage: the image file, not the program in its mount.
    if cfg!(target_os = "linux") {
        let mounted = dir.path().join(".mount_x/usr/lib/mcbench-lite");
        std::fs::create_dir_all(&mounted).unwrap();
        std::fs::write(mounted.join("install-source"), "appimage\n").unwrap();
        let image = dir.path().join("mcbench-lite.AppImage");
        assert_eq!(
            mcbench_updater::installed_program_for(
                &mounted.join("mcbench-lite"),
                Some(image.clone())
            ),
            image
        );
        assert_eq!(
            mcbench_updater::installed_program_for(&mounted.join("mcbench-lite"), None),
            mounted.join("mcbench-lite")
        );
    }
}

/// A check whose answer never comes gives up after 60 seconds. The request
/// reaches the server on the running clock; only once the server has read
/// it is the clock paused (and then advanced while the runtime is idle), so
/// the 60 seconds can run out only while the answer does not come.
#[tokio::test]
async fn a_stalled_check_gives_up() {
    let key = Key::new();
    let base = serve(HashMap::new()).await;
    let path = "/hang/a-stalled-check";
    let url = format!("{base}{path}");
    let checking = {
        let (url, key) = (url.clone(), key.trusted());
        tokio::spawn(async move {
            check(
                &client(),
                &url,
                "1.0.0",
                TARGET,
                &key,
                &CancellationToken::new(),
            )
            .await
            .map(drop)
        })
    };
    while !common::hung(path) {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    tokio::time::pause();
    assert_eq!(
        checking.await.unwrap().unwrap_err(),
        format!("Fetching {url} took longer than 60 seconds.")
    );
}

/// A download that stops receiving bytes gives up after 60 seconds. The
/// first bytes arrive on the running clock; only then is the clock paused,
/// so the 60 seconds can run out only while no byte comes.
#[tokio::test]
async fn a_stalled_download_gives_up() {
    let key = Key::new();
    let stalled: Vec<u8> = [STALL, b" and then nothing"].concat();
    let mut whole = stalled.clone();
    whole.extend([0u8; 100]);
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ TARGET.name(): entry(&key, "1.1.0", TARGET.name(), NAME, &whole) }),
        ),
        &[(NAME, &stalled)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("mcbench-lite.AppImage");
    let (release, _) = check(
        &client(),
        &url,
        "1.0.0",
        TARGET,
        &key.trusted(),
        &CancellationToken::new(),
    )
    .await
    .unwrap()
    .unwrap();
    let file = dir.path().join(".mcbench-lite.AppImage.update");
    let downloading = tokio::spawn(async move {
        download(&client(), &release, &image, &CancellationToken::new())
            .await
            .map(drop)
    });
    while std::fs::metadata(&file).map_or(0, |m| m.len()) < stalled.len() as u64 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    tokio::time::pause();
    assert_eq!(
        downloading.await.unwrap().unwrap_err(),
        format!("The download of {NAME} received nothing for 60 seconds.")
    );
}

/// F8: a held version no newer than the running one (installed since by
/// other means) is forgotten and its file removed.
#[test]
fn a_held_version_no_newer_than_this_one_is_forgotten() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, _logs) = linux_install(dir.path());
    let file = dir.path().join("lib/.mcbench-lite.update.held");
    std::fs::write(&file, "1.1.0\nwhy\n").unwrap();
    assert!(mcbench_updater::read_held(&installed, "1.0.0").is_some());
    assert!(file.exists());
    assert_eq!(mcbench_updater::read_held(&installed, "1.1.0"), None);
    assert!(!file.exists());
}

/// W2: a starting app alone on an install whose payload is not its own
/// (a swap cut off after the version file was renamed in, before the
/// program) and with a verified download waiting runs the swap, which
/// finishes it, instead of answering Changed.
#[tokio::test]
async fn a_cut_off_swap_is_finished_by_the_next_start() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let url = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url,
        key: key.trusted(),
    };
    check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
    // The cut-off swap: the new version file is in, the old program stays.
    std::fs::write(installed.version_file().unwrap(), "1.1.0\n").unwrap();
    assert_eq!(
        start_with(&feed, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart {
            version: "1.1.0".into()
        }
    );
    assert_eq!(read(&installed.program), "new");
    // With no download waiting, the same state is Changed.
    std::fs::write(installed.version_file().unwrap(), "1.2.0\n").unwrap();
    assert_eq!(
        start_with(&feed, &installed, "1.1.0", &logs).unwrap().0,
        Start::Changed
    );
}

/// A refused release is remembered beside the payload and not fetched again
/// while latest.json stays the same: the tampered asset (fetched once), a
/// tampered latest.json, an older version and a foreign key (never
/// fetched). The second check fetches only latest.json, writes no line,
/// and answers the first refusal's sentence.
#[tokio::test]
async fn a_refused_release_is_not_fetched_again() {
    let key = Key::new();
    let other = Key::new();
    let target = Target::LinuxX86_64;
    let t = target.name();
    let archive = linux_archive();
    let good = entry(&key, "1.1.0", t, target.asset(), &archive);
    let mut tampered_entry = good.clone();
    tampered_entry["sha256"] = sha256(b"something else").into();
    let tampered_asset: Vec<u8> = archive.iter().map(|b| b ^ 1).collect();
    // What, latest.json, the asset served, how many asset requests.
    let cases: Vec<(&str, serde_json::Value, Vec<u8>, usize)> = vec![
        (
            "a tampered asset",
            latest("1.1.0", vec![], serde_json::json!({ t: good.clone() })),
            tampered_asset,
            1,
        ),
        (
            "a tampered latest.json",
            latest("1.1.0", vec![], serde_json::json!({ t: tampered_entry })),
            archive.clone(),
            0,
        ),
        (
            "an older version",
            latest(
                "0.9.0",
                vec![],
                serde_json::json!({ t: entry(&key, "0.9.0", t, target.asset(), &archive) }),
            ),
            archive.clone(),
            0,
        ),
        (
            "a foreign key",
            latest(
                "1.1.0",
                vec![],
                serde_json::json!({ t: entry(&other, "1.1.0", t, target.asset(), &archive) }),
            ),
            archive.clone(),
            0,
        ),
    ];
    for (what, json, asset, fetched) in cases {
        let url = release(json, &[(target.asset(), &asset)]).await;
        let asset_url = url.replace("latest.json", target.asset());
        let dir = tempfile::tempdir().unwrap();
        let (installed, logs) = linux_install(dir.path());
        let feed = Feed {
            url: url.clone(),
            key: key.trusted(),
        };
        let first = check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
        let UpdateState::Refused(sentence) = first.clone() else {
            panic!("{what}: {first:?}")
        };
        let log = read(&logs.join(mcbench_updater::LOG_FILE));
        assert_eq!(log.lines().count(), 1, "{what}: {log}");
        assert!(log.ends_with(&format!(" {sentence}\n")), "{what}: {log}");
        let refused = dir.path().join("lib/.mcbench-lite.update.refused");
        assert!(refused.is_file(), "{what}");
        for _ in 0..2 {
            assert_eq!(
                check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await,
                UpdateState::Refused(sentence.clone()),
                "{what}"
            );
        }
        assert_eq!(
            common::requests(&url),
            3,
            "{what}: latest.json at every check"
        );
        assert_eq!(common::requests(&asset_url), fetched, "{what}: the asset");
        assert_eq!(
            read(&logs.join(mcbench_updater::LOG_FILE)),
            log,
            "{what}: no new line"
        );
        assert!(
            !dir.path().join("lib/.mcbench-lite.update").exists(),
            "{what}"
        );
    }
}

/// A changed latest.json clears the remembered refusal and the check goes
/// on as usual.
#[tokio::test]
async fn a_changed_latest_json_clears_the_refusal() {
    let key = Key::new();
    let target = Target::LinuxX86_64;
    let archive = linux_archive();
    let refused = release(
        latest(
            "0.9.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "0.9.0", target.name(), target.asset(), &archive) }),
        ),
        &[],
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url: refused,
        key: key.trusted(),
    };
    assert!(matches!(
        check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await,
        UpdateState::Refused(_)
    ));
    let file = dir.path().join("lib/.mcbench-lite.update.refused");
    assert!(file.is_file());
    let good = release(
        latest(
            "1.1.0",
            vec![],
            serde_json::json!({ target.name(): entry(&key, "1.1.0", target.name(), target.asset(), &archive) }),
        ),
        &[(target.asset(), &archive)],
    )
    .await;
    let feed = Feed {
        url: good,
        key: key.trusted(),
    };
    assert_eq!(
        check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await,
        UpdateState::Ready("1.1.0".into())
    );
    assert!(!file.exists());
}

/// A failure that is not a refusal of the release (here no server at the
/// address) is logged at every check and not remembered.
#[tokio::test]
async fn a_network_failure_is_not_remembered() {
    let dir = tempfile::tempdir().unwrap();
    let (installed, logs) = linux_install(dir.path());
    let feed = Feed {
        url: "http://127.0.0.1:9/download/latest.json".into(),
        key: Key::new().trusted(),
    };
    for n in 1..=2 {
        let state = check_once(&client(), &feed, &installed, "1.0.0", None, &logs).await;
        assert!(matches!(state, UpdateState::Failed(_)), "{state:?}");
        assert_eq!(
            read(&logs.join(mcbench_updater::LOG_FILE)).lines().count(),
            n
        );
    }
    assert!(!dir.path().join("lib/.mcbench-lite.update.refused").exists());
}
