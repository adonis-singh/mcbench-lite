"""The selection tool and the camera rows not yet seen on the real
renderer (pane agent; run by real_pane.sh with
MCBENCH_REAL_DRIVER=real_select.py), X11: Alt+S, a drag, Escape; three
selections (A, B, C with the core's icons); a right-click on a draft box;
a chip hovered lighting its box; then in Fill: Flat, Spectator,
Perspective, the zoom buttons, Reset, and a double click on the world.
A picture after each. Writes select.json."""

import json
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-select")
PANE = "Live Minecraft world"
seen = {"steps": []}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def press(name):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 15.0, name), 0)
    time.sleep(0.8)


def chips():
    return [n.get_name()[len("Remove "):] for n in common.find(app()) if (n.get_name() or "").startswith("Remove ")]


def selecting():
    lines = [a for a in common.actions() if a.get("action") == "select"]
    return lines[-1].get("on") if lines else None


def step(name, **extra):
    seen["steps"].append({"step": name, "chips": chips(), "selecting": selecting(), **extra})
    common.screenshot(f"select-{len(seen['steps']):02}-{name}.png")


def centre():
    e = node(PANE).get_extents(Atspi.CoordType.WINDOW)
    return e.x + e.width // 2, e.y + e.height // 2


def drag(x0, y0, dx, dy):
    common.pointer("mousemove", str(x0), str(y0))
    common.pointer("mousedown", "1")
    for i in range(1, 9):
        common.pointer("mousemove", str(int(x0 + dx * i / 8)), str(int(y0 + dy * i / 8)))
        time.sleep(0.04)
    common.pointer("mouseup", "1")
    time.sleep(2.0)


app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
cx, cy = centre()
Atspi.Component.grab_focus(node(PANE))
time.sleep(0.3)
subprocess.run(["xdotool", "key", "alt+s"], check=True)
time.sleep(0.6)
step("alt-s")
drag(cx - 40, cy + 40, 30, 12)
step("drag-a")
common.key("Escape", settle=0.6)
step("escape")
# B and C start on the ground near the centre (farther out the pick
# finds no block from this height).
for letter, (x0, y0) in (("b", (cx + 20, cy + 10)), ("c", (cx - 60, cy + 5))):
    Atspi.Component.grab_focus(node(PANE))
    time.sleep(0.3)
    subprocess.run(["xdotool", "key", "alt+s"], check=True)
    time.sleep(0.6)
    drag(x0, y0, 24, 10)
    common.key("Escape", settle=0.6)
step("three")
# A right-click inside B's box (where its drag began).
common.pointer("mousemove", str(cx + 24), str(cy + 12))
time.sleep(0.3)
common.pointer("click", "3")
time.sleep(1.5)
step("right-click-b")
# A chip hovered: its box lit.
chip = [n for n in common.find(app()) if (n.get_description() or "").endswith(" blocks") and n.get_role_name() in ("panel", "grouping", "group")]
extents = None
if chip:
    e = chip[0].get_extents(Atspi.CoordType.WINDOW)
    extents = [e.x, e.y, e.width, e.height]
    # Into the chip from beside it: two moves, as a pointer arrives.
    common.pointer("mousemove", str(e.x + e.width // 2 - 20), str(e.y + e.height // 2))
    time.sleep(0.2)
    common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
    time.sleep(1.2)
step("chip-hovered", hovered=chip[0].get_name() if chip else None, extents=extents,
     lines=[a for a in common.actions() if a.get("action") == "chip hovered"][-2:])
common.pointer("mousemove", "5", "5")
press("Fill the window")
for name, label in (("flat", "Orthographic / Flat-View"), ("spectator", "Spectator: fly through everything"),
                    ("perspective", "Perspective-View")):
    press(label)
    time.sleep(1.2)
    step(name)
for _ in range(3):
    press("Zoom in")
time.sleep(1.0)
step("zoom-in")
for _ in range(3):
    press("Zoom out")
time.sleep(1.0)
step("zoom-out")
press("Reset Camera & Position")
time.sleep(1.5)
step("reset")
cx, cy = centre()
common.pointer("mousemove", str(cx), str(cy + 60))
time.sleep(0.2)
common.pointer("click", "--repeat", "2", "--delay", "80", "1")
time.sleep(1.5)
step("double-click", full=[a for a in common.actions() if a.get("action") == "fill"][-1:])
seen["log"] = [a for a in common.actions() if a.get("action") in ("select", "selection", "removed selection", "camera mode", "fill")][-14:]
report.check("three chips were in the composer", any(len(s["chips"]) >= 3 for s in seen["steps"]), json.dumps([s["chips"] for s in seen["steps"]]))
with open(common.out("select.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
