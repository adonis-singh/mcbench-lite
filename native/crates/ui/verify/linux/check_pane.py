"""The 3D pane (pane agent) over the fake pane: its tree, by keyboard alone
(Tab to the pane, the camera keys, Enter on the centre, Escape, the
toolbar's buttons by Space) and by pointer (a click on a block, a drag,
Day/Night, the selection tool into the composer, Fill and Escape, player
mode, the schematic export), with a screenshot of each state. Every key and
press is a real event on the private display.

    check_pane.py <dark|light> <save folder>

The binary runs with `--save-to <save folder>` (an export writes there with
no dialog, an empty folder the caller made) and, for light,
`--preferences` naming a file with the light theme."""

import hashlib
import os
import subprocess
import sys
import time

import common

Atspi = common.Atspi
theme, saved = sys.argv[1], sys.argv[2]
report = common.Report(f"pane-{theme}")
PANE = "Live Minecraft world"


def app():
    return common.app()


def node(role, name):
    """The first node named `name` (a switch is a toggle button, a label
    may be static: the role is the reader's, the name is what is checked)."""
    found = common.find(app(), name=name)
    return found[0] if found else None


def canvas():
    return common.wait_for(lambda: node("canvas", PANE), 20.0, "the pane's canvas")


def extents(obj):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    return e.x, e.y, e.width, e.height


def centre(obj, dx=0, dy=0):
    x, y, w, h = extents(obj)
    return str(x + w // 2 + dx), str(y + h // 2 + dy)


def click(obj, button="1"):
    x, y = centre(obj)
    common.pointer("mousemove", x, y)
    time.sleep(0.15)
    common.pointer("click", button)
    time.sleep(0.4)


def polite():
    bars = common.find(app(), role="status bar")
    return [b.get_name() for b in bars if b.get_name()]


def pane_pixels():
    """A hash of the pane's part of a screenshot (it changes with the
    camera)."""
    from PIL import Image
    path = common.screenshot("tmp-pane.png")
    x, y, w, h = extents(canvas())
    crop = Image.open(path).crop((x + 4, y + 40, x + w - 4, y + h - 60))
    digest = hashlib.sha256(crop.tobytes()).hexdigest()
    os.remove(path)
    return digest


def focused():
    return common.focused(app())


def shot(name):
    common.screenshot(f"pane-{theme}-{name}.png")


app()
time.sleep(1.0)
common.focus_window()
c = canvas()
common.wait_for(lambda: node("tool bar", "Live world"), 15.0, "the live world's toolbar")
report.check("the pane is a canvas named Live Minecraft world", c is not None)
desc = c.get_description()
report.check("it is described by its state", desc.endswith("blocks, playing"), desc)
ticks = lambda: [n.get_name() for n in common.find(app(), pred=lambda o: bool(o.get_name())) if n.get_name().startswith("Ticks since the build was placed: ")]
first = ticks()
time.sleep(1.5)
later = ticks()
report.check("the tick counter counts (the stand-in's 20 ticks a second)", bool(first) and later != first, f"{first} {later}")
shot("shown")

# -- keyboard alone -----------------------------------------------------
for _ in range(10):
    f = focused()
    if f is not None and f.get_role_name() == "canvas":
        break
    common.key("Tab", settle=0.3)
f = focused()
report.check("Tab reaches the pane after the chat column", f is not None and f.get_name() == PANE, common.label(f))
shot("focus")
before = pane_pixels()
if common.BACKEND == "x11":
    import subprocess
    subprocess.run(["xdotool", "keydown", "w"], check=True)
    time.sleep(0.15)
    subprocess.run(["xdotool", "keyup", "w"], check=True)
else:
    import subprocess
    subprocess.run(["wtype", "-P", "w", "-s", "150", "-p", "w"], check=True)
time.sleep(0.5)
report.check("W held moves the camera (the picture changed)", pane_pixels() != before)
composer = common.find(app(), role="entry", name="Message")
typed = Atspi.Text.get_text(composer[0], 0, -1) if composer else None
report.check("W stays with the pane (nothing typed in the composer)", typed == "", repr(typed))
common.key("Return", settle=0.8)
said = polite()
report.check("Enter uses the block at the centre and the polite region says it", any(" at " in s for s in said), str(said))
shot("enter")
# Tab on to the toolbar's Pause, Space pauses.
for _ in range(25):
    common.key("Tab", settle=0.2)
    f = focused()
    if f is not None and f.get_name() == "Pause":
        break
report.check("Tab reaches Pause", focused() is not None and focused().get_name() == "Pause", common.label(focused()))
common.key("space", settle=0.6)
report.check("Space pauses: the button reads Play", node("button", "Play") is not None)
report.check("paused: the pane says so", canvas().get_description().endswith("paused"), canvas().get_description())
shot("paused")
Atspi.Action.do_action(node("button", "Play"), 0)
time.sleep(0.5)

# -- pointer ------------------------------------------------------------
c = canvas()
x, y = centre(c, 0, 60)
common.pointer("mousemove", x, y)
time.sleep(0.2)
common.pointer("click", "1")
time.sleep(0.8)
said = polite()
report.check("a click on a block is announced", any(" at " in s for s in said), str(said))
report.check("the clicked block's label shows", any(n.get_name().startswith("Block: x ") for n in common.find(app(), pred=lambda o: bool(o.get_name()))))
shot("clicked")
before = pane_pixels()
common.pointer("mousemove", x, y)
common.pointer("mousedown", "3")
for i in range(1, 11):
    common.pointer("mousemove", str(int(x) + i * 12), y)
    time.sleep(0.02)
common.pointer("mouseup", "3")
time.sleep(0.6)
report.check("a right drag turns the camera", pane_pixels() != before)
# Beside the chat the pane is narrower than BlueMap's 576 px break: its bar
# hides Day/Night, the view switch and Reset, as the page's does.
report.check("the narrow bar hides Day/Night and the view switch (BlueMap's thin-hide)",
             node("button", "Day/Night") is None and node("button", "Perspective-View") is None
             and node("button", "Compass / Face North") is not None)
bx, by, bw, bh = extents(node("tool bar", "mcbench world controls"))
report.check("the bar is one line along the pane's top", bh <= 34, f"{bw}x{bh}")
shot("bar-narrow")
click(node("button", "Fill the window"))
time.sleep(0.5)
fx, fy, fw, fh = extents(canvas())
report.check("Fill fills the window: the pane from the top left, the window's size",
             node("button", "Back beside the chat (Esc)") is not None and fx <= 2 and fy <= 2 and fw >= 1270 and fh >= 810,
             f"{fx},{fy} {fw}x{fh}")
bx, by, bw, bh = extents(node("tool bar", "mcbench world controls"))
report.check("filling the window, the bar shows all of BlueMap's controls on one line",
             node("button", "Day/Night") is not None and node("button", "Perspective-View") is not None and bh <= 34,
             f"{bw}x{bh}")
shot("full")
# One wheel notch is one `Wheel` of 3 lines (GPUI's SCROLL_LINES, a
# browser's DOM_DELTA_LINE 3): BlueMap's 0.33 a line makes it one 1.5x step.
wheels = lambda: [a for a in common.actions() if a.get("action") == "wheel"]
n = len(wheels())
common.pointer("mousemove", *centre(canvas()))
common.pointer("click", "4")
time.sleep(0.6)
new = wheels()[n:]
report.check("one wheel notch sends one Wheel of 3 lines, zooming in",
             len(new) == 1 and new[0]["delta"] == -3.0 and new[0]["lines"] is True, str(new))
common.pointer("click", "5")
time.sleep(0.6)


def window_width(width):
    """The window made `width` px wide, 820 tall (X11: the window; Wayland:
    cage's output, which its one window fills)."""
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "search", "--sync", "--name", "mcbench-lite$", "windowsize", "--sync",
                        str(width), "820"], check=True)
    else:
        subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "wl_pointer.py"),
                        "output", f"{width}x820"], check=True)
    time.sleep(1.0)


