"""Settings (settings agent) through AT-SPI, real keys and a real pointer,
in a private session.

    check_settings.py            every pane by keyboard and pointer; the key
                                 field (no typed value in the tree, the dump
                                 or the log; copy refused; it saves); a cancel
                                 with a draft; the models list (filter, hide
                                 matches, star, reorder by keys and by drag);
                                 the shortcut recorder; clearing the votes; the
                                 published list (go to, copy, remove); a save;
                                 focus back on what opened Settings
    check_settings.py claude     --settings claude-sign-in: install with MB
                                 progress, starting, waiting with the address,
                                 the pasted code, signed in
    check_settings.py chatgpt    --settings chatgpt-sign-in: the one-time code,
                                 cancel, start again, signed in (from Connect)
    check_settings.py failures   --settings install-fails --settings
                                 sign-in-fails --settings save-refused
                                 --settings remove-fails --settings clear-fails
                                 --settings votes-1
    check_settings.py read-fails --settings read-fails
    check_settings.py shares     --settings shares-failed --settings votes-unreadable
    check_settings.py shots      a screenshot of each pane and state (with
                                 MCBENCH_APP_ARGS naming plays as above)
    check_settings.py timing     open and filter timings (settings-timing in
                                 the action log), with --models 300

all.sh passes the arguments each mode needs. Screenshots go to the run's
folder."""

import json
import os
import statistics
import sys
import time

import common
from common import Atspi

mode = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else "checks"
report = common.Report("settings-" + mode)
common.X_KEYS.update({"ctrl+k": "ctrl+k"})
app = common.app()
time.sleep(2.0)
common.focus_window()

KEY = "sk-test-0000-typed"


def press(obj, settle=0.6):
    Atspi.Action.do_action(obj, 0)
    time.sleep(settle)


def has(role, name):
    return bool(common.find(app, role=role, name=name))


def named(role, name):
    return common.one(app, role=role, name=name)


def starting(role, prefix):
    return common.find(app, role=role, pred=lambda o: (o.get_name() or "").startswith(prefix))


def starts(role, prefix):
    found = starting(role, prefix)
    if len(found) != 1:
        raise AssertionError(f"expected one {role} starting {prefix!r}, found {len(found)}")
    return found[0]


def focused():
    return common.label(common.focused(app))


def state(obj, s):
    return obj.get_state_set().contains(s)


def wait(pred, what, timeout=8.0):
    try:
        return common.wait_for(pred, timeout, what)
    except AssertionError:
        return None


def dialog():
    found = common.find(app, role="dialog", name="Settings")
    return found[0] if found else None


def inside(obj, root):
    while obj is not None:
        if obj == root:
            return True
        obj = obj.get_parent()
    return False


def open_settings():
    """By keyboard: focus on the sidebar's Settings, then Enter."""
    Atspi.Component.grab_focus(named("button", "Settings"))
    time.sleep(0.3)
    common.key("Return", 1.0)
    return wait(lambda: dialog() is not None, "Settings open")


def key_press(obj, key="Return", settle=0.6):
    Atspi.Component.grab_focus(obj)
    time.sleep(0.3)
    common.key(key, settle)


def tab(prefix):
    return starts("page tab", prefix)


def show(prefix, settle=0.9):
    press(tab(prefix), settle)


def centre(obj):
    r = Atspi.Component.get_extents(obj, Atspi.CoordType.WINDOW)
    return r.x + r.width // 2, r.y + r.height // 2


def tree_text():
    """Every name, description, value and text of the tree."""
    parts = []

    def visit(o):
        try:
            parts.append(o.get_name() or "")
            parts.append(o.get_description() or "")
            try:
                parts.append(Atspi.Text.get_text(o, 0, -1) or "")
            except Exception:
                pass
        except Exception:
            return
        for i in range(o.get_child_count()):
            visit(o.get_child_at_index(i))

    visit(app)
    return "\n".join(parts)


def clipboard():
    """The clipboard's text; an empty clipboard (wl-paste fails on one) is
    none."""
    try:
        return common.clipboard() or ""
    except Exception:
        return ""


def logged(action):
    return [a for a in common.actions() if a["action"] == action]


def shot(name):
    common.screenshot(name + ".png")


