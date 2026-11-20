"""Player mode on the real core, second walk (pane agent; real_pane.sh
with MCBENCH_REAL_DRIVER=real_player2.py), X11: off the build's pad to
the west for a fall out of the world (its note and the respawn), then
over the wall by flight to stand north of the lever (the stored world's
one lever, world (-2, 65, -1), on the wall's north face), turned to face
it, and a right-click (use); the gate pictured then, a second and five
seconds after. The camera's x and z fields follow the player. Writes
player2.json."""

import json
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-player2")
PANE = "Live Minecraft world"
PLAYER = "Player mode: walk and fly as a Minecraft player"
seen = {"steps": []}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def press(name):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 15.0, name), 0)
    time.sleep(0.8)


def field(axis):
    n = node(f"Camera position {axis}")
    try:
        return int(Atspi.Text.get_text(n, 0, -1))
    except (TypeError, ValueError):
        return None


def said():
    return [n.get_name() for n in common.find(app(), role="status bar") if n.get_name()]


def hud():
    return [n.get_name() for n in common.find(app()) if (n.get_name() or "").startswith(("Fell", "The pointer", "WASD", "Click to"))]


def hold(keys, seconds):
    subprocess.run(["xdotool", "keydown", *keys], check=True)
    time.sleep(seconds)
    subprocess.run(["xdotool", "keyup", *reversed(keys)], check=True)
    time.sleep(0.5)


def tap(key, times=1, gap=0.12):
    for _ in range(times):
        subprocess.run(["xdotool", "key", key], check=True)
        time.sleep(gap)


def turn(pixels):
    step = 100 if pixels > 0 else -100
    for _ in range(abs(pixels) // 100):
        subprocess.run(["xdotool", "mousemove_relative", "--", str(step), "0"], check=True)
        time.sleep(0.03)
    time.sleep(0.5)


def step(name):
    seen["steps"].append({"step": name, "x": field("x"), "z": field("z"), "hud": hud(), "said": said()[-2:]})
    common.screenshot(f"player2-{len(seen['steps']):02}-{name}.png")


import os
import sys

# What the X server sees of every key, beside the window's own log.
recorder = subprocess.Popen([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "xi_keys.py"),
                             common.out("xi-keys.txt")])
seen["monotonic_at_start"] = time.monotonic()
app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
press("Fill the window")
press(PLAYER)
common.wait_for(lambda: [n for n in common.find(app(), name="Click to look around")], 20.0, "standing")
c = node(PANE)
e = c.get_extents(Atspi.CoordType.WINDOW)
common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
time.sleep(0.2)
common.pointer("click", "1")
time.sleep(0.8)
step("stood")
# West off the pad: sprint with A until falling.
hold(["ctrl", "a"], 9.0)
step("off-west")
try:
    fell = common.wait_for(lambda: [h for h in hud() if h.startswith("Fell")] or None, 20.0, "the fall's note")
except AssertionError:
    fell = None
step("fell-note")
time.sleep(6.0)
step("respawned")
seen["fell"] = fell
# Over the wall north, by flight.
tap("space", 2)
hold(["space"], 2.0)
step("flying-up")
for _ in range(12):
    z = field("z")
    if z is not None and z <= -4:
        break
    hold(["w"], 0.4)
step("north-of-wall")
for _ in range(10):
    x = field("x")
    if x is None or x <= -2:
        break
    hold(["a"], 0.15)
for _ in range(10):
    x = field("x")
    if x is None or x >= -2:
        break
    hold(["d"], 0.15)
step("over-lever-column")
hold(["shift"], 3.0)
step("landed")
# Half round to face south, toward the wall's north face: 180 degrees at
# 0.15 a pixel.
turn(1200)
time.sleep(0.5)
step("facing-wall")
# The lever stands below the lantern left of the crosshair: about 12
# degrees left and 16 down from the walk's first look at it, at 0.15 a
# pixel of the locked pointer.
subprocess.run(["xdotool", "mousemove_relative", "--", "-81", "107"], check=True)
time.sleep(0.8)
step("aimed")
before = said()
common.pointer("click", "3")
time.sleep(1.0)
seen["use_said"] = [s for s in said() if s not in before]
step("used")
time.sleep(1.0)
step("one-second")
time.sleep(4.0)
step("five-seconds")
report.check("a right-click on the lever toggles it", any(s.startswith("Lever") for s in seen["use_said"]), str(seen["use_said"]))
# Focus lost with W held: the player stops (keys released on blur).
subprocess.run(["xdotool", "keydown", "w"], check=True)
time.sleep(0.8)
moving_from = (field("x"), field("z"))
common.deactivate_window()
time.sleep(0.4)
stopped_at = (field("x"), field("z"))
time.sleep(1.5)
later = (field("x"), field("z"))
subprocess.run(["xdotool", "keyup", "w"], check=True)
seen["blur"] = {"moving_from": moving_from, "at_blur": stopped_at, "1.5 s later": later}
report.check("focus lost with W held: the player stops", stopped_at == later, json.dumps(seen["blur"]))
recorder.terminate()
with open(common.out("player2.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
