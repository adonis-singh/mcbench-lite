"""The AT-SPI tree of the dev binary's window, as JSON.

    python3 atspi_tree.py <out.json> [--wait SECONDS]

Finds the application whose name is `mcbench-ui-dev` on the session's
AT-SPI bus (the private session of session.sh), and writes every object
under it: role, name, description, states, the interfaces it offers, its
text (Text), value (Value), actions (Action), table shape (Table), link
(Hyperlink), selection count (Selection), relations and screen extents.
Also importable: `find_app()`, `walk()`, `describe()`.
"""

import json
import sys
import os
import time

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

# pane agent: the real app (real_pane.sh) is `mcbench-lite`.
APP = os.environ.get("MCBENCH_APP_NAME", "mcbench-ui-dev")


def find_app(wait=20.0):
    deadline = time.time() + wait
    while time.time() < deadline:
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(i)
            if app is not None and app.get_name() == APP:
                return app
        time.sleep(0.2)
    raise SystemExit(f"no application named {APP} on the AT-SPI bus after {wait} s")


def interfaces(obj):
    return sorted(obj.get_interfaces())


def describe(obj):
    node = {
        "role": obj.get_role_name(),
        "name": obj.get_name(),
        "description": obj.get_description(),
        "states": sorted(
            Atspi.StateType(s).value_nick for s in range(Atspi.StateType.LAST_DEFINED) if obj.get_state_set().contains(s)
        ),
        "interfaces": interfaces(obj),
    }
    try:
        ext = obj.get_extents(Atspi.CoordType.WINDOW)
        node["extents"] = [ext.x, ext.y, ext.width, ext.height]
    except Exception:  # an object without Component
        pass
    names = node["interfaces"]
    if "Text" in names:
        text = obj.get_text_iface() if hasattr(obj, "get_text_iface") else obj
        count = Atspi.Text.get_character_count(obj)
        node["text"] = Atspi.Text.get_text(obj, 0, count)
        node["caret"] = Atspi.Text.get_caret_offset(obj)
        node["selections"] = [
            [r.start_offset, r.end_offset]
            for r in (Atspi.Text.get_selection(obj, i) for i in range(Atspi.Text.get_n_selections(obj)))
        ]
        del text
    if "Value" in names:
        node["value"] = Atspi.Value.get_current_value(obj)
    if "Action" in names:
        node["actions"] = [Atspi.Action.get_action_name(obj, i) for i in range(Atspi.Action.get_n_actions(obj))]
    if "Table" in names:
        node["table"] = [Atspi.Table.get_n_rows(obj), Atspi.Table.get_n_columns(obj)]
    if "Hyperlink" in names or "Hypertext" in names:
        try:
            link = obj.get_hyperlink()
            node["uri"] = link.get_uri(0) if link else None
        except Exception as error:  # noqa: BLE001
            node["uri_error"] = str(error)
    if "Selection" in names:
        node["selected_children"] = Atspi.Selection.get_n_selected_children(obj)
    attributes = obj.get_attributes() or {}
    if attributes:
        node["attributes"] = dict(attributes)
    relations = []
    for relation in obj.get_relation_set() or []:
        relations.append(
            {
                "type": relation.get_relation_type().value_nick,
                "targets": [relation.get_target(i).get_name() for i in range(relation.get_n_targets())],
            }
        )
    if relations:
        node["relations"] = relations
    return node


def walk(obj, depth=0, limit=60):
    node = describe(obj)
    if depth < limit:
        children = []
        for i in range(obj.get_child_count()):
            child = obj.get_child_at_index(i)
            if child is not None:
                children.append(walk(child, depth + 1, limit))
        if children:
            node["children"] = children
    return node


def main():
    out = sys.argv[1]
    wait = float(sys.argv[sys.argv.index("--wait") + 1]) if "--wait" in sys.argv else 20.0
    app = find_app(wait)
    tree = walk(app)
    with open(out, "w") as f:
        json.dump(tree, f, indent=1, ensure_ascii=False)
    print(f"wrote {out}")


if __name__ == "__main__":
    main()