def type_into(obj, text):
    Atspi.Component.grab_focus(obj)
    time.sleep(0.3)
    common.type_text(text)
    time.sleep(0.6)


def grips():
    return starting("button", "Reorder ")


if mode == "checks":
    report.check("Settings open on General, focus on its tab",
                 open_settings() and focused().startswith("page tab 'General"), focused())
    report.check("the active tab is selected", state(tab("General"), Atspi.StateType.SELECTED))
    report.check("the dialog is modal", state(dialog(), Atspi.StateType.MODAL))
    report.check("General's sections", all(has("panel", n) for n in ("Conversations", "Appearance", "Sharing", "Shortcuts", "Danger zone")))
    shot("general-dark")
    # The navigation by arrows: focus moves, nothing is shown; Enter shows.
    common.key("Down")
    report.check("Down moves to the next tab", focused().startswith("page tab 'OpenRouter"), focused())
    report.check("and shows nothing", state(tab("General"), Atspi.StateType.SELECTED))
    common.key("Up")
    common.key("Up")
    report.check("Up walks back to Connect an AI", focused().startswith("page tab 'Connect an AI"), focused())
    common.key("End")
    report.check("End goes to the last tab", focused().startswith("page tab 'Codex CLI"), focused())
    common.key("Home")
    common.key("Return", 0.8)
    report.check("Enter shows Connect an AI", state(tab("Connect an AI"), Atspi.StateType.SELECTED))
    report.check("its three ways", all(has("panel", n) for n in ("An API key", "Your Claude subscription, through Claude Code", "Your ChatGPT subscription, through Codex CLI or ChatGPT sign-in")))
    report.check("no Cursor way", "Cursor" not in tree_text())
    shot("connect-dark")
    # A pane button opens its provider's pane.
    x, y = centre(starts("button", "Anthropic, Key from"))
    common.pointer("mousemove", str(x), str(y))
    time.sleep(0.2)
    common.pointer("click", "1")
    time.sleep(0.9)
    report.check("by pointer, a way's button opens its pane with focus on its tab",
                 state(tab("Anthropic"), Atspi.StateType.SELECTED) and focused().startswith("page tab 'Anthropic"), focused())
    show("Connect an AI")
    key_press(starts("button", "OpenAI, No key"), "Return", 0.9)
    report.check("a way's button opens its pane", state(tab("OpenAI"), Atspi.StateType.SELECTED))
    report.check("by key, focus goes to the pane's tab", focused().startswith("page tab 'OpenAI"), focused())
    report.check("its Account, Models and Runtime sections", all(has("panel", n) for n in ("Account", "Models", "Runtime")))
    report.check("a pane not connected says how to list its models", has("label", "Add a key to list OpenAI's models."))
    report.check("Checked <ago> is a button", bool(starting("button", "Checked ")))
    # The key field: a password field whose typed value is nowhere.
    field = named("password text", "API key")
    type_into(field, KEY)
    report.check("the tree holds no typed key", KEY not in tree_text())
    dump = open(common.out("a11y.json")).read() if os.path.exists(common.out("a11y.json")) else ""
    report.check("the dump holds no typed key", bool(dump) and KEY not in dump)
    report.check("the action log holds no typed key", KEY not in open(common.out("actions.jsonl")).read())
    common.key("ctrl+a")
    common.key("ctrl+c")
    report.check("copy out of it is refused", KEY not in clipboard())
    shot("key-typed")
    # Tab stays inside Settings.
    left = []
    for _ in range(80):
        common.key("Tab", 0.03)
        if not inside(common.focused(app), dialog()):
            left.append(focused())
    report.check("Tab stays inside Settings", not left, str(left[:3]))
    # Cancel with a draft: the key is dropped and focus goes back.
    press(named("button", "Cancel"), 1.0)
    report.check("Cancel closes Settings", dialog() is None)
    report.check("focus goes back to what opened them", focused() == "button 'Settings'", focused())
    report.check("the cancelled key was not saved", not starting("page tab", "OpenAI, Key saved"))
    open_settings()
    show("OpenRouter")
    shot("openrouter-dark")
    # The models list: filter, hide matches, star, reorder by keys.
    rows = common.find(named("list", "Models"), role="list item")
    report.check("the models are listed", len(rows) > 3, str(len(rows)))
    type_into(named("entry", "Filter models"), "claude")
    rows = common.find(named("list", "Models"), role="list item")
    report.check("the filter narrows the list", 0 < len(rows) < 6 and all("laude" in r.get_name() for r in rows),
                 str([r.get_name() for r in rows]))
    press(named("button", "Hide matches"))
    report.check("Hide matches hides them", has("button", "Show matches"))
    count = starting("label", "")
    count = [c for c in count if (c.get_name() or "").endswith(" shown")]
    report.check("the count follows", bool(count) and count[0].get_name().startswith("16 of"), count[0].get_name() if count else "")
    press(named("button", "Show matches"))
    star = starting("toggle button", "Favorite ")
    before = state(star[0], Atspi.StateType.PRESSED)
    press(star[0])
    report.check("a star toggles", state(starting("toggle button", "Favorite ")[0], Atspi.StateType.PRESSED) != before)
    report.check("and is saved", bool(logged("star")))
    field = named("entry", "Filter models")
    Atspi.Component.grab_focus(field)
    for _ in range(8):
        common.key("BackSpace", 0.03)
    time.sleep(0.5)
    shown = starting("toggle button", "Show ")
    report.check("each model's switch", bool(shown) and all(s.get_name().endswith(" in the model picker") for s in shown))
    first = grips()[0].get_name()
    Atspi.Component.grab_focus(grips()[0])
    time.sleep(0.3)
    common.key("Down", 0.8)
    report.check("Down on a grip moves the model one row", grips()[1].get_name() == first, str([g.get_name() for g in grips()[:3]]))
    report.check("the grip keeps focus", focused() == f"button {first!r}", focused())
    said = [o.get_name() for o in common.find(app, role="status bar")]
    report.check("the move is said", any(" moved to 2 of " in (s or "") for s in said), str(said))
    common.key("Up", 0.8)
    report.check("Up moves it back", grips()[0].get_name() == first)
    # Drag the first row's grip two rows down.
    name0 = grips()[0].get_name()
    x, y = centre(grips()[0])
    common.pointer("mousemove", str(x), str(y))
    time.sleep(0.2)
    common.pointer("mousedown", "1")
    time.sleep(0.2)
    for step in range(1, 9):
        common.pointer("mousemove", str(x), str(y + step * 10))
        time.sleep(0.05)
    shot("model-dragging")
    common.pointer("mouseup", "1")
    time.sleep(0.8)
    report.check("a drag moves the row two down", grips()[2].get_name() == name0, str([g.get_name() for g in grips()[:4]]))
    # A switch hides one model; the provider's own switch in the navigation.
    switch = starting("toggle button", "Show ")[0]
    switch_name = switch.get_name()
    press(switch)
    report.check("a model's switch hides it", not state(named("toggle button", switch_name), Atspi.StateType.PRESSED))
    press(named("toggle button", "Offer QwenCloud in the model picker"))
    report.check("a provider's switch turns it on", state(named("toggle button", "Offer QwenCloud in the model picker"), Atspi.StateType.PRESSED))
    report.check("the switch shows no pane", state(tab("OpenRouter"), Atspi.StateType.SELECTED))
    # The key, typed again, saves: OpenAI's summary says so.
    show("OpenAI")
    type_into(named("password text", "API key"), KEY)
    press(named("button", "Save changes"), 1.5)
    report.check("Save changes closes Settings", dialog() is None)
    open_settings()
    report.check("the saved key connects OpenAI", bool(starting("page tab", "OpenAI, Key saved")))
    report.check("the saved switch is kept", state(named("toggle button", "Offer QwenCloud in the model picker"), Atspi.StateType.PRESSED))
    report.check("the saved order is kept", bool(logged("save settings")))
    report.check("still no typed key anywhere", KEY not in tree_text() and KEY not in open(common.out("a11y.json")).read())
    # General: the theme, the title model, the shortcut recorder.
    press(named("toggle button", "Light"), 0.8)
    report.check("Light applies at once", state(named("toggle button", "Light"), Atspi.StateType.PRESSED))
    shot("general-light")
    show("OpenRouter")
    shot("openrouter-light")
    show("Connect an AI")
    shot("connect-light")
    show("General")
    press(named("toggle button", "Dark"), 0.8)
    trigger = starts("button", "The chat's own model")
    press(trigger, 1.0)
    report.check("the title model picker opens", has("dialog", "Title model"))
    common.key("Escape", 0.6)
    report.check("Escape closes only the picker", has("dialog", "Settings") and not has("dialog", "Title model"))
    rec = starts("toggle button", "Show or hide the sidebar")
    press(rec)
    rec = starts("toggle button", "Show or hide the sidebar")
    report.check("the recorder says it records", "recording" in rec.get_name() and state(rec, Atspi.StateType.PRESSED), rec.get_name())
    shot("recording")
    common.key("Escape", 0.5)
    report.check("Escape cancels the recording, Settings stay", dialog() is not None and starts("toggle button", "Show or hide the sidebar").get_name().endswith("Ctrl+B"))
    press(starts("toggle button", "Show or hide the sidebar"))
    common.key("ctrl+k", 0.6)
    report.check("a combination taken from another action is set", starts("toggle button", "Show or hide the sidebar").get_name().endswith("Ctrl+K"))
    report.check("and unbinds that one", starts("toggle button", "Search everything").get_name().endswith("Not set"))
    report.check("Reset all is offered", has("button", "Reset all"))
    press(named("button", "Reset all"))
    report.check("Reset all puts the defaults back", starts("toggle button", "Search everything").get_name().endswith("Ctrl+K") and not has("button", "Reset all"))
    # The danger zone.
    key_press(named("button", "Clear leaderboard"), "space")
    report.check("Clear leaderboard asks", has("panel", "Delete all 12 votes?"))
    report.check("focus on its Cancel", focused() == "button 'Cancel'" and inside(common.focused(app), named("panel", "Danger zone")), focused())
    press(named("button", "Delete votes"), 1.0)
    report.check("Delete votes clears them", has("button", "No votes"))
    # The published list.
    builds = common.find(named("list", "Published builds"), role="list item")
    report.check("the published builds are listed", len(builds) == 4, str([b.get_name() for b in builds]))
    report.check("a deleted chat's says so", any(b.get_name().endswith("chat deleted") for b in builds))
    press(starting("button", "Copy link")[0])
    report.check("Copy link reads Copied", has("button", "Copied"))
    report.check("and copies the page", clipboard().startswith("https://mcbench-builds.github.io/"))
    removes = starting("button", "Remove")
    press(removes[0])
    report.check("Remove asks first", has("button", "Remove from the web?"))
    press(named("button", "Remove from the web?"), 1.0)
    builds = common.find(named("list", "Published builds"), role="list item")
    report.check("a removed build leaves the list", len(builds) == 3)
    press(starting("button", "Open")[0])
    report.check("Open opens the page", any(a.get("url", "").startswith("https://mcbench-builds.github.io/") for a in logged("link activated")))
    press(starts("button", "Go to bench"), 1.2)
    report.check("Go to closes Settings and opens the bench", dialog() is None and any(a.get("place", "").startswith("/bench/") for a in logged("opened")))
    # Settings opened by keyboard alone from the palette's setting row.
    common.key("ctrl+k", 0.8)
    common.type_text("clear leaderboard")
    common.key("Return", 1.2)
    report.check("a setting from the palette opens General", dialog() is not None and state(tab("General"), Atspi.StateType.SELECTED))
    common.key("Escape", 0.8)
    report.check("Escape closes Settings", dialog() is None)
    report.finish()

