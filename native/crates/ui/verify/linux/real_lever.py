"""The real app's lever (pane agent; run by real_pane.sh with
MCBENCH_REAL_DRIVER=real_lever.py): its place read from the stored world
(the one `minecraft:lever` of the chat's build, at build (-2, 1, -1) on a
wall, facing north: world (-2, 65, -1) over grass at y 64), seen from the
north, its centre projected through the camera of the frame on screen
(the pane logs it with each pick) and clicked exactly there, zoomed in
until it is several pixels wide. The pick's answer is read; when it names
the lever, the gate is pictured paused before, then a second and five
seconds after Play. Writes lever.json with every probe."""

import hashlib
import json
import math
import os
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-lever")
PANE = "Live Minecraft world"
# The lever's model stands against the block south of its cell (it faces
# north): its centre is near the cell's south side.
LEVER = (-1.5, 65.45, -0.2)
CELL = (-2, 65, -1)
seen = {"probes": []}


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


def press(name):
    Atspi.Action.do_action(common.wait_for(lambda: node(name), 15.0, name), 0)
    time.sleep(0.6)


def polite():
    return [b.get_name() for b in common.find(app(), role="status bar") if b.get_name()]


def shot(name):
    return common.screenshot(f"lever-{name}.png")


def pane_hash():
    from PIL import Image
    path = common.screenshot("tmp-lever.png")
    x, y, w, h = extents(canvas())
    crop = Image.open(path).convert("RGB").crop((x + 4, y + 50, x + w - 4, y + h - 70))
    os.remove(path)
    return hashlib.sha256(crop.tobytes()).hexdigest()


def set_field(axis, value):
    field = common.wait_for(lambda: node(f"Camera position {axis}"), 10.0, f"the {axis} field")
    Atspi.Component.grab_focus(field)
    time.sleep(0.3)
    common.key("ctrl+a", settle=0.2)
    subprocess.run(["xdotool", "type", "--delay", "20", "--", value], check=True)
    time.sleep(0.3)
    common.key("Return", settle=0.8)


def picks():
    return [a for a in common.actions() if a.get("action") in ("pick", "picked")]


def click_at(px, py):
    """A click at a window pixel; the pick's log line (with the frame's
    camera) and the announcement after it."""
    n = len(picks())
    common.pointer("mousemove", str(int(px)), str(int(py)))
    time.sleep(0.15)
    common.pointer("click", "1")
    time.sleep(0.7)
    new = picks()[n:]
    said = [s for s in polite() if " at " in s]
    pick = next((p for p in new if p["action"] == "pick"), None)
    picked = next((p for p in new if p["action"] == "picked"), None)
    return pick, picked, (said[-1] if said else None)


def project(camera, size, point):
    """The frame pixel of a world point: the pane's `ray` turned round
    (forward to the target, right (cos r, 0, sin r), up = right x forward);
    with its depth and the half field of view's tangent."""
    eye, target = camera["eye"], camera["target"]
    f = [target[i] - eye[i] for i in range(3)]
    fl = math.sqrt(sum(v * v for v in f))
    f = [v / fl for v in f]
    r = [math.cos(camera["rotation"]), 0.0, math.sin(camera["rotation"])]
    u = [r[1] * f[2] - r[2] * f[1], r[2] * f[0] - r[0] * f[2], r[0] * f[1] - r[1] * f[0]]
    d = [point[i] - eye[i] for i in range(3)]
    depth = sum(d[i] * f[i] for i in range(3))
    if depth <= 0:
        return None
    t = math.tan(math.radians(camera["v_fov"]) / 2)
    w, h = size
    nx = sum(d[i] * r[i] for i in range(3)) / depth
    ny = sum(d[i] * u[i] for i in range(3)) / depth
    return ((nx / (t * w / h) + 1) * w / 2, (1 - ny / t) * h / 2, depth, t)


def drag(dx, dy, steps=30):
    x, y, w, h = extents(canvas())
    cx, cy = x + w // 2, y + h // 2
    x0 = cx - dx // 2
    common.pointer("mousemove", str(x0), str(cy))
    common.pointer("mousedown", "3")
    for i in range(1, steps + 1):
        common.pointer("mousemove", str(int(x0 + dx * i / steps)), str(int(cy + dy * i / steps)))
        time.sleep(0.02)
    common.pointer("mouseup", "3")
    time.sleep(1.5)


app()
common.focus_window()
canvas()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
press("Fill the window")
press("Pause")
set_field("x", str(CELL[0]))
set_field("z", str(CELL[2] - 2))
press("Compass / Face North")
time.sleep(1.0)
x, y, w, h = extents(canvas())
# Half round (BlueMap: dx * 6 / width radians): from the north, looking south.
drag(int(math.pi * w / 6 / 2), 0)
drag(int(math.pi * w / 6 / 2), 0)
target = None
for zoom in range(10):
    x, y, w, h = extents(canvas())
    pick, picked, said = click_at(x + w // 2, y + h // 2)
    camera, size = pick["camera"], pick["size"]
    at = project(camera, size, LEVER)
    # A quarter block across, in frame pixels, at the lever's depth.
    width_px = (size[1] / 2) / (at[3] * at[2]) * 0.25 if at else 0
    seen["probes"].append({"zoom": zoom, "camera": camera, "size": size, "lever_px": at[:2] if at else None,
                           "lever_width_px": round(width_px, 1), "centre_said": said})
    if at and width_px >= 8:
        target = at
        break
    common.pointer("mousemove", str(x + w // 2), str(y + h // 2))
    common.pointer("click", "4")
    time.sleep(1.2)
report.check("the lever projects into the frame, several pixels wide", target is not None, json.dumps(seen["probes"][-1:]))
if target is not None:
    x, y, w, h = extents(canvas())
    scale = w / seen["probes"][-1]["size"][0]
    px, py = x + target[0] * scale, y + target[1] * scale
    seen["lever_window_px"] = [round(px, 1), round(py, 1)]
    common.pointer("mousemove", str(int(px)), str(int(py)))
    time.sleep(0.5)
    shot("1-aimed-paused")
    before = pane_hash()
    pick, picked, said = click_at(px, py)
    seen["lever_click"] = {"pick": pick, "picked": picked, "said": said}
    report.check("the pick at the lever's pixel names the lever", bool(said) and said.startswith("Lever"), str(said))
    shot("2-clicked")
    if said and said.startswith("Lever"):
        press("Play")
        time.sleep(1.0)
        shot("3-one-second")
        one = pane_hash()
        time.sleep(4.0)
        shot("4-five-seconds")
        seen["changed_1s"] = one != before
        seen["changed_5s"] = pane_hash() != one
with open(common.out("lever.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
