"""The composer's pickers (mcbench-ui-dev --gallery, its Pickers section)
through AT-SPI and real keys: the model picker opens on the chosen model's
provider with focus in its search; typing searches every provider, names
the failing catalogue and waits for the slow one; Escape closes it back to
its trigger and the search's text stays; arrows and Enter choose a model
(remembered in the log); a star toggles; a failing tab says why; the effort
list's arrows and Enter choose an effort; the resolution slider steps with
the arrow keys.

    check_pickers.py            the checks above
    check_pickers.py timing     opens the model picker 20 times, types 20
                                characters into its search and scrolls its
                                list (40 wheel clicks), then reports the app's
                                own timings (picker-timing in the action log)

Run with MCBENCH_APP_ARGS=--gallery (run.sh); for the 400-model timing the
binary is a wrapper that sets MCBENCH_GALLERY_MODELS=400."""

import json
import statistics
import sys
import time

import common
from common import Atspi

mode = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "checks"
report = common.Report("pickers" if mode == "checks" else "pickers-timing")
app = common.app()
time.sleep(2.0)
common.focus_window()

MODEL = "claude-opus-5-5"


def button(name):
    return common.one(app, role="button", name=name)


def press(obj, settle=0.8):
    Atspi.Action.do_action(obj, 0)
    time.sleep(settle)


def focused_label():
    return common.label(common.focused(app))


def has_state(obj, state):
    return obj.get_state_set().contains(state)


def text_of(obj):
    try:
        return Atspi.Text.get_text(obj, 0, -1)
    except Exception:
        return None


def has_label(prefix):
    """A label (the list's states) whose name or text starts with `prefix`."""
    return bool(common.find(app, pred=lambda o: o.get_role_name() == "label"
                            and ((o.get_name() or "").startswith(prefix) or (text_of(o) or "").startswith(prefix))))


def options():
    """The model list's option names, in order."""
    lists = common.find(app, role="list box", name="Chat model")
    if not lists:
        return []
    return [o.get_name() for o in common.find(lists[0], role="list item")]


def wait_options(pred, what, timeout=6.0):
    try:
        return common.wait_for(lambda: pred(options()) and options(), timeout, what)
    except AssertionError:
        return options()


def search_text():
    entry = common.one(app, name="Search models", pred=lambda o: o.get_role_name() in ("entry", "text"))
    return Atspi.Text.get_text(entry, 0, -1)


if mode == "timing":
    trigger = button(f"Chat model: {MODEL}")
    # Every model listed: a search that matches every OpenRouter id ("/"),
    # kept between openings.
    press(trigger)
    common.type_text("/", settle=1.5)
    common.key("Escape", settle=0.8)
    for _ in range(20):
        common.key("Return", settle=0.6)  # the trigger has focus: opens
        common.key("Escape", settle=0.4)
    common.key("Return", settle=0.8)
    common.key("ctrl+a", settle=0.2)
    common.key("BackSpace", settle=0.5)
    for ch in "synthetic-model-12345":
        common.type_text(ch, settle=0.25)
    common.key("ctrl+a", settle=0.2)
    common.key("BackSpace", settle=0.5)
    common.type_text("/", settle=1.0)
    list_box = common.one(app, role="list box", name="Chat model")
    extents = list_box.get_extents(Atspi.CoordType.WINDOW)
    x, y = extents.x + extents.width // 2, extents.y + extents.height // 2
    common.pointer("mousemove", str(x), str(y))
    for button_no in ["5"] * 30 + ["4"] * 10:
        common.pointer("click", button_no)
        time.sleep(0.05)
    time.sleep(1.0)
    timings = {}
    listed = {}
    for a in common.actions():
        if a["action"] == "picker-timing":
            timings.setdefault(a["what"], []).append(a["ms"])
            listed[a["what"]] = max(listed.get(a["what"], 0), a["listed"])
    summary = {}
    for what, values in timings.items():
        values.sort()
        summary[what] = {
            "count": len(values),
            "p50_ms": round(statistics.median(values), 2),
            "p90_ms": round(values[int(len(values) * 0.9) - 1 if len(values) > 1 else 0], 2),
            "max_ms": round(max(values), 2),
        }
    with open(common.out("pickers-timing-summary.json"), "w") as f:
        json.dump({"most_listed": listed, "timings": summary}, f, indent=1)
    report.check("the picker was timed opening with 400 models listed",
                 "open" in summary and listed.get("open", 0) >= 400, json.dumps(summary) + " " + json.dumps(listed))
    report.check("keys in its search were timed", "key" in summary, json.dumps(summary.get("key")))
    report.check("its list's scrolling was timed", "scroll" in summary, json.dumps(summary.get("scroll")))
    common.quit_key(settle=2.0)
    report.finish()

