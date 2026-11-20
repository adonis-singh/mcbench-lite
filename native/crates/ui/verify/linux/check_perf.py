"""First numbers: idle, key-to-paint, scrolling and the first frame.

    check_perf.py idle|idle-unfocused|idle-after-input|idle-inactive|blink|typing|scroll

idle: the window open and nothing changing for 20 s, the composer focused
(its caret blinks): frames drawn in that time and the process's CPU share
(from /proc). idle-unfocused: the same with focus on a message (no caret).
idle-after-input: the composer focused, one key, then 12 s for the caret
to stop blinking (10 s), then the 20 s window. idle-inactive: the composer
focused, one key, then the window loses the keyboard to no window (X11) or
to a second client (Wayland, foot), then the 20 s window. blink: the 8 s after a key,
while the caret blinks: frames, CPU and the work of each frame. resize (with --fixture long): 60 window sizes 33 ms apart.
typing: 60 characters typed
into the composer, 120 ms apart. scroll (run with --fixture long): 6 s of
mouse-wheel scrolling over the transcript. launch-scroll (with --fixture
long): the open item (e) of ACCESSIBILITY.md, the lag after launch: as soon
as an AT-SPI client has found the app, 120 wheel clicks 25 ms apart, each
sent and timed on the app's clock, then 4 s for the app to catch up; the
app's own record says when each wheel event reached it and when frames
were drawn, so the queue is the gap between the two. Each run quits with Ctrl+Q; the
app's metrics (first frame, every frame's time, key-to-paint) come from its
action log, and perf-<scenario>.json holds the result."""

import json
import os
import sys
import time

import common

scenario = sys.argv[1]
pid = int(os.environ["MCBENCH_APP_PID"])
ticks = os.sysconf("SC_CLK_TCK")


def cpu_seconds():
    fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / ticks


def app_ms():
    """Milliseconds since the app's process started, on its own clock."""
    start = int(open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[19]) / ticks
    return (float(open("/proc/uptime").read().split()[0]) - start) * 1000


# With MCBENCH_A11Y=off the session never enables accessibility and the
# check does not touch AT-SPI, so the app builds no tree.
if os.environ.get("MCBENCH_A11Y") != "off":
    common.app()
window = {}
sent = []
if scenario == "launch-scroll":
    # Right after launch: no settling. The window takes the keyboard first
    # (X11 has no window manager); the pointer goes over the transcript.
    common.focus_window()
    common.pointer("mousemove", "700", "400")
    window["from_ms"] = app_ms()
    cpu0 = cpu_seconds()
    for button in ["4"] * 60 + ["5"] * 60:
        sent.append(round(app_ms(), 3))
        common.pointer("click", button)
        time.sleep(0.025)
    time.sleep(4.0)
    window["cpu_percent"] = round((cpu_seconds() - cpu0) / ((app_ms() - window["from_ms"]) / 1000) * 100, 1)
    window["to_ms"] = app_ms()
else:
    time.sleep(3.0)
    common.focus_window()
    time.sleep(2.0)
if scenario == "idle-unfocused":
    common.key("shift+Tab", settle=1.0)
other = None
if scenario == "idle-inactive":
    # The composer keeps focus; the window loses it.
    common.type_text("x", settle=0.6)
    other = common.deactivate_window()
if scenario == "idle-after-input":
    common.type_text("x", settle=12.0)
if scenario == "launch-scroll":
    pass
elif scenario == "blink":
    common.type_text("x", settle=0.6)
    window["from_ms"] = app_ms()
    cpu0 = cpu_seconds()
    time.sleep(8.0)
    window["cpu_percent"] = round((cpu_seconds() - cpu0) / 8.0 * 100, 3)
    window["to_ms"] = app_ms()
elif scenario in ("idle", "idle-unfocused", "idle-after-input", "idle-inactive"):
    window["from_ms"] = app_ms()
    cpu0 = cpu_seconds()
    time.sleep(20.0)
    window["cpu_percent"] = round((cpu_seconds() - cpu0) / 20.0 * 100, 3)
    window["to_ms"] = app_ms()
elif scenario == "typing":
    window["from_ms"] = app_ms()
    for ch in "the quick brown fox jumps over the lazy dog, then a third lamp":
        common.type_text(ch, settle=0.12)
    window["to_ms"] = app_ms()
