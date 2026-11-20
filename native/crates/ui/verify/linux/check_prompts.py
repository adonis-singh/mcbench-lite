"""The prompt library (bench agent) through AT-SPI, real keys and the
pointer, over the stand-in's `bench` fixture in Bench
(MCBENCH_APP_ARGS="--open /bench", FIXTURE=bench): the palette opened by
`/` in the composer, its sections and preview, Down and Return inserting a
prompt; the prompts button with the pointer, its search narrowing the list,
Escape closing only it; a prompt with a fill-in, typed into and sent; a
prompt opened from the sidebar into the editor, its title typed and saved
(the sidebar's row follows), a tag added, Improve with AI, (par-bench)
the AI menu's items, the note box closed by Escape, the tag field's
suggestions, a row dragged two rows down with the pointer. With
`failed` (MCBENCH_APP_ARGS adds --bench prompts-fail) the library's first
read fails and Try again reads it.

    check_prompts.py [theme|failed]
"""

import sys
import time

import common
from common import Atspi

mode = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "dark"
report = common.Report("prompts-" + mode)
app = common.app()
time.sleep(2.0)
common.focus_window()


def shot(name):
    common.screenshot(f"prompts-{name}-{mode}.png")


def press(obj, settle=0.8):
    Atspi.Action.do_action(obj, 0)
    time.sleep(settle)


def click(obj, settle=0.8):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(settle)


def text_of(obj):
    try:
        return Atspi.Text.get_text(obj, 0, -1)
    except Exception:
        return ""


def composer():
    found = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))
    return found[0] if found else None


def palette_open():
    return bool(common.find(app, role="dialog", name="Prompts"))


def options():
    box = common.find(app, role="list box", name="Saved prompts")
    return [o.get_name() for o in common.find(box[0], role="list item")] if box else []


if mode == "failed":
    alert = common.wait_for(lambda: common.find(app, name="Prompts failed to load."), 10, "the failed library")
    report.check("a library that cannot be read says so", bool(alert))
    shot("failed")
    retry = common.find(app, role="button", name="Try again")
    report.check("Try again is offered", bool(retry))
    if retry:
        press(retry[0], 1.5)
    report.check("Try again reads the library", bool(common.find(app, role="list", name="Prompts")), "")
    common.quit_key(settle=2.0)
    report.finish()

# The palette, by keyboard: `/` in the composer.
box = common.wait_for(composer, 15, "the composer")
Atspi.Component.grab_focus(box)
time.sleep(0.4)
common.key("slash", settle=1.0)
report.check("/ opens the palette", palette_open())
names = options()
report.check("favourites first, then the recent ones", names[:2] == ["Castle with a moat, favorite", "Wheat field"], str(names))
shot("palette")
common.key("Down", settle=0.4)
common.key("Return", settle=1.0)
report.check("Return inserts the highlighted prompt and closes", not palette_open(), "")
inserted = text_of(composer())
report.check("the prompt's text is in the message", "A small wheat farm with water in the middle." in inserted, repr(inserted))
report.check("its use is recorded", any(a.get("action") == "inserted prompt" for a in common.actions()), "")

# The prompts button, with the pointer; its search; Escape.
button = common.find(app, role="toggle button", name="Insert a saved prompt") or common.find(app, role="button", name="Insert a saved prompt")
report.check("the prompts button is there", bool(button))
if button:
    click(button[0], 1.0)
    report.check("the button opens the palette with its search", palette_open() and bool(common.find(app, name="Search prompts", pred=lambda o: o.get_role_name() in ("text", "entry"))), "")
    common.type_text("bridge", settle=1.0)
    report.check("its search narrows the list", options() == ["Rope bridge"] or options() == ["Two towers", "Rope bridge"], str(options()))
    common.key("Escape", settle=0.8)
    report.check("Escape closes only the palette", not palette_open() and bool(composer()), "")

# A prompt with a fill-in: a box in the one text input, the caret in it,
# its name said on entering; a send waits for it; typing fills it; Right at
# its end steps out and Tab comes back in.
def said(text):
    return bool(common.find(app, name=text, pred=lambda o: o.get_role_name() in ("status bar", "label", "notification")))


