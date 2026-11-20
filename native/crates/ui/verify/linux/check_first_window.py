"""The first window with no stored placement (run with MCBENCH_PLACE=first,
so no --size or --origin, on X11 with MCBENCH_SCREEN): it lies wholly inside
the screen, centred, shrunk to it when the screen is smaller than 1280 by
820. Xvfb has no window manager and no panel, so the visible area is the
screen. Its argument is the screen size, WxH. The origin checked is the
one the app asked for (see the 2-pixel note below)."""

import os
import sys
import time

import common

report = common.Report("first-window")
common.app()
time.sleep(1.5)
from Xlib import display

screen_w, screen_h = (int(v) for v in sys.argv[1].split("x"))
d = display.Display(os.environ["DISPLAY"])


def windows(window):
    yield window
    for child in window.query_tree().children:
        yield from windows(child)


found = [w for w in windows(d.screen().root) if (w.get_wm_name() or "").endswith("mcbench-lite")]
report.check("the app's window is found", len(found) == 1, repr(len(found)))
g = found[0].get_geometry()
at = found[0].translate_coords(d.screen().root, 0, 0)
# GPUI's X11 backend creates every window 2 pixels right of the origin it
# is given (upstream's workaround in gpui-pre-linux `x11/window.rs`
# `create_window`, `bounds.origin.x.0 + 2`): the origin the app asked for
# is 2 to the left of where X has it.
x, y = -at.x - 2, -at.y
box = f"{g.width}x{g.height} asked at {x},{y} (X has {x + 2},{y}) on {screen_w}x{screen_h}"
report.check("the window lies inside the screen", x >= 0 and y >= 0 and x + g.width <= screen_w and y + g.height <= screen_h, box)
want_w, want_h = min(1280, screen_w), min(820, screen_h)
report.check("its size is 1280 by 820 shrunk to the screen", (g.width, g.height) == (want_w, want_h), box)
report.check("it is centred", abs(x - (screen_w - g.width) / 2) <= 1 and abs(y - (screen_h - g.height) / 2) <= 1, box)
common.screenshot("first-window.png")
common.quit_key(settle=1.5)
report.finish()
