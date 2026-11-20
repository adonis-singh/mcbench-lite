"""Keyboard alone: the whole Tab walk with a screenshot of each focus stop
(focus-NN.png), the chat list's and the transcript's keys, Tab into a
message's parts, Enter and Space on them, the composer's Enter, Shift+Enter
and Escape, and Ctrl+A then Ctrl+C in the transcript with the clipboard
read back. Every key is a real key event on the private display."""

import time

import common

Atspi = common.Atspi
report = common.Report("keyboard")
shots = 0


def focused():
    return common.focused(common.app())


def at(what):
    return common.label(focused())


def shot():
    global shots
    shots += 1
    common.screenshot(f"focus-{shots:02}.png")


def expect(key, role, name=None, prefix=None, settle=0.4):
    common.key(key, settle=settle)
    f = focused()
    ok = f is not None and f.get_role_name() == role and (
        (name is None or f.get_name() == name) and (prefix is None or f.get_name().startswith(prefix))
    )
    # A message's name is its sender and its text: matched by its start.
    report.check(f"{key} focuses {role} {name or prefix or ''}".strip(), ok, common.label(f))
    shot()
    return f


common.app()
time.sleep(2.0)
common.focus_window()
report.check("the composer has focus at start", at("") == "entry 'Message'", at(""))
shot()

# The Tab walk from the composer around the window, and back: the sidebar
# top to bottom, the transcript's current message, the composer, Attach
# file and the footer's pickers (chat agent: the composer's footer, in the
# page's order), Send.
expect("Tab", "button", "Attach file")
# bench agent's prompts button follows Attach file (pane agent: added here).
expect("Tab", "toggle button", "Insert a saved prompt")
expect("Tab", "button", "Resolution: Standard")
expect("Tab", "button", "Chat model: claude-opus-5-5")
expect("Tab", "button", "Reasoning effort: medium")
# pane agent: the 3D pane after the chat column: its canvas, its bar (the
# switches are toggle buttons), the zoom buttons and the live world's
# toolbar (Step is disabled while playing: no stop).
expect("Tab", "canvas", "Live Minecraft world")
for role, name in [
    # BlueMap's bar beside the chat (narrower than its 576 px break: no
    # Day/Night, view switch or Reset), then the mcbench panel.
    ("entry", "Camera position x"),
    ("entry", "Camera position z"),
    ("button", "Compass / Face North"),
    ("toggle button", "Select blocks"),
    ("button", "Download as a WorldEdit schematic (.schem)"),
    ("button", "Export as a Minecraft world (a zip to unpack in .minecraft/saves)"),
    ("button", "Fill the window"),
    ("button", "Zoom in"),
    ("button", "Zoom out"),
    ("button", "Pause"),
    ("button", "Restart from the build"),
]:
    expect("Tab", role, name, settle=0.2)
expect("Tab", "button", "mcbuild")
expect("Tab", "button", "New chat")
expect("Tab", "button", "Collapse sidebar")
# sidebar agent: the search, its filter button, the current row's archive
# button and the archived section's toggle.
expect("Tab", "entry", "Search chats")
expect("Tab", "button", "Filter")
expect("Tab", "list item", "Redstone lamp with a lever, open")
expect("Tab", "button", "Archive build")
expect("Tab", "button", "Show archived")
expect("Tab", "button", "Settings")
expect("Tab", "article", prefix="claude-opus-5-5, ")
expect("Tab", "entry", "Message")
common.type_text("x")
expect("Tab", "button", "Attach file")
# bench agent's prompts button follows Attach file (pane agent: added here).
expect("Tab", "toggle button", "Insert a saved prompt")
expect("Tab", "button", "Resolution: Standard")
expect("Tab", "button", "Chat model: claude-opus-5-5")
expect("Tab", "button", "Reasoning effort: medium")
expect("Tab", "button", "Send message")
expect("shift+Tab", "button", "Reasoning effort: medium")
expect("shift+Tab", "button", "Chat model: claude-opus-5-5")
expect("shift+Tab", "button", "Resolution: Standard")
expect("shift+Tab", "toggle button", "Insert a saved prompt")
expect("shift+Tab", "button", "Attach file")
expect("shift+Tab", "entry", "Message")
common.key("ctrl+a", settle=0.2)
common.key("BackSpace", settle=0.3)
# Shift+Tab walks back the same way.
expect("shift+Tab", "article", prefix="claude-opus-5-5, ")
expect("shift+Tab", "button", "Settings")
expect("shift+Tab", "button", "Show archived")
expect("shift+Tab", "button", "Archive build")
expect("shift+Tab", "list item", "Redstone lamp with a lever, open")
expect("shift+Tab", "button", "Filter")
expect("shift+Tab", "entry", "Search chats")
expect("shift+Tab", "button", "Collapse sidebar")
expect("shift+Tab", "button", "New chat")