Atspi.Component.grab_focus(composer())
time.sleep(0.3)
common.key("ctrl+a", settle=0.2)
common.key("BackSpace", settle=0.4)
common.key("slash", settle=0.8)
common.type_text("castle", settle=0.8)
common.key("Return", settle=1.0)
report.check("entering the box says its name", said("Fill in: width"), "")
common.key("Return", settle=1.0)
report.check("a send with the box empty waits and says so", said("Fill in this part before sending") and "{" not in text_of(composer()), repr(text_of(composer())))
common.type_text("wide", settle=0.8)
filled = text_of(composer())
report.check("a fill-in is filled by typing", filled == "A castle with four corner towers and a wide moat.", repr(filled))
focus = common.focused(app)
report.check("the box is in the one text input", focus is not None and focus.get_name() == "Message" and Atspi.Text.get_caret_offset(focus) == len("A castle with four corner towers and a wide"), common.label(focus))
common.key("Right", settle=0.5)
common.key("Tab", settle=0.8)
report.check("Tab comes back into the box and says its name", said("Fill in: width"), "")
shot("filled")

# The editor, from the sidebar's row.
row = common.find(app, role="list item", name="Two towers")
report.check("the sidebar lists the library", bool(row))
if row:
    click(row[0], 1.5)
    report.check("the library's place opens", bool(common.find(app, name="Prompts", pred=lambda o: o.get_role_name() in ("panel", "landmark", "section"))), "")
    title = common.find(app, name="Title", pred=lambda o: o.get_role_name() in ("text", "entry"))
    report.check("the editor shows its title", bool(title) and text_of(title[0]) == "Two towers", "")
    if title:
        Atspi.Component.grab_focus(title[0])
        time.sleep(0.3)
        common.key("End", settle=0.2)
        common.type_text(" again", settle=1.5)
        report.check("the sidebar's row follows the edit", bool(common.find(app, role="list item", name="Two towers again")), "")
    tag = common.find(app, name="Add tag", pred=lambda o: o.get_role_name() in ("text", "entry"))
    if tag:
        Atspi.Component.grab_focus(tag[0])
        time.sleep(0.3)
        common.type_text("Stone", settle=0.3)
        common.key("Return", settle=1.0)
        report.check("a tag is added, lower case", bool(common.find(app, role="button", name="Remove tag stone")), "")
    # The generation model, chosen in its picker by keyboard.
    chooser = [o for o in common.find(app, role="button") if (o.get_name() or "").startswith(("Choose generation model", "Generation model"))]
    if chooser:
        press(chooser[0], 1.2)
        listed = common.find(app, role="list box", name="Generation model")
        first = common.find(listed[0], role="list item")[:1] if listed else []
        if first:
            Atspi.Component.grab_focus(first[0])
            time.sleep(0.3)
            common.key("Return", settle=1.0)
    improve = common.find(app, role="button", name="Improve with AI")
    report.check("Improve with AI is offered", bool(improve), "")
    if improve:
        press(improve[0], 3.0)
        body = common.find(app, name="Prompt text", pred=lambda o: o.get_role_name() in ("text", "entry"))
        report.check("the improved text replaces the body", bool(body) and "light it with lanterns" in text_of(body[0]), "")
    shot("editor")
    # par-bench: the AI controls as the page's: the menu's items with their
    # lines, the note box closed by Escape, the tag field's suggestions.
    mode_button = common.find(app, role="button", name="Choose what the AI works from")
    if mode_button:
        press(mode_button[0], 0.8)
        items = [o.get_name() for o in common.find(app, role="radio menu item")]
        report.check("the AI menu's items say what each does",
                     items == ["Improve the text Rewrites what is in the editor", "Write from a note Opens a box for a short description"], str(items))
        common.screenshot(f"bench-editor-ai-menu-{mode}.png")
        note_item = common.find(app, role="radio menu item", name="Write from a note Opens a box for a short description")
        if note_item:
            press(note_item[0], 0.8)
        write = common.find(app, role="button", name="Write with AI")
        if write:
            press(write[0], 0.8)
        note_box = lambda: common.find(app, name="Note for the AI", pred=lambda o: o.get_role_name() in ("text", "entry"))
        report.check("Write with AI opens the note box", bool(note_box()), "")
        common.screenshot(f"bench-editor-ai-note-{mode}.png")
        common.key("Escape", settle=0.8)
        report.check("Escape closes the note box", not note_box(), "")
    tag = common.find(app, name="Add tag", pred=lambda o: o.get_role_name() in ("text", "entry"))
    if tag:
        Atspi.Component.grab_focus(tag[0])
        time.sleep(0.3)
        common.type_text("fa", settle=0.8)
        box = common.find(app, role="list box", name="Tag suggestions")
        report.check("the tag field suggests the other prompts' tags",
                     bool(box) and [o.get_name() for o in common.find(box[0], role="list item")] == ["farm"], "")
        common.screenshot(f"bench-editor-tag-suggestions-{mode}.png")
        common.key("BackSpace", settle=0.2)
        common.key("BackSpace", settle=0.5)

