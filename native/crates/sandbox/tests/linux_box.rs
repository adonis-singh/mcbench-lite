//! The Linux sandbox against real boxes: the ports of
//! `sandbox.integration.test.js` (2), `sandbox-storage.integration.test.js`
//! (1), `sandbox-workspace.integration.test.js` (1), `sandbox-state.test.js`
//! (1) and the user-session part of `sandbox-docker-missing.test.js`, and
//! today's container limits. Needs `MCBENCH_SANDBOX_ROOTFS` and
//! `MCBENCH_SANDBOX_ROOTFS_SHA256` (bubblewrap, user namespaces, a user
//! systemd session); `MCBENCH_SANDBOX_VIA_AGENT=1` runs them through the
//! guest agent over a pipe. Run with `cargo test -p mcbench-sandbox --test
//! linux_box -- --ignored`.
//!
//! On macOS the same tests run against the app's VM (the boxes are the
//! same Linux implementation, inside the guest, reached through the guest
//! agent as `MCBENCH_SANDBOX_VIA_AGENT=1` reaches it on Linux); they need
//! the guest base and the signed test binary as `tests/macos_vm.rs` says.
//! The two tests that look at the Linux host itself (its `/proc`, its
//! systemd) are Linux's; `tests/macos_vm.rs` has the VM's own.

#![cfg(any(target_os = "linux", target_os = "macos", windows))]

mod support;

use std::time::{Duration, Instant};

use support::*;
use tokio_util::sync::CancellationToken;

const NEEDS: &str = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256";
const MIB: u64 = 1024 * 1024;

