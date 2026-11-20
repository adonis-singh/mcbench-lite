"""One line per element of a `guestctl ax` dump (JSON lines): indent by
depth, role, subrole, title, description, value, focused, position, size,
actions. Menus are skipped. usage: python3 -I axsum.py <ax.jsonl>"""
import json, sys

SKIP = {"AXMenuItem", "AXMenu", "AXMenuBarItem", "AXMenuBar"}
for line in open(sys.argv[1]):
    d = json.loads(line)
    if d.get("AXRole") in SKIP:
        continue
    v = str(d.get("AXValue", ""))[:70].replace("\n", "\\n")
    print(" " * d["depth"], d.get("path", ""), d.get("AXRole"), d.get("AXSubrole", ""), "|", d.get("AXTitle", ""), "|",
          d.get("AXDescription", ""), "|", v, "| focused" if d.get("AXFocused") else "", d.get("AXPosition"),
          d.get("AXSize"), d.get("actions", ""))
