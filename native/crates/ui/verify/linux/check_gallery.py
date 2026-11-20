"""The shared controls (mcbench-ui-dev --gallery) through AT-SPI: each
button's role, name and enabled state; a press by action reaches it (the
action log); the switch's checked state flips; the confirm button asks
first; the popover menu opens on its trigger's action, puts focus on its
first item, its arrow keys wrap, and Escape closes it back to its trigger.

Run with MCBENCH_APP_ARGS=--gallery (run.sh)."""

import os
import time

import common
from common import Atspi

report = common.Report("gallery")
app = common.app()
time.sleep(2.0)
common.focus_window()


def button(name):
    # settings agent: the gallery's Settings section repeats some names
    # (a sign-in's `Copy code`); the shared controls come first.
    found = common.find(app, role="button", name=name)
    if not found:
        raise AssertionError(f"no button {name!r}")
    return found[0]


def enabled(obj):
    return obj.get_state_set().contains(Atspi.StateType.ENABLED)


for name in ["Raised", "Primary", "Delete for good", "Run bench", "Share", "New chat", "Copy code", "Remove"]:
    report.check(f"{name} is an enabled button", enabled(button(name)))
for name in ["Raised, disabled", "Primary, disabled", "Run bench, disabled", "Settings, disabled"]:
    report.check(f"{name} is not enabled", not enabled(button(name)))

Atspi.Action.do_action(button("Raised"), 0)
time.sleep(0.5)
pressed = [a.get("control") for a in common.actions() if a["action"] == "pressed"]
report.check("a press by action reaches the button", pressed == ["Raised"], str(pressed))

switch = common.find(app, name="Show the grid")
report.check("the switch is one node", len(switch) == 1, str([common.label(s) for s in switch]))
def checked():
    states = switch[0].get_state_set()
    return states.contains(Atspi.StateType.CHECKED) or states.contains(Atspi.StateType.PRESSED)


report.check("the switch is on at start (checked or pressed)", checked(),
             str([s.value_nick for s in switch[0].get_state_set().get_states()]))
was = checked()
Atspi.Action.do_action(switch[0], 0)
time.sleep(0.5)
report.check("its press flips it", checked() != was, f"{common.label(switch[0])} {was}")

Atspi.Action.do_action(button("Remove"), 0)
time.sleep(0.5)
report.check("the confirm button asks first", len(common.find(app, role="button", name="Remove from the web?")) == 1)

trigger = button("Effort: medium")
Atspi.Action.do_action(trigger, 0)
time.sleep(0.8)
menus = common.find(app, role="menu", name="Effort")
report.check("the trigger opens its menu", len(menus) == 1)
report.check("focus is on the first item", common.label(common.focused(app)) == "radio menu item 'low'",
             common.label(common.focused(app)))
common.key("Up", settle=0.4)
report.check("Up from the first wraps to the last", common.focused(app).get_name() == "high", common.label(common.focused(app)))
common.key("Escape", settle=0.8)
report.check("Escape closes it", not common.find(app, role="menu", name="Effort"))
report.check("and focus is back on its trigger", common.focused(app).get_name() == "Effort: medium",
             common.label(common.focused(app)))
# settings agent: Settings' controls in both themes (the gallery's Settings
# section, scrolled into view by the wheel).
report.check("Settings' controls are in the gallery",
             bool(common.find(app, role="password text", name="API key"))
             and bool(common.find(app, role="page tab list", name="Settings sections")))
common.pointer("mousemove", "640", "400")
shots = []
for theme in ("dark", "light"):
    if theme == "light":
        for _ in range(400):
            common.pointer("click", "4")
        time.sleep(0.5)
        Atspi.Action.do_action(button("Light theme"), 0)
        time.sleep(0.8)
    for _ in range(400):
        common.pointer("click", "5")
    time.sleep(0.6)
    for n in range(6):
        shots.append(common.screenshot(f"gallery-settings-{theme}-{n}.png"))
        for _ in range(10):
            common.pointer("click", "4")
        time.sleep(0.5)
