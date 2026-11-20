"""Bench's and the library's speed (bench agent): the app's own key-to-paint
times (the `metrics` line of the action log at quit) for one kind of key
press a run, so each run's numbers are that action's alone. Focus moves by
AT-SPI (no key), then the timed key.

    check_bench_perf.py compare     Return on a bench's row: its comparison
                                    opens (eight models, three rounds, and
                                    a two-round bench), 20 times
    check_bench_perf.py rounds      Return on the round switch's buttons, 20
    check_bench_perf.py leaderboard Return on the Leaderboard row and on a
                                    bench's row, 10 each
    check_bench_perf.py palette     `/` and ten letters in the composer, five
                                    times (run with --prompts 200)
    check_bench_perf.py editor      40 characters typed into a prompt's body
                                    (run with --prompts 1 --prompt-chars 20000)

FIXTURE=bench; MCBENCH_DUMP=off."""

import json
import sys
import time

import common
from common import Atspi

mode = sys.argv[1]
report = common.Report("bench-perf-" + mode)
app = common.app()
time.sleep(3.0)
common.focus_window()


def focus(obj, settle=0.4):
    Atspi.Component.grab_focus(obj)
    time.sleep(settle)


def row(title):
    return common.find(app, role="list item", pred=lambda o: (o.get_name() or "") == title
                       or (o.get_name() or "").startswith(title + ","))[0]


if mode == "compare":
    for _ in range(10):
        for title in ("Castle with a moat", "Village houses"):
            focus(row(title))
            common.key("Return", settle=1.0)
elif mode == "rounds":
    for _ in range(10):
        for n in (1, 2):
            focus(common.find(app, name=f"Round {n}", pred=lambda o: o.get_role_name() == "toggle button")[0])
            common.key("Return", settle=0.6)
elif mode == "leaderboard":
    for _ in range(10):
        focus(common.find(app, role="button", name="Leaderboard")[0])
        common.key("Return", settle=1.0)
        focus(row("Castle with a moat"))
        common.key("Return", settle=1.0)
elif mode == "palette":
    box = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))[0]
    for _ in range(5):
        focus(box)
        common.key("slash", settle=0.6)
        for ch in "generated7":
            common.type_text(ch, settle=0.25)
        common.key("Escape", settle=0.5)
        common.key("ctrl+a", settle=0.2)
        common.key("BackSpace", settle=0.4)
elif mode == "editor":
    first = common.find(app, role="list item", pred=lambda o: (o.get_name() or "").startswith("Generated prompt 0"))[0]
    focus(first)
    common.key("Return", settle=2.0)
    body = common.find(app, name="Prompt text", pred=lambda o: o.get_role_name() in ("text", "entry"))[0]
    report.check("the prompt is 20,000 characters or more", len(Atspi.Text.get_text(body, 0, -1)) >= 20000, "")
    focus(body)
    common.key("End", settle=0.5)
    for ch in "and a bridge over the river to the east":
        common.type_text(ch, settle=0.15)
common.quit_key(settle=3.0)
# par-bench: the line comes as the app ends, later on a debug build with a
# 20,000-character prompt (seen on Wayland): waited for up to 15 s more.
metrics = []
for _ in range(30):
    metrics = [a for a in common.actions() if a.get("action") == "metrics"]
    if metrics:
        break
    time.sleep(0.5)
report.check("the app wrote its metrics", bool(metrics), "")
if metrics:
    m = metrics[-1]
    summary = {k: m.get(k) for k in ("first_frame_ms", "key_to_paint_ms", "frame_work_ms", "frame_interval_ms")}
    with open(common.out(f"bench-perf-{mode}.json"), "w") as f:
        json.dump(summary, f, indent=1)
    keys = m.get("key_to_paint_ms") or {}
    report.check("keys were timed", (keys.get("count") or 0) > 0, json.dumps(keys))
report.finish()
