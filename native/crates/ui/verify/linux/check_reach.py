"""What the macOS pass of the shell and chat found (chat-fix-2), through
AT-SPI and real keys on the private display:

    check_reach.py          (all.sh: `--archive-ms 3000 --unreadable chat-farm`)
                            Cancel edit is described by its tip; a button of a
                            transcript row kept above the visible part is
                            brought into view by ScrollTo, pressed by its
                            action and focused by GrabFocus, each scrolling
                            its row in first; the filter menu's values have
                            their group's position and size; the model list
                            counts its models only (each star inside its
                            option); `Set up providers` and `Compare several
                            in Bench`; the archived section's `Loading…`; a
                            chat that cannot be read and `Read again`; `No
                            matches`; a place not built yet (`New bench`,
                            `Leaderboard`, `Prompts`); `No prompts match.`
    check_reach.py fails    (all.sh: `--search-fails`) a failed search's
                            sentence is said once in the polite region, in
                            the sidebar's search and in the palette
    check_reach.py shots    the composer's footer and the open model picker
                            (screenshots only, for comparing builds)

Each step is read back from AT-SPI's tree, its events and the dev binary's
action log (reach.json)."""

import sys
import time

from gi.repository import GLib

import common

Atspi = common.Atspi
part = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "main"
report = common.Report({"main": "reach", "fails": "reach-fails", "shots": "reach-shots"}[part])
common.X_KEYS.update({"ctrl+k": "ctrl+k", "ctrl+1": "ctrl+1", "ctrl+2": "ctrl+2"})

# A walk of the tree can meet an object the app removed meanwhile (a row
# scrolled away): walk again, up to five times.
_find = common.find


def _find_again(*args, **kwargs):
    for attempt in range(5):
        try:
            return _find(*args, **kwargs)
        except GLib.GError:
            if attempt == 4:
                raise
            time.sleep(0.3)


common.find = _find_again


def app():
    return common.app()


def names(role=None):
    return [o.get_name() for o in common.find(app(), role)]


def logged(action, **fields):
    return any(a["action"] == action and all(a.get(k) == v for k, v in fields.items()) for a in common.actions())


def press(obj, settle=0.8):
    Atspi.Action.do_action(obj, 0)
    time.sleep(settle)


def grab(obj, settle=0.5):
    Atspi.Component.grab_focus(obj)
    time.sleep(settle)


def extents(obj):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    return e.x, e.y, e.width, e.height


def attrs(obj):
    return obj.get_attributes() or {}


common.app()
time.sleep(2.0)
common.focus_window()

if part == "shots":
    common.screenshot("composer.png")
    trigger = common.one(app(), "button", pred=lambda o: o.get_name().startswith("Chat model: "))
    press(trigger, settle=1.2)
    common.screenshot("model-picker.png")
    report.check("the model picker opened", len(common.find(app(), "dialog", "Chat model")) == 1)
    common.key("Escape", settle=0.6)
    common.quit_key(settle=1.5)
    report.finish()

if part == "fails":
    events = []

    def on_event(event):
        try:
            source = event.source
            role = source.get_role_name() if source else "?"
        except Exception:  # an object gone by the time the event is read
            role = "gone"
        events.append((event.type, role, str(event.any_data) if event.any_data is not None else ""))

    listener = Atspi.EventListener.new(on_event)
    for kind in ["object:property-change:accessible-name", "object:announcement"]:
        listener.register(kind)

    def pump(seconds):
        context = GLib.MainContext.default()
        end = time.time() + seconds
        while time.time() < end:
            while context.pending():
                context.iteration(False)
            time.sleep(0.02)

    FAILURE = "Search failed: the index could not be read (a fixture error)"

    def said():
        return [e for e in events if FAILURE in e[2] and (e[0] == "object:announcement" or e[1] == "status bar")]

    grab(common.one(app(), "entry", "Search chats"))
    common.type_text("zz", settle=0.2)
    pump(1.5)
    report.check("the failed search's sentence is shown", FAILURE in names("label"), repr(names("label")))
    first = len(said())
    report.check("it is said in the polite region when it appears", first >= 1, repr(events[-6:]))
    common.screenshot("reach-failed-search.png")
    common.type_text("q", settle=0.2)
    pump(1.5)
    report.check("the next query failing the same way is not said again", len(said()) == first, repr(said()))
    common.key("Escape", settle=0.6)
    common.key("ctrl+k", settle=0.8)
    before = len(said())
    common.type_text("gate", settle=0.2)
    pump(1.5)
    report.check("the palette shows the sentence", FAILURE in names("label"), repr(names("label")))
    report.check("and says it once", len(said()) > before, repr(said()))
    common.screenshot("reach-failed-palette.png")
    common.key("Escape", settle=0.6)
    for kind in ["object:property-change:accessible-name", "object:announcement"]:
        listener.deregister(kind)
    common.quit_key(settle=1.5)
    report.finish()

