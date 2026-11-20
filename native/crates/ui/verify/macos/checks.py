"""The shell and chat milestone's checks on macOS, in the guest VM (helpers in
`macpass.py`). Each part launches `mcbench-ui-dev` with its own flags and
writes $OUT/evidence/<run>/<part>/checks.json.

usage: python3 -I checks.py <run> <part>...
Environment: what vm/env.sh needs (RIG_HOME, OUT, VM)."""
import json
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from macpass import *  # noqa: F401,F403,E402
import macpass as m  # noqa: E402

GATE = "--fixture gate"


def rows(t, list_title):
    lst = m.one(t, role="AXList", title=list_title)
    if not lst:
        return []
    depth = lst["path"].count(".") + 1
    return [n for n in t if n["path"].startswith(lst["path"] + ".") and n["path"].count(".") == depth]


def chat_row(p, title):
    """The Build list's row whose name is the title, or starts with it and a comma."""
    for r in rows(m.tree(p), "Chats"):
        if m.name(r) == title or m.name(r).startswith(title + ", "):
            return r
    raise RuntimeError(f"no chat row {title!r}: {row_names(p, 'Chats')}")


def row_names(p, list_title):
    return [m.name(r) for r in rows(m.tree(p), list_title)]


@m.part
def p_sidebar(pt):
    """The Build list: names with status, walking without opening, Enter and
    Space, archive, the running row's archive button, search and Escape."""
    p = m.launch(f"{GATE} --running chat-gate")
    t = m.tree(p)
    lst = m.one(t, role="AXList", title="Chats")
    pt.check("the Build list is an AXList named Chats", lst is not None, m.summary(lst))
    names = row_names(p, "Chats")
    pt.check("each row's name says open and its status", names == [
        "Redstone lamp with a lever, open", "Castle gate, Working",
        "Wheat farm, Finished, not opened yet, Resolution: Very high (5×)"], names)
    r0 = rows(t, "Chats")
    pt.note("a row's attributes", m.node(p, r0[1]["path"]))
    # A list, not a list box (chats.rs): the open chat says `open` in its
    # name and is aria-current; no description.
    helps = [m.node(p, r["path"]).get("AXHelp") for r in r0]
    pt.check("no row has a description", all(h in (None, "<error -25212>") for h in helps), helps)
    pt.note("the open row's every attribute (what AX gives for aria-current)", m.describe_all(p, r0[0]["path"]))
    # The running row's Archive build: disabled, its description the refusal.
    gate_row = r0[1]
    arch = [n for n in t if n["path"].startswith(gate_row["path"] + ".") and m.name(n) == "Archive build"]
    a = m.node(p, arch[0]["path"]) if arch else {}
    pt.check("the running chat's Archive build is not enabled", arch and a.get("AXEnabled") is False, m.summary(a))
    pt.check("its help says why", a.get("AXHelp") == "Stop the running turn before archiving", a.get("AXHelp"))
    a0 = [n for n in t if n["path"].startswith(r0[0]["path"] + ".") and m.name(n) == "Archive build"]
    pt.check("an idle row's Archive build says what it does",
             m.node(p, a0[0]["path"]).get("AXHelp") == "Archive build (it can be restored for 7 days)",
             m.node(p, a0[0]["path"]).get("AXHelp"))
    # Walk the list with real keys: Tab from the composer to the current row.
    m.keys(*["tab"] * 10)
    pt.check("Tab reaches the current row", m.fname(p) == ("AXGroup", "Redstone lamp with a lever, open"), m.fname(p))
    before = len(m.log())
    for k, want in [("down", "Castle gate, Working"), ("end", "Wheat farm, Finished, not opened yet, Resolution: Very high (5×)"),
                    ("home", "Redstone lamp with a lever, open"), ("down", "Castle gate, Working")]:
        m.keys(k)
        pt.check(f"{k} focuses {want}", m.fname(p) == ("AXGroup", want), m.fname(p))
    pt.check("walking opened nothing", not [a for a in m.log()[before:] if a["action"].startswith("opened")], m.log()[before:])
    pt.check("the focused row is not selected by walking",
             m.node(p, rows(m.tree(p), "Chats")[1]["path"]).get("AXSelected") in (False, 0, None),
             m.node(p, rows(m.tree(p), "Chats")[1]["path"]).get("AXSelected"))
    m.keys("space", settle=0.8)
    pt.check("Space opens the focused row", m.logged("opened chat", chat="chat-gate"), m.log()[-2:])
    pt.check("the window's title follows", m.title(p) == "Castle gate · mcbench-lite", m.title(p))
    pt.check("the opened row says open", m.fname(p) == ("AXGroup", "Castle gate, open, Working"), m.fname(p))
    m.keys("down")
    m.keys("return", settle=1.0)
    pt.check("Enter opens the focused row", m.logged("opened chat", chat="chat-farm"), m.log()[-2:])
    pt.check("the open chat is marked seen at once",
             m.wait_for(lambda: "Wheat farm, open, Resolution: Very high (5×)" in row_names(p, "Chats"), 5),
             row_names(p, "Chats"))
    pt.check("focus stays on the row after opening", m.fname(p)[1] is not None and m.fname(p)[1].startswith("Wheat farm"), m.fname(p))
    pt.shot("build-list")
    # Search: typing narrows by rank with the snippet in the name; Escape clears.
    s = m.one(m.tree(p), role="AXTextField", title="Search chats")
    pt.note("the search field", m.summary(m.node(p, s["path"]), keys=("AXRole", "AXTitle", "AXPlaceholderValue", "AXValue", "AXHelp")))
    pt.check("the search field's placeholder", m.node(p, s["path"]).get("AXPlaceholderValue") == "Search",
             m.node(p, s["path"]).get("AXPlaceholderValue"))
    m.click(s)
    m.typ("portcullis", settle=1.0)
    found = row_names(p, "Chats")
    pt.check("the search narrows the list with the snippet in the name",
             len(found) == 1 and found[0].startswith("Castle gate, Working, ") and "portcullis" in found[0], found)
    pt.check("the field holds the text", m.node(p, s["path"]).get("AXValue") == "portcullis", m.node(p, s["path"]).get("AXValue"))
    pt.shot("search")
    m.keys("escape", settle=0.8)
    pt.check("Escape clears the search", len(row_names(p, "Chats")) == 3, row_names(p, "Chats"))
    pt.check("Escape leaves the field", m.fname(p) != ("AXTextField", "Search chats"), m.fname(p))
    m.typ("nothing-like-this", settle=1.0)
    pt.note("typing after Escape goes to", [m.fname(p), m.node(p, "find:AXTextArea:Message").get("AXValue")])
    # Archive the open chat by AXPress: the next row below opens, else the nearest above.
    t = m.tree(p)
    farm = [r for r in rows(t, "Chats") if m.name(r).startswith("Wheat farm")][0]
    btn = [n for n in t if n["path"].startswith(farm["path"] + ".") and m.name(n) == "Archive build"][0]
    m.press(p, btn, settle=1.0)
    pt.check("Archive build archives the row", m.logged("archive", chat="chat-farm"), m.log()[-3:])
    pt.check("the open chat moves to the nearest row above", m.title(p) == "Castle gate · mcbench-lite", m.title(p))
    pt.check("the list lost the row", not any(n.startswith("Wheat farm") for n in row_names(p, "Chats")), row_names(p, "Chats"))


@m.part
def p_archive(pt):
    """The archived section: toggle (Enter, Space, AXPress), rows with time
    left, Restore, and the two-press Delete permanently with its armed name."""
    p = m.launch(GATE)
    t = m.tree(p)
    tog = m.one(t, role="AXButton", title="Show archived")
    n = m.node(p, tog["path"])
    pt.check("Show archived is a button, collapsed", n.get("AXExpanded") in (False, 0), m.summary(n))
    m.setfocus(p, tog)
    m.keys("return", settle=1.0)
    t = m.tree(p)
    tog = m.one(t, role="AXButton", title="Hide archived")
    pt.check("Enter opens it and the toggle says Hide archived, expanded",
             tog is not None and m.node(p, tog["path"]).get("AXExpanded") in (True, 1), m.summary(tog and m.node(p, tog["path"])))
    lst = m.one(t, role="AXList", title="Archived")
    pt.check("the archived rows are an AXList named Archived", lst is not None, m.summary(lst))
    names = row_names(p, "Archived")
    # Archived two days before the fixture's today: five of seven days left.
    pt.check("a row says its time left", "Old windmill, 5 days left" in names, names)
    pt.shot("archived")
    m.keys("space", settle=0.8)
    pt.check("Space closes it", m.one(m.tree(p), role="AXButton", title="Show archived") is not None)
    tog = m.one(m.tree(p), role="AXButton", title="Show archived")
    m.press(p, tog, settle=0.8)
    pt.check("AXPress opens it", m.one(m.tree(p), role="AXList", title="Archived") is not None)
    t = m.tree(p)
    row = m.one(t, role="AXGroup", title="Old windmill, 5 days left")
    kids = [x for x in t if x["path"].startswith(row["path"] + ".")]
    pt.note("the archived row and its buttons", [m.summary(m.node(p, x["path"])) for x in [row] + kids])
    delete = [x for x in kids if m.name(x) == "Delete permanently"][0]
    pt.check("Delete permanently says it cannot be undone",
             m.node(p, delete["path"]).get("AXHelp") == "Delete permanently: this cannot be undone", m.node(p, delete["path"]).get("AXHelp"))
    restore = [x for x in kids if m.name(x) == "Restore"][0]
    pt.check("Restore says what it does", m.node(p, restore["path"]).get("AXHelp") == "Restore to the list and open it",
             m.node(p, restore["path"]).get("AXHelp"))
    # The two presses: Tab to Delete permanently, Space arms it.
    m.setfocus(p, delete)
    sh(f"{G} observe {p} 8 > /tmp/obs-arm.jsonl 2>&1 &")
    time.sleep(0.8)
    m.keys("space", settle=0.6)
    t = m.tree(p)
    armed = m.one(t, role="AXButton", title="Delete for good")
    pt.check("the first press arms it: the button is named Delete for good", armed is not None,
             [m.name(x) for x in t if x["path"].startswith(row["path"])])
    pt.check("armed, its help says to click again",
             armed is not None and m.node(p, armed["path"]).get("AXHelp") == "Click again to delete it for good",
             armed and m.node(p, armed["path"]).get("AXHelp"))
    pt.check("the row's name says Delete for good?", any(m.name(x) == "Delete for good?" for x in t), [m.name(x) for x in t if (m.name(x) or "").startswith("Delete")])
    pt.check("focus stays on the button", m.fname(p) == ("AXButton", "Delete for good"), m.fname(p))
    pt.shot("armed")
    time.sleep(4.5)
    t = m.tree(p)
    pt.check("after 4 s it is Delete permanently again", m.one(t, role="AXButton", title="Delete permanently") is not None,
             [m.name(x) for x in t if (m.name(x) or "").startswith("Delete")])
    obs = jl(sh("sleep 3; cat /tmp/obs-arm.jsonl"))
    pt.note("notifications while arming", [{k: o.get(k) for k in ("note", "role", "label", "info")} for o in obs if o.get("note") not in (None, "AXValueChanged")][:20])
    ann = [(o.get("info") or {}).get("AXAnnouncementKey") for o in obs if o.get("note") == "AXAnnouncementRequested"]
    pt.check("arming announces Delete for good? once, and disarming nothing", ann == ["Delete for good?"], ann)
    m.keys("space", settle=0.4)
    m.keys("space", settle=1.0)
    pt.check("two presses within 4 s delete it", m.logged("delete") or m.logged("deleted"), m.log()[-3:])
    pt.check("the row is gone", not any((m.name(x) or "").startswith("Old windmill") for x in m.tree(p)))
    # Restore the archived bench in Bench? The Build section is now empty: the toggle hides.
    pt.check("with nothing archived the toggle is gone", m.one(m.tree(p), prefix="Show archived") is None and m.one(m.tree(p), prefix="Hide archived") is None,
             [m.name(x) for x in m.tree(p) if "archived" in (m.name(x) or "")])
    # Restore: archive Wheat farm, then restore it with Space.
    t = m.tree(p)
    farm = [r for r in rows(t, "Chats") if m.name(r).startswith("Wheat farm")][0]
    m.press(p, [n for n in t if n["path"].startswith(farm["path"] + ".") and m.name(n) == "Archive build"][0], settle=1.0)
    t = m.tree(p)
    tog = m.one(t, role="AXButton", title="Show archived") or m.one(t, role="AXButton", title="Hide archived")
    pt.check("archiving a chat brings the section back", tog is not None, [m.name(x) for x in t if "rchiv" in (m.name(x) or "")])
    if m.name(tog) == "Show archived":
        m.press(p, tog, settle=0.8)
    t = m.tree(p)
    row = m.one(t, prefix="Wheat farm, 7 days left")
    pt.check("the archived chat says 7 days left", row is not None, row_names(p, "Archived"))
    restore = [x for x in t if x["path"].startswith(row["path"] + ".") and m.name(x) == "Restore"][0]
    m.setfocus(p, restore)
    m.keys("space", settle=1.5)
    pt.check("Space on Restore restores it and opens it", m.title(p) == "Wheat farm · mcbench-lite", m.title(p))
    pt.check("the row is back in the list", any(n.startswith("Wheat farm") for n in row_names(p, "Chats")), row_names(p, "Chats"))
    pt.note("focus after Restore", m.fname(p))


