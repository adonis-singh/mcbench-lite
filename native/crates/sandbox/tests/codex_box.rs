//! Codex's box on Linux (the container part of
//! the benchmark's batch isolation integration test): a host file
//! outside its mounts cannot be read, a direct connection fails, and the
//! relay on 127.0.0.1:3128 reaches the egress proxy, which refuses a host
//! not on its list and logs it. Needs `MCBENCH_SANDBOX_AGENT_STATIC=<the
//! static mcbench-sandbox-agent>` beside the rootfs variables. Run with
//! `cargo test -p mcbench-sandbox --test codex_box -- --ignored`.

#![cfg(target_os = "linux")]

mod support;

use mcbench_sandbox::proxy::{CodexRun, codex_command};
use support::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_AGENT_STATIC, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn codex_runs_with_the_allow_list_proxy_as_its_only_network() {
    let agent = std::path::PathBuf::from(
        std::env::var_os("MCBENCH_SANDBOX_AGENT_STATIC")
            .expect("MCBENCH_SANDBOX_AGENT_STATIC=<the static mcbench-sandbox-agent> is needed"),
    );
    let (tarball, sha) = rootfs();
    let unpacked = mcbench_sandbox::linux::rootfs::unpack(&tarball, &sha, &shared_root()).unwrap();
    let scratch = Scratch::new("codex");
    let dir = &scratch.0;
    let canary = dir.join("canary.txt");
    std::fs::write(&canary, "reference build secret").unwrap();
    for folder in ["run/sock", "home", "workspace", "install/bin"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    std::fs::write(dir.join("auth.json"), "{}").unwrap();
    let log = dir.join("egress.ndjson");
    let probe = [
        "import socket".to_string(),
        format!(
            "try: open({:?}).read(); print('read')",
            canary.to_string_lossy()
        ),
        "except OSError: print('no-file')".into(),
        "try: socket.create_connection(('1.1.1.1', 443), 5); print('direct')".into(),
        "except OSError: print('no-direct')".into(),
        "s = socket.create_connection(('127.0.0.1', 3128), 5)".into(),
        "s.sendall(b'CONNECT example.com:443 HTTP/1.1\\r\\nHost: example.com:443\\r\\n\\r\\n')"
            .into(),
        "print(s.recv(100).decode().splitlines()[0])".into(),
    ]
    .join("\n");
    let command = ["python3".to_string(), "-c".into(), probe];
    let run = codex_command(&CodexRun {
        rootfs: unpacked.rootfs.clone(),
        image_env: unpacked.image.env.clone(),
        relay: agent,
        install: dir.join("install"),
        home: dir.join("home"),
        auth_file: dir.join("auth.json"),
        workspace: dir.join("workspace"),
        sock_dir: dir.join("run/sock"),
        allowed_hosts: vec!["chatgpt.com".into(), "auth.openai.com".into()],
        proxy_log: log.clone(),
        command: command.to_vec(),
    })
    .unwrap();
    let out = tokio::process::Command::new(&run.command)
        .args(&run.args)
        .env_clear()
        .envs(run.env.iter().cloned())
        .output()
        .await
        .unwrap();
    (run.close_proxy)().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .lines()
            .collect::<Vec<_>>(),
        ["no-file", "no-direct", "HTTP/1.1 403 Forbidden"]
    );
    let targets: Vec<String> = std::fs::read_to_string(&log)
        .unwrap()
        .lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["target"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(targets, ["example.com:443"]);
}

/// A box of `codex_command` over a scratch folder, running `command`; its
/// install folder is `<scratch>/install`.
struct CodexBox {
    scratch: Scratch,
    run: mcbench_sandbox::proxy::CodexCommand,
}

fn codex_box(
    label: &str,
    command: &[&str],
    install: impl FnOnce(&std::path::Path, &std::path::Path),
) -> CodexBox {
    let agent = std::path::PathBuf::from(
        std::env::var_os("MCBENCH_SANDBOX_AGENT_STATIC")
            .expect("MCBENCH_SANDBOX_AGENT_STATIC=<the static mcbench-sandbox-agent> is needed"),
    );
    let (tarball, sha) = rootfs();
    let unpacked = mcbench_sandbox::linux::rootfs::unpack(&tarball, &sha, &shared_root()).unwrap();
    let scratch = Scratch::new(label);
    let dir = scratch.0.clone();
    for folder in ["run/sock", "home", "workspace", "install/bin"] {
        std::fs::create_dir_all(dir.join(folder)).unwrap();
    }
    std::fs::write(dir.join("auth.json"), "{}").unwrap();
    install(&unpacked.rootfs, &dir.join("install"));
    let run = codex_command(&CodexRun {
        rootfs: unpacked.rootfs.clone(),
        image_env: unpacked.image.env.clone(),
        relay: agent,
        install: dir.join("install"),
        home: dir.join("home"),
        auth_file: dir.join("auth.json"),
        workspace: dir.join("workspace"),
        sock_dir: dir.join("run/sock"),
        allowed_hosts: vec!["chatgpt.com".into()],
        proxy_log: dir.join("egress.ndjson"),
        command: command.iter().map(|s| s.to_string()).collect(),
    })
    .unwrap();
    CodexBox { scratch, run }
}

impl CodexBox {
    fn command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.run.command);
        command
            .args(&self.run.args)
            .env_clear()
            .envs(self.run.env.iter().cloned())
            .kill_on_drop(true);
        command
    }

    fn close(self) {
        (self.run.close_proxy)().unwrap();
        drop(self.scratch);
    }
}

