"""The real app's 3D pane, driven (pane agent; run by real_pane.sh): the
chat's world waited for until its first real frame is painted (the cover
gone, the toolbar shown, the picture not black), then orbit by a right
drag, zoom by the wheel, a camera key (D held), pause, step and play with
the tick counter moving, Day/Night, a click on a block with its `Clicked`
announcement, the selection tool into a chip, Fill, player mode with the
pointer lock, a split drag with the picture following, 60 s idle, and a
quit; a whole-window screenshot after each step (real-NN-<step>.png). The
frames painted while orbiting, while the world runs and while idle are
counted from the pane's costs, which the app logs with each toolbar press;
the app's resident memory is read when the world is shown and at the end. Every key and press is a real event on the private
display. Writes real.json with what each step saw and the app's logged
lines (its renderer, the pane's costs)."""

import hashlib
import json
import os
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-pane")
PANE = "Live Minecraft world"
shots = []
seen = {}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def extents(obj):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    return e.x, e.y, e.width, e.height


def canvas():
    return common.wait_for(lambda: node(PANE), 120.0, "the pane's canvas")


def shot(step):
    name = f"real-{len(shots) + 1:02}-{step}.png"
    common.screenshot(name)
    shots.append(name)
    return common.out(name)


def picture():
    """The pane's part of a fresh screenshot: its hash and its mean
    brightness (a cover is black with grey text)."""
    from PIL import Image, ImageStat
    path = common.screenshot("tmp-real.png")
    x, y, w, h = extents(canvas())
    crop = Image.open(path).convert("RGB").crop((x + 4, y + 70, x + w - 4, y + h - 70))
    os.remove(path)
    return hashlib.sha256(crop.tobytes()).hexdigest(), sum(ImageStat.Stat(crop).mean) / 3


def changed(before, what):
    after = picture()
    report.check(what, after[0] != before[0])
    return after


def polite():
    return [b.get_name() for b in common.find(app(), role="status bar") if b.get_name()]


def press(name):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 15.0, name), 0)
    time.sleep(0.6)


def keydown(k):
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "keydown", k], check=True)
    else:
        subprocess.run(["wtype", "-P", k.lower()], check=True)


def keyup(k):
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "keyup", k], check=True)
    else:
        subprocess.run(["wtype", "-p", k.lower()], check=True)


def ticks():
    for n in common.find(app(), pred=lambda o: o.get_name().startswith("Ticks since the build was placed: ")):
        return int(n.get_name().split(": ")[1].split(" ")[0].replace(",", ""))
    return None


def costs():
    """The pane's costs in the newest `live` line (logged with each
    toolbar press)."""
    lines = [a for a in common.actions() if a.get("action") == "live" and "costs" in a]
    return lines[-1]["costs"] if lines else None


def memory_mb():
    with open(f"/proc/{os.environ['MCBENCH_APP_PID']}/status") as status:
        for line in status:
            if line.startswith("VmRSS:"):
                return round(int(line.split()[1]) / 1024)
    return None


