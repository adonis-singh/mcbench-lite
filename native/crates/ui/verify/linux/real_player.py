"""Player mode on the real core (pane agent; run by real_pane.sh with
MCBENCH_REAL_DRIVER=real_player.py), X11: in Fill, player mode entered and
the pointer taken, then W, A, S, D held, a jump, Space twice (fly) and
held, Shift, Ctrl with W (sprint), a right-click (use) on what is ahead,
a long sprint toward the void for a fall, and leaving while a spawn is
pending; after each the camera's x and z fields, the polite region and
the HUD's notes are read and the window pictured. Writes player.json."""

import json
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-player")
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
    return Atspi.Text.get_text(n, 0, -1) if n else None


def notes():
    texts = [n.get_name() for n in common.find(app()) if n.get_name() and n.get_role_name() in ("status bar", "label")]
    return [t for t in texts if not t.startswith("Ticks since") and t != "Near weekly limit"][-4:]


def hold(keys, seconds):
    subprocess.run(["xdotool", "keydown", *keys], check=True)
    time.sleep(seconds)
    subprocess.run(["xdotool", "keyup", *reversed(keys)], check=True)
    time.sleep(0.6)


def step(name):
    seen["steps"].append({"step": name, "x": field("x"), "z": field("z"), "notes": notes(),
                          "player": [a for a in common.actions() if a.get("action") in ("player", "pointer", "player note")][-2:]})
    common.screenshot(f"player-{len(seen['steps']):02}-{name}.png")


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
step("stood-locked")
hold(["w"], 1.5)
step("w")
hold(["a"], 1.0)
step("a")
hold(["s"], 1.0)
step("s")
hold(["d"], 1.0)
step("d")
subprocess.run(["xdotool", "key", "space"], check=True)
time.sleep(0.25)
step("jump-mid")
time.sleep(1.0)
subprocess.run(["xdotool", "key", "space"], check=True)
time.sleep(0.12)
subprocess.run(["xdotool", "key", "space"], check=True)
time.sleep(0.2)
hold(["space"], 1.2)
step("fly-up")
hold(["shift"], 0.8)
step("shift-down")
subprocess.run(["xdotool", "key", "space"], check=True)
time.sleep(0.12)
subprocess.run(["xdotool", "key", "space"], check=True)
time.sleep(1.5)
step("fly-off-fallen")
hold(["ctrl", "w"], 1.5)
step("sprint")
common.pointer("click", "3")
time.sleep(1.0)
step("right-click-use")
# Toward the void: the build's ground is a pad in the void.
hold(["ctrl", "w"], 12.0)
time.sleep(3.0)
step("toward-void")
time.sleep(4.0)
step("after-fall")
common.key("Escape", settle=0.6)
press("Perspective-View")
# Leaving while a spawn is pending: in and straight out.
press(PLAYER)
press("Perspective-View")
time.sleep(2.0)
step("left-while-pending")
seen["log"] = [a for a in common.actions() if a.get("action") in ("player", "pointer", "player note", "camera mode")][-12:]
report.check("player mode ran", True)
with open(common.out("player.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