@m.part
def p_filters(pt):
    """The filter button and menu: focus on its first value, Up wraps,
    Escape gives focus back, Space chooses and keeps the menu, chips."""
    p = m.launch(GATE)
    t = m.tree(p)
    f = m.one(t, role="AXButton", title="Filter")
    pt.check("Filter is a button, collapsed", m.node(p, f["path"]).get("AXExpanded") in (False, 0), m.summary(m.node(p, f["path"])))
    m.setfocus(p, f)
    m.keys("return", settle=0.8)
    t = m.tree(p)
    menu = m.one(t, role="AXMenu") or m.one(t, title="Filter", pred=lambda n: n["AXRole"] != "AXButton")
    pt.check("the menu is there, named Filter", menu is not None and m.name(menu) == "Filter", m.summary(menu))
    pt.note("the menu's groups and items", [m.summary(x, keys=("path", "AXRole", "AXSubrole", "AXTitle", "AXDescription", "AXHelp", "AXValue"))
                                           for x in t if menu and x["path"].startswith(menu["path"])][:30])
    pt.check("the trigger is expanded", m.node(p, f["path"]).get("AXExpanded") in (True, 1), m.node(p, f["path"]).get("AXExpanded"))
    pt.check("focus on the menu's first value", m.fname(p)[1] == "Standard", m.fname(p))
    m.keys("up")
    pt.check("Up from the first wraps to the last", m.fname(p)[1] == "claude-opus-5-5", m.fname(p))
    m.keys("down")
    pt.check("Down wraps back", m.fname(p)[1] == "Standard", m.fname(p))
    pt.shot("menu")
    m.keys("escape", settle=0.6)
    pt.check("Escape closes it and focus goes back to the button", m.fname(p) == ("AXButton", "Filter"), m.fname(p))
    pt.check("the menu is gone", m.one(m.tree(p), role="AXMenu") is None)
    m.keys("return", settle=0.8)
    m.keys("down", "down")
    pt.check("Down to Very high (5×)", m.fname(p)[1] == "Very high (5×)", m.fname(p))
    m.keys("space", settle=0.6)
    fo = m.focused(p)
    pt.note("a chosen value", m.summary(fo, keys=("AXRole", "AXSubrole", "AXTitle", "AXValue", "AXMenuItemMarkChar", "AXSelected", "AXHelp")))
    pt.check("Space chooses it and the menu stays", m.one(m.tree(p), role="AXMenu") is not None and m.fname(p)[1] == "Very high (5×)", m.fname(p))
    m.keys("escape", settle=0.6)
    pt.check("the button says how many are chosen", m.fname(p) == ("AXButton", "Filter, 1 chosen"), m.fname(p))
    pt.check("the list is narrowed", row_names(p, "Chats") == ["Wheat farm, Finished, not opened yet, Resolution: Very high (5×)"], row_names(p, "Chats"))
    t = m.tree(p)
    chips = m.one(t, title="Filters")
    pt.check("the chips are a group named Filters", chips is not None, m.summary(chips))
    chip = m.one(t, role="AXButton", title="Remove filter Size: Very high (5×)")
    clear = m.one(t, role="AXButton", title="Clear all")
    pt.check("a chip is a button named Remove filter <full name>", chip is not None, [m.name(x) for x in t if x["path"].startswith(chips["path"])] if chips else None)
    pt.check("and Clear all", clear is not None)
    pt.shot("chips")
    m.keys("tab")
    pt.check("Tab from Filter reaches the chip", m.fname(p) == ("AXButton", "Remove filter Size: Very high (5×)"), m.fname(p))
    m.keys("return", settle=0.8)
    pt.check("removing the last chip gives focus to the filter button", m.fname(p) == ("AXButton", "Filter"), m.fname(p))
    pt.check("no filter: every row", len(row_names(p, "Chats")) == 3, row_names(p, "Chats"))
    # A press outside closes the menu.
    m.keys("return", settle=0.8)
    m.click(m.one(m.tree(p), role="AXGroup", sub="AXLandmarkRegion"), settle=0.6)
    pt.check("a press outside closes the menu", m.one(m.tree(p), role="AXMenu") is None)
    # Tab out closes it.
    f = m.one(m.tree(p), role="AXButton", title="Filter")
    m.setfocus(p, f)
    m.keys("return", settle=0.8)
    m.keys("down")
    m.keys("tab", settle=1.5)
    pt.shot("tab-out")
    pt.check("Tab out closes the menu", m.one(m.tree(p), role="AXMenu") is None, m.fname(p))
    pt.check("Tab out goes on to the stop after the Filter button (the current row)", m.fname(p) == ("AXGroup", "Redstone lamp with a lever, open"), m.fname(p))
    m.keys("tab:shift")
    pt.check("Shift-Tab back to Filter", m.fname(p) == ("AXButton", "Filter"), m.fname(p))
    m.keys("return", settle=0.8)
    m.keys("tab:shift", settle=1.0)
    pt.check("Shift-Tab out of the menu goes to the stop before the button", m.fname(p) == ("AXTextField", "Search chats"), m.fname(p))
    pt.check("and closes the menu", m.one(m.tree(p), role="AXMenu") is None)


@m.part
def p_palette(pt):
    """The palette: Cmd-K opens a dialog with its field focused; the results
    list box; typing; the arrows move a highlight said in the polite region;
    Enter runs it; Escape and Cmd-K close it, focus back to the composer."""
    p = m.launch(GATE)
    sh(f"{G} observe {p} 30 > /tmp/obs-pal.jsonl 2>&1 &")
    time.sleep(0.8)
    m.keys("k:cmd", settle=1.0)
    t = m.tree(p)
    dlg = m.one(t, title="Search everything", pred=lambda n: n["AXRole"] != "AXTextField")
    pt.check("Cmd-K opens a dialog named Search everything", dlg is not None, m.summary(dlg))
    pt.note("the dialog's attributes", m.node(p, dlg["path"]) if dlg else None)
    pt.check("its field has focus", m.fname(p) == ("AXTextField", "Search everything"), m.fname(p))
    res = m.one(t, title="Results")
    pt.check("the results are a list named Results", res is not None, m.summary(res))
    inside = [x for x in t if res and x["path"].startswith(res["path"] + ".")]
    pt.note("the results' groups and items", [m.summary(x, keys=("path", "AXRole", "AXSubrole", "AXTitle", "AXSelected")) for x in inside][:40])
    sel = [x for x in inside if m.node(p, x["path"]).get("AXSelected")]
    pt.check("one item is selected (the highlight)", len(sel) == 1, [m.name(x) for x in sel])
    pt.shot("palette-open")
    m.keys("down", settle=0.5)
    m.keys("down", settle=0.5)
    sel2 = [m.name(x) for x in m.tree(p) if res and x["path"].startswith(res["path"] + ".") and m.node(p, x["path"]).get("AXSelected")]
    pt.check("Down moves the highlight; focus stays in the field", m.fname(p) == ("AXTextField", "Search everything") and sel2 and sel2 != [m.name(x) for x in sel],
             [sel2, m.fname(p)])
    m.typ("gate", settle=1.2)
    t = m.tree(p)
    res = m.one(t, title="Results")
    opts = [m.name(x) for x in t if res and x["path"].startswith(res["path"] + ".") and x["AXRole"] != "AXGroup" or (
        res and x["path"].startswith(res["path"] + ".") and x.get("AXTitle") and x["path"].count(".") > res["path"].count(".") + 1)]
    pt.check("the core's builds are listed with their detail", any((o or "").startswith("Castle gate") for o in opts), opts[:8])
    pt.shot("palette-gate")
    m.keys("return", settle=1.2)
    pt.check("Enter runs the highlighted item", m.title(p) == "Castle gate · mcbench-lite", m.title(p))
    pt.check("and closes the palette", m.one(m.tree(p), title="Search everything") is None)
    m.keys("k:cmd", settle=0.8)
    m.typ("zzzqqq", settle=1.2)
    t = m.tree(p)
    pt.check("nothing found says so", any((m.name(x) or "").startswith("Nothing matches") or str(x.get("AXValue", "")).startswith("Nothing matches") for x in t),
             [m.name(x) or x.get("AXValue") for x in t if "atch" in str(m.name(x) or x.get("AXValue") or "")])
    m.keys("escape", settle=0.8)
    pt.check("Escape closes it, focus to the composer", m.one(m.tree(p), title="Search everything") is None and m.fname(p) == ("AXTextArea", "Message"), m.fname(p))
    m.keys("k:cmd", settle=0.8)
    m.keys("k:cmd", settle=0.8)
    pt.check("Cmd-K again closes it", m.one(m.tree(p), title="Search everything") is None, m.fname(p))
    # A press on the scrim closes it.
    m.keys("k:cmd", settle=0.8)
    gx(G, "click", "200", "600")
    time.sleep(0.8)
    pt.check("a press on the scrim closes it", m.one(m.tree(p), title="Search everything") is None, m.fname(p))
    time.sleep(1)
    obs = jl(sh("cat /tmp/obs-pal.jsonl"))
    pt.note("announcements and focus changes while the palette was used",
            [{k: o.get(k) for k in ("note", "role", "label", "info")} for o in obs if o.get("note") in ("AXAnnouncementRequested", "AXFocusedUIElementChanged", "AXTitleChanged")][:40])


@m.part
def p_bench(pt):
    """Cmd-2 to Bench: the bench list, its fold, its models, walking and
    opening; the mode switcher's name and description; Cmd-1 back."""
    p = m.launch(GATE)
    t = m.tree(p)
    sw = m.one(t, role="AXButton", title="mcbuild")
    pt.check("the mode switcher's name and help", sw and m.node(p, sw["path"]).get("AXHelp") == "Switch to Bench (⌘2)", m.summary(m.node(p, sw["path"])) if sw else None)
    m.keys("two:cmd", settle=1.0)
    t = m.tree(p)
    pt.check("Cmd-2 switches to Bench (the switcher says mcbench)", m.one(t, role="AXButton", title="mcbench") is not None, [m.name(x) for x in t[:6]])
    lst = m.one(t, role="AXList", title="Benches")
    pt.check("the bench list is an AXList named Benches", lst is not None, m.summary(lst))
    pt.check("its benches", row_names(p, "Benches") == ["Towers and a bridge"], row_names(p, "Benches"))
    pt.note("Bench's sidebar", [m.summary(x, keys=("path", "AXRole", "AXSubrole", "AXTitle", "AXHelp")) for x in t if x["path"].startswith("0.0.") and x["path"].count(".") <= 4][:40])
    bench = rows(t, "Benches")[0]
    m.setfocus(p, bench)
    m.keys("tab")
    pt.check("Tab from the bench reaches Show models", m.fname(p) == ("AXButton", "Show models"), m.fname(p))
    fold = m.focused(p)
    pt.check("Show models is collapsed", fold.get("AXExpanded") in (False, 0), m.summary(fold))
    m.keys("return", settle=0.8)
    fold = m.focused(p)
    pt.check("Enter opens the fold (Hide models, expanded)", m.name(fold) == "Hide models" and fold.get("AXExpanded") in (True, 1), m.summary(fold))
    models = row_names(p, "Towers and a bridge models")
    pt.check("the bench's models by their letters", models == ["Build A, Stopped or failed", "Build B"], models)
    pt.check("opening the fold opened no bench", not m.logged("opened", place="/bench/bench-towers"), m.log()[-3:])
    m.setfocus(p, rows(m.tree(p), "Benches")[0])
    before = len(m.log())
    m.keys("down")
    pt.check("Down walks into the open bench's models", m.fname(p)[1] == "Build A, Stopped or failed", m.fname(p))
    m.keys("down")
    pt.check("Down to Build B", m.fname(p)[1] == "Build B", m.fname(p))
    m.keys("up", "up")
    pt.check("Up back to the bench", m.fname(p)[1] == "Towers and a bridge", m.fname(p))
    pt.check("walking opened nothing", not [a for a in m.log()[before:] if a["action"].startswith("opened")], m.log()[before:])
    m.keys("down")
    m.keys("return", settle=1.0)
    pt.check("Enter opens a model's chat", m.logged("opened", place="/bench/bench-towers/member-b"), [a for a in m.log() if a["action"] == "opened"][-2:])
    pt.shot("bench")
    t = m.tree(p)
    lb = m.one(t, role="AXButton", title="Leaderboard")
    pt.check("the Leaderboard row is a button", lb is not None, m.summary(lb))
    pt.check("the prompts' titles below", row_names(p, "Prompts") == ["Wheat field", "Two towers"], row_names(p, "Prompts"))
    pf = m.one(t, role="AXButton", title="Prompts")
    pt.check("the prompts' fold is a button with its expanded state", pf is not None and m.node(p, pf["path"]).get("AXExpanded") is not None,
             m.summary(pf and m.node(p, pf["path"])))
    m.keys("one:cmd", settle=1.0)
    pt.check("Cmd-1 back to Build", m.one(m.tree(p), role="AXButton", title="mcbuild") is not None)