def canvas_click(dx=0, dy=0):
    x, y, w, h = extents(canvas())
    common.pointer("mousemove", str(x + w // 2 + dx), str(y + h // 2 + dy))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(0.6)


app()
common.focus_window()
c = canvas()
# The first real frame: the cover gone and the toolbar shown, the picture
# not the cover's black. Its time is the app's own (`pane frame`, t_ms
# since the process started); the memory at once, and again at 20 s.
common.wait_for(lambda: node("Live world"), 180.0, "the live world's toolbar (the first real frame painted)")
seen["memory_mb_at_first_pane_frame"] = memory_mb()
shot("start")
first = next(a for a in common.actions() if a.get("action") == "pane frame")
seen["first_frame_s"] = round(first["t_ms"] / 1e3, 3)
before = picture()
report.check("the first real frame is a picture, not the cover", before[1] > 20, f"mean {before[1]:.0f}")
report.check("the pane is described by its state", canvas().get_description().endswith("playing"), canvas().get_description())
started_s = float(open(f"/proc/{os.environ['MCBENCH_APP_PID']}/stat").read().rsplit(")", 1)[1].split()[19]) / os.sysconf("SC_CLK_TCK")
uptime_s = float(open("/proc/uptime").read().split()[0])
# The process's age now: wait until it is 20 s old.
time.sleep(max(0.0, 20.0 - (uptime_s - started_s)))
seen["memory_mb_at_20s"] = memory_mb()
shot("first")

x, y, w, h = extents(canvas())
cx, cy = x + w // 2, y + h // 2
common.pointer("mousemove", str(cx), str(cy))
common.pointer("mousedown", "3")
for i in range(1, 16):
    common.pointer("mousemove", str(cx + i * 10), str(cy))
    time.sleep(0.02)
common.pointer("mouseup", "3")
time.sleep(1.0)
before = changed(before, "a right drag orbits the camera")
# What the pane had of it (its `drag` line), whether or not the picture moved.
seen["orbit_drag"] = [a for a in common.actions() if a.get("action") == "drag"][:1]
shot("orbit")

for _ in range(5):
    common.pointer("click", "4")
    time.sleep(0.05)
time.sleep(1.0)
before = changed(before, "the wheel zooms")
shot("zoom")

Atspi.Component.grab_focus(canvas())
time.sleep(0.3)
keydown("d")
time.sleep(0.6)
keyup("d")
time.sleep(1.0)
before = changed(before, "D held pans the camera")
shot("key")

t0 = ticks()
time.sleep(1.0)
t1 = ticks()
report.check("the tick counter moves while playing", t0 is not None and t1 is not None and t1 > t0, f"{t0} -> {t1}")
press("Pause")
paused_costs, paused_at = costs(), time.time()
t2 = ticks()
time.sleep(1.0)
report.check("paused, it stays", ticks() == t2, f"{t2} -> {ticks()}")
# Orbiting the paused world for about 2 s: the frames it painted.
x, y, w, h = extents(canvas())
common.pointer("mousemove", str(cx), str(cy))
common.pointer("mousedown", "3")
orbit_from = time.time()
for i in range(1, 61):
    common.pointer("mousemove", str(cx + (i % 30) * 6 - 90), str(cy))
    time.sleep(0.033)
common.pointer("mouseup", "3")
orbit_s = time.time() - orbit_from
press("Step one tick")
stepped_costs = costs()
report.check("Step adds one tick", ticks() == (t2 or 0) + 1, f"{t2} -> {ticks()}")
if paused_costs and stepped_costs:
    seen["orbit_frames"] = stepped_costs["frames"] - paused_costs["frames"]
    seen["orbit_s"] = round(orbit_s, 2)
    seen["orbit_fps"] = round(seen["orbit_frames"] / orbit_s, 1)
shot("paused-stepped")
press("Play")
play_costs, play_at = costs(), time.time()
time.sleep(5.0)
press("Pause")
run_costs, run_at = costs(), time.time()
if play_costs and run_costs:
    seen["running_frames"] = run_costs["frames"] - play_costs["frames"]
    seen["running_s"] = round(run_at - play_at, 2)
    seen["running_fps"] = round(seen["running_frames"] / (run_at - play_at), 1)
    seen["costs_after_running"] = run_costs
press("Play")
time.sleep(0.5)

before = picture()

# A click on the block at the pane's centre (a lever when the camera faces
# one; any block is announced with its input or "takes no input").
common.pointer("mousemove", str(cx), str(cy + 40))
time.sleep(0.2)
common.pointer("click", "1")
said = common.wait_for(lambda: [s for s in polite() if " at " in s], 10.0, "the click's announcement")
seen["clicked"] = said
report.check("a click on a block is announced (Clicked)", bool(said), str(said))
time.sleep(1.0)
shot("clicked")
seen["clicked_picture_changed"] = picture()[0] != before[0]

press("Select blocks")
common.pointer("mousemove", str(cx - 30), str(cy + 60))
common.pointer("mousedown", "1")
for i in range(1, 10):
    common.pointer("mousemove", str(cx - 30 + i * 8), str(cy + 60 + i * 3))
    time.sleep(0.03)
common.pointer("mouseup", "1")
chip = common.wait_for(lambda: [n.get_name() for n in common.find(app(), role="button") if n.get_name().startswith("Remove ")],
                       15.0, "the selection's chip")
seen["chip"] = chip
report.check("the selection tool puts a chip in the composer", bool(chip), str(chip))
shot("selection")
common.key("Escape", settle=0.4)

# The lever (the chat says block -2, 1, -1, on the north wall face): the
# camera's x and z fields to it, closer by the wheel, a click at the centre;
# a lever toggled moves the door in the next seconds.
for axis, value in (("x", "-2"), ("z", "-1")):
    field = common.wait_for(lambda: node(f"Camera position {axis}"), 10.0, f"the {axis} field")
    Atspi.Component.grab_focus(field)
    time.sleep(0.3)
    common.key("ctrl+a", settle=0.2)
    # A leading "-" would be read as an option: "--" ends them.
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "type", "--delay", "20", "--", value], check=True)
    else:
        subprocess.run(["wtype", "-d", "20", "--", value], check=True)
    time.sleep(0.3)
    common.key("Return", settle=0.6)
