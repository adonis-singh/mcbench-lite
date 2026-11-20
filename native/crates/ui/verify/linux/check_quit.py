"""Quitting from every state: the quit key (Ctrl+Q) or the window's close
request (WM_DELETE_WINDOW on X11, the foreign-toplevel close on Wayland, as
common.close_window sends them) ends the app with status 0 within one
second, whatever is open.

    check_quit.py <state>-<key|close>

The states (all.sh gives each its app arguments):

    landing       the window as it opens
    streaming     a chat while its turn streams (--rate-ms 150 --running
                  chat-gate --join chat-gate --simulate-ms 300: the stand-in
                  also steps the list's states)
    composer      the composer focused with text and an attachment (--pick)
    model         the model picker open, focus in its search
    palette       the palette open (Ctrl+K), focus in its field
    settings      Settings open
    lightbox      the lightbox open on a message's picture (--pick)
    confirm       Delete permanently armed in the archived section
    held          a send held while setup runs (--setup-pending)
    drag          a selection being dragged in the transcript, the button
                  still down

After the quit: the app's process (MCBENCH_APP_PID, the one run.sh started)
has exited within one second (a zombie, or reaped: run.sh's shell reaps a
background child as soon as it exits and keeps its status for `wait`),
every process it had started is gone (by the pids listed before the quit),
the scratch TMPDIR is empty, and the action log's last line is the one the
app writes when it shuts down (`metrics`). The exit status is run.sh's
`wait` in exit.txt; all.sh prints it beside each case, and it must read
`app exit 0`.

quit-ask: in `streaming` a build runs, so the quit asks first
(check_quit_ask.py checks the question); this check answers `Quit` by
keyboard and times the exit from that answer."""

import json
import os
import sys
import time

import common

Atspi = common.Atspi
arg = sys.argv[1]
state, how = arg.rsplit("-", 1)
assert how in ("key", "close"), f"check_quit.py <state>-<key|close>, not {arg}"
report = common.Report("quit")
common.X_KEYS.update({"ctrl+k": "ctrl+k"})
pid = int(os.environ["MCBENCH_APP_PID"])


def app():
    return common.app()


def has(name, role=None):
    return bool(common.find(app(), role, name))


def press(name, role=None):
    found = common.find(app(), role, name)
    if not found:
        raise AssertionError(f"no {role} {name!r}")
    Atspi.Action.do_action(found[-1], 0)
    time.sleep(0.6)


def press_row(title):
    Atspi.Action.do_action(common.row(app(), title), 0)
    time.sleep(0.8)


def focus_composer():
    entry = common.one(app(), None, "Message", pred=lambda o: o.get_role_name() == "entry")
    Atspi.Component.grab_focus(entry)
    time.sleep(0.3)


def stat(p):
    """The fields of /proc/<p>/stat after the command name, or None."""
    try:
        with open(f"/proc/{p}/stat") as f:
            text = f.read()
    except (FileNotFoundError, ProcessLookupError):
        # Gone, or reaped between the open and the read.
        return None
    return text[text.rindex(")") + 2:].split()


def descendants(p):
    """Every process under `p`, by /proc's children lists."""
    found = []
    try:
        tasks = os.listdir(f"/proc/{p}/task")
    except FileNotFoundError:
        return found
    for task in tasks:
        try:
            with open(f"/proc/{p}/task/{task}/children") as f:
                kids = [int(k) for k in f.read().split()]
        except (FileNotFoundError, ProcessLookupError):
            continue
        for kid in kids:
            found.append(kid)
            found.extend(descendants(kid))
    return found


def text_node(starts):
    found = common.find(
        app(), "paragraph",
        pred=lambda o: "Text" in o.get_interfaces()
        and Atspi.Text.get_text(o, 0, Atspi.Text.get_character_count(o)).startswith(starts),
    )
    assert found, f"no paragraph starting {starts!r} on screen"
    return found[0]


