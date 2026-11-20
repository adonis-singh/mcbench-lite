"""The window's first_frame line (pane agent): the app started, its
window found, the line read from its log, then quit. Works for the dev
binary (run.sh) and the real app (real_pane.sh with
MCBENCH_REAL_DRIVER=check_first_frame.py). Writes first_frame.json."""

import json
import time

import common

report = common.Report("first-frame")
common.app(60.0)
line = common.wait_for(lambda: [a for a in common.actions() if a.get("action") == "first_frame"], 60.0,
                       "the first_frame line")
time.sleep(2.0)
lines = [a for a in common.actions() if a.get("action") == "first_frame"]
report.check("one first_frame line", len(lines) == 1, json.dumps(lines))
with open(common.out("first_frame.json"), "w") as f:
    json.dump(lines[0], f)
common.quit_key(settle=3.0)
report.finish()
