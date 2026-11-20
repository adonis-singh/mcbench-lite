//! Confinement: native code in the child's place under the platform
//! sandbox, the port of build-sandbox.test.js's escape probe
//! (`test/fixtures/build-escape-probe.js`) for a child whose realm holds no
//! Node: what code that took over the whole process reaches. Today's probe
//! ran under Node's permission model and the OS sandbox; the V8 child has no
//! permission model (its JavaScript has no file, network or process API), so
//! here the OS sandbox alone must refuse everything, as today's "the macOS
//! profile alone" and "the Linux mount namespace alone" tests require.
//!
//! The probe is this test binary in the child's place (the shipped child
//! holds no test code), started by the platform sandbox exactly as the
//! runner starts `mcbench-build-child`, with the probe's variable added.
//!
//! Probes of Node's own surface (a worker, an addon, `process.binding`, the
//! inspector, `node:sqlite`, `require` outside the grant) have no
//! counterpart in a process without Node and are not ported.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::common::{Test, test, test_binary};

/// The variable that makes this test binary the probe.
pub const PROBE: &str = "MCBENCH_BUILD_ESCAPE_PROBE";

fn code(error: &std::io::Error) -> String {
    let names = [
        (libc::EPERM, "EPERM"),
        (libc::EACCES, "EACCES"),
        (libc::ENOENT, "ENOENT"),
        (libc::EROFS, "EROFS"),
        (libc::ESRCH, "ESRCH"),
        (libc::ENOTDIR, "ENOTDIR"),
        (libc::ENETUNREACH, "ENETUNREACH"),
        (libc::EHOSTUNREACH, "EHOSTUNREACH"),
        (libc::ECONNREFUSED, "ECONNREFUSED"),
        (libc::EADDRNOTAVAIL, "EADDRNOTAVAIL"),
        (libc::ETIMEDOUT, "ETIMEDOUT"),
    ];
    match error.raw_os_error() {
        Some(raw) => names
            .iter()
            .find(|(errno, _)| *errno == raw)
            .map_or_else(|| format!("errno {raw}"), |(_, name)| name.to_string()),
        None => format!("{}", error.kind()),
    }
}

/// What of the probe's own process leads out of its box through /proc (it
/// is the box's only process, its pid 1): a descriptor other than 0, 1 and
/// 2, one that is neither a pipe nor a path, or a descriptor, cwd, root or
/// program whose file is not the one the box sees at its path. `none`, or
/// the first few of them.
#[cfg(unix)]
fn links_out_of_the_box() -> std::io::Result<String> {
    use std::os::unix::fs::MetadataExt;
    let mut links: Vec<(String, PathBuf)> = std::fs::read_dir("/proc/self/fd")?
        .map(|entry| {
            entry.map(|entry| {
                (
                    format!("fd/{}", entry.file_name().to_string_lossy()),
                    entry.path(),
                )
            })
        })
        .collect::<std::io::Result<_>>()?;
    for name in ["cwd", "root", "exe"] {
        links.push((name.to_string(), Path::new("/proc/self").join(name)));
    }
    let mut out = Vec::new();
    for (name, link) in links {
        // The listing's own descriptor is closed by now.
        let Ok(target) = std::fs::read_link(&link) else {
            continue;
        };
        let shown = target.to_string_lossy().into_owned();
        let descriptor = name.starts_with("fd/");
        let standard = matches!(name.as_str(), "fd/0" | "fd/1" | "fd/2");
        let inside = if shown.starts_with("pipe:[") {
            standard
        } else {
            (standard || !descriptor)
                && target.is_absolute()
                && matches!(
                    (std::fs::metadata(&link), std::fs::metadata(&target)),
                    (Ok(at), Ok(seen)) if at.dev() == seen.dev() && at.ino() == seen.ino()
                )
        };
        if !inside {
            out.push(format!("{name} {shown}"));
        }
    }
    Ok(if out.is_empty() {
        "none".to_string()
    } else {
        out.join(",")
    })
}