def named(t, title, roles=None):
    """The first element named `title` (title or description), of one of `roles`."""
    for n in t:
        if m.name(n) == title and (roles is None or n.get("AXRole") in roles):
            return n
    return None


def has(p, title, roles=None):
    return named(m.tree(p), title, roles) is not None


def label_text(n):
    return m.name(n) or (n.get("AXValue") if isinstance(n.get("AXValue"), str) else None)


def articles(p):
    return [m.name(n) for n in m.tree(p) if n.get("AXSubrole") == "AXDocumentArticle"]


def wait_turn(p, timeout=40):
    m.wait_for(lambda: has(p, "Stop generating", ["AXButton"]), 10)
    return m.wait_for(lambda: not has(p, "Stop generating", ["AXButton"]), timeout)


def focus_composer(p):
    # The right end: VoiceOver's caption panel covers the left part.
    c = m.composer(p)
    m.click(c, dx=c["AXSize"][0] - 40, dy=20, settle=0.4)


CHAT = f"{GATE} --turn full --rate-ms 15 --running chat-gate --join chat-gate --refuse chat-farm --select"


@m.part
def p_chat(pt):
    """A full streamed turn (working line, busy, the fold, plan card, notice,
    tool rows, the shot), the lightbox, Stop, edit and branch with the
    arrows, the selection chip, drafts per chat, joining, a refused send."""
    p = m.launch(CHAT)
    sh(f"{G} observe {p} 40 > /tmp/obs-turn.jsonl 2>&1 &")
    time.sleep(0.8)
    focus_composer(p)
    m.typ("Make it brighter", settle=0.3)
    m.keys("return", settle=0.6)
    region = m.one(m.tree(p), sub="AXLandmarkRegion")
    pt.note("the transcript while the turn runs", m.node(p, region["path"]))
    working = m.wait_for(lambda: [m.name(n) for n in m.tree(p) if (m.name(n) or "").startswith("Working for ")], 10)
    pt.check("the working line shows while the turn runs", bool(working), working)
    wl = [n for n in m.tree(p) if (m.name(n) or "").startswith("Working for ")]
    pt.note("the working line's node", m.node(p, wl[0]["path"]) if wl else None)
    pt.shot("streaming")
    wait_turn(p)
    time.sleep(1.0)
    rows = articles(p)
    pt.note("the articles after the full turn", rows)
    pt.check("the fold says its steps", has(p, "Worked · 4 steps"), [r for r in rows if r and "Worked" in r])
    pt.check("the plan card reads Tasks, 3 of 3", any("Tasks, 3 of 3" in (m.name(n) or "") for n in m.tree(p)))
    pt.check("the warning notice reads Warning: …", any((m.name(n) or "").startswith("Warning: The model fell back") or "Warning: The model fell back" in (m.name(n) or "") for n in m.tree(p)))
    obs = m.jl(sh("sleep 1; cat /tmp/obs-turn.jsonl"))
    titles = [o.get("label") for o in obs if o.get("note") == "AXTitleChanged" and (o.get("label") or "").startswith("Working for")]
    ann = [o.get("info") for o in obs if o.get("note") == "AXAnnouncementRequested"]
    pt.note("the working line's title changes (once a second, not announced)", titles)
    pt.check("announcements during the turn: only the answer at its end", len(ann) == 1, ann)
    fold = named(m.tree(p), "Worked · 4 steps", ["AXButton"])
    m.press(p, fold)
    # The fold's own focus keeps its message laid out (a virtual list).
    m.setfocus(p, named(m.tree(p), "Worked · 4 steps", ["AXButton"]))
    m.wait_for(lambda: has(p, "Built: a stone base and a lamp"), 10)
    for row in ["Built: a stone base and a lamp", "Minecraft test completed: 2 tests: input and block-state observations",
                "Inspected build: (iso_se)", "Command failed: cat /work/notes.txt"]:
        n = named(m.tree(p), row)
        pt.check(f"the tool row {row!r}", n is not None, m.summary(n and m.node(p, n["path"])))
    shot = named(m.tree(p), "Picture 1")
    pt.check("the shot is a picture named Picture 1", shot is not None, m.summary(shot))
    pt.shot("full-turn")
    # The lightbox: a dialog named Picture with Close focused; Escape closes it, focus back.
    if shot:
        m.setfocus(p, shot)
        before = m.fname(p)
        m.keys("return", settle=0.8)
        dlg = named(m.tree(p), "Picture", ["AXGroup", "AXWindow", "AXSheet"])
        pt.check("the lightbox is a dialog named Picture", dlg is not None, m.summary(dlg and m.node(p, dlg["path"])))
        pt.check("it takes focus on Close", m.fname(p) == ("AXButton", "Close"), m.fname(p))
        pt.shot("lightbox")
        m.keys("escape", settle=0.6)
        pt.check("Escape closes it", named(m.tree(p), "Picture", ["AXGroup"]) is None or not any(
            n.get("AXSubrole") not in (None, "AXUnknown") and m.name(n) == "Picture" for n in m.tree(p)))
        pt.check("focus goes back where it was", m.fname(p) == before, [before, m.fname(p)])
    # Stop.
    focus_composer(p)
    m.typ("Again, slower", settle=0.3)
    m.keys("return")
    m.wait_for(lambda: has(p, "Stop generating", ["AXButton"]), 10)
    stop = named(m.tree(p), "Stop generating", ["AXButton"])
    pt.check("Stop generating is a button while the turn streams", stop is not None, m.summary(stop))
    m.press(p, stop, settle=1.5)
    pt.check("Stop pressed (logged)", m.logged("stop"))
    m.wait_for(lambda: not has(p, "Stop generating", ["AXButton"]), 20)
    pt.check("a stopped turn says so", m.wait_for(lambda: (articles(p)[-1] or "") == "claude-opus-5-5, (stopped by user)", 10), articles(p)[-1:])
    # Edit and branch, then the arrows.
    eb = m.wait_for(lambda: [n for n in m.tree(p) if m.name(n) == "Edit and branch"], 10)[-1]
    m.press(p, eb)
    pt.check("the editing line", m.wait_for(lambda: has(p, "Editing a message: sending it starts a new branch"), 8))
    pt.check("the composer holds the message", m.composer(p).get("AXValue") == "Again, slower", m.composer(p).get("AXValue"))
    cancel = named(m.tree(p), "Cancel edit", ["AXButton"])
    pt.check("Cancel edit is a button with its tip", cancel is not None and m.node(p, cancel["path"]).get("AXHelp") == "Cancel edit (Esc)",
             m.summary(cancel and m.node(p, cancel["path"])))
    gx(G, "key", str(m.K["end"]))
    m.typ(" please", settle=0.3)
    m.keys("return")
    wait_turn(p)
    time.sleep(0.8)
    pt.check("the edit is the message now", "You, Again, slower please" in articles(p), articles(p)[-3:])
    # The edited message is above the turn's answer: focus it (its row
    # scrolls into view) so its parts are laid out.
    edited = [n for n in m.tree(p) if n.get("AXSubrole") == "AXDocumentArticle" and m.name(n) == "You, Again, slower please"]
    if edited:
        m.setfocus(p, edited[-1], settle=1.0)
    grp = m.wait_for(lambda: named(m.tree(p), "Conversation branch, 2 of 2"), 10)
    pt.check("the branch counter is a group named in words", grp is not None, m.summary(grp))
    prev = named(m.tree(p), "Previous branch", ["AXButton"])
    nxt = named(m.tree(p), "Next branch", ["AXButton"])
    pt.check("Next branch is disabled on the last version", nxt is not None and m.node(p, nxt["path"]).get("AXEnabled") is False,
             m.summary(nxt and m.node(p, nxt["path"])))
    m.press(p, prev, settle=1.0)
    pt.check("Previous branch shows the first version", m.wait_for(lambda: has(p, "Conversation branch, 1 of 2") and "You, Again, slower" in articles(p), 8), articles(p)[-4:])
    pt.shot("branch")
    # The new chat's selection chip.
    m.press(p, named(m.tree(p), "New chat", ["AXButton"]))
    chip = [m.name(n) for n in m.tree(p) if (m.name(n) or "").startswith("Redstone Lamp, unlit, 1×2×1")]
    pt.check("the selection chip, named by label and size", bool(chip), chip)
    n = [n for n in m.tree(p) if (m.name(n) or "").startswith("Redstone Lamp, unlit, 1×2×1")]
    pt.check("its description says the blocks", n and m.node(p, n[0]["path"]).get("AXHelp") == "2 blocks", n and m.node(p, n[0]["path"]).get("AXHelp"))
    m.press(p, named(m.tree(p), "Remove Redstone Lamp, unlit", ["AXButton"]))
    pt.check("Remove takes the chip away", not any((m.name(n) or "").startswith("Redstone Lamp, unlit") for n in m.tree(p)))
    pt.check("the empty chat's heading", has(p, "What should we build?"), [m.name(n) for n in m.tree(p) if n.get("AXRole") == "AXHeading"])
    # Drafts per chat; typing that starts on a chat row goes to the composer.
    m.press(p, chat_row(p, "Redstone lamp with a lever"), settle=0.8)
    focus_composer(p)
    m.typ("a second lamp", settle=0.3)
    m.press(p, chat_row(p, "Castle gate"), settle=1.0)
    pt.check("a running turn is joined when its chat opens", m.wait_for(lambda: m.logged("joined", chat="chat-gate"), 10))
    wait_turn(p)
    pt.check("the other chat's composer is empty", m.composer(p).get("AXValue") == "", m.composer(p).get("AXValue"))
    m.setfocus(p, chat_row(p, "Castle gate"))
    m.typ("a taller gate", settle=0.5)
    pt.check("typing on a chat row goes to the composer", m.composer(p).get("AXValue") == "a taller gate", m.composer(p).get("AXValue"))
    m.press(p, chat_row(p, "Redstone lamp with a lever"), settle=1.0)
    pt.check("the first chat's draft is back", m.composer(p).get("AXValue") == "a second lamp", m.composer(p).get("AXValue"))
    m.press(p, chat_row(p, "Castle gate"), settle=1.0)
    pt.check("the second chat's draft is back", m.composer(p).get("AXValue") == "a taller gate", m.composer(p).get("AXValue"))
    # A refused send puts the draft back and shows a notice.
    m.press(p, chat_row(p, "Wheat farm"), settle=1.0)
    focus_composer(p)
    m.typ("One more", settle=0.3)
    m.keys("return", settle=1.2)
    err = [m.name(n) for n in m.tree(p) if (m.name(n) or "").startswith("Error, ")]
    pt.check("a refused send is an Error article", any("A turn is already running for this conversation" in e for e in err), err)
    pt.check("the refused message is back in the composer", m.composer(p).get("AXValue") == "One more", m.composer(p).get("AXValue"))
    pt.shot("refused")


