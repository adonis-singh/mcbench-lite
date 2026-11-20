"""Real-key walk in the guest: presses each key with guestctl (HID events in
the guest's session), then records the focused node(s) as the elements report
them (AXFocused), the application's AXFocusedUIElement, and a screenshot.

usage: python3 -I keywalk.py <pid> <out-name> <key>...
  key: tab, shift+tab, space, return, escape, up, down, left, right, cmd+q,
       or a raw code with mods, e.g. 96:cmd,fn
Environment: what vm/env.sh needs (RIG_HOME, OUT, VM). Writes
$OUT/evidence/<out-name>/walk.jsonl and NN-<key>.png."""
import json, os, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.environ["OUT"]
TART = os.path.join(os.environ["RIG_HOME"], "tools/tart-2.41.1/tart.app/Contents/MacOS/tart")
VM = os.environ.get("VM", "rig-s4")
G = "/usr/local/rig/guestctl"
CODES = {"tab": 48, "space": 49, "return": 36, "escape": 53, "left": 123, "right": 124, "down": 125, "up": 126,
         "q": 12, "w": 13, "m": 46, "f": 3, "a": 0, "c": 8, "v": 9}

def gx(*args):
    env = dict(os.environ, TART_HOME=os.path.join(os.environ["RIG_HOME"], "vm"))
    return subprocess.run([TART, "exec", VM, *args], capture_output=True, text=True, env=env, timeout=60)

def press(spec):
    if ":" in spec:
        code, mods = spec.split(":", 1)
    else:
        *mods, k = spec.split("+")
        code, mods = str(CODES[k]), ",".join(mods)
    r = gx(G, "key", code, *([mods] if mods else []))
    if r.returncode:
        sys.exit(f"key {spec}: {r.stderr}")

def main():
    pid, name, keys = sys.argv[1], sys.argv[2], sys.argv[3:]
    d = os.path.join(OUT, "evidence", name)
    os.makedirs(d, exist_ok=True)
    log = open(os.path.join(d, "walk.jsonl"), "w")
    for i, k in enumerate(["start"] + keys):
        if k != "start":
            press(k)
        time.sleep(0.4)
        nodes = json.loads(gx(G, "axfocus", pid).stdout or "{}").get("focusedNodes")
        app = gx(G, "focused", pid)
        app_focus = json.loads(app.stdout) if app.returncode == 0 else {"error": app.stderr.strip()}
        shot = os.path.join(d, f"{i:02d}-{k.replace(':', '_').replace('+', '_')}.png")
        with open(shot, "wb") as f:
            f.write(subprocess.run([TART, "exec", VM, "sh", "-c", "screencapture -x /tmp/k.png && cat /tmp/k.png"],
                                   capture_output=True, env=dict(os.environ, TART_HOME=os.path.join(os.environ["RIG_HOME"], "vm"))).stdout)
        rec = {"step": i, "key": k, "focusedNodes": [{x: n.get(x) for x in ("path", "AXRole", "AXSubrole", "AXTitle", "AXDescription", "AXValue")} for n in nodes or []],
               "appFocusedUIElement": {x: app_focus.get(x) for x in ("path", "AXRole", "AXTitle", "error")}, "shot": shot}
        log.write(json.dumps(rec, ensure_ascii=False) + "\n")
        print(json.dumps(rec, ensure_ascii=False))

main()