#[cfg(not(unix))]
fn links_out_of_the_box() -> std::io::Result<String> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// `kill(pid, 0)`: whether the parent can be signalled.
#[cfg(unix)]
fn signal(pid: i64) -> std::io::Result<String> {
    // SAFETY: kill with signal 0 sends nothing; it only checks the target.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } == 0 {
        Ok("0".into())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(unix))]
fn signal(_: i64) -> std::io::Result<String> {
    unimplemented!("the Windows sandbox and its probe are not built yet")
}

/// The probe, this test binary run under the sandbox with [`PROBE`] set:
/// its parameters come on stdin, what it reached goes to stdout as JSON.
pub fn probe() -> ! {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let params: Value = serde_json::from_str(&input).unwrap();
    // As the child confines itself before it runs JavaScript.
    #[cfg(target_os = "linux")]
    mcbench_build::confine::install();
    let mut results = BTreeMap::new();
    let mut attempt = |name: String, result: std::io::Result<String>| {
        let text = match result {
            Ok(value) => format!("allowed:{}", value.chars().take(80).collect::<String>()),
            Err(error) => format!("denied:{}", code(&error)),
        };
        results.insert(name, text);
    };
    let read =
        |file: &str| std::fs::read(file).map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let list = |dir: &str| {
        std::fs::read_dir(dir).and_then(|entries| {
            let mut names = entries
                .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
                .collect::<std::io::Result<Vec<_>>>()?;
            names.sort();
            Ok(names.join(","))
        })
    };
    let canary = params["canary"].as_str().unwrap();
    attempt("readCanary".into(), read(canary));
    attempt("readPasswd".into(), read("/etc/passwd"));
    attempt("readProcEnviron".into(), read("/proc/1/environ"));
    attempt("listRoot".into(), list("/"));
    // The mount points of the host's file shares the child sees (9p, WSL's
    // drvfs, virtiofs): the child is given none.
    attempt(
        "shareMounts".into(),
        read("/proc/self/mountinfo").map(|mounts| {
            mounts
                .lines()
                .filter(|line| {
                    line.split_once(" - ").is_some_and(|(_, after)| {
                        matches!(after.split(' ').next(), Some("9p" | "drvfs" | "virtiofs"))
                    })
                })
                .map(|line| line.split(' ').nth(4).unwrap_or_default().to_string())
                .collect::<Vec<_>>()
                .join(",")
        }),
    );
    // WSL's GPU libraries and Windows' driver store, mounted by WSL under
    // the distro's /usr.
    attempt("list /usr/lib/wsl".into(), list("/usr/lib/wsl"));
    for file in params["read"].as_array().unwrap() {
        let file = file.as_str().unwrap();
        attempt(format!("read {file}"), read(file));
    }
    for dir in params["list"].as_array().unwrap() {
        let dir = dir.as_str().unwrap();
        attempt(format!("list {dir}"), list(dir));
    }
    for file in params["write"].as_array().unwrap() {
        let file = file.as_str().unwrap();
        attempt(
            format!("write {file}"),
            std::fs::write(file, "escaped").map(|()| String::new()),
        );
    }
    attempt(
        "writeFile".into(),
        std::fs::write(params["writeTarget"].as_str().unwrap(), "escaped").map(|()| String::new()),
    );
    attempt(
        "spawn".into(),
        std::process::Command::new("/usr/bin/id")
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned()),
    );
    attempt(
        "signalParent".into(),
        signal(params["parentPid"].as_i64().unwrap()),
    );
    attempt("pid".into(), Ok(std::process::id().to_string()));
    attempt("linksOutOfTheBox".into(), links_out_of_the_box());
    let env: Vec<String> = std::env::vars()
        .filter(|(key, value)| {
            key != PROBE && key != "MCBENCH_MC_Y_OFFSET" && !(key == "PWD" && value == "/")
        })
        .map(|(key, _)| key)
        .collect();
    attempt(
        "env".into(),
        if env.is_empty() {
            Ok("empty".into())
        } else {
            Err(std::io::Error::other(env.join(",")))
        },
    );
    let connect = |to: &str| {
        let address = to.parse().unwrap();
        TcpStream::connect_timeout(&address, Duration::from_secs(2))
            .map(|_| "connected".to_string())
    };
    attempt(
        "tcpLoopback".into(),
        connect(&format!("127.0.0.1:{}", params["port"])),
    );
    attempt("tcpInternet".into(), connect("1.1.1.1:80"));
    attempt(
        "dns".into(),
        ("example.com", 80)
            .to_socket_addrs()
            .map(|mut addresses| addresses.next().map(|a| a.to_string()).unwrap_or_default()),
    );
    attempt(
        "listen".into(),
        TcpListener::bind("0.0.0.0:0")
            .map(|listener| listener.local_addr().unwrap().port().to_string()),
    );
    let mut out = std::io::stdout().lock();
    out.write_all(serde_json::to_string(&results).unwrap().as_bytes())
        .unwrap();
    out.flush().unwrap();
    std::process::exit(0)
}

