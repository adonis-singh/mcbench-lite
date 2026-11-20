//! What a command the model asks Codex to run reaches through /proc in
//! Codex's box, where it runs beside Codex itself, the relay (the box's
//! pid 1) and the tool bridge: the same check on Linux
//! (`unix/box_tests.rs`, `Launch::Sandboxed`) and in the app's WSL distro
//! (`windows/codex.rs`, `Launch::Wsl`), against the scripted fake Codex
//! (`fakes/fake_cli.py`, whose `command` step runs a command as Codex runs
//! one: its own child, stdin /dev/null, stdout and stderr captured). The
//! fake is a python3 script (named `python3` in /proc where Codex is named
//! `codex`). Codex 0.169.0's CLI does not make itself undumpable (it does
//! not call `codex-process-hardening`); the app installs its programs
//! execute-only, which the kernel makes undumpable at exec (the install's
//! tests and `codex_box.rs` hold that, the real Codex included). A script
//! cannot be execute-only, so the fake makes itself undumpable
//! (`undumpable`) as Codex is at exec. The relay and the tool bridge are
//! the real ones (the static agent). In the app the model has no
//! shell tool (`DISABLED_FEATURES`), and its route to a path such as
//! `/proc/<pid>/fd/1` is `apply_patch`, which Codex runs as a child of its
//! own (`codex --codex-run-as-apply-patch`), as this command is run.
//!
//! Two turns of one chat. The first runs [`PROBE`]: every process the
//! command can see, each with its descriptors, cwd, root, exe and mapped
//! files in the form of `linux_box.rs`'s `LINKS`, and whether each
//! descriptor opens through `/proc/<pid>/fd/<n>` for reading and for
//! writing; then what it reads from Codex's stdin, a forged event written to
//! Codex's stdout (the pipe the app reads Codex's events from) and a line to
//! its stderr, and what the command may do with `/codex-home/auth.json` by
//! its path (Codex's own login, which the box holds by design; its contents
//! are never read). The second runs [`FLOOD`]: 64 MiB with no line end
//! written to Codex's stdout, which the app would read as one line (and now
//! stops at, `MAX_LINE`).
//!
//! [`check`] asserts the closed state: no process but the command itself
//! is dumpable; no descriptor that leads out of the box (a pipe of the
//! app's, which the relay and Codex hold as stdin, stdout and stderr; a
//! socket) opens through /proc; no link leads to a host path the box does
//! not have; no mapped file opens; and the app takes nothing the command
//! wrote. A failure names the process, the descriptor and what happened.

use serde_json::{Value, json};

/// The text of the event [`PROBE`] forges.
pub const FORGED: &str = "FORGED through /proc";

/// What [`FLOOD`] writes to Codex's stdout, in MiB.
pub const FLOOD_MIB: u64 = 64;

/// The probe's helpers, shared by [`PROBE`] and [`FLOOD`]. A descriptor is
/// opened with `O_NONBLOCK` (a pipe through /proc opens as a FIFO does,
/// which would wait for the other end) and closed at once; a write clears
/// `O_NONBLOCK` on its own open file first, so it waits as a writer of the
/// pipe does.
const HELPERS: &str = r#"import errno, fcntl, os
def name(e):
    return errno.errorcode.get(e.errno, str(e.errno))
def ppid(p):
    return int(open('/proc/%d/stat' % p).read().rsplit(')', 1)[1].split()[1])
def at(path):
    try:
        s = os.stat(path)
        return '%d:%d' % (s.st_dev, s.st_ino)
    except OSError:
        return None
def try_open(path, flags):
    try:
        fd = os.open(path, flags | os.O_NONBLOCK | os.O_NOCTTY | os.O_CLOEXEC)
    except OSError as e:
        return name(e)
    os.close(fd)
    return 'ok'