/// The uid a box shows for root, which its user namespace does not map:
/// the owner of a process's /proc files when it is not dumpable.
fn overflow_uid() -> u32 {
    std::fs::read_to_string("/proc/sys/kernel/overflowuid")
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

/// What a box command sees of another process (`<pid>`): the uid that owns
/// its `/proc/<pid>/fd` (the overflow uid when the process is not dumpable)
/// and what opening its stdout through /proc gives.
const REACH: &str = r#"
import errno, os
def reach(pid):
    try:
        owner = str(os.stat('/proc/%d/fd' % pid).st_uid)
    except OSError as e:
        owner = errno.errorcode[e.errno]
    try:
        os.close(os.open('/proc/%d/fd/1' % pid, os.O_WRONLY | os.O_APPEND | os.O_NONBLOCK))
        opened = 'ok'
    except OSError as e:
        opened = errno.errorcode[e.errno]
    return 'owner %s open %s' % (owner, opened)
"#;

// The relay is the box's pid 1 (`--as-pid-1`, no init of bubblewrap's): not
// dumpable, so a command in the box cannot open its descriptors (the app's
// pipes); it reaps a process orphaned in the box; and it exits with its
// command's code as soon as the command has exited, though an orphan still
// runs.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_AGENT_STATIC, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_relay_is_pid_1_not_dumpable_reaps_orphans_and_exits_with_its_commands_code() {
    let probe = format!(
        "{REACH}{}",
        r#"
import subprocess, sys, time
print('pid1 %s ppid %d uid %d' % (open('/proc/1/comm').read().strip(), os.getppid(), os.getuid()))
print('pid1 ' + reach(1))
# An orphan: the shell's child, left to pid 1 when the shell exits at once.
orphan = int(subprocess.run(['sh', '-c', 'sh -c "exit 0" & echo $!'], capture_output=True, text=True).stdout)
started = time.monotonic()
state = 'gone'
while time.monotonic() - started < 30:
    try:
        state = open('/proc/%d/stat' % orphan).read().rsplit(')', 1)[1].split()[0]
    except OSError:
        state = 'gone'
        break
    time.sleep(0.01)
print('orphan ' + state)
# One left running: the box ends with the command all the same.
subprocess.Popen(['sleep', '600'], start_new_session=True)
sys.exit(3)
"#
    );
    let codex = codex_box("relay-pid1", &["python3", "-c", &probe], |_, _| {});
    let started = std::time::Instant::now();
    let out = codex.command().output().await.unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    println!("{stdout}{}", String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines[0].starts_with("pid1 mcbench-relay ppid 1 "),
        "the relay is pid 1 and the command's parent: {stdout}"
    );
    assert_eq!(
        lines[1],
        format!("pid1 owner {} open EACCES", overflow_uid()),
        "the relay is not dumpable"
    );
    assert_eq!(lines[2], "orphan gone", "the relay reaped the orphan");
    assert_eq!(out.status.code(), Some(3), "the command's code");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(120),
        "the box ended with its command, not with the orphan"
    );
    codex.close();
}