# The chat list.
expect("Tab", "button", "Collapse sidebar")
expect("Tab", "entry", "Search chats")
expect("Tab", "button", "Filter")
expect("Tab", "list item", "Redstone lamp with a lever, open")
expect("Down", "list item", prefix="Castle gate")
expect("End", "list item", prefix="Wheat farm, Finished, not opened yet")
expect("Home", "list item", "Redstone lamp with a lever, open")
expect("Down", "list item", prefix="Castle gate")
common.key("Return", settle=0.8)
report.check("Enter opens the chat", any(a["action"] == "opened chat" and a["chat"] == "chat-gate" for a in common.actions()))
expect("Up", "list item", "Redstone lamp with a lever")
common.key("Return", settle=0.8)

# The transcript: messages by Up, Down, Home, End, Page Up, Page Down.
expect("Tab", "button", "Archive build")
expect("Tab", "button", "Show archived")
expect("Tab", "button", "Settings")
expect("Tab", "article", prefix="claude-opus-5-5, ")
first = expect("Home", "article", prefix="You, ")
expect("Down", "article", prefix="claude-opus-5-5, ")
expect("Page_Down", "article", None)
expect("Page_Up", "article", prefix="claude-opus-5-5, ")
expect("End", "article", prefix="claude-opus-5-5, ")

# Tab from a message goes into its parts: the work fold, then Enter opens it.
common.key("Home", settle=0.4)
expect("Down", "article", prefix="claude-opus-5-5, ")
fold = expect("Tab", "button", prefix="Worked")
common.key("Return", settle=0.6)
fold = focused()
report.check("Enter opens the fold", fold is not None and fold.get_state_set().contains(Atspi.StateType.EXPANDED), common.label(fold))
expect("Tab", "button", "Reasoning")
expect("Tab", "button", "Built: a lamp with a lever")
common.key("space", settle=0.6)
report.check("Space opens the tool row", any(a["action"] == "toggled" and a["key"] == "call-build" and a["open"] for a in common.actions()))
expect("Tab", "button", "Ran command: ls -la /work")
# The next message's link and Copy code button.
common.key("shift+Tab", settle=0.3)
common.key("shift+Tab", settle=0.3)
common.key("shift+Tab", settle=0.3)
common.key("shift+Tab", settle=0.3)
expect("Down", "article", prefix="claude-opus-5-5, ")
expect("Tab", "link", "the wiki page on lamps")
common.key("Return", settle=0.5)
report.check("Enter activates the link", any(a["action"] == "link activated" for a in common.actions()))
expect("Tab", "button", "Copy code")
common.key("space", settle=0.5)
report.check("Space activates Copy code", any(a["action"] == "copied" and a["text"].startswith("fn build(") for a in common.actions()))
report.check("Copy code reached the clipboard", (common.clipboard() or "").startswith("fn build("), (common.clipboard() or "")[:30])

# Select all and copy in the transcript.
common.key("ctrl+a", settle=0.5)
common.key("ctrl+c", settle=0.5)
copied = common.clipboard() or ""
report.check(
    "Ctrl+A, Ctrl+C copies the whole transcript",
    copied.startswith("Build a redstone lamp") and copied.rstrip().endswith("nothing else moved."),
    f"{len(copied)} characters: {copied[:40]!r} ... {copied[-40:]!r}",
)
report.check("a table copies as tab-separated rows", "Block\tCount\tLight\nstone\t9\t0" in copied)
report.check("code copies verbatim", "fn build(world: &mut World) {\n    world.fill(" in copied)
shot()

# The composer: Shift+Enter is a new line, Enter sends, Escape keeps a streaming turn.
common.key("End", settle=0.3)
expect("Tab", "entry", "Message")
common.type_text("one")
common.key("shift+Return", settle=0.3)
common.type_text("two")
composer = focused()
count = Atspi.Text.get_character_count(composer)
report.check("Shift+Enter makes a new line", Atspi.Text.get_text(composer, 0, count) == "one\ntwo", repr(Atspi.Text.get_text(composer, 0, count)))
common.key("Return", settle=0.8)
report.check("Enter sends", any(a["action"] == "sent" and a["text"] == "one\ntwo" for a in common.actions()))
report.check("Stop appears while streaming", len(common.find(common.app(), "button", "Stop generating")) == 1)
common.key("Escape", settle=0.5)
report.check("Escape while streaming stops nothing", not any(a["action"] == "stop" for a in common.actions()) and len(common.find(common.app(), "button", "Stop generating")) == 1)
expect("Tab", "button", "Attach file")
# bench agent's prompts button follows Attach file (pane agent: added here).
expect("Tab", "toggle button", "Insert a saved prompt")
expect("Tab", "button", "Resolution: Standard")
expect("Tab", "button", "Chat model: claude-opus-5-5")
expect("Tab", "button", "Reasoning effort: medium")
expect("Tab", "button", "Stop generating")
shot()
common.key("Return", settle=1.5)
report.check("Enter on Stop stops the turn", any(a["action"] == "stop" for a in common.actions()))
common.quit_key(settle=1.5)
report.finish()