# The footer's triggers.
for name in [f"Chat model: {MODEL}", "Resolution: Standard", "Reasoning effort: medium"]:
    found = common.find(app, role="button", name=name)
    report.check(f"{name} is a button", len(found) == 1, str(len(found)))
# chat-fix-2: the meter's percentage is its value alone, with no
# description, so a reader says it once.
meters = common.find(app, role="level bar", name="Context · 840k of 1M tokens")
report.check("the context meter is a level bar named by its tokens",
             len(meters) == 1, str([common.label(m) for m in common.find(app, role="level bar")]))
if meters:
    report.check("its value is the percentage, 0 to 100, and it has no description",
                 Atspi.Value.get_current_value(meters[0]) == 84.0 and Atspi.Value.get_minimum_value(meters[0]) == 0.0
                 and Atspi.Value.get_maximum_value(meters[0]) == 100.0 and meters[0].get_description() == "",
                 f"{Atspi.Value.get_current_value(meters[0])} {meters[0].get_description()!r}")
    report.check("its name holds no percentage", "%" not in meters[0].get_name(), meters[0].get_name())
# chat-fix-2: the resolution tags are labels described by their titles.
for tag, title in [("2×", "Resolution: High (2×)"), ("5×", "Resolution: Very high (5×)"), ("10×", "Resolution: Maximum (10×)")]:
    found = common.find(app, role="label", name=tag)
    report.check(f"the tag {tag} is a label described {title!r}",
                 len(found) == 1 and found[0].get_description() == title, repr([f.get_description() for f in found]))
chips = common.find(app, pred=lambda o: o.get_name() == "Near weekly limit" or text_of(o) == "Near weekly limit")
report.check("the limit chip is named by its text", len(chips) >= 1, str([common.label(c) for c in chips]))

# The model picker opens with focus in its search, on Claude Code.
trigger = button(f"Chat model: {MODEL}")
report.check("the trigger is collapsed", not has_state(trigger, Atspi.StateType.EXPANDED))
press(trigger)
report.check("it opens a dialog named Chat model", len(common.find(app, role="dialog", name="Chat model")) == 1)
report.check("the trigger is expanded", has_state(trigger, Atspi.StateType.EXPANDED))
report.check("focus is in the search", common.focused(app) is not None and common.focused(app).get_name() == "Search models",
             focused_label())
tabs = [t.get_name() for t in common.find(common.one(app, role="page tab list", name="Providers"), role="page tab")]
report.check("the rail has every provider but QwenCloud (off) and Cursor", len(tabs) == 9 and "Cursor" not in tabs, str(tabs))
openai = common.one(app, role="page tab", name="OpenAI — not configured")
report.check("a provider not connected is disabled and says why", not has_state(openai, Atspi.StateType.ENABLED))
report.check("the chosen model's provider is the tab shown",
             has_state(common.one(app, role="page tab", name="Claude Code harness: its own agent loop with mcbench tools attached"),
                       Atspi.StateType.SELECTED))
report.check("its models are listed", options() == ["Claude Opus 5.5", "Claude Haiku 5.5"], str(options()))