@m.part
def p_attach(pt):
    """Attachments: the macOS open panel from Attach file with real input
    (Cmd-Shift-G, a path, Return), the tiles' names and Reading state,
    Remove attachment; a picture pasted from the guest's pasteboard; the
    limit errors' texts (nine files; one over 100 MB)."""
    sh("rm -rf /tmp/pick; mkdir -p /tmp/pick/nine /tmp/pick/big", check=False)
    p = m.launch(GATE)
    sh(f"cp {SCRATCH_PIC} /tmp/pick/door.png; printf 'plans\\n' > /tmp/pick/notes.txt; "
       "for i in 1 2 3 4 5 6 7 8 9; do printf x > /tmp/pick/nine/f$i.txt; done; "
       "mkfile 101m /tmp/pick/big/big.bin", check=False)
    att = named(m.tree(p), "Attach file", ["AXButton"])
    pt.check("Attach file has its tooltip as help", m.node(p, att["path"]).get("AXHelp") is not None, m.node(p, att["path"]).get("AXHelp"))
    m.press(p, att, settle=2.0)
    pt.shot("open-panel")
    wins = [w for w in m.jl(gx(G, "windows")) if w.get("pid") == int(p)]
    pt.note("windows after Attach file", wins)
    gx(G, "key", "5", "cmd,shift")  # Cmd-Shift-G: go to folder
    time.sleep(1.0)
    m.typ("/tmp/pick/door.png", settle=0.5)
    m.keys("return", settle=1.0)
    m.keys("return", settle=2.0)
    tiles = [m.name(n) for n in m.tree(p) if (m.name(n) or "").startswith("door.png")]
    pt.check("the picked picture is a tile door.png, jpeg (redrawn)", "door.png, jpeg" in tiles, tiles)
    pt.check("focus is back in the window after the panel", m.fname(p)[0] is not None, m.fname(p))
    pt.shot("tile")
    rm = [n for n in m.tree(p) if m.name(n) == "Remove attachment"]
    pt.check("each read tile has Remove attachment", len(rm) == 1, len(rm))
    m.press(p, rm[0])
    pt.check("Remove attachment removes the tile", not any((m.name(n) or "").startswith("door.png") for n in m.tree(p)))
    # Paste a picture from the pasteboard.
    sh("osascript -e 'set the clipboard to (read (POSIX file \"/tmp/pick/door.png\") as «class PNGf»)'", check=False)
    focus_composer(p)
    sh(f"{G} observe {p} 6 > /tmp/obs-paste.jsonl 2>&1 &")
    time.sleep(0.5)
    m.keys("v:cmd", settle=0.1)
    reading = [m.name(n) for n in m.tree(p) if (m.name(n) or "").endswith("Reading…")]
    pt.note("right after Cmd-V (a Reading… status may already be gone)", reading)
    time.sleep(1.5)
    tiles = [m.name(n) for n in m.tree(p) if ", jpeg" in (m.name(n) or "") or ", png" in (m.name(n) or "")]
    pt.check("a pasted picture becomes a tile", bool(tiles), tiles)
    obs = m.jl(sh("sleep 5; cat /tmp/obs-paste.jsonl"))
    pt.note("titles seen while the paste was read", [o.get("label") for o in obs if o.get("note") in ("AXTitleChanged", "AXCreated", "AXAnnouncementRequested")])
    # Limits: nine files at once; a file over 100 MB.
    # Go to a file (Cmd-Shift-G, its path, Return): the panel selects it with
    # focus in its column; Cmd-A then selects the folder's files.
    for first, want in [("/tmp/pick/nine/f1.txt", "A message supports at most 8 attachments"), ("/tmp/pick/big/big.bin", '"big.bin" exceeds 100 MB')]:
        m.press(p, named(m.tree(p), "Attach file", ["AXButton"]), settle=2.0)
        gx(G, "key", "5", "cmd,shift")
        time.sleep(1.0)
        m.typ(first, settle=0.5)
        m.keys("return", settle=1.5)
        m.keys("a:cmd", settle=0.8)
        m.keys(*["down:shift"] * 8, settle=0.2)
        pt.shot("panel-selection")
        m.keys("return", settle=3.0)
        t = m.tree(p)
        texts = [label_text(n) for n in t if want in (label_text(n) or "")]
        pt.check(f"the limit's text: {want}", bool(texts), [label_text(n) for n in t if "exceed" in (label_text(n) or "") or "at most" in (label_text(n) or "")])
        pt.shot("limit")
        for n in [n for n in t if m.name(n) == "Dismiss error"]:
            m.press(p, n)



@m.part
def p_held(pt):
    """The held send (--setup-pending --setup-ms 12000): a send while setup
    runs is held with its line and Don't send; Don't send keeps the text;
    sent again it goes when the step finishes."""
    # The step runs 40 s: the launch waits for AX and the window first.
    p = m.launch(f"{GATE} --setup-pending --setup-ms 150000")
    held = "Sends when the app is ready. 1 of 2 done."
    focus_composer(p)
    m.typ("Hold this", settle=0.3)
    m.keys("return", settle=0.8)
    line = named(m.tree(p), held)
    pt.check("a send while setup runs is held, with its line", line is not None, m.summary(line and m.node(p, line["path"])))
    pt.check("and its Don't send button", has(p, "Don't send", ["AXButton"]))
    pt.check("nothing is sent meanwhile", not m.logged("sent"))
    pt.shot("held")
    m.press(p, named(m.tree(p), "Don't send", ["AXButton"]))
    pt.check("Don't send drops the hold and keeps the text", not has(p, held) and m.composer(p).get("AXValue") == "Hold this", m.composer(p).get("AXValue"))
    focus_composer(p)
    m.keys("return", settle=0.8)
    pt.check("sent again, it is held again", has(p, held))
    pt.check("it goes when the running step finishes", bool(m.wait_for(lambda: m.logged("sent"), 160)))
    pt.check("the line is gone", bool(m.wait_for(lambda: not has(p, held), 5)))

SCRATCH_PIC = f"{m.SCRATCH}/lamp.png"
PICKERS = "--gallery"


@m.part
def p_pickers(pt):
    """The pickers in the gallery (its providers include a failing and a
    slow one): the model picker, its rail, search, a pick, a star; effort;
    the resolution slider by keys and by AXIncrement and AXDecrement; the
    context meter and the limit chip."""
    p = m.launch(PICKERS)
    t = m.tree(p)
    MODEL = "claude-opus-5-5"
    for nm in [f"Chat model: {MODEL}", "Resolution: Standard", "Reasoning effort: medium"]:
        n = named(t, nm, ["AXButton", "AXPopUpButton", "AXMenuButton"])
        pt.check(f"{nm} is a button", n is not None, m.summary(n and m.node(p, n["path"])))
    meter = named(t, "84% of context · 840k of 1M tokens")
    pt.check("the context meter is named by its tip", meter is not None, m.summary(meter and m.node(p, meter["path"]), keys=(
        "AXRole", "AXSubrole", "AXRoleDescription", "AXTitle", "AXValue", "AXMinValue", "AXMaxValue", "AXHelp", "AXPosition", "AXSize")))
    chip = [n for n in t if label_text(n) == "Near weekly limit"]
    pt.check("the limit chip is named by its text, Resets at … as help", bool(chip) and str(m.node(p, chip[0]["path"]).get("AXHelp", "")).startswith("Resets at"),
             m.summary(chip and m.node(p, chip[0]["path"])))
    trig = named(t, f"Chat model: {MODEL}")
    pt.check("the trigger's help is the full id", m.node(p, trig["path"]).get("AXHelp") is not None, m.node(p, trig["path"]).get("AXHelp"))
    pt.check("the trigger is collapsed", m.node(p, trig["path"]).get("AXExpanded") in (False, 0), m.node(p, trig["path"]).get("AXExpanded"))
    m.setfocus(p, trig)
    m.keys("return", settle=1.0)
    t = m.tree(p)
    dlg = named(t, "Chat model", ["AXGroup", "AXSheet", "AXWindow"])
    pt.check("it opens a dialog named Chat model", dlg is not None, m.summary(dlg and m.node(p, dlg["path"])))
    pt.check("the trigger is expanded", m.node(p, trig["path"]).get("AXExpanded") in (True, 1))
    pt.check("focus is in the search", m.fname(p)[1] == "Search models", m.fname(p))
    rail = named(t, "Providers")
    tabs = [x for x in t if rail and x["path"].startswith(rail["path"] + ".") and x.get("AXRole") in ("AXRadioButton", "AXTab", "AXButton")]
    pt.note("the rail and its tabs", [m.summary(m.node(p, x["path"]), keys=("AXRole", "AXSubrole", "AXRoleDescription", "AXTitle", "AXValue", "AXEnabled", "AXSelected")) for x in ([rail] if rail else []) + tabs])
    openai = named(t, "OpenAI — not configured")
    pt.check("a provider not connected is disabled and says why", openai is not None and m.node(p, openai["path"]).get("AXEnabled") is False,
             m.summary(openai and m.node(p, openai["path"])))
    lst = named(t, "Chat model", ["AXList"])
    # Each option beside its star (a toggle button, a sibling in the list).
    opts = [m.name(x) for x in t if lst and x["path"].startswith(lst["path"] + ".") and x["path"].count(".") == lst["path"].count(".") + 1
            and x.get("AXSubrole") != "AXToggle"]
    pt.note("the model list's children", [m.summary(m.node(p, x["path"]), keys=("AXRole", "AXSubrole", "AXTitle", "AXHelp", "AXSelected"))
                                          for x in t if lst and x["path"].startswith(lst["path"] + ".") and x["path"].count(".") == lst["path"].count(".") + 1])
    pt.check("Claude Code's models are listed", opts[:2] == ["Claude Opus 5.5", "Claude Haiku 5.5"], opts)
    pt.shot("model-picker")
    m.typ("gemini", settle=1.0)
    found = m.wait_for(lambda: [m.name(x) for x in m.tree(p) if (m.name(x) or "").startswith("Gemini 3 Pro, ")], 6)
    pt.check("typing searches every provider", found and "Gemini 3 Pro, OpenRouter" in found, found)
    pt.check("the failing catalogue is named with its provider", any((label_text(x) or "").startswith("Groq: LLM request failed") for x in m.tree(p)))
    slow = m.wait_for(lambda: "Gemini 3 Pro, Google" in [m.name(x) for x in m.tree(p)], 4)
    pt.check("the slow catalogue comes once it answered", bool(slow))
    pt.shot("model-search")
    m.keys("escape", settle=0.6)
    pt.check("Escape closes it back to its trigger", m.fname(p) == ("AXButton", f"Chat model: {MODEL}") and named(m.tree(p), "Chat model", ["AXGroup"]) is None, m.fname(p))
    m.keys("return", settle=0.8)
    pt.check("the search kept its text", m.focused(p).get("AXValue") == "gemini", m.focused(p).get("AXValue"))
    m.keys("a:cmd", "delete", settle=0.6)
    m.keys("down")
    pt.check("Down goes to the first model", m.fname(p)[1] == "Claude Opus 5.5", m.fname(p))
    m.keys("down")
    m.keys("return", settle=0.8)
    pt.check("Enter chooses it and closes", has(p, "Chat model: claude-haiku-5-5", ["AXButton"]), m.fname(p))
    m.press(p, named(m.tree(p), "Chat model: claude-haiku-5-5", ["AXButton"]), settle=1.0)
    star = named(m.tree(p), "Favorite Claude Opus 5.5")
    s0 = m.node(p, star["path"]) if star else {}
    pt.note("a star", m.summary(s0, keys=("AXRole", "AXSubrole", "AXRoleDescription", "AXTitle", "AXValue", "AXHelp")))
    m.press(p, star, settle=0.6)
    s1 = m.node(p, named(m.tree(p), "Favorite Claude Opus 5.5")["path"])
    pt.check("a star's press toggles it (value or help changes)", (s0.get("AXValue"), s0.get("AXHelp")) != (s1.get("AXValue"), s1.get("AXHelp")),
             [s0.get("AXValue"), s0.get("AXHelp"), s1.get("AXValue"), s1.get("AXHelp")])
    m.press(p, named(m.tree(p), "Groq"), settle=0.6)
    pt.check("a failing tab shows its error", any("LLM request failed" in (label_text(x) or "") for x in m.tree(p)))
    pt.shot("model-error")
    m.keys("escape", settle=0.6)
    # Effort.
    m.press(p, named(m.tree(p), "Reasoning effort: medium", ["AXButton"]), settle=0.8)
    pt.check("the effort list opens on the current effort", m.fname(p)[1] == "medium", m.fname(p))
    lb = named(m.tree(p), "Reasoning effort", ["AXList", "AXGroup"])
    pt.note("the effort list", m.summary(lb and m.node(p, lb["path"])))
    m.keys("down")
    pt.check("Down moves to the next effort", m.fname(p)[1] == "high", m.fname(p))
    m.keys("return", settle=0.6)
    pt.check("Enter chooses it, focus back on the trigger", m.fname(p) == ("AXButton", "Reasoning effort: high"), m.fname(p))
    # Resolution: the slider by keys, then by AX increment and decrement.
    m.press(p, named(m.tree(p), "Resolution: Standard", ["AXButton"]), settle=0.8)
    sl = m.focused(p)
    pt.check("the slider has focus", sl.get("AXRole") == "AXSlider" and m.name(sl) == "Resolution",
             m.summary(sl, keys=("AXRole", "AXTitle", "AXValue", "AXMinValue", "AXMaxValue", "AXValueDescription", "AXHelp")))
    pt.note("the slider's attributes", m.node(p, "focused"))
    m.keys("right")
    pt.check("Right steps it to High (2×)", has(p, "Resolution: High (2×)", ["AXButton"]), m.node(p, "focused").get("AXValue"))
    r = gx(G, "act", p, "focused", "AXIncrement", check=False)
    time.sleep(0.6)
    pt.check("AXIncrement steps it on", has(p, "Resolution: Very high (5×)", ["AXButton"]), [r, m.node(p, "focused").get("AXValue")])
    gx(G, "act", p, "focused", "AXDecrement", check=False)
    time.sleep(0.6)
    pt.check("AXDecrement steps it back", has(p, "Resolution: High (2×)", ["AXButton"]), m.node(p, "focused").get("AXValue"))
    pt.shot("resolution")
    m.keys("escape", settle=0.6)
    pt.check("Escape closes it back to its trigger", m.fname(p) == ("AXButton", "Resolution: High (2×)"), m.fname(p))
    # Typing while a picker is open stays in the picker.
    m.press(p, named(m.tree(p), "Chat model: claude-haiku-5-5", ["AXButton"]), settle=1.0)
    m.typ("opus", settle=0.6)
    pt.check("typing while the model picker is open goes to its search", m.focused(p).get("AXValue") == "opus", m.focused(p).get("AXValue"))
    m.keys("escape")


