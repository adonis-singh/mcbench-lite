"""Two pictures on the real app (pane agent; real_pane.sh with
MCBENCH_REAL_DRIVER=real_footer_tip.py), X11: the divider's tip ("Drag to
resize") beside the pointer resting on the divider, and the composer's
footer with the model's name cut by an ellipsis once the divider is
dragged right until the chat column is narrow. Each picture is also
cropped around what it shows (tip-crop.png, footer-crop.png)."""

import time

from PIL import Image

import common

Atspi = common.Atspi
report = common.Report("real-footer-tip")
PANE = "Live Minecraft world"


def app():
    return common.app(60.0)


def node(name):
    found = common.find(app(), name=name)
    return found[0] if found else None


app()
common.focus_window()
common.wait_for(lambda: node("Live world"), 180.0, "the first real frame")
time.sleep(1.0)
e = node(PANE).get_extents(Atspi.CoordType.WINDOW)
divider = e.x - 3
mid = e.y + e.height // 2
if divider < 10:
    # The narrow window: no divider; the footer at the window's width.
    path = common.screenshot("footer-narrow.png")
    Image.open(path).crop((0, 620, 2 + e.width, 822)).save(common.out("footer-crop.png"))
    common.quit_key(settle=3.0)
    report.finish()
    raise SystemExit
common.pointer("mousemove", str(divider - 30), str(mid))
time.sleep(0.3)
common.pointer("mousemove", str(divider), str(mid))
time.sleep(1.5)
path = common.screenshot("tip.png")
Image.open(path).crop((divider - 120, mid - 60, divider + 220, mid + 80)).save(common.out("tip-crop.png"))
report.check("the divider's tip shows", node("Drag to resize") is not None, "")
# The divider right: the chat column narrows.
common.pointer("mousedown", "1")
for i in range(1, 31):
    common.pointer("mousemove", str(divider + i * 12), str(mid))
    time.sleep(0.03)
common.pointer("mouseup", "1")
common.pointer("mousemove", str(divider + 400), str(mid))
time.sleep(1.5)
path = common.screenshot("footer.png")
composer = [n for n in common.find(app(), role="text")]
e2 = node(PANE).get_extents(Atspi.CoordType.WINDOW)
im = Image.open(path)
im.crop((240, 660, e2.x + 4, 820)).save(common.out("footer-crop.png"))
report.check("the chat column narrowed", e2.x > e.x + 200, f"{e.x} -> {e2.x}")
common.quit_key(settle=3.0)
report.finish()
