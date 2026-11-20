"""The session's own proof that it stayed private (session.sh runs it every
time).

    privacy.py snapshot <file>
        The user's real runtime folder's `at-spi/` listing, the user's
        dconf file's modification time and the user's accessibility switch
        (`org.gnome.desktop.interface toolkit-accessibility`), written to
        <file>.
    privacy.py switch
        Exits 1, saying so, when the user's accessibility switch reads
        `true`: session.sh runs it before anything else and stops, changing
        nothing, since someone else set it.
    privacy.py watch <root pid> <runtime> <before file> <log file> <stop file>
        Until <stop file> exists, twice a second: every descendant of
        <root pid> and every process whose environment has
        XDG_RUNTIME_DIR=<runtime> (the session's, reparented ones too) is
        checked for a socket bound under the real
        runtime folder or connected to one, and for any open file there; the
        snapshot is taken again and compared with <before file>. Each finding
        is a line in <log file>; the samples taken are counted on its last
        line.
    privacy.py procs <runtime> <file>
        The processes whose environment has XDG_RUNTIME_DIR=<runtime>
        (pid, parent, process group, name), written to <file>; exits 1 when
        there are any.
    privacy.py verdict <before file> <after file> <log file>
        Exits 1 when the snapshots differ or the log holds a finding.

The real runtime folder is /run/user/<uid>; the dconf file is
~/.config/dconf/user (the real home's, whatever XDG_CONFIG_HOME says).
The switch is read with `dconf read` against the user's real configuration
folder and with no D-Bus address, so the read writes nothing and touches no
bus; an unset key reads as empty, which counts as false. A run whose switch
reads `true` before or after fails (the verdict)."""

import os
import subprocess
import sys
import time

REAL_RUNTIME = f"/run/user/{os.getuid()}"
DCONF = os.path.join(os.path.expanduser("~"), ".config", "dconf", "user")


SWITCH = "/org/gnome/desktop/interface/toolkit-accessibility"


def switch():
    """The user's accessibility switch as `dconf read` prints it ('' when unset)."""
    env = {k: v for k, v in os.environ.items() if k not in ("DBUS_SESSION_BUS_ADDRESS", "DCONF_PROFILE")}
    env["XDG_CONFIG_HOME"] = os.path.join(os.path.expanduser("~"), ".config")
    read = subprocess.run(["dconf", "read", SWITCH], capture_output=True, text=True, env=env)
    if read.returncode != 0:
        sys.exit(f"privacy.py: dconf read {SWITCH} failed: {read.stderr.strip()}")
    return read.stdout.strip()


def snapshot():
    listing = subprocess.run(
        ["ls", "-la", "--time-style=full-iso", os.path.join(REAL_RUNTIME, "at-spi")],
        capture_output=True, text=True,
    )
    try:
        dconf = f"{os.stat(DCONF).st_mtime_ns} {DCONF}"
    except FileNotFoundError:
        dconf = f"absent {DCONF}"
    return (f"at-spi listing:\n{listing.stdout}{listing.stderr}dconf mtime: {dconf}\n"
            f"toolkit-accessibility: {switch() or '(unset)'}\n")


def descendants(root):
    children = {}
    for entry in os.listdir("/proc"):
        if not entry.isdigit():
            continue
        try:
            with open(f"/proc/{entry}/stat") as f:
                stat = f.read()
        except OSError:
            continue
        ppid = int(stat[stat.rindex(")") + 2:].split()[1])
        children.setdefault(ppid, []).append(int(entry))
    found, todo = [], [root]
    while todo:
        pid = todo.pop()
        found.append(pid)
        todo.extend(children.get(pid, []))
    return found


def of_runtime(runtime):
    """Our processes whose environment names `runtime` as XDG_RUNTIME_DIR."""
    wanted = f"XDG_RUNTIME_DIR={runtime}".encode()
    found = []
    for entry in os.listdir("/proc"):
        if not entry.isdigit() or int(entry) == os.getpid():
            continue
        try:
            if os.stat(f"/proc/{entry}").st_uid != os.getuid():
                continue
            with open(f"/proc/{entry}/environ", "rb") as f:
                if wanted in f.read().split(b"\0"):
                    found.append(int(entry))
        except OSError:
            continue
    return found


