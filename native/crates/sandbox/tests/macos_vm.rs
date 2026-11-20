//! The macOS VM itself: it boots from the guest base, its agent answers,
//! it ends with its sandbox and with its process, and a VM that cannot
//! start makes the sandbox unavailable in the unavailable form.
//!
//! Needs `MCBENCH_SANDBOX_GUEST` and `MCBENCH_SANDBOX_GUEST_SHA256` (the
//! guest base, `guest/build.sh`) beside `MCBENCH_SANDBOX_ROOTFS` and
//! `MCBENCH_SANDBOX_ROOTFS_SHA256` (the aarch64 root filesystem), and the
//! test binary signed with the virtualization entitlement:
//!
//! ```text
//! CARGO_TARGET_AARCH64_APPLE_DARWIN_RUNNER=crates/sandbox/macos/run-signed.sh \
//!   cargo test -p mcbench-sandbox --test macos_vm --test linux_box -- --ignored --test-threads 1
//! ```

#![cfg(target_os = "macos")]

mod support;

use std::time::{Duration, Instant};

use mcbench_sandbox::macos::{VM_UNAVAILABLE, VirtualMachine};
use mcbench_sandbox::{GuestBase, Sandbox, SandboxError};
use support::*;

const MIB: u64 = 1024 * 1024;

/// The pids of the framework's VM service processes now (only read: the
/// VM's process is not a child of ours, and other VMs of the machine's own,
/// Docker Desktop's among them, run in processes of the same name).
fn vm_processes() -> Vec<u32> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-axo", "pid=,comm="])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|line| line.ends_with("com.apple.Virtualization.VirtualMachine"))
        .filter_map(|line| line.split_whitespace().next()?.parse().ok())
        .collect()
}

