"""Bench's places (bench agent) through AT-SPI, real keys and the pointer,
over the stand-in's `bench` fixture opened on the castle bench's
comparison (MCBENCH_APP_ARGS="--open /bench/bench-castle", FIXTURE=bench):
the rounds and their rows, a vote cast by keyboard alone (Tab to it,
Return) and announced, the round switch pressed with the pointer, Reveal
(a confirm), the leaderboard, a member's read-only chat and Fork to Build,
and a new bench run on two models chosen in its picker by keyboard; then
(par-bench) a round sent there with the file `--pick` names, shown in its
prompt, its picture opening the lightbox. A screenshot of each place.

    check_bench.py [theme]    theme names the screenshots (dark, light)
"""

import sys
import time

import common
from common import Atspi

theme = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "dark"
report = common.Report("bench-" + theme)
app = common.app()
time.sleep(2.0)
common.focus_window()


def shot(name):
    common.screenshot(f"bench-{name}-{theme}.png")


def names(role):
    return [o.get_name() for o in common.find(app, role=role)]


def starting(role, prefix):
    return [o for o in common.find(app, role=role) if (o.get_name() or "").startswith(prefix)]


def press(obj, settle=0.8):
    Atspi.Action.do_action(obj, 0)
    time.sleep(settle)


def click(obj, settle=0.8):
    e = obj.get_extents(Atspi.CoordType.WINDOW)
    common.pointer("mousemove", str(e.x + e.width // 2), str(e.y + e.height // 2))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(settle)


def focused_name():
    f = common.focused(app)
    return f.get_name() if f is not None else ""


def pressed(obj):
    states = obj.get_state_set()
    return states.contains(Atspi.StateType.PRESSED) or states.contains(Atspi.StateType.CHECKED)


def status_texts():
    return [o.get_name() for o in common.find(app, pred=lambda o: o.get_role_name() in ("status bar", "label", "notification"))]


# The comparison.
common.wait_for(lambda: starting("panel", "Castle with a moat comparison") or starting("landmark", "Castle with a moat comparison")
                or common.find(app, name="Castle with a moat comparison"), 15, "the comparison")
lists = names("list")
report.check("one list per round", all(f"Round {n}" in lists for n in (1, 2, 3)), str(lists))
rows = [n for n in names("list item") if n.startswith("Build ")]
report.check("eight rows a round, by letter while blind", len(rows) >= 24, str(len(rows)))
report.check("the running, failed and stopped models say so",
             any("Working" in r for r in rows) and any("Failed" in r for r in rows) and any("Stopped" in r for r in rows), "")
report.check("a hidden bench shows no time or tokens", not any(" tok" in r for r in rows), "")
shot("compare")

# A vote by keyboard alone: Tab until a pick button, Return.
target = None
for _ in range(120):
    common.key("Tab", settle=0.12)
    name = focused_name()
    if name.startswith("Pick Build") and name.endswith("as the best build"):
        target = name
        break
report.check("Tab reaches a round's vote", target is not None, target or focused_name())
if target:
    picked = target[len("Pick "):-len(" as the best build")]
    common.key("Return", settle=1.2)
    taken = common.find(app, role="toggle button", name=f"Take back the pick of {picked}") or common.find(
        app, role="button", name=f"Take back the pick of {picked}")
    report.check("the vote shows as the pick, pressed", bool(taken) and pressed(taken[0]), str([common.label(t) for t in taken]))
    said = f"Your pick for this round: {picked}"
    report.check("the vote is announced", said in status_texts(), str(status_texts())[:300])
    report.check("the vote reached the core", any(a.get("action") == "vote" for a in common.actions()), "")
    shot("voted")

# The round switch, with the pointer.
switch = common.find(app, name="Round 1", pred=lambda o: o.get_role_name() in ("toggle button", "button"))
report.check("the round switch is a group of buttons", bool(switch), str(len(switch)))
if switch:
    click(switch[0])
    report.check("a press shows round 1", any(a.get("action") == "round" and a.get("round") == 1 for a in common.actions()), "")
    report.check("the round's button is pressed", pressed(common.find(app, name="Round 1", pred=lambda o: o.get_role_name() in ("toggle button", "button"))[0]), "")

# Reveal: a confirm.
reveal = common.find(app, name="Reveal models")
report.check("Reveal models is offered while blind", bool(reveal), "")
if reveal:
    press(reveal[0])
    armed = common.find(app, name="Reveal for good")
    report.check("Reveal asks first", bool(armed), "")
    if armed:
        press(armed[0], 1.5)
    rows = names("list item")
    report.check("revealed: the models' names", any(r.startswith("claude-opus-5-5, ") for r in rows), str(rows[:4]))
    report.check("gpt-5.5 twice: each with its effort", any(r.startswith("gpt-5.5, ") and ", high" in r for r in rows)
                 and any(r.startswith("gpt-5.5, ") and ", low" in r for r in rows), "")
    shot("revealed")

# A member's chat: read only, its breadcrumb, Fork to Build.
opener = starting("button", "Open claude-opus-5-5")
report.check("each row opens its model's chat", bool(opener), "")
if opener:
    press(opener[0], 1.5)
    report.check("a member's chat has no composer", not common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry")), "")
    report.check("its breadcrumb goes back", bool(common.find(app, role="button", name="Castle with a moat")), "")
    shot("member")
    fork = common.find(app, name="Fork to Build")
    report.check("Fork to Build is offered", bool(fork), "")
    if fork:
        press(fork[0], 2.0)
        report.check("the fork opens Build", any(a.get("action") == "forked" for a in common.actions()), "")

# The leaderboard, from Bench's list.
common.key("ctrl+2", settle=1.0)
board_row = common.find(app, name="Leaderboard", pred=lambda o: o.get_role_name() == "button")
if board_row:
    press(board_row[0], 1.5)
tables = common.find(app, role="table", name="Models ranked by blind votes")
report.check("the leaderboard is a table", bool(tables), "")
if tables:
    report.check("one row a model", len(common.find(tables[0], role="table row")) >= 6, "")
shot("leaderboard")

# A new bench: two models from the picker, by keyboard; Run.
new = common.find(app, name="New bench", pred=lambda o: o.get_role_name() == "button")
report.check("New bench is offered", bool(new), "")
if new:
    press(new[0], 1.5)
    report.check("the new bench asks its question", bool(common.find(app, name="New bench", pred=lambda o: o.get_role_name() in ("panel", "landmark", "section"))), "")
    trigger = common.find(app, role="button", name="Models") or common.find(app, role="button", name="Add")
    report.check("the Bench models picker is there", bool(trigger), "")
    if trigger:
        press(trigger[0], 1.2)
        options = [o for o in common.find(app, role="list item") if o.get_name()]
        chosen = 0
        box = common.find(app, role="list box", name="Bench models")
        within = box[0] if box else app
        # By name, found again before each choice: a choice redraws the list.
        for name in [o.get_name() for o in common.find(within, role="list item")[:2]]:
            box = common.find(app, role="list box", name="Bench models")
            item = common.find(box[0] if box else app, role="list item", name=name)
            if not item:
                break
            Atspi.Component.grab_focus(item[0])
            time.sleep(0.3)
            common.key("Return", settle=0.8)
            chosen += 1
        report.check("two models chosen from the picker by keyboard", chosen == 2, str(len(options)))
        common.key("Escape", settle=0.8)
    shot("new-bench")
    message = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))
    if message:
        Atspi.Component.grab_focus(message[0])
        time.sleep(0.3)
        common.type_text("A lighthouse on a cliff", settle=0.8)
        common.key("Return", settle=2.5)
        report.check("the new bench opens its comparison",
                     any(a.get("action") == "opened" and str(a.get("place", "")).startswith("/bench/bench-new-") for a in common.actions()),
                     str([a for a in common.actions() if a.get("action") == "opened"][-2:]))
        shot("new-compare")
        # par-bench: a round sent with a picked file (`--pick`) shows it in
        # its prompt, and the picture opens the lightbox.
        attach = common.find(app, role="button", name="Attach file")
        report.check("the comparison's composer attaches files", bool(attach), "")
        if attach:
            press(attach[0], 1.5)
            message = common.find(app, name="Message", pred=lambda o: o.get_role_name() in ("text", "entry"))
            if message:
                Atspi.Component.grab_focus(message[0])
                time.sleep(0.3)
            common.type_text("Hang banners on the tower", settle=0.5)
            common.key("Return", settle=3.0)
            pictures = common.find(app, role="button", name="door.png")
            report.check("the round's picture is in its prompt", bool(pictures), str(names("button")[-12:]))
            common.screenshot(f"bench-round-files-{theme}.png")
            if pictures:
                press(pictures[-1], 1.0)
                report.check("the round's picture opens the lightbox",
                             any(a.get("action") == "opened" and a.get("layer") == "lightbox" for a in common.actions()), "")
                common.key("Escape", settle=0.8)

common.quit_key(settle=2.0)
report.finish()
