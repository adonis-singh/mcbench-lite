"""The real app's model picker (pane agent; run by real_pane.sh with
MCBENCH_REAL_DRIVER=real_models.py): the chat model picker opened, its
provider rail and each provider's list read through AT-SPI, a picture of
it. Writes models.json."""

import json
import time

import common

Atspi = common.Atspi
report = common.Report("real-models")
seen = {}


def app():
    return common.app(60.0)


app()
common.focus_window()
time.sleep(10.0)
trigger = common.wait_for(lambda: [n for n in common.find(app())
                                   if (n.get_name() or "").startswith("Chat model:")], 30.0, "the model picker")[0]
seen["trigger"] = trigger.get_name()
Atspi.Action.do_action(trigger, 0)
time.sleep(3.0)
tabs = common.find(app(), role="page tab")
seen["providers"] = [t.get_name() for t in tabs]
seen["lists"] = {}
for tab in tabs:
    name = tab.get_name()
    if not tab.get_state_set().contains(Atspi.StateType.ENABLED):
        continue
    Atspi.Action.do_action(tab, 0)
    time.sleep(3.0)
    dialog = [d for d in common.find(app(), name="Chat model") if d.get_role_name() == "dialog"]
    root = dialog[0] if dialog else app()
    seen["lists"][name] = [n.get_name() for n in common.find(root, role="list item") if n.get_name()][:40]
    seen.setdefault("labels", {})[name] = [n.get_name() for n in common.find(root, role="label") if n.get_name()][:6]
    common.screenshot(f"models-{len(seen['lists'])}.png")
claude = [k for k in seen["lists"] if "Claude Code" in k]
report.check("the picker has a Claude Code provider listing its models",
             bool(claude) and bool(seen["lists"][claude[0]]), json.dumps({k: v[:6] for k, v in seen["lists"].items()}))
with open(common.out("models.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.key("Escape", settle=0.5)
common.quit_key(settle=3.0)
report.finish()
