"""The world's bar at the pane's width (pane agent; real_pane.sh with
MCBENCH_REAL_DRIVER=real_bar.py and MCBENCH_REAL_SIZE), X11: a picture of
the pane's top, every world control with its place, and from the pane,
Tab after Tab, the controls the keyboard reaches. Writes bar.json."""

import json
import time

from PIL import Image

import common

Atspi = common.Atspi
report = common.Report("real-bar")
PANE = "Live Minecraft world"
WANTED = ["Day/Night", "Perspective-View", "Orthographic / Flat-View",
          "Player mode: walk and fly as a Minecraft player", "Spectator: fly through everything",
          "Reset Camera & Position", "Compass / Face North", "Select blocks", "Fill the window"]


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
time.sleep(1.0)
pane = node(PANE)
e = pane.get_extents(Atspi.CoordType.WINDOW)
seen = {"pane": [e.x, e.y, e.width, e.height], "controls": {}, "tab": []}
for name in WANTED:
    n = node(name)
    if n is not None:
        b = n.get_extents(Atspi.CoordType.WINDOW)
        seen["controls"][name] = [b.x, b.y, b.width, b.height]
path = common.screenshot("bar.png")
Image.open(path).crop((e.x, e.y, e.x + e.width + 4, e.y + 110)).save(common.out("bar-crop.png"))
Atspi.Component.grab_focus(pane)
time.sleep(0.3)
for _ in range(30):
    common.key("Tab", settle=0.15)
    f = common.focused(app())
    if f is not None:
        seen["tab"].append(f.get_name())
for name in WANTED:
    report.check(f"{name}: on the bar inside the pane", name in seen["controls"]
                 and e.x <= seen["controls"][name][0] < e.x + e.width, json.dumps(seen["controls"].get(name)))
    report.check(f"{name}: reached by Tab", name in seen["tab"], "")
with open(common.out("bar.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
