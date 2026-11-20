"""The sidebar's numbers, on the `many` fixture (500 chats, 5,000 messages):

    check_sidebar_perf.py scroll|palette|idle-pulse|pulse

scroll: the first frame, then 120 wheel clicks down and 120 up over the
sidebar's list, 25 ms apart: frame intervals and wheel-to-paint in that
time. palette: Ctrl+K, then 40 characters typed into its field 120 ms
apart (each a search over the stand-in's 5,000 messages, 90 ms after the
last key): key-to-paint. idle-pulse (run with --simulate-ms 40000): 12 s
for the caret to stop blinking, then 20 s with nothing running (frames in
that window), then from 42 s, with one chat running (the stand-in starts it
at 40 s), 20 s more: the pulse's frames. pulse (any fixture, run with
`--running <chat>`): after 3 s, 20 s of the pulse alone; with `--open
/new` or not, it compares a step's work without and with a transcript
on screen. a11y-pulse (chat-fix; run with `--running chat-gate --join
chat-gate --rate-ms 600000 --open /chat/chat-gate`, so the row's dot pulses
and the working line's clock ticks while its turn waits on its first word):
an AT-SPI client attached (listening to every object event), 20 s: the
events it receives per second by kind, and the interface (main) thread's
CPU time per frame drawn. Each run quits with Ctrl+Q; the
app's metrics come from its action log; sidebar-perf-<scenario>.json holds
the result."""

import json
import os
import sys
import time

import common

scenario = sys.argv[1]
pid = int(os.environ["MCBENCH_APP_PID"])
ticks = os.sysconf("SC_CLK_TCK")
common.X_KEYS.update({"ctrl+k": "ctrl+k"})


def cpu_seconds():
    fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / ticks


def main_thread_seconds():
    """The interface thread's CPU time: the main thread's (its task id is
    the process id)."""
    fields = open(f"/proc/{pid}/task/{pid}/stat").read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / ticks


def app_ms():
    start = int(open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[19]) / ticks
    return (float(open("/proc/uptime").read().split()[0]) - start) * 1000


def stats(values):
    values = sorted(values)
    if not values:
        return None
    return {
        "count": len(values),
        "mean": round(sum(values) / len(values), 3),
        "p50": values[len(values) // 2],
        "p99": values[int((len(values) - 1) * 0.99)],
        "worst": values[-1],
    }


common.app()
windows = {}
time.sleep(3.0)
common.focus_window()
time.sleep(1.0)
if scenario == "scroll":
    common.pointer("mousemove", "120", "400")
    start = app_ms()
    cpu0 = cpu_seconds()
    common.pointer("click", "--repeat", "120", "--delay", "25", "5")
    common.pointer("click", "--repeat", "120", "--delay", "25", "4")
    windows["scroll"] = (start, app_ms(), round((cpu_seconds() - cpu0) / ((app_ms() - start) / 1000) * 100, 1))
elif scenario == "palette":
    common.key("ctrl+k", settle=0.8)
    start = app_ms()
    for ch in "redstone lamps for build four hundred and":
        common.type_text(ch, settle=0.12)
    time.sleep(0.5)
    windows["typing"] = (start, app_ms(), None)
elif scenario == "idle-pulse":
    # The caret blinks for 10 s after the last focus change.
    time.sleep(12.0)
    start = app_ms()
    cpu0 = cpu_seconds()
    time.sleep(20.0)
    windows["idle"] = (start, app_ms(), round((cpu_seconds() - cpu0) / 20.0 * 100, 3))
    # The stand-in starts a chat's turn 40 s after launch.
    while app_ms() < 42000:
        time.sleep(0.2)
    start = app_ms()
    cpu0 = cpu_seconds()
    time.sleep(20.0)
    windows["pulse"] = (start, app_ms(), round((cpu_seconds() - cpu0) / 20.0 * 100, 3))
elif scenario == "pulse":
    start = app_ms()
    cpu0 = cpu_seconds()
    time.sleep(20.0)
    windows["pulse"] = (start, app_ms(), round((cpu_seconds() - cpu0) / 20.0 * 100, 3))
elif scenario == "a11y-pulse":
    from gi.repository import GLib

    Atspi = common.Atspi
    counts = {}

    def on_event(event):
        counts[event.type] = counts.get(event.type, 0) + 1

    kinds = ["object:children-changed", "object:state-changed", "object:property-change",
             "object:bounds-changed", "object:text-changed", "object:text-caret-moved",
             "object:active-descendant-changed", "object:visible-data-changed", "object:announcement"]
    listener = Atspi.EventListener.new(on_event)
    for kind in kinds:
        listener.register(kind)
    context = GLib.MainContext.default()
    settle = time.monotonic() + 2.0
    while time.monotonic() < settle:
        context.iteration(False)
        time.sleep(0.005)
    counts.clear()
    start = app_ms()
    cpu0 = cpu_seconds()
    main0 = main_thread_seconds()
    end = time.monotonic() + 20.0
    while time.monotonic() < end:
        context.iteration(False)
        time.sleep(0.005)
    windows["pulse"] = (start, app_ms(), round((cpu_seconds() - cpu0) / 20.0 * 100, 3))
    main_cpu = main_thread_seconds() - main0
    for kind in kinds:
        listener.deregister(kind)
else:
    raise SystemExit(f"no scenario {scenario}")
common.quit_key(settle=2.0)
metrics = [a for a in common.actions() if a["action"] == "metrics"]
assert metrics, "the app logged no metrics at quit"
m = metrics[-1]
result = {
    "scenario": scenario,
    "backend": common.BACKEND,
    "first_frame_ms": m["first_frame_ms"],
    "key_to_paint_ms": m["key_to_paint_ms"],
    "scroll_to_paint_ms": m.get("scroll_to_paint_ms"),
    "frame_work_ms": m["frame_work_ms"],
    "load": open("/proc/loadavg").read().split()[:3],
}
for name, (start, end, cpu) in windows.items():
    inside = [t for t in m["frame_times_ms"] if start <= t <= end]
    gaps = [round(b - a, 3) for a, b in zip(inside, inside[1:])]
    result[name] = {
        "window_s": round((end - start) / 1000, 2),
        "frames": len(inside),
        "cpu_percent": cpu,
        "frame_interval_ms": stats(gaps),
    }
if scenario == "a11y-pulse":
    frames = result["pulse"]["frames"]
    result["atspi_events_per_s"] = {k: round(v / 20.0, 2) for k, v in sorted(counts.items())}
    result["main_thread_cpu_s"] = round(main_cpu, 3)
    result["main_thread_ms_per_frame"] = round(main_cpu * 1000 / frames, 3) if frames else None
with open(common.out(f"sidebar-perf-{scenario}.json"), "w") as f:
    json.dump(result, f, indent=1)
print(json.dumps(result))
