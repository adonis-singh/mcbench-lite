"""The chat column with real input on the private display (all.sh runs it
with `--turn full --rate-ms 15 --running chat-gate --join chat-gate --refuse chat-farm --select`
and two `--pick` files): a full streamed turn (the working line while it
runs, then the fold, plan, warning and answer), Stop, edit and branch with
the branch arrows, a selection chip, attachments by path and their message,
the lightbox, drafts across two chats with typing that starts outside the
composer, joining a running turn, and a refused send.

With the argument `held` (all.sh runs it with `--setup-pending --setup-ms
12000`, chat-fix) it checks only the held send instead: a message sent while
setup runs is held with `Sends when the app is ready. 1 of 2 done.` and
`Don't send`; Don't send drops the hold and keeps the text; sent again, it
goes when the running step finishes."""

import sys
import time

import common

Atspi = common.Atspi
report = common.Report("chat")


def app():
    return common.app()


def names(role=None):
    return [o.get_name() for o in common.find(app(), role)]


def has(name, role=None):
    return any(n == name for n in names(role))


def press(name, role=None, last=True):
    found = [o for o in common.find(app(), role, name)]
    if not found:
        raise AssertionError(f"no {role} {name!r}")
    Atspi.Action.do_action(found[-1] if last else found[0], 0)
    time.sleep(0.5)


def focus_composer():
    entry = common.one(app(), None, "Message", pred=lambda o: o.get_role_name() == "entry")
    Atspi.Component.grab_focus(entry)
    time.sleep(0.3)


def press_row(title):
    """A chat row by its title (its name adds its status)."""
    Atspi.Action.do_action(common.row(app(), title), 0)
    time.sleep(0.5)


def busy():
    region = common.one(app(), "landmark", "Conversation")
    return region.get_state_set().contains(Atspi.StateType.BUSY)


def composer_text():
    entry = common.one(app(), None, "Message", pred=lambda o: o.get_role_name() == "entry")
    return Atspi.Text.get_text(entry, 0, -1)


def articles():
    return [o.get_name() for o in common.find(common.one(app(), "landmark", "Conversation"), "article")]


def logged(action, **fields):
    return any(a["action"] == action and all(a.get(k) == v for k, v in fields.items()) for a in common.actions())


def wait_turn(what):
    common.wait_for(lambda: has("Stop generating"), 10, f"Stop while {what} streams")
    common.wait_for(lambda: not has("Stop generating") and not busy(), 30, f"{what} to end")
    time.sleep(0.6)


common.app()
time.sleep(1.5)
common.focus_window()

if sys.argv[1:] == ["held"]:
    held = "Sends when the app is ready. 1 of 2 done."
    focus_composer()
    common.type_text("Hold this")
    common.key("Return", settle=0.8)
    report.check("a send while setup runs is held, with its line", has(held, "status bar"), repr(names("status bar")))
    report.check("and its Don't send button", has("Don't send"), repr(names("push button")))
    report.check("nothing is sent meanwhile", not logged("sent"))
    common.screenshot("held.png")
    press("Don't send")
    report.check("Don't send drops the hold", not has(held, "status bar") and logged("dont send"))
    report.check("and keeps the text", composer_text() == "Hold this", repr(composer_text()))
    focus_composer()
    common.key("Return", settle=0.8)
    report.check("sent again, it is held again", has(held, "status bar"))
    common.wait_for(lambda: logged("sent"), 15, "the held send to go")
    report.check("it goes when the running step finishes", logged("sent"))
    common.wait_for(lambda: any(a == "You, Hold this" for a in articles()), 5, "the sent message")
    report.check("the message is in the transcript and the line is gone",
                 any(a == "You, Hold this" for a in articles()) and not has(held, "status bar"), repr(articles()[-2:]))
    common.quit_key(settle=1.5)
    report.finish()

# A full streamed turn.
common.type_text("Make it brighter")
common.key("Return")
common.wait_for(lambda: busy(), 10, "the transcript busy")
working = common.wait_for(
    lambda: [n for n in names() if n.startswith("Working for ") or " · " in n and n.endswith("s")], 10, "the working line"
)
report.check("the working line shows while the turn runs", bool(working), working[0])
wait_turn("the full turn")
rows = articles()
at = max(i for i, r in enumerate(rows) if r == "You, Make it brighter")
after = rows[at + 1:]
report.check("a settled turn reads: work, warning, plan, answer", len(after) == 4, repr([a[:40] for a in after]))
report.check("the fold says its steps", has("Worked · 4 steps"))
report.check("the plan card's count", any("Tasks, 3 of 3" in r for r in after))
report.check("the warning notice", any("Warning: The model fell back" in r for r in after))
report.check("the completion retry dropped what it had streamed", not any("Checking the lamp" in r for r in rows))
press("Worked · 4 steps")
for row in [
    "Built: a stone base and a lamp",
    "Minecraft test completed: 2 tests: input and block-state observations",
    "Inspected build: (iso_se)",
    "Command failed: cat /work/notes.txt",
]:
    report.check(f"the tool row {row!r}", has(row))
