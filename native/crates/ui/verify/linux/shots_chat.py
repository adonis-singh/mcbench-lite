"""Pictures of the chat column for the parity pass (par-chat): a full turn's
tool rows with one program open, the header's model warning and the
composer, in the theme the run's preferences name. Run with `--turn full
--rate-ms 15 --model-facts <unknown|no-tools> --preferences <file>`; the one
argument is the pictures' suffix (`dark` or `light`)."""

import sys
import time

import common

Atspi = common.Atspi
report = common.Report("shots_chat")
theme = sys.argv[1]


def app():
    return common.app()


def names(role=None):
    return [o.get_name() for o in common.find(app(), role)]


def press(name, role=None):
    found = common.find(app(), role, name)
    if not found:
        raise AssertionError(f"no {role} {name!r}")
    Atspi.Action.do_action(found[-1], 0)
    time.sleep(0.6)


def focus_composer():
    entry = common.one(app(), None, "Message", pred=lambda o: o.get_role_name() == "entry")
    Atspi.Component.grab_focus(entry)
    time.sleep(0.3)


common.wait_for(lambda: any(n.startswith("Redstone lamp with a lever") for n in names()), 20, "the chat list")
Atspi.Action.do_action(common.row(app(), "Redstone lamp with a lever"), 0)
time.sleep(1.0)
warning = [n for n in names() if n in ("unknown model", "no tools")]
report.check("the header shows the model warning", bool(warning), repr(warning))
focus_composer()
common.type_text("Add a second lamp")
common.key("Return")
common.wait_for(lambda: any(n == "Stop generating" for n in names()), 10, "the turn to start")
common.wait_for(lambda: not any(n == "Stop generating" for n in names()), 60, "the turn to end")
time.sleep(1.0)
fold = [n for n in names() if n.startswith("Worked")]
report.check("the turn folded", bool(fold), repr(fold))
press(fold[-1])
built = [n for n in names() if n.startswith("Built") or n.startswith("Ran command") or n.startswith("Inspected")]
report.check("the fold holds tool rows", bool(built), repr(built))
opened = [n for n in built if n.startswith("Built")]
if opened:
    press(opened[-1])
time.sleep(0.8)
common.screenshot(f"chat-tools-{theme}.png")
focus_composer()
common.type_text("A tower beside the lamp, three blocks high")
time.sleep(0.6)
common.screenshot(f"chat-composer-{theme}.png")
common.quit_key(settle=1.5)
report.finish()
