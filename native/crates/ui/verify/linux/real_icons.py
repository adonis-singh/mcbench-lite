"""A real block icon (pane agent; run by real_pane.sh with
MCBENCH_REAL_DRIVER=real_icons.py): a selection dragged with the tool on
the chat's world, its chip in the composer with the core's icon and its
marker over the pane, pictured beside the chat and in Fill. Writes
icons.json."""

import json
import time

import common

Atspi = common.Atspi
report = common.Report("real-icons")
PANE = "Live Minecraft world"
seen = {}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def press(name):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 15.0, name), 0)
    time.sleep(0.6)


app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
c = common.wait_for(lambda: node(PANE), 30.0, "the pane")
e = c.get_extents(Atspi.CoordType.WINDOW)
cx, cy = e.x + e.width // 2, e.y + e.height // 2
press("Select blocks")
common.pointer("mousemove", str(cx - 20), str(cy + 20))
common.pointer("mousedown", "1")
for i in range(1, 10):
    common.pointer("mousemove", str(cx - 20 + i * 5), str(cy + 20 + i * 3))
    time.sleep(0.03)
common.pointer("mouseup", "1")
chip = common.wait_for(lambda: [n.get_name() for n in common.find(app()) if (n.get_name() or "").startswith("Remove ")],
                       15.0, "the chip")
seen["chip"] = chip
common.key("Escape", settle=0.4)
common.pointer("mousemove", "5", "5")
time.sleep(1.5)
common.screenshot("icons-beside.png")
press("Fill the window")
time.sleep(1.5)
common.screenshot("icons-fill.png")
seen["selection"] = [a for a in common.actions() if a.get("action") in ("selection",)][-1:]
report.check("a selection's chip is in the composer", bool(chip), str(chip))
with open(common.out("icons.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