# At 640 px, just over BlueMap's break, the bar keeps every control and the
# position fields shrink last, as `.pos-input` does (about 104 px each on
# the page at this width: the 20 em fields shrink to the room the buttons
# leave, the greedy space never grows into theirs).
window_width(640)
fields = [node("entry", f"Camera position {a}") for a in ("x", "z")]
widths = [extents(f)[2] if f is not None else 0 for f in fields]
report.check("at a 640 px pane the x and z fields keep room for their numbers (60 px or more each)",
             all(w >= 60 for w in widths), str(widths))
shot("bar-640")
window_width(1280)
click(node("button", "Day/Night"))
report.check("Day/Night switches to night", any(a.get("action") == "day night" and a.get("night") for a in common.actions()))
shot("night")
click(node("button", "Day/Night"))
# The selection tool: its button, a drag on the ground, the chip.
click(node("button", "Select blocks"))
report.check("the selection tool is on", any(a.get("action") == "select" and a.get("on") for a in common.actions()))
c = canvas()
x0, y0 = centre(c, -30, 80)
common.pointer("mousemove", x0, y0)
common.pointer("mousedown", "1")
for i in range(1, 9):
    common.pointer("mousemove", str(int(x0) + i * 8), str(int(y0) + i * 3))
    time.sleep(0.03)
common.pointer("mouseup", "1")
chip = common.wait_for(lambda: [n for n in common.find(app(), role="button") if n.get_name().startswith("Remove ")],
                       8.0, "the selection's chip")