@m.part
def p_gallery(pt):
    """The gallery's controls one by one: buttons, the switch, the copy
    button, the confirm button's 4 s, the popover menu, tooltips on hover
    after 500 ms only."""
    p = m.launch("--gallery")
    t = m.tree(p)
    for nm in ["Raised", "Primary", "Delete for good", "Run bench", "Share", "New chat", "Copy code", "Remove"]:
        n = named(t, nm, ["AXButton"])
        pt.check(f"{nm} is an enabled button", n is not None and m.node(p, n["path"]).get("AXEnabled") is True, m.summary(n))
    for nm in ["Raised, disabled", "Primary, disabled", "Run bench, disabled", "Settings, disabled"]:
        n = named(t, nm, ["AXButton"])
        pt.check(f"{nm} is not enabled", n is not None and m.node(p, n["path"]).get("AXEnabled") is False, m.summary(n))
    m.press(p, named(t, "Raised", ["AXButton"]))
    pt.check("a press reaches the button", [a.get("control") for a in m.log() if a["action"] == "pressed"] == ["Raised"])
    sw = named(m.tree(p), "Show the grid")
    s0 = m.node(p, sw["path"])
    pt.note("the switch", m.summary(s0, keys=("AXRole", "AXSubrole", "AXRoleDescription", "AXTitle", "AXValue")))
    m.press(p, sw)
    s1 = m.node(p, sw["path"])
    pt.check("the switch's press flips its value", s0.get("AXValue") != s1.get("AXValue"), [s0.get("AXValue"), s1.get("AXValue")])
    cc = named(m.tree(p), "Copy code", ["AXButton"])
    m.press(p, cc, settle=0.3)
    pt.check("Copy code keeps its name and says Copied as help", m.node(p, cc["path"]).get("AXTitle") == "Copy code" and m.node(p, cc["path"]).get("AXHelp") == "Copied",
             m.summary(m.node(p, cc["path"])))
    time.sleep(1.5)
    pt.check("Copied goes after 1.2 s", m.node(p, cc["path"]).get("AXHelp") != "Copied", m.node(p, cc["path"]).get("AXHelp"))
    rm = named(m.tree(p), "Remove", ["AXButton"])
    m.press(p, rm, settle=0.3)
    pt.check("the confirm button asks first", has(p, "Remove from the web?", ["AXButton"]))
    time.sleep(4.2)
    pt.check("after 4 s it is Remove again", has(p, "Remove", ["AXButton"]) and not has(p, "Remove from the web?", ["AXButton"]))
    trig = named(m.tree(p), "Effort: medium", ["AXButton"])
    m.press(p, trig, settle=0.8)
    menu = named(m.tree(p), "Effort", ["AXMenu"])
    pt.check("the trigger opens its menu (AXMenu Effort)", menu is not None, m.summary(menu))
    pt.check("focus is on the first item", m.fname(p) == ("AXMenuItem", "low"), m.fname(p))
    pt.note("the menu items", [m.summary(m.node(p, x["path"]), keys=("AXRole", "AXTitle", "AXValue", "AXMenuItemMarkChar")) for x in m.tree(p) if menu and x["path"].startswith(menu["path"] + ".")])
    m.keys("up")
    pt.check("Up from the first wraps to the last", m.fname(p)[1] == "high", m.fname(p))
    m.keys("escape", settle=0.8)
    pt.check("Escape closes it, focus back on its trigger", m.fname(p) == ("AXButton", "Effort: medium"), m.fname(p))
    # Tooltips: a hover shows one after 500 ms, not before.
    hosts = [n for n in m.tree(p) if m.node(p, n["path"]).get("AXHelp") not in (None, "<error -25212>") and n.get("AXRole") == "AXButton"]
    if hosts:
        h = hosts[0]
        gx(G, "move", str(h["AXPosition"][0] + h["AXSize"][0] / 2), str(h["AXPosition"][1] + h["AXSize"][1] / 2))
        time.sleep(0.25)
        tips = lambda: [m.summary(x) for x in m.tree(p) if x.get("AXSubrole") == "AXUserInterfaceTooltip"]
        early = tips()
        time.sleep(0.6)
        late = tips()
        pt.check("no tooltip 250 ms into a hover", not early, early)
        pt.check("a tooltip (AXGroup / AXUserInterfaceTooltip) after 850 ms, its text its host's help",
                 bool(late) and late[0].get("AXTitle") == m.node(p, h["path"]).get("AXHelp"), [late, m.node(p, h["path"]).get("AXHelp")])
        pt.shot("tooltip")


@m.part
def p_keys(pt):
    """Shortcuts and Escape's order: Cmd-1, Cmd-2, Cmd-K, Cmd-Shift-O,
    Cmd-B, Cmd-[ and Cmd-]; Escape closes the palette, then the lightbox,
    then Settings, then cancels an edit; typing anywhere goes to the
    composer except while a picker or the palette is open."""
    p = m.launch(GATE)
    m.keys("two:cmd", settle=1.0)
    pt.check("Cmd-2: Bench", has(p, "mcbench", ["AXButton"]))
    m.keys("one:cmd", settle=1.0)
    pt.check("Cmd-1: Build", has(p, "mcbuild", ["AXButton"]))
    m.keys("o:cmd,shift", settle=1.0)
    pt.check("Cmd-Shift-O: a new chat", m.logged("new chat") and m.title(p) == "New build · mcbench-lite", m.title(p))
    m.keys("b:cmd", settle=0.8)
    pt.check("Cmd-B hides the sidebar (Open sidebar in the pane header)", has(p, "Open sidebar", ["AXButton"]) and not has(p, "Chats", ["AXList"]))
    pt.shot("sidebar-hidden")
    m.keys("b:cmd", settle=0.8)
    pt.check("Cmd-B shows it again", has(p, "Chats", ["AXList"]))
    m.press(p, chat_row(p, "Castle gate"), settle=1.0)
    m.press(p, chat_row(p, "Wheat farm"), settle=1.0)
    trail = []
    for k in ["lbracket:cmd", "lbracket:cmd", "lbracket:cmd", "rbracket:cmd", "rbracket:cmd"]:
        m.keys(k, settle=0.8)
        trail.append(m.title(p))
    pt.check("Cmd-[ goes back and Cmd-] forward through the places opened",
             trail == ["Castle gate · mcbench-lite", "New build · mcbench-lite", "Redstone lamp with a lever · mcbench-lite",
                       "New build · mcbench-lite", "Castle gate · mcbench-lite"], trail)
    # Escape's order: palette over Settings over an edit.
    m.press(p, chat_row(p, "Redstone lamp with a lever"), settle=1.0)
    m.press(p, [n for n in m.tree(p) if m.name(n) == "Edit and branch"][-1], settle=0.6)
    m.press(p, named(m.tree(p), "Settings", ["AXButton"]), settle=0.8)
    m.keys("k:cmd", settle=0.8)
    state = lambda: [has(p, "Search everything", ["AXGroup"]) or bool([n for n in m.tree(p) if m.name(n) == "Search everything" and n["AXRole"] != "AXTextField"]),
                     bool([n for n in m.tree(p) if m.name(n) == "Settings" and n["AXRole"] != "AXButton"]),
                     has(p, "Editing a message: sending it starts a new branch")]
    s = [state()]
    for _ in range(3):
        m.keys("escape", settle=0.8)
        s.append(state())
    pt.check("Escape closes the palette, then Settings, then cancels the edit",
             s == [[True, True, True], [False, True, True], [False, False, True], [False, False, False]], s)
    # Typing anywhere goes to the composer.
    m.setfocus(p, named(m.tree(p), "Settings", ["AXButton"]))
    m.typ("hello", settle=0.6)
    pt.check("typing on a button goes to the composer", m.composer(p).get("AXValue", "").endswith("hello") and m.fname(p) == ("AXTextArea", "Message"),
             [m.composer(p).get("AXValue"), m.fname(p)])
    m.keys("k:cmd", settle=0.8)
    m.typ("gate", settle=0.6)
    pt.check("typing with the palette open goes to its field", m.focused(p).get("AXValue") == "gate", m.focused(p).get("AXValue"))
    m.keys("escape")