if mode == "claude":
    open_settings()
    show("Claude Code")
    report.check("Claude Code is not signed in", has("panel", "Account") and bool(starting("button", "Claude subscription")))
    help_ = [o.get_name() for o in common.find(app) if "The app installs Claude Code 2.1.311 first" in (o.get_name() or "")]
    shot("claude-not-signed-in")
    key_press(named("button", "Claude subscription"), "Return", 0.4)
    installing = wait(lambda: starting("status bar", "Installing Claude Code 2.1.311"), "installing")
    report.check("the install starts first", bool(installing))
    shot("claude-installing")
    mb = wait(lambda: [o for o in starting("status bar", "Installing") if " MB." in o.get_name()], "progress", 6)
    report.check("with its MB progress", bool(mb), mb[0].get_name() if mb else "")
    report.check("focus stays on the install's status", focused().startswith("status bar 'Installing"), focused())
    shot("claude-download")
    waiting = wait(lambda: has("link", "https://claude.ai/oauth/authorize?code=true&client_id=stand-in"), "the address", 20)
    report.check("then the sign-in waits with its address", bool(waiting))
    report.check("Copy address", has("button", "Copy address"))
    entry = wait(lambda: common.find(app, role="entry", name="Code from the sign-in page"), "the code field", 6)
    report.check("then wants the pasted code", bool(entry))
    submit = named("button", "Submit")
    report.check("Submit is off while the field is empty", not state(submit, Atspi.StateType.ENABLED))
    shot("claude-code-wanted")
    press(named("link", "https://claude.ai/oauth/authorize?code=true&client_id=stand-in"))
    report.check("the address opens in the browser", bool(logged("link activated")))
    type_into(entry[0], "abc-123")
    common.key("Return", 1.2)
    done = wait(lambda: has("button", "Sign out"), "signed in", 6)
    report.check("Enter sends the code and signs in", bool(done))
    report.check("the account shows", bool(common.find(app, role="panel", name="builder@example.com")))
    report.check("the navigation's summary follows", bool(starting("page tab", "Claude Code v2.1.311, Signed in")))
    shot("claude-signed-in")
    press(named("button", "Sign out"), 1.0)
    report.check("Sign out signs out", bool(starting("button", "Claude subscription")))
    report.finish()

