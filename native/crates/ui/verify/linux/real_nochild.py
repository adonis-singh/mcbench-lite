"""The real app with setup's preparation failing (pane agent; run by
real_pane.sh with MCBENCH_REAL_DRIVER=real_nochild.py, the build child
moved out of the binary's folder): after 30 s, the pane's state, every
status and alert's text, and a picture. Writes nochild.json."""

import json
import time

import common

Atspi = common.Atspi
report = common.Report("real-nochild")
PANE = "Live Minecraft world"
seen = {}
app = common.app(60.0)
common.focus_window()
time.sleep(30.0)
pane = common.find(app, name=PANE)
seen["pane"] = pane[0].get_description() if pane else None
seen["texts"] = sorted({(n.get_role_name(), n.get_name()) for n in common.find(app)
                        if n.get_name() and n.get_role_name() in ("status bar", "alert", "label", "notification")})
common.screenshot("nochild.png")
report.check("the pane says why", bool(seen["pane"]), str(seen["pane"]))
with open(common.out("nochild.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