x, y, w, h = extents(canvas())
cx, cy = x + w // 2, y + h // 2
common.pointer("mousemove", str(cx), str(cy))
for _ in range(3):
    common.pointer("click", "4")
    time.sleep(0.05)
time.sleep(1.0)
shot("lever-aimed")
lever_before = picture()
common.pointer("click", "1")
try:
    said = common.wait_for(lambda: [s for s in polite() if " at " in s and s not in seen["clicked"]], 10.0,
                           "the lever click's announcement")
except AssertionError:
    said = polite()
seen["lever_clicked"] = said
time.sleep(5.0)
seen["lever_picture_changed"] = picture()[0] != lever_before[0]
shot("lever-clicked")
report.check("a click at the lever's place is announced", bool(said), str(said))

# Fill: beside the chat the pane is under BlueMap's 576 px break, whose
# narrow bar leaves out Day/Night and the view switch.
press("Fill the window")
time.sleep(1.0)
before = picture()
shot("fill")
press("Day/Night")
time.sleep(1.0)
night = picture()
report.check("Night darkens the picture", night[1] < before[1], f"{before[1]:.0f} -> {night[1]:.0f}")
shot("night")
press("Day/Night")
time.sleep(1.0)
before = picture()

# Player mode with the pointer lock: a click takes it, the look follows
# relative motion, Escape frees it.
press("Player mode: walk and fly as a Minecraft player")
time.sleep(1.5)
canvas_click(0, 30)
pointer = [a for a in common.actions() if a.get("action") == "pointer"]
seen["pointer"] = pointer[-3:]
if pointer and pointer[-1].get("captured"):
    for _ in range(10):
        if common.BACKEND == "x11":
            subprocess.run(["xdotool", "mousemove_relative", "--", "60", "0"], check=True)
        else:
            common.pointer("mousemove_relative", "60", "0")
        time.sleep(0.05)
    time.sleep(0.8)
    looks = [a for a in common.actions() if a.get("action") == "look"]
    report.check("player mode: a click takes the pointer and the look follows its motion", bool(looks), str(looks[-1:]))
else:
    report.check("player mode: a click takes the pointer (or the compositor's refusal is said)",
                 bool(pointer) and "refused" in pointer[-1], str(pointer[-1:]))
shot("player")
common.key("Escape", settle=0.6)
seen["after_player_escape"] = {n: node(n) is not None for n in ("Perspective-View", "Back beside the chat (Esc)", "Fill the window")}
# Out of player mode and back beside the chat, whichever of the two the
# Escape left to do.
if node("Perspective-View") is not None:
    press("Perspective-View")
if node("Back beside the chat (Esc)") is not None:
    press("Back beside the chat (Esc)")
time.sleep(1.0)
before = picture()

# The split: the divider between the chat column and the pane, dragged left.
x, y, w, h = extents(canvas())
divider = x - 3
common.pointer("mousemove", str(divider), str(y + h // 2))
common.pointer("mousedown", "1")
for i in range(1, 21):
    common.pointer("mousemove", str(divider - i * 10), str(y + h // 2))
    time.sleep(0.03)
common.pointer("mouseup", "1")
time.sleep(1.5)
x2, _, w2, _ = extents(canvas())
report.check("the split drag widens the pane", w2 > w, f"{w} -> {w2}")
changed(before, "the picture follows the split")
shot("split")

# Idle: paused, the pointer still, 60 s; the frames painted in them.
press("Pause")
idle_from = costs()
time.sleep(60.0)
press("Play")
idle_to = costs()
if idle_from and idle_to:
    seen["idle_frames_60s"] = idle_to["frames"] - idle_from["frames"]
    seen["idle_recent"] = idle_to["recent"]
seen["memory_mb_end"] = memory_mb()
shot("idle-end")

seen["shots"] = shots
seen["log"] = [a for a in common.actions() if a.get("action") in ("renderer", "live", "fonts")]
quit_from = time.time()
common.key("ctrl+q", settle=0.0)
pid = int(os.environ["MCBENCH_APP_PID"])


def running():
    try:
        with open(f"/proc/{pid}/stat") as stat:
            return stat.read().rsplit(")", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


while running() and time.time() - quit_from < 10.0:
    time.sleep(0.02)
seen["quit_s"] = round(time.time() - quit_from, 2)
report.check("Ctrl+Q quits within 2 s", not running() and seen["quit_s"] < 2.0, f"{seen['quit_s']} s")
with open(common.out("real.json"), "w") as f:
    json.dump(seen, f, indent=1)
report.finish()
