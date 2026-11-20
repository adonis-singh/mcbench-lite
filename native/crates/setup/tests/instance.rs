//! The data folder's lock between processes: the native app against
//! today's localhost app (`server/index.js` taking `server.pid` with
//! `server/pid-lock.js`, run by `tests/node/pid-lock-holder.js`), in both
//! directions, and two native apps against each other. Each native holder
//! is a child process of this test binary (`native_holder`).
//!
//! The tests against Node need `node` on `PATH` and `MCBENCH_DIFF_REPO`, a
//! checkout whose `server/` is today's (no `node_modules` needed); without
//! it they are ignored, and with it a missing file fails them. Run them with
//! `--include-ignored --skip native_holder` (the holder runs only as a
//! child).

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Output, Stdio};

use mcbench_setup::InstanceLock;
use mcbench_setup::instance::pid_lock;

/// The lock's file in the data folder, as `server/index.js` names it.
const SERVER_PID: &str = "server.pid";

/// A scratch folder removed when dropped, also when the test fails.
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(name: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "mcbench-setup-instance-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

fn names(dir: &Path) -> Vec<String> {
    let mut list: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    list.sort();
    list
}

/// The sentence a holder that lives is refused with, by either program.
fn held(file: &Path, pid: u32) -> String {
    format!(
        "{} is held by process {pid}, which is still running",
        file.display()
    )
}

/// A process that has taken a lock: it holds it until its stdin closes.
/// Killed and waited for when dropped, so no exit path leaves it running.
struct Holder {
    child: Child,
    out: BufReader<ChildStdout>,
    pid: u32,
}

impl Holder {
    /// Starts `command` and waits for its `held <pid>` line.
    fn start(mut command: Command) -> Holder {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut holder = Holder { child, out, pid: 0 };
        let mut line = String::new();
        loop {
            line.clear();
            assert_ne!(
                holder.out.read_line(&mut line).unwrap(),
                0,
                "no `held` line"
            );
            if let Some(pid) = line.trim_end().strip_prefix("held ") {
                holder.pid = pid.parse().unwrap();
                assert_eq!(holder.pid, holder.child.id());
                return holder;
            }
        }
    }

    /// Closes its stdin: it releases the lock and exits.
    fn release(mut self) {
        drop(self.child.stdin.take());
        let mut rest = String::new();
        self.out.read_to_string(&mut rest).unwrap();
        assert!(rest.contains("released"), "{rest}");
        assert!(self.child.wait().unwrap().success());
    }

    /// Waits for it to exit by itself (`die`): its pid is then no process's.
    fn died(mut self) -> u32 {
        assert!(self.child.wait().unwrap().success());
        self.pid
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The checkout whose `server/` is today's.
fn checkout() -> PathBuf {
    let repo = PathBuf::from(
        std::env::var("MCBENCH_DIFF_REPO").expect("MCBENCH_DIFF_REPO names a checkout"),
    );
    for file in ["server/pid-lock.js", "server/storage.js", "server/index.js"] {
        assert!(repo.join(file).is_file(), "{} is missing", file);
    }
    repo
}

/// Today's server taking `file` of data folder `dir` (`mode`: `hold`, `die`
/// or `take`, `tests/node/pid-lock-holder.js`).
fn node(dir: &Path, file: &str, mode: &str) -> Command {
    let mut command = Command::new("node");
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node/pid-lock-holder.js"))
        .arg(checkout())
        .arg(file)
        .arg(mode)
        .env("MCBENCH_DATA_DIR", dir);
    command
}

/// Asserts that today's server was refused `file` because process `pid`
/// holds it, with its own sentence, before it held anything.
fn node_refused(output: Output, file: &Path, pid: u32) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = format!("Error: {}", held(file, pid));
    println!("node: exit {:?}, `{line}`", output.status.code());
    assert_eq!(output.status.code(), Some(1), "{stdout}\n{stderr}");
    assert!(!stdout.contains("held"), "{stdout}");
    assert!(stderr.lines().any(|l| l == line), "{stderr}");
}

/// The native app taking data folder `dir` in a child process of this test
/// binary (`mode`: `hold` or `die`).
fn native(dir: &Path, mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "native_holder", "--ignored", "--nocapture"])
        .env("MCBENCH_LOCK_HOLDER", dir)
        .env("MCBENCH_LOCK_MODE", mode);
    command
}

#[test]
#[ignore = "run by this file's tests in a child process, with MCBENCH_LOCK_HOLDER and MCBENCH_LOCK_MODE set"]
fn native_holder() {
    let dir = PathBuf::from(std::env::var("MCBENCH_LOCK_HOLDER").unwrap());
    let lock = InstanceLock::take(&dir).unwrap();
    println!("held {}", std::process::id());
    match std::env::var("MCBENCH_LOCK_MODE").unwrap().as_str() {
        "hold" => {
            std::io::stdin().read_to_end(&mut Vec::new()).unwrap();
            drop(lock);
            println!("released");
        }
        "die" => std::process::exit(0),
        mode => panic!("unknown mode {mode}"),
    }
}

