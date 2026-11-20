"""The first start's screen (settings agent) through AT-SPI and real keys,
in a private session, on the empty new chat while setup runs.

    check_setup.py fails     --settings setup-fails --settings no-ai --open /new:
                             the steps move with download progress and the
                             count is a status that changes; a needed step
                             fails (A step failed., its note's address a
                             link); Try again by keyboard runs it again and
                             setup ends, the screen giving way to the empty
                             chat; Connect an AI while this runs opens
                             Settings on Connect an AI
    check_setup.py optional  --settings setup-optional-fails --open /new: the
                             optional step cannot be done, no failure is
                             shown, the app gets ready; the last line
                             (an AI is connected)
    check_setup.py wsl, wsl-restart, wsl-refused  the shell's WSL row while
                             setup runs (see the modes below)

  s4-needs: a step that needs the person with setup otherwise ready (the
  shell is optional, nothing is held):

    check_setup.py needs-empty  --settings setup-wsl-optional-refused --open
                             /new: the ready empty chat keeps its own
                             heading (no Getting ready, first-start sentence,
                             bar or count) with a card of only the step that
                             needs the person and its Install WSL, then the
                             ready chat's last line (its Bench link); no
                             failure, no held line, no second line over the
                             composer; the running installer's row stays; a
                             refused install brings the button back with
                             still no failure
    check_setup.py needs-chat   --settings setup-wsl-optional --open
                             /chat/chat-gate: one status over the composer
                             with the core's sentence and Install WSL, before
                             the text box in the Tab order; while the
                             installer runs its sentence and no button, the
                             focus on the line; gone once WSL is ready
    check_setup.py needs-bench  --settings setup-wsl-optional-restart --open
                             /bench: the restart's sentence and Restart now
                             over a new bench's composer; one press sends
                             RestartComputer and the line stays

Screenshots go to the run's folder."""

import sys
import time

import common
from common import Atspi

mode = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "fails"
report = common.Report("setup-" + mode)
app = common.app()
time.sleep(2.0)
common.focus_window()


def find(role=None, name=None, pred=None):
    return common.find(app, role, name, pred)


def has(role, name):
    return bool(find(role, name))


def count():
    found = find("status bar", pred=lambda o: (o.get_name() or "").endswith(" done") and " of " in (o.get_name() or ""))
    return found[0].get_name() if found else None


def wait(pred, what, timeout=15.0):
    try:
        return common.wait_for(pred, timeout, what)
    except AssertionError:
        return None


def step(prefix):
    found = find("list item", pred=lambda o: (o.get_name() or "").startswith(prefix))
    return found[0].get_name() if found else None


if not mode.startswith("needs-"):
    report.check("the screen shows while setup runs", bool(wait(lambda: has("landmark", "Getting ready") or has("heading", "Getting ready"), "the screen")))
    report.check("the count is a status", count() is not None, str(count()))
if not mode.startswith("wsl") and not mode.startswith("needs-"):
    progress = wait(lambda: step("Downloading Minecraft's textures") and " MB" in step("Downloading Minecraft's textures"), "download progress")
    report.check("a running step shows its download progress", bool(progress), str(step("Downloading Minecraft's textures")))
    common.screenshot(f"setup-{mode}-running.png")

if mode == "fails":
    failed = wait(lambda: has("notification", "A step failed."), "the failure")
    report.check("a needed step failing says so", bool(failed))
    report.check("its row says Failed with the core's note", "Failed" in (step("Minecraft's textures") or ""), str(step("Minecraft's textures")))
    link = find("link", "https://www.minecraft.net/en-us/download/server")
    report.check("the note's address is a link, without its full stop", bool(link))
    report.check("Connect an AI while this runs (no AI connected)", has("button", "Connect an AI while this runs"))
    common.screenshot("setup-failed.png")
    Atspi.Component.grab_focus(common.one(app, "button", "Connect an AI while this runs"))
    time.sleep(0.3)
    common.key("Return", 1.2)
    report.check("Connect an AI opens Settings on Connect an AI",
                 has("dialog", "Settings") and bool(find("page tab", pred=lambda o: o.get_name() == "Connect an AI" and o.get_state_set().contains(Atspi.StateType.SELECTED))))
    common.screenshot("setup-connect.png")
    common.key("Escape", 0.8)
    Atspi.Component.grab_focus(common.one(app, "button", "Try again"))
    time.sleep(0.3)
    common.key("Return", 1.0)
    report.check("Try again runs the step again", not has("notification", "A step failed."))
    report.check("Try again was sent", any(a["action"] == "retry setup" for a in common.actions()))
    progress = wait(lambda: " MB" in (step("Downloading Minecraft's textures") or ""), "download progress")
    report.check("the step downloads again with its progress", bool(progress), str(step("Downloading Minecraft's textures")))
    before = count()
    changed = wait(lambda: count() != before, "the count to change")
    report.check("the count changes as steps end", bool(changed), f"{before} -> {count()}")
    ready = wait(lambda: not has("heading", "Getting ready"), "setup to end", 20)
    report.check("once ready the screen gives way to the empty chat", bool(ready) and has("heading", "What should we build?"))