fn alive(pid: u32) -> bool {
    // SAFETY: kill with signal 0 only asks whether the pid exists.
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

async fn wait_gone(pids: &[u32], within: Duration) -> bool {
    let deadline = Instant::now() + within;
    while pids.iter().any(|pid| alive(*pid)) {
        if Instant::now() > deadline {
            return false;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    true
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_GUEST, MCBENCH_SANDBOX_ROOTFS (and their sha256) and the signed test binary"]
async fn the_vm_boots_runs_a_command_and_stops_with_its_sandbox() {
    let before = vm_processes();
    let scratch = Scratch::new("vm");
    let started = Instant::now();
    let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
    eprintln!("boot and prepare: {:?}", started.elapsed());
    let ours: Vec<u32> = vm_processes()
        .into_iter()
        .filter(|pid| !before.contains(pid))
        .collect();
    assert_eq!(ours.len(), 1, "one VM process of ours: {ours:?}");
    let sandbox = any.sandbox();
    let ws = ws(&scratch, "vm");
    let run = run(
        sandbox,
        &ws,
        "uname -m; id; echo hi > /work/hi.txt; ls -ln /work; cat /proc/self/cgroup /sys/fs/cgroup/memory.max; date +%Y",
        10_000.0,
        false,
    )
    .await
    .unwrap();
    eprintln!("{}", run.formatted);
    assert_eq!(run.exit_code, 0, "{}", run.formatted);
    assert_eq!(
        std::fs::read_to_string(ws.host.join("hi.txt")).unwrap(),
        "hi\n"
    );
    sandbox.destroy(&ws).await.unwrap();
    sandbox.shutdown().await;
    assert!(
        wait_gone(&ours, Duration::from_secs(10)).await,
        "the VM process {ours:?} outlived the sandbox's shutdown"
    );
}

// sandbox-state.test.js's case on macOS (linux_box.rs has Linux's): no box
// outlives the sandbox that made it, here because its VM does not.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_GUEST, MCBENCH_SANDBOX_ROOTFS (and their sha256) and the signed test binary"]
async fn no_box_outlives_the_sandbox_that_made_it() {
    let before = vm_processes();
    let scratch = Scratch::new("vm-state");
    let ws = ws(&scratch, "left-behind");
    let ours = {
        let any = sandbox(budget(&scratch, 4 * 1024 * MIB)).await;
        let sandbox = any.sandbox();
        run(sandbox, &ws, "true", 5000.0, true).await.unwrap();
        let out = run(sandbox, &ws, "sleep 3000 & echo started", 5000.0, false)
            .await
            .unwrap();
        assert_eq!(out.exit_code, 0);
        let ours: Vec<u32> = vm_processes()
            .into_iter()
            .filter(|pid| !before.contains(pid))
            .collect();
        assert_eq!(ours.len(), 1, "one VM process of ours: {ours:?}");
        ours
        // The sandbox is dropped here without shutdown, as when the app ends.
    };
    assert!(
        wait_gone(&ours, Duration::from_secs(10)).await,
        "the VM process {ours:?} outlived its sandbox"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_GUEST, MCBENCH_SANDBOX_ROOTFS (and their sha256) and the signed test binary"]
async fn the_vm_ends_when_the_process_that_started_it_is_killed() {
    if let Ok(marker) = std::env::var("MCBENCH_SANDBOX_VM_CHILD") {
        // The child: start a VM, say so, and wait to be killed.
        let any = sandbox(budget(&Scratch::new("vm-child"), 4 * 1024 * MIB)).await;
        std::fs::write(&marker, "up").unwrap();
        let _keep = any;
        tokio::time::sleep(Duration::from_secs(120)).await;
        panic!("the child was not killed");
    }
    let scratch = Scratch::new("vm-kill");
    let marker = scratch.0.join("up");
    let before = vm_processes();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "the_vm_ends_when_the_process_that_started_it_is_killed",
            "--ignored",
            "--nocapture",
        ])
        .env("MCBENCH_SANDBOX_VM_CHILD", &marker)
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(60);
    while !marker.exists() {
        assert!(Instant::now() < deadline, "the child's VM did not come up");
        assert!(child.try_wait().unwrap().is_none(), "the child ended early");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let ours: Vec<u32> = vm_processes()
        .into_iter()
        .filter(|pid| !before.contains(pid))
        .collect();
    assert_eq!(ours.len(), 1, "one VM process of the child's: {ours:?}");
    // SAFETY: SIGKILL to our own child.
    unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
    child.wait().unwrap();
    // The killed child could not remove its scratch folder.
    let prefix = format!("mcbench-box-vm-child-{}-", child.id());
    for entry in std::fs::read_dir(scratch.0.parent().unwrap())
        .unwrap()
        .flatten()
    {
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            std::fs::remove_dir_all(entry.path()).unwrap();
        }
    }
    assert!(
        wait_gone(&ours, Duration::from_secs(10)).await,
        "the VM process {ours:?} outlived the killed app"
    );
}

// sandbox-docker-missing.test.js's form: what is missing is said in the
// unavailable form, for the VM.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_vm_that_cannot_start_makes_every_call_unavailable() {
    let scratch = Scratch::new("vm-missing");
    let vm = VirtualMachine::start(mcbench_sandbox::Config {
        root: scratch.sandbox_dir(),
        rootfs: (scratch.0.join("rootfs.tar.zst"), "0".repeat(64)),
        helper: Vec::new(),
        budget: budget(&scratch, MIB),
        guest: GuestBase {
            asset: scratch.0.join("missing-guest.tar.zst"),
            sha256: "0".repeat(64),
        },
    });
    let error = vm.prepare().await.unwrap_err();
    let SandboxError::Unavailable(message) = &error else {
        panic!("{error:?}")
    };
    assert!(
        message.starts_with(&format!("{VM_UNAVAILABLE}: the guest base ")),
        "{message}"
    );
    let ws = ws(&scratch, "vm-missing");
    let again = run(&vm, &ws, "true", 1000.0, false).await.unwrap_err();
    assert_eq!(again, error);
    vm.shutdown().await;
}