# Item 1: Cancel edit's tip is its description.
edits = common.find(app(), "push button", "Edit and branch") or common.find(app(), "button", "Edit and branch")
press(edits[-1])
cancel = common.find(app(), None, "Cancel edit", pred=lambda o: o.get_role_name() in ("push button", "button"))
report.check("Cancel edit is a button described by its tip `Cancel edit (Esc)`",
             len(cancel) == 1 and cancel[0].get_description() == "Cancel edit (Esc)",
             repr([c.get_description() for c in cancel]))
if cancel:
    press(cancel[0])
report.check("Cancel edit cancels", logged("cancel edit") or not common.find(app(), "status bar", "Editing a message: sending it starts a new branch"))


# Item 2: a node of a row kept above the transcript's visible part.
def transcript():
    return common.one(app(), "landmark", "Conversation")


def viewport():
    x, y, w, h = extents(transcript())
    return y, y + h


def in_view(obj):
    top, bottom = viewport()
    _, y, _, h = extents(obj)
    return top < y + h / 2 < bottom


def button_above():
    """A button of a row wholly above the list's visible part, after moving
    the current message up from the end until a row above holds one."""
    articles = common.find(transcript(), "article")
    grab(articles[-1])
    common.key("End", settle=0.6)
    for _ in range(12):
        top, _ = viewport()
        for article in common.find(transcript(), "article"):
            _, y, _, h = extents(article)
            if y + h > top:
                continue
            buttons = [b for b in common.find(article, pred=lambda o: o.get_role_name() in ("push button", "button", "toggle button"))
                       if b.get_state_set().contains(Atspi.StateType.SENSITIVE)]
            if buttons:
                return buttons[0]
        common.key("Up", settle=0.5)
    raise AssertionError("no row kept above the visible part holds a button")


b = button_above()
name = b.get_name()
report.check(f"{name} (a row above the visible part) offers ScrollTo, and is out of view",
             not in_view(b) and not b.get_state_set().contains(Atspi.StateType.SHOWING), common.label(b))
Atspi.Component.scroll_to(b, Atspi.ScrollType.ANYWHERE)
time.sleep(0.8)
report.check(f"ScrollTo on {name} brings it into view", in_view(b), repr(extents(b)))
b = button_above()
name = b.get_name()
count = len(common.actions())
press(b, settle=1.0)
report.check(f"pressing {name} out of view scrolls it in and presses it",
             len(common.actions()) > count and in_view(b), repr(common.actions()[count:]) + " " + repr(extents(b)))
b = button_above()
name = b.get_name()
grab(b, settle=0.8)
f = common.focused(app())
report.check(f"GrabFocus on {name} out of view focuses it in view",
             f is not None and f.get_name() == name and in_view(f), common.label(f))
common.screenshot("reach-offscreen.png")

# Item 3: the filter menu's values by group; the model list's models.
grab(common.one(app(), "button", "Filter"))
common.key("Return", settle=0.6)
menu = common.find(app(), "menu", "Filter")
groups = {}
if menu:
    for group in common.find(menu[0], "panel"):
        groups[group.get_name()] = [(attrs(i).get("posinset"), attrs(i).get("setsize")) for i in common.find(group, "check menu item")]
size = groups.get("Size", [])
report.check("the filter menu's Size values say their place in Size",
             size and [p for p, _ in size] == [str(i + 1) for i in range(len(size))] and all(s == str(len(size)) for _, s in size),
             repr(groups))
report.check("every group counts its own values", all(
    [p for p, _ in v] == [str(i + 1) for i in range(len(v))] and all(s == str(len(v)) for _, s in v) for v in groups.values()
), repr(groups))
common.key("Escape", settle=0.6)

trigger = common.one(app(), "button", pred=lambda o: o.get_name().startswith("Chat model: "))
press(trigger, settle=1.2)
listbox = common.find(app(), "list box", "Chat model")
options = common.find(listbox[0], "list item") if listbox else []
children = [listbox[0].get_child_at_index(i).get_role_name() for i in range(listbox[0].get_child_count())] if listbox else []
report.check("the model list's children are its models only",
             children and all(c == "list item" for c in children), repr(children))