if mode == "optional":
    unavailable = wait(lambda: "Not available" in (step("Model's shell") or ""), "the optional step")
    report.check("the optional step cannot be done", bool(unavailable), str(step("Model's shell")))
    report.check("which is no failure", not has("notification", "A step failed."))
    report.check("its note's address is a link", has("link", "https://learn.microsoft.com/windows/wsl/install"))
    report.check("the last line (an AI is connected)", has("label", "You can send your first build now. It starts when the app is ready."))
    common.screenshot("setup-optional.png")
    ready = wait(lambda: not has("heading", "Getting ready"), "setup to end", 20)
    report.check("the app gets ready with the optional step not done", bool(ready))

if mode.startswith("wsl"):
    # The stand-in's WSL plays (--settings setup-wsl, setup-wsl-restart,
    # setup-wsl-refused): the shell's row says the core's sentence and shows
    # the one button its need names, the row's Tab stop; the button keeps
    # focus through the row's changes.
    def shell():
        return step("Model's shell") or step("Downloading the model's shell") or ""

    def shell_row():
        found = find("list item", pred=lambda o: (o.get_name() or "").startswith("Model's shell") or (o.get_name() or "").startswith("Downloading the model's shell"))
        return found[0] if found else None

if mode == "wsl":
    row = wait(lambda: "Windows needs WSL. Install it here" in shell(), "the shell's row")
    report.check("the shell's row says what WSL needs, as the core writes it", bool(row), shell())
    button = common.one(app, "button", "Install WSL")
    common.screenshot("setup-wsl.png")
    Atspi.Component.grab_focus(button)
    time.sleep(0.3)
    common.key("Return", 0.8)
    report.check("Install WSL sends InstallWsl", any(a["action"] == "install wsl" for a in common.actions()))
    report.check("while the installer runs: its sentence, the dots, no button",
                 "Installing WSL. Windows is asking" in shell() and not has("button", "Install WSL"), shell())
    focus = common.focused(app)
    report.check("focus stays on the shell's row", focus is not None and focus.get_role_name() == "list item"
                 and ("Installing WSL" in (focus.get_name() or "") or (focus.get_name() or "").startswith("Model's shell")),
                 common.label(focus))
    common.screenshot("setup-wsl-installing.png")
    done = wait(lambda: "Done" in shell(), "WSL ready")
    report.check("then the step goes on", bool(done), shell())

if mode == "wsl-restart":
    row = wait(lambda: "Restart Windows to finish" in shell(), "the shell's row")
    report.check("the row says Windows needs a restart", bool(row), shell())
    report.check("no confirmation anywhere", not common.find(app, pred=lambda o: "Restart Windows now?" in (o.get_name() or "")))
    common.screenshot("setup-wsl-restart.png")
    Atspi.Component.grab_focus(common.one(app, "button", "Restart now"))
    time.sleep(0.3)
    common.key("Return", 1.0)
    report.check("one press sends RestartComputer", len([a for a in common.actions() if a["action"] == "restart computer"]) == 1)
    time.sleep(2.0)
    # Windows took the request and nothing follows (no update): the row is
    # as it was, its button there, enabled, usable again.
    button = common.find(app, role="button", name="Restart now")
    report.check("with no update after it the row stays as it was",
                 "Restart Windows to finish" in shell() and bool(button)
                 and button[0].get_state_set().contains(Atspi.StateType.ENABLED), shell())
    Atspi.Component.grab_focus(button[0])
    time.sleep(0.3)
    common.key("Return", 1.0)
    report.check("and it can be pressed again", len([a for a in common.actions() if a["action"] == "restart computer"]) == 2)

if mode == "wsl-refused":
    wait(lambda: has("button", "Install WSL"), "the button")
    Atspi.Component.grab_focus(common.one(app, "button", "Install WSL"))
    time.sleep(0.3)
    common.key("Return", 0.8)
    refused = wait(lambda: "the administrator's permission was not given" in shell(), "the refusal")
    report.check("a refused install says why, as the core writes it", bool(refused), shell())
    report.check("and offers Install WSL again", has("button", "Install WSL"))
    focus = common.focused(app)
    report.check("focus is back on the button", focus is not None and focus.get_name() == "Install WSL", common.label(focus))
    common.screenshot("setup-wsl-refused.png")

# s4-needs
WSL_NEEDED = "To use bash and build, Windows needs WSL. Install it here: Windows asks for an administrator's permission once, and for a restart when its virtualisation feature was off."
WSL_INSTALLING = "Installing WSL. Windows is asking for an administrator's permission."
WSL_RESTART = "WSL is installed. Restart Windows to finish; the app opens again by itself and carries on."
WSL_REFUSED = "WSL was not installed: the administrator's permission was not given."


def line(text):
    """The line over the composer: a status named the step's text."""
    return has("status bar", text)


def any_line():
    return find("status bar", pred=lambda o: "WSL" in (o.get_name() or ""))