@m.part
def p_window(pt):
    """Pointer drags of the sidebar edge and the split, collapse under 140,
    double-click; full screen and minimise; quit and start again with the
    same preferences file, restoring the window's size, position and place."""
    prefs = "/tmp/prefs.json"
    sh(f"rm -f {prefs}", check=False)
    p = m.launch(f"{GATE} --preferences {prefs}")
    t = m.tree(p)
    lst = m.one(t, role="AXList", title="Chats")
    region = m.one(t, sub="AXLandmarkRegion")
    edge = lst["AXPosition"][0] + lst["AXSize"][0] + (region["AXPosition"][0] - lst["AXPosition"][0] - lst["AXSize"][0]) / 2
    y = 500
    w0 = lst["AXSize"][0]
    gx(G, "drag", str(edge), str(y), str(edge + 80), str(y))
    time.sleep(0.8)
    w1 = m.one(m.tree(p), role="AXList", title="Chats")["AXSize"][0]
    pt.check("dragging the sidebar's edge right widens it", w1 > w0 + 40, [w0, w1])
    lst = m.one(m.tree(p), role="AXList", title="Chats")
    edge = edge + 80
    gx(G, "drag", str(edge), str(y), str(edge + 600), str(y))
    time.sleep(0.8)
    wmax = m.one(m.tree(p), role="AXList", title="Chats")["AXSize"][0]
    pt.note("the widest the drag allows (list width)", wmax)
    pt.shot("sidebar-wide")
    r = m.one(m.tree(p), sub="AXLandmarkRegion")
    edge = m.one(m.tree(p), role="AXList", title="Chats")["AXPosition"][0] + wmax + 6
    gx(G, "drag", str(edge), str(y), str(m.one(m.tree(p), role="AXList", title="Chats")["AXPosition"][0] + 100), str(y))
    time.sleep(0.8)
    pt.check("dragging under 140 collapses it", not has(p, "Chats", ["AXList"]) and has(p, "Open sidebar", ["AXButton"]))
    pt.shot("collapsed")
    m.press(p, named(m.tree(p), "Open sidebar", ["AXButton"]))
    lst = m.one(m.tree(p), role="AXList", title="Chats")
    edge = lst["AXPosition"][0] + lst["AXSize"][0] + 6
    gx(G, "dclick", str(edge), str(y))
    time.sleep(0.8)
    pt.check("a double-click on the edge collapses it", not has(p, "Chats", ["AXList"]))
    m.press(p, named(m.tree(p), "Open sidebar", ["AXButton"]))
    # The split between the chat column and the world pane.
    r = m.one(m.tree(p), sub="AXLandmarkRegion")
    split = r["AXPosition"][0] + r["AXSize"][0] + 4
    c0 = r["AXSize"][0]
    gx(G, "drag", str(split), str(y), str(split + 150), str(y))
    time.sleep(0.8)
    c1 = m.one(m.tree(p), sub="AXLandmarkRegion")["AXSize"][0]
    pt.check("dragging the split widens the chat column", c1 > c0 + 60, [c0, c1])
    pt.shot("split")
    # Window: full screen, minimise (the window key again after the AX presses).
    gx(G, "activate", p)
    focus_composer(p)
    res = {}
    wins = lambda: [{k: w.get(k) for k in ("AXTitle", "minimized", "fullscreen", "AXPosition", "AXSize")} for w in m.jl(gx(G, "win", p, "list"))]
    m.keys("f:cmd,ctrl", settle=2.5); res["full screen"] = wins(); pt.shot("fullscreen")
    m.keys("f:cmd,ctrl", settle=2.5); res["out"] = wins()
    m.keys("m:cmd", settle=1.5); res["minimised"] = wins()
    gx(G, "win", p, "unminimize"); time.sleep(1.5); res["back"] = wins()
    pt.note("full screen and minimise", res)
    pt.check("Ctrl-Cmd-F enters and leaves full screen, Cmd-M minimises", bool(res["full screen"] and res["full screen"][0].get("fullscreen"))
             and not res["out"][0].get("fullscreen") and res["minimised"][0].get("minimized"), res)
    # Move and resize, open Castle gate, quit, start again.
    gx(G, "activate", p); time.sleep(0.4)
    gx(G, "win", p, "move", "120", "80"); gx(G, "win", p, "resize", "1100", "700"); time.sleep(1.0)
    m.press(p, chat_row(p, "Castle gate"), settle=1.0)
    before = {"window": wins(), "title": m.title(p), "sidebar": m.one(m.tree(p), role="AXList", title="Chats")["AXSize"],
              "column": m.one(m.tree(p), sub="AXLandmarkRegion")["AXSize"]}
    m.keys("q:cmd", settle=2.0)
    pt.check("Cmd-Q quits", not sh("pgrep -x mcbench-ui-dev || true").strip())
    saved = sh(f"cat {prefs} 2>/dev/null || echo none")
    pt.note("the preferences file after quitting", saved[:2000])
    sh(f"cd /tmp; {APP} --scratch {m.SCRATCH} --log /tmp/ui.log --fixture gate --preferences {prefs} > /tmp/ui.out 2>&1 &")
    time.sleep(5)
    p = m.gate.pid()
    m.wait_for(lambda: len(m.tree(p)) > 20, 30, 0.5)
    m.wait_for(lambda: m.one(m.tree(p), sub="AXLandmarkRegion"), 20)
    after = {"window": wins(), "title": m.title(p), "sidebar": (m.one(m.tree(p), role="AXList", title="Chats") or {}).get("AXSize"),
             "column": (m.one(m.tree(p), sub="AXLandmarkRegion") or {}).get("AXSize")}
    pt.note("before quitting and after starting again", {"before": before, "after": after})
    pt.check("the window's position and size come back", before["window"][0]["AXPosition"] == after["window"][0]["AXPosition"]
             and before["window"][0]["AXSize"] == after["window"][0]["AXSize"], [before["window"], after["window"]])
    pt.check("the open place comes back", after["title"] == "Castle gate · mcbench-lite", after["title"])
    pt.check("the sidebar's width comes back", before["sidebar"] == after["sidebar"], [before["sidebar"], after["sidebar"]])
    pt.check("the split comes back", before["column"] == after["column"], [before["column"], after["column"]])
    pt.shot("restored")


@m.part
def p_settings(pt):
    """Settings as a layer: a dialog named Settings with focus, Escape
    closes it and focus goes to the composer."""
    p = m.launch(GATE)
    b = named(m.tree(p), "Settings", ["AXButton"])
    m.setfocus(p, b)
    m.keys("return", settle=0.8)
    t = m.tree(p)
    d = [n for n in t if m.name(n) == "Settings" and n["AXRole"] != "AXButton"]
    pt.check("Settings opens as a dialog named Settings", bool(d), [m.summary(m.node(p, x["path"])) for x in d])
    pt.check("it has focus", m.fname(p)[1] == "Settings" and m.fname(p)[0] != "AXButton", m.fname(p))
    pt.check("no history entry (the title is the place's)", m.title(p) == "Redstone lamp with a lever · mcbench-lite", m.title(p))
    pt.shot("settings")
    m.keys("escape", settle=0.8)
    pt.check("Escape closes it", not [n for n in m.tree(p) if m.name(n) == "Settings" and n["AXRole"] != "AXButton"])
    pt.check("focus goes to the composer", m.fname(p) == ("AXTextArea", "Message"), m.fname(p))
    # A stored picture the core cannot give: Castle gate's shot (not running here).
    m.press(p, chat_row(p, "Castle gate"), settle=1.0)
    # The stored shot sits in the turn's work, under its fold.
    m.press(p, m.wait_for(lambda: named(m.tree(p), "Worked", ["AXButton"]), 8), settle=1.0)
    pics = m.wait_for(lambda: [m.summary(m.node(p, n["path"])) for n in m.tree(p) if (m.name(n) or "").startswith("Picture")], 8)
    pt.check("a missing picture is named Picture <n>, <the core's sentence>",
             bool(pics) and any(x.get("AXTitle", "").startswith("Picture 1, ") for x in pics), pics)
    pt.shot("missing-picture")


@m.part
def p_fatal(pt):
    """The fatal screen: the alert's name holds the message; Restart has
    focus; VoiceOver's phrase; Return starts the app again (par-shell)."""
    msg = "The engine could not start: a test message"
    p = m.launch(f"--fatal '{msg}'", min_nodes=7)
    t = m.tree(p)
    alert = [n for n in t if (m.name(n) or "").startswith("mcbench-lite stopped on an error")]
    pt.check("the alert's name holds the message", bool(alert) and m.name(alert[0]) == f"mcbench-lite stopped on an error: {msg}",
             [m.summary(m.node(p, a["path"])) for a in alert])
    pt.check("Restart has focus", m.fname(p) == ("AXButton", "Restart"), m.fname(p))
    pt.shot("fatal")
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(2.0)
    pt.note("VoiceOver after activating the fatal window", m.vo_watch(4))
    pt.note("VO-Right", m.vo(pt, "VO-Right", "right", m.VO, wait=1.0))
    pt.note("VO-Left", m.vo(pt, "VO-Left", "left", m.VO, wait=1.0))
    m.vo_off()
    before = sh("pgrep -x mcbench-ui-dev || true").split()
    m.keys("return", settle=4.0)
    after = sh("pgrep -x mcbench-ui-dev || true").split()
    pt.check("Return restarts: the screen's process ends and a new one runs",
             bool(after) and not set(before) & set(after), f"{before} -> {after}")
    if after:
        sh(f"kill {' '.join(after)}")


@m.part
def p_crash(pt):
    """--crash panic|abort|segv|objc: each leaves a log, exits quietly, no
    dialog, nothing in DiagnosticReports."""
    for how in ["panic", "abort", "segv", "objc"]:
        reports = sh("ls ~/Library/Logs/DiagnosticReports /Library/Logs/DiagnosticReports 2>/dev/null | sort").splitlines()
        m.launch(f"{GATE} --crash {how} --crash-after-ms 4000", wait=3.0, activate=False, wait_tree=False)
        first = sh("pgrep -x mcbench-ui-dev || true").split()
        time.sleep(4.0)
        # par-shell: a panic starts the program again into the error screen
        # (a new process); the others end with no screen.
        gone = m.wait_for(lambda: not set(first) & set(sh("pgrep -x mcbench-ui-dev || true").split()), 10)
        if how == "panic":
            screen = m.wait_for(lambda: sh("pgrep -x mcbench-ui-dev || true").split(), 10) or []
            pt.check("panic: the error screen opens in a new process", bool(screen), screen)
            time.sleep(2.0)
            pt.shot("after-panic-screen")
            if screen:
                sh(f"kill {' '.join(screen)}")
        time.sleep(3.0)
        wins = [w for w in m.jl(gx(G, "windows")) if w.get("owner") in ("ProblemReporter", "UserNotificationCenter", "CrashReporter", "ReportCrash", "Problem Reporter")]
        after = sh("ls ~/Library/Logs/DiagnosticReports /Library/Logs/DiagnosticReports 2>/dev/null | sort").splitlines()
        logs = sh(f"ls {m.SCRATCH}/logs 2>/dev/null; tail -c 600 {m.SCRATCH}/logs/* 2>/dev/null")
        pt.check(f"{how}: the process exits", bool(gone))
        pt.check(f"{how}: a crash log is written", bool(sh(f"ls {m.SCRATCH}/logs 2>/dev/null").strip()), logs[:600])
        pt.check(f"{how}: no crash dialog", not wins, wins)
        pt.check(f"{how}: nothing new in DiagnosticReports", [x for x in after if x not in reports and "mcbench" in x] == [],
                 [x for x in after if x not in reports])
        pt.shot(f"after-{how}")
        pt.note(f"{how}: stdout and stderr", sh("cat /tmp/ui.out")[-800:])


def press_to_screen(p, rounds=6):
    """AXPress on the archived section's toggle, `rounds` times: how long
    until the screen shows it and until AX does (the frame that answers an
    action that came in without a key event). Observations, not timings."""
    h = lambda: sh("screencapture -x -R85,700,250,180 /tmp/f.png && /sbin/md5 -q /tmp/f.png").strip()[:8]
    out = []
    for _ in range(rounds):
        t = m.tree(p)
        n = named(t, "Show archived", ["AXButton"]) or named(t, "Hide archived", ["AXButton"])
        was = m.name(n)
        h0 = h()
        gx(G, "act", p, n["path"], "AXPress")
        t0 = time.time()
        seen = [None, None]
        while time.time() - t0 < 30 and None in seen:
            if seen[0] is None and h() != h0:
                seen[0] = round(time.time() - t0, 1)
            if seen[1] is None and m.node(p, n["path"]).get("AXTitle") != was:
                seen[1] = round(time.time() - t0, 1)
        out.append({"from": was, "screen_s": seen[0], "ax_s": seen[1]})
        time.sleep(1.0)
    return out


@m.part
def p_frames(pt):
    """Frames that answer an AX action (open question (e)'s other face):
    AXPress on Show archived six times on the gate fixture, idle and with a
    chat running (its dot pulses), and with APP_ENV in the app's environment."""
    for label, args in [("idle", GATE), ("a chat running", f"{GATE} --running chat-gate")]:
        p = m.launch(args)
        time.sleep(2)
        res = press_to_screen(p)
        pt.note(f"{label} ({os.environ.get('APP_ENV', 'no APP_ENV')}): seconds until the screen and AX show an AXPress", res)
        pt.check(f"{label}: every AXPress shows on the screen within 1 s", all(r["screen_s"] is not None and r["screen_s"] <= 1.0 for r in res), res)