# Typing searches every provider; the failing one says so; the slow one comes.
common.type_text("gemini", settle=0.8)
found = wait_options(lambda o: "Gemini 3 Pro, OpenRouter" in o, "OpenRouter's Gemini")
report.check("typing searches every usable provider", "Gemini 3 Pro, OpenRouter" in found, str(found))
report.check("the failing catalogue is named with its provider",
             has_label("Groq: LLM request failed"))
found = wait_options(lambda o: "Gemini 3 Pro, Google" in o, "Google's slow catalogue", 4.0)
report.check("the slow catalogue (Google, 1.2 s, asked at start) is listed once it answered", "Gemini 3 Pro, Google" in found, str(found))
common.screenshot("pickers-search.png")

# Escape closes only the picker, back to its trigger; the text stays.
common.key("Escape", settle=0.6)
report.check("Escape closes it", not common.find(app, role="dialog", name="Chat model"))
report.check("focus is back on its trigger", focused_label() == f"button 'Chat model: {MODEL}'", focused_label())
common.key("Return", settle=0.8)
report.check("Enter on the trigger opens it again", len(common.find(app, role="dialog", name="Chat model")) == 1)
report.check("the search kept its text", search_text() == "gemini", search_text())
common.key("ctrl+a", settle=0.2)
common.key("BackSpace", settle=0.6)

# Arrows and Enter choose a model.
common.key("Down", settle=0.4)
report.check("Down goes to the first model", focused_label() == "list item 'Claude Opus 5.5'", focused_label())
common.key("Down", settle=0.4)
common.key("Return", settle=0.8)
report.check("Enter chooses it and closes", not common.find(app, role="dialog", name="Chat model"))
report.check("the trigger shows it", len(common.find(app, role="button", name="Chat model: claude-haiku-5-5")) == 1)
remembered = [a for a in common.actions() if a["action"] == "remembered" and "model" in a]
report.check("it is remembered as the last model", bool(remembered) and remembered[-1]["model"] == ["claude-code", "claude-haiku-5-5"],
             str(remembered[-1:]))

# A star toggles; a failing tab says why.
press(button("Chat model: claude-haiku-5-5"))
star = common.one(app, role="toggle button", name="Favorite Claude Opus 5.5")
was = has_state(star, Atspi.StateType.PRESSED) or has_state(star, Atspi.StateType.CHECKED)
press(star, 0.6)
star = common.one(app, role="toggle button", name="Favorite Claude Opus 5.5")
now = has_state(star, Atspi.StateType.PRESSED) or has_state(star, Atspi.StateType.CHECKED)
report.check("a star's press toggles it", now != was, f"{was} -> {now}")
press(common.one(app, role="page tab", name="Groq"), 0.6)
report.check("a failing tab shows its error",
             has_label("LLM request failed"))
common.screenshot("pickers-error.png")
common.key("Escape", settle=0.6)

# The effort list.
press(button("Reasoning effort: medium"))
report.check("the effort list opens on the current effort", focused_label() == "list item 'medium'", focused_label())
common.key("Down", settle=0.3)
report.check("Down moves to the next effort", focused_label() == "list item 'high'", focused_label())
common.key("Return", settle=0.6)
report.check("Enter chooses it", len(common.find(app, role="button", name="Reasoning effort: high")) == 1)

# The resolution slider.
press(button("Resolution: Standard"))
report.check("the slider has focus", focused_label() == "slider 'Resolution'", focused_label())
common.key("Right", settle=0.4)
report.check("Right steps it to High", len(common.find(app, role="button", name="Resolution: High (2×)")) == 1)
slider = common.one(app, role="slider", name="Resolution")
report.check("the slider's value is the stop's place", Atspi.Value.get_current_value(slider) == 1.0)
common.screenshot("pickers-resolution.png")
common.key("Escape", settle=0.6)
report.check("Escape closes it back to its trigger",
             focused_label() == "button 'Resolution: High (2×)'", focused_label())

common.quit_key(settle=2.0)
report.finish()
