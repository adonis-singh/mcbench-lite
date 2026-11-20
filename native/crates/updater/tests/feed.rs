//! `mcbench-feed` end to end: keygen, pack, sign, the release folder served
//! from 127.0.0.1, then the updater's check, download, background check,
//! and the swap at the next start, on scratch folders.

mod common;

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::{client, serve};
use mcbench_updater::{
    Feed, Installed, Start, Target, UpdateKey, UpdateState, check, check_once, download, install,
    start_with,
};
use tokio_util::sync::CancellationToken;

const FEED: &str = env!("CARGO_BIN_EXE_mcbench-feed");

fn feed(args: &[&str], key: Option<&Path>) -> Output {
    let mut command = Command::new(FEED);
    command.args(args).env_remove("MCBENCH_SIGNING_KEY_FILE");
    if let Some(key) = key {
        command.env("MCBENCH_SIGNING_KEY_FILE", key);
    }
    command.output().unwrap()
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn failed(output: Output) -> String {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    String::from_utf8(output.stderr).unwrap()
}

/// The base64 line of a `minisign.pub` or `minisign.key`.
fn line(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .nth(1)
        .unwrap()
        .to_string()
}

fn trusted(keys: &Path) -> UpdateKey {
    UpdateKey::from_base64(&line(&keys.join("minisign.pub"))).unwrap()
}

fn write(path: &Path, text: &str, mode: u32) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    #[cfg(not(unix))]
    let _ = mode;
}

/// A Linux payload folder of version `v`: the three programs, a desktop
/// entry and a symlink.
fn payload(root: &Path, v: &str) -> PathBuf {
    let dir = root.join(format!("payload-{v}"));
    for name in [
        "mcbench-lite",
        "mcbench-build-child",
        "mcbench-sandbox-agent",
    ] {
        write(&dir.join(name), &format!("{name} {v}"), 0o700);
    }
    write(
        &dir.join("share/applications/mcbench-lite.desktop"),
        "[Desktop Entry]",
        0o600,
    );
    write(&dir.join("version"), &format!("{v}\n"), 0o644);
    #[cfg(unix)]
    std::os::unix::fs::symlink("mcbench-lite", dir.join("mcbench-lite-link")).unwrap();
    dir
}

/// Every entry of a `.tar.gz`: path, kind, mode, time, owner, group.
fn listing(archive: &Path) -> Vec<(String, char, u32, u64, u64, u64)> {
    let gz = flate2::read::GzDecoder::new(std::fs::File::open(archive).unwrap());
    let mut tar = tar::Archive::new(gz);
    tar.entries()
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            let h = e.header();
            let kind = match h.entry_type() {
                tar::EntryType::Directory => 'd',
                tar::EntryType::Symlink => 'l',
                tar::EntryType::Regular => 'f',
                other => panic!("{other:?}"),
            };
            (
                e.path().unwrap().to_string_lossy().into_owned(),
                kind,
                h.mode().unwrap(),
                h.mtime().unwrap(),
                h.uid().unwrap(),
                h.gid().unwrap(),
            )
        })
        .collect()
}