async fn wait_for(file: &std::path::Path, within: Duration) {
    let deadline = Instant::now() + within;
    while !file.exists() {
        assert!(
            Instant::now() < deadline,
            "{} did not appear",
            file.display()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn a_command_runs_in_each_kind_of_shell() {
    let scratch = Scratch::new("smoke");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "smoke");
    sandbox
        .remove_work_files(&ws, &["nothing".into()])
        .await
        .unwrap();
    let persistent = run(
        sandbox,
        &ws,
        "echo out; echo err >&2; exit_code=3; (exit $exit_code)",
        5000.0,
        false,
    )
    .await
    .unwrap();
    assert_eq!(
        persistent.formatted.split_once("Wall").unwrap().0,
        "Exit code: 3\n"
    );
    assert_eq!(output(&persistent), "out\nerr");
    let networked = run(sandbox, &ws, "echo net", 5000.0, true).await.unwrap();
    assert_eq!(output(&networked), "net\n", "{}", networked.formatted);
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// sandbox.integration.test.js: "sandbox renders SVG and cancellation kills
// foreground and background processes in both shell modes"
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn sandbox_renders_svg_and_cancellation_kills_foreground_and_background_processes_in_both_shell_modes()
 {
    let _ = NEEDS;
    let scratch = Scratch::new("integration");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "integration");
    mcbench_sandbox::work::write_work_file(&ws, "fixture.svg", br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10" fill="red"/></svg>"#).unwrap();
    let svg = run(sandbox, &ws, "rsvg-convert fixture.svg -o native.png && convert fixture.svg converted.png && identify native.png converted.png", 20_000.0, false).await.unwrap();
    assert_eq!(svg.exit_code, 0, "{}", svg.formatted);
    assert!(svg.formatted.contains("PNG 10x10"), "{}", svg.formatted);
    for network in [false, true] {
        let tag = if network { "network" } else { "persistent" };
        let cancel = CancellationToken::new();
        let command = cmd(
            &format!("touch {tag}-started; (sleep 3; touch {tag}-leaked) & wait"),
            15_000.0,
            network,
        );
        let running = sandbox.exec(&ws, &command, &cancel);
        tokio::pin!(running);
        let started = ws.host.join(format!("{tag}-started"));
        tokio::select! {
            result = &mut running => panic!("the command ended first: {result:?}"),
            () = wait_for(&started, Duration::from_secs(15)) => {}
        }
        cancel.cancel();
        assert_eq!(
            running.await.unwrap_err(),
            mcbench_sandbox::SandboxError::Cancelled
        );
        tokio::time::sleep(Duration::from_millis(3200)).await;
        assert!(
            !ws.host.join(format!("{tag}-leaked")).exists(),
            "{tag}: a background job outlived the cancel"
        );
    }
    let next = run(
        sandbox,
        &ws,
        "test -f fixture.svg && printf recovered",
        5000.0,
        false,
    )
    .await
    .unwrap();
    assert_eq!(next.exit_code, 0);
    assert!(next.formatted.contains("recovered"));

    // A timeout ends the shell and its jobs but keeps the box.
    run(
        sandbox,
        &ws,
        "cd /tmp && export MARK=1 && touch /tmp/kept",
        5000.0,
        false,
    )
    .await
    .unwrap();
    let timed_out = run(
        sandbox,
        &ws,
        "(sleep 3; touch /work/timeout-leaked) & sleep 30",
        1000.0,
        false,
    )
    .await
    .unwrap();
    assert_eq!(timed_out.exit_code, 124);
    assert!(
        timed_out.formatted.contains("shell session was reset"),
        "{}",
        timed_out.formatted
    );
    let fresh = run(
        sandbox,
        &ws,
        "pwd; printf \"mark=%s\" \"$MARK\"; test -f /tmp/kept && printf ' tmp-kept'",
        5000.0,
        false,
    )
    .await
    .unwrap();
    assert!(
        fresh.formatted.ends_with("/work\nmark= tmp-kept"),
        "the box (and its /tmp) stayed: {}",
        fresh.formatted
    );
    tokio::time::sleep(Duration::from_millis(3200)).await;
    assert!(!ws.host.join("timeout-leaked").exists());

    // A networked command is one-shot: what it leaves running ends with it.
    let shot = run(
        sandbox,
        &ws,
        "(sleep 2; touch net-bg) & printf launched",
        10_000.0,
        true,
    )
    .await
    .unwrap();
    assert!(shot.formatted.contains("launched"));
    let started = Instant::now();
    let again = run(sandbox, &ws, "printf again", 10_000.0, true)
        .await
        .unwrap();
    assert!(again.formatted.contains("again"));
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "a warm networked call took {:?}",
        started.elapsed()
    );
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert!(!ws.host.join("net-bg").exists());
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// sandbox.integration.test.js: "the networked sandbox cannot reach the
// host, the LAN or a metadata service"
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_networked_sandbox_cannot_reach_the_host_the_lan_or_a_metadata_service() {
    let scratch = Scratch::new("egress");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "egress");
    // A port the system gives: no other run of this test holds it.
    let listener = std::net::TcpListener::bind("0.0.0.0:0").expect("a free port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        use std::io::Write;
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nHOST-REACHED");
        }
    });
    // Today's probe, with the stack's gateway where Docker's is, and the
    // listener's port where today's test has 3146.
    let probe = r#"for url in http://host.docker.internal:3146/ http://$(awk '$2 == "00000000" { printf "%d.%d.%d.%d", "0x" substr($3,7,2), "0x" substr($3,5,2), "0x" substr($3,3,2), "0x" substr($3,1,2) }' /proc/net/route):3146/ http://169.254.169.254/; do
    curl -s -m 3 -o /dev/null -w "%{http_code} " "$url"; done; getent hosts example.com > /dev/null && printf dns
    printf ' ipv6-off:%s addresses:%s' "$(cat /proc/sys/net/ipv6/conf/*/disable_ipv6 | sort -u | tr -d '\n')" "$(wc -l < /proc/net/if_inet6)""#
        .replace(":3146/", &format!(":{port}/"));
    for round in ["fresh", "made again"] {
        let result = run(sandbox, &ws, &probe, 30_000.0, true).await.unwrap();
        assert!(
            !result.formatted.contains("HOST-REACHED") && !result.formatted.contains("200"),
            "{round}: {}",
            result.formatted
        );
        assert!(
            result
                .formatted
                .contains("000 000 000 dns ipv6-off:1 addresses:0"),
            "{round}: {}",
            result.formatted
        );
        // Today the container is stopped and made again; here the boxes are removed.
        sandbox.reset("egress").await;
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// sandbox-storage.integration.test.js: "a chat over its storage budget can
// only delete in /work, and a background writer is stopped"
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn a_chat_over_its_storage_budget_can_only_delete_in_work_and_a_background_writer_is_stopped()
{
    let scratch = Scratch::new("storage");
    let budget = budget(&scratch, 64 * MIB);
    let any = sandbox(budget.clone()).await;
    let sandbox = any.sandbox();
    let id = format!("storage-{}", std::process::id());
    let ws = ws(&scratch, &id);
    let layer = run(
        sandbox,
        &ws,
        "touch /tmp/ok && echo tmp-ok; touch /var/tmp/no 2>&1; touch /no 2>&1; true",
        20_000.0,
        false,
    )
    .await
    .unwrap();
    assert!(layer.formatted.contains("tmp-ok"));
    assert!(
        layer
            .formatted
            .contains("'/var/tmp/no': Read-only file system"),
        "{}",
        layer.formatted
    );
    assert!(layer.formatted.contains("'/no': Read-only file system"));

    // The box itself writes past the budget in one command, before the
    // watchdog's next look (every second). In the Windows VM a box writes
    // /work through drvfs at about 12 MB/s, so an 80 MiB dd takes about
    // 7 s and the watchdog stops it at the budget, as it stops a slow writer
    // anywhere; there the box makes the file at its length at once instead.
    let fill = if cfg!(windows) {
        "truncate -s 80M big.bin && ls -l big.bin | cut -d' ' -f5"
    } else {
        "dd if=/dev/zero of=big.bin bs=1M count=80 2>&1 | tail -1"
    };
    let filled = run(sandbox, &ws, fill, 30_000.0, false).await.unwrap();
    assert_eq!(
        filled.storage, None,
        "the command that passes the budget runs"
    );
    assert_eq!(
        std::fs::metadata(ws.host.join("big.bin")).unwrap().len(),
        80 * MIB
    );
    mcbench_sandbox::Budget::measure(&*budget, &id)
        .await
        .unwrap();

    let refused = run(
        sandbox,
        &ws,
        "echo hello > note.txt; python3 -c \"open('py.txt','w').write('x')\"; ls",
        20_000.0,
        false,
    )
    .await
    .unwrap();
    let storage = refused.storage.clone().unwrap();
    assert!(
        storage.starts_with("This chat's storage is full: ")
            && storage.contains(" hold 80.")
            && storage.contains(" MiB of its 64.0 MiB budget"),
        "{storage}"
    );
    assert!(storage.contains("After this command: This chat's storage is full"));
    assert!(
        refused.formatted.contains("File too large"),
        "{}",
        refused.formatted
    );
    assert_eq!(
        std::fs::metadata(ws.host.join("note.txt")).unwrap().len(),
        0
    );
    assert_eq!(std::fs::metadata(ws.host.join("py.txt")).unwrap().len(), 0);

    let freed = run(sandbox, &ws, "rm big.bin && echo freed", 20_000.0, false)
        .await
        .unwrap();
    assert!(
        freed
            .storage
            .unwrap()
            .contains("After this command the chat is back within its limits")
    );
    assert!(freed.formatted.contains("freed"));
    let normal = run(
        sandbox,
        &ws,
        "echo hello > note.txt && cat note.txt",
        20_000.0,
        false,
    )
    .await
    .unwrap();
    assert_eq!(normal.storage, None);
    assert!(normal.formatted.contains("hello"));

    // About 400 MiB in 10 MiB files, written in the background: the
    // watchdog sees the disk's free space drop and stops it. The disk's
    // reading is the files' size taken off a fixed free space (btrfs, as on
    // adipc, updates statfs only when it commits, seconds later).
    let count = || {
        std::fs::read_dir(&ws.host)
            .unwrap()
            .flatten()
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.starts_with("bg") && n.ends_with(".bin")
            })
            .count()
    };
    let disk = 100 * 1024 * MIB;
    budget.free.store(disk, std::sync::atomic::Ordering::SeqCst);
    any.watch_storage().await;
    run(sandbox, &ws, "(for i in $(seq 1 40); do dd if=/dev/zero of=bg$i.bin bs=1M count=10 2>/dev/null; sleep 0.1; done; touch bg-finished) > /dev/null 2>&1 &", 20_000.0, false).await.unwrap();
    let (mut last, mut still) = (usize::MAX, 0);
    while !ws.host.join("bg-finished").exists() && still < 6 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        budget.free.store(
            disk - count() as u64 * 10 * MIB,
            std::sync::atomic::Ordering::SeqCst,
        );
        any.watch_storage().await;
        let now = count();
        still = if now == last { still + 1 } else { 0 };
        last = now;
    }
    let written = count();
    assert!(
        !ws.host.join("bg-finished").exists(),
        "the writer was stopped"
    );
    assert!(written < 40, "{written} of 40 files written");
    eprintln!("[storage] the watchdog stopped the writer after {written} of 40 files");
    let after = run(sandbox, &ws, "ls | wc -l", 20_000.0, false)
        .await
        .unwrap();
    assert!(
        after
            .storage
            .unwrap()
            .contains("This chat's storage is full")
    );
    run(sandbox, &ws, "rm -f bg*.bin", 20_000.0, false)
        .await
        .unwrap();

    // Reports are deleted inside the box: a link out of /work names nothing there.
    let outside = scratch.0.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("kept.json"), "{}").unwrap();
    std::fs::write(ws.host.join("report.json"), "{}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, ws.host.join("linked")).unwrap();
    // A Windows standard user cannot make a symbolic link; the box makes a
    // Linux one, which in the distro names the folder outside /work.
    #[cfg(windows)]
    run(sandbox, &ws, "ln -s ../../outside linked", 20_000.0, false)
        .await
        .unwrap();
    sandbox
        .remove_work_files(&ws, &["report.json".into(), "linked/kept.json".into()])
        .await
        .unwrap();
    assert!(!ws.host.join("report.json").exists());
    assert!(outside.join("kept.json").exists());
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

// sandbox-workspace.integration.test.js: "same conversation ID in recovered
// roots keeps Bash and build files aligned, and stale mounts are replaced"
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_same_chat_id_in_recovered_roots_keeps_bash_and_build_files_aligned_and_a_stale_box_is_replaced()
 {
    let original = Scratch::new("original");
    let resumed = Scratch::new("resumed");
    let a_any = sandbox(budget(&original, 4 * 1024 * MIB)).await;
    let b_any = sandbox(budget(&resumed, 4 * 1024 * MIB)).await;
    let (a, b) = (a_any.sandbox(), b_any.sandbox());
    let id = format!("recovery-{}", std::process::id());
    let (wa, wb) = (ws(&original, &id), ws(&resumed, &id));
    mcbench_sandbox::work::write_work_file(&wa, "identity.txt", b"original").unwrap();
    mcbench_sandbox::work::write_work_file(&wb, "identity.txt", b"resumed").unwrap();
    assert!(
        run(a, &wa, "cat identity.txt", 5000.0, false)
            .await
            .unwrap()
            .formatted
            .contains("original")
    );
    let second = run(
        b,
        &wb,
        "cat identity.txt; printf model-generated > generated.txt",
        5000.0,
        false,
    )
    .await
    .unwrap();
    assert!(second.formatted.contains("resumed"));
    assert_eq!(
        std::fs::read_to_string(wb.host.join("generated.txt")).unwrap(),
        "model-generated"
    );
    assert!(!wa.host.join("generated.txt").exists());
    // One sandbox asked for the chat with the other root's /work replaces
    // its box (today: a container whose mount is stale).
    let repaired = run(a, &wb, "cat identity.txt; cat generated.txt", 5000.0, false)
        .await
        .unwrap();
    assert_eq!(repaired.exit_code, 0);
    assert!(
        repaired.formatted.contains("resumed") && repaired.formatted.contains("model-generated")
    );
    assert!(
        run(a, &wa, "cat identity.txt", 5000.0, false)
            .await
            .unwrap()
            .formatted
            .contains("original")
    );
    a.destroy(&wa).await.unwrap();
    b.destroy(&wb).await.unwrap();
    a.shutdown().await;
    b.shutdown().await;
}

// sandbox-state.test.js: "containers an earlier run left are adopted, and
// only this /work root's": here no box outlives the sandbox that made it, so
// a new process has none to adopt.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn no_box_outlives_the_sandbox_that_made_it() {
    let scratch = Scratch::new("state");
    let ws = ws(&scratch, "left-behind");
    let pid = {
        let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
        let sandbox = any.sandbox();
        run(sandbox, &ws, "true", 5000.0, true).await.unwrap();
        let marker = 3000 + std::process::id() % 1000;
        let out = run(
            sandbox,
            &ws,
            &format!("sleep {marker} & echo started"),
            5000.0,
            false,
        )
        .await
        .unwrap();
        assert_eq!(out.exit_code, 0);
        // The box's sleep, found from the host by its argument.
        let wanted = format!("sleep\0{marker}\0");
        std::fs::read_dir("/proc")
            .unwrap()
            .flatten()
            .find(|e| std::fs::read(e.path().join("cmdline")).is_ok_and(|c| c == wanted.as_bytes()))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .expect("the box's sleep is a host process")
        // The sandbox is dropped here without shutdown, as when the app ends.
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    while std::path::Path::new(&format!("/proc/{pid}")).exists() {
        assert!(
            Instant::now() < deadline,
            "box process {pid} outlived its sandbox"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

// sandbox-docker-missing.test.js: "with the Docker daemon not running":
// no user systemd session (the probe without XDG_RUNTIME_DIR or a bus).
#[cfg(target_os = "linux")]
#[tokio::test]
#[ignore = "needs a Linux machine with systemd-run"]
async fn without_a_user_systemd_session_the_scope_probe_fails() {
    let path = std::env::var_os("PATH").unwrap();
    let systemd_run =
        mcbench_sandbox::linux::find_program("systemd-run", &path).expect("systemd-run");
    let bwrap = mcbench_sandbox::linux::find_program("bwrap", &path).expect("bwrap");
    assert!(
        mcbench_sandbox::linux::user_scope_works(
            &systemd_run,
            &bwrap,
            &[
                ("PATH", "/usr/bin:/bin"),
                (
                    "XDG_RUNTIME_DIR",
                    &std::env::var("XDG_RUNTIME_DIR").unwrap()
                )
            ]
        )
        .await
    );
    assert!(
        !mcbench_sandbox::linux::user_scope_works(
            &systemd_run,
            &bwrap,
            &[("PATH", "/usr/bin:/bin")]
        )
        .await
    );
}

// Today's container limits: --memory 512m --memory-swap 512m (the OOM
// killer ends one process, the shell lives), --pids-limit 256, --cpus 2,
// --tmpfs /tmp size=256m; and the box's own, the same on every host: 65536
// open files (today's container on adipc), no core dumps (a crash prints no
// "(core dumped)", whatever the machine's core dump handler).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_box_has_todays_container_limits() {
    let scratch = Scratch::new("limits");
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "limits");
    let alloc = run(sandbox, &ws, "python3 -I -c \"b = bytearray(600*1024*1024); b[::4096] = b'x' * len(b[::4096]); print('allocated')\"; echo code=$?", 30_000.0, false).await.unwrap();
    assert!(output(&alloc).ends_with("code=137"), "{}", alloc.formatted);
    // The children are waited for: an orphan's zombie stays with the box's
    // init, `sleep infinity`, which reaps nothing (as today's container's).
    let fork = run(sandbox, &ws, "python3 -I -c \"\nimport os, time\nn = 0\ntry:\n    while n < 400:\n        if os.fork() == 0:\n            time.sleep(3); os._exit(0)\n        n += 1\nexcept OSError as e:\n    print('forked', n, 'then', e.errno)\nelse:\n    print('forked', n)\nwhile True:\n    try: os.wait()\n    except ChildProcessError: break\"", 30_000.0, false).await.unwrap();
    assert!(output(&fork).contains("then 11"), "{}", fork.formatted);
    let forked: u32 = output(&fork)
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    assert!(
        (250..=254).contains(&forked),
        "forked {forked}: 256 tasks with sleep, bash and python"
    );
    let tmp = run(
        sandbox,
        &ws,
        "head -c 300M /dev/zero > /tmp/big; echo rc=$?; du -m /tmp/big | cut -f1; rm /tmp/big",
        30_000.0,
        false,
    )
    .await
    .unwrap();
    assert!(
        output(&tmp).contains("No space left on device") && output(&tmp).contains("rc=1\n256"),
        "{}",
        tmp.formatted
    );
    let cpu = run(sandbox, &ws, "cat /sys/fs/cgroup/cpu.max /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory.swap.max /sys/fs/cgroup/pids.max; cat /proc/self/cgroup", 30_000.0, false).await.unwrap();
    assert_eq!(
        output(&cpu),
        "200000 100000\n536870912\n0\n256\n0::/",
        "{}",
        cpu.formatted
    );
    let own = run(
        sandbox,
        &ws,
        "ulimit -n; ulimit -Hn; ulimit -c; ulimit -Hc; bash -c 'kill -SEGV $$'; echo code=$?",
        5000.0,
        false,
    )
    .await
    .unwrap();
    let text = output(&own);
    assert!(
        text.starts_with("65536\n65536\n0\n0\nbash: line ")
            && text.ends_with(" Segmentation fault      bash -c 'kill -SEGV $$'\ncode=139"),
        "{}",
        own.formatted
    );
    let caps = run(sandbox, &ws, "grep -E '^(CapInh|CapPrm|CapEff|CapBnd|CapAmb|NoNewPrivs)' /proc/self/status; id -u; cat /proc/1/cmdline | tr '\\0' ' '", 5000.0, false).await.unwrap();
    let uid = box_uid();
    assert_eq!(
        output(&caps),
        format!(
            "CapInh:\t0000000000000000\nCapPrm:\t0000000000000000\nCapEff:\t0000000000000000\nCapBnd:\t0000000000000000\nCapAmb:\t0000000000000000\nNoNewPrivs:\t1\n{uid}\nsleep infinity "
        )
    );
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}

/// The host pids whose `cmdline` is `sleep infinity` (every box's init) and
/// that descend from `ancestor`.
#[cfg(target_os = "linux")]
fn box_inits_under(ancestor: u32) -> Vec<u32> {
    let parent = |pid: u32| -> Option<u32> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        stat.rsplit_once(") ")?.1.split(' ').nth(1)?.parse().ok()
    };
    std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|pid| {
            std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == b"sleep\0infinity\0")
        })
        .filter(|pid| {
            let mut at = *pid;
            while let Some(up) = parent(at) {
                if up == ancestor {
                    return true;
                }
                if up <= 1 {
                    return false;
                }
                at = up;
            }
            false
        })
        .collect()
}

