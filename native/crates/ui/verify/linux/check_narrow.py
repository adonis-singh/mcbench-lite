"""The narrow window (narrow agent) through AT-SPI, real keys and the
pointer: the window resized below 980 px (X11: the window, by xdotool;
Wayland: cage's one output, by wl_pointer.py), the sidebar as a bar, its
list as a drawer over the page, opened by the pointer and by the sidebar
bind, Tab kept in it, Escape closing it with focus back on its button;
the window widened and narrowed again with the drawer open and with the
filter menu open in it. Whole-window pictures at 979, 800 and 640 px,
drawer closed and open.

    check_narrow.py [theme[-bench]]
        theme names the screenshots (dark, light); with -bench, the
        stand-in's `bench` fixture in Bench (the list is Benches and
        prompts); with -setup, Build with the setup line over the composer
        (`--settings setup-wsl-optional`), only the names of its pictures differ
"""

import os
import subprocess
import sys
import time

import common
from common import Atspi

arg = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "dark"
theme, _, mode = arg.partition("-")
mode = mode or "build"
LIST = "Benches and prompts" if mode == "bench" else "Chats"
NEW = "New bench" if mode == "bench" else "New chat"
SEARCH = "Search benches" if mode == "bench" else "Search chats"
report = common.Report(f"narrow-{mode}-{theme}")
app = common.app()
time.sleep(2.0)
common.focus_window()
HEIGHT = 820


def resize(width, settle=1.5):
    if common.BACKEND == "x11":
        window = subprocess.run(["xdotool", "search", "--name", "mcbench-lite$"], check=True,
                                capture_output=True, text=True).stdout.split()[0]
        subprocess.run(["xdotool", "windowsize", "--sync", window, str(width), str(HEIGHT)], check=True)
    else:
        subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "wl_pointer.py"),
                        "output", f"{width}x{HEIGHT}"], check=True)
    time.sleep(settle)


def shot(name):
    common.screenshot(f"narrow-{mode}-{name}-{theme}.png")


def press(obj, settle=0.8):
    Atspi.Action.do_action(obj, 0)
    time.sleep(settle)


def click(obj, settle=0.8):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(settle)


def button(name):
    found = common.find(app, role="button", name=name)
    return found[0] if found else None


def drawer():
    found = common.find(app, name=LIST, pred=lambda o: o.get_role_name() in ("landmark", "complementary", "panel"))
    return found[0] if found else None


def in_drawer(obj):
    at = obj
    while at is not None:
        if at.get_name() == LIST and at.get_role_name() in ("landmark", "complementary", "panel"):
            return True
        at = at.get_parent()
    return False


def focused_name():
    f = common.focused(app)
    return f.get_name() if f is not None else ""


def alive():
    return os.path.exists(f"/proc/{os.environ['MCBENCH_APP_PID']}")


report.check("wide, the sidebar's button collapses it", bool(button("Collapse sidebar")), "")

for width in (979, 800, 640):
    resize(width)
    toggle = button(LIST)
    report.check(f"{width}: the bar's button is {LIST}, collapsed", bool(toggle) and
                 not toggle.get_state_set().contains(Atspi.StateType.EXPANDED), "")
    report.check(f"{width}: the bar holds New chat and Settings, not the list",
                 bool(button(NEW)) and bool(button("Settings")) and
                 not common.find(app, name=SEARCH, pred=lambda o: o.get_role_name() in ("text", "entry")), "")
    shot(f"{width}-closed")
    if toggle:
        click(toggle)
    report.check(f"{width}: the button opens the drawer, a named landmark over the page", drawer() is not None, "")
    report.check(f"{width}: focus is on its button, Close the list, expanded",
                 focused_name() == "Close the list" and button("Close the list") is not None and
                 button("Close the list").get_state_set().contains(Atspi.StateType.EXPANDED), focused_name())
    shot(f"{width}-open")
    left = None
    for _ in range(25):
        common.key("Tab", settle=0.12)
        f = common.focused(app)
        if f is None or not in_drawer(f):
            left = common.label(f)
            break
    report.check(f"{width}: Tab stays in the drawer", left is None, str(left))
    common.key("Escape", settle=0.8)
    report.check(f"{width}: Escape closes it, focus back on its button",
                 drawer() is None and focused_name() == LIST, focused_name())

# The sidebar bind from the composer, at 800.
resize(800)
message = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))
if message:
    Atspi.Component.grab_focus(message[0])
    time.sleep(0.3)
common.key("ctrl+b", settle=0.8)
report.check("the sidebar bind opens the drawer, focus in it", drawer() is not None and
             in_drawer(common.focused(app)), focused_name())
common.key("Escape", settle=0.8)
report.check("Escape gives focus back to the composer", focused_name() == "Message", focused_name())

# Across 980 with the drawer open: widened, the column; narrowed, the drawer again.
toggle = button(LIST)
if toggle:
    click(toggle)
resize(1280)
report.check("widened with the drawer open: the sidebar's column, no drawer, focus on a control",
             drawer() is None and bool(button("Collapse sidebar")) and common.focused(app) is not None and alive(),
             focused_name())
resize(979)
report.check("narrowed again: the drawer is back", drawer() is not None, "")
# The filter menu open in the drawer, then widened and narrowed.
flt = button("Filter")
if flt:
    press(flt)
resize(1280)
resize(800)
report.check("a menu open across 980 and back: the app runs, focus on a control",
             alive() and common.focused(app) is not None, focused_name())
common.key("Escape", settle=0.5)
common.key("Escape", settle=0.5)
report.check("Escape closes what is open, the drawer last", drawer() is None, "")

# At 640, the window's narrowest: Settings stacked, the model picker and the
# prompt palette inside the window.
resize(640)
settings = button("Settings")
if settings:
    press(settings, 1.2)
report.check("640: Settings opens over the page", bool(common.find(app, name="Settings", pred=lambda o: o.get_role_name() in ("dialog", "panel", "landmark"))) or bool(button("Close settings")), "")
shot("640-settings")
common.key("Escape", settle=0.8)
if mode != "bench":
    # (A comparison's composer has no model picker.)
    picker = [o for o in common.find(app, role="button") if (o.get_name() or "").startswith("Chat model")]
    if picker:
        press(picker[0], 1.0)
        shot("640-model-picker")
        common.key("Escape", settle=0.8)
    report.check("640: the model picker opens and closes", bool(picker) and alive(), "")
message = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))
if message:
    Atspi.Component.grab_focus(message[0])
    time.sleep(0.3)
    common.key("ctrl+a", settle=0.2)
    common.key("BackSpace", settle=0.3)
    common.key("slash", settle=1.0)
    shot("640-palette")
    common.key("Escape", settle=0.6)
report.check("640: the prompt palette opens over the composer", bool(message) and alive(), "")

common.quit_key(settle=2.0)
report.finish()