#[test]
#[ignore = "needs Node and today's server/: set MCBENCH_DIFF_REPO"]
fn the_native_app_is_refused_a_folder_todays_server_holds_until_it_lets_go() {
    let dir = scratch("node-holds");
    let file = dir.join(SERVER_PID);
    let server = Holder::start(node(&dir, SERVER_PID, "hold"));
    let refused = InstanceLock::take(&dir).unwrap_err();
    println!("native: `{refused}`");
    assert_eq!(refused, held(&file, server.pid));
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        server.pid.to_string()
    );
    server.release();
    assert!(names(&dir).is_empty());
    let lock = InstanceLock::take(&dir).unwrap();
    assert_eq!(names(&dir), [SERVER_PID]);
    drop(lock);
}

#[test]
#[ignore = "needs Node and today's server/: set MCBENCH_DIFF_REPO"]
fn todays_server_is_refused_a_folder_the_native_app_holds_until_it_lets_go() {
    let dir = scratch("native-holds");
    let file = dir.join(SERVER_PID);
    let app = Holder::start(native(&dir, "hold"));
    node_refused(
        node(&dir, SERVER_PID, "take").output().unwrap(),
        &file,
        app.pid,
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), app.pid.to_string());
    app.release();
    assert!(names(&dir).is_empty());
    assert!(node(&dir, SERVER_PID, "take").status().unwrap().success());
    assert!(names(&dir).is_empty());
}

#[test]
#[ignore = "needs Node and today's server/: set MCBENCH_DIFF_REPO"]
fn the_native_app_takes_over_the_lock_a_dead_server_left_as_node_does() {
    let dir = scratch("node-died");
    let file = dir.join(SERVER_PID);
    let dead = Holder::start(node(&dir, SERVER_PID, "die")).died();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), dead.to_string());
    // Today's server takes it over, leaving only its own lock.
    let server = Holder::start(node(&dir, SERVER_PID, "hold"));
    assert_eq!(names(&dir), [SERVER_PID]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        server.pid.to_string()
    );
    drop(server);
    // Killed, it left its lock too: the native app takes it over the same way.
    let lock = InstanceLock::take(&dir).unwrap();
    println!("native: took over a dead server's lock");
    assert_eq!(names(&dir), [SERVER_PID]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        std::process::id().to_string()
    );
    drop(lock);
    assert!(names(&dir).is_empty());
}

#[test]
#[ignore = "needs Node and today's server/: set MCBENCH_DIFF_REPO"]
fn todays_server_takes_over_the_lock_a_dead_native_app_left() {
    let dir = scratch("native-died");
    let file = dir.join(SERVER_PID);
    let dead = Holder::start(native(&dir, "die")).died();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), dead.to_string());
    let server = Holder::start(node(&dir, SERVER_PID, "hold"));
    println!("node: took over a dead native app's lock");
    assert_eq!(names(&dir), [SERVER_PID]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        server.pid.to_string()
    );
    server.release();
    assert!(names(&dir).is_empty());
}

#[test]
#[ignore = "needs Node and today's server/: set MCBENCH_DIFF_REPO"]
fn a_takeover_in_progress_in_one_program_holds_off_the_other() {
    // Today's server is removing a dead holder's lock (it holds the
    // takeover): the native app waits for it, then is refused naming it.
    let dir = scratch("node-takes-over");
    let file = dir.join(SERVER_PID);
    let takeover = dir.join(format!("{SERVER_PID}.takeover"));
    let dead = Holder::start(node(&dir, SERVER_PID, "die")).died();
    let server = Holder::start(node(&dir, &format!("{SERVER_PID}.takeover"), "hold"));
    let refused = InstanceLock::take(&dir).unwrap_err();
    println!("native: `{refused}`");
    assert_eq!(refused, held(&takeover, server.pid));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), dead.to_string());
    server.release();
    let lock = InstanceLock::take(&dir).unwrap();
    assert_eq!(names(&dir), [SERVER_PID]);
    drop(lock);
    // And the other way: the native app holds the takeover of a dead native
    // app's lock; today's server waits, then is refused naming it.
    let dead = Holder::start(native(&dir, "die")).died();
    pid_lock(&takeover, 0).unwrap();
    node_refused(
        node(&dir, SERVER_PID, "take").output().unwrap(),
        &takeover,
        std::process::id(),
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), dead.to_string());
    std::fs::remove_file(&takeover).unwrap();
    assert!(node(&dir, SERVER_PID, "take").status().unwrap().success());
    assert!(names(&dir).is_empty());
}

#[test]
fn a_second_native_app_is_refused_while_the_first_lives_and_takes_over_once_it_is_dead() {
    let dir = scratch("two-native");
    let file = dir.join(SERVER_PID);
    let first = Holder::start(native(&dir, "hold"));
    let refused = InstanceLock::take(&dir).unwrap_err();
    println!("native: `{refused}`");
    assert_eq!(refused, held(&file, first.pid));
    first.release();
    assert!(names(&dir).is_empty());
    let dead = Holder::start(native(&dir, "die")).died();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), dead.to_string());
    let second = Holder::start(native(&dir, "hold"));
    println!("native: a second start took over a dead one's lock");
    assert_eq!(names(&dir), [SERVER_PID]);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        second.pid.to_string()
    );
    second.release();
    assert!(names(&dir).is_empty());
}
