"""A fly-to in player mode on the real app (pane agent; real_pane.sh with
MCBENCH_REAL_PLACE=/bench/<id> and MCBENCH_REAL_DRIVER=real_bench_fly.py),
X11: the comparison shown, player mode entered and the pointer taken, the
eye read from a `player input` line (a pixel's turn there and back sends
two); a member framed by its row's action, which in player mode respawns
the player before that member's build (`teleportTo`); the `teleport` line
and the eye after it read, and a picture before and after. Writes
bench_fly.json."""

import json
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-bench-fly")
PANE = "Live Minecraft world"
PLAYER = "Player mode: walk and fly as a Minecraft player"
seen = {}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def inputs():
    return [a for a in common.actions() if a.get("action") == "player input"]


def nudge():
    n = len(inputs())
    subprocess.run(["xdotool", "mousemove_relative", "--", "1", "0"], check=True)
    time.sleep(0.15)
    subprocess.run(["xdotool", "mousemove_relative", "--", "-1", "0"], check=True)
    common.wait_for(lambda: len(inputs()) >= n + 2 or None, 5.0, "the nudge's inputs")
    return inputs()[-1]


app()
common.focus_window()
pane = common.wait_for(lambda: node(PANE), 120.0, "the pane")
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
time.sleep(2.0)
Atspi.Action.do_action(common.wait_for(lambda: node(PLAYER), 15.0, PLAYER), 0)
common.wait_for(lambda: [n for n in common.find(app(), name="Click to look around")], 30.0, "standing")
e = pane.get_extents(Atspi.CoordType.WINDOW)
common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
common.pointer("click", "1")
time.sleep(1.0)
before = nudge()
seen["before"] = {"eye": before.get("eye"), "yaw": before["yaw"], "pitch": before["pitch"]}
common.screenshot("fly-1-before.png")
# Escape gives the pointer back (player mode stays); a row's action frames
# its member.
common.key("Escape", settle=0.5)
rows = [n for n in common.find(app(), role="list item")
        if (n.get_name() or "")[:8] in ("Build A,", "Build B,", "Build C,", "Build D,")]
seen["rows"] = [r.get_name() for r in rows]
n = len(common.actions())
Atspi.Action.do_action(rows[1], 0)
time.sleep(3.0)
lines = [a for a in common.actions()[n:] if a.get("action") in ("frame member", "teleport")]
seen["lines"] = lines
common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
common.pointer("click", "1")
time.sleep(1.0)
after = nudge()
seen["after"] = {"eye": after.get("eye"), "yaw": after["yaw"], "pitch": after["pitch"]}
seen["member_bounds"] = [a for a in common.actions() if a.get("action") == "live state"][-1:]
common.screenshot("fly-2-after.png")
teleport = next((a for a in lines if a["action"] == "teleport"), None)
report.check("the row's action in player mode is a teleport", teleport is not None, json.dumps(lines))
if teleport:
    eye = after.get("eye") or [0, 0, 0]
    feet = teleport["at"]
    report.check("the player stands at the teleport's feet (eye 1.62 above)",
                 abs(eye[0] - feet[0]) < 0.5 and abs(eye[2] - feet[2]) < 0.5 and abs(eye[1] - feet[1] - 1.62) < 0.5,
                 json.dumps({"eye": eye, "feet": feet}))
with open(common.out("bench_fly.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
