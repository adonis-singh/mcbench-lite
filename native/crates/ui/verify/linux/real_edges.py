"""The pane's edges on the real renderer (pane agent; real_pane.sh with
MCBENCH_REAL_DRIVER=real_edges.py), X11: the frame should reach every edge
of the pane's box, with no black rim, wide, at 979, 800 and 640 px, wide
again, and after a split drag. At each, the box's extents and the pixels
of its outermost two rows and columns, sampled where nothing is drawn over
the world (the corners clear of the bars, the sides at half height).
Writes edges.json and a picture of each."""

import json
import subprocess
import time

from PIL import Image

import common

Atspi = common.Atspi
report = common.Report("real-edges")
PANE = "Live Minecraft world"
HEIGHT = 820
seen = {"widths": []}


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def extents():
    e = common.wait_for(lambda: node(PANE), 30.0, "the pane").get_extents(Atspi.CoordType.WINDOW)
    return e.x, e.y, e.width, e.height


def resize(width):
    window = subprocess.run(["xdotool", "search", "--name", "mcbench-lite$"], check=True,
                            capture_output=True, text=True).stdout.split()[0]
    subprocess.run(["xdotool", "windowsize", "--sync", window, str(width), str(HEIGHT)], check=True)
    time.sleep(2.5)


def origin():
    """The window's place on the screen the picture is of (the window
    manager may keep it off 0, 0)."""
    window = subprocess.run(["xdotool", "search", "--name", "mcbench-lite$"], check=True,
                            capture_output=True, text=True).stdout.split()[0]
    geometry = subprocess.run(["xdotool", "getwindowgeometry", "--shell", window], check=True,
                              capture_output=True, text=True).stdout
    values = dict(line.split("=", 1) for line in geometry.split())
    return int(values["X"]), int(values["Y"])


def black(p):
    return max(p) <= 6


def rim(name):
    path = common.screenshot(f"edges-{name}.png")
    im = Image.open(path).convert("RGB")
    x, y, w, h = extents()
    ox, oy = origin()
    x, y = x + ox, y + oy
    samples = {}
    for side, points in {
        "left": [(x + i, y + h // 2 + d) for i in (0, 1) for d in (-40, 0, 40)],
        "right": [(x + w - 1 - i, y + h // 2 + d) for i in (0, 1) for d in (-40, 0, 40)],
        "top": [(x + w // 2 + d, y + i) for i in (0, 1) for d in (-60, 0, 60)],
        "bottom": [(x + w // 2 + d, y + h - 1 - i) for i in (0, 1) for d in (-30, 0, 30)],
    }.items():
        samples[side] = [im.getpixel(p) for p in points]
    rimmed = [side for side, ps in samples.items() if sum(black(p) for p in ps) >= len(ps) // 2]
    seen["widths"].append({"at": name, "origin": [ox, oy], "box": [x, y, w, h], "samples": samples, "black_sides": rimmed})
    report.check(f"{name}: no black rim on the box's edges", not rimmed, json.dumps({"box": [x, y, w, h], "black": rimmed}))


app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
time.sleep(1.0)
for width in (1280, 979, 800, 640, 1280):
    resize(width)
    rim(f"{width}" if not seen["widths"] or width != 1280 else "1280-again")
x, y, w, h = extents()
divider = x - 3
common.pointer("mousemove", str(divider), str(y + h // 2))
common.pointer("mousedown", "1")
for i in range(1, 21):
    common.pointer("mousemove", str(divider - i * 10), str(y + h // 2))
    time.sleep(0.03)
common.pointer("mouseup", "1")
time.sleep(2.0)
common.pointer("mousemove", "5", "5")
time.sleep(0.5)
rim("split")
with open(common.out("edges.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
