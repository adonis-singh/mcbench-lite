"""The 3D pane's speed over the fake pane (pane agent): the pane fills the
window (so its size is the window's), then one of

    check_pane_perf.py orbit   a 3 s orbit (End held: rotate right)
    check_pane_perf.py drag    a 3 s right drag
    check_pane_perf.py key     20 taps of W, 300 ms apart
    check_pane_perf.py run     the world running at 20 ticks a second for 60 s
    check_pane_perf.py idle    the world paused, untouched, for 60 s: counted
                               from 1 s after the pane says it is paused
                               (the tick in flight at the press paints 18
                               to 66 ms after it, measured on adipc: the
                               pause's own); none may paint later than
                               500 ms after the press

The window's own numbers (each new frame's take, copy and paint; from a
camera input to the paint of the frame after it) come from the costs the
binary logs with each press of the toolbar (Pause at the end); frames painted between two presses of the toolbar
from its `live` lines; the process's CPU from /proc. Writes perf.json."""

import json
import os
import subprocess
import sys
import time

import common

Atspi = common.Atspi
mode = sys.argv[1]
report = common.Report(f"pane-perf-{mode}")
PID = int(os.environ["MCBENCH_APP_PID"])
TICK = os.sysconf("SC_CLK_TCK")


def app():
    return common.app()


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def press(name):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 10.0, name), 0)
    time.sleep(0.3)


def cpu():
    with open(f"/proc/{PID}/stat") as f:
        fields = f.read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / TICK


def frames_logged():
    return [a["frames"] for a in common.actions() if a.get("action") == "live" and "frames" in a]


def load():
    with open("/proc/loadavg") as f:
        return f.read().split()[0]


app()
time.sleep(1.0)
common.focus_window()
common.wait_for(lambda: node("Live world"), 20.0, "the live world's toolbar")
press("Fill the window")
time.sleep(1.0)
canvas = common.wait_for(lambda: node("Live Minecraft world"), 10.0, "the canvas")
Atspi.Component.grab_focus(canvas)
time.sleep(0.5)
e = canvas.get_extents(Atspi.CoordType.WINDOW)
result = {"mode": mode, "pane_logical": [e.width, e.height], "load_before": load()}


def keydown(k):
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "keydown", k], check=True)
    else:
        subprocess.run(["wtype", "-P", k.lower()], check=True)


def keyup(k):
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "keyup", k], check=True)
    else:
        subprocess.run(["wtype", "-p", k.lower()], check=True)


if mode == "orbit":
    keydown("End")
    time.sleep(3.0)
    keyup("End")
    time.sleep(1.0)
elif mode == "drag":
    x, y = e.x + e.width // 2, e.y + e.height // 2
    common.pointer("mousemove", str(x), str(y))
    common.pointer("mousedown", "3")
    start = time.time()
    i = 0
    while time.time() - start < 3.0:
        i += 1
        common.pointer("mousemove", str(x + (i % 200) - 100), str(y))
        time.sleep(0.008)
    common.pointer("mouseup", "3")
    time.sleep(1.0)
elif mode == "key":
    for _ in range(20):
        keydown("w")
        time.sleep(0.05)
        keyup("w")
        time.sleep(0.3)
elif mode in ("run", "idle"):
    if mode == "idle":
        press("Pause")
        # Settled: the pane says paused, and a second more.
        common.wait_for(lambda: node("Live Minecraft world").get_description().endswith("paused") or None,
                        10.0, "the pane paused")
        time.sleep(1.0)
    else:
        press("Pause")
        press("Play")
    c0, t0 = cpu(), time.time()
    time.sleep(60.0)
    c1, t1 = cpu(), time.time()
    press("Play" if mode == "idle" else "Pause")
    ended = time.time()
    logged = frames_logged()
    result["frames_in_60s"] = logged[-1] - logged[-2]
    # The frames painted in the 60 s, by when: one painted within a tick of
    # the Pause is the running world's last tick, already drawn when the
    # Pause was sent; the idle pane is what follows it.
    live = [a for a in common.actions() if a.get("action") == "live" and "costs" in a]
    at_pause, at_end = live[-2]["costs"]["recent"], live[-1]["costs"]["recent"]
    last = at_pause[-1]
    same = [r for r in at_end if r["seq"] == last["seq"]]
    # The press was this long before the end: that frame's age now less its
    # age at the press.
    press = same[0]["ms_ago"] - last["ms_ago"] if same else None
    painted = [r for r in at_end if r["seq"] > last["seq"]]
    result["painted_ms_after_pause"] = [press - r["ms_ago"] for r in painted] if press is not None else None
    result["frames_after_the_first_100_ms"] = (
        len([r for r in painted if press - r["ms_ago"] > 100]) if press is not None else len(painted)
    )
    result["cpu_percent"] = round((c1 - c0) / (t1 - t0) * 100, 1)
    if mode == "idle":
        # The frames of the counted 60 s: painted less than (end - start)
        # before the costs were logged at the closing press.
        window_ms = (ended - t0) * 1000
        settled = [r for r in at_end if r["ms_ago"] < window_ms]
        result["painted_in_the_settled_60s"] = len(settled)
        report.check("a settled idle pane paints nothing for 60 s", not settled,
                     f'{len(settled)} painted in the 60 s from 1 s after paused; all after the press: {result["painted_ms_after_pause"]} ms')
        late = [ms for ms in (result["painted_ms_after_pause"] or []) if ms > 500]
        report.check("nothing paints later than 500 ms after the Pause", not late and result["painted_ms_after_pause"] is not None,
                     f'{result["painted_ms_after_pause"]} ms after the Pause')
    else:
        report.check("a running world paints its ticks (about 20 a second)", result["frames_in_60s"] > 600, str(result["frames_in_60s"]))
if mode in ("orbit", "drag", "key"):
    press("Pause")
result["load_after"] = load()
live = [a for a in common.actions() if a.get("action") == "live" and "costs" in a]
result["window"] = live[-1]["costs"] if live else None
report.check("the window logged its costs", bool(live))
common.quit_key(settle=3.0)
with open(common.out("perf.json"), "w") as f:
    json.dump(result, f, indent=1)
print(json.dumps(result), flush=True)
report.finish()
