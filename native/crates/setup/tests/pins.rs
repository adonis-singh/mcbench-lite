//! `test/tools.test.js`'s rules of pinned downloads as Rust tests (the
//! tests of the JDK, rustup, Chromium's shell, the engine's pins, the zip
//! reader and the CLI on PATH are of programs the native app does not
//! download, or of other crates). Every download comes from a server on
//! loopback, and every file is written in a scratch folder.

mod common;

use std::collections::HashMap;
use std::sync::Mutex;

use common::{Answer, names, scratch, serve, sha256};
use mcbench_setup::Bytes;
use mcbench_setup::pins::{
    self, Download, PlatformPin, Program, Unpack, download, install, platform,
};

fn pin(name: &'static str, url: String, sha: String, unpack: Unpack, bin: &'static str) -> Program {
    let p: &'static str = Box::leak(platform().into_boxed_str());
    Program {
        name,
        title: name,
        version: "1.2.3",
        unpack,
        platforms: vec![PlatformPin {
            platform: p,
            download: Download {
                url,
                sha256: sha,
                bytes: 1,
            },
            bin,
        }],
    }
}

fn none(_: Bytes) {}

#[test]
fn every_program_is_pinned_for_the_four_platforms_by_address_sha256_and_size() {
    for program in pins::programs() {
        let mut platforms: Vec<&str> = program.platforms.iter().map(|p| p.platform).collect();
        platforms.sort();
        assert_eq!(
            platforms,
            ["darwin-arm64", "darwin-x64", "linux-arm64", "linux-x64"],
            "{}",
            program.name
        );
        for p in &program.platforms {
            assert!(
                p.download.url.starts_with("https://"),
                "{} {}",
                program.name,
                p.platform
            );
            assert!(
                p.download.sha256.len() == 64
                    && p.download
                        .sha256
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
            );
            assert!(p.download.bytes > 0);
            assert!(!p.bin.is_empty());
        }
    }
}

// The sizes README.md states are the pins' own, from the smallest
// platform's download to the largest, as the terminal rounds them.
#[test]
fn bluemaps_jar_is_pinned_as_server_bluemap_js_pins_it() {
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../server/bluemap.js"
    ))
    .unwrap();
    let pin = pins::bluemap();
    assert!(source.contains(&format!("const VERSION = '{}';", pins::BLUEMAP_VERSION)));
    assert!(source.contains(
        "const JAR_URL = `https://github.com/BlueMap-Minecraft/BlueMap/releases/download/v${VERSION}/${JAR_NAME}`;"
    ));
    assert!(source.contains("const JAR_NAME = `bluemap-${VERSION}-cli.jar`;"));
    assert_eq!(
        pin.url,
        format!(
            "https://github.com/BlueMap-Minecraft/BlueMap/releases/download/v{0}/bluemap-{0}-cli.jar",
            pins::BLUEMAP_VERSION
        )
    );
    assert!(source.contains(&format!("const JAR_SHA256 = '{}';", pin.sha256)));
    // JAR_MB, the size the progress line counts in.
    assert!(source.contains(&format!(
        "const JAR_MB = {};",
        (pin.bytes as f64 / 1e6).ceil() as u64
    )));
}

#[test]
fn readme_states_each_pinned_downloads_size_as_the_pins_have_it() {
    let readme =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../README.md"))
            .unwrap();
    for program in pins::programs() {
        let mb: Vec<u64> = program
            .platforms
            .iter()
            .map(|p| (p.download.bytes as f64 / 1e6).round() as u64)
            .collect();
        let size = format!(
            "{} to {} MB",
            mb.iter().min().unwrap(),
            mb.iter().max().unwrap()
        );
        assert!(
            readme
                .lines()
                .any(|l| l.contains(program.version) && l.contains(&size)),
            "README.md names {} with {size}",
            program.title
        );
    }
}