/// What the probe reached, with today's host files made under a new
/// folder of `root`: a canary, an app data folder with settings holding an
/// API key and another chat's world; ~/.ssh, /etc and the binary's folder
/// listed; / and /dev/shm written.
struct Probe {
    results: BTreeMap<String, String>,
    secrets: Vec<String>,
    writes: Vec<String>,
    wrote: bool,
    connections: usize,
    out: String,
    /// The program as the probe sees it.
    seen: String,
}

/// The probe in the child's place: this test binary, or on Windows the Linux
/// build of it that `MCBENCH_BUILD_PROBE` names (the sandbox runs Linux
/// programs there), and its path as the probe sees it.
fn probe_program() -> (PathBuf, String) {
    #[cfg(windows)]
    {
        let program = PathBuf::from(std::env::var_os(PROBE_PROGRAM).unwrap_or_else(|| {
            panic!("{PROBE_PROGRAM}=<the Linux x86_64 build of this test binary> is needed")
        }));
        let seen = mcbench_build::sandbox::windows::Wsl::new()
            .inside(&program)
            .unwrap();
        (program, seen)
    }
    #[cfg(not(windows))]
    {
        let program = test_binary();
        let seen = program.to_str().unwrap().to_string();
        (program, seen)
    }
}

/// On Windows, the Linux build of this test binary that the escape probe
/// runs inside the distro.
#[cfg(windows)]
pub const PROBE_PROGRAM: &str = "MCBENCH_BUILD_PROBE";

/// What the probe would reach in the WSL2 distro if it were not confined:
/// the chats' folders and the root filesystem (the host's files by drvfs),
/// the bash sandbox's own folder, the distro's configuration and WSL's init.
const DISTRO_FILES: [&str; 5] = [
    "/mnt/sandbox",
    "/mnt/rootfs",
    "/var/mcbench",
    "/etc/wsl.conf",
    "/init",
];

