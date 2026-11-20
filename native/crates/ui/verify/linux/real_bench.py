"""A bench comparison on the real renderer (pane agent; run by
real_pane.sh with MCBENCH_REAL_PLACE=/bench/<id> and
MCBENCH_REAL_DRIVER=real_bench.py): the round's world with every member's
label, Build B framed by its row's action, Build C by the key 3 with
the pane focused, the whole round by the round's prompt button; a picture
after each. Writes bench.json."""

import json
import time

import common

Atspi = common.Atspi
report = common.Report("real-bench")
PANE = "Live Minecraft world"
seen = {}


def app():
    return common.app(60.0)


def find(pred):
    return [n for n in common.find(app(), pred=pred)]


app()
common.focus_window()
time.sleep(25.0)
pane = common.wait_for(lambda: find(lambda o: o.get_name() == PANE), 120.0, "the pane")[0]
seen["pane"] = pane.get_description()
seen["toolbar"] = bool(find(lambda o: o.get_name() == "Live world"))
seen["statuses"] = [n.get_name() for n in common.find(app(), role="status bar") if n.get_name()]
common.screenshot("bench-1-round.png")
# The round's result rows (`Build A, Done`), not the sidebar's benches.
rows = [n for n in common.find(app(), role="list item")
        if (n.get_name() or "")[:8] in ("Build A,", "Build B,", "Build C,", "Build D,")]
seen["rows"] = [r.get_name() for r in rows]
rounds = [n for n in common.find(app()) if (n.get_name() or "").startswith("Round 1:")]
seen["round_buttons"] = [r.get_name()[:80] for r in rounds]
if len(rows) > 1:
    Atspi.Action.do_action(rows[1], 0)
    time.sleep(2.5)
    common.screenshot("bench-2-member-row.png")
Atspi.Component.grab_focus(pane)
time.sleep(0.4)
import subprocess
subprocess.run(["xdotool", "key", "3"], check=True)
time.sleep(2.5)
common.screenshot("bench-3-member-digit.png")
if rounds:
    Atspi.Action.do_action(rounds[0], 0)
    time.sleep(2.5)
    common.screenshot("bench-4-whole-round.png")
seen["log"] = [a for a in common.actions() if a.get("action") in ("live show", "frame member", "frame round", "live")][-8:]
report.check("the comparison's world shows with its toolbar", seen["toolbar"], json.dumps(seen)[:400])
with open(common.out("bench.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
