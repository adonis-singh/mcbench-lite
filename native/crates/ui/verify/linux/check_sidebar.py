"""The sidebar with real keys and AT-SPI (the gate fixture, run with
`--running chat-gate`): the Build list
walked with Up, Down, Home and End and opened with Enter and Space; a row
archived through AT-SPI, the open chat moving to the next row; the archived
section opened by Enter, a row restored with Space, Delete permanently
armed (said once in the polite region, nothing said when it disarms) and
pressed twice; the sidebar search
narrowing the list and Escape clearing it; the filter menu (focus on its
first value, Up wrapping, Escape giving focus back to the button, a value
chosen with Space, its chip removed with Enter, focus back on the button,
Tab out of it to the stop after the button with its ring, Shift+Tab to the
stop before it);
the palette (Ctrl+K, typing, the results, Enter running the highlighted
item, Escape closing it); Bench's list (Ctrl+2, the fold opened with Enter,
its models, one opened with Enter). Each step is checked in AT-SPI's tree,
the focus, and the dev binary's action log (sidebar.json)."""

import time

import common

Atspi = common.Atspi
report = common.Report("sidebar")
common.X_KEYS.update({"ctrl+k": "ctrl+k", "ctrl+1": "ctrl+1", "ctrl+2": "ctrl+2"})


def app():
    return common.app()


def focused():
    return common.focused(app())


def name_of(obj):
    return obj.get_name() if obj is not None else None


def logged(action, **fields):
    return any(a["action"] == action and all(a.get(k) == v for k, v in fields.items()) for a in common.actions())


def expect(key, role, name=None, prefix=None, settle=0.4):
    common.key(key, settle=settle)
    f = focused()
    ok = f is not None and f.get_role_name() == role and (
        (name is None or f.get_name() == name) and (prefix is None or f.get_name().startswith(prefix))
    )
    report.check(f"{key} focuses {role} {name or prefix or ''}".strip(), ok, common.label(f))
    return f


def grab(obj):
    Atspi.Component.grab_focus(obj)
    time.sleep(0.4)


def press(obj):
    Atspi.Action.do_action(obj, 0)
    time.sleep(0.6)


def rows(list_name):
    found = common.find(app(), "list", list_name)
    if not found:
        return []
    out = []
    for i in range(found[0].get_child_count()):
        child = found[0].get_child_at_index(i)
        if child.get_role_name() == "list item":
            out.append(child.get_name())
    return out


def window_title():
    return app().get_child_at_index(0).get_name()


app()
time.sleep(2.0)
common.focus_window()

# The Build list: each row's status in its name.
names = rows("Chats")
report.check(
    "the Build list names each row's status",
    names == [
        "Redstone lamp with a lever, open",
        "Castle gate, Working",
        "Wheat farm, Finished, not opened yet, Resolution: Very high (5×)",
    ],
    repr(names),
)
chats = common.one(app(), "list", "Chats")
report.check("the open chat says open in its name, not in a description",
             [r.get_description() for r in common.find(chats, "list item")] == ["", "", ""],
             repr([r.get_description() for r in common.find(chats, "list item")]))
report.check("the list's rows have position and size", all(
    common.find(chats, "list item")[i].get_attributes().get("posinset") == str(i + 1)
    and common.find(chats, "list item")[i].get_attributes().get("setsize") == "3"
    for i in range(3)
), repr([r.get_attributes() for r in common.find(chats, "list item")]))

# seen: a finished chat opened while the window is hidden stays unread
# (X11: the window unmapped, as minimising does); opened while the window
# is visible but not active (the input focus on no window) it is marked
# seen: the signal is GPUI's visibility, not activation. On Wayland the
# window is covered by a second client cage activates.
def farm():
    return [r for r in rows("Chats") if r.startswith("Wheat farm")]


def open_row(title):
    press(common.row(app(), title))
    time.sleep(0.6)


if common.BACKEND == "x11":
    report.check("the window is unmapped", common.set_window_mapped(False) == 1)
    open_row("Wheat farm")
    open_row("Redstone lamp with a lever")
    common.set_window_mapped(True)
    common.focus_window()
    time.sleep(1.0)
    report.check("opened while hidden: still unread", farm() == ["Wheat farm, Finished, not opened yet, Resolution: Very high (5×)"], repr(farm()))
    report.check("both opens happened", logged("opened chat", chat="chat-farm") and logged("opened chat", chat="chat-lamp"))
    other = common.deactivate_window()
    open_row("Wheat farm")
    time.sleep(1.0)
    open_row("Redstone lamp with a lever")
    time.sleep(1.0)
    report.check("opened while visible and inactive: marked seen", farm() == ["Wheat farm, Resolution: Very high (5×)"], repr(farm()))
    common.focus_window()
