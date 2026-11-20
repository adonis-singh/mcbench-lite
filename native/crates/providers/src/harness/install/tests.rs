//! `test/cli-install.test.js` ported, and the pinned download against a
//! local file server (never the real hosts): checksum, retries, layout.

use std::sync::atomic::{AtomicUsize, Ordering};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

fn scratch(name: &str) -> crate::scratch::Scratch {
    crate::scratch::Scratch::new(&format!("install-{name}"))
}

/// A lookup with an empty PATH and no variables, as the test's PATH=bin.
fn nothing_on_path(bin: &Path) -> Lookup {
    Lookup {
        path: bin.as_os_str().to_owned(),
        claude_binary: None,
        codex_binary: None,
    }
}

/// The install as `install` makes it, with a gate: progress at once, then
/// the program in its version's folder once released (or the failure).
fn fake_installer(
    tools: PathBuf,
    fail: Option<&'static str>,
) -> (Installer, Arc<tokio::sync::Notify>, Arc<AtomicUsize>) {
    let gate = Arc::new(tokio::sync::Notify::new());
    let calls = Arc::new(AtomicUsize::new(0));
    let (g, c) = (gate.clone(), calls.clone());
    let installer: Installer = Arc::new(
        move |pin: Pin, progress: Arc<dyn Fn(Progress) + Send + Sync>| {
            c.fetch_add(1, Ordering::SeqCst);
            progress(Progress {
                done: 5,
                total: Some(10),
            });
            let (gate, tools) = (g.clone(), tools.clone());
            Box::pin(async move {
                gate.notified().await;
                if let Some(fail) = fail {
                    return Err(fail.to_owned());
                }
                let (folder, program) = pin.installed(&tools, &platform());
                std::fs::create_dir_all(program.parent().unwrap()).unwrap();
                std::fs::write(&program, "#!/bin/sh\necho 9.9.9\n").unwrap();
                assert!(folder.exists());
                Ok(program)
            })
        },
    );
    (installer, gate, calls)
}