def point_in(obj, offset):
    e = Atspi.Text.get_character_extents(obj, offset, Atspi.CoordType.WINDOW)
    return e.x + max(1, e.width // 2), e.y + e.height // 2


common.app()
time.sleep(2.0)
common.focus_window()

# Bring the app to the state.
if state == "landing":
    report.check("the landing is open", has("New chat", "button") or has("New chat"))
elif state == "streaming":
    press_row("Castle gate")
    common.wait_for(lambda: has("Stop generating"), 10, "the joined turn to stream")
    region = common.one(app(), "landmark", "Conversation")
    report.check("a turn streams", region.get_state_set().contains(Atspi.StateType.BUSY))
elif state == "composer":
    press_row("Redstone lamp with a lever")
    press("Attach file")
    common.wait_for(lambda: has("notes.txt, plain"), 10, "the attachment tile")
    focus_composer()
    common.type_text("Quit with this draft")
    entry = common.one(app(), None, "Message", pred=lambda o: o.get_role_name() == "entry")
    report.check(
        "the composer has focus, text and an attachment",
        entry.get_state_set().contains(Atspi.StateType.FOCUSED)
        and Atspi.Text.get_text(entry, 0, -1) == "Quit with this draft" and has("notes.txt, plain"),
        repr(Atspi.Text.get_text(entry, 0, -1)),
    )
elif state == "model":
    press_row("Redstone lamp with a lever")
    press("Chat model: claude-opus-5-5", "button")
    focus = common.focused(app())
    report.check("the model picker is open, focus in its search", focus is not None and focus.get_role_name() == "entry"
                 and focus.get_name() != "Message", common.label(focus))
elif state == "palette":
    common.key("ctrl+k", settle=0.8)
    report.check("the palette is open", has("Search everything", "dialog"))
elif state == "settings":
    press("Settings", "button")
    report.check("Settings is open", has("Settings", "dialog") or has("Settings", "heading"),
                 repr([common.label(o) for o in common.find(app(), "dialog")]))
elif state == "lightbox":
    press_row("Redstone lamp with a lever")
    press("Attach file")
    common.wait_for(lambda: has("door.png, jpeg"), 10, "the attachment tile")
    focus_composer()
    common.type_text("A door")
    common.key("Return")
    common.wait_for(lambda: has("door.png"), 15, "the sent picture")
    press("door.png")
    report.check("the lightbox is open", has("Picture", "dialog"))
elif state == "confirm":
    press("Show archived", "button")
    mill = common.find(app(), "list item", pred=lambda o: o.get_name().startswith("Old windmill, "))
    Atspi.Component.grab_focus(common.find(mill[0], "button", "Delete permanently")[0])
    time.sleep(0.4)
    common.key("space", settle=0.6)
    report.check("Delete permanently is armed", any(o.get_name() == "Old windmill, Delete for good?"
                                                    for o in common.find(app(), "list item")))
elif state == "held":
    press_row("Redstone lamp with a lever")
    focus_composer()
    common.type_text("Hold this")
    common.key("Return", settle=0.8)
    report.check("the send is held", has("Sends when the app is ready. 1 of 2 done.", "status bar"))
elif state == "drag":
    articles = common.find(app(), "article")
    Atspi.Component.grab_focus(articles[-1])
    time.sleep(0.3)
    common.key("Home", settle=0.5)
    start = point_in(text_node("A lamp only needs power"), 2)
    common.pointer("mousemove", str(start[0]), str(start[1]))
    time.sleep(0.2)
    common.pointer("mousedown", "1")
    time.sleep(0.2)
    for i in range(1, 7):
        common.pointer("mousemove", str(start[0] + 20 * i), str(start[1] + 12 * i))
        time.sleep(0.05)
    report.check("a selection is being dragged (the button is down)", True)
else:
    raise AssertionError(f"no state {state}")
common.screenshot(f"quit-{state}.png")

# Quit.
children = descendants(pid)
tmpdir = os.environ["TMPDIR"]
before = stat(pid)
report.check("the app is running before the quit", before is not None and before[0] != "Z")
started = time.monotonic()
if how == "key":
    common.key("ctrl+q", settle=0)
else:
    report.check("the app's window is found and asked to close", common.close_window() == 1)
# quit-ask: a build runs while streaming: the quit asks first.
if state == "streaming":
    common.wait_for(lambda: common.quit_asked() == 1, 2, "the quit question")
    time.sleep(0.3)
    common.key("Tab")
    started = time.monotonic()
    common.key("Return", settle=0)
fields = stat(pid)
while fields is not None and fields[0] != "Z" and time.monotonic() - started < 5:
    time.sleep(0.01)
    fields = stat(pid)
took = time.monotonic() - started
exited = fields is None or fields[0] == "Z"
report.check(f"the {'quit key' if how == 'key' else 'close request'} ends the app within 1 s", exited and took <= 1.0,
             f"{took:.3f} s" if exited else f"still running after {took:.1f} s")
# pane agent: GPUI's quit stopped the core once before the exit
# (`Core::shutdown`; the stand-in says so on stderr, into app.log).
shutdowns = open(common.out("app.log")).read().count("[standin] shutdown")
report.check("the core was stopped once before the exit (Core::shutdown)", shutdowns == 1, f"{shutdowns} times")
left = [c for c in children if stat(c) is not None and stat(c)[0] != "Z"]
report.check("no process it started is left", not left, f"{len(children)} before, alive: {left}")
report.check("the scratch TMPDIR is empty", os.listdir(tmpdir) == [], repr(os.listdir(tmpdir)))
lines = [json.loads(line) for line in open(common.out("actions.jsonl")) if line.strip()]
report.check("the log's last line is the clean close", bool(lines) and lines[-1]["action"] == "metrics",
             repr(lines[-1] if lines else None)[:120])
with open(common.out("quit-time.txt"), "w") as f:
    f.write(f"{state} {how} {common.BACKEND} exited={exited} took={took:.3f}\n")
report.finish()