else:
    other = common.deactivate_window()
    open_row("Wheat farm")
    open_row("Redstone lamp with a lever")
    other.terminate()
    other.wait()
    time.sleep(1.5)
    common.focus_window()
    # cage sends no `suspended`, so GPUI reports the covered window visible.
    report.check("opened while covered by another client (still visible to GPUI): marked seen", farm() == ["Wheat farm, Resolution: Very high (5×)"], repr(farm()))
common.screenshot("sidebar-seen.png")

# Walking the list opens nothing; Enter and Space open.
grab(common.row(app(), "Redstone lamp with a lever"))
expect("Down", "list item", "Castle gate, Working")
expect("End", "list item", prefix="Wheat farm")
expect("Home", "list item", "Redstone lamp with a lever, open")
report.check("walking opened nothing", not logged("opened chat", chat="chat-gate"))
expect("Down", "list item", "Castle gate, Working")
common.key("space", settle=0.8)
report.check("Space opens the focused row", logged("opened chat", chat="chat-gate"))
expect("Down", "list item", prefix="Wheat farm")
farm_opens = len([a for a in common.actions() if a["action"] == "opened chat" and a.get("chat") == "chat-farm"])
common.key("Return", settle=1.0)
report.check("Enter opens the focused row", len([a for a in common.actions() if a["action"] == "opened chat" and a.get("chat") == "chat-farm"]) == farm_opens + 1)
common.wait_for(lambda: "Wheat farm, open, Resolution: Very high (5×)" in rows("Chats"), 5.0, "the row seen")
report.check("the open chat is marked seen", "Wheat farm, open, Resolution: Very high (5×)" in rows("Chats"), repr(rows("Chats")))

# Archive the open chat through AT-SPI: the next row takes its place, or the
# nearest above.
row_buttons = common.find(common.row(app(), "Wheat farm"), "button", "Archive build")
press(row_buttons[0])
common.wait_for(lambda: not any(r.startswith("Wheat farm") for r in rows("Chats")), 5.0, "the row archived")
report.check("Archive build archives the row", logged("archive", chat="chat-farm"))
report.check("the open chat moves to the nearest row above", window_title() == "Castle gate · mcbench-lite", window_title())
running = common.find(common.row(app(), "Castle gate"), "button", "Archive build")
report.check(
    "the running chat's archive button is disabled and says why",
    running and not running[0].get_state_set().contains(Atspi.StateType.SENSITIVE)
    and running[0].get_description() == "Stop the running turn before archiving",
    common.label(running[0]) + " " + repr(running[0].get_description()) if running else "none",
)

# The archived section: Enter opens it, Space on Restore brings the row back.
toggle = common.one(app(), "button", "Show archived")
grab(toggle)
common.key("Return", settle=1.0)
toggle = common.one(app(), "button", "Hide archived")
report.check("Enter opens the archived section", toggle.get_state_set().contains(Atspi.StateType.EXPANDED))
archived = rows("Archived")
report.check("the archived rows say the time left", any(r == "Wheat farm, 7 days left" for r in archived), repr(archived))
farm = common.find(app(), "list item", "Wheat farm, 7 days left")[0]
grab(common.find(farm, "button", "Restore")[0])
common.key("space", settle=1.5)
common.wait_for(lambda: any(r.startswith("Wheat farm") for r in rows("Chats")), 5.0, "the row restored")
report.check("Space on Restore restores it and opens it", window_title() == "Wheat farm · mcbench-lite", window_title())
# chat-fix: arming Delete permanently says the armed text once in the
# polite region; disarming after 4 s says nothing; the second press deletes.
def polite():
    return [o.get_name() for o in common.find(app(), "status bar") if o.get_name()]


toggle = common.find(app(), "button", "Show archived")
if toggle:
    press(toggle[0])
mill = common.find(app(), "list item", pred=lambda o: o.get_name().startswith("Old windmill, ") and o.get_name().endswith(" left"))
report.check("the archived windmill is listed", len(mill) == 1, repr(rows("Archived")))
grab(common.find(mill[0], "button", "Delete permanently")[0])
common.key("space", settle=0.8)
report.check("arming says Delete for good? in the polite region", "Delete for good?" in polite(), repr(polite()))
report.check("the armed row says it in its name", "Old windmill, Delete for good?" in rows("Archived"), repr(rows("Archived")))
said = polite()
time.sleep(4.5)
report.check("disarming after 4 s says nothing new", polite() == said and any(r.startswith("Old windmill, ") and r.endswith(" left") for r in rows("Archived")), repr(polite()))
common.key("space", settle=0.6)
common.key("space", settle=1.0)
report.check("a second press within 4 s deletes it", logged("delete", id="chat-windmill"), repr([a for a in common.actions() if a["action"].startswith("delete")]))
common.screenshot("sidebar-archived.png")

# The sidebar search: typing narrows the list with the snippet in the name;
# Escape clears it.
grab(common.one(app(), "entry", "Search chats"))
common.type_text("portcullis", settle=1.0)
found = rows("Chats")
report.check("the search narrows the list by its match", len(found) == 1 and found[0].startswith("Castle gate, Working, ") and "portcullis" in found[0], repr(found))
common.screenshot("sidebar-search.png")
common.key("Escape", settle=0.8)
report.check("Escape clears the search", len(rows("Chats")) == 3, repr(rows("Chats")))

