//! The WSL2 distro itself (the Windows counterpart of `macos_vm.rs`): it is
//! imported from the guest base, its agent answers, the agent ends with its
//! sandbox and with the process that started it, and a distro that cannot
//! start makes the sandbox unavailable in the unavailable form.
//!
//! Needs WSL installed, `MCBENCH_WSL_DISTRO` (the test's distro, imported
//! by the first test that runs), `MCBENCH_SANDBOX_GUEST` and
//! `MCBENCH_SANDBOX_GUEST_SHA256` (the x86_64 guest base) beside
//! `MCBENCH_SANDBOX_ROOTFS` and `MCBENCH_SANDBOX_ROOTFS_SHA256`, and
//! `MCBENCH_SANDBOX_SCRATCH`; run with `cargo test -p mcbench-sandbox --test
//! windows_wsl --test linux_box -- --ignored --test-threads 1`.

#![cfg(windows)]

mod support;

use std::time::{Duration, Instant};

use mcbench_sandbox::windows::{WSL_MISSING, WSL_UNAVAILABLE, WslDistro, WslSetup, WslState};
use mcbench_sandbox::{GuestBase, Sandbox, SandboxError};
use support::*;

const MIB: u64 = 1024 * 1024;

fn distro() -> String {
    std::env::var("MCBENCH_WSL_DISTRO").expect("MCBENCH_WSL_DISTRO names the test's distro")
}