report.check("the shot is a picture", has("Picture 1", "image"))
common.screenshot("full-turn.png")

# The shot in the lightbox; Escape closes it.
press("Picture 1", "image")
report.check("the lightbox is a dialog", has("Picture", "dialog"))
common.screenshot("lightbox.png")
common.key("Escape")
time.sleep(0.4)
report.check("Escape closes the lightbox", not has("Picture", "dialog"))

# Stop.
common.focus_window()
focus_composer()
common.type_text("Again, slower")
common.key("Return")
common.wait_for(lambda: has("Stop generating"), 10, "Stop")
time.sleep(0.4)
press("Stop generating")
common.wait_for(lambda: not has("Stop generating"), 20, "the stopped turn to end")
time.sleep(0.8)
report.check("Stop pressed", logged("stop"))
report.check("a stopped turn says so", articles()[-1] == "claude-opus-5-5, (stopped by user)", articles()[-1][:60])

# Edit and branch, then the arrows.
press("Edit and branch")
report.check("the editing line", has("Editing a message: sending it starts a new branch"))
report.check("the composer holds the message", composer_text() == "Again, slower", composer_text())
common.key("End")
common.type_text(" please")
common.key("Return")
wait_turn("the edited turn")
report.check("the edit is the message now", "You, Again, slower please" in articles())
report.check("the branch counter", has("Conversation branch, 2 of 2"))
press("Previous branch")
time.sleep(0.8)
report.check("Previous branch shows the first version", has("Conversation branch, 1 of 2") and "You, Again, slower" in articles())

# The new chat's selection chip.
press("New chat")
time.sleep(0.6)
chip = [n for n in names() if n.startswith("Redstone Lamp, unlit, 1×2×1")]
report.check("the selection chip, named by label and size", bool(chip), repr(chip))
press("Remove Redstone Lamp, unlit")
report.check("Remove takes the chip away", not any(n.startswith("Redstone Lamp, unlit") for n in names()))

# Attachments by path, sent, then the lightbox on the message's picture.
press_row("Redstone lamp with a lever")
time.sleep(0.8)
press("Attach file")
# par-chat: the tiles' names are the check (the wait raises when they never come).
tiles = common.wait_for(lambda: has("notes.txt, plain") and has("door.png, jpeg"), 10, "the attachment tiles")
report.check("the attachment tiles name file and kind", tiles)
common.screenshot("attachments.png")
# par-chat: Remove attachment takes the first tile away; Attach file brings it back.
press("Remove attachment", last=False)
report.check("Remove attachment takes the tile away", not has("notes.txt, plain") and has("door.png, jpeg"))
press("Remove attachment")
press("Attach file")
common.wait_for(lambda: has("notes.txt, plain") and has("door.png, jpeg"), 10, "the attachment tiles again")
focus_composer()
common.type_text("Here are the plans")
common.key("Return")
wait_turn("the turn with files")
report.check("the message's picture", has("door.png", "push button") or has("door.png"))
report.check("the message's file", has("notes.txt", "link"))
press("door.png")
report.check("the picture opens in the lightbox", has("Picture", "dialog"))
common.key("Escape")
time.sleep(0.4)

# Drafts across two chats; the typing starts on a chat row.
focus_composer()
common.type_text("a second lamp")
press_row("Castle gate")
joined = common.wait_for(lambda: logged("joined", chat="chat-gate"), 10, "the running turn joined")
report.check("a running turn is joined when its chat opens", bool(joined))
wait_turn("the joined turn")
report.check("the joined turn's answer", articles()[-1].startswith("claude-opus-5-5, "), articles()[-1][:50])
report.check("the other chat's composer is empty", composer_text() == "", composer_text())
press_row("Castle gate")
common.type_text("a taller gate")
report.check("typing on a chat row goes to the composer", composer_text() == "a taller gate", composer_text())
press_row("Redstone lamp with a lever")
time.sleep(0.6)
report.check("the first chat's draft is back", composer_text() == "a second lamp", composer_text())
press_row("Castle gate")
time.sleep(0.6)
report.check("the second chat's draft is back", composer_text() == "a taller gate", composer_text())

# A refused send.
press_row("Wheat farm")
time.sleep(0.8)
focus_composer()
common.type_text("One more")
common.key("Return")
time.sleep(1.0)
report.check(
    "a refused send is a notice", has("Error, A turn is already running for this conversation", "article")
)
report.check("the refused message is back in the composer", composer_text() == "One more", composer_text())
common.screenshot("refused.png")
# quit: the quit key ends the run, as every other check ends (run.sh ends
# an app still running 5 s after the check with SIGTERM: status 143).
common.quit_key(settle=1.5)
report.finish()