#[tokio::test]
async fn a_download_is_kept_once_its_sha256_matches_and_its_progress_is_told_in_bytes() {
    let bytes: Vec<u8> = (0..200_000u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
        .collect();
    let server = serve(HashMap::from([("/file".to_string(), bytes.clone())])).await;
    let dir = scratch("download");
    let target = dir.join("download").join("file");
    let progress = Mutex::new(Vec::new());
    download(
        &common::client(),
        &format!("{}/file", server.url),
        &target,
        &sha256(&bytes),
        &|b| progress.lock().unwrap().push(b),
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    let progress = progress.into_inner().unwrap();
    assert!(progress.len() >= 2);
    assert_eq!(
        progress.last(),
        Some(&Bytes {
            done: bytes.len() as u64,
            total: Some(bytes.len() as u64)
        })
    );
    assert_eq!(
        names(target.parent().unwrap()),
        ["file"],
        "no temporary file is left"
    );
}

#[tokio::test]
async fn a_download_whose_sha256_differs_fails_naming_both_and_nothing_of_it_is_kept() {
    let server = serve(HashMap::from([("/file".to_string(), b"tampered".to_vec())])).await;
    let folder = scratch("tampered");
    let expected = sha256(b"the pinned bytes");
    let url = format!("{}/file", server.url);
    let error = download(
        &common::client(),
        &url,
        &folder.join("file"),
        &expected,
        &none,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        format!(
            "{url} downloaded with sha256 {}, but mcbench-lite expects {expected}. It was not kept.",
            sha256(b"tampered")
        )
    );
    assert!(names(&folder).is_empty());
}

#[tokio::test]
async fn a_download_the_server_refuses_fails_with_its_status() {
    let server = serve(HashMap::new()).await;
    let error = download(
        &common::client(),
        &format!("{}/missing", server.url),
        &scratch("refused").join("file"),
        &"0".repeat(64),
        &none,
    )
    .await
    .unwrap_err();
    assert!(error.contains("HTTP 404"), "{error}");
}

#[tokio::test]
async fn a_download_that_fails_on_the_network_is_tried_again_three_times_in_all() {
    let bytes = vec![5u8; 100_000];
    let server = serve(HashMap::from([("/file".to_string(), bytes.clone())])).await;
    server.state.lock().unwrap().script = vec![Answer::Drop, Answer::Short];
    let flaky = scratch("flaky");
    let target = flaky.join("file");
    download(
        &common::client(),
        &format!("{}/file", server.url),
        &target,
        &sha256(&bytes),
        &none,
    )
    .await
    .unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
    assert_eq!(server.state.lock().unwrap().requests.len(), 3);
    assert_eq!(
        names(target.parent().unwrap()),
        ["file"],
        "no temporary file is left"
    );
}

#[tokio::test]
async fn a_server_error_is_tried_again_and_the_third_failure_fails_the_download_saying_so() {
    let server = serve(HashMap::from([("/file".to_string(), b"unused".to_vec())])).await;
    server.state.lock().unwrap().script = vec![
        Answer::Status(503),
        Answer::Status(502),
        Answer::Status(500),
    ];
    let server_error = scratch("server-error");
    let folder = server_error.join("inner");
    let url = format!("{}/file", server.url);
    let error = download(
        &common::client(),
        &url,
        &folder.join("file"),
        &"0".repeat(64),
        &none,
    )
    .await
    .unwrap_err();
    assert_eq!(
        error,
        format!("The download of {url} failed: HTTP 500 (tried 3 times)")
    );
    assert_eq!(server.state.lock().unwrap().requests.len(), 3);
    assert!(
        !folder.exists(),
        "nothing is written for an answer that is not the file"
    );
}

#[tokio::test]
async fn a_checksum_that_differs_and_a_refused_download_are_not_tried_again() {
    let server = serve(HashMap::from([("/file".to_string(), b"tampered".to_vec())])).await;
    let dir = scratch("once");
    let error = download(
        &common::client(),
        &format!("{}/file", server.url),
        &dir.join("file"),
        &"0".repeat(64),
        &none,
    )
    .await
    .unwrap_err();
    assert!(error.ends_with(&format!("expects {}. It was not kept.", "0".repeat(64))));
    assert_eq!(server.state.lock().unwrap().requests.len(), 1);
    let refused = serve(HashMap::new()).await;
    refused.state.lock().unwrap().script = vec![Answer::Status(404)];
    let error = download(
        &common::client(),
        &format!("{}/file", refused.url),
        &dir.join("refused"),
        &"0".repeat(64),
        &none,
    )
    .await
    .unwrap_err();
    assert!(error.contains("HTTP 404"));
    assert_eq!(refused.state.lock().unwrap().requests.len(), 1);
}

/// Runs a program a test has just installed. The install (the app's code,
/// in this process) wrote it, and a file just written can be busy for as
/// long as another test's fork takes to exec (`ETXTBSY`: the fork holds the
/// write descriptor until then), so the start is tried again then, for up
/// to a minute.
#[cfg(unix)]
fn output_of(program: &std::path::Path) -> std::process::Output {
    let started = std::time::Instant::now();
    loop {
        match std::process::Command::new(program).output() {
            Ok(output) => return output,
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                assert!(
                    started.elapsed() < std::time::Duration::from_secs(60),
                    "{} stayed busy",
                    program.display()
                );
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(error) => panic!("{}: {error}", program.display()),
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_single_file_program_is_installed_executable_into_its_versions_folder_and_once() {
    let bytes = b"#!/bin/sh\necho installed\n".to_vec();
    let server = serve(HashMap::from([("/program".to_string(), bytes.clone())])).await;
    let dir = scratch("file-tools");
    let pinned = pin(
        "program",
        format!("{}/program", server.url),
        sha256(&bytes),
        Unpack::File,
        "program",
    );
    let program = install(&common::client(), &pinned, &dir, &platform(), &none)
        .await
        .unwrap();
    assert_eq!(program, dir.join("program-1.2.3").join("program"));
    assert_eq!(output_of(&program).stdout, b"installed\n");
    assert_eq!(
        install(&common::client(), &pinned, &dir, &platform(), &none)
            .await
            .unwrap(),
        program
    );
    assert_eq!(
        server
            .state
            .lock()
            .unwrap()
            .requests
            .iter()
            .map(|(p, _)| p.as_str())
            .collect::<Vec<_>>(),
        ["/program"],
        "an installed program is not downloaded again"
    );
    assert_eq!(
        names(&dir)
            .into_iter()
            .filter(|n| !n.ends_with(".lock"))
            .collect::<Vec<_>>(),
        ["program-1.2.3"]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_tar_gz_is_unpacked_into_the_versions_folder_and_the_program_is_found_at_its_pinned_path()
{
    let source = scratch("tar-source");
    std::fs::create_dir_all(source.join("package/bin")).unwrap();
    std::fs::write(
        source.join("package/bin/tool"),
        "#!/bin/sh\necho from tar\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            source.join("package/bin/tool"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let archive = source.join("tool.tar.gz");
    assert!(
        std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&*source)
            .arg("package")
            .status()
            .unwrap()
            .success()
    );
    let bytes = std::fs::read(&archive).unwrap();
    let server = serve(HashMap::from([("/tool.tar.gz".to_string(), bytes.clone())])).await;
    let pinned = pin(
        "tool",
        format!("{}/tool.tar.gz", server.url),
        sha256(&bytes),
        Unpack::Tar,
        "package/bin/tool",
    );
    let tools = scratch("tar-tools");
    let program = install(&common::client(), &pinned, &tools, &platform(), &none)
        .await
        .unwrap();
    assert_eq!(output_of(&program).stdout, b"from tar\n");
}

#[tokio::test]
async fn an_install_whose_download_fails_its_checksum_leaves_no_folder_so_the_next_start_downloads_again()
 {
    let server = serve(HashMap::from([(
        "/program".to_string(),
        b"tampered".to_vec(),
    )]))
    .await;
    let dir = scratch("failed-tools");
    let pinned = pin(
        "program",
        format!("{}/program", server.url),
        sha256(b"pinned"),
        Unpack::File,
        "program",
    );
    for _ in 0..2 {
        assert!(
            install(&common::client(), &pinned, &dir, &platform(), &none)
                .await
                .unwrap_err()
                .contains("It was not kept")
        );
    }
    assert_eq!(server.state.lock().unwrap().requests.len(), 2);
    assert!(names(&dir).into_iter().all(|n| n.ends_with(".lock")));
}

#[tokio::test]
async fn a_program_with_no_pin_for_this_platform_fails_naming_the_platform() {
    let pinned = Program {
        name: "program",
        title: "A program",
        version: "1",
        unpack: Unpack::File,
        platforms: vec![],
    };
    assert_eq!(
        install(
            &common::client(),
            &pinned,
            &scratch("no-platform"),
            &platform(),
            &none
        )
        .await
        .unwrap_err(),
        format!("mcbench-lite has no pinned A program for {}", platform())
    );
}

// The pins are tools.js's, field for field (`MCBENCH_DIFF_REPO`: a checkout
// with its node_modules, run with `node`).
#[test]
#[ignore = "needs Node and a checkout with node_modules: set MCBENCH_DIFF_REPO"]
fn the_pins_are_tools_js_pins() {
    let repo = std::env::var("MCBENCH_DIFF_REPO").expect("MCBENCH_DIFF_REPO names a checkout");
    let script = "const t = require(process.argv[1] + '/server/tools'); console.log(JSON.stringify(['claude', 'codex'].map((n) => [n, t.PINS[n]])))";
    let out = std::process::Command::new("node")
        .args(["-e", script, &repo])
        .env(
            "MCBENCH_TOOLS_DIR",
            std::env::temp_dir().join("mcbench-setup-pins-tools"),
        )
        .output()
        .expect("node runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let node: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    for (program, (name, pin)) in pins::programs().iter().zip(
        node.as_array()
            .unwrap()
            .iter()
            .map(|p| (p[0].as_str().unwrap(), &p[1])),
    ) {
        assert_eq!(program.name, name);
        assert_eq!(program.title, pin["title"]);
        assert_eq!(program.version, pin["version"]);
        assert_eq!(
            match program.unpack {
                Unpack::File => "file",
                Unpack::Tar => "tar",
            },
            pin["unpack"]
        );
        assert_eq!(
            program.platforms.len(),
            pin["platforms"].as_object().unwrap().len()
        );
        for p in &program.platforms {
            let theirs = &pin["platforms"][p.platform];
            assert_eq!(p.download.url, theirs["url"]);
            assert_eq!(p.download.sha256, theirs["sha256"]);
            assert_eq!(p.download.bytes, theirs["bytes"].as_u64().unwrap());
            assert_eq!(p.bin, theirs["bin"]);
        }
    }
}