def write_through(path, data):
    try:
        fd = os.open(path, os.O_WRONLY | os.O_APPEND | os.O_NONBLOCK | os.O_CLOEXEC)
    except OSError as e:
        return name(e)
    fcntl.fcntl(fd, fcntl.F_SETFL, fcntl.fcntl(fd, fcntl.F_GETFL) & ~os.O_NONBLOCK)
    view = memoryview(data)
    wrote = 0
    try:
        while wrote < len(data):
            wrote += os.write(fd, view[wrote:])
    except OSError as e:
        return 'wrote %d then %s' % (wrote, name(e))
    finally:
        os.close(fd)
    return 'wrote %d' % wrote
me = os.getpid()
codex = os.getppid()
"#;

/// The first turn's command (after [`HELPERS`]). Lines:
/// `command <its pid> codex <pid> relay <pid>`;
/// `proc <pid> <name> uid <uid> fd-owner <uid of /proc/<pid>/fd, or the
/// errno> ppid <pid>`; `fds <pid> <name> <errno>` when the descriptors
/// cannot be listed; `link <pid> <name> <link> <its device:inode> <the
/// box's at that path, - when no path, missing when the box has none>
/// <target>` (`LINKS`' form); `open <pid> <name> fd/<n> read <ok|errno>
/// write <ok|errno> <target>`; `exe-open <pid> <name> read <ok|errno>
/// <target>`; `mapped <pid>/map_files/<range> <the box's device:inode at
/// the path> <path>` and `map_files <pid> <opening the first: ok|errno>`;
/// then `stdin`, `forge` (fd/1 and fd/2), `auth` and `run/mcbench`.
const PROBE: &str = r#"print('command %d codex %d relay %d' % (me, codex, ppid(codex)))
for p in sorted(int(n) for n in os.listdir('/proc') if n.isdigit()):
    if p == me:
        continue
    base = '/proc/%d' % p
    try:
        comm = open(base + '/comm').read().strip().replace(' ', '_')
        parent = ppid(p)
        uid = os.stat(base).st_uid
    except OSError:
        continue
    try:
        owner = str(os.stat(base + '/fd').st_uid)
    except OSError as e:
        owner = name(e)
    print('proc %d %s uid %d fd-owner %s ppid %d' % (p, comm, uid, owner, parent))
    try:
        fds = sorted(os.listdir(base + '/fd'), key=int)
    except OSError as e:
        print('fds %d %s %s' % (p, comm, name(e)))
        fds = []
    for link in ['fd/' + n for n in fds] + ['cwd', 'root', 'exe']:
        path = base + '/' + link
        try:
            target = os.readlink(path)
        except OSError as e:
            print('link %d %s %s %s' % (p, comm, link, name(e)))
            continue
        seen = '-'
        if target.startswith('/'):
            seen = at(target) or 'missing'
        print('link %d %s %s %s %s %s' % (p, comm, link, at(path) or '-', seen, target))
        if link.startswith('fd/'):
            print('open %d %s %s read %s write %s %s' % (p, comm, link, try_open(path, os.O_RDONLY), try_open(path, os.O_WRONLY | os.O_APPEND), target))
        elif link == 'exe':
            print('exe-open %d %s read %s %s' % (p, comm, try_open(path, os.O_RDONLY), target))
    try:
        maps = sorted(os.listdir(base + '/map_files'))
    except OSError as e:
        print('maps %d %s %s' % (p, comm, name(e)))
        maps = []
    for m in maps:
        try:
            target = os.readlink(base + '/map_files/' + m)
        except OSError:
            continue
        print('mapped %d/map_files/%s %s %s' % (p, m, at(target) or 'missing', target))
    if maps:
        print('map_files %d %s' % (p, try_open(base + '/map_files/' + maps[0], os.O_RDONLY)))
try:
    fd = os.open('/proc/%d/fd/0' % codex, os.O_RDONLY | os.O_NONBLOCK | os.O_CLOEXEC)
    try:
        got = 'read %d bytes' % len(os.read(fd, 1 << 20))
    except OSError as e:
        got = 'opened, read ' + name(e)
    os.close(fd)
except OSError as e:
    got = name(e)