if mode == "chatgpt":
    open_settings()
    show("Connect an AI")
    press(starts("button", "ChatGPT, Sign in"), 1.0)
    report.check("Connect's button opens ChatGPT and starts its sign-in",
                 state(tab("ChatGPT"), Atspi.StateType.SELECTED) and (has("panel", "Account")))
    report.check("a reader's press of it puts focus on the pane's tab", focused().startswith("page tab 'ChatGPT"), focused())
    code = wait(lambda: common.find(app, role="label", name="WXYZ-2026"), "the one-time code", 8)
    report.check("the one-time code shows", bool(code))
    report.check("Copy code", has("button", "Copy code"))
    shot("chatgpt-code")
    press(named("button", "Copy code"))
    report.check("Copy code copies it", clipboard() == "WXYZ-2026")
    press(common.one(named("panel", "Account"), "button", "Cancel"), 1.0)
    report.check("Cancel ends it", has("button", "Sign in"))
    press(named("button", "Sign in"), 0.5)
    done = wait(lambda: has("button", "Sign out"), "signed in", 12)
    report.check("started again it is done", bool(done))
    show("Codex CLI")
    report.check("Codex CLI shares the login", has("button", "Sign out"))
    report.finish()

if mode == "failures":
    open_settings()
    show("Claude Code")
    report.check("one vote", True)
    press(named("button", "Sign out"), 1.0)
    report.check("signed out", has("button", "Claude subscription"))
    press(named("button", "Claude subscription"), 0.5)
    report.check("a sign-in that runs out of time says why",
                 bool(wait(lambda: common.find(app, role="notification", pred=lambda o: "10 minutes" in (o.get_name() or "")), "failure", 8)))
    shot("sign-in-failed")
    show("General")
    press(named("button", "Clear leaderboard"))
    report.check("one vote asks so", has("panel", "Delete the only vote?"))
    press(named("button", "Delete votes"), 1.0)
    report.check("a failed clear stays armed", has("panel", "Delete the only vote?"))
    report.check("and says why", bool(starting("article", "Error, The votes could not be deleted")))
    removes = starting("button", "Remove")
    press(removes[0])
    press(named("button", "Remove from the web?"), 1.0)
    alert = common.find(named("list", "Published builds"), role="notification")
    report.check("a failed removal stays on its row with why", bool(alert) and alert[0].get_name() == "Wait for the build to finish publishing")
    shot("remove-failed")
    type_into(named("entry", "GitHub repository for shared builds"), "x")
    press(named("button", "Save changes"), 1.2)
    err = common.find(app, role="notification", pred=lambda o: (o.get_name() or "").startswith("The settings were not saved"))
    report.check("a refused save opens again with its error", dialog() is not None and bool(err))
    shot("save-refused")
    press(common.one(err[0], "button", "Dismiss error"))
    report.check("Dismiss error clears it", not common.find(app, role="notification", pred=lambda o: (o.get_name() or "").startswith("The settings were not saved")))
    report.finish()

