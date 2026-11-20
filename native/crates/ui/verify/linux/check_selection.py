"""Selection and copy with a real pointer (X11, XTest): a drag that starts
in a table cell and runs on out of the table into the next blocks, a drag
from a paragraph through a list and a code block into the table, a
double-click on a word and a triple-click on a block, each copied with
Ctrl+C and read back from the private display's clipboard."""

import time

import common

Atspi = common.Atspi
report = common.Report("selection")


def extents(obj):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    return e.x, e.y, e.width, e.height


def text_of(obj):
    return Atspi.Text.get_text(obj, 0, Atspi.Text.get_character_count(obj))


def node(role, starts):
    found = common.find(common.app(), role, pred=lambda o: "Text" in o.get_interfaces() and text_of(o).startswith(starts))
    assert found, f"no {role} starting {starts!r} on screen"
    return found[0]


def point_in(obj, offset):
    """The window point of character `offset` of a text node (its middle)."""
    e = Atspi.Text.get_character_extents(obj, offset, Atspi.CoordType.WINDOW)
    return e.x + max(1, e.width // 2), e.y + e.height // 2


def drag(start, end):
    common.pointer("mousemove", str(start[0]), str(start[1]))
    time.sleep(0.2)
    common.pointer("mousedown", "1")
    time.sleep(0.2)
    steps = 12
    for i in range(1, steps + 1):
        x = start[0] + (end[0] - start[0]) * i // steps
        y = start[1] + (end[1] - start[1]) * i // steps
        common.pointer("mousemove", str(x), str(y))
        time.sleep(0.03)
    common.pointer("mouseup", "1")
    time.sleep(0.3)


def copy():
    common.key("ctrl+c", settle=0.5)
    return common.clipboard() or ""


common.app()
time.sleep(2.0)
common.focus_window()
# Bring the long answer in: focus the last message, Home, then Down twice.
articles = common.find(common.app(), "article")
Atspi.Component.grab_focus(articles[-1])
time.sleep(0.3)
common.key("Home", settle=0.5)
common.key("Down", settle=0.3)
common.key("Down", settle=0.3)
common.key("Down", settle=0.6)
common.screenshot("selection-start.png")

# 1. From inside a table cell, out of the table and into the quote below.
cell = node("table cell", "stone")
quote = node("paragraph", "A lamp only needs power")
drag(point_in(cell, 1), point_in(quote, 6))
common.screenshot("selection-cell-drag.png")
copied = copy()
report.check(
    "a drag from a cell runs on past the table",
    copied.startswith("tone\t9\t0\nredstone_lamp\t1\t15\nlever\t1\t0\nA lamp"),
    repr(copied[:80]),
)

# 2. From a paragraph through the list and the code into the table. The
# user message under the answer is taller since it has its tools (chat
# agent), so the paragraph may sit above the view: the wheel brings it in.
region = common.one(common.app(), "landmark", "Conversation")
top = extents(region)[1]
for _ in range(10):
    if point_in(node("paragraph", "A sign that says"), 2)[1] > top + 30:
        break
    common.pointer("mousemove", "500", "400")
    common.pointer("click", "4")
    time.sleep(0.4)
para = node("paragraph", "A sign that says")
header = node("column header", "Count")
drag(point_in(para, 2), point_in(header, 3))
common.screenshot("selection-blocks-drag.png")
copied = copy()
report.check("the drag starts mid-paragraph", copied.startswith("sign that says Pull me\nThe program\nfn build("), repr(copied[:60]))
report.check("code is copied verbatim", "fn build(world: &mut World) {\n    world.fill((0, 0, 0), (2, 0, 2), \"stone\");" in copied)
report.check("the table row is tab-separated", copied.endswith("Blocks used\nBlock\tCou"), repr(copied[-40:]))
# The picture: the selection is drawn on text only. Between the end of
# "Block" and the start of "Count" in the header row no pixel has the
# selection's colour (--waiting at 35% over the header's --raised).
from PIL import Image  # noqa: E402

shot = Image.open(common.out("selection-blocks-drag.png")).convert("RGB")
block = node("column header", "Block")
count = node("column header", "Count")
b = Atspi.Text.get_range_extents(block, 0, 5, Atspi.CoordType.WINDOW)
c = Atspi.Text.get_range_extents(count, 0, 5, Atspi.CoordType.WINDOW)
selected = (0x37, 0x50, 0x6A)
painted = 0
for x in range(b.x + b.width + 4, c.x - 4):
    for y in range(b.y + 2, b.y + b.height - 2):
        r, g, bl = shot.getpixel((x, y))
        if abs(r - selected[0]) < 14 and abs(g - selected[1]) < 14 and abs(bl - selected[2]) < 14:
            painted += 1
report.check("no selection drawn between the header cells' text", painted == 0, f"{painted} selection pixels between x {b.x + b.width} and {c.x}")
inside = 0
for x in range(c.x + 2, c.x + c.width // 2):
    r, g, bl = shot.getpixel((x, c.y + c.height // 2))
    if abs(r - selected[0]) < 30 and abs(g - selected[1]) < 30 and abs(bl - selected[2]) < 30:
        inside += 1
report.check("the selection is drawn on the selected text of the right-aligned cell", inside > 0, f"{inside} pixels")

# 3. A double-click selects a word, a triple-click a block.
word_at = point_in(node("paragraph", "That is all. Pull the lever"), 14)
common.pointer("mousemove", str(word_at[0]), str(word_at[1]))
common.pointer("click", "--repeat", "2", "--delay", "80", "1")
time.sleep(0.4)
copied = copy()
report.check("a double-click copies the word", copied == "Pull", repr(copied))
heading = node("heading", "Blocks used")
at = point_in(heading, 3)
common.pointer("mousemove", str(at[0]), str(at[1]))
common.pointer("click", "--repeat", "3", "--delay", "80", "1")
time.sleep(0.4)
copied = copy()
report.check("a triple-click copies the block", copied == "Blocks used", repr(copied))
common.screenshot("selection-triple.png")

# 4. Across messages: from the user's last question into the answer under it.
common.key("End", settle=0.6)
question = node("paragraph", "Thanks. Is the base")
answer = node("paragraph", "Yes: the base is still")
drag(point_in(question, 0), point_in(answer, 10))
copied = copy()
report.check("a drag across messages copies both", copied == "Thanks. Is the base still three by three?\nYes: the b", repr(copied))
common.quit_key(settle=1.5)
report.finish()