def describe(pid):
    with open(f"/proc/{pid}/stat") as f:
        stat = f.read()
    comm = stat[stat.index("(") + 1:stat.rindex(")")]
    fields = stat[stat.rindex(")") + 2:].split()
    return f"{pid} ppid {fields[1]} pgid {fields[2]} {comm}"


def real_runtime_sockets():
    """Inodes of sockets bound under the real runtime folder, and the inodes
    of their peers (the clients connected to them)."""
    bound, peers = set(), set()
    out = subprocess.run(["ss", "-xan"], capture_output=True, text=True).stdout
    for line in out.splitlines()[1:]:
        fields = line.split()
        if len(fields) < 8:
            continue
        local, local_inode, peer_inode = fields[4], fields[5], fields[7]
        if local.startswith(REAL_RUNTIME + "/"):
            bound.add(local_inode)
            if peer_inode not in ("*", "0"):
                peers.add(peer_inode)
    return bound, peers


def check(root, runtime):
    findings = []
    bound, peers = real_runtime_sockets()
    for pid in sorted(set(descendants(root)) | set(of_runtime(runtime))):
        try:
            fds = os.listdir(f"/proc/{pid}/fd")
            comm = open(f"/proc/{pid}/comm").read().strip()
        except OSError:
            continue
        for fd in fds:
            try:
                target = os.readlink(f"/proc/{pid}/fd/{fd}")
            except OSError:
                continue
            if target.startswith(REAL_RUNTIME + "/") or target == REAL_RUNTIME:
                findings.append(f"pid {pid} ({comm}) fd {fd} is open on {target}")
            elif target.startswith("socket:["):
                inode = target[len("socket:["):-1]
                if inode in bound:
                    findings.append(f"pid {pid} ({comm}) fd {fd} is a socket bound under {REAL_RUNTIME}")
                elif inode in peers:
                    findings.append(f"pid {pid} ({comm}) fd {fd} is connected to a socket under {REAL_RUNTIME}")
    return findings


def main():
    mode = sys.argv[1]
    if mode == "snapshot":
        with open(sys.argv[2], "w") as f:
            f.write(snapshot())
    elif mode == "switch":
        value = switch()
        if value == "true":
            sys.exit(f"privacy.py: the user's {SWITCH} reads true at the start of this run: "
                     "someone else set it; nothing was changed and the run stops")
    elif mode == "watch":
        root, runtime, before, log, stop = int(sys.argv[2]), sys.argv[3], sys.argv[4], sys.argv[5], sys.argv[6]
        expected = open(before).read()
        samples = 0
        with open(log, "w") as f:
            while not os.path.exists(stop):
                for finding in check(root, runtime):
                    f.write(finding + "\n")
                now = snapshot()
                if now != expected:
                    f.write("the snapshot changed during the run:\n" + now)
                samples += 1
                f.flush()
                time.sleep(0.5)
            f.write(f"samples: {samples}\n")
    elif mode == "procs":
        lines = []
        for pid in of_runtime(sys.argv[2]):
            try:
                lines.append(describe(pid))
            except OSError:
                continue
        with open(sys.argv[3], "w") as f:
            f.write("".join(line + "\n" for line in lines) or "none\n")
        sys.exit(1 if lines else 0)
    elif mode == "verdict":
        before, after, log = (open(p).read() for p in sys.argv[2:5])
        lines = [l for l in log.splitlines() if not l.startswith("samples: ")]
        switched = "toolkit-accessibility: true" in before + after
        ok = before == after and not lines and "samples: " in log and not switched
        print(("private: " if ok else "NOT PRIVATE: ")
              + f"snapshots {'equal' if before == after else 'differ'}, "
              + f"{len(lines)} findings, {log.splitlines()[-1] if log else 'no samples'}"
              + (", the user's toolkit-accessibility reads true" if switched else ""))
        sys.exit(0 if ok else 1)
    else:
        sys.exit(f"privacy.py: unknown mode {mode}")


main()