# The filters.
button = common.one(app(), "button", "Filter")
grab(button)
common.key("Return", settle=0.6)
report.check("the menu opens on its first value", name_of(focused()) == "Standard", common.label(focused()))
expect("Up", "check menu item", "claude-opus-5-5")
expect("Down", "check menu item", "Standard")
common.key("Escape", settle=0.6)
report.check("Escape gives focus back to the button", name_of(focused()) == "Filter", common.label(focused()))
common.key("Return", settle=0.6)
expect("Down", "check menu item", "High (2×)")
expect("Down", "check menu item", "Very high (5×)")
common.key("space", settle=0.6)
value = focused()
report.check("Space chooses a value and the menu stays", value is not None and value.get_state_set().contains(Atspi.StateType.CHECKED), common.label(value))
common.screenshot("sidebar-filter-menu.png")
common.key("Escape", settle=0.6)
report.check("the button says how many are chosen", name_of(focused()) == "Filter, 1 chosen", common.label(focused()))
report.check("the list is narrowed", len(rows("Chats")) == 1, repr(rows("Chats")))
chip = common.one(app(), "button", "Remove filter Size: Very high (5×)")
grab(chip)
common.key("Return", settle=0.6)
report.check("removing the last chip gives focus to the filter button", name_of(focused()) == "Filter", common.label(focused()))
report.check("no filter: every row", len(rows("Chats")) == 3, repr(rows("Chats")))
# Tab out of the menu goes on from the Filter button, with its ring (the
# screenshot); Shift+Tab to the stop before the button.
common.key("Return", settle=0.6)
expect("Down", "check menu item", "High (2×)")
common.key("Tab", settle=0.8)
now = focused()
report.check("Tab out of the menu goes to the stop after the button (the current row)",
             now is not None and now.get_role_name() == "list item" and now.get_parent().get_name() == "Chats",
             common.label(now))
report.check("Tab closes the menu", not common.find(app(), "menu", "Filter"))
common.screenshot("sidebar-filter-tabout.png")
expect("shift+Tab", "button", "Filter")
common.key("Return", settle=0.6)
common.key("shift+Tab", settle=0.8)
report.check("Shift+Tab out of the menu goes to the stop before the button", name_of(focused()) == "Search chats", common.label(focused()))
report.check("Shift+Tab closes the menu", not common.find(app(), "menu", "Filter"))

# The palette.
common.key("ctrl+k", settle=0.8)
dialog = common.find(app(), "dialog", "Search everything")
report.check("Ctrl+K opens the palette", len(dialog) == 1)
report.check("its field has focus", common.label(focused()) == "entry 'Search everything'", common.label(focused()))
common.type_text("gate", settle=1.0)
options = [o.get_name() for o in common.find(common.one(app(), "dialog", "Search everything"), "list item")]
report.check("the core's builds are listed", any(o.startswith("Castle gate") for o in options), repr(options[:6]))
common.screenshot("sidebar-palette.png")
common.key("Return", settle=1.0)
report.check("Enter runs the highlighted item", window_title() == "Castle gate · mcbench-lite", window_title())
report.check("and closes the palette", not common.find(app(), "dialog", "Search everything"))
common.key("ctrl+k", settle=0.8)
common.key("Escape", settle=0.6)
report.check("Escape closes the palette", not common.find(app(), "dialog", "Search everything"))

# Bench.
common.key("ctrl+2", settle=1.0)
benches = rows("Benches")
report.check("Ctrl+2 shows the bench list", benches == ["Towers and a bridge"], repr(benches))
grab(common.row(app(), "Towers and a bridge"))
expect("Tab", "button", "Show models")
common.key("Return", settle=0.8)
fold = common.one(app(), "button", "Hide models")
report.check("Enter opens the fold", fold.get_state_set().contains(Atspi.StateType.EXPANDED))
models = rows("Towers and a bridge models")
report.check("the bench's models by their letters", models == ["Build A, Stopped or failed", "Build B"], repr(models))
grab(common.row(app(), "Towers and a bridge"))
expect("Down", "list item", "Build A, Stopped or failed")
common.key("Return", settle=1.0)
report.check("Enter opens a model's chat", logged("opened", place="/bench/bench-towers/member-b"), repr([a for a in common.actions() if a["action"] == "opened"][-1:]))
models = rows("Towers and a bridge models")
report.check("the open model says open in its name", models == ["Build A, open, Stopped or failed", "Build B"], repr(models))
common.screenshot("sidebar-bench.png")
prompts = rows("Prompts")
report.check("the prompts' titles below", prompts == ["Wheat field", "Two towers"], repr(prompts))
common.quit_key(settle=1.5)
report.finish()
