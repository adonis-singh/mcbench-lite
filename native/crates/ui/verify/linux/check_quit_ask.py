"""quit-ask: the question before the person's quit while builds run,
through AT-SPI, real keys and a real pointer, in a private session.

    check_quit_ask.py <theme>   one build (--running chat-gate): the quit
                                key asks (`A build is running`, its body,
                                focus on `Keep building`); a second quit key
                                does nothing new; Escape keeps the app, focus
                                back where it was; the close request asks;
                                Tab stays between the two buttons; Return on
                                Keep building keeps the app; by pointer, Keep
                                building keeps it and Quit quits within 1 s,
                                the core stopped once, the log's last line
                                the clean close
    check_quit_ask.py two       a chat and a bench model (FIXTURE=bench
                                --running chat-gate): the plural texts

The theme (`dark` or `light`, from the run's --preferences) names the
picture, `quit-ask-<theme>-<backend>.png`. The privacy proof is
session.sh's, before and after."""

import json
import os
import sys
import time

import common
from common import Atspi

mode = sys.argv[1]
assert mode in ("dark", "light", "two"), f"check_quit_ask.py dark|light|two, not {mode}"
report = common.Report("quit-ask-" + mode)
ONE = ("A build is running", "Quitting stops it. What it built so far stays in the chat.")
TWO = ("2 builds are running", "Quitting stops them. What they built so far stays in their chats.")


def app():
    return common.app()


def questions():
    return common.find(app(), pred=lambda o: o.get_role_name() in ("alert", "dialog")
                       and (o.get_name() or "").endswith("running"))


def question():
    found = questions()
    assert len(found) <= 1, f"{len(found)} questions"
    return (found[0].get_name(), found[0].get_description()) if found else None


def focused():
    f = common.focused(app())
    return (f.get_role_name(), f.get_name() or "") if f else None


def button(name):
    found = common.find(app(), "button", name)
    assert found, f"no button {name!r}"
    return found[-1]


def click(obj, settle=0.8):
    r = Atspi.Component.get_extents(obj, Atspi.CoordType.WINDOW)
    common.pointer("mousemove", str(r.x + r.width // 2), str(r.y + r.height // 2))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(settle)


def asks(n, what):
    common.wait_for(lambda: common.quit_asked() == n, 3, f"the question ({what})")
    time.sleep(0.5)


common.app()
time.sleep(2.0)
common.focus_window()
common.wait_for(lambda: common.find(app(), pred=lambda o: (o.get_name() or "").startswith("Castle gate")),
                20, "the lists")
texts = TWO if mode == "two" else ONE

if mode == "two":
    common.key("ctrl+q", settle=0)
    asks(1, "quit key")
    report.check("two builds: the plural title and body", question() == texts, repr(question()))
    common.key("Return", settle=0.6)
    report.check("Return on Keep building keeps the app", question() is None and not common.app_exited())
    common.quit_key()
    report.check("the app quits after Quit", common.app_exited())
    report.finish()

before = focused()
report.check("focus is somewhere before the quit", before is not None, repr(before))

# By keyboard: the quit key asks.
common.key("ctrl+q", settle=0)
asks(1, "quit key")
report.check("the quit key asks: one build's title and body", question() == texts, repr(question()))
report.check("focus opens on Keep building", focused() == ("button", "Keep building"), repr(focused()))
report.check("the question has a Quit button", bool(button("Quit")))
common.screenshot(f"quit-ask-{mode}-{common.BACKEND}.png")
# A second quit request does nothing new.
common.key("ctrl+q", settle=0.6)
report.check("a second quit key does nothing new", question() == texts and common.quit_asked() == 1
             and focused() == ("button", "Keep building") and not common.app_exited(), repr(focused()))
common.key("Escape", settle=0.6)
report.check("Escape keeps the app open, focus back", question() is None and focused() == before
             and not common.app_exited(), repr(focused()))

# The close request asks; Tab stays between the two buttons.
report.check("the app's window is found and asked to close", common.close_window() == 1)
asks(2, "close request")
report.check("the close request asks", question() == texts, repr(question()))
report.check("a second close request does nothing new", common.close_window() == 1)
time.sleep(0.6)
report.check("still one question, the app open", question() == texts and common.quit_asked() == 2
             and not common.app_exited())
common.key("Tab")
report.check("Tab goes to Quit", focused() == ("button", "Quit"), repr(focused()))
common.key("Tab")
report.check("Tab goes back to Keep building", focused() == ("button", "Keep building"), repr(focused()))
common.key("shift+Tab")
common.key("shift+Tab")
report.check("Shift+Tab stays inside too", focused() == ("button", "Keep building"), repr(focused()))
common.key("Return", settle=0.6)
report.check("Return on Keep building keeps the app, focus back", question() is None and focused() == before
             and not common.app_exited(), repr(focused()))

# By pointer: Keep building keeps the app, Quit quits.
common.key("ctrl+q", settle=0)
asks(3, "quit key for the pointer")
click(button("Keep building"))
report.check("a click on Keep building keeps the app", question() is None and not common.app_exited())
common.key("ctrl+q", settle=0)
asks(4, "quit key for Quit")
go = button("Quit")
r = Atspi.Component.get_extents(go, Atspi.CoordType.WINDOW)
common.pointer("mousemove", str(r.x + r.width // 2), str(r.y + r.height // 2))
time.sleep(0.2)
started = time.monotonic()
common.pointer("click", "1")
while not common.app_exited() and time.monotonic() - started < 5:
    time.sleep(0.01)
took = time.monotonic() - started
report.check("a click on Quit ends the app within 1 s", common.app_exited() and took <= 1.0, f"{took:.3f} s")
shutdowns = open(common.out("app.log")).read().count("[standin] shutdown")
report.check("the core was stopped once (Core::shutdown)", shutdowns == 1, f"{shutdowns} times")
lines = [json.loads(line) for line in open(common.out("actions.jsonl")) if line.strip()]
quits = [line for line in lines if line["action"] == "quit"]
report.check("one quit, after the question", len(quits) == 1 and quits[0]["asked"] is True, repr(quits))
report.check("the log's last line is the clean close", lines[-1]["action"] == "metrics", repr(lines[-1])[:120])
report.check("the scratch TMPDIR is empty", os.listdir(os.environ["TMPDIR"]) == [])
report.finish()
