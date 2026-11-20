"""The real app's numbers (pane agent; real_pane.sh with
MCBENCH_REAL_DRIVER=real_numbers.py and MCBENCH_REAL_SIZE / GPUI_X11_SCALE_FACTOR
for the window), X11. In Fill: a 3 s orbit (End held, rotate right), the
world running 60 s, paused and untouched 60 s; then beside the chat a 3 s
split drag. The window's frame times come from its metrics line (written
at quit), cut by the driver's marks; the pane's per-frame cost and
input-to-paint from the `live` lines' costs (cumulative, read at each mark);
the process's memory (RSS) at each mark; the frame images' bytes from the
pane's physical size. Writes numbers.json. MCBENCH_REAL_NUMBERS=short does
the orbit alone (the comparison, the narrow window)."""

import json
import os
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-numbers")
PID = int(os.environ["MCBENCH_APP_PID"])
SHORT = os.environ.get("MCBENCH_REAL_NUMBERS") == "short"
seen = {"marks": {}, "memory_mb": {}, "costs": {}}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def press(name, settle=0.4):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 15.0, name), 0)
    time.sleep(settle)


def rss_mb():
    with open(f"/proc/{PID}/status") as f:
        for line in f:
            if line.startswith("VmRSS:"):
                return round(int(line.split()[1]) / 1024)


def mark(name):
    seen["marks"][name] = time.time() * 1e3
    seen["memory_mb"][name] = rss_mb()
    lives = [a for a in common.actions() if a.get("action") == "live" and "costs" in a]
    if lives:
        seen["costs"][name] = lives[-1]["costs"]


def key(k, down):
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "keydown" if down else "keyup", k], check=True)
    else:
        subprocess.run(["wtype", "-P" if down else "-p", k], check=True)


def age_s():
    """The process's age."""
    with open(f"/proc/{PID}/stat") as f:
        start = float(f.read().rsplit(")", 1)[1].split()[19]) / os.sysconf("SC_CLK_TCK")
    with open("/proc/uptime") as f:
        return float(f.read().split()[0]) - start


app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
mark("first_pane_frame_seen")
time.sleep(1.5)
mark("shown")
# Keys to the composer while the world turns: a right drag on the pane,
# the composer given the focus once the drag has begun, a key every 100 ms.
ec = node("Live Minecraft world").get_extents(Atspi.CoordType.WINDOW)
cx, cy = ec.x + ec.width // 2, ec.y + ec.height // 2
common.pointer("mousemove", str(cx - 150), str(cy))
common.pointer("mousedown", "3")
time.sleep(0.1)
composer = [n for n in common.find(app(), role="text")]
mark("keys_from")
for i in range(1, 31):
    common.pointer("mousemove", str(cx - 150 + i * 10), str(cy))
    if i == 2 and composer:
        Atspi.Component.grab_focus(composer[-1])
    if i > 2:
        if common.BACKEND == "x11":
            subprocess.run(["xdotool", "type", "--delay", "0", "k"], check=True)
        else:
            subprocess.run(["wtype", "k"], check=True)
    time.sleep(0.1)
common.pointer("mouseup", "3")
mark("keys_to")
time.sleep(0.5)
press("Fill the window", 1.5)
canvas = common.wait_for(lambda: node("Live Minecraft world"), 10.0, "the canvas")
e = canvas.get_extents(Atspi.CoordType.WINDOW)
seen["pane_logical"] = [e.width, e.height]
Atspi.Component.grab_focus(canvas)
time.sleep(0.5)
press("Pause", 1.0)  # the costs so far, logged
mark("orbit_from")
if common.BACKEND == "x11":
    key("End", True)
    time.sleep(3.0)
    key("End", False)
else:
    # wtype's held key ends with wtype: a right drag for 3 s instead.
    cx, cy = e.x + e.width // 2, e.y + e.height // 2
    common.pointer("mousemove", str(cx - 300), str(cy))
    common.pointer("mousedown", "3")
    for i in range(1, 151):
        common.pointer("mousemove", str(cx - 300 + i * 4), str(cy))
        time.sleep(0.02)
    common.pointer("mouseup", "3")
mark("orbit_to")
time.sleep(0.5)

Atspi.Component.grab_focus(canvas)
time.sleep(0.3)
time.sleep(max(0.0, 20.0 - age_s()))
mark("at_20s")
press("Play", 0.2)
mark("run_from")
if not SHORT:
    time.sleep(60.0)
    press("Pause", 0.2)
    mark("run_to")
    mark("idle_from")
    time.sleep(60.0)
    mark("idle_to")
    press("Back beside the chat (Esc)", 1.5)
    canvas = node("Live Minecraft world")
    e = canvas.get_extents(Atspi.CoordType.WINDOW)
    x, y, w, h = e.x, e.y, e.width, e.height
    divider = x - 3
    # The narrow window (980 px or less) has no split: the world is a row.
    if divider > 10:
        common.pointer("mousemove", str(divider), str(y + h // 2))
        common.pointer("mousedown", "1")
        mark("split_from")
        for i in range(1, 61):
            common.pointer("mousemove", str(divider - (i if i <= 30 else 60 - i) * 8), str(y + h // 2))
            time.sleep(0.05)
        common.pointer("mouseup", "1")
        mark("split_to")
        time.sleep(0.5)
# The costs logged by a toolbar press: the world is paused after the idle
# minute, running after the short orbit.
press("Play" if node("Play") is not None else "Pause", 0.5)
mark("end")
with open(common.out("numbers.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.check("the marks were taken", "orbit_to" in seen["marks"], "")
report.finish()