elif scenario == "scroll":
    common.pointer("mousemove", "700", "400")
    window["from_ms"] = app_ms()
    cpu0 = cpu_seconds()
    common.pointer("click", "--repeat", "120", "--delay", "25", "4")
    common.pointer("click", "--repeat", "120", "--delay", "25", "5")
    window["cpu_percent"] = round((cpu_seconds() - cpu0) / ((app_ms() - window["from_ms"]) / 1000) * 100, 1)
    window["to_ms"] = app_ms()
elif scenario == "resize":
    # A live resize: the window's size changes 60 times, 33 ms apart, as a
    # drag of its corner does (X11: the window itself, no window manager;
    # Wayland: cage's one output, which its client fills).
    import subprocess
    window["from_ms"] = app_ms()
    cpu0 = cpu_seconds()
    for i in range(60):
        w, h = 1280 - (i % 30) * 12, 820 - (i % 30) * 6
        if common.BACKEND == "x11":
            subprocess.run(["xdotool", "search", "--name", "mcbench-lite$", "windowsize", str(w), str(h)], check=True)
        else:
            subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "wl_pointer.py"),
                            "output", f"{w}x{h}"], check=True)
        time.sleep(0.033)
    window["cpu_percent"] = round((cpu_seconds() - cpu0) / ((app_ms() - window["from_ms"]) / 1000) * 100, 1)
    window["to_ms"] = app_ms()
elif scenario != "launch-scroll":
    raise SystemExit(f"no scenario {scenario}")
time.sleep(0.5)
rss_kb = int(next(l for l in open(f"/proc/{pid}/status") if l.startswith("VmRSS:")).split()[1])
if scenario == "idle-inactive":
    if other:
        other.terminate()
        other.wait()
    common.focus_window()
common.quit_key(settle=2.0)
metrics = [a for a in common.actions() if a["action"] == "metrics"]
assert metrics, "the app logged no metrics at quit"
m = metrics[-1]
inside = [t for t in m["frame_times_ms"] if window["from_ms"] <= t <= window["to_ms"]]
gaps = sorted(b - a for a, b in zip(inside, inside[1:]))
result = {
    "scenario": scenario,
    "backend": common.BACKEND,
    "first_frame_ms": m["first_frame_ms"],
    "window_s": round((window["to_ms"] - window["from_ms"]) / 1000, 2),
    "frames_in_window": len(inside),
    "cpu_percent": window.get("cpu_percent"),
    "key_to_paint_ms": m["key_to_paint_ms"] if scenario == "typing" else None,
    "frame_work_ms": m["frame_work_ms"],
    "frame_interval_ms_in_window": {
        "count": len(gaps),
        "mean": round(sum(gaps) / len(gaps), 3) if gaps else None,
        "p50": gaps[len(gaps) // 2] if gaps else None,
        "p99": gaps[int((len(gaps) - 1) * 0.99)] if gaps else None,
        "worst": gaps[-1] if gaps else None,
    },
    "accessibility": os.environ.get("MCBENCH_A11Y") != "off",
    "scroll_to_paint_ms": m.get("scroll_to_paint_ms"),
    "rss_kb_at_end": rss_kb,
    "load": open("/proc/loadavg").read().split()[:3],
}
if scenario == "launch-scroll":
    arrived = m.get("scroll_times_ms") or []
    # Each click is a press and a release; the app sees one wheel event per
    # click. The queue: each event's arrival less its click's sending.
    paired = list(zip(sent, arrived))
    delays = sorted(round(a - s, 1) for s, a in paired)
    result["wheel_events"] = {"sent": len(sent), "arrived": len(arrived)}
    result["wheel_queue_ms"] = {
        "p50": delays[len(delays) // 2] if delays else None,
        "worst": delays[-1] if delays else None,
        "by_click": [round(a - s, 1) for s, a in paired],
    }
    caught_up = arrived[-1] if arrived else None
    result["last_wheel_after_sending_ms"] = round(caught_up - sent[-1], 1) if arrived and sent else None
    result["frame_times_ms"] = inside
with open(common.out(f"perf-{scenario}.json"), "w") as f:
    json.dump(result, f, indent=1)
print(json.dumps(result))