// A box ends when the process that owns it is killed (SIGKILL: no drop, no
// shutdown), the persistent box and the networked one, also where an
// AppArmor profile for bwrap clears bubblewrap's parent-death signal: the
// owner here is the agent program (`serve`), as the app owns the boxes.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_boxes_end_when_the_process_that_owns_them_is_killed() {
    use mcbench_sandbox::Sandbox;
    let scratch = Scratch::new("owner-killed");
    let ws = ws(&scratch, "owner-killed");
    let (tarball, sha) = rootfs();
    let mut owner = tokio::process::Command::new(AGENT)
        .arg("serve")
        .arg(shared_root())
        .arg(&tarball)
        .arg(&sha)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let owner_pid = owner.id().unwrap();
    let client = mcbench_sandbox::agent::GuestClient::new(
        owner.stdout.take().unwrap(),
        owner.stdin.take().unwrap(),
        std::sync::Arc::new(|ws: &mcbench_sandbox::Workspace| Ok(ws.host.clone())),
    );
    let sandboxes =
        mcbench_sandbox::lifecycle::Sandboxes::new(client, budget(&scratch, 4 * 1024 * MIB));
    sandboxes.prepare().await.unwrap();
    let persistent = run(&sandboxes, &ws, "echo up", 5000.0, false)
        .await
        .unwrap();
    assert_eq!(persistent.exit_code, 0);
    let networked = run(&sandboxes, &ws, "echo up", 30_000.0, true)
        .await
        .unwrap();
    assert_eq!(networked.exit_code, 0);
    let inits = box_inits_under(owner_pid);
    assert_eq!(
        inits.len(),
        2,
        "the persistent and the networked box's inits: {inits:?}"
    );
    owner.start_kill().unwrap();
    owner.wait().await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Some(left) = inits
        .iter()
        .find(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
    {
        assert!(
            Instant::now() < deadline,
            "box init {left} outlived the process that owned it"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Every link of every process a box can see, one line each: its pid, its
/// name, the link (`fd/<n>`, `cwd`, `root` or `exe`), the device and inode
/// it leads to, those of its target's path as the box sees that path (`-`
/// when the target is no path, `missing` when the box has no such path),
/// and the target (`readlink`). The glob is read before the loop runs, so
/// the listing's own children are not in it. Then every mapped file of
/// those processes (`mapped <pid>/map_files/<range> <the box's
/// device:inode at the path> <path>`: the link can be read, not followed),
/// and what opening one of pid 1's gives (that needs a capability no box
/// has).
const LINKS: &str = r#"for p in /proc/[0-9]*; do c=$(cat "$p/comm" 2>/dev/null) || continue; for l in "$p"/fd/* "$p/cwd" "$p/root" "$p/exe"; do [ -L "$l" ] || continue; t=$(readlink "$l") || continue; a=$(stat -L -c %d:%i "$l" 2>/dev/null || echo -); b=-; case "$t" in /*) b=$(stat -c %d:%i "$t" 2>/dev/null || echo missing);; esac; echo "${p#/proc/} $c ${l#"$p"/} $a $b $t"; done; done; for m in /proc/[0-9]*/map_files/*; do t=$(readlink "$m") || continue; b=$(stat -c %d:%i "$t" 2>/dev/null || echo missing); echo "mapped ${m#/proc/} $b $t"; done; m=$(ls /proc/1/map_files/ | head -n 1); echo "map_files: $(cat "/proc/1/map_files/$m" 2>&1 >/dev/null)""#;

/// The lines of [`LINKS`]' output that lead out of the box: a descriptor
/// other than 0, 1 and 2, a socket or anything else that is neither a pipe
/// nor a path, or a path whose file is not the one the box sees at that
/// path (a host file outside the box's tree, such as the keeper's
/// `box.log`, has a path the box does not have), or a mapped file at a path
/// the box does not have.
fn links_out_of_the_box(listing: &str) -> Vec<String> {
    listing
        .lines()
        .filter(|line| !line.starts_with("map_files: "))
        .filter(|line| {
            if let Some(mapped) = line.strip_prefix("mapped ") {
                let seen = mapped.split(' ').nth(1).unwrap_or("missing");
                return seen == "missing";
            }
            let words: Vec<&str> = line.splitn(6, ' ').collect();
            let [_pid, _name, link, at, seen, target] = words[..] else {
                return true;
            };
            let descriptor = link.starts_with("fd/");
            if descriptor && !matches!(link, "fd/0" | "fd/1" | "fd/2") {
                return true;
            }
            if target.starts_with("pipe:[") {
                return !descriptor;
            }
            !(target.starts_with('/') && at != "-" && at == seen)
        })
        .map(str::to_string)
        .collect()
}

// Nothing in a box leads out of it through /proc: in the persistent shell,
// the networked one-shot shell and the write-disabled shell of a chat over
// its storage budget, every process the box can see, its init included,
// holds only stdin, stdout and stderr, each a pipe or a file of the box's
// own tree, its cwd, root, program and mapped files are the box's own, and
// a mapped file cannot be opened through /proc. Writing through the init's stdout and stderr
// reaches no host file. (The build child's box is held by the escape
// probe in mcbench-build's child tests.)
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn nothing_in_a_box_leads_out_of_it_through_proc() {
    let scratch = Scratch::new("links");
    let budget = budget(&scratch, 64 * MIB);
    let any = sandbox(budget.clone()).await;
    let sandbox = any.sandbox();
    let id = format!("links-{}", std::process::id());
    let ws = ws(&scratch, &id);
    let mut shells = Vec::new();
    for (kind, network) in [("persistent", false), ("networked", true)] {
        let out = run(sandbox, &ws, LINKS, 30_000.0, network).await.unwrap();
        assert_eq!(out.exit_code, 0, "{kind}: {}", out.formatted);
        shells.push((kind, output(&out).to_string()));
    }
    // The init's stdout and stderr written to from a box command. On Linux
    // the boxes' run folders are this machine's: every file in them keeps
    // its size (other tests' boxes may come and go meanwhile: a file that
    // is gone afterwards, or new, is not compared).
    #[cfg(target_os = "linux")]
    let sizes = || {
        let mut sizes = Vec::new();
        for run in std::fs::read_dir(shared_root().join(".run"))
            .unwrap()
            .flatten()
        {
            for file in std::fs::read_dir(run.path()).unwrap().flatten() {
                let meta = file.metadata().unwrap();
                if meta.is_file() {
                    sizes.push((file.path(), meta.len()));
                }
            }
        }
        sizes.sort();
        sizes
    };
    #[cfg(target_os = "linux")]
    let before = sizes();
    let wrote = run(
        sandbox,
        &ws,
        "printf 'from the box\\n' >> /proc/1/fd/1; printf 'from the box\\n' >> /proc/1/fd/2; echo $?",
        30_000.0,
        false,
    )
    .await
    .unwrap();
    assert_eq!(output(&wrote).trim_end(), "0", "{}", wrote.formatted);
    #[cfg(target_os = "linux")]
    {
        let after = sizes();
        assert!(
            after.iter().any(|(path, _)| path.ends_with("box.log")),
            "the boxes' run folders hold their box.log: {after:?}"
        );
        let grown: Vec<_> = before
            .iter()
            .filter(|(path, size)| after.iter().any(|(p, s)| p == path && s != size))
            .collect();
        assert!(
            grown.is_empty(),
            "a box command's writes to /proc/1/fd/1 and /proc/1/fd/2 reached these host files: {grown:?}"
        );
    }
    // Over the budget, as in the storage test above.
    let fill = if cfg!(windows) {
        "truncate -s 80M big.bin"
    } else {
        "dd if=/dev/zero of=big.bin bs=1M count=80 2>/dev/null"
    };
    run(sandbox, &ws, fill, 30_000.0, false).await.unwrap();
    mcbench_sandbox::Budget::measure(&*budget, &id)
        .await
        .unwrap();
    let out = run(sandbox, &ws, LINKS, 30_000.0, false).await.unwrap();
    assert!(
        out.storage.is_some(),
        "the chat is over its budget: {}",
        out.formatted
    );
    shells.push(("write-disabled", output(&out).to_string()));
    for (kind, listing) in &shells {
        println!("== {kind}\n{listing}");
        assert!(
            listing
                .lines()
                .any(|line| line.starts_with("1 sleep fd/0 ")),
            "{kind}: the listing shows the box's init: {listing}"
        );
        let out_of_the_box = links_out_of_the_box(listing);
        assert!(
            out_of_the_box.is_empty(),
            "{kind}: these lead out of the box (pid, name, link, its device:inode, the box's at that path, target): {out_of_the_box:?}\n{listing}"
        );
        let map_files = listing
            .lines()
            .find_map(|line| line.strip_prefix("map_files: "))
            .unwrap_or_default();
        assert!(
            listing
                .lines()
                .any(|line| line.starts_with("mapped 1/map_files/")),
            "{kind}: the listing shows pid 1's mapped files: {listing}"
        );
        assert!(
            map_files.ends_with("Operation not permitted"),
            "{kind}: opening one of pid 1's mapped files: {map_files}"
        );
    }
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
}