/// What busybox's `sh -c script` prints as root in the test's distro.
fn in_distro(script: &str) -> String {
    let output = std::process::Command::new("wsl.exe")
        .args([
            "--distribution",
            &distro(),
            "--user",
            "root",
            "--cd",
            "/",
            "--exec",
        ])
        .args(["/usr/bin/busybox", "sh", "-c", script])
        .env("WSL_UTF8", "1")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// What busybox's `sh -c script` prints (both streams) as `user` in the
/// test's distro.
fn in_distro_as(user: &str, script: &str) -> String {
    let output = std::process::Command::new("wsl.exe")
        .args([
            "--distribution",
            &distro(),
            "--user",
            user,
            "--cd",
            "/",
            "--exec",
        ])
        .args(["/usr/bin/busybox", "sh", "-c", script])
        .env("WSL_UTF8", "1")
        .output()
        .unwrap();
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// The guest agents running in the distro (`guest -`; the boxes' keepers
/// are the same program, `keep`).
fn agents() -> Vec<String> {
    in_distro("ps -o pid,args | grep '[m]cbench-sandbox-agent guest'")
        .lines()
        .map(str::to_string)
        .collect()
}

async fn wait_none(what: impl Fn() -> Vec<String>, within: Duration) -> bool {
    let deadline = Instant::now() + within;
    loop {
        if what().is_empty() {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

#[test]
#[ignore = "needs WSL installed"]
fn wsl_is_ready() {
    assert_eq!(mcbench_sandbox::windows::wsl_state(), Ok(WslState::Ready));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL, MCBENCH_WSL_DISTRO, MCBENCH_SANDBOX_GUEST and MCBENCH_SANDBOX_ROOTFS (and their sha256)"]
async fn the_distro_starts_runs_a_command_and_its_agent_ends_with_its_sandbox() {
    let scratch = Scratch::new("wsl");
    let started = Instant::now();
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    eprintln!("import or start, and prepare: {:?}", started.elapsed());
    assert_eq!(agents().len(), 1, "one agent: {:?}", agents());
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "wsl");
    let run = run(
        sandbox,
        &ws,
        "uname -m; id; echo hi > /work/hi.txt; ls -ln /work; cat /proc/self/cgroup /sys/fs/cgroup/memory.max; ls /mnt 2>&1; ls /init 2>&1; cat /proc/mounts | grep -c drvfs",
        10_000.0,
        false,
    )
    .await
    .unwrap();
    eprintln!("{}", run.formatted);
    assert_eq!(
        std::fs::read_to_string(ws.host.join("hi.txt")).unwrap(),
        "hi\n"
    );
    assert!(
        run.formatted.contains("uid=1000 gid=1000"),
        "{}",
        run.formatted
    );
    // Inside the box: no Windows drive, no WSL init, the one drvfs mount
    // of /work only.
    assert!(
        run.formatted.contains("ls: cannot access '/init'"),
        "{}",
        run.formatted
    );
    let config = in_distro("cat /etc/wsl.conf; ls /mnt; grep -c drvfs /proc/mounts");
    assert!(config.contains("[automount]\nenabled=false"), "{config}");
    assert!(config.contains("[interop]\nenabled=false"), "{config}");
    assert!(config.contains("[gpu]\nenabled=false"), "{config}");
    // With the GPU off WSL mounts nothing under /usr/lib/wsl.
    assert_eq!(
        in_distro("grep -c ' /usr/lib/wsl' /proc/mounts").trim(),
        "0"
    );
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
    assert!(
        wait_none(agents, Duration::from_secs(15)).await,
        "the agent {:?} outlived the sandbox's shutdown",
        agents()
    );
}

// sandbox-state.test.js's case on Windows (linux_box.rs has Linux's): no box
// outlives the sandbox that made it.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL, MCBENCH_WSL_DISTRO, MCBENCH_SANDBOX_GUEST and MCBENCH_SANDBOX_ROOTFS (and their sha256)"]
async fn no_box_outlives_the_sandbox_that_made_it() {
    let scratch = Scratch::new("wsl-state");
    let ws = ws(&scratch, "left-behind");
    {
        let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
        let sandbox = any.sandbox();
        run(sandbox, &ws, "true", 5000.0, true).await.unwrap();
        let out = run(sandbox, &ws, "sleep 3000 & echo started", 5000.0, false)
            .await
            .unwrap();
        assert_eq!(out.exit_code, 0);
        assert!(in_distro("ps -o args | grep '[s]leep 3000'").contains("sleep 3000"));
        // The sandbox is dropped here without shutdown, as when the app ends.
    }
    let boxes = || {
        in_distro("ps -o args | grep -e '[s]leep 3000' -e '[s]leep infinity'")
            .lines()
            .map(str::to_string)
            .collect::<Vec<_>>()
            .join("\n")
            .split_whitespace()
            .map(str::to_string)
            .collect()
    };
    assert!(
        wait_none(boxes, Duration::from_secs(15)).await,
        "a box outlived its sandbox: {}",
        in_distro("ps -o pid,args")
    );
    assert!(wait_none(agents, Duration::from_secs(15)).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL, MCBENCH_WSL_DISTRO, MCBENCH_SANDBOX_GUEST and MCBENCH_SANDBOX_ROOTFS (and their sha256)"]
async fn the_agent_ends_when_the_process_that_started_it_is_killed() {
    if let Ok(marker) = std::env::var("MCBENCH_SANDBOX_WSL_CHILD") {
        // The child: start the distro's agent and a box, say so, and wait to
        // be killed.
        let scratch = Scratch::new("wsl-child");
        let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
        let ws = ws(&scratch, "wsl-child");
        run(
            any.sandbox(),
            &ws,
            "sleep 3000 & echo started",
            5000.0,
            false,
        )
        .await
        .unwrap();
        std::fs::write(&marker, "up").unwrap();
        let _keep = any;
        tokio::time::sleep(Duration::from_secs(120)).await;
        panic!("the child was not killed");
    }
    let scratch = Scratch::new("wsl-kill");
    let marker = scratch.0.join("up");
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_agent_ends_when_the_process_that_started_it_is_killed",
            "--ignored",
            "--nocapture",
        ])
        .env("MCBENCH_SANDBOX_WSL_CHILD", &marker)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(120);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "the child's agent did not come up"
        );
        assert!(child.try_wait().unwrap().is_none(), "the child ended early");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(agents().len(), 1, "the child's agent: {:?}", agents());
    // TerminateProcess: the app dying with no word to anyone.
    child.kill().unwrap();
    child.wait().unwrap();
    let prefix = format!("mcbench-box-wsl-child-{}-", child.id());
    for entry in std::fs::read_dir(scratch.0.parent().unwrap())
        .unwrap()
        .flatten()
    {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
    assert!(
        wait_none(agents, Duration::from_secs(15)).await,
        "the agent {:?} outlived the killed app",
        agents()
    );
    assert!(
        !in_distro("ps -o args | grep '[s]leep 3000'").contains("sleep 3000"),
        "{}",
        in_distro("ps -o pid,args")
    );
}

// sandbox-docker-missing.test.js's form: what is missing is said in the
// unavailable form, for the distro.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_distro_that_cannot_start_makes_every_call_unavailable() {
    let scratch = Scratch::new("wsl-missing");
    let distro = WslDistro::start(mcbench_sandbox::Config {
        root: scratch.sandbox_dir(),
        rootfs: (scratch.0.join("rootfs.tar.zst"), "0".repeat(64)),
        helper: Vec::new(),
        budget: budget(&scratch, MIB),
        guest: GuestBase {
            asset: scratch.0.join("missing-guest.tar.zst"),
            sha256: "0".repeat(64),
        },
    });
    let error = distro.prepare().await.unwrap_err();
    let SandboxError::Unavailable(message) = &error else {
        panic!("{error:?}")
    };
    let installed = std::process::Command::new("wsl.exe")
        .arg("--status")
        .output()
        .unwrap()
        .status
        .success();
    if installed {
        assert!(
            message.starts_with(&format!("{WSL_UNAVAILABLE}: the guest base ")),
            "{message}"
        );
    } else {
        assert_eq!(message, WSL_MISSING);
    }
    let ws = ws(&scratch, "wsl-missing");
    let again = run(&distro, &ws, "true", 1000.0, false).await.unwrap_err();
    assert_eq!(again, error);
    distro.shutdown().await;
}

// The setup screen's facts (`ui-seam-wsl-install.md`): a restart is owed
// while the restart file names this boot of Windows, not after.
#[test]
fn the_restart_file_holds_for_this_boot_only() {
    let scratch = Scratch::new("wsl-restart");
    let root = scratch.sandbox_dir();
    let file = root
        .join(".wsl")
        .join(mcbench_sandbox::windows::RESTART_FILE);
    let boot = mcbench_sandbox::windows::boot_id().unwrap();
    assert_eq!(boot, mcbench_sandbox::windows::boot_id().unwrap());
    mcbench_sandbox::windows::record_restart(&root).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), boot.to_string());
    // This boot's file holds whatever wsl.exe --status says.
    assert_eq!(
        mcbench_sandbox::windows::setup_state(&root).unwrap(),
        WslSetup::Restart
    );
    // A file of another boot is stale: removed, and WSL's own state decides.
    std::fs::write(&file, (boot.wrapping_sub(1)).to_string()).unwrap();
    let ready = mcbench_sandbox::windows::wsl_state() == Ok(WslState::Ready);
    assert_eq!(
        mcbench_sandbox::windows::setup_state(&root).unwrap(),
        if ready {
            WslSetup::Ready
        } else {
            WslSetup::Install
        }
    );
    assert!(!file.exists(), "the stale restart file stayed");
}

/// The python of the rule's check: `socket(AF_VSOCK, type)` for every type
/// the kernel's vsock knows and one it does not, then one `AF_INET` and one
/// `AF_UNIX` socket, each as `<what>: made` or `<what>: errno <n>`.
const SOCKETS: &str = r#"
import socket
for name in ["SOCK_STREAM", "SOCK_DGRAM", "SOCK_SEQPACKET", "SOCK_RAW"]:
    try:
        socket.socket(socket.AF_VSOCK, getattr(socket, name)).close()
        print("AF_VSOCK %s: made" % name)
    except OSError as e:
        print("AF_VSOCK %s: errno %d" % (name, e.errno))
for family, name in [(socket.AF_INET, "AF_INET"), (socket.AF_UNIX, "AF_UNIX")]:
    try:
        socket.socket(family, socket.SOCK_STREAM).close()
        print("%s SOCK_STREAM: made" % name)
    except OSError as e:
        print("%s SOCK_STREAM: errno %d" % (name, e.errno))
"#;

/// What [`SOCKETS`] prints in a box under the keeper's rule
/// (`crates/sandbox/src/linux/refused.rs`): every vsock socket refused with
/// `EPERM` (1), the other families made.
const SOCKETS_UNDER_THE_RULE: &str = "AF_VSOCK SOCK_STREAM: errno 1
AF_VSOCK SOCK_DGRAM: errno 1
AF_VSOCK SOCK_SEQPACKET: errno 1
AF_VSOCK SOCK_RAW: errno 1
AF_INET SOCK_STREAM: made
AF_UNIX SOCK_STREAM: made";

// A1 as a standing test: no box makes a vsock socket of any type, and the
// families a model uses are still made, in both kinds of box.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL and the test distro, as the tests above"]
async fn no_box_makes_a_vsock_socket() {
    let scratch = Scratch::new("wsl-vsock");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "wsl-vsock");
    let probe = format!("python3 -I -c {} 2>&1", quoted(SOCKETS));
    for network in [false, true] {
        let out = run(sandbox, &ws, &probe, 60_000.0, network).await.unwrap();
        assert_eq!(
            // The one-shot networked transcript ends with a newline the
            // persistent one does not have.
            (out.exit_code, output(&out).trim_end()),
            (0, SOCKETS_UNDER_THE_RULE),
            "network {network}: {}",
            out.formatted
        );
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

/// The interop sockets that exist in the test's distro now
/// (`/run/WSL/*_interop`, WSL's per-instance sockets that `/init` uses to
/// start a Windows program), listed as root.
fn interop_sockets() -> Vec<String> {
    in_distro("for f in /run/WSL/*_interop; do [ -S \"$f\" ] && echo \"$f\"; done")
        .lines()
        .map(str::to_string)
        .collect()
}

/// The Windows processes of this image name (`tasklist`'s CSV, one quoted
/// line each; with none it prints an `INFO:` line, which is left out).
fn windows_processes(image: &str) -> Vec<String> {
    let out = std::process::Command::new(
        mcbench_build::sandbox::wsl::system_program("tasklist.exe").unwrap(),
    )
    .args(["/FO", "CSV", "/NH", "/FI", &format!("IMAGENAME eq {image}")])
    .output()
    .unwrap();
    assert!(out.status.success(), "tasklist: {out:?}");
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.starts_with('"'))
        .map(str::to_string)
        .collect()
}

// A2, point 1: an MZ file run from a box starts no Windows program, with no
// WSL_INTEROP and with WSL_INTEROP set to each interop socket the distro has
// at that moment, from a persistent and from a networked box. The file is
// Windows' own cmd.exe copied into /work under a marker name, told to write
// a marker file on the Windows side; what is observed from Windows is that
// the marker file never appears and no process of the marker name is
// running afterwards. The box's answer is today's Docker box's: bash's
// `cannot execute binary file: Exec format error` and 126.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL and the test distro, as the tests above"]
async fn an_mz_file_run_from_a_box_starts_no_windows_program() {
    const IMAGE: &str = "mcbench-mz-marker.exe";
    let scratch = Scratch::new("wsl-mz");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "wsl-mz");
    let cmd = mcbench_build::sandbox::wsl::system_program("cmd.exe").unwrap();
    std::fs::copy(&cmd, ws.host.join(IMAGE)).unwrap();
    // A box starts with the interop sockets of the distro's running
    // instance; the distro runs once the sandbox does.
    run(sandbox, &ws, "true", 10_000.0, false).await.unwrap();
    let sockets = interop_sockets();
    println!("interop sockets in the distro: {sockets:?}");
    let mut tries = vec![None];
    tries.extend(sockets.into_iter().map(Some));
    for network in [false, true] {
        for (n, interop) in tries.iter().enumerate() {
            let marker = scratch.0.join(format!("marker-{network}-{n}.txt"));
            let at = marker.to_str().unwrap().replace('\\', "/");
            let env = interop
                .as_ref()
                .map(|path| format!("WSL_INTEROP={path} "))
                .unwrap_or_default();
            let script =
                format!("chmod +x /work/{IMAGE}; {env}/work/{IMAGE} /c \"echo ran> {at}\"");
            let out = run(sandbox, &ws, &script, 30_000.0, network).await.unwrap();
            println!(
                "== network {network}, WSL_INTEROP {interop:?}: exit {}\n{}",
                out.exit_code, out.formatted
            );
            // Under the box's own binfmt_misc no handler takes the file: the
            // kernel answers ENOEXEC and bash says what it says in today's
            // Docker box (its line number is the session's).
            let said = output(&out).trim_end();
            assert!(
                out.exit_code == 126
                    && said.starts_with("bash: line ")
                    && said.ends_with(&format!(
                        ": /work/{IMAGE}: cannot execute binary file: Exec format error"
                    ))
                    && said.lines().count() == 1,
                "network {network}, WSL_INTEROP {interop:?}: {}",
                out.formatted
            );
            std::thread::sleep(Duration::from_secs(2));
            assert!(
                !marker.exists(),
                "a Windows program ran from a box: {}",
                marker.display()
            );
            assert_eq!(windows_processes(IMAGE), Vec::<String>::new());
        }
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

/// The names directly under `rootfs/usr/lib/` in the root filesystem's tar, sorted:
/// what `ls /usr/lib` prints in a Linux box made of it.
fn rootfs_usr_lib() -> Vec<String> {
    let (tar_zst, _) = rootfs();
    let file = std::fs::File::open(tar_zst).unwrap();
    let decoder = ruzstd::decoding::StreamingDecoder::new(std::io::BufReader::new(file)).unwrap();
    let mut archive = tar::Archive::new(decoder);
    let mut names = std::collections::BTreeSet::new();
    for entry in archive.entries().unwrap() {
        let entry = entry.unwrap();
        let path = entry.path().unwrap().to_string_lossy().into_owned();
        // The box's root is the tar's rootfs/ folder.
        if let Some(rest) = path.strip_prefix("rootfs/usr/lib/")
            && let Some(name) = rest.split('/').next().filter(|name| !name.is_empty())
        {
            names.insert(name.to_string());
        }
    }
    names.into_iter().collect()
}

// A3: a box sees nothing of Windows' files. Its only host file share (9p,
// drvfs, virtiofs) is /work, and /usr/lib holds what the root filesystem's
// does (no /usr/lib/wsl: WSL's GPU libraries and Windows' driver store), in
// both kinds of box.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL and the test distro, as the tests above"]
async fn a_box_sees_no_windows_files_but_its_work_folder() {
    let scratch = Scratch::new("wsl-shares");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "wsl-shares");
    let script = "awk '{ split($0, s, \" - \"); split(s[2], t, \" \"); if (t[1] == \"9p\" || t[1] == \"drvfs\" || t[1] == \"virtiofs\") print \"share \" $5 }' /proc/self/mountinfo; ls -1 /usr/lib";
    let expected = std::iter::once("share /work".to_string())
        .chain(rootfs_usr_lib())
        .collect::<Vec<_>>()
        .join("\n");
    for network in [false, true] {
        let out = run(sandbox, &ws, script, 30_000.0, network).await.unwrap();
        assert_eq!(
            (out.exit_code, output(&out).trim_end()),
            (0, expected.as_str()),
            "network {network}: {}",
            out.formatted
        );
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

/// The MZ file and the three ways it is run in the measurement below, as
/// the guest agent's user in the distro: in a user and mount namespace of
/// its own holding an empty binfmt_misc, in bubblewrap started inside that
/// namespace, and in bubblewrap alone (today's box: the distro's shared
/// WSLInterop entry applies).
const OWN_BINFMT: &str = r#"
printf 'MZ\220\000\003\000\000\000' > /tmp/mcbench-mz.exe; chmod 755 /tmp/mcbench-mz.exe
r=$(ls -d /var/mcbench/.rootfs-*/rootfs | head -n 1)
cat > /tmp/mcbench-box.sh <<EOF
/usr/bin/bwrap --unshare-all --die-with-parent --ro-bind $r / --proc /proc --dev /dev --ro-bind /tmp/mcbench-mz.exe /work/mz.exe -- /bin/bash -c '/work/mz.exe; echo "exit \$?"'
EOF
echo "== a namespace of its own"
/usr/bin/busybox unshare -Urm /usr/bin/busybox sh -c 'mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc; echo "mount $?"; ls -la /proc/sys/fs/binfmt_misc; cat /proc/sys/fs/binfmt_misc/status; /tmp/mcbench-mz.exe; echo "exit $?"' 2>&1
echo "== bubblewrap in it"
/usr/bin/busybox unshare -Urm /usr/bin/busybox sh -c 'mount -t binfmt_misc binfmt_misc /proc/sys/fs/binfmt_misc && /usr/bin/busybox sh /tmp/mcbench-box.sh' 2>&1
echo "== bubblewrap alone"
/usr/bin/busybox sh /tmp/mcbench-box.sh 2>&1
"#;

// The coordinator's point 3, measured: does a user namespace of the box's
// own with an empty binfmt_misc make the MZ file answer what the Linux
// reference answers (bash's "cannot execute binary file: Exec format
// error", 126)? Prints the exact bytes of each way.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "a measurement: needs WSL and the test distro, as the tests above"]
async fn an_mz_file_under_a_binfmt_misc_of_its_own() {
    let scratch = Scratch::new("wsl-binfmt");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "wsl-binfmt");
    // The distro runs and its root filesystem is unpacked once a box ran.
    run(sandbox, &ws, "true", 10_000.0, false).await.unwrap();
    println!("uname -r: {}", in_distro("uname -r").trim());
    println!(
        "{}",
        in_distro_as(mcbench_sandbox::windows::USER, OWN_BINFMT)
    );
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

/// The python that connects to each abstract unix socket `NAMES` lists
/// (comma-separated, without the leading `@`), printing what each connect did.
const ABSTRACT_PROBE: &str = r#"
import errno, os, socket
for name in [n for n in os.environ["NAMES"].split(",") if n]:
    s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    try:
        s.connect("\0" + name)
        print("@%s: CONNECTED" % name)
    except OSError as e:
        print("@%s: %s" % (name, errno.errorcode.get(e.errno, e.errno)))
    finally:
        s.close()
"#;

// A2, point 2: the box has no path to WSL's interop sockets. /run/WSL is not
// in the box's mount namespace, and no abstract unix socket the distro's
// side lists (root's /proc/net/unix) is reachable from either kind of box.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs WSL and the test distro, as the tests above"]
async fn a_box_has_no_path_to_wsl_s_sockets() {
    let scratch = Scratch::new("wsl-sockets");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "wsl-sockets");
    run(sandbox, &ws, "true", 10_000.0, false).await.unwrap();
    let names: Vec<String> =
        in_distro("awk 'NR > 1 && $8 ~ /^@/ { print substr($8, 2) }' /proc/net/unix | sort -u")
            .lines()
            .map(str::to_string)
            .collect();
    println!("abstract sockets in the distro: {names:?}");
    let probe = format!(
        "ls -d /run/WSL 2>&1; NAMES={} python3 -I -c {} 2>&1",
        names.join(","),
        quoted(ABSTRACT_PROBE)
    );
    for network in [false, true] {
        let out = run(sandbox, &ws, &probe, 60_000.0, network).await.unwrap();
        println!("== network {network}\n{}", out.formatted);
        let text = output(&out);
        let mut lines = text.lines();
        assert_eq!(
            lines.next(),
            Some("ls: cannot access '/run/WSL': No such file or directory"),
            "{text}"
        );
        for (name, line) in names.iter().zip(lines) {
            assert!(
                line.starts_with(&format!("@{name}: ")) && !line.ends_with("CONNECTED"),
                "network {network}: {line}"
            );
        }
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

/// `script` quoted for `sh -c '...'`.
fn quoted(script: &str) -> String {
    format!("'{}'", script.replace('\'', "'\\''"))
}
