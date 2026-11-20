"""The error screen after a panic (par-shell, parity pass finding 1), in a
private session: the dev binary panics on purpose and starts itself again
into the error screen.

    check_fatal.py draw[-light]   --crash draw: a panic on the window's own
                                  thread while drawing
    check_fatal.py panic[-light]  --crash panic: a panic on a thread of its
                                  own
    check_fatal.py screen         --crash screen: as draw, then the error
                                  screen's own process panics: it ends and
                                  starts nothing

For draw and panic: the first process exits; a new one shows the alert
`mcbench-lite stopped on an error: <the panic's message>` with Restart
focused, in the ended window's place and theme; a crash log is written;
`light` switches the theme in Settings first. For draw, Restart (Return)
starts the app again and the error screen's process ends. Every process
this check saw is ended by its pid at the end. Pictures: fatal-<mode>-<theme>.png."""

import glob
import os
import signal
import sys
import time

import common
from common import Atspi

mode, _, theme = sys.argv[1].partition("-")
theme = theme or "dark"
report = common.Report(f"fatal-{mode}-{theme}")
seen = set()


def apps():
    """The dev binary's applications on the bus, by process id."""
    found = {}
    desktop = Atspi.get_desktop(0)
    for i in range(desktop.get_child_count()):
        a = desktop.get_child_at_index(i)
        if a is not None and a.get_name() == common.atspi_tree.APP:
            try:
                pid = a.get_process_id()
            except Exception:
                continue
            if alive(pid):
                found[pid] = a
    seen.update(found)
    return found


def alive(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def wait(pred, timeout, what):
    try:
        return common.wait_for(pred, timeout, what)
    except AssertionError:
        return None


def crash_logs():
    return sorted(p for p in glob.glob(os.path.join(common.OUT, "dev", "logs", "crash-*.log"))
                  if not p.endswith("crash-signals.log"))


def focus_screen():
    """The error screen's window takes the keyboard (X11 has no window
    manager here; cage focuses its newest client)."""
    if common.BACKEND == "x11":
        import subprocess
        subprocess.run(["xdotool", "search", "--sync", "--name", "stopped$", "windowfocus", "--sync"], check=True)
    else:
        common.focus_window()
    time.sleep(0.3)


def frame(app):
    found = common.find(app, "frame")
    if not found:
        return None
    e = found[0].get_extents(Atspi.CoordType.SCREEN)
    return (e.width, e.height)


first = common.app()
first_pid = first.get_process_id()
seen.add(first_pid)
time.sleep(1.5)
common.focus_window()
first_frame = frame(first)
if theme == "light":
    Atspi.Component.grab_focus(common.one(first, "button", "Settings"))
    time.sleep(0.3)
    common.key("Return", 1.0)
    Atspi.Action.do_action(common.one(first, "toggle button", "Light"), 0)
    time.sleep(0.6)
    common.key("Escape", 0.8)

# The panic: draw and screen at the window's next draw (a key draws it).
def ended():
    if not alive(first_pid):
        return True
    if mode != "panic":
        try:
            common.key("Tab", 0.2)
        except Exception:
            pass
    return False


report.check("the panicking process ends", bool(wait(ended, 30, "the first process to end")))
report.check("its crash log is written", bool(wait(lambda: len(crash_logs()) >= 1, 5, "a crash log")), str(crash_logs()))

if mode == "screen":
    # The error screen's process starts, panics at its third frame (keys
    # draw it) and starts nothing more.
    def screen_gone():
        others = {p: a for p, a in apps().items() if p != first_pid}
        for _ in others:
            try:
                common.key("Tab", 0.2)
            except Exception:
                pass
        return len(crash_logs()) >= 2 and not others
    report.check("the error screen's own panic ends it", bool(wait(screen_gone, 30, "the screen's process to end")),
                 f"logs {len(crash_logs())}, apps {list(apps())}")
    time.sleep(3.0)
    report.check("and starts nothing more", not apps() and len(crash_logs()) == 2,
                 f"apps {list(apps())}, logs {crash_logs()}")
else:
    def screen_app():
        for pid, a in apps().items():
            if pid != first_pid and common.find(
                    a, pred=lambda o: (o.get_name() or "").startswith("mcbench-lite stopped on an error")):
                return (pid, a)
        return None
    found = wait(screen_app, 20, "the error screen")
    report.check("a new process shows the error screen", bool(found))
    if found:
        pid, screen = found
        time.sleep(1.0)
        focus_screen()
        message = "a panic on purpose while drawing (--crash draw)" if mode == "draw" else "a panic on purpose (--crash panic)"
        alert = common.find(screen, pred=lambda o: (o.get_name() or "").startswith("mcbench-lite stopped on an error"))
        name = alert[0].get_name() if alert else ""
        report.check("the alert says the heading and the panic's message", name == f"mcbench-lite stopped on an error: {message}", name)
        restart = common.find(screen, "button", "Restart")
        report.check("Restart is its button", bool(restart))
        focus = wait(lambda: common.focused(screen), 5, "focus")
        report.check("Restart has focus", focus is not None and focus.get_name() == "Restart", common.label(focus))
        report.check("in the ended window's size", frame(screen) == first_frame, f"{frame(screen)} against {first_frame}")
        common.screenshot(f"fatal-{mode}-{theme}.png")
        if mode == "draw":
            Atspi.Component.grab_focus(restart[0])
            time.sleep(0.3)
            common.key("Return", 0.5)
            again = wait(lambda: [p for p, a in apps().items() if p not in (first_pid, pid)
                                  and common.find(a, "landmark", "Conversation")], 30, "the app again")
            report.check("Restart starts the app again", bool(again), str(list(apps())))
            report.check("and the error screen's process ends", bool(wait(lambda: not alive(pid), 10, "the screen to end")))

for pid in sorted(seen):
    if pid != first_pid and alive(pid):
        os.kill(pid, signal.SIGTERM)
time.sleep(0.5)
report.finish()