#[test]
fn keygen_writes_a_pair_refuses_to_overwrite_and_prints_no_secret() {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    let out = ok(feed(&["keygen", keys.to_str().unwrap()], None));
    let secret = line(&keys.join("minisign.key"));
    assert!(!out.contains(&secret));
    assert_eq!(
        out,
        format!(
            "Wrote {} and {}; the public key is {}.\n",
            keys.join("minisign.pub").display(),
            keys.join("minisign.key").display(),
            line(&keys.join("minisign.pub"))
        )
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(keys.join("minisign.key"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    // build.rs reads the public key file as the app's build does.
    assert!(minisign_verify::PublicKey::from_file(keys.join("minisign.pub")).is_ok());
    let before = std::fs::read(keys.join("minisign.key")).unwrap();
    assert_eq!(
        failed(feed(&["keygen", keys.to_str().unwrap()], None)),
        format!(
            "mcbench-feed: {} exists; keygen never overwrites a key.\n",
            keys.join("minisign.pub").display()
        )
    );
    assert_eq!(std::fs::read(keys.join("minisign.key")).unwrap(), before);
}

#[test]
fn pack_is_deterministic_and_fixes_times_owners_and_modes() {
    let dir = tempfile::tempdir().unwrap();
    let src = payload(dir.path(), "1");
    let a = dir.path().join("a.tar.gz");
    let b = dir.path().join("b.tar.gz");
    ok(feed(
        &[
            "pack",
            "--as",
            "mcbench-lite",
            src.to_str().unwrap(),
            a.to_str().unwrap(),
        ],
        None,
    ));
    // The same files written again later, with other times.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let again = dir.path().join("again");
    std::fs::rename(&src, &again).unwrap();
    let src = payload(dir.path(), "1");
    std::fs::remove_dir_all(&again).unwrap();
    ok(feed(
        &[
            "pack",
            "--as",
            "mcbench-lite",
            src.to_str().unwrap(),
            b.to_str().unwrap(),
        ],
        None,
    ));
    assert_eq!(std::fs::read(&a).unwrap(), std::fs::read(&b).unwrap());
    let mut gz = Vec::new();
    std::fs::File::open(&a)
        .unwrap()
        .read_to_end(&mut gz)
        .unwrap();
    // gzip: magic, deflate, no flags, time 0, OS 255.
    assert_eq!(&gz[..4], &[0x1f, 0x8b, 8, 0]);
    assert_eq!(&gz[4..8], &[0, 0, 0, 0]);
    assert_eq!(gz[9], 255);
    let mut want = vec![
        ("mcbench-lite/", 'd', 0o755),
        ("mcbench-lite/mcbench-build-child", 'f', 0o755),
        ("mcbench-lite/mcbench-lite", 'f', 0o755),
        ("mcbench-lite/mcbench-sandbox-agent", 'f', 0o755),
        ("mcbench-lite/share/", 'd', 0o755),
        ("mcbench-lite/share/applications/", 'd', 0o755),
        (
            "mcbench-lite/share/applications/mcbench-lite.desktop",
            'f',
            0o644,
        ),
        ("mcbench-lite/version", 'f', 0o644),
    ];
    if cfg!(unix) {
        want.insert(3, ("mcbench-lite/mcbench-lite-link", 'l', 0o644));
    } else {
        // Windows has no execute bit: only `.exe` files are 0755.
        for entry in &mut want[1..4] {
            entry.2 = 0o644;
        }
    }
    let got: Vec<_> = listing(&a)
        .into_iter()
        .map(|(path, kind, mode, time, uid, gid)| {
            assert_eq!((time, uid, gid), (0, 0, 0), "{path}");
            (path, kind, mode)
        })
        .collect();
    let want: Vec<_> = want
        .into_iter()
        .map(|(p, k, m)| (p.to_string(), k, m))
        .collect();
    assert_eq!(got, want);
    // Without --as the contents are at the root.
    let c = dir.path().join("c.tar.gz");
    ok(feed(
        &["pack", src.to_str().unwrap(), c.to_str().unwrap()],
        None,
    ));
    assert_eq!(listing(&c)[0].0, "mcbench-build-child");
}

#[test]
fn sign_refuses_a_missing_asset_a_bad_target_and_a_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    ok(feed(&["keygen", keys.to_str().unwrap()], None));
    let key = keys.join("minisign.key");
    let release = dir.path().join("release");
    std::fs::create_dir(&release).unwrap();
    let r = release.to_str().unwrap();
    let err = failed(feed(&["sign", r, "1.0.0", "windows-x86_64"], Some(&key)));
    assert!(
        err.starts_with(&format!(
            "mcbench-feed: The asset for windows-x86_64 is missing: {} cannot be read (",
            release.join("mcbench-lite-windows-x86_64.tar.gz").display()
        )),
        "{err}"
    );
    assert!(!release.join("latest.json").exists());
    assert!(
        failed(feed(&["sign", r, "1.0.0", "windows"], Some(&key)))
            .starts_with("mcbench-feed: \"windows\" is not a target; the targets are ")
    );
    assert_eq!(
        failed(feed(&["sign", r, "1.0", "windows-x86_64"], Some(&key))),
        "mcbench-feed: The release's version \"1.0\" is not MAJOR.MINOR.PATCH.\n"
    );
    assert_eq!(
        failed(feed(&["sign", r, "1.0.0", "windows-x86_64"], None)),
        "mcbench-feed: MCBENCH_SIGNING_KEY_FILE is not set.\n"
    );
    let public = keys.join("minisign.pub");
    let err = failed(feed(&["sign", r, "1.0.0", "windows-x86_64"], Some(&public)));
    assert!(
        err.starts_with(&format!(
            "mcbench-feed: MCBENCH_SIGNING_KEY_FILE names {}, which is not an unencrypted minisign secret key",
            public.display()
        )),
        "{err}"
    );
    assert_eq!(feed(&["sign", r], Some(&key)).status.code(), Some(2));
}

/// A release folder served as GitHub serves `releases/latest/download/`.
async fn serve_release(release: &Path) -> String {
    let files: HashMap<String, Vec<u8>> = std::fs::read_dir(release)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                format!("/download/{}", e.file_name().to_string_lossy()),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    format!("{}/download/latest.json", serve(files).await)
}

/// A Linux `script` install of version 1.0.0 under `root`.
fn linux_install(root: &Path) -> (Installed, PathBuf) {
    let lib = root.join("home/.local/lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::rename(payload(root, "1.0.0"), lib.join("mcbench-lite")).unwrap();
    std::fs::write(lib.join("mcbench-lite/install-source"), "script\n").unwrap();
    let logs = root.join("logs");
    std::fs::create_dir(&logs).unwrap();
    let installed = Installed::locate(
        &lib.join("mcbench-lite/mcbench-lite"),
        Target::LinuxX86_64,
        None,
    )
    .unwrap();
    (installed, logs)
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[tokio::test]
async fn keygen_pack_sign_then_check_download_and_install() {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    ok(feed(&["keygen", keys.to_str().unwrap()], None));
    let key = keys.join("minisign.key");
    let release = dir.path().join("release");
    std::fs::create_dir(&release).unwrap();
    let src = payload(dir.path(), "1.1.0");
    let linux = release.join(Target::LinuxX86_64.asset());
    ok(feed(
        &[
            "pack",
            "--as",
            "mcbench-lite",
            src.to_str().unwrap(),
            linux.to_str().unwrap(),
        ],
        None,
    ));
    std::fs::write(
        release.join(Target::LinuxX86_64Appimage.asset()),
        "the new AppImage",
    )
    .unwrap();
    let out = ok(feed(
        &[
            "sign",
            release.to_str().unwrap(),
            "1.1.0",
            "linux-x86_64",
            "linux-x86_64-appimage",
        ],
        Some(&key),
    ));
    assert_eq!(
        out,
        format!("Wrote {}.\n", release.join("latest.json").display())
    );
    assert!(!out.contains(&line(&key)));
    let latest: serde_json::Value =
        serde_json::from_str(&read(&release.join("latest.json"))).unwrap();
    assert_eq!(latest["version"], "1.1.0");
    assert_eq!(latest["keys"], serde_json::json!([]));
    assert_eq!(
        latest["assets"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["linux-x86_64", "linux-x86_64-appimage"]
    );
    let url = serve_release(&release).await;

    // The check and download the updater makes, then the swap.
    let (installed, logs) = linux_install(dir.path());
    let cancel = CancellationToken::new();
    let (found, _) = check(
        &client(),
        &url,
        "1.0.0",
        Target::LinuxX86_64,
        &trusted(&keys),
        &cancel,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(found.name, "mcbench-lite-linux-x86_64.tar.gz");
    let downloaded = download(&client(), &found, &installed.path, &cancel)
        .await
        .unwrap();
    install(&downloaded, &installed).unwrap();
    drop(downloaded);
    for name in [
        "mcbench-lite",
        "mcbench-build-child",
        "mcbench-sandbox-agent",
    ] {
        assert_eq!(read(&installed.path.join(name)), format!("{name} 1.1.0"));
    }
    assert_eq!(read(&installed.path.join("install-source")), "script\n");
    #[cfg(unix)]
    assert_eq!(
        std::fs::read_link(installed.path.join("mcbench-lite-link")).unwrap(),
        Path::new("mcbench-lite")
    );

    // The AppImage from the same feed.
    let image = dir.path().join("mcbench-lite.AppImage");
    std::fs::write(&image, "the old AppImage").unwrap();
    let appimage = Installed::locate(
        Path::new("/tmp/.mount/usr/bin/mcbench-lite"),
        Target::LinuxX86_64Appimage,
        Some(image.clone()),
    )
    .unwrap();
    let (found, _) = check(
        &client(),
        &url,
        "1.0.0",
        appimage.target,
        &trusted(&keys),
        &cancel,
    )
    .await
    .unwrap()
    .unwrap();
    let downloaded = download(&client(), &found, &appimage.path, &cancel)
        .await
        .unwrap();
    install(&downloaded, &appimage).unwrap();
    assert_eq!(read(&image), "the new AppImage");
    assert!(logs.read_dir().unwrap().next().is_none());
}

#[tokio::test]
async fn a_signed_feed_downloads_in_the_background_and_swaps_at_the_next_start() {
    let dir = tempfile::tempdir().unwrap();
    let keys = dir.path().join("keys");
    ok(feed(&["keygen", keys.to_str().unwrap()], None));
    let release = dir.path().join("release");
    std::fs::create_dir(&release).unwrap();
    let src = payload(dir.path(), "1.1.0");
    let linux = release.join(Target::LinuxX86_64.asset());
    ok(feed(
        &[
            "pack",
            "--as",
            "mcbench-lite",
            src.to_str().unwrap(),
            linux.to_str().unwrap(),
        ],
        None,
    ));
    ok(feed(
        &["sign", release.to_str().unwrap(), "1.1.0", "linux-x86_64"],
        Some(&keys.join("minisign.key")),
    ));
    let feed_ = Feed {
        url: serve_release(&release).await,
        key: trusted(&keys),
    };
    let (installed, logs) = linux_install(dir.path());
    assert_eq!(
        check_once(&client(), &feed_, &installed, "1.0.0", None, &logs).await,
        UpdateState::Ready("1.1.0".into())
    );
    assert_eq!(read(&installed.program), "mcbench-lite 1.0.0");
    assert_eq!(
        start_with(&feed_, &installed, "1.0.0", &logs).unwrap().0,
        Start::Restart {
            version: "1.1.0".into()
        }
    );
    assert_eq!(
        read(&installed.path.join("mcbench-sandbox-agent")),
        "mcbench-sandbox-agent 1.1.0"
    );
    assert_eq!(
        start_with(&feed_, &installed, "1.1.0", &logs).unwrap().0,
        Start::AsIs
    );
    // What the background thread does once the new program's core started.
    mcbench_updater::remove_leftover(&installed).unwrap();
    let mut left: Vec<_> = std::fs::read_dir(installed.path.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    assert_eq!(
        left,
        [".mcbench-lite.gate", ".mcbench-lite.lock", "mcbench-lite"]
    );
    assert_eq!(
        check_once(&client(), &feed_, &installed, "1.1.0", None, &logs).await,
        UpdateState::Current
    );
}

#[tokio::test]
async fn a_rotation_with_sign_key_and_chain() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a"), dir.path().join("b"));
    for k in [&a, &b] {
        ok(feed(&["keygen", k.to_str().unwrap()], None));
    }
    // The current key a signs the next key b.
    let entry = ok(feed(
        &["sign-key", b.join("minisign.pub").to_str().unwrap()],
        Some(&a.join("minisign.key")),
    ));
    assert!(!entry.contains(&line(&a.join("minisign.key"))));
    let entry: serde_json::Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(entry["public_key"], line(&b.join("minisign.pub")));
    let chain = dir.path().join("chain.json");
    std::fs::write(
        &chain,
        serde_json::to_string(&serde_json::json!({
            "version": "1.0.0", "keys": [entry], "assets": {}
        }))
        .unwrap(),
    )
    .unwrap();
    let release = dir.path().join("release");
    std::fs::create_dir(&release).unwrap();
    std::fs::write(
        release.join(Target::LinuxAarch64Appimage.asset()),
        "the new AppImage",
    )
    .unwrap();
    let r = release.to_str().unwrap();
    let c = chain.to_str().unwrap();
    // Signing with a under a chain that ends at b is refused.
    assert_eq!(
        failed(feed(
            &["sign", "--chain", c, r, "1.1.0", "linux-aarch64-appimage"],
            Some(&a.join("minisign.key"))
        )),
        format!(
            "mcbench-feed: The chain in {} ends at the key {}, and the signing key is {}: apps would refuse this feed.\n",
            chain.display(),
            line(&b.join("minisign.pub")),
            line(&a.join("minisign.pub"))
        )
    );
    ok(feed(
        &["sign", "--chain", c, r, "1.1.0", "linux-aarch64-appimage"],
        Some(&b.join("minisign.key")),
    ));
    let url = serve_release(&release).await;
    // An app built with a follows the chain to b; one with b trusts b.
    for k in [&a, &b] {
        let found = check(
            &client(),
            &url,
            "1.0.0",
            Target::LinuxAarch64Appimage,
            &trusted(k),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
        assert_eq!(found.unwrap().0.version, "1.1.0");
    }
}