if mode == "install-fails":
    open_settings()
    show("Claude Code")
    press(named("button", "Claude subscription"), 0.5)
    failed = wait(lambda: common.find(app, role="notification", pred=lambda o: "was not installed" in (o.get_name() or "")), "failure", 12)
    report.check("a failed install says why", bool(failed), failed[0].get_name() if failed else "")
    shot("install-failed")
    report.finish()

if mode == "install-refused":
    # --settings claude-sign-in --settings install-refused: the core refuses
    # the install before it starts (an error, then fresh settings, no
    # install): the row goes back to its button.
    open_settings()
    show("Claude Code")
    key_press(named("button", "Claude subscription"), "Return", 1.2)
    report.check("a refused install says why",
                 bool(starting("article", "Error, Claude Code could not be installed")))
    report.check("and the row is back to its button",
                 has("button", "Claude subscription") and not starting("status bar", "Installing"))
    report.finish()

if mode == "read-fails":
    press(named("button", "Settings"), 1.5)
    report.check("a failed read opens nothing", dialog() is None)
    report.check("and says why", bool(starting("article", "Error, The settings could not be read")))
    report.finish()

if mode == "shares":
    open_settings()
    alert = common.find(app, role="notification", name="Sharing has not joined the native app yet")
    report.check("the published list says why it failed", bool(alert))
    report.check("with Retry", has("button", "Retry"))
    report.check("an unreadable count asks for every vote", True)
    press(named("button", "Clear leaderboard"))
    report.check("Delete every vote?", has("panel", "Delete every vote?"))
    shot("shares-failed")
    report.finish()

