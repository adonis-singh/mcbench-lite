"""Tree: the AT-SPI tree of the gate fixture as the app starts (tree.json),
then with the first rows brought in by the keyboard and the turn's work
fold opened through its Action interface (tree-open.json); a screenshot of
each; Ctrl+Q at the end. tree-open.json without its screen extents is
compared with the committed expected/gate-tree-open.json (the same on X11
and Wayland); MCBENCH_UPDATE_EXPECTED=1 rewrites that file instead. Every
object's interfaces, states, text, actions, table shape, link and
relations are in the dumps (atspi_tree.py)."""

import json
import os
import time

import common

EXPECTED = os.path.join(os.path.dirname(os.path.abspath(__file__)), "expected", "gate-tree-open.json")
report = common.Report("tree")


def dump(name):
    tree = common.atspi_tree.walk(common.app())
    with open(common.out(name), "w") as f:
        json.dump(tree, f, indent=1, ensure_ascii=False)
    return tree


def normalized(node):
    """A node without what depends on the display: its extents."""
    out = {k: v for k, v in node.items() if k not in ("extents", "children")}
    if "children" in node:
        out["children"] = [normalized(c) for c in node["children"]]
    return out


def own(node):
    return {k: v for k, v in node.items() if k != "children"}


def differences(a, b, path="app"):
    if own(a) != own(b):
        yield f"{path}: {json.dumps(own(a), ensure_ascii=False)[:240]} != {json.dumps(own(b), ensure_ascii=False)[:240]}"
    ca, cb = a.get("children", []), b.get("children", [])
    if len(ca) != len(cb):
        yield f"{path}: {len(ca)} children != {len(cb)}"
    for i, (x, y) in enumerate(zip(ca, cb)):
        yield from differences(x, y, f"{path}/{i}:{x.get('role')}")


common.app()
time.sleep(2.0)
common.focus_window()
dump("tree.json")
common.screenshot("window.png")
# From the composer, Shift+Tab reaches the current (last) message; Home
# brings the first in, Down the second, which holds the fold.
common.key("shift+Tab")
common.key("Home", settle=0.6)
common.key("Down", settle=0.6)
fold = common.one(common.app(), "button", pred=lambda o: o.get_name().startswith("Worked"))
common.Atspi.Action.do_action(fold, 0)
time.sleep(0.8)
tree = normalized(dump("tree-open.json"))
common.screenshot("window-open.png")
if os.environ.get("MCBENCH_UPDATE_EXPECTED"):
    os.makedirs(os.path.dirname(EXPECTED), exist_ok=True)
    with open(EXPECTED, "w") as f:
        json.dump(tree, f, indent=1, ensure_ascii=False)
        f.write("\n")
    report.check("expected tree written", True, EXPECTED)
else:
    with open(EXPECTED) as f:
        expected = json.load(f)
    found = list(differences(tree, expected))
    for line in found[:20]:
        print("  " + line)
    report.check("the tree matches the expected dump", not found, f"{len(found)} differences")
common.quit_key(settle=1.5)
report.finish()
