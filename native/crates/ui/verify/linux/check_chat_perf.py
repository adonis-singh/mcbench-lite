"""The chat's numbers (run by run.sh; scenario as the argument):

    check_chat_perf.py stream-typing    (--fixture long --rate-ms 17 --answer <a long text>)
    check_chat_perf.py scroll-pictures  (--fixture long-pictures)
    check_chat_perf.py idle-after-turn  (--fixture long --rate-ms 17)

stream-typing: in the 400-message chat a turn streams at 60 words a second
(one word each 17 ms); meanwhile 40 characters are typed into the composer,
150 ms apart: the key-to-paint of those keys and the frame intervals while
the turn streams. scroll-pictures: 240 wheel clicks up and 240 down over
the 400-message chat whose answers hold 20 pictures of their own files:
frame intervals while scrolling and the resident memory after. idle-after-
turn: a turn streams to its end, then 12 s for the caret to stop blinking,
then 20 s in which a settled chat draws no frame (the working line's clock
stops with the turn). Each writes chatperf-<scenario>.json with the load."""

import json
import os
import sys
import time

import common

scenario = sys.argv[1]
pid = int(os.environ["MCBENCH_APP_PID"])
ticks = os.sysconf("SC_CLK_TCK")


def app_ms():
    start = int(open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()[19]) / ticks
    return (float(open("/proc/uptime").read().split()[0]) - start) * 1000


def rss_kb():
    return int(next(l for l in open(f"/proc/{pid}/status") if l.startswith("VmRSS:")).split()[1])


def busy():
    region = common.one(common.app(), "landmark", "Conversation")
    return region.get_state_set().contains(common.Atspi.StateType.BUSY)


def stats(values):
    values = sorted(values)
    if not values:
        return None
    return {
        "count": len(values),
        "p50": round(values[len(values) // 2], 3),
        "p95": round(values[int((len(values) - 1) * 0.95)], 3),
        "p99": round(values[int((len(values) - 1) * 0.99)], 3),
        "worst": round(values[-1], 3),
    }


common.app()
time.sleep(3.0)
common.focus_window()
time.sleep(1.0)
result = {"scenario": scenario, "backend": common.BACKEND, "uptime_before": open("/proc/loadavg").read().split()[:3]}
window = {}
typed = []
if scenario in ("stream-typing", "idle-after-turn"):
    common.type_text("go")
    common.key("Return")
    common.wait_for(busy, 10, "the turn to start")
    window["from_ms"] = app_ms()
    if scenario == "stream-typing":
        for ch in "the quick brown fox jumps over the lamp":
            typed.append(app_ms())
            common.type_text(ch, settle=0.15)
        result["still_streaming_after_typing"] = busy()
    common.wait_for(lambda: not busy(), 120, "the turn to end")
    window["to_ms"] = app_ms()
    if scenario == "idle-after-turn":
        time.sleep(12.0)
        window = {"from_ms": app_ms()}
        time.sleep(20.0)
        window["to_ms"] = app_ms()
elif scenario == "scroll-pictures":
    common.pointer("mousemove", "400", "400")
    rss_start = rss_kb()
    window["from_ms"] = app_ms()
    for button in ("4", "5"):
        for _ in range(8):
            common.pointer("click", "--repeat", "30", "--delay", "25", button)
    window["to_ms"] = app_ms()
    time.sleep(1.0)
    result["rss_kb_before_scrolling"] = rss_start
    result["rss_kb_after_scrolling"] = rss_kb()
else:
    raise SystemExit(f"no scenario {scenario}")
result["uptime_after"] = open("/proc/loadavg").read().split()[:3]
result["rss_kb_at_end"] = rss_kb()
common.quit_key(settle=2.0)
metrics = [a for a in common.actions() if a["action"] == "metrics"]
assert metrics, "the app logged no metrics at quit"
m = metrics[-1]
inside = [t for t in m["frame_times_ms"] if window["from_ms"] <= t <= window["to_ms"]]
result["window_s"] = round((window["to_ms"] - window["from_ms"]) / 1000, 2)
result["frames_in_window"] = len(inside)
result["frame_interval_ms"] = stats([b - a for a, b in zip(inside, inside[1:])])
result["frame_work_ms_whole_run"] = m["frame_work_ms"]
result["key_to_paint_ms_whole_run"] = m["key_to_paint_ms"] if scenario == "stream-typing" else None
result["keys_typed_while_streaming"] = len(typed)
with open(common.out(f"chatperf-{scenario}.json"), "w") as f:
    json.dump(result, f, indent=1)
print(json.dumps(result))
