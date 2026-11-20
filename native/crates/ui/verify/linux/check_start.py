"""The start place before the core's first lists (start-place agent) through
AT-SPI and real keys: the app started at a chat (or, with -bench, a bench's
comparison) with `--lists-ms N` holding its first lists back, as the real
core's cold start does. Before the lists: that place, the composer focused,
typed text kept; a picture of the wait. After them: the same place, its
title named, the text still in its composer, no second open.

    check_start.py [theme[-bench]]   theme names the pictures (dark, light)
"""

import json
import os
import sys
import time

import common

arg = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "dark"
theme, _, mode = arg.partition("-")
mode = mode or "chat"
report = common.Report(f"start-{mode}-{theme}")
app = common.app()
time.sleep(0.8)
common.focus_window()


def actions():
    path = os.path.join(os.environ["MCBENCH_OUT"], "actions.jsonl")
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


def opened(action):
    return [a for a in actions() if a.get("action") == action]


def focused_name():
    f = common.focused(app)
    return f.get_name() if f is not None else ""


lists_seen = lambda: any(a.get("action") == "start place" for a in actions())
report.check("the start place opened before the lists", lists_seen(), str([a.get("action") for a in actions()][:12]))
if mode == "chat":
    report.check("the chat's place: its transcript region and the composer focused",
                 bool(common.find(app, name="Conversation")) and focused_name() == "Message", focused_name())
else:
    report.check("the comparison's place: the bench's composer focused", focused_name() == "Message", focused_name())
common.screenshot(f"start-place-{mode}-wait-{theme}.png")
common.type_text("Make it taller", settle=0.4)
# The lists come (--lists-ms): the place goes on.
time.sleep(4.0)
title = "Castle gate" if mode == "chat" else "Castle with a moat"
frame = common.one(app, role="frame").get_name()
report.check("the lists name it: its title in the window's title", frame == f"{title} · mcbench-lite", frame)
message = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))
text = ""
if message:
    from common import Atspi
    t = message[0]
    text = Atspi.Text.get_text(t, 0, Atspi.Text.get_character_count(t))
report.check("the typed text is still in that place's composer", "Make it taller" in text, repr(text))
if mode == "chat":
    report.check("no second open of the chat", len(opened("opened chat")) == 1, str(len(opened("opened chat"))))
else:
    report.check("the comparison is still the place", any(a.get("place") == "/bench/bench-castle" for a in opened("opened")), "")
common.screenshot(f"start-place-{mode}-after-{theme}.png")
common.quit_key(settle=2.0)
report.finish()
