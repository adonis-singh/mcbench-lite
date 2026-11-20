"""Player mode's pointer lock alone (vendor patch 50), for a compositor
that offers pointer constraints (wayland_lock.sh: sway in a container,
whose AT-SPI reports no focused state, so check_pane.py's keyboard walk
cannot run there). The same steps as check_pane.py's lock on X11: a click
on the world takes the pointer, the look follows relative motion past the
window's edge, Escape frees it first, the window losing focus frees it,
and the app quits while it holds the pointer. What the app did is read from
its action log (`pointer` and `look` lines).

    check_pane_lock.py <save folder>"""

import os
import sys
import time

import common

Atspi = common.Atspi
report = common.Report("pane-lock")
PANE = "Live Minecraft world"
PLAYER = "Player mode: walk and fly as a Minecraft player"


def app():
    return common.app()


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


def canvas():
    return common.wait_for(lambda: node(PANE), 20.0, "the pane's canvas")


def centre(obj, dy=0):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    return str(e.x + e.width // 2), str(e.y + e.height // 2 + dy)


def click(obj, dy=0):
    x, y = centre(obj, dy)
    common.pointer("mousemove", x, y)
    time.sleep(0.15)
    common.pointer("click", "1")
    time.sleep(0.5)


def captures():
    return [a for a in common.actions() if a.get("action") == "pointer"]


def yaws():
    return [a["yaw"] for a in common.actions() if a.get("action") == "look"]


# Headless sway has no keyboard (WLR_LIBINPUT_NO_DEVICES). A wtype call's
# virtual keyboard comes and goes within microseconds, before a busy app
# reads the seat's new keyboard, and its keys are lost; so one virtual
# keyboard stays for the key presses, as a desktop's does (it sleeps, and
# is ended before it would type). For the focus-loss step it is ended and
# the seat has no keyboard at all: the window's activity then comes from
# the toplevel's activated state alone (vendor patch 51).
import json
import subprocess


def keep_keyboard():
    return subprocess.Popen(["wtype", "-s", "3600000", "-M", "shift", "-m", "shift"])


def seat_capabilities():
    """sway's seat capabilities: 1 pointer, 2 keyboard."""
    seats = json.loads(subprocess.run(["swaymsg", "-t", "get_seats"], capture_output=True, text=True).stdout)
    return [seat.get("capabilities") for seat in seats]


keyboard = keep_keyboard()
app()
time.sleep(1.0)
common.focus_window()
canvas()
common.wait_for(lambda: node("Live world"), 15.0, "the live world's toolbar")
# Beside the chat the pane is under 576 px, where BlueMap's narrow bar
# leaves out the view switch: Fill first, as check_pane.py does.
click(node("Fill the window"))
click(common.wait_for(lambda: node(PLAYER), 8.0, "the view switch"))
common.wait_for(lambda: [n for n in common.find(app(), name="Click to look around")], 8.0, "the player's hint")
report.check("player mode stands beside the build", True)

click(canvas(), 30)
took = [a for a in captures() if a.get("captured")]
refused = [a for a in captures() if a.get("refused")]
report.check("a click takes the pointer (the lock)", bool(took) and not refused, str(captures()))
common.screenshot("lock-taken.png")
before = took[-1]["yaw"] if took else None
for _ in range(10):
    common.pointer("mousemove_relative", "200", "0")
    time.sleep(0.05)
time.sleep(0.5)
after = yaws()[-1] if yaws() else None
turned = (after - before) if before is not None and after is not None else 0.0
report.check("the look follows relative motion past the window's edge (2000 px moved, more than the window's width)",
             turned > 0.15 * 1300, f"yaw turned {turned:.1f} degrees")
common.key("Escape", settle=0.5)
report.check("Escape frees the pointer first", bool(captures()) and captures()[-1].get("captured") is False,
             str(captures()[-2:]))
report.check("player mode stays after that Escape", node(PLAYER) is not None
             and [n for n in common.find(app(), name="Click to look around")] != [])

click(canvas(), 30)
n = len([a for a in captures() if a.get("captured") is False])
report.check("a second click takes it again", captures()[-1].get("captured") is True, str(captures()[-1:]))
keyboard.terminate()
keyboard.wait()
time.sleep(0.5)
caps = seat_capabilities()
report.check("the seat has no keyboard now (sway's capabilities have no keyboard bit)",
             bool(caps) and all(c & 2 == 0 for c in caps), str(caps))
other = common.deactivate_window()
time.sleep(0.8)


def focused_window():
    """The window sway says has focus (its app_id), and whether foot runs."""

    def walk(node):
        if node.get("focused"):
            return node.get("app_id") or node.get("name")
        for child in node.get("nodes", []) + node.get("floating_nodes", []):
            found = walk(child)
            if found:
                return found
        return None

    tree = subprocess.run(["swaymsg", "-t", "get_tree"], capture_output=True, text=True).stdout
    return walk(json.loads(tree)), other.poll() is None


common.screenshot("lock-other-window.png")
report.check("the window losing focus frees the pointer",
             len([a for a in captures() if a.get("captured") is False]) > n,
             f"{captures()[-2:]}; sway's focused window and foot running: {focused_window()}")
other.terminate()
other.wait()
keyboard = keep_keyboard()
time.sleep(1.0)
common.focus_window()

# Quitting while the pointer is held: the app ends (its constraint goes with
# its connection) and ends at once.
click(canvas(), 30)
report.check("locked again before quitting", captures()[-1].get("captured") is True, str(captures()[-1:]))
pid = int(os.environ["MCBENCH_APP_PID"])


def running():
    """Whether the app runs (an ended child of run.sh's script stays a
    zombie until the script waits for it, after this check)."""
    try:
        with open(f"/proc/{pid}/stat") as stat:
            return stat.read().rsplit(")", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


started = time.monotonic()
common.key("ctrl+q", settle=0.0)
while running() and time.monotonic() - started < 5.0:
    time.sleep(0.02)
gone = time.monotonic() - started
report.check("quitting while locked ends the app within a second", not running() and gone < 1.0, f"{gone:.2f} s")
keyboard.terminate()
keyboard.wait()
report.finish()