/// Waits until the install of `name` runs no more, failing after a minute
/// (a hang, not a slow machine).
async fn ended(cli: &CliInstaller, name: &str) {
    let started = std::time::Instant::now();
    while cli
        .state(name)
        .unwrap()
        .is_some_and(|view| view.state == "running")
    {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "the install of {name} ends"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
}

fn pin(name: &str) -> Pin {
    pins().into_iter().find(|p| p.name == name).unwrap()
}

#[cfg_attr(windows, ignore = "the provider CLIs have no Windows pins yet")]
#[tokio::test]
async fn a_cli_that_is_not_found_is_installed_into_the_tools_folder_and_the_app_runs_it_from_there()
{
    let root = scratch("not-found");
    let tools = root.join("tools");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let lookup = nothing_on_path(&bin);
    let (installer, gate, calls) = fake_installer(tools.clone(), None);
    let refreshed = Arc::new(Mutex::new(Vec::new()));
    let r = refreshed.clone();
    let cli = CliInstaller::with_installer(
        tools.clone(),
        platform(),
        lookup.clone(),
        installer,
        Arc::new(move |n: &str| r.lock().unwrap().push(n.to_owned())),
    );
    assert_eq!(cli.state("claude").unwrap(), None);
    let running = InstallView {
        state: "running".to_owned(),
        progress: Some(Progress {
            done: 5,
            total: Some(10),
        }),
        error: None,
    };
    assert_eq!(cli.start("claude").unwrap(), running);
    assert_eq!(
        cli.start("claude").unwrap(),
        cli.state("claude").unwrap().unwrap(),
        "one install at a time"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    gate.notify_one();
    ended(&cli, "claude").await;
    assert_eq!(
        cli.state("claude").unwrap(),
        Some(InstallView {
            state: "done".to_owned(),
            progress: None,
            error: None
        })
    );
    assert_eq!(*refreshed.lock().unwrap(), ["claude"]);
    assert_eq!(
        cli_program(&pin("claude"), &tools, &platform(), &lookup),
        pin("claude").installed(&tools, &platform()).1
    );
    assert_eq!(
        cli.start("claude").unwrap_err().status,
        409,
        "installed in the tools folder already"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn an_install_that_fails_says_why_and_can_be_started_again() {
    let root = scratch("fails");
    let tools = root.join("tools");
    let (installer, gate, calls) = fake_installer(
        tools.clone(),
        Some("downloaded with sha256 0000, but mcbench-lite expects 1111"),
    );
    let cli = CliInstaller::with_installer(
        tools.clone(),
        platform(),
        nothing_on_path(&root.join("bin")),
        installer,
        Arc::new(|_: &str| {}),
    );
    cli.start("codex").unwrap();
    gate.notify_one();
    ended(&cli, "codex").await;
    assert_eq!(
        cli.state("codex").unwrap(),
        Some(InstallView {
            state: "failed".to_owned(),
            progress: None,
            error: Some("downloaded with sha256 0000, but mcbench-lite expects 1111".to_owned())
        })
    );
    assert!(!pin("codex").installed(&tools, &platform()).0.exists());
    assert_eq!(
        cli.start("codex").unwrap().state,
        "running",
        "it can be started again"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_cli_on_path_or_named_by_its_variable_is_the_one_the_app_runs_and_is_not_installed() {
    // On Windows the app runs Codex's Linux build in its WSL distro, so a
    // codex on PATH is not the one it runs (`platform_of`): the CLI that is
    // looked for on PATH there is Claude Code, as `claude.exe`.
    let (name, file) = if cfg!(windows) {
        ("claude", "claude.exe")
    } else {
        ("codex", "codex")
    };
    let root = scratch("on-path");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join(file), "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(
        bin.join(file),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let (installer, _, _) = fake_installer(root.join("tools"), None);
    let cli = CliInstaller::with_installer(
        root.join("tools"),
        platform(),
        nothing_on_path(&bin),
        installer.clone(),
        Arc::new(|_: &str| {}),
    );
    assert_eq!(
        cli.start(name).unwrap_err(),
        InstallRefused {
            status: 409,
            message: format!("{name} is installed already, and the app runs that one")
        }
    );
    let named = if cfg!(windows) {
        Lookup {
            claude_binary: Some("/somewhere/claude".to_owned()),
            ..nothing_on_path(&root.join("empty"))
        }
    } else {
        Lookup {
            codex_binary: Some("/somewhere/codex".to_owned()),
            ..nothing_on_path(&root.join("empty"))
        }
    };
    let cli = CliInstaller::with_installer(
        root.join("tools"),
        platform(),
        named,
        installer,
        Arc::new(|_: &str| {}),
    );
    assert_eq!(cli.start(name).unwrap_err().status, 409);
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn the_page_names_a_cli_never_a_path() {
    let root = scratch("names");
    let (installer, _, _) = fake_installer(root.join("tools"), None);
    let cli = CliInstaller::with_installer(
        root.join("tools"),
        platform(),
        nothing_on_path(&root),
        installer,
        Arc::new(|_: &str| {}),
    );
    for name in ["/bin/sh", "../claude", "cursor", ""] {
        assert_eq!(
            cli.start(name).unwrap_err(),
            InstallRefused {
                status: 400,
                message: format!("No CLI named {name} can be installed: claude or codex")
            }
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_pins_are_todays() {
    let claude = pin("claude");
    assert_eq!(
        (
            claude.title.as_str(),
            claude.version.as_str(),
            claude.unpack
        ),
        ("Claude Code 2.1.311", "2.1.311", Unpack::File)
    );
    let codex = pin("codex");
    assert_eq!(
        (codex.title.as_str(), codex.version.as_str(), codex.unpack),
        ("Codex CLI 0.169.0", "0.169.0", Unpack::Tar)
    );
    let tools = include_str!("../../../../../../server/tools.js");
    // Claude Code's Windows build is pinned here and not in tools.js: the
    // release's manifest.json names it with this sha256 and size.
    assert_eq!(
        claude.asset("win32-x64"),
        Some(&asset(
            format!("{CLAUDE_CODE}/win32-x64/claude.exe"),
            "966f4b2ce962298f5dee6a942e8c47c8f97392e69e728e622319a98b259c0680",
            298870432,
            "claude.exe"
        ))
    );
    assert_eq!(codex.asset("win32-x64"), None);
    for p in [&claude, &codex] {
        for (platform, asset) in p.platforms.iter().filter(|(p, _)| p != "win32-x64") {
            assert!(
                tools.contains(&format!(
                    "'{platform}': {{ url: `${{{}}}",
                    if p.name == "claude" {
                        "CLAUDE_CODE"
                    } else {
                        "CODEX"
                    }
                )),
                "{platform}"
            );
            assert!(
                tools.contains(&format!(
                    "sha256: '{}', bytes: {}, bin: '{}'",
                    asset.sha256, asset.bytes, asset.bin
                )),
                "{} {platform}",
                p.name
            );
        }
    }
    assert!(tools.contains(&format!("const CLAUDE_CODE = '{CLAUDE_CODE}';")));
    assert!(tools.contains(&format!("const CODEX = '{CODEX}';")));
    let (folder, program) = codex.installed(Path::new("/t"), "linux-x64");
    assert_eq!(
        (folder, program),
        (
            PathBuf::from("/t/codex-0.169.0"),
            PathBuf::from("/t/codex-0.169.0/bin/codex")
        )
    );
    assert_eq!(claude.download_mb("linux-x64"), 294);
}

/// A file server on a free port of 127.0.0.1, and its origin: `/file`, `/tar`,
/// `/missing` (404), `/broken` (500, counted), `/short` (a body that ends
/// before its length).
async fn serve(
    files: Vec<(&'static str, Vec<u8>)>,
    broken: Arc<AtomicUsize>,
) -> (tokio::task::JoinHandle<()>, String) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let files = files.clone();
            let broken = broken.clone();
            tokio::spawn(async move {
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buffer).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    request.extend_from_slice(&buffer[..n]);
                }
                let line = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                let path = line.split(' ').nth(1).unwrap().to_owned();
                let (status, body, length) = match path.as_str() {
                    "/missing" => ("404 Not Found", b"no".to_vec(), 2),
                    "/broken" => {
                        broken.fetch_add(1, Ordering::SeqCst);
                        ("500 Internal Server Error", b"oops".to_vec(), 4)
                    }
                    "/short" => ("200 OK", b"half".to_vec(), 100),
                    other => match files.iter().find(|(p, _)| *p == other) {
                        Some((_, body)) => ("200 OK", body.clone(), body.len()),
                        None => ("404 Not Found", Vec::new(), 0),
                    },
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (server, origin)
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn package() -> Vec<u8> {
    let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
        Vec::new(),
        flate2::Compression::default(),
    ));
    for (name, text, mode) in [
        (
            "codex-package.json",
            "{\"entrypoint\":\"bin/codex\"}",
            0o644,
        ),
        ("bin/codex", "#!/bin/sh\necho codex-cli 0.169.0\n", 0o755),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        archive
            .append_data(&mut header, name, text.as_bytes())
            .unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

fn local_pin(origin: &str, name: &str, unpack: Unpack, path: &str, body: &[u8], bin: &str) -> Pin {
    Pin {
        name: name.to_owned(),
        title: format!("{name} test"),
        version: "9.9.9".to_owned(),
        unpack,
        platforms: vec![(
            platform(),
            Asset {
                url: format!("{origin}{path}"),
                sha256: sha(body),
                bytes: body.len() as u64,
                bin: bin.to_owned(),
            },
        )],
    }
}

#[tokio::test]
async fn the_pinned_download_is_checked_unpacked_into_its_version_folder_and_retried_only_on_the_network()
 {
    let root = scratch("download");
    let tools = root.join("tools");
    let program = b"#!/bin/sh\necho 2.1.311\n".to_vec();
    let tarball = package();
    let broken = Arc::new(AtomicUsize::new(0));
    let (server, origin) = serve(
        vec![("/file", program.clone()), ("/tar", tarball.clone())],
        broken.clone(),
    )
    .await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let s = seen.clone();
    let progress = move |p: Progress| s.lock().unwrap().push(p);

    // A program that is the download: made executable.
    let claude = local_pin(&origin, "claude", Unpack::File, "/file", &program, "claude");
    let installed = install(&claude, &tools, &platform(), &progress)
        .await
        .unwrap();
    assert_eq!(installed, tools.join("claude-9.9.9/claude"));
    assert_eq!(std::fs::read(&installed).unwrap(), program);
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&installed).unwrap().permissions()
        ) & 0o777,
        0o755
    );
    assert_eq!(
        seen.lock().unwrap().last(),
        Some(&Progress {
            done: program.len() as u64,
            total: Some(program.len() as u64)
        })
    );
    // An install that exists is not downloaded again.
    let again = local_pin(
        &origin,
        "claude",
        Unpack::File,
        "/missing",
        &program,
        "claude",
    );
    assert_eq!(
        install(&again, &tools, &platform(), &progress)
            .await
            .unwrap(),
        installed
    );

    // A release package: unpacked, its layout kept.
    let codex = local_pin(&origin, "codex", Unpack::Tar, "/tar", &tarball, "bin/codex");
    let installed = install(&codex, &tools, &platform(), &progress)
        .await
        .unwrap();
    assert_eq!(installed, tools.join("codex-9.9.9/bin/codex"));
    assert!(tools.join("codex-9.9.9/codex-package.json").exists());
    #[cfg(unix)]
    assert_eq!(
        std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&installed).unwrap().permissions()
        ) & 0o777,
        0o755
    );

    // A checksum that does not match fails at once, and nothing is kept.
    let mut wrong = local_pin(&origin, "bad", Unpack::File, "/file", &program, "bad");
    let expected = "1".repeat(64);
    wrong.platforms[0].1.sha256 = expected.clone();
    let error = install(&wrong, &tools, &platform(), &progress)
        .await
        .unwrap_err();
    assert_eq!(
        error,
        format!(
            "{origin}/file downloaded with sha256 {}, but mcbench-lite expects {expected}. It was not kept.",
            sha(&program)
        )
    );
    assert!(!tools.join("bad-9.9.9").exists());
    let left: Vec<_> = std::fs::read_dir(&tools)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        left.iter()
            .all(|n| n == "claude-9.9.9" || n == "codex-9.9.9"),
        "{left:?}"
    );

    // A 4xx fails at once; a 5xx is tried three times in all.
    let missing = local_pin(&origin, "gone", Unpack::File, "/missing", &program, "gone");
    assert_eq!(
        install(&missing, &tools, &platform(), &progress)
            .await
            .unwrap_err(),
        format!("The download of {origin}/missing failed (HTTP 404)")
    );
    let failing = local_pin(&origin, "down", Unpack::File, "/broken", &program, "down");
    assert_eq!(
        install(&failing, &tools, &platform(), &progress)
            .await
            .unwrap_err(),
        format!("The download of {origin}/broken failed: HTTP 500 (tried 3 times)")
    );
    assert_eq!(broken.load(Ordering::SeqCst), 3);
    // A body that ends early is a network failure too.
    let short = local_pin(&origin, "short", Unpack::File, "/short", &program, "short");
    let error = install(&short, &tools, &platform(), &progress)
        .await
        .unwrap_err();
    assert!(
        error.starts_with(&format!("The download of {origin}/short failed: ")),
        "{error}"
    );
    assert!(error.ends_with("(tried 3 times)"), "{error}");

    // A platform with no pin.
    let mut unpinned = claude.clone();
    unpinned.platforms.clear();
    assert_eq!(
        install(&unpinned, &tools, &platform(), &progress)
            .await
            .unwrap_err(),
        format!("mcbench-lite has no pinned claude test for {}", platform())
    );
    server.abort();
    let _ = std::fs::remove_dir_all(&root);
}

/// The watcher hears of the start, of progress at most once per
/// `PROGRESS_EVERY` (a burst is held to one), and of the end, each once and
/// in order; nothing is polled.
#[cfg_attr(windows, ignore = "the provider CLIs have no Windows pins yet")]
#[tokio::test]
async fn a_watcher_hears_each_change_of_an_install_once_in_order() {
    let root = scratch("watched");
    let tools = root.join("tools");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let (step, steps) = tokio::sync::mpsc::unbounded_channel::<Option<Vec<Progress>>>();
    let steps = Arc::new(tokio::sync::Mutex::new(steps));
    let dir = tools.clone();
    let installer: Installer = Arc::new(
        move |pin: Pin, progress: Arc<dyn Fn(Progress) + Send + Sync>| {
            let (steps, dir) = (steps.clone(), dir.clone());
            Box::pin(async move {
                // Each step the test sends is its progress readings, given
                // one after the other at once; `None` ends it.
                while let Some(Some(readings)) = steps.lock().await.recv().await {
                    for bytes in readings {
                        progress(bytes);
                    }
                }
                let (_, program) = pin.installed(&dir, &platform());
                std::fs::create_dir_all(program.parent().unwrap()).unwrap();
                std::fs::write(&program, "").unwrap();
                Ok(program)
            })
        },
    );
    let cli = CliInstaller::with_installer(
        tools,
        platform(),
        nothing_on_path(&bin),
        installer,
        Arc::new(|_: &str| {}),
    );
    let (told, mut heard) = tokio::sync::mpsc::unbounded_channel();
    cli.watch(Arc::new(move |change: (String, InstallView)| {
        told.send(change).unwrap()
    }));
    let at = |done| Progress {
        done,
        total: Some(100),
    };
    let running = |progress| InstallView {
        state: "running".to_owned(),
        progress,
        error: None,
    };
    cli.start("codex").unwrap();
    assert_eq!(
        heard.recv().await.unwrap(),
        ("codex".to_owned(), running(None))
    );
    // A burst within PROGRESS_EVERY of the first: only the first is told.
    step.send(Some(vec![at(10), at(11), at(12)])).unwrap();
    assert_eq!(heard.recv().await.unwrap().1, running(Some(at(10))));
    tokio::time::sleep(PROGRESS_EVERY + Duration::from_millis(20)).await;
    step.send(Some(vec![at(50)])).unwrap();
    assert_eq!(heard.recv().await.unwrap().1, running(Some(at(50))));
    step.send(None).unwrap();
    assert_eq!(
        heard.recv().await.unwrap(),
        (
            "codex".to_owned(),
            InstallView {
                state: "done".to_owned(),
                progress: None,
                error: None
            }
        )
    );
    assert!(heard.try_recv().is_err(), "nothing else was told");
    let _ = std::fs::remove_dir_all(&root);
}

/// A Codex package's programs install execute-only on Linux, which the
/// kernel makes undumpable at exec: a process of the same user cannot open
/// their `/proc/<pid>/fd` (where Codex's stdin, stdout and stderr are the
/// app's pipes). Two proofs: a stand-in, and the real pinned Codex
/// (`MCBENCH_CODEX_PACKAGE`). A program's dumpable state is not in
/// `/proc/<pid>/status`; the kernel shows it as the owner of
/// `/proc/<pid>/fd` (root when not dumpable) and refuses the open.
#[cfg(target_os = "linux")]
mod execute_only {
    use super::*;

    fn mode(path: &Path) -> u32 {
        std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(path).unwrap().permissions())
            & 0o7777
    }

    /// A running program's `/proc/<pid>/fd` owner, and what opening its
    /// stdout through /proc gives (`ok` or the error).
    fn reach(pid: u32) -> (u32, String) {
        let owner = std::os::unix::fs::MetadataExt::uid(
            &std::fs::metadata(format!("/proc/{pid}/fd")).unwrap(),
        );
        let opened = std::fs::OpenOptions::new()
            .append(true)
            .open(format!("/proc/{pid}/fd/1"))
            .map(|_| "ok".to_owned())
            .unwrap_or_else(|e| e.to_string());
        (owner, opened)
    }

    fn uid() -> u32 {
        // SAFETY: getuid cannot fail.
        unsafe { libc::getuid() }
    }

    /// The first file on PATH named `name`.
    fn on_path(name: &str) -> PathBuf {
        std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|dir| dir.join(name))
            .find(|file| file.is_file())
            .unwrap_or_else(|| panic!("{name} on PATH"))
    }

    fn tarball(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::fast(),
        ));
        for (name, body, mode) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(*mode);
            header.set_cksum();
            archive.append_data(&mut header, name, *body).unwrap();
        }
        archive.into_inner().unwrap().finish().unwrap()
    }

    // The stand-in: a package whose program is a compiled one that waits
    // (the system's `sleep`), beside a shared library (the libc this test
    // runs on), a data file marked executable and the manifest. Only the
    // program becomes 0111; started, it is not dumpable, where the same
    // program readable is.
    #[tokio::test]
    async fn a_packages_programs_install_execute_only_and_run_undumpable() {
        let root = scratch("execute-only");
        let tools = root.join("tools");
        let program = std::fs::read(on_path("sleep")).unwrap();
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
        let library = maps
            .lines()
            .filter_map(|l| l.split_whitespace().nth(5))
            .find(|p| p.contains("/libc.so"))
            .expect("this test's libc in /proc/self/maps");
        let library = std::fs::read(library).unwrap();
        let tarball = tarball(&[
            (
                "codex-package.json",
                b"{\"entrypoint\":\"bin/codex\"}",
                0o644,
            ),
            ("bin/codex", &program, 0o755),
            ("lib/libc.so.6", &library, 0o755),
            ("resources/runtime.json", b"{}", 0o755),
        ]);
        let (server, origin) = serve(vec![("/tar", tarball.clone())], Default::default()).await;
        let codex = local_pin(&origin, "codex", Unpack::Tar, "/tar", &tarball, "bin/codex");
        let installed = install(&codex, &tools, &platform(), &|_| {}).await.unwrap();
        server.abort();
        let folder = installed.parent().unwrap().parent().unwrap();
        let modes: Vec<(&str, u32)> = [
            "bin/codex",
            "lib/libc.so.6",
            "resources/runtime.json",
            "codex-package.json",
        ]
        .into_iter()
        .map(|f| (f, mode(&folder.join(f))))
        .collect();
        assert_eq!(
            modes,
            [
                ("bin/codex", 0o111),
                ("lib/libc.so.6", 0o755),
                ("resources/runtime.json", 0o755),
                ("codex-package.json", 0o644)
            ]
        );
        let readable = root.join("readable-sleep");
        std::fs::write(&readable, &program).unwrap();
        std::fs::set_permissions(
            &readable,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        for (what, file, closed) in [
            ("installed", &installed, true),
            ("readable", &readable, false),
        ] {
            let mut child = std::process::Command::new(file)
                .arg("600")
                .stdout(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            let (owner, opened) = reach(child.id());
            child.kill().unwrap();
            child.wait().unwrap();
            println!(
                "{what} {}: /proc/<pid>/fd owner {owner}, its fd/1 opened: {opened}",
                file.display()
            );
            if closed {
                assert_eq!(owner, 0, "the installed program is not dumpable");
                assert!(opened.contains("Permission denied"), "{opened}");
            } else {
                assert_eq!(owner, uid(), "the same program readable is dumpable");
                assert_eq!(opened, "ok");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // The real pinned Codex 0.169.0, installed by `install` from the
    // package `MCBENCH_CODEX_PACKAGE` names (served locally, checked
    // against the pin's sha256): every program its ELF headers name is
    // 0111 and the rest keep their modes; `codex exec --json
    // --skip-git-repo-check -` (the box's command, with an empty CODEX_HOME:
    // no sign-in) waits on its stdin, not dumpable, and with stdin closed
    // exits 1 saying no prompt came.
    #[tokio::test]
    #[ignore = "needs MCBENCH_CODEX_PACKAGE=<the pinned Linux codex-package tar.gz>"]
    async fn the_pinned_codex_installs_its_programs_execute_only_and_runs_undumpable() {
        let package = std::fs::read(
            std::env::var("MCBENCH_CODEX_PACKAGE")
                .expect("MCBENCH_CODEX_PACKAGE=<the pinned Linux codex-package tar.gz> is needed"),
        )
        .expect("MCBENCH_CODEX_PACKAGE names a readable file");
        let root = scratch("execute-only-codex");
        let mut codex = pin("codex");
        let (server, origin) = serve(vec![("/codex", package.clone())], Default::default()).await;
        for (_, asset) in &mut codex.platforms {
            asset.url = format!("{origin}/codex");
        }
        let installed = install(&codex, &root.join("tools"), &platform(), &|_| {})
            .await
            .unwrap();
        server.abort();
        let folder = installed.parent().unwrap().parent().unwrap();
        // The package as it was, to read each file's ELF header.
        let reference = root.join("reference");
        tar::Archive::new(flate2::read::GzDecoder::new(package.as_slice()))
            .unpack(&reference)
            .unwrap();
        let mut programs = Vec::new();
        let mut stack = vec![reference.clone()];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = path.strip_prefix(&reference).unwrap();
                let before = mode(&path);
                let program =
                    before & 0o111 != 0 && mcbench_sandbox::proxy::is_program(&path).unwrap();
                let after = mode(&folder.join(name));
                if program {
                    programs.push(name.display().to_string());
                    assert_eq!(after, 0o111, "{} is a program", name.display());
                } else {
                    assert_eq!(after, before, "{} keeps its mode", name.display());
                }
            }
        }
        programs.sort();
        println!("execute-only: {programs:?}");
        assert!(programs.contains(&"bin/codex".to_owned()), "{programs:?}");
        let home = root.join("codex-home");
        std::fs::create_dir_all(&home).unwrap();
        let mut child = std::process::Command::new(&installed)
            .args(["exec", "--json", "--skip-git-repo-check", "-"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", &home)
            .env("CODEX_HOME", &home)
            .current_dir(&root)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let (owner, opened) = reach(child.id());
        println!(
            "codex exec (pid {}): /proc/<pid>/fd owner {owner}, its fd/1 opened: {opened}",
            child.id()
        );
        drop(child.stdin.take());
        let out = child.wait_with_output().unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        println!(
            "codex exec exited {:?}: {}",
            out.status.code(),
            stderr.trim()
        );
        assert_eq!(owner, 0, "Codex is not dumpable");
        assert!(opened.contains("Permission denied"), "{opened}");
        assert_eq!(out.status.code(), Some(1));
        assert!(stderr.contains("No prompt provided via stdin."), "{stderr}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The patch the apply_patch tests give Codex: one file of the
    /// workspace changed, and what it holds before and after.
    const PATCH: &str = "*** Begin Patch\n*** Update File: hello.txt\n@@\n-hello\n+hello from the box\n*** End Patch";
    const BEFORE: &str = "hello\n";
    const AFTER: &str = "hello from the box\n";

    // apply_patch from the execute-only install, in Codex's box: the real
    // pinned Codex 0.169.0 installed through the pin, run as its arg0
    // dispatch runs apply_patch (`codex-rs/arg0/src/lib.rs`: argv[1]
    // `--codex-run-as-apply-patch`, the `CODEX_CORE_APPLY_PATCH_ARG1` of
    // `codex-rs/apply-patch/src/lib.rs`, argv[2] the patch, applied against
    // the working folder), which is what the `apply_patch` alias Codex puts
    // on PATH and Windows' `.bat` alias run. Codex 0.169.0 applies the
    // model's apply_patch call in its own process when no sandbox is asked
    // for (`core/src/tools/runtimes/apply_patch.rs`, the unsandboxed
    // `LocalFileSystem`), as the box's `danger-full-access` has it. The
    // workspace is the box's `/workspace`; no sign-in, no network.
    #[tokio::test]
    #[ignore = "needs MCBENCH_CODEX_PACKAGE, MCBENCH_SANDBOX_ROOTFS, MCBENCH_SANDBOX_ROOTFS_SHA256 and MCBENCH_SANDBOX_AGENT_STATIC"]
    async fn the_pinned_codex_applies_a_patch_in_its_box_from_its_execute_only_install() {
        let var = |key: &str| std::env::var(key).unwrap_or_else(|_| panic!("{key} is needed"));
        let package = std::fs::read(var("MCBENCH_CODEX_PACKAGE"))
            .expect("MCBENCH_CODEX_PACKAGE names a readable file");
        let root = scratch("apply-patch");
        let mut codex = pin("codex");
        let (server, origin) = serve(vec![("/codex", package)], Default::default()).await;
        for (_, asset) in &mut codex.platforms {
            asset.url = format!("{origin}/codex");
        }
        let installed = install(&codex, &root.join("tools"), &platform(), &|_| {})
            .await
            .unwrap();
        server.abort();
        assert_eq!(mode(&installed), 0o111, "installed execute-only");
        let unpacked = mcbench_sandbox::linux::rootfs::unpack(
            Path::new(&var("MCBENCH_SANDBOX_ROOTFS")),
            &var("MCBENCH_SANDBOX_ROOTFS_SHA256"),
            &root.join("rootfs"),
        )
        .unwrap();
        for folder in ["home", "workspace", "run/sock"] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::write(root.join("auth.json"), "{}").unwrap();
        std::fs::write(root.join("workspace/hello.txt"), BEFORE).unwrap();
        let run = mcbench_sandbox::proxy::codex_command(&mcbench_sandbox::proxy::CodexRun {
            rootfs: unpacked.rootfs,
            image_env: unpacked.image.env,
            relay: var("MCBENCH_SANDBOX_AGENT_STATIC").into(),
            install: installed.parent().unwrap().parent().unwrap().to_path_buf(),
            home: root.join("home"),
            auth_file: root.join("auth.json"),
            workspace: root.join("workspace"),
            sock_dir: root.join("run/sock"),
            allowed_hosts: Vec::new(),
            proxy_log: root.join("egress.ndjson"),
            command: ["/opt/codex/bin/codex", "--codex-run-as-apply-patch", PATCH]
                .map(String::from)
                .into(),
        })
        .unwrap();
        let out = tokio::process::Command::new(&run.command)
            .args(&run.args)
            .env_clear()
            .envs(run.env.iter().cloned())
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        (run.close_proxy)().unwrap();
        let said = format!(
            "exit {:?}; stdout: {}; stderr: {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).trim(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
        println!("codex --codex-run-as-apply-patch in its box: {said}");
        assert_eq!(
            std::fs::read_to_string(root.join("workspace/hello.txt")).unwrap(),
            AFTER,
            "the patched file on the host; Codex said {said}"
        );
        assert_eq!(out.status.code(), Some(0), "Codex said {said}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