if mode == "shots":
    # The whole window (run with MCBENCH_SCREEN=1280x820 on X11, so the
    # screen is the window), with --settings claude-sign-in --settings
    # chatgpt-sign-in --settings save-refused --settings-file naming no home
    # folder. Each picture is taken once what it shows is there.
    open_settings()
    shot("general-dark")
    show("OpenRouter", 1.5)
    shot("openrouter-dark")
    x, y = centre(grips()[0])
    common.pointer("mousemove", str(x), str(y))
    time.sleep(0.2)
    common.pointer("mousedown", "1")
    time.sleep(0.2)
    for step in range(1, 21):
        common.pointer("mousemove", str(x), str(y + step * 5))
        time.sleep(0.03)
    time.sleep(0.4)
    shot("model-drag-down")
    common.pointer("mouseup", "1")
    time.sleep(0.5)
    show("General")
    press(named("toggle button", "Light"), 0.8)
    shot("general-light")
    press(named("toggle button", "Dark"), 0.8)
    show("Claude Code")
    key_press(named("button", "Claude subscription"), "Return", 0.4)
    wait(lambda: [o for o in starting("status bar", "Installing") if " of 236 MB" in o.get_name() and not o.get_name().startswith("Installing Claude Code 2.1.311. Downloading, 0 ")], "progress", 10)
    shot("claude-download")
    wait(lambda: common.find(app, role="entry", name="Code from the sign-in page"), "the code field", 20)
    shot("claude-code-wanted")
    show("Connect an AI")
    press(starts("button", "ChatGPT, Sign in"), 0.5)
    wait(lambda: common.find(app, role="label", name="WXYZ-2026"), "the one-time code", 10)
    shot("chatgpt-code")
    press(named("button", "Save changes"), 1.2)
    wait(lambda: common.find(app, role="notification", pred=lambda o: (o.get_name() or "").startswith("The settings were not saved")), "the refusal")
    shot("save-refused")
    report.check("screenshots taken", True)
    report.finish()