@m.part
def p_lag(pt):
    """The lag after launch (open question (e)): --fixture long, 30 wheel
    clicks over the transcript at once after the window shows, then how long
    until the AX tree and the screen stop changing; with and without
    --a11y-dump, with and without an AX client before the scroll; a sample
    of the process while it is behind. Observations, not timings (the
    Mac's rule: no timing in this task)."""
    for dump in (False, True):
        for client in (True, False):
            label = f"dump-{'on' if dump else 'off'}-client-{'on' if client else 'off'}"
            args = "--fixture long" + (" --a11y-dump /tmp/dump.json" if dump else "")
            sh("pkill -x mcbench-ui-dev; sleep 0.5; rm -f /tmp/dump.json", check=False)
            sh(f"rm -rf {m.SCRATCH}; mkdir -p {m.SCRATCH}; cd /tmp; {APP} --scratch {m.SCRATCH} --log /tmp/ui.log {args} > /tmp/ui.out 2>&1 &")
            time.sleep(3.0)
            p = m.gate.pid()
            if client:
                m.wait_for(lambda: len(m.tree(p)) > 20, 30, 0.5)
            sent = time.time()
            for _ in range(30):
                gx(G, "scroll", "600", "450", "5")
            sh(f"(sample {p} 5 -file /tmp/sample.txt > /dev/null 2>&1 &)")
            shots = []
            prev = None
            still = 0
            for i in range(60):
                h = sh("screencapture -x -R330,140,500,600 /tmp/l.png && /sbin/md5 -q /tmp/l.png").strip()
                ax = None
                if client:
                    arts = [n["AXPosition"][1] for n in m.tree(p) if n.get("AXSubrole") == "AXDocumentArticle"]
                    ax = tuple(arts[:3])
                cur = (h, ax)
                shots.append({"t": round(time.time() - sent, 1), "screen": h[:8], "ax": ax})
                still = still + 1 if cur == prev else 0
                prev = cur
                if still >= 4:
                    break
                time.sleep(0.5)
            pt.note(f"{label}: screen and AX over time after 30 wheel clicks", shots)
            time.sleep(5)
            sample = sh("cat /tmp/sample.txt 2>/dev/null | sed -n '1,/^Total number/p' | head -120")
            pt.note(f"{label}: sample of the process right after the scroll (top of the main thread)", sample[:6000])
            pt.save()



def vo_seq(pt, steps, wait=0.7):
    """Keys with VoiceOver on; each step's last phrase recorded. A step is
    (label, key, mods) or ("watch", seconds)."""
    out = []
    for st in steps:
        if st[0] == "watch":
            ph = m.vo_watch(st[1])
            pt.note(f"VO heard over {st[1]} s", ph)
            out.append(ph)
            continue
        label, k, mods = st
        out.append(m.vo(pt, label, k, mods, wait=wait))
    return out


@m.part
def p_vo_sidebar(pt):
    """VoiceOver on the sidebar: the rows' spoken names, walking the list
    with the app's arrows (opens nothing), the bench fold's state, the
    palette as the highlight moves, the filter menu, arming Delete."""
    p = m.launch(f"{GATE} --running chat-gate")
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(1.5)
    m.keys(*["tab"] * 10)
    ph = vo_seq(pt, [("focus the current row (Tab, then read)", "tab", "shift"), ("Tab to the row", "tab", ""),
                     ("Down", "down", ""), ("Down", "down", ""), ("Home", "home", ""),
                     ("VO-Down in the list", "down", m.VO), ("VO-Down", "down", m.VO)])
    pt.check("walking the list with VoiceOver opened nothing", not [a for a in m.log() if a["action"].startswith("opened")], m.log()[-3:])
    pt.check("a running row is spoken with its status", any("Castle gate, Working" in (x or "") for x in ph), ph)
    pt.check("an unseen row is spoken with Finished, not opened yet", any("Finished, not opened yet" in (x or "") for x in ph), ph)
    m.keys("k:cmd", settle=1.0)
    ph = vo_seq(pt, [("watch", 2), ("Down in the palette", "down", ""), ("watch", 2), ("Down", "down", ""), ("watch", 2)])
    pt.check("the palette's highlight is spoken as it moves", sum(1 for x in ph if x) >= 2, ph)
    m.keys("escape", settle=0.8)
    f = named(m.tree(p), "Filter", ["AXButton"])
    m.setfocus(p, f)
    vo_seq(pt, [("read Filter (VO-F3 is not used: Tab back and forth)", "tab", "shift"), ("Tab to Filter", "tab", ""),
                ("Enter opens the menu", "return", ""), ("Down", "down", ""), ("Space chooses", "space", ""), ("Escape", "escape", "")])
    m.keys("tab", "tab")  # the chip, then on; the list is narrowed: clear it
    for n in [n for n in m.tree(p) if m.name(n) == "Clear all"]:
        m.press(p, n)
    tog = named(m.tree(p), "Show archived", ["AXButton"])
    m.setfocus(p, tog)
    vo_seq(pt, [("Shift-Tab and Tab to Show archived", "tab", "shift"), ("Tab", "tab", ""), ("Enter opens it", "return", ""),
                ("Tab to Restore", "tab", ""), ("Tab to Delete permanently", "tab", ""), ("Space arms it", "space", ""),
                ("watch", 5)], wait=0.8)
    m.keys("two:cmd", settle=1.0)
    bench = rows(m.tree(p), "Benches")[0]
    m.setfocus(p, bench)
    vo_seq(pt, [("Shift-Tab, Tab to the bench", "tab", "shift"), ("Tab", "tab", ""), ("Tab to Show models", "tab", ""),
                ("Enter opens the fold", "return", ""), ("VO-Space toggles it", "space", m.VO), ("VO-Space again", "space", m.VO)], wait=0.9)
    m.vo_off()


@m.part
def p_vo_chat(pt):
    """VoiceOver during a streamed turn (--turn full): what is said while it
    runs (nothing every second from the working line), the tool rows and
    plan card by VO-Right, the lightbox, edit, Settings and its Escape."""
    p = m.launch(CHAT)
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(1.5)
    focus_composer(p)
    m.typ("Make it brighter", settle=0.3)
    m.keys("return")
    heard = m.vo_watch(14, 0.4)
    pt.note("VoiceOver while the full turn runs (polled every 0.4 s)", heard)
    pt.check("the working line is not announced every second", sum(1 for x in heard if (x or "").startswith("Working for")) <= 1, heard)
    gx(G, "key", str(m.K["tab"]), "shift")
    time.sleep(0.6)
    vo_seq(pt, [("the current message", "up", ""), ("Up", "up", ""), ("Up", "up", ""), ("Right into the message", "right", ""),
                ("Right", "right", ""), ("Right", "right", ""), ("Right", "right", ""), ("Tab to its parts", "tab", ""), ("Tab", "tab", ""),
                ("Tab", "tab", ""), ("Tab", "tab", "")], wait=0.8)
    shot = named(m.tree(p), "Picture 1")
    if shot:
        m.setfocus(p, shot)
        vo_seq(pt, [("Enter on the picture (the lightbox)", "return", ""), ("watch", 2), ("Escape", "escape", ""), ("watch", 2)])
    s = named(m.tree(p), "Settings", ["AXButton"])
    m.setfocus(p, s)
    vo_seq(pt, [("Enter on Settings", "return", ""), ("watch", 2), ("Escape", "escape", ""), ("watch", 2)])
    eb = [n for n in m.tree(p) if m.name(n) == "Edit and branch"]
    if eb:
        m.setfocus(p, eb[-1])
        vo_seq(pt, [("Space on Edit and branch", "space", ""), ("watch", 2), ("Escape cancels the edit", "escape", ""), ("watch", 2)])
    m.vo_off()


@m.part
def p_vo_pickers(pt):
    """VoiceOver on the pickers (the gallery): the triggers, the model
    picker's dialog, its rail, a star; the effort list; the slider by keys
    and by VoiceOver's own increment (VO-Up and VO-Down while interacting);
    the context meter and the limit chip."""
    p = m.launch(PICKERS)
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(1.5)
    trig = named(m.tree(p), "Chat model: claude-opus-5-5", ["AXButton"])
    m.setfocus(p, trig)
    vo_seq(pt, [("Shift-Tab, Tab to the trigger", "tab", "shift"), ("Tab", "tab", ""), ("Enter opens it", "return", ""), ("watch", 2),
                ("Down to the first model", "down", ""), ("Down", "down", ""), ("VO-Right", "right", m.VO), ("VO-Right", "right", m.VO),
                ("Escape", "escape", "")], wait=0.9)
    res = named(m.tree(p), "Resolution: Standard", ["AXButton"])
    m.setfocus(p, res)
    vo_seq(pt, [("Shift-Tab, Tab to Resolution", "tab", "shift"), ("Tab", "tab", ""), ("Enter opens the slider", "return", ""), ("watch", 2),
                ("Right", "right", ""), ("VO-Shift-Down (interact)", "down", m.VO + ",shift"), ("VO-Up (increment)", "up", m.VO),
                ("VO-Down (decrement)", "down", m.VO), ("VO-Shift-Up", "up", m.VO + ",shift"), ("Escape", "escape", "")], wait=0.9)
    eff = named(m.tree(p), "Reasoning effort: medium", ["AXButton"])
    m.setfocus(p, eff)
    vo_seq(pt, [("Shift-Tab, Tab to effort", "tab", "shift"), ("Tab", "tab", ""), ("Enter opens it", "return", ""),
                ("Down", "down", ""), ("Escape", "escape", "")], wait=0.9)
    meter = named(m.tree(p), "84% of context · 840k of 1M tokens")
    if meter:
        m.setfocus(p, meter)
        vo_seq(pt, [("Shift-Tab, Tab to the meter", "tab", "shift"), ("Tab", "tab", ""), ("Tab to the chip", "tab", "")], wait=0.9)
    m.vo_off()



def win_frame(p):
    w = m.jl(gx(G, "win", p, "list"))
    return w[0].get("AXPosition"), w[0].get("AXSize")


@m.part
def p_frame(pt):
    """The first window inside the display's visible area (vendor patch 35),
    then quit and start three times with the same --scratch and
    --preferences: the frame must not drift."""
    sh("rm -f /tmp/prefs-frame.json", check=False)
    p = m.launch(f"{GATE} --preferences /tmp/prefs-frame.json")
    pos, size = win_frame(p)
    vis = sh("osascript -l JavaScript -e 'ObjC.import(\"AppKit\"); var f=$.NSScreen.mainScreen.visibleFrame; var s=$.NSScreen.mainScreen.frame; JSON.stringify([s.size.width,s.size.height,f.origin.x,f.origin.y,f.size.width,f.size.height])'").strip()
    sw, shh, vx, vy, vw, vh = json.loads(vis)
    top = shh - (vy + vh)  # the visible area's top in top-left coordinates (below the menu bar)
    pt.note("the screen and its visible area (top-left coordinates: x, top, width, height)", [sw, shh, vx, top, vw, vh])
    inside = pos[0] >= vx and pos[1] >= top and pos[0] + size[0] <= vx + vw and pos[1] + size[1] <= top + vh
    pt.check("the first window lies inside the visible area", inside, [pos, size])
    pt.check("and is centred in it", abs((pos[0] + size[0] / 2) - (vx + vw / 2)) <= 1 and abs((pos[1] + size[1] / 2) - (top + vh / 2)) <= 1, [pos, size])
    pt.shot("first-window")
    frames = [[pos, size]]
    for k in range(3):
        m.keys("q:cmd", settle=2.0)
        p = m.launch(f"{GATE} --preferences /tmp/prefs-frame.json")
        frames.append(list(win_frame(p)))
    pt.note("the frame at each start", frames)
    pt.check("three restarts keep the same frame", all(f == frames[0] for f in frames), frames)
    pt.note("the preferences file", sh("cat /tmp/prefs-frame.json"))