def both_themes(name):
    """`name`-dark.png, then Settings' Light, `name`-light.png, and Dark
    again (Settings opened from the sidebar's Settings by keyboard)."""
    common.screenshot(f"{name}-dark.png")
    for theme, shot in (("Light", f"{name}-light.png"), ("Dark", None)):
        Atspi.Component.grab_focus(common.one(app, "button", "Settings"))
        time.sleep(0.3)
        common.key("Return", 1.0)
        wait(lambda: has("toggle button", theme), "Settings open")
        Atspi.Action.do_action(common.one(app, "toggle button", theme), 0)
        time.sleep(0.8)
        common.key("Escape", 0.8)
        if shot:
            common.screenshot(shot)


if mode == "needs-empty":
    def rows():
        return [o.get_name() for o in find("list item", pred=lambda o: o.get_parent() is not None and o.get_parent().get_name() == "Setup steps")]

    def ready_heading():
        return (has("heading", "What should we build?") and not has("heading", "Getting ready") and not has("landmark", "Getting ready")
                and not find("progress bar") and count() is None
                and not find(pred=lambda o: (o.get_name() or "").startswith("The first start downloads")))

    shown = wait(lambda: any(WSL_NEEDED in r for r in rows()), "the shell's row")
    report.check("the shell's row says what WSL needs", bool(shown), str(rows()))
    report.check("the ready chat's own heading: no Getting ready, first-start sentence, bar or count", ready_heading())
    report.check("the card holds only the step that needs the person", len(rows()) == 1 and rows()[0].startswith("Model's shell"), str(rows()))
    report.check("with its Install WSL", has("button", "Install WSL"))
    report.check("then the ready chat's last line (its Bench link)", has("button", "Bench"))
    report.check("not the line of an app still getting ready", not has("label", "You can send your first build now. It starts when the app is ready."))
    report.check("no failure", not has("notification", "A step failed.") and not has("button", "Try again"))
    report.check("no second line over the composer", not any_line(), str([common.label(o) for o in any_line()]))
    both_themes("needs-empty")
    Atspi.Component.grab_focus(common.one(app, "button", "Install WSL"))
    time.sleep(0.3)
    common.key("Return", 0.8)
    report.check("Install WSL sends InstallWsl", any(a["action"] == "install wsl" for a in common.actions()))
    report.check("the installer's row stays while it runs, under the same heading",
                 len(rows()) == 1 and WSL_INSTALLING in rows()[0] and ready_heading(), str(rows()))
    refused = wait(lambda: any(WSL_REFUSED in r for r in rows()), "the refusal")
    report.check("a refused install says why", bool(refused), str(rows()))
    report.check("its button is back", has("button", "Install WSL"))
    report.check("and still no failure (the step is optional)", not has("notification", "A step failed."))

if mode == "needs-chat":
    shown = wait(lambda: line(WSL_NEEDED), "the line over the composer")
    report.check("a status over the composer with the core's sentence", bool(shown))
    report.check("and Install WSL", has("button", "Install WSL"))
    report.check("no steps in a chat with messages", not has("heading", "Getting ready"))
    report.check("no held line", not find("status bar", pred=lambda o: (o.get_name() or "").startswith("Sends when the app is ready")))
    both_themes("needs-chat")
    Atspi.Component.grab_focus(common.one(app, "button", "Install WSL"))
    time.sleep(0.3)
    common.key("Tab", 0.5)
    focus = common.focused(app)
    report.check("Tab goes from the button to the composer's text box",
                 focus is not None and focus.get_name() == "Message" and focus.get_role_name() in ("text", "entry"), common.label(focus))
    common.key("shift+Tab", 0.5)
    focus = common.focused(app)
    report.check("and Shift+Tab back to the button", focus is not None and focus.get_name() == "Install WSL", common.label(focus))
    common.key("Return", 0.8)
    report.check("Install WSL sends InstallWsl", any(a["action"] == "install wsl" for a in common.actions()))
    report.check("while the installer runs: its sentence and no button", line(WSL_INSTALLING) and not has("button", "Install WSL"),
                 str([common.label(o) for o in any_line()]))
    focus = common.focused(app)
    report.check("focus stays on the line", focus is not None and focus.get_name() == WSL_INSTALLING, common.label(focus))
    gone = wait(lambda: not any_line(), "the line to go")
    report.check("the line goes once WSL is ready", bool(gone), str([common.label(o) for o in any_line()]))

if mode == "needs-bench":
    shown = wait(lambda: line(WSL_RESTART), "the line over the composer")
    report.check("a status over the new bench's composer with the restart's sentence", bool(shown))
    report.check("and Restart now", has("button", "Restart now"))
    Atspi.Component.grab_focus(common.one(app, "button", "Restart now"))
    time.sleep(0.3)
    common.key("Return", 1.0)
    report.check("one press sends RestartComputer", len([a for a in common.actions() if a["action"] == "restart computer"]) == 1)
    time.sleep(1.5)
    report.check("with no update after it the line stays, its button there", line(WSL_RESTART) and has("button", "Restart now"))

report.finish()