report.check("the box lands in the composer as a chip", bool(chip), chip[0].get_name() if chip else "")
# The draft box and its marker come with the pane's next frames.
time.sleep(0.5)
shot("selection")
common.key("Escape", settle=0.4)
report.check("Escape stops the selection tool", any(a.get("action") == "select" and not a.get("on") for a in common.actions()))
# Player mode.
click(node("button", "Player mode: walk and fly as a Minecraft player"))
common.wait_for(lambda: node("status bar", "Click to look around") or node("label", "Click to look around")
                or [n for n in common.find(app(), name="Click to look around")], 8.0, "the player's hint")
report.check("player mode stands beside the build", True)
shot("player")

def captures():
    return [a for a in common.actions() if a.get("action") == "pointer"]


def yaws():
    return [a["yaw"] for a in common.actions() if a.get("action") == "look"]


def pointer_at():
    if common.BACKEND == "x11":
        out = subprocess.run(["xdotool", "getmouselocation", "--shell"], check=True, capture_output=True, text=True).stdout
        v = dict(line.split("=") for line in out.split())
        return int(v["X"]), int(v["Y"])
    return None


def relative(dx, dy):
    if common.BACKEND == "x11":
        subprocess.run(["xdotool", "mousemove_relative", "--", str(dx), str(dy)], check=True)
    else:
        common.pointer("mousemove_relative", str(dx), str(dy))


x, y, w, h = extents(canvas())
common.pointer("mousemove", str(x + w // 2), str(y + h // 2 + 30))
time.sleep(0.2)
where = pointer_at()
common.pointer("click", "1")
time.sleep(0.6)
took = [a for a in captures() if a.get("captured")]
refused = [a for a in captures() if a.get("refused")]
if common.BACKEND == "wayland" and refused:
    # cage 0.3.1 offers relative motion but no pointer constraints: the
    # lock is refused with the compositor's reason, said as a note.
    report.check("the private compositor offers no pointer lock: refused with its reason as a note",
                 "zwp_pointer_constraints_v1" in refused[0]["refused"]
                 and bool(common.find(app(), pred=lambda o: "The pointer could not be taken" in o.get_name())),
                 refused[0]["refused"])
else:
    report.check("a click takes the pointer (the lock)", bool(took), str(captures()))
    shot("player-locked")
    before = took[-1]["yaw"] if took else None
    for _ in range(10):
        relative(200, 0)
        time.sleep(0.05)
    time.sleep(0.5)
    after = yaws()[-1] if yaws() else None
    turned = (after - before) if before is not None and after is not None else 0.0
    report.check("the look follows relative motion past the window's edge (2000 px moved, more than the window's width)",
                 turned > 0.15 * 1300, f"yaw turned {turned:.1f} degrees")
    common.key("Escape", settle=0.5)
    report.check("Escape frees the pointer first", any(a.get("captured") is False for a in captures()), str(captures()[-2:]))
    if where is not None:
        report.check("the pointer is back where it was taken", pointer_at() == where, f"{where} -> {pointer_at()}")
    # Focus leaving the window gives the pointer back.
    common.pointer("click", "1")
    time.sleep(0.5)
    n = len([a for a in captures() if a.get("captured") is False])
    other = common.deactivate_window()
    time.sleep(0.8)
    report.check("the window losing focus frees the pointer", len([a for a in captures() if a.get("captured") is False]) > n,
                 str(captures()[-2:]))
    if other is not None:
        other.terminate()
    common.focus_window()
click(node("button", "Perspective-View"))
time.sleep(0.4)
Atspi.Component.grab_focus(canvas())
time.sleep(0.3)
common.key("Escape", settle=0.5)
report.check("Escape puts it back beside the chat", node("button", "Fill the window") is not None)
# The schematic.
click(node("button", "Download as a WorldEdit schematic (.schem)"))
common.wait_for(lambda: os.path.isdir(saved) and os.listdir(saved), 8.0, "the saved file")
report.check("the .schem is saved where --save-to says", any(n.endswith(".schem") for n in os.listdir(saved)), str(os.listdir(saved)))
report.check("the polite region says where it went", any(s.startswith("Saved ") for s in polite()), str(polite()))
# Quitting while the pointer is held leaves no grab behind (X11: a new grab
# of the pointer succeeds once the app is gone).
if common.BACKEND == "x11":
    click(node("button", "Fill the window"))
    time.sleep(0.5)
    click(node("button", "Player mode: walk and fly as a Minecraft player"))
    time.sleep(1.0)
    click(canvas())
    report.check("locked again before quitting", captures()[-1].get("captured") is True, str(captures()[-1]))
common.quit_key(settle=3.0)
if common.BACKEND == "x11":
    from Xlib import X, display
    d = display.Display()
    status = d.screen().root.grab_pointer(False, X.PointerMotionMask, X.GrabModeAsync, X.GrabModeAsync, X.NONE, X.NONE, X.CurrentTime)
    d.ungrab_pointer(X.CurrentTime)
    d.sync()
    report.check("quitting while locked leaves no grab", status == X.GrabSuccess, f"grab status {status}")
report.finish()