@m.part
def p_focus(pt):
    """Focus never lost (W1): after each action the app's AXFocusedUIElement
    is a real control and Tab moves on; with VoiceOver on, what it says (it
    must not re-read the window)."""
    sh("rm -rf /tmp/pick; mkdir -p /tmp/pick; cp /tmp/s4/lamp.png /tmp/pick/door.png 2>/dev/null; true", check=False)
    p = m.launch(f"{GATE} --refuse chat-farm")
    sh(f"cp {m.SCRATCH}/lamp.png /tmp/pick/door.png")
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(1)

    def after(label):
        time.sleep(0.8)
        f = m.focused(p)
        ph = m.phrase()
        role, nm = f.get("AXRole"), m.name(f)
        ok = role not in (None, "AXWindow", "AXApplication") and bool(nm)
        m.keys("tab", settle=0.8)
        f2 = m.focused(p)
        pt.check(f"{label}: focus on a real control, and Tab moves on", ok and (f2.get("AXRole"), m.name(f2)) != (role, nm),
                 {"focus": [role, nm], "spoken": ph, "after Tab": [f2.get("AXRole"), m.name(f2)]})
        pt.check(f"{label}: VoiceOver does not re-read the window", "mcbench-lite" not in (ph or "") and "window" not in (ph or "").lower(), ph)

    # A notice dismissed.
    m.press(p, chat_row(p, "Wheat farm"), settle=1.0)
    focus_composer(p)
    m.typ("One more", settle=0.3)
    m.keys("return", settle=1.5)
    d = m.wait_for(lambda: named(m.tree(p), "Dismiss error", ["AXButton"]), 8)
    m.setfocus(p, d)
    m.keys("space")
    after("Dismiss error")
    # Archiving the open chat.
    t = m.tree(p)
    row = chat_row(p, "Wheat farm")
    btn = [n for n in t if n["path"].startswith(row["path"] + ".") and m.name(n) == "Archive build"][0]
    m.setfocus(p, btn)
    m.keys("space")
    after("Archive build on the open chat")
    # Deleting an archived item.
    tog = named(m.tree(p), "Show archived", ["AXButton"])
    if tog:
        m.press(p, tog)
    dels = [n for n in m.tree(p) if m.name(n) == "Delete permanently"]
    m.setfocus(p, dels[0])
    m.keys("space", settle=0.5)
    m.keys("space")
    after("Delete permanently, twice")
    # Cmd-2 to New bench.
    m.keys("two:cmd", settle=1.0)
    after("Cmd-2")
    m.keys("one:cmd", settle=1.0)
    # Closing the palette.
    m.keys("k:cmd", settle=1.0)
    m.keys("escape")
    after("the palette closed with Escape")
    # The lightbox.
    m.press(p, chat_row(p, "Redstone lamp with a lever"), settle=1.0)
    img = m.wait_for(lambda: [n for n in m.tree(p) if n.get("AXRole") == "AXImage"], 8)
    if img:
        m.setfocus(p, m.visible(p, img[0]))
        m.keys("return", settle=1.0)
        m.keys("escape")
        after("the lightbox closed with Escape")
    # Removing an attachment.
    m.press(p, named(m.tree(p), "Attach file", ["AXButton"]), settle=2.0)
    gx(G, "key", "5", "cmd,shift"); time.sleep(1.0)
    m.typ("/tmp/pick/door.png", settle=0.5)
    m.keys("return", settle=1.0)
    m.keys("return", settle=2.0)
    rm = m.wait_for(lambda: named(m.tree(p), "Remove attachment", ["AXButton"]), 8)
    m.setfocus(p, rm)
    m.keys("space")
    after("Remove attachment")
    m.vo_off()


@m.part
def p_hidden(pt):
    """Rows kept outside the transcript's viewport (W2): what AX reports for
    them; VoiceOver's next item from the last row on screen; a click on the
    title bar."""
    p = m.launch(GATE)
    region = m.one(m.tree(p), sub="AXLandmarkRegion")
    top, bottom = region["AXPosition"][1], region["AXPosition"][1] + region["AXSize"][1]
    arts = [n for n in m.tree(p) if n.get("AXSubrole") == "AXDocumentArticle"]
    outside = [n for n in arts if n["AXPosition"][1] + n["AXSize"][1] <= top or n["AXPosition"][1] >= bottom]
    pt.note("the transcript's rows: name, y, height, outside the viewport", [((m.name(n) or "")[:40], n["AXPosition"][1], n["AXSize"][1], n in outside) for n in arts])
    for n in outside[:2]:
        pt.note("a row outside the viewport, every attribute", m.describe_all(p, n["path"]))
    vis_children = m.node(p, region["path"]).get("AXVisibleChildren")
    pt.note("the region's AXVisibleChildren", vis_children)
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(1)
    m.setfocus(p, arts[-1])
    heard = []
    for k in range(10):
        heard.append(m.vo(pt, "VO-Left from the last row", "left", m.VO, wait=0.6))
    pt.note("VO-Left through the transcript (the rows above the screen scroll in as focus reaches them)", heard)
    w = [w for w in m.jl(gx(G, "windows")) if w.get("pid") == int(p)][0]["b"]
    gx(G, "click", str(w[0] + w[2] / 2), str(w[1] + 12))
    time.sleep(1.5)
    pt.note("VoiceOver after a click on the title bar", m.vo_watch(3))
    m.vo_off()


@m.part
def p_sets(pt):
    """Set sizes spoken (W4): the filter menu, the provider rail, the model
    list, the effort list; the notice's Dismiss error before the transcript
    in the Tab order; the search's failure line (--search-fails)."""
    p = m.launch(f"{GATE} --refuse chat-farm --search-fails")
    m.vo_on()
    gx(G, "activate", p)
    time.sleep(1)
    f = named(m.tree(p), "Filter", ["AXButton"])
    m.setfocus(p, f)
    m.keys("return", settle=1.0)
    menu = [m.vo(pt, "filter menu Down", "down", "", wait=0.7) for _ in range(3)]
    pt.check("the filter menu's items are spoken with n of m", any(" of " in (x or "") for x in menu), menu)
    m.keys("escape", settle=0.8)
    m.press(p, named(m.tree(p), "Reasoning effort: medium", ["AXButton"]), settle=1.0)
    eff = [m.vo(pt, "effort Down", "down", "", wait=0.7) for _ in range(2)]
    pt.check("the effort list's options are spoken with n of m", any(" of " in (x or "") for x in eff), eff)
    m.keys("escape", settle=0.8)
    m.press(p, named(m.tree(p), "Chat model: claude-opus-5-5", ["AXButton"]), settle=1.0)
    mods = [m.vo(pt, "model list Down", "down", "", wait=0.7) for _ in range(2)]
    pt.check("the model list's options are spoken with n of m", any(" of " in (x or "") for x in mods), mods)
    # With VoiceOver on, Down in a list box chooses (VoiceOver sets AXSelected,
    # AccessKit makes it a click: FOR-CHAT 10), so the picker may be closed.
    if not named(m.tree(p), "Providers"):
        trig = [n for n in m.tree(p) if (m.name(n) or "").startswith("Chat model:") and n.get("AXRole") == "AXButton"][0]
        m.press(p, trig, settle=1.0)
    rail = named(m.tree(p), "Providers")
    tab = [n for n in m.tree(p) if rail and n["path"].startswith(rail["path"] + ".") and n.get("AXSubrole") == "AXTabButton" and m.node(p, n["path"]).get("AXEnabled")]
    m.setfocus(p, tab[0])
    tabs = [m.vo(pt, "rail Tab and Shift-Tab", k, mods_, wait=0.7) for k, mods_ in [("tab", "shift"), ("tab", "")]]
    tabs += [m.vo(pt, "rail Down", "down", "", wait=0.7) for _ in range(2)]
    pt.check("a provider tab is spoken with n of m", any(" of " in (x or "") for x in tabs), tabs)
    pt.shot("rail")
    m.keys("escape", settle=0.8)
    # The search fails: the palette and the sidebar say so.
    m.keys("k:cmd", settle=1.0)
    m.typ("gate", settle=1.5)
    t = m.tree(p)
    fail = [label_text(n) for n in t if "fail" in (label_text(n) or "").lower() or "could not" in (label_text(n) or "").lower()]
    pt.check("the palette shows the search's failure line", bool(fail), fail)
    pt.note("VoiceOver on the failure", m.vo_watch(2))
    pt.shot("palette-search-fails")
    m.keys("escape", settle=0.8)
    s = named(m.tree(p), "Search chats", ["AXTextField"])
    m.click(s)
    m.typ("gate", settle=1.5)
    t = m.tree(p)
    fail = [label_text(n) for n in t if "fail" in (label_text(n) or "").lower() or "could not" in (label_text(n) or "").lower()]
    pt.check("the sidebar shows the search's failure line", bool(fail), fail)
    pt.shot("sidebar-search-fails")
    m.keys("escape", settle=0.8)
    # The notice's Dismiss error: before the transcript in the Tab order.
    m.press(p, chat_row(p, "Wheat farm"), settle=1.0)
    focus_composer(p)
    m.typ("One more", settle=0.3)
    m.keys("return", settle=1.5)
    order = []
    for _ in range(16):
        m.keys("tab", settle=0.4)
        order.append(m.fname(p))
    pt.note("the Tab order with a notice", order)
    names_ = [x[1] for x in order]
    di = names_.index("Dismiss error") if "Dismiss error" in names_ else None
    ti = next((i for i, x in enumerate(order) if x[0] == "AXGroup" and (x[1] or "").startswith(("claude-opus", "You,", "Error,"))), None)
    pt.check("Dismiss error comes before the transcript's current message", di is not None and ti is not None and di < ti, [di, ti])
    m.vo_off()


@m.part
def p_pulse(pt):
    """The running dot (W6): it moves while the window is active and rests
    while it is inactive. GPUI's macOS window reports no GPU specs, which
    the app takes for a hardware adapter."""
    p = m.launch(f"{GATE} --running chat-gate")
    row = chat_row(p, "Castle gate")
    x, y = row["AXPosition"][0], row["AXPosition"][1]
    h = lambda: sh(f"screencapture -x -R{x},{y},24,{row['AXSize'][1]} /tmp/d.png && /sbin/md5 -q /tmp/d.png").strip()[:8]
    active = [h() for _ in range(8)]
    pt.check("the dot moves while the window is active", len(set(active)) > 1, active)
    gx(G, "click", "1300", "880")  # the desktop: Finder becomes active
    time.sleep(1.5)
    pt.note("front after clicking the desktop", gx(G, "front").strip())
    inactive = [h() for _ in range(8)]
    pt.check("the dot rests while the window is inactive", len(set(inactive)) == 1, inactive)
    pt.shot("inactive")



@m.part
def p_shots(pt):
    """The four screenshots: the whole window on the gate fixture with the
    pickers, the palette open, the model picker open, a streamed turn with
    tool rows and a picture."""
    p = m.launch(GATE)
    gx(G, "move", "1420", "880")
    time.sleep(1.5)
    pt.shot("window-gate")
    m.keys("k:cmd", settle=1.5)
    pt.shot("palette")
    m.keys("escape", settle=0.8)
    m.press(p, named(m.tree(p), "Chat model: claude-opus-5-5", ["AXButton"]), settle=1.5)
    pt.shot("model-picker")
    m.keys("escape", settle=0.8)
    p = m.launch(f"{GATE} --turn full --rate-ms 15")
    focus_composer(p)
    m.typ("Make it brighter", settle=0.3)
    m.keys("return")
    wait_turn(p)
    time.sleep(1.0)
    fold = m.wait_for(lambda: named(m.tree(p), "Worked · 4 steps", ["AXButton"]), 10)
    m.press(p, fold, settle=1.0)
    m.setfocus(p, named(m.tree(p), "Worked · 4 steps", ["AXButton"]), settle=1.0)
    tool = m.wait_for(lambda: named(m.tree(p), "Built: a stone base and a lamp"), 10)
    pt.check("the tool rows are open for the shot", tool is not None)
    gx(G, "move", "1420", "880")
    time.sleep(1.5)
    pt.shot("streamed-turn")


if __name__ == "__main__":
    run = sys.argv[1]
    failed = {}
    for name_ in sys.argv[2:]:
        m.gate.on_battery_stop()
        pt = m.Part(run, name_)
        try:
            m.PARTS[name_](pt)
        except Exception as e:  # a part that breaks is a failed check; the next part runs
            import traceback
            pt.check(f"the part ran to its end ({type(e).__name__}: {e})", False, traceback.format_exc()[-1200:])
        failed[name_] = pt.save()
        m.gate.on_battery_stop()
    print(json.dumps(failed, ensure_ascii=False))
