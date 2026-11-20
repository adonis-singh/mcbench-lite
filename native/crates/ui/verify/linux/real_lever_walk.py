"""The walk-up use of the lever on the real app (pane agent; real_pane.sh
with MCBENCH_REAL_DRIVER=real_lever_walk.py), X11. The stored world's one
lever is at world (-2, 65, -1) on the wall's north face; the player stands
south of the wall. Up by flight, north over the wall, down, then steered in
a closed loop: taps of W of 100 to 150 ms, the eye read after each from the
window's `player input` line (a one-pixel nudge there and back sends one),
turned toward a standing place 2.7 blocks north of the lever. The turn's
degrees per pixel and the walking direction of a yaw are measured first.
Stopped, the look is turned onto the lever, the gate pictured, a
right-click used, and the gate pictured a second and five seconds after.
Writes lever_walk.json with every step and the pick, use and click lines."""

import json
import math
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-lever-walk")
PANE = "Live Minecraft world"
PLAYER = "Player mode: walk and fly as a Minecraft player"
# The lever's model, against the wall block south of its cell.
LEVER = (-1.5, 65.4, -0.25)
STAND = (-1.5, -2.95)
seen = {"steps": [], "loop": []}


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


def xdo(*args):
    subprocess.run(["xdotool", *args], check=True)


def hold(keys, seconds):
    xdo("keydown", *keys)
    time.sleep(seconds)
    xdo("keyup", *reversed(keys))
    time.sleep(0.4)


def tap(key, times=1, gap=0.12):
    for _ in range(times):
        xdo("key", key)
        time.sleep(gap)


def last_input():
    lines = [a for a in common.actions() if a.get("action") == "player input"]
    return lines[-1] if lines else None


def nudge():
    """A pixel right and back: two inputs, the look as it was; the second
    carries the eye of the frame then."""
    n = len([a for a in common.actions() if a.get("action") == "player input"])
    xdo("mousemove_relative", "--", "1", "0")
    time.sleep(0.15)
    xdo("mousemove_relative", "--", "-1", "0")
    common.wait_for(lambda: len([a for a in common.actions() if a.get("action") == "player input"]) >= n + 2 or None,
                    5.0, "the nudge's inputs")
    return last_input()


def turn(pixels_x, pixels_y=0):
    """Moves the locked pointer in steps of at most 100 pixels."""
    px, py = int(round(pixels_x)), int(round(pixels_y))
    while px or py:
        sx = max(-100, min(100, px))
        sy = max(-100, min(100, py))
        xdo("mousemove_relative", "--", str(sx), str(sy))
        px -= sx
        py -= sy
        time.sleep(0.03)
    time.sleep(0.4)


def step(name):
    i = last_input()
    seen["steps"].append({"step": name, "x": field("x"), "z": field("z"),
                          "eye": i and i.get("eye"), "yaw": i and i.get("yaw"), "pitch": i and i.get("pitch")})
    common.screenshot(f"walk-{len(seen['steps']):02}-{name}.png")


def wrap(deg):
    return (deg + 180.0) % 360.0 - 180.0


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
time.sleep(1.0)
step("stood")

# The turn per pixel: 100 pixels right, read, and back.
a = nudge()
xdo("mousemove_relative", "--", "100", "0")
time.sleep(0.4)
b = nudge()
dpp = (b["yaw"] - a["yaw"]) / 100.0
xdo("mousemove_relative", "--", "-100", "0")
time.sleep(0.4)
seen["degrees_per_pixel"] = dpp
report.check("the turn per pixel is measured", abs(dpp) > 0.01, f"{dpp}")

# Up and over the wall north (the first look faces north, toward the build).
tap("space", 2)
hold(["space"], 2.0)
step("flying-up")
for _ in range(14):
    z = field("z")
    if z is not None and z <= -4:
        break
    hold(["w"], 0.4)
step("north-of-wall")
hold(["shift"], 3.0)
step("landed")

# The walking direction of a yaw: one tap of W, the eye before and after.
p0 = nudge()
hold(["w"], 0.15)
time.sleep(0.6)
p1 = nudge()
dx, dz = p1["eye"][0] - p0["eye"][0], p1["eye"][2] - p0["eye"][2]
moved_at = math.degrees(math.atan2(dz, dx))
# heading (atan2(dz, dx) of the walk) = offset + sign * yaw; the sign is
# Minecraft's (yaw 0 walks +z, 90 walks -x), checked by the offset.
offset = wrap(moved_at - p0["yaw"])
seen["calibration"] = {"from": p0["eye"], "to": p1["eye"], "yaw": p0["yaw"], "walk_deg": moved_at, "offset": offset}

# Closed loop: face the standing place, a tap of W, the eye read.
for i in range(40):
    cur = nudge()
    ex, ez = cur["eye"][0], cur["eye"][2]
    gx, gz = STAND[0] - ex, STAND[1] - ez
    dist = math.hypot(gx, gz)
    entry = {"i": i, "eye": cur["eye"], "yaw": cur["yaw"], "to_go": round(dist, 3)}
    seen["loop"].append(entry)
    if dist < 0.3:
        break
    want = wrap(math.degrees(math.atan2(gz, gx)) - offset)
    dyaw = wrap(want - cur["yaw"])
    turn(dyaw / dpp)
    ms = 100 if dist < 0.8 else 150
    entry.update({"turned_deg": round(dyaw, 2), "tap_ms": ms})
    hold(["w"], ms / 1000.0)
    time.sleep(0.5)
step("stopped")

# The look onto the lever.
cur = nudge()
ex, ey, ez = cur["eye"]
lx, ly, lz = LEVER
want_yaw = wrap(math.degrees(math.atan2(lz - ez, lx - ex)) - offset)
want_pitch = math.degrees(math.atan2(ey - ly, math.hypot(lx - ex, lz - ez)))
turn(wrap(want_yaw - cur["yaw"]) / dpp, (want_pitch - cur["pitch"]) / dpp)
aimed = nudge()
ax, ay, az = aimed["eye"]
seen["aim"] = {"eye": aimed["eye"], "yaw": aimed["yaw"], "pitch": aimed["pitch"], "want_yaw": want_yaw,
               "want_pitch": want_pitch, "eye_to_lever": math.dist((ax, ay, az), LEVER),
               "eye_to_lever_cell_centre": math.dist((ax, ay, az), (-1.5, 65.5, -0.5))}
step("aimed")
report.check("stopped within 3 blocks of the lever", math.hypot(LEVER[0] - ax, LEVER[2] - az) <= 3.0,
             json.dumps(seen["aim"]))

before = said()
n = len(common.actions())
common.pointer("click", "3")
time.sleep(1.0)
lines = [a for a in common.actions()[n:] if a.get("action") in ("pick", "picked", "use", "click")]
seen["use_lines"] = lines
seen["use_said"] = [s for s in said() if s not in before]
step("used")
time.sleep(1.0)
step("one-second")
time.sleep(4.0)
step("five-seconds")
use = next((a for a in lines if a["action"] == "use"), None)
report.check("the pick at the crosshair is the lever's cell", bool(use) and use["block"] == [-2, 65, -1], json.dumps(use))
report.check("the use is within reach", bool(use) and use["within_reach"], json.dumps(use))
report.check("a right-click on the lever toggles it", any(s.startswith("Lever") for s in seen["use_said"]),
             str(seen["use_said"]))
with open(common.out("lever_walk.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
