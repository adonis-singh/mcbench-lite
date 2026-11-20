"""The 400-message chat (run with --fixture long): the accessibility tree's
size, Up and Down over all 400 messages with focus right at each step, and
scrolling with a screen reader's event listener attached.

    check_long.py

1. Size: the nodes AT-SPI shows in the app, and the nodes in the app's own
   tree dump (--a11y-dump, the tree AccessKit holds), at the start.
2. The walk: focus the transcript, Home, then Down 399 times and Up 399
   times; after each key the focused object is the article at that position
   (`posinset`, counted from 1), of 400, with the right author.
   At the last message, the articles kept outside the transcript are not
   SHOWING and those in it are (chat-fix, W2: the transcript clips its rows).
3. Scrolling: an AT-SPI listener for the object events a screen reader
   follows is registered, the
   wheel scrolls the transcript for about 6 s (down, then up), and the app's
   frames in that time, its CPU share, the CPU per frame and the events
   received are recorded.
Results go to long.json beside the report."""

import json
import os
import threading
import time

import common

Atspi = common.Atspi
report = common.Report("long")
pid = int(os.environ["MCBENCH_APP_PID"])
ticks = os.sysconf("SC_CLK_TCK")
result = {"backend": common.BACKEND}


def cpu_seconds():
    fields = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / ticks


def app_ms():
    start = int(open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[19]) / ticks
    return (float(open("/proc/uptime").read().split()[0]) - start) * 1000


def count(obj):
    total = 1
    for i in range(obj.get_child_count()):
        child = obj.get_child_at_index(i)
        if child is not None:
            total += count(child)
    return total


def dump_nodes():
    """The nodes in the app's tree dump: every object in it with a role."""
    def walk(value):
        if isinstance(value, dict):
            return ("role" in value) + sum(walk(v) for v in value.values())
        if isinstance(value, list):
            return sum(walk(v) for v in value)
        return 0

    with open(common.out("a11y.json")) as f:
        return walk(json.load(f))


app = common.app()
time.sleep(3.0)
common.focus_window()
time.sleep(1.0)
result["atspi_nodes_at_start"] = count(common.app())
common.wait_for(lambda: os.path.exists(common.out("a11y.json")), what="the tree dump")
result["dump_nodes_at_start"] = dump_nodes()
report.check("the tree holds the laid-out rows only", result["atspi_nodes_at_start"] < 600, str(result["atspi_nodes_at_start"]))


def posinset(obj):
    attributes = obj.get_attributes() or {}
    return int(attributes.get("posinset", "0")), int(attributes.get("setsize", "0"))


def author(index):
    # The long fixture alternates: even positions (from 0) are the user's.
    return "You" if index % 2 == 0 else "claude-opus-5-5"


# 2. The walk.
common.key("shift+Tab", settle=0.5)
common.key("Home", settle=0.8)
wrong = []
times = []


def expect(index):
    started = time.monotonic()

    def right():
        f = common.focused(common.app())
        return f is not None and f.get_role_name() == "article" and posinset(f) == (index + 1, 400)

    try:
        common.wait_for(right, timeout=5.0, what=f"message {index + 1} focused")
    except Exception:
        f = common.focused(common.app())
        wrong.append((index + 1, common.label(f), posinset(f) if f is not None else None))
        return
    times.append(time.monotonic() - started)
    f = common.focused(common.app())
    if not f.get_name().startswith(author(index)):
        wrong.append((index + 1, f.get_name(), "author"))


expect(0)
for index in range(1, 400):
    common.key("Down", settle=0.0)
    expect(index)
# chat-fix (W2): at the end, the rows kept outside the transcript (its
# neighbours, for a reader's next move) are not showing; the rows in it are.
time.sleep(0.5)
region = common.one(common.app(), "landmark", "Conversation")
box = Atspi.Component.get_extents(region, Atspi.CoordType.WINDOW)
outside, inside = [], []
for article in common.find(region, "article"):
    e = Atspi.Component.get_extents(article, Atspi.CoordType.WINDOW)
    showing = article.get_state_set().contains(Atspi.StateType.SHOWING)
    out = e.y + e.height <= box.y or e.y >= box.y + box.height
    (outside if out else inside).append((posinset(article)[0], showing))
report.check("rows scrolled out of the transcript are not showing",
             outside and not any(s for _, s in outside), repr(outside))
report.check("rows in the transcript are showing", inside and all(s for _, s in inside), repr(inside))
result["clipped_rows"] = {"outside": outside, "inside": inside}
for index in range(398, -1, -1):
    common.key("Up", settle=0.0)
    expect(index)
report.check("Down and Up: focus on the right message at every one of 798 steps", not wrong, str(wrong[:5]))
times.sort()
result["walk"] = {
    "steps": 798,
    "wrong": len(wrong),
    "key_to_focus_seen_ms": {
        "p50": round(times[len(times) // 2] * 1000, 1) if times else None,
        "p99": round(times[int((len(times) - 1) * 0.99)] * 1000, 1) if times else None,
        "worst": round(times[-1] * 1000, 1) if times else None,
    },
}
result["atspi_nodes_after_walk"] = count(common.app())
result["dump_nodes_after_walk"] = dump_nodes()

# 3. Scrolling with a listener.
events = {"count": 0}


def on_event(event):
    events["count"] += 1


KINDS = ["object:children-changed", "object:state-changed", "object:property-change",
         "object:bounds-changed", "object:text-changed", "object:text-caret-moved",
         "object:active-descendant-changed", "object:visible-data-changed"]
listener = Atspi.EventListener.new(on_event)
for kind in KINDS:
    listener.register(kind)
# The registration reaches the app through the registry: let it settle.
from gi.repository import GLib  # noqa: E402

settle = time.monotonic() + 1.0
while time.monotonic() < settle:
    GLib.MainContext.default().iteration(False)
    time.sleep(0.01)
common.pointer("mousemove", "700", "400")
done = threading.Event()


def wheel():
    common.pointer("click", "--repeat", "120", "--delay", "25", "5")
    common.pointer("click", "--repeat", "120", "--delay", "25", "4")
    done.set()


window_from = app_ms()
cpu0 = cpu_seconds()
thread = threading.Thread(target=wheel)
thread.start()
context = GLib.MainContext.default()
while not done.is_set():
    context.iteration(False)
    time.sleep(0.005)
thread.join()
window_to = app_ms()
cpu = cpu_seconds() - cpu0
deadline = time.monotonic() + 1.0
while time.monotonic() < deadline:
    context.iteration(False)
    time.sleep(0.005)
for kind in KINDS:
    listener.deregister(kind)

common.quit_key(settle=2.0)
metrics = [a for a in common.actions() if a["action"] == "metrics"]
report.check("the app logged its metrics at quit", bool(metrics))
frames = [t for t in metrics[-1]["frame_times_ms"] if window_from <= t <= window_to] if metrics else []
seconds = (window_to - window_from) / 1000
result["scroll_with_listener"] = {
    "window_s": round(seconds, 2),
    "frames": len(frames),
    "cpu_percent": round(cpu / seconds * 100, 1),
    "cpu_ms_per_frame": round(cpu * 1000 / len(frames), 2) if frames else None,
    "atspi_events_received": events["count"],
    "frame_work_ms": metrics[-1]["frame_work_ms"] if metrics else None,
}
report.check("frames were drawn while scrolling", len(frames) > 50, str(len(frames)))
result["load"] = open("/proc/loadavg").read().split()[:3]
with open(common.out("long.json"), "w") as f:
    json.dump(result, f, indent=1)
print(json.dumps(result))
report.finish()