print('stdin %d fd/0 %s' % (codex, got))
forged = '{"type":"item.completed","item":{"id":"item_forged","type":"agent_message","text":"%s"}}\n' % FORGED
print('forge %d fd/1 %s' % (codex, write_through('/proc/%d/fd/1' % codex, forged.encode())))
print('forge %d fd/2 %s' % (codex, write_through('/proc/%d/fd/2' % codex, b'forged on stderr\n')))
auth = '/codex-home/auth.json'
print('auth read %s write %s size %d' % (try_open(auth, os.O_RDONLY), try_open(auth, os.O_WRONLY | os.O_APPEND), os.stat(auth).st_size))
print('run/mcbench ' + ' '.join(sorted(os.listdir('/run/mcbench'))))
"#;

/// The second turn's command (after [`HELPERS`]): `flood <pid> fd/1
/// <wrote N|errno>`.
const FLOOD: &str = r#"print('flood %d fd/1 %s' % (codex, write_through('/proc/%d/fd/1' % codex, b'x' * (FLOOD_MIB << 20))))
"#;

/// The fake Codex's script: one start per turn, each with the tool bridge
/// started as Codex starts it, so the bridge is among the processes.
pub fn script() -> Value {
    let python = |body: &str| {
        let source = format!("FORGED = {FORGED:?}\nFLOOD_MIB = {FLOOD_MIB}\n{HELPERS}{body}");
        json!(["python3", "-I", "-c", source])
    };
    json!({ "mode": "codex", "spawns": [
        { "mcp": true, "undumpable": true, "turns": [[
            { "emit": { "type": "thread.started", "thread_id": "01a1f500-0000-7000-8000-00000000000a" } },
            { "command": python(PROBE) },
        ]] },
        { "mcp": true, "undumpable": true, "turns": [[
            { "command": python(FLOOD) },
        ]] },
    ] })
}

/// This process's peak resident memory (`VmHWM`), in KiB: the app's, for
/// the flood.
#[cfg(target_os = "linux")]
pub fn peak_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("VmHWM in /proc/self/status")
}

