"""The real app quit with its world shown (pane agent; run by real_pane.sh
with MCBENCH_REAL_DRIVER=real_quit.py and MCBENCH_QUIT=key or close): the
first real frame waited for, then Ctrl+Q or the window's close request,
and the time from it to the process gone. Writes quit.json."""

import json
import os
import time

import common

report = common.Report("real-quit")
how = os.environ["MCBENCH_QUIT"]
pid = int(os.environ["MCBENCH_APP_PID"])


def running():
    try:
        with open(f"/proc/{pid}/stat") as stat:
            return stat.read().rsplit(")", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


app = common.app(60.0)
common.focus_window()
common.wait_for(lambda: common.find(common.app(), name="Live world"), 180.0, "the first real frame")
time.sleep(2.0)
started = time.monotonic()
if how == "key":
    common.key("ctrl+q", settle=0.0)
else:
    report.check("the window is asked to close", common.close_window() == 1)
while running() and time.monotonic() - started < 15.0:
    time.sleep(0.01)
took = time.monotonic() - started
report.check(f"the {how} ends the app", not running(), f"{took:.3f} s")
with open(common.out("quit.json"), "w") as f:
    json.dump({"how": how, "took_s": round(took, 3), "gone": not running()}, f)
report.finish()