# par-bench: a prompt dragged two rows down with the pointer: the rows give
# way while it is held, and 150 ms after the drop the order is the new one.
plist = common.find(app, role="list", name="Prompts")
prow = common.find(plist[0], role="list item") if plist else []
if len(prow) >= 3:
    lifted = prow[0].get_name()
    e = prow[0].get_extents(Atspi.CoordType.WINDOW)
    x, y = e.x + e.width // 3, e.y + e.height // 2
    common.pointer("mousemove", str(x), str(y))
    time.sleep(0.2)
    common.pointer("mousedown", "1")
    for dy in range(6, 78, 6):
        common.pointer("mousemove", str(x), str(y + dy))
        time.sleep(0.03)
    time.sleep(0.3)
    common.screenshot(f"bench-prompt-drag-{mode}.png")
    common.pointer("mouseup", "1")
    time.sleep(0.8)
    after = [o.get_name() for o in common.find(common.find(app, role="list", name="Prompts")[0], role="list item")]
    report.check("a prompt dragged two rows down lands there", len(after) > 2 and after[2] == lifted, f"{lifted!r} in {after}")

# The sidebar's row tools: the star, the delete that asks first, New prompt.
stars = common.find(app, role="button", name="Favorite prompt")
report.check("each row has its star", len(stars) >= 4, str(len(stars)))
before = len(common.find(app, role="button", name="Unfavorite prompt"))
if stars:
    press(stars[0], 1.0)
report.check("a star makes a favourite", len(common.find(app, role="button", name="Unfavorite prompt")) == before + 1, "")
rows_before = len(common.find(common.find(app, role="list", name="Prompts")[0], role="list item"))
delete = common.find(app, role="button", name="Delete prompt")
if delete:
    press(delete[-1], 0.6)
    confirm = common.find(app, role="button", name="Confirm delete")
    report.check("delete asks first", bool(confirm), "")
    if confirm:
        press(confirm[0], 1.2)
rows_after = len(common.find(common.find(app, role="list", name="Prompts")[0], role="list item"))
report.check("the prompt is deleted", rows_after == rows_before - 1, f"{rows_before} to {rows_after}")
new = common.find(app, role="button", name="New prompt")
if new:
    press(new[0], 1.2)
report.check("New prompt opens an empty prompt", bool(common.find(app, name="Prompt Untitled prompt")), "")

# A row moved by Alt+Down (the list not narrowed), then the tag filter in
# the search bar.
rows = common.find(common.find(app, role="list", name="Prompts")[0], role="list item")
if len(rows) >= 2:
    first = rows[0].get_name()
    Atspi.Component.grab_focus(rows[0])
    time.sleep(0.4)
    common.key("alt+Down", settle=1.0)
    names = [o.get_name() for o in common.find(common.find(app, role="list", name="Prompts")[0], role="list item")]
    report.check("Alt+Down moves the focused prompt one row down", len(names) > 1 and names[1] == first,
                 f"{first!r} focused {common.label(common.focused(app))} in {[r.get_name() for r in rows]} gives {names}")
search = common.find(app, role="button", name="Search prompts")
if search:
    press(search[0], 0.8)
    tagbutton = common.find(app, name="Filter by tag", pred=lambda o: o.get_role_name() in ("button", "toggle button"))
    report.check("the search bar offers the tag filter", bool(tagbutton), "")
    if tagbutton:
        press(tagbutton[0], 0.8)
        items = [o.get_name() for o in common.find(app, role="check menu item")]
        report.check("the tag menu lists the tags in order", items == sorted(items) and "castle" in items, str(items))
        castle = common.find(app, role="check menu item", name="castle")
        if castle:
            press(castle[0], 0.8)
            rows = [o.get_name() for o in common.find(common.find(app, role="list", name="Prompts")[0], role="list item")]
            report.check("a chosen tag narrows the list", rows and all(r in ("Castle with a moat", "Two towers", "Two towers again") for r in rows), str(rows))
            shot("tag-filter")
        common.key("Escape", settle=0.5)
        common.key("Escape", settle=0.5)
shot("rows")

common.quit_key(settle=2.0)
report.finish()
