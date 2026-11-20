//! The macOS VM's numbers for the speed table: VM start to the agent's
//! first answer (`prepare`, which runs the bubblewrap probe in the guest),
//! a chat's first `bash` call (its box made, its shell started), a warm
//! call, and the resident memory of the framework's VM process idle and
//! while a box imports NumPy.
//!
//! ```text
//! vm_bench <sandbox folder> <guest.tar.zst> <sha256> <rootfs.tar.zst> <sha256> <rounds>
//! ```
//!
//! The binary must be signed with the virtualization entitlement
//! (`macos/run-signed.sh`). Each round boots a VM, measures, and shuts it
//! down; one line per measurement on stdout.

#[cfg(target_os = "macos")]
#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use mcbench_sandbox::macos::VirtualMachine;
    use mcbench_sandbox::testing::TestBudget;
    use mcbench_sandbox::{BashCommand, GuestBase, Sandbox, Workspace};
    use tokio_util::sync::CancellationToken;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [root, guest, guest_sha, rootfs, rootfs_sha, rounds] = args.as_slice() else {
        eprintln!(
            "usage: vm_bench <sandbox folder> <guest.tar.zst> <sha256> <rootfs.tar.zst> <sha256> <rounds>"
        );
        std::process::exit(2)
    };
    let vm_processes = || -> Vec<u32> {
        let out = std::process::Command::new("/bin/ps")
            .args(["-axo", "pid=,comm="])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.ends_with("com.apple.Virtualization.VirtualMachine"))
            .filter_map(|l| l.split_whitespace().next()?.parse().ok())
            .collect()
    };
    let rss_kib = |pid: u32| -> u64 {
        let out = std::process::Command::new("/bin/ps")
            .args(["-o", "rss=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    };
    let root = std::path::PathBuf::from(root);
    for round in 0..rounds.parse::<usize>().unwrap() {
        let before = vm_processes();
        let budget = Arc::new(TestBudget::new(root.clone(), 4 << 30));
        let started = Instant::now();
        let vm = VirtualMachine::start(mcbench_sandbox::Config {
            root: root.clone(),
            rootfs: (rootfs.into(), rootfs_sha.clone()),
            helper: Vec::new(),
            budget,
            guest: GuestBase {
                asset: guest.into(),
                sha256: guest_sha.clone(),
            },
        });
        vm.prepare().await.expect("prepare");
        let first_answer = started.elapsed();
        let pid = vm_processes()
            .into_iter()
            .find(|p| !before.contains(p))
            .expect("the VM's process");
        let ws = Workspace {
            chat: format!("bench-{}-{round}", std::process::id()),
            host: root.join(format!("bench-{}-{round}", std::process::id())),
        };
        mcbench_sandbox::work::work_dir(&ws).unwrap();
        let call = |command: &str| BashCommand {
            command: command.to_string(),
            timeout_ms: Some(30_000.0),
            network: false,
        };
        let at = Instant::now();
        let run = vm
            .exec(&ws, &call("true"), &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(run.exit_code, 0);
        let first_call = at.elapsed();
        let mut warm = Vec::new();
        for _ in 0..5 {
            let at = Instant::now();
            vm.exec(&ws, &call("true"), &CancellationToken::new())
                .await
                .unwrap();
            warm.push(at.elapsed());
        }
        warm.sort();
        tokio::time::sleep(Duration::from_secs(2)).await;
        let idle = rss_kib(pid);
        let peak = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let sampling = {
            let peak = peak.clone();
            let stop = CancellationToken::new();
            let token = stop.clone();
            let task = tokio::spawn(async move {
                while !token.is_cancelled() {
                    peak.fetch_max(rss_kib(pid), std::sync::atomic::Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            });
            (stop, task)
        };
        let numpy = vm
            .exec(
                &ws,
                &call("python3 -c 'import numpy, time; numpy.zeros(10); time.sleep(2)'"),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(numpy.exit_code, 0, "{}", numpy.formatted);
        sampling.0.cancel();
        sampling.1.await.unwrap();
        println!(
            "round {round} first_answer_ms {} first_call_ms {} warm_median_ms {} warm_min_ms {} warm_max_ms {} rss_idle_mib {} rss_numpy_peak_mib {}",
            first_answer.as_millis(),
            first_call.as_millis(),
            warm[2].as_secs_f64() * 1000.0,
            warm[0].as_secs_f64() * 1000.0,
            warm[4].as_secs_f64() * 1000.0,
            idle / 1024,
            peak.load(std::sync::atomic::Ordering::SeqCst) / 1024
        );
        vm.destroy(&ws).await.unwrap();
        vm.shutdown().await;
        drop(vm);
        let deadline = Instant::now() + Duration::from_secs(10);
        // SAFETY: kill with signal 0 only asks whether the pid exists.
        while unsafe { libc::kill(pid as i32, 0) } == 0 && Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("vm_bench measures the macOS VM");
    std::process::exit(2)
}