report.check("pictures of Settings' controls in both themes", len(shots) == 12)
# bench agent: Bench's toggles, pressed or not, in both themes (light now).
toggles = {n: common.find(app, role="toggle button", name=n) for n in
           ("Pick Build A as the best build", "Take back the pick of Build B", "Take back the tie", "Blind", "Round 1", "Round 2")}
report.check("Bench's toggles are toggle buttons", all(toggles.values()), str({k: len(v) for k, v in toggles.items()}))
report.check("a pick is pressed and a vote not",
             all(toggles.values()) and toggles["Take back the pick of Build B"][0].get_state_set().contains(Atspi.StateType.PRESSED)
             and not toggles["Pick Build A as the best build"][0].get_state_set().contains(Atspi.StateType.PRESSED))
if toggles["Blind"]:
    Atspi.Component.grab_focus(toggles["Blind"][0])
    time.sleep(0.6)
    common.screenshot("gallery-bench-light.png")
    Atspi.Action.do_action(button("Dark theme"), 0)
    time.sleep(0.8)
    Atspi.Component.grab_focus(toggles["Blind"][0])
    time.sleep(0.6)
    common.screenshot("gallery-bench-dark.png")
# pane agent: the 3D pane in its states and the selection chips' squares
# (a block's icon, the core's Err, empty space), pictured in both themes
# (dark now), the section's first pane scrolled near the window's top.
PANE_STATES = {
    "not joined": "The 3D viewer has not joined the app yet",
    "no adapter": "No graphics adapter",
    "placement failed": "Minecraft kept the previous world",
    "stopped": "The live world stopped: the engine ran out of memory",
}


def texts():
    said = []
    for n in common.find(app):
        said.append(n.get_name() or "")
        said.append(n.get_description() or "")
    return "\n".join(said)


said = common.wait_for(lambda: (lambda t: t if all(v in t for v in PANE_STATES.values()) else None)(texts()),
                       15.0, "the pane's states")
for state, sentence in PANE_STATES.items():
    report.check(f"the gallery's pane shows its {state} state", bool(said) and sentence in said)
report.check("the gallery's running pane has its toolbar", bool(common.find(app, name="Live world")))


def sky_top():
    """The first window row where daytime sky runs 440 px or more: the
    running pane's, under its bar (the smaller panes are 380 px wide).
    GPUI's accessibility bounds do not follow the wheel."""
    from PIL import Image
    path = common.screenshot("tmp-gallery.png")
    image = Image.open(path).convert("RGB")
    os.remove(path)
    w, h = image.size
    pixels = image.load()
    for y in range(0, h, 4):
        run = 0
        for x in range(0, min(w, 1280), 4):
            r, g, b = pixels[x, y]
            run = run + 1 if (b > 200 and r < 160 and 140 < g < 200) else 0
            if run >= 110:
                return y
    return None


def scroll_to_panes():
    """From the gallery's end, wheels up until that row is 80 to 250 px
    down; the rows seen."""
    common.pointer("mousemove", "640", "400")
    for _ in range(400):
        common.pointer("click", "5")
    time.sleep(0.6)
    seen = []
    for _ in range(150):
        y = sky_top()
        seen.append(y)
        if y is not None and 80 <= y <= 250:
            return True, seen
        common.pointer("click", "4")
        time.sleep(0.3)
    return False, seen


pane_shots = []
for theme in ("dark", "light"):
    if theme == "light":
        Atspi.Action.do_action(button("Light theme"), 0)
        time.sleep(0.8)
    ok, seen = scroll_to_panes()
    report.check(f"the 3D pane section in view ({theme})", ok, str(seen[:6] + seen[-6:]))
    time.sleep(0.8)
    pane_shots.append(common.screenshot(f"gallery-pane-{theme}.png"))
report.check("pictures of the 3D pane's states in both themes", len(pane_shots) == 2)
common.quit_key(settle=2.0)
report.finish()
