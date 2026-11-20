"""The window's layout and places (the shell): the sidebar shows and hides
by keyboard (Ctrl+B) and by its buttons, its edge drags its width between
200 and 420 and collapses it under 140; the divider drags the chat column
between its limits; Back and Forward (Alt+Left, Alt+Right) move between
the chats opened; the window's name is the page's title form.

Run with the gate fixture at 1280x820 (run.sh)."""

import time

import common
from common import Atspi

report = common.Report("layout")
app = common.app()
time.sleep(2.0)
common.focus_window()
time.sleep(0.5)


def window():
    return common.one(app, role="frame")


def chats():
    return common.find(app, role="list", name="Chats")


def extents(obj):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    return e.x, e.y, e.width, e.height


def transcript_width():
    # The transcript region's width is the chat column's (less its padding).
    return extents(common.one(app, role="landmark", name="Conversation"))[2]


# The sidebar's header controls draw their icons (focus-03.png in the
# review: GPUI's svg paints only with a colour of its own, and the icons
# were blank). Each button's box must hold pixels well off the background.
def ink(obj, shot):
    from PIL import Image
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    scale = Image.open(shot).size[0] / 1280 if common.BACKEND == "wayland" else 1
    box = Image.open(shot).convert("L").crop((int(e.x * scale), int(e.y * scale),
                                              int((e.x + e.width) * scale), int((e.y + e.height) * scale)))
    return sum(1 for p in box.getdata() if p > 90)


shot = common.screenshot("sidebar-header.png")
for name in ["New chat", "Collapse sidebar", "Settings"]:
    count = ink(common.one(app, role="button", name=name), shot)
    report.check(f"{name}'s icon draws", count > 15, f"{count} bright pixels")

report.check("the window's name is the page's title form",
             window().get_name() == "Redstone lamp with a lever · mcbench-lite", window().get_name())

# Keyboard: Ctrl+B hides and shows the sidebar.
common.key("ctrl+b", settle=0.8)
report.check("Ctrl+B hides the sidebar", not chats())
open_button = common.find(app, role="button", name="Open sidebar")
report.check("the pane header offers Open sidebar while it is hidden", len(open_button) == 1)
common.key("ctrl+b", settle=0.8)
report.check("Ctrl+B shows it again", len(chats()) == 1)
# Its buttons, by their actions.
Atspi.Action.do_action(common.one(app, role="button", name="Collapse sidebar"), 0)
time.sleep(0.8)
report.check("Collapse sidebar hides it", not chats())
Atspi.Action.do_action(common.one(app, role="button", name="Open sidebar"), 0)
time.sleep(0.8)
report.check("Open sidebar shows it", len(chats()) == 1)

# Pointer: the edge is the 7 px at the sidebar's right (x 229 to 236).
before = transcript_width()


def drag(x0, x1, y=400):
    common.pointer("mousemove", str(x0), str(y))
    # A frame under the pointer first: the app hit-tests the press against
    # the last frame drawn, and on Wayland the pointer's first motion is
    # its entry into the window.
    time.sleep(0.3)
    common.pointer("mousemove", str(x0 + 1), str(y))
    time.sleep(0.3)
    common.pointer("mousedown", "1")
    for i in range(1, 7):
        common.pointer("mousemove", str(int(x0 + (x1 - x0) * i / 6)), str(y))
        time.sleep(0.05)
    common.pointer("mouseup", "1")
    time.sleep(0.8)


drag(233, 330)
list_width = extents(chats()[0])[2]
report.check("dragging the edge to 330 widens the sidebar", 300 <= list_width <= 330, f"list {list_width}")
drag(327, 700)
list_width = extents(chats()[0])[2]
report.check("it stops at 420", 390 <= list_width <= 420, f"list {list_width}")
drag(417, 60)
report.check("dragged under 140 it collapses", not chats())
common.pointer("mousemove", "3", "400")
common.pointer("click", "1")
time.sleep(0.8)
report.check("a click on the strip at the left edge opens it", len(chats()) == 1)
list_width = extents(chats()[0])[2]
report.check("at the width it had before collapsing", 390 <= list_width <= 420, f"list {list_width}")
drag(417, 236)
report.check("dragged back to 236", 200 <= extents(chats()[0])[2] <= 236)

# The divider: minmax(360px, 42%) of 1280 puts it at 236 + 537.6.
x = 236 + 538 + 2
drag(x, 236 + 500)
w = transcript_width()
report.check("the divider drags the chat column", abs(w - before) > 20 and w < before, f"{before} to {w}")
drag(236 + 502, 300)
narrow = transcript_width()
drag(236 + 322, 1279)
wide = transcript_width()
# The chat keeps 320, the world 360: 1280 - 236 - 5 - 360 = 679 at most.
report.check("the chat column stays between 320 and 679", narrow < wide and wide - narrow <= 679 - 320 + 2,
             f"{narrow} to {wide}")

# Back and Forward.
for name in ["Castle gate", "Wheat farm"]:
    Atspi.Action.do_action(common.row(app, name), 0)
    time.sleep(1.0)
common.focus_window()
common.key("alt+Left", settle=1.0)
report.check("Alt+Left goes back to Castle gate", window().get_name() == "Castle gate · mcbench-lite", window().get_name())
common.key("alt+Left", settle=1.0)
report.check("and to the first chat", window().get_name() == "Redstone lamp with a lever · mcbench-lite", window().get_name())
common.key("alt+Right", settle=1.0)
report.check("Alt+Right goes forward", window().get_name() == "Castle gate · mcbench-lite", window().get_name())
logged = [a["action"] for a in common.actions()]
report.check("the drags were logged", "sidebar" in logged and "split" in logged)
common.quit_key(settle=2.0)
report.finish()