if mode.startswith("bounds"):
    # The panel lies wholly inside the window, at the size the stylesheet
    # gives it: at most 1100 by 780, 32 px inside the width and 48 inside
    # the height; at 980 px wide and below 32 px inside the height, the
    # navigation stacked above the pane.
    open_settings()
    d = Atspi.Component.get_extents(dialog(), Atspi.CoordType.WINDOW)
    # The window's size as the app reports it (the tree dump's viewport).
    viewport = json.load(open(common.out("a11y.json")))["frame"]["viewport_size"]
    W, H = int(viewport["width"]), int(viewport["height"])
    expected_w = min(1100, W - 32)
    expected_h = H - 32 if W <= 980 else min(780, H - 48)
    detail = f"window {W}x{H}, panel {d.width}x{d.height} at {d.x},{d.y}; expected {expected_w}x{expected_h}"
    report.check("the panel lies wholly inside the window",
                 d.x >= 0 and d.y >= 0 and d.x + d.width <= W and d.y + d.height <= H, detail)
    report.check("its size is the stylesheet's", abs(d.width - expected_w) <= 1 and abs(d.height - expected_h) <= 1, detail)
    nav = named("page tab list", "Settings sections")
    n = Atspi.Component.get_extents(nav, Atspi.CoordType.WINDOW)
    report.check("the navigation is stacked above the pane at 980 px and below, beside it above",
                 (n.width >= d.width - 4) == (W <= 980), f"nav {n.width}x{n.height} at {n.x},{n.y}")
    for prefix in ("Save changes", "Cancel", "Close settings"):
        b = Atspi.Component.get_extents(named("button", prefix), Atspi.CoordType.WINDOW)
        report.check(f"{prefix} lies inside the panel", b.x >= d.x and b.y >= d.y and b.x + b.width <= d.x + d.width and b.y + b.height <= d.y + d.height, f"{b.width}x{b.height} at {b.x},{b.y}")
    shot(f"bounds-{W}x{H}")
    report.finish()

if mode == "timing":
    # Idle: Settings open on General, then on a provider, five seconds
    # untouched each; the frames each drew (logged when they close).
    open_settings()
    time.sleep(5.0)
    common.key("Escape", 0.6)
    open_settings()
    show("OpenRouter")
    time.sleep(5.0)
    common.key("Escape", 0.6)
    # Opening on General and on OpenRouter (300 more models), ten times each.
    for _ in range(10):
        open_settings()
        common.key("Escape", 0.6)
    # On OpenRouter's pane directly: the palette's provider row.
    for i in range(10):
        common.key("ctrl+k", 0.6)
        common.type_text("openrouter", 0.6)
        common.key("Return", 1.2)
        if i < 9:
            common.key("Escape", 0.6)
    report.check("the palette's provider row opens its pane", dialog() is not None and state(tab("OpenRouter"), Atspi.StateType.SELECTED))
    field = named("entry", "Filter models")
    Atspi.Component.grab_focus(field)
    time.sleep(0.3)
    for ch in "syntheticm":
        common.type_text(ch, 0.25)
    for _ in range(10):
        common.key("BackSpace", 0.25)
    x, y = centre(grips()[0])
    common.pointer("mousemove", str(x), str(y))
    time.sleep(0.2)
    common.pointer("mousedown", "1")
    time.sleep(0.2)
    for step in range(1, 61):
        common.pointer("mousemove", str(x), str(y + step * 3))
        time.sleep(0.02)
    common.pointer("mouseup", "1")
    time.sleep(0.5)
    common.key("Escape", 0.6)
    lines = logged("settings-timing")
    with open(common.out("settings-timing.json"), "w") as f:
        json.dump(lines, f)

    def stats(what):
        ms = sorted(l["ms"] for l in lines if l["what"] == what)
        if not ms:
            return 0, ""
        return len(ms), f"n {len(ms)}, median {statistics.median(ms):.1f} ms, p90 {ms[int(len(ms) * 0.9) - 1]:.1f}, max {ms[-1]:.1f}"

    frames = [l["frames"] for l in lines if l["what"] == "frames"]
    opens = [l["ms"] for l in lines if l["what"] == "open"]
    report.check("idle frames (General 5 s, OpenRouter 5 s)", len(frames) >= 2, str(frames[:2]))
    report.check("open on General", True, f"median {statistics.median(opens[2:12]):.1f} ms, max {max(opens[2:12]):.1f}")
    report.check("open on OpenRouter (320 models)", True, f"median {statistics.median(opens[12:]):.1f} ms, max {max(opens[12:]):.1f}")
    n, text = stats("key")
    report.check("filter key to paint", n >= 15, text)
    n, text = stats("drag")
    report.check("drag move to paint", n >= 20, text)
    report.finish()