// The relay's exit status for a command a signal ended (`256 - signal`, as
// container-shim.py's), and SIGTERM passed on to its command.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_AGENT_STATIC, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn the_relay_passes_sigterm_on_and_exits_256_less_the_signal_that_ended_its_command() {
    let killed = codex_box(
        "relay-signal",
        &[
            "python3",
            "-c",
            "import os, signal; os.kill(os.getpid(), signal.SIGTERM)",
        ],
        |_, _| {},
    );
    let out = killed.command().output().await.unwrap();
    assert_eq!(
        out.status.code(),
        Some(256 - libc::SIGTERM),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    killed.close();
    let waiting = codex_box(
        "relay-sigterm",
        &[
            "python3",
            "-c",
            "import signal, sys\nsignal.signal(signal.SIGTERM, lambda *_: sys.exit(7))\nprint('ready', flush=True)\nwhile True: signal.pause()",
        ],
        |_, _| {},
    );
    let mut child = waiting
        .command()
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    tokio::io::AsyncBufReadExt::read_line(&mut stdout, &mut line)
        .await
        .unwrap();
    assert_eq!(line, "ready\n");
    // The relay, as this machine sees it: bubblewrap's child (the command
    // started is bubblewrap itself, systemd-run --scope having exec'd it).
    let bwrap = child.id().unwrap();
    let relay: i32 = std::fs::read_to_string(format!("/proc/{bwrap}/task/{bwrap}/children"))
        .unwrap()
        .split_whitespace()
        .next()
        .expect("bubblewrap's child, the relay")
        .parse()
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(format!("/proc/{relay}/comm"))
            .unwrap()
            .trim(),
        "mcbench-relay"
    );
    // SAFETY: kill of the relay's pid, read above from its parent, which
    // this test started and has not reaped.
    assert_eq!(unsafe { libc::kill(relay, libc::SIGTERM) }, 0);
    let status = tokio::time::timeout(std::time::Duration::from_secs(60), child.wait())
        .await
        .expect("the box ends after SIGTERM")
        .unwrap();
    assert_eq!(status.code(), Some(7), "the command's own exit on SIGTERM");
    waiting.close();
}

// The compiled stand-in in the box: a program installed execute-only (0111,
// as `execute_only` leaves Codex's) runs, and a command in the box cannot
// open its `/proc/<pid>/fd`; the same program readable can be. The
// stand-in is the box's own `sleep` (a program of the root filesystem, so it
// runs there).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs MCBENCH_SANDBOX_AGENT_STATIC, MCBENCH_SANDBOX_ROOTFS and MCBENCH_SANDBOX_ROOTFS_SHA256"]
async fn a_program_installed_execute_only_runs_in_the_box_and_is_not_dumpable() {
    let probe = format!(
        "{REACH}{}",
        r#"
import subprocess
for path in ['/opt/codex/bin/standin', '/opt/codex/plain/readable']:
    child = subprocess.Popen([path, '600'], stdout=subprocess.PIPE)
    print('%s %s' % (path, reach(child.pid)))
    child.kill()
    child.wait()
"#
    );
    let codex = codex_box(
        "execute-only",
        &["python3", "-c", &probe],
        |rootfs, install| {
            use std::os::unix::fs::PermissionsExt;
            let sleep = std::fs::read(rootfs.join("usr/bin/sleep")).unwrap();
            std::fs::create_dir_all(install.join("plain")).unwrap();
            for file in ["bin/standin", "plain/readable"] {
                std::fs::write(install.join(file), &sleep).unwrap();
                std::fs::set_permissions(
                    install.join(file),
                    std::fs::Permissions::from_mode(0o755),
                )
                .unwrap();
            }
            mcbench_sandbox::proxy::execute_only(&install.join("bin")).unwrap();
        },
    );
    let out = codex.command().output().await.unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    println!("{stdout}{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success());
    assert_eq!(
        stdout.lines().collect::<Vec<_>>(),
        [
            format!(
                "/opt/codex/bin/standin owner {} open EACCES",
                overflow_uid()
            ),
            // SAFETY: getuid cannot fail.
            format!("/opt/codex/plain/readable owner {} open ok", unsafe {
                libc::getuid()
            }),
        ]
    );
    codex.close();
}
