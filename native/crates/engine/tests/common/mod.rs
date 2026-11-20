//! What the engine's tests share: the tables (`MCBENCH_TABLES`), the
//! process's in-process engine, the children's program
//! (`MCBENCH_ENGINE_PROGRAM`), fake children (shell scripts on Unix,
//! PowerShell scripts on Windows), and Node (`MCBENCH_DIFF_NODE`) for
//! fixtures made by today's server modules.

#![allow(dead_code)]

use mcbench_engine::{Engine, EngineProgram};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The folder of Minecraft's tables the tests read.
pub fn tables() -> PathBuf {
    PathBuf::from(
        std::env::var("MCBENCH_TABLES").expect(
            "MCBENCH_TABLES names the folder of Minecraft's tables (core/minecraft/1.21.10)",
        ),
    )
}

/// The process's in-process engine, started once.
pub fn engine() -> Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    *ENGINE.get_or_init(|| Engine::start(&tables()).unwrap())
}

/// The children's program: `MCBENCH_ENGINE_PROGRAM` names a built
/// `mcbench-lite`, given the tables of `MCBENCH_TABLES`.
pub fn program() -> EngineProgram {
    let binary = PathBuf::from(std::env::var("MCBENCH_ENGINE_PROGRAM").expect(
        "MCBENCH_ENGINE_PROGRAM names a built mcbench-lite, the program engine children run",
    ));
    assert!(
        binary.is_file(),
        "MCBENCH_ENGINE_PROGRAM names {}, which is not a file",
        binary.display()
    );
    EngineProgram::new(binary, tables())
}

/// A fake child: a shell script of `body` (it is given `headless engine
/// <tables>` and runs in the child's folder), in `folder`. The script is
/// written by a shell of its own: this process never holds it open for
/// writing, so another test's fork (which keeps this process's descriptors
/// until it execs) can never leave it busy (`ETXTBSY`) when it is started.
#[cfg(unix)]
pub fn fake(folder: &Path, name: &str, body: &str) -> EngineProgram {
    use std::io::Write as _;
    let script = folder.join(name);
    let mut shell = std::process::Command::new("/bin/sh")
        .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
        .arg(&script)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    shell
        .stdin
        .take()
        .unwrap()
        .write_all(format!("#!/bin/sh\n{body}").as_bytes())
        .unwrap();
    assert!(
        shell.wait().unwrap().success(),
        "{} is written",
        script.display()
    );
    EngineProgram::new(script, folder)
}

/// The fake's body for this platform: `sh` on Unix, `powershell` on
/// Windows.
pub fn script<'a>(sh: &'a str, powershell: &'a str) -> &'a str {
    if cfg!(windows) { powershell } else { sh }
}

/// A fake child on Windows: a PowerShell script of `body` and the `.cmd`
/// that starts it with its arguments (`name` with `.cmd` and `.ps1` for its
/// `.sh`), in `folder`. Its tree is cmd.exe and powershell.exe.
#[cfg(windows)]
pub fn fake(folder: &Path, name: &str, body: &str) -> EngineProgram {
    let stem = Path::new(name).file_stem().unwrap().to_str().unwrap();
    let ps1 = folder.join(format!("{stem}.ps1"));
    std::fs::write(&ps1, body).unwrap();
    let cmd = folder.join(format!("{stem}.cmd"));
    std::fs::write(
        &cmd,
        format!(
            "@powershell -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File \"{}\" %*\r\n",
            ps1.display()
        ),
    )
    .unwrap();
    EngineProgram::new(cmd, folder)
}

/// Starts a child of a fake.
pub fn spawn(program: &EngineProgram) -> mcbench_engine::ChildEngine {
    program.spawn().unwrap()
}

/// Whether a process with this id runs (`kill -0`).
#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success()
}

/// Whether a process with this id runs: it opens and has no exit code yet.
#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    // SAFETY: OpenProcess takes plain values and answers a handle or null;
    // the handle is read once and closed here.
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let read = GetExitCodeProcess(process, &mut code);
        CloseHandle(process);
        read != 0 && code == STILL_ACTIVE as u32
    }
}

/// Kills process `pid` from outside, as the system's out-of-memory killer
/// or a user would: `kill -KILL` on Unix, `taskkill /F` on Windows (which
/// ends that process alone, with exit code 1).
pub fn kill(pid: u32) {
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("taskkill");
        command.args(["/F", "/PID", &pid.to_string()]);
        command
    } else {
        let mut command = std::process::Command::new("kill");
        command.args(["-KILL", &pid.to_string()]);
        command
    };
    assert!(
        command
            .stdout(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    );
}

/// The words of an exit the tests' children were not given by this
/// process: a `SIGKILL` from outside on Unix, exit code 1 of `taskkill /F`
/// on Windows.
pub const KILLED_FROM_OUTSIDE: &str = if cfg!(windows) { "code 1" } else { "SIGKILL" };

/// "At once" for a child told to stop, or an answer that would otherwise
/// wait for a run of many ticks: a bound a starved machine meets.
pub const AT_ONCE: std::time::Duration = std::time::Duration::from_secs(10);

/// Waits up to `within` for process `pid` to be gone; false if it is not.
pub async fn gone_within(pid: u32, within: std::time::Duration) -> bool {
    let began = std::time::Instant::now();
    while alive(pid) {
        if began.elapsed() > within {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    true
}

/// The repository's root (server/ is today's Node server).
pub fn repository() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

/// Runs a script of `tests/node/` with `MCBENCH_DIFF_NODE`, failing the test
/// with Node's stderr when it fails; answers its stdout.
pub fn node(script: &str, args: &[&str]) -> String {
    let node = std::env::var("MCBENCH_DIFF_NODE")
        .expect("MCBENCH_DIFF_NODE names the node program that makes this test's fixtures");
    let output = std::process::Command::new(node)
        .arg(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/node")
                .join(script),
        )
        .args(args)
        .current_dir(repository())
        .output()
        .expect("node runs");
    assert!(
        output.status.success(),
        "node {script} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// A folder of the test's own under the system's temporary folder.
pub fn scratch(name: &str) -> PathBuf {
    let folder =
        std::env::temp_dir().join(format!("mcbench-engine-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).unwrap();
    folder
}