report.check("each model says its place among the models",
             [(attrs(o).get("posinset"), attrs(o).get("setsize")) for o in options] == [(str(i + 1), str(len(options))) for i in range(len(options))],
             repr([(o.get_name(), attrs(o).get("posinset"), attrs(o).get("setsize")) for o in options]))
report.check("each model's star is inside its option",
             all(any(c.get_name().startswith("Favorite ") for c in common.find(o, pred=lambda x: x.get_role_name() in ("push button", "toggle button"))) for o in options),
             repr([o.get_name() for o in options]))
common.screenshot("reach-model-picker.png")

# Item 7: the picker's two buttons.
set_up = common.find(app(), None, "Set up providers", pred=lambda o: o.get_role_name() in ("push button", "button"))
report.check("Set up providers is a button", len(set_up) == 1)
if set_up:
    press(set_up[0], settle=1.0)
report.check("it closes the picker and opens Settings",
             not common.find(app(), "dialog", "Chat model") and len(common.find(app(), "dialog", "Settings")) == 1)
common.key("Escape", settle=0.8)
trigger = common.one(app(), "button", pred=lambda o: o.get_name().startswith("Chat model: "))
press(trigger, settle=1.2)
compare = common.find(app(), None, "Compare several in Bench", pred=lambda o: o.get_role_name() in ("push button", "button"))
report.check("Compare several in Bench is a button", len(compare) == 1)
if compare:
    press(compare[0], settle=1.0)
report.check("it goes to the new bench, a place not built yet",
             not common.find(app(), "dialog", "Chat model") and len(common.find(app(), "landmark", "New bench")) == 1,
             repr(names("landmark")))
common.key("ctrl+1", settle=1.0)

# The archived section's Loading… (the binary answers the archive after 3 s).
toggle = common.one(app(), "button", "Show archived")
press(toggle, settle=0.3)
report.check("the archived section says Loading… while the archive is read", "Loading…" in names("label"), repr(names("label")))
common.screenshot("reach-loading.png")
time.sleep(3.5)
report.check("the rows replace it once read", "Loading…" not in names("label") and any(n.startswith("Old windmill, ") for n in names("list item")), repr(names("list item")))
press(common.one(app(), "button", "Hide archived"), settle=0.5)

# A chat that cannot be read.
press(common.row(app(), "Wheat farm"), settle=1.0)
alerts = [n for n in names("notification") if n.startswith("This chat could not be read: ")]
report.check("a chat that cannot be read says so", alerts == ["This chat could not be read: the stand-in cannot read this chat (a fixture error)"], repr(names("notification")))
again = common.find(app(), None, "Read again", pred=lambda o: o.get_role_name() in ("push button", "button"))
report.check("with a Read again button", len(again) == 1)
if again:
    press(again[0], settle=1.0)
report.check("Read again reads it again", logged("read again", chat="chat-farm"))
common.screenshot("reach-unreadable.png")

# No matches.
grab(common.one(app(), "entry", "Search chats"))
common.type_text("zzqqx", settle=1.0)
report.check("a search that finds nothing says No matches", "No matches" in names("label"), repr(names("label")))
common.key("Escape", settle=0.6)

# The places not built yet, and No prompts match.
common.key("ctrl+2", settle=1.0)
report.check("Bench opens on the new bench, a region named New bench", len(common.find(app(), "landmark", "New bench")) == 1, repr(names("landmark")))
press(common.one(app(), "button", "Leaderboard"), settle=1.0)
report.check("Leaderboard is a region named Leaderboard", len(common.find(app(), "landmark", "Leaderboard")) == 1, repr(names("landmark")))
press(common.row(app(), "Wheat field"), settle=1.0)
report.check("a prompt opens the Prompts place, a region named Prompts", len(common.find(app(), "landmark", "Prompts")) == 1, repr(names("landmark")))
press(common.one(app(), "button", "Search prompts"), settle=0.6)
common.type_text("zzqq", settle=1.0)
report.check("a prompt search that finds nothing says No prompts match.", "No prompts match." in names("label"), repr(names("label")))
common.screenshot("reach-no-prompts.png")
common.key("Escape", settle=0.6)
common.quit_key(settle=1.5)
report.finish()