async fn run_probe(root: &Path) -> Probe {
    let dir = root.join(format!("mcbench-escape-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("data/worlds/other-chat")).unwrap();
    let dir = std::fs::canonicalize(&dir).unwrap();
    let canary = dir.join("reference-build.json");
    let write_target = dir.join("written.txt");
    std::fs::write(&canary, "TOP-SECRET-REFERENCE-BUILD").unwrap();
    std::fs::write(
        dir.join("data/settings.json"),
        r#"{"openrouterApiKey":"TOP-SECRET-API-KEY"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("data/worlds/other-chat/world.json"),
        "TOP-SECRET-OTHER-CHAT",
    )
    .unwrap();
    let home =
        PathBuf::from(std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap());
    let reads = [
        dir.join("data/settings.json"),
        dir.join("data/worlds/other-chat/world.json"),
    ];
    let lists = [home.join(".ssh"), dir.join("data/worlds"), dir.clone()];
    let path = |p: &PathBuf| p.to_str().unwrap().to_string();
    let (program, seen) = probe_program();
    let binary_dir = seen.rsplit_once('/').unwrap().0.to_string();
    let distro: &[&str] = if cfg!(windows) { &DISTRO_FILES } else { &[] };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let writes = vec![
        "/mcbench-escape.txt".to_string(),
        "/dev/shm/mcbench-escape.txt".to_string(),
    ];
    let params = json!({
        "canary": path(&canary),
        "writeTarget": path(&write_target),
        "port": port,
        "parentPid": std::process::id(),
        "read": reads.iter().map(path).chain([seen.clone()]).chain(distro.iter().map(|f| f.to_string())).collect::<Vec<_>>(),
        "list": lists.iter().map(path).chain(["/etc".to_string(), binary_dir]).chain(distro.iter().map(|f| f.to_string())).collect::<Vec<_>>(),
        "write": writes,
    });
    let sandbox = mcbench_build::sandbox::platform().unwrap();
    let mut child = sandbox
        .spawn(&program, &[(PROBE, OsStr::new("1"))])
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(params.to_string().as_bytes())
        .await
        .unwrap();
    drop(stdin);
    let mut stderr = child.stderr.take().unwrap();
    let stderr = tokio::spawn(async move {
        let mut text = String::new();
        stderr.read_to_string(&mut text).await.unwrap();
        text
    });
    let mut out = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut out)
        .await
        .unwrap();
    child.wait().await.unwrap();
    let stderr = stderr.await.unwrap();
    let mut connections = 0;
    while listener.accept().is_ok() {
        connections += 1;
    }
    let results: BTreeMap<String, String> = serde_json::from_str(&out)
        .unwrap_or_else(|_| panic!("the probe printed {out:?}, stderr {stderr:?}"));
    let wrote = write_target.exists();
    std::fs::remove_dir_all(&dir).unwrap();
    Probe {
        results,
        secrets: reads
            .iter()
            .map(|file| format!("read {}", path(file)))
            .chain(lists.iter().map(|folder| format!("list {}", path(folder))))
            .chain(
                distro
                    .iter()
                    .flat_map(|f| [format!("read {f}"), format!("list {f}")]),
            )
            .collect(),
        seen,
        writes: writes.iter().map(|file| format!("write {file}")).collect(),
        wrote,
        connections,
        out,
    }
}

fn is_denied(probe: &Probe, name: &str, allowed: &[&str]) {
    let result = &probe.results[name];
    assert!(
        allowed
            .iter()
            .any(|code| result == &format!("denied:{code}")),
        "{name}: {result}, expected denied:{allowed:?}"
    );
}

fn common_checks(probe: &Probe) {
    println!("    the probe reached: {}", probe.out);
    assert!(!probe.wrote, "the probe wrote its target");
    assert_eq!(
        probe.connections, 0,
        "the probe reached the parent's listener"
    );
    assert!(
        !probe.out.contains("SECRET") && !probe.out.contains("root:x:0"),
        "{}",
        probe.out
    );
    assert_eq!(probe.results["env"], "allowed:empty");
}

pub fn tests() -> Vec<Test> {
    let mut tests = Vec::new();
    if cfg!(target_os = "macos") {
        tests.push(test(
            "escape: the macOS profile alone refuses every host file, the network, exec and signals; the binary stays readable",
            || async {
                let probe = run_probe(&std::env::temp_dir()).await;
                common_checks(&probe);
                for name in ["readCanary", "readPasswd", "list /etc", "writeFile", "spawn", "listen"]
                    .iter()
                    .map(|name| name.to_string())
                    .chain(probe.secrets.clone())
                {
                    is_denied(&probe, &name, &["EPERM"]);
                }
                for name in &probe.writes {
                    is_denied(&probe, name, &["EPERM", "ENOENT"]);
                }
                is_denied(&probe, "readProcEnviron", &["ENOENT", "EPERM"]);
                for name in ["tcpLoopback", "tcpInternet"] {
                    is_denied(&probe, name, &["EPERM"]);
                }
                assert!(probe.results["dns"].starts_with("denied:"), "dns: {}", probe.results["dns"]);
                is_denied(&probe, "signalParent", &["EPERM", "ESRCH"]);
                // Only the listing of / and the binary itself are readable.
                assert!(probe.results["listRoot"].starts_with("allowed:"));
                let program = test_binary();
                assert!(probe.results[&format!("read {}", program.display())].starts_with("allowed:"));
                is_denied(&probe, &format!("list {}", program.parent().unwrap().display()), &["EPERM"]);
            },
        ));
    }
    // On Windows the same probe runs inside the app's WSL2 distro, where
    // the host files beside the build are the Windows host's (never mounted
    // in the distro) and the distro's own (the chats' folders by drvfs, the
    // bash sandbox's folder, WSL's init) are added to the list.
    if cfg!(target_os = "linux")
        || (cfg!(windows) && std::env::var_os(mcbench_build::sandbox::wsl::DISTRO_VAR).is_some())
    {
        tests.push(test(
            "escape: the Linux namespaces hide every host file but the binary, and the seccomp filter refuses programs and sockets",
            || async {
                // As today's test: host files beside the build (here in the
                // target folder that holds the binary) are not there.
                let probe = run_probe(Path::new(env!("CARGO_TARGET_TMPDIR"))).await;
                common_checks(&probe);
                for name in ["readCanary", "readPasswd"]
                    .iter()
                    .map(|name| name.to_string())
                    .chain(probe.secrets.clone())
                {
                    is_denied(&probe, &name, &["ENOENT"]);
                }
                // On Windows the target is a Windows path, which in the
                // distro names a file in the read-only root.
                is_denied(&probe, "writeFile", if cfg!(windows) { &["EROFS"] } else { &["ENOENT"] });
                for name in &probe.writes {
                    is_denied(&probe, name, &["EROFS"]);
                }
                let (dir, name) = probe.seen.rsplit_once('/').unwrap();
                assert!(probe.results[&format!("read {}", probe.seen)].starts_with("allowed:"));
                assert_eq!(probe.results[&format!("list {dir}")], format!("allowed:{name}"));
                if cfg!(windows) {
                    // The WSL2 distro's userland has neither /etc/ld.so.cache
                    // nor /etc/localtime, so the box binds neither and has no
                    // /etc at all.
                    assert_eq!(probe.results["list /etc"], "denied:ENOENT");
                } else {
                    let etc = probe.results["list /etc"].trim_start_matches("allowed:").to_string();
                    assert!(["", "ld.so.cache", "localtime", "ld.so.cache,localtime"].contains(&etc.as_str()), "/etc: {etc}");
                }
                // No file share of the host, and nothing of Windows under /usr.
                assert_eq!(probe.results["shareMounts"], "allowed:");
                is_denied(&probe, "list /usr/lib/wsl", &["ENOENT"]);
                let first = probe.seen.split('/').nth(1).unwrap().to_string();
                let visible = ["bin", "dev", "etc", "lib", "lib64", "proc", "sbin", "usr", first.as_str()];
                for entry in probe.results["listRoot"].trim_start_matches("allowed:").split(',') {
                    assert!(visible.contains(&entry), "/{entry} is visible");
                }
                // The seccomp filter: no program starts and no socket opens.
                for name in ["spawn", "tcpLoopback", "tcpInternet", "listen"] {
                    is_denied(&probe, name, &["EPERM"]);
                }
                assert!(probe.results["dns"].starts_with("denied:"), "dns: {}", probe.results["dns"]);
                is_denied(&probe, "signalParent", &["ESRCH"]);
                assert_eq!(probe.results["pid"], "allowed:1");
                // Nothing of the child's process leads out of its box
                // through /proc: its descriptors are stdin, stdout and
                // stderr, its cwd, root and program the box's own.
                assert_eq!(probe.results["linksOutOfTheBox"], "allowed:none");
            },
        ));
    }
    tests
}