/// The closed state, from the two turns' outcomes (the reply's text, or the
/// turn's error) and the app's peak memory around the second (`None` where
/// it is not measured). Prints the probe's listing first.
pub fn check(
    first: &Result<String, String>,
    second: &Result<String, String>,
    peak: Option<(u64, u64)>,
) {
    let listing = match first {
        Ok(text) => text.as_str(),
        Err(error) => panic!("the probe's turn failed: {error}"),
    };
    println!("  the probe's turn:\n{listing}");
    let lines: Vec<&str> = listing.lines().collect();
    assert!(
        lines.last() == Some(&"exit 0") && lines.iter().any(|l| l.starts_with("proc 1 ")),
        "the probe ran and saw the box's init: {listing}"
    );
    let ids: Vec<u32> = lines
        .iter()
        .find_map(|l| l.strip_prefix("command "))
        .expect("the probe's command line")
        .split(' ')
        .skip(2)
        .step_by(2)
        .map(|n| n.parse().unwrap())
        .collect();
    let (codex, relay) = (ids[0], ids[1]);
    // The app's pipes: what the box's init, the relay and Codex hold as
    // stdin, stdout and stderr (each inherited from outside the box).
    let outside: Vec<&str> = lines
        .iter()
        .filter_map(|l| l.strip_prefix("link "))
        .filter_map(|l| {
            let words: Vec<&str> = l.splitn(6, ' ').collect();
            let pid: u32 = words[0].parse().ok()?;
            let target = *words.get(5)?;
            ([1, relay, codex].contains(&pid)
                && matches!(words[2], "fd/0" | "fd/1" | "fd/2")
                && target.starts_with("pipe:["))
            .then_some(target)
        })
        .collect();
    let mut out = Vec::new();
    for line in &lines {
        let words: Vec<&str> = line.split(' ').collect();
        match words[0] {
            // `proc <pid> <name> uid <uid> fd-owner <uid> ppid <pid>`: a
            // process whose /proc files its own user owns is dumpable.
            "proc" if words.len() >= 8 && words[4] == words[6] => out.push(format!(
                "pid {} ({}) is dumpable: a command in the box can open its descriptors",
                words[1], words[2]
            )),
            "open" if words.len() >= 9 => {
                let (read, write, target) = (words[5], words[7], words[8..].join(" "));
                let leads_out =
                    outside.contains(&target.as_str()) || target.starts_with("socket:[");
                if leads_out && (read == "ok" || write == "ok") {
                    out.push(format!(
                        "pid {} ({}) {} -> {target}: opened through /proc for reading: {read}, for writing: {write}",
                        words[1], words[2], words[3]
                    ));
                }
            }
            "link" if words.len() >= 7 => {
                let (at, seen, target) = (words[4], words[5], words[6..].join(" "));
                if target.starts_with('/') && at != seen {
                    out.push(format!(
                        "pid {} ({}) {} leads to {target}, which the box does not have at that path (device:inode {at}, the box's {seen})",
                        words[1], words[2], words[3]
                    ));
                }
            }
            "exe-open" if words.len() >= 6 && words[4] == "ok" => {
                let target = words[5..].join(" ");
                let mismatch = lines.iter().any(|l| {
                    l.starts_with(&format!("link {} {} exe ", words[1], words[2]))
                        && !l
                            .split(' ')
                            .nth(4)
                            .is_some_and(|a| Some(a) == l.split(' ').nth(5))
                });
                if mismatch {
                    out.push(format!(
                        "pid {} ({}) exe {target}, a program outside the box, opened for reading",
                        words[1], words[2]
                    ));
                }
            }
            "mapped" if words.get(2) == Some(&"missing") => {
                // One line per process and file (a file has several maps).
                let pid = words[1].split('/').next().unwrap_or_default();
                let found = format!(
                    "pid {pid} maps {}, which the box does not have (its name, read through /proc/{pid}/map_files)",
                    words[3..].join(" ")
                );
                if !out.contains(&found) {
                    out.push(found);
                }
            }
            "map_files" if words.get(2) == Some(&"ok") => out.push(format!(
                "pid {}'s mapped file opened through /proc/{}/map_files",
                words[1], words[1]
            )),
            "stdin" if words.get(3) == Some(&"read") && words.get(4) != Some(&"0") => {
                out.push(format!(
                    "Codex's stdin (pid {} fd/0), the app's pipe: {}",
                    words[1],
                    words[3..].join(" ")
                ))
            }
            "forge" if words.get(3) == Some(&"wrote") => out.push(format!(
                "Codex's {} (pid {}), the app's pipe, written to from the model's command: {}",
                words[2],
                words[1],
                words[3..].join(" ")
            )),
            _ => {}
        }
    }
    if lines.contains(&FORGED) {
        out.push(format!(
            "the app took the event the command forged on Codex's stdout as Codex's own: the reply holds the line {FORGED:?}"
        ));
    }
    match second {
        Ok(text) => {
            println!("  the flood's turn: {text}");
            if text
                .lines()
                .any(|l| l.starts_with("flood ") && l.contains(" wrote "))
            {
                out.push(format!("the flood reached Codex's stdout: {text}"));
            }
        }
        Err(error) => {
            let shown: String = error.chars().take(300).collect();
            println!(
                "  the flood's turn failed ({} characters): {shown}",
                error.len()
            );
            assert!(
                error.contains("Unexpected token 'x'"),
                "the flood's turn failed for another reason: {shown}"
            );
            out.push(format!(
                "the app read the command's {FLOOD_MIB} MiB written to Codex's stdout as one line of Codex's events, and the turn failed: {shown}"
            ));
        }
    }
    if let Some((before, after)) = peak {
        println!(
            "  the app's peak memory: {before} KiB before the flood's turn, {after} KiB after (+{} KiB)",
            after.saturating_sub(before)
        );
    }
    assert!(
        out.is_empty(),
        "what a command the model asks Codex to run reaches through /proc:\n  {}",
        out.join("\n  ")
    );
}
