"""The chat screen's contract checks on macOS, in the guest VM, against
`mcbench-ui-dev --fixture gate`. Each check writes its records under
$OUT/evidence/<run>/<check>/ and prints a one-line summary.

usage: python3 -I gate.py <run> <check>...
checks: launch tree text actions events keys select cell ime cache window vo
`launch` starts the app (installed at /usr/local/rig/mcbench-ui-dev) with
--log /tmp/ui.log and activates its window with a click; the others find
it by name. Environment: what vm/env.sh needs (RIG_HOME, OUT, VM)."""
import json, os, subprocess, sys, time

OUT = os.environ["OUT"]
RIG = os.environ["RIG_HOME"]
VM = os.environ.get("VM", "rig-s4")
TART = os.path.join(RIG, "tools/tart-2.41.1/tart.app/Contents/MacOS/tart")
ENV = dict(os.environ, TART_HOME=os.path.join(RIG, "vm"))
G = "/usr/local/rig/guestctl"
APP = "/usr/local/rig/mcbench-ui-dev"
K = {"tab": 48, "space": 49, "return": 36, "escape": 53, "left": 123, "right": 124, "down": 125, "up": 126,
     "home": 115, "end": 119, "pageup": 116, "pagedown": 121, "a": 0, "c": 8, "q": 12, "w": 13, "m": 46,
     "f": 3, "h": 4, "l": 37, "u": 32, "i": 34, "f5": 96, "delete": 51}


def gx(*args, inp=None, check=True, timeout=120):
    r = subprocess.run([TART, "exec", *(["-i"] if inp is not None else []), VM, *args], input=inp,
                       capture_output=True, env=ENV, timeout=timeout)
    if check and r.returncode:
        sys.exit(f"guest command {args[:3]} failed: {r.stderr.decode(errors='replace')}")
    return r.stdout.decode(errors="replace")


def sh(cmd, **kw):
    return gx("sh", "-c", cmd, **kw)


def key(name, mods=""):
    gx(G, "key", str(K[name]), *([mods] if mods else []))


def jl(text):
    return [json.loads(l) for l in text.splitlines() if l.strip().startswith("{")]


def pid():
    p = sh("pgrep -x mcbench-ui-dev || true").strip()
    if not p:
        sys.exit("mcbench-ui-dev is not running")
    return p


class Run:
    def __init__(self, run, check):
        self.dir = os.path.join(OUT, "evidence", run, check)
        os.makedirs(self.dir, exist_ok=True)
        self.n = 0

    def save(self, name, data):
        with open(os.path.join(self.dir, name), "w") as f:
            f.write(data if isinstance(data, str) else json.dumps(data, ensure_ascii=False, indent=1))

    def shot(self, label):
        self.n += 1
        path = os.path.join(self.dir, f"{self.n:02d}-{label}.png")
        with open(path, "wb") as f:
            f.write(subprocess.run([TART, "exec", VM, "sh", "-c", "screencapture -x /tmp/g.png && cat /tmp/g.png"],
                                   capture_output=True, env=ENV).stdout)
        return path

    def log_lines(self):
        return [json.loads(l) for l in sh("cat /tmp/ui.log 2>/dev/null || true").splitlines() if l.strip()]


def tree(p):
    return [d for d in jl(gx(G, "ax", p)) if not str(d.get("AXRole", "")).startswith("AXMenu")]


def focus(p):
    nodes = json.loads(gx(G, "axfocus", p)).get("focusedNodes", [])
    app = gx(G, "focused", p, check=False)
    a = json.loads(app) if app.strip().startswith("{") else {"error": app.strip()}
    pick = lambda n: {k: n.get(k) for k in ("path", "AXRole", "AXSubrole", "AXTitle", "AXDescription") if n.get(k) is not None}
    return {"nodes": [pick(n) for n in nodes], "app": pick(a) if "error" not in a else a}


def first(nodes, **want):
    for n in nodes:
        if all((n.get(k) == v) if not callable(v) else v(n.get(k)) for k, v in want.items()):
            return n
    return None


def c_launch(r):
    sh("pkill -x mcbench-ui-dev; rm -f /tmp/ui.log; sleep 0.5", check=False)
    sh(f"cd /tmp; rm -rf /tmp/s4; {APP} --fixture gate --scratch /tmp/s4 --log /tmp/ui.log {os.environ.get('APP_ARGS', '')} > /tmp/ui.out 2>&1 &")
    time.sleep(5)
    p = pid()
    win = [w for w in jl(gx(G, "windows")) if w.get("pid") == int(p)]
    before = json.loads(gx(G, "front"))
    r.save("start.json", {"pid": p, "windows": win, "front_before_click": before, "focus_before_click": focus(p)})
    b = win[0]["b"]
    gx(G, "click", str(b[0] + 100), str(b[1] + b[3] // 2))  # the sidebar's empty middle, below the rows
    time.sleep(0.6)
    after = {"front": json.loads(gx(G, "front")), "focus": focus(p)}
    r.save("after-click.json", after)
    r.shot("launched")
    print("launch", p, "front before click:", before["name"], "focus after click:", after["focus"])


def c_tree(r):
    p = pid()
    r.save("axall.jsonl", gx(G, "axall", p))
    t = tree(p)
    r.save("ax.jsonl", "\n".join(json.dumps(d, ensure_ascii=False) for d in t))
    print("tree", len(t), "elements")


def c_text(r):
    p = pid()
    t = tree(p)
    picks = {
        "heading": first(t, AXRole="AXHeading"),
        "paragraph": first(t, AXRole="AXGroup", AXSubrole="AXUnknown", AXValue=lambda v: isinstance(v, str) and len(v) > 20),
        "code": first(t, AXSubrole="AXCodeStyleGroup"),
        "cell": first(t, AXRole="AXCell"),
        "composer": first(t, AXRole="AXTextArea"),
    }
    out = {}
    for name, n in picks.items():
        if not n:
            out[name] = "not in the tree (not laid out or absent)"
            continue
        out[name] = jl(gx(G, "text", p, n["path"]))
    r.save("text.json", out)
    # The composer as real typing changes it.
    c = picks["composer"]
    steps = []
    gx(G, "click", str(c["AXPosition"][0] + 40), str(c["AXPosition"][1] + 20))
    for action in [("type", "Hello world"), ("key", "left", "alt"), ("key", "left", "alt,shift"), ("key", "a", "cmd"),
                   ("key", "delete", "")]:
        if action[0] == "type":
            gx(G, "type", action[1])
        else:
            key(action[1], action[2])
        time.sleep(0.3)
        n = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
        steps.append({"did": action, "AXValue": n.get("AXValue"), "AXSelectedTextRange": n.get("AXSelectedTextRange"),
                      "AXSelectedText": n.get("AXSelectedText"), "AXNumberOfCharacters": n.get("AXNumberOfCharacters")})
    r.save("composer.json", steps)
    for name, v in out.items():
        print("text", name, (v if isinstance(v, str) else json.dumps(v[:2], ensure_ascii=False))[:300])
    for s in steps:
        print("composer", s)


def c_actions(r):
    p = pid()
    nodes = jl(gx(G, "axall", p))
    acts = [{"path": n["_path"], "role": n.get("AXRole"), "title": n.get("AXTitle"), "actions": n.get("_actions")}
            for n in nodes if n.get("_actions") and not str(n.get("AXRole", "")).startswith("AXMenu")]
    r.save("actions.json", acts)
    results = []
    for role, label, action in [("AXLink", "the lever page", "AXPress"), ("AXImage", None, "AXPress"), ("escape", None, None),
                                ("top", None, None),
                                ("AXButton", "Worked · 2 steps", "AXPress"), ("AXButton", "Worked · 2 steps", "AXPress"),
                                ("AXGroup", "Castle gate", "AXPress"), ("AXGroup", "Redstone lamp with a lever", "AXPress"),
                                ("AXButton", "New chat", "AXPress")]:
        if role == "escape":
            # The image's press opened the lightbox; Escape closes it.
            key("escape")
            time.sleep(0.5)
            continue
        if role == "top":
            # The fold is in the second message, above the screen at launch:
            # Shift-Tab from the composer to the current message, Home, Down.
            c = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
            gx(G, "click", str(c["AXPosition"][0] + 40), str(c["AXPosition"][1] + 20))
            for k, m in [("tab", "shift"), ("home", ""), ("down", "")]:
                key(k, m)
                time.sleep(0.4)
            continue
        t = tree(p)
        n = first(t, AXRole=role, **({"AXTitle": label} if label else {}))
        if not n:
            results.append({"target": [role, label], "result": "not in tree"})
            continue
        before = len(r.log_lines())
        res = json.loads(gx(G, "act", p, n["path"], action, check=False) or "{}")
        time.sleep(0.6)
        results.append({"target": [role, label], "AXError": res.get("AXError"), "log": r.log_lines()[before:]})
    r.save("perform.json", results)
    for a in acts:
        print("offers", a["role"], a["title"], a["actions"])
    for x in results:
        print("act", x)


def c_events(r):
    p = pid()
    gx(G, "pbclear")
    sh(f"{G} observe {p} 40 > /tmp/obs.jsonl 2>&1 &")
    time.sleep(1)
    c = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
    gx(G, "click", str(c["AXPosition"][0] + 40), str(c["AXPosition"][1] + 20))
    gx(G, "type", "Make it brighter")
    time.sleep(0.5)
    key("return")
    time.sleep(1.0)
    r.shot("streaming")
    time.sleep(5)
    t = tree(p)
    fold = first(t, AXRole="AXButton", AXTitle=lambda v: isinstance(v, str) and v.startswith("Worked"))
    if fold:
        gx(G, "act", p, fold["path"], "AXPress", check=False)
    time.sleep(31)
    obs = jl(sh("cat /tmp/obs.jsonl"))
    r.save("observer.jsonl", "\n".join(json.dumps(o, ensure_ascii=False) for o in obs))
    kinds = {}
    for o in obs:
        if "note" in o:
            kinds.setdefault(o["note"], []).append(o.get("role"))
    for k, v in sorted(kinds.items()):
        print("event", k, len(v), sorted(set(x for x in v if x)))
    print("log", r.log_lines()[-4:])


def walk(r, keys, label):
    p = pid()
    rec = []
    for k in keys:
        before = len(r.log_lines())
        name, _, mods = k.partition(":")
        key(name, mods)
        time.sleep(0.4)
        f = focus(p)
        shot = r.shot(f"{label}-{k.replace(':', '_').replace(',', '_')}")
        rec.append({"key": k, "focus": f, "log": r.log_lines()[before:], "shot": os.path.basename(shot)})
        print(label, k, f["nodes"][:1], "app:", f["app"].get("AXRole"), f["app"].get("AXTitle"), r.log_lines()[before:])
    return rec


def c_keys(r):
    p = pid()
    c = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
    gx(G, "click", str(c["AXPosition"][0] + 40), str(c["AXPosition"][1] + 20))
    time.sleep(0.3)
    # Composer, Attach file, the three pickers, the sidebar (mode switcher, New
    # chat, Collapse sidebar, search, Filter, the current row, its Archive
    # build, Show archived, Settings), the current message, the composer.
    rec = walk(r, ["tab"] * 15 + ["tab:shift"] * 15, "tab")
    # The chat list: Tab to it, then Down, Up, End, Home, Return.
    rec += walk(r, ["tab"] * 10, "to-list")
    rec += walk(r, ["down", "up", "end", "home", "return"], "list")
    # The transcript: Tab to the current message, then Up, Down, Home, End, Page Up, Page Down,
    # Return on a fold, Cmd-A, Cmd-C.
    rec += walk(r, ["tab"] * 4 + ["up", "up", "down", "home", "end", "pageup", "pagedown", "return", "a:cmd", "c:cmd"], "transcript")
    r.save("walk.json", rec)
    print("pasteboard", gx(G, "pb")[:300])


def c_select(r):
    p = pid()
    t = tree(p)
    # The text runs on screen (a block's text is its AXStaticText children's values).
    paras = [n for n in t if n.get("AXRole") in ("AXStaticText", "AXHeading") and n.get("AXPosition") and n.get("AXSize")
             and n["AXPosition"][1] > 140 and n["AXPosition"][1] + n["AXSize"][1] < 770
             and isinstance(n.get("AXValue"), str) and len(n["AXValue"].strip()) > 3 and n["path"].startswith("0.0.8.")]
    res = []
    def copy(label, act):
        gx(G, "pbclear")
        act()
        time.sleep(0.3)
        key("c", "cmd")
        time.sleep(0.4)
        pb = json.loads(gx(G, "pb"))
        res.append({"how": label, "copied": pb.get("string")})
        print("select", label, repr((pb.get("string") or "")[:160]))
    if len(paras) >= 3:
        a, b = paras[0], paras[min(4, len(paras) - 1)]
        x1, y1 = a["AXPosition"][0] + 2, a["AXPosition"][1] + a["AXSize"][1] / 2
        x2, y2 = b["AXPosition"][0] + 60, b["AXPosition"][1] + b["AXSize"][1] / 2
        copy(f"drag {a.get('AXValue','')[:20]!r} -> {b.get('AXValue','')[:20]!r}", lambda: gx(G, "drag", str(x1), str(y1), str(x2), str(y2)))
        r.shot("drag")
        w = paras[1]
        wx, wy = w["AXPosition"][0] + 12, w["AXPosition"][1] + w["AXSize"][1] / 2
        copy("double click", lambda: gx(G, "dclick", str(wx), str(wy)))
        copy("triple click", lambda: gx(G, "tclick", str(wx), str(wy)))
    cell = first(t, AXRole="AXCell")
    if cell:
        cx, cy = cell["AXPosition"][0] + 4, cell["AXPosition"][1] + cell["AXSize"][1] / 2
        last = paras[-1]
        copy("drag from a table cell", lambda: gx(G, "drag", str(cx), str(cy), str(last["AXPosition"][0] + 80), str(last["AXPosition"][1] + 5)))
    else:
        res.append({"how": "drag from a table cell", "copied": "no table cell in the tree on this screen"})
        print("select: no table cell laid out")
    msg = [n for n in t if n.get("AXSubrole") == "AXDocumentArticle" and n["AXPosition"][1] > 140][-1]
    gx(G, "click", str(msg["AXPosition"][0] + msg["AXSize"][0] - 20), str(msg["AXPosition"][1] + 6))
    copy("Cmd-A in the transcript", lambda: key("a", "cmd"))
    r.save("select.json", res)


def c_ime(r):
    p = pid()
    c = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
    gx(G, "click", str(c["AXPosition"][0] + 40), str(c["AXPosition"][1] + 20))
    key("a", "cmd"); key("delete")
    gx(G, "input-select", "com.apple.inputmethod.Kotoeri.RomajiTyping.Japanese")
    time.sleep(0.5)
    gx(G, "type", "nihongo")
    time.sleep(0.6)
    pre = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
    r.shot("preedit")
    key("space")
    time.sleep(0.8)
    r.shot("candidates")
    key("return")
    time.sleep(0.5)
    done = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
    r.shot("committed")
    gx(G, "input-select", "com.apple.keylayout.US")
    out = {"preedit": {k: pre.get(k) for k in ("AXValue", "AXSelectedTextRange", "AXNumberOfCharacters")},
           "committed": {k: done.get(k) for k in ("AXValue", "AXSelectedTextRange", "AXNumberOfCharacters")},
           "log": r.log_lines()[-3:]}
    r.save("ime.json", out)
    key("a", "cmd"); key("delete")
    print("ime", out)


def c_cache(r):
    p = pid()
    strip = lambda t: [{k: v for k, v in d.items() if k not in ("t",)} for d in t]
    a = strip(tree(p))
    # Repaints that change nothing in the transcript: pointer moves over it, a hover on the sidebar.
    for x, y in [(600, 300), (700, 400), (100, 150), (600, 300)]:
        gx(G, "move", str(x), str(y)); time.sleep(0.3)
    b = strip(tree(p))
    same = a == b
    r.save("cache.json", {"identical": same, "first": len(a), "second": len(b),
                          "diff": [x for x in zip(a, b) if x[0] != x[1]][:10]})
    print("cache identical:", same, len(a), len(b))


def c_window(r):
    p = pid()
    res = {}
    def wins():
        return [{k: w.get(k) for k in ("AXTitle", "minimized", "fullscreen", "AXPosition", "AXSize")} for w in jl(gx(G, "win", p, "list"))]
    res["menus"] = [d.get("AXTitle") for d in jl(gx(G, "ax", p, "2")) if d.get("AXRole") == "AXMenuBarItem"]
    key("m", "cmd"); time.sleep(1.5); res["cmd-m"] = wins(); r.shot("cmd-m")
    gx(G, "win", p, "unminimize"); time.sleep(1.5); res["restored"] = wins()
    gx(G, "activate", p); time.sleep(0.4)
    key("f", "cmd,ctrl"); time.sleep(2.5); res["ctrl-cmd-f"] = wins(); r.shot("full-screen")
    key("f", "cmd,ctrl"); time.sleep(2.5); res["ctrl-cmd-f again"] = wins()
    key("w", "cmd"); time.sleep(1.0); res["cmd-w"] = {"windows": wins(), "alive": bool(sh("pgrep -x mcbench-ui-dev || true").strip())}
    r.shot("cmd-w")
    if res["cmd-w"]["alive"]:
        key("q", "cmd"); time.sleep(2)
    res["cmd-q alive after"] = bool(sh("pgrep -x mcbench-ui-dev || true").strip())
    res["DiagnosticReports"] = sh("ls ~/Library/Logs/DiagnosticReports | grep -i -E 'mcbench|ui-dev' || echo none").strip()
    res["crash logs"] = sh("ls /tmp/s4/logs 2>/dev/null || echo none").strip()
    r.save("window.json", res)
    for k, v in res.items():
        print("window", k, v)


def phrase():
    return sh("osascript -e 'tell application \"VoiceOver\" to get content of last phrase' 2>&1 || true", check=False).strip()


def vo_steps(r, steps, wait=0.6):
    rec = []
    for label, code, mods in steps:
        gx(G, "key", str(code), *([mods] if mods else []))
        time.sleep(wait)
        ph = phrase()
        # The traffic lights' pixels: they grey out when the window stops being key.
        lights = sh("screencapture -x -R86,60,56,16 /tmp/t.png && /sbin/md5 -q /tmp/t.png", check=False).strip()[:8]
        rec.append({"step": label, "phrase": ph, "lights": lights})
        print("vo", label, "|", lights, "|", ph[:160].replace("\n", " / "))
    return rec


def c_vo(r):
    p = pid()
    if not sh("pgrep -x VoiceOver || true").strip():
        key("f5", "cmd,fn")
        time.sleep(10)
    VO = "ctrl,alt"
    # The sidebar from the top: mcbuild, New chat, Collapse sidebar, Search
    # chats, Filter, then the Chats list.
    rec = vo_steps(r, [("VO-Home", 115, VO + ",fn")] + [("VO-Right", 124, VO)] * 5)
    # Into the chat list, through it, open Castle gate and come back.
    rec += vo_steps(r, [("VO-Shift-Down (interact)", 125, VO + ",shift"), ("VO-Right", 124, VO),
                        ("VO-Space (open)", 49, VO), ("VO-Left", 123, VO), ("VO-Space (open)", 49, VO),
                        ("VO-Shift-Up (stop interacting)", 126, VO + ",shift")])
    # Show archived, Settings, then the transcript: each kind of block.
    rec += vo_steps(r, [("VO-Right", 124, VO)] * 3 + [("VO-Shift-Down (interact)", 125, VO + ",shift")] +
                    [("VO-Right", 124, VO)] * 6)
    # Back past the first message laid out at launch (the chat opens at its end).
    rec += vo_steps(r, [("VO-Left", 123, VO)] * 10)
    rec += vo_steps(r, [("VO-Cmd-H next heading", 4, VO + ",cmd"), ("VO-Cmd-L next link", 37, VO + ",cmd"),
                        ("VO-Space on the link", 49, VO), ("VO-A read from here", 0, VO)], wait=1.5)
    sh("sleep 3")
    rec += vo_steps(r, [("Control (stop speech)", 59, "")])
    # Folds: find the Worked fold with VO-Right from the top of the transcript.
    t = tree(p)
    fold = first(t, AXRole="AXButton", AXTitle=lambda v: isinstance(v, str) and v.startswith("Worked"))
    if fold:
        gx(G, "act", p, fold["path"], "AXScrollToVisible", check=False)
    rec += vo_steps(r, [("VO-Shift-Up", 126, VO + ",shift")] + [("VO-Right", 124, VO)] * 6)
    # Composer, Send, Stop with real keys, out of the transcript first.
    rec += vo_steps(r, [("VO-Shift-Up", 126, VO + ",shift")] * 3)
    # The click lands on the composer's right end: VoiceOver's caption panel
    # covers its left part (about x 100 to 700, y 720 to 830) and a click
    # there makes the panel key.
    c = json.loads(gx(G, "node", p, "find:AXTextArea:Message"))
    gx(G, "click", str(c["AXPosition"][0] + c["AXSize"][0] - 40), str(c["AXPosition"][1] + 20))
    time.sleep(0.6)
    rec.append({"step": "click composer", "phrase": phrase()})
    gx(G, "type", "Add a torch")
    time.sleep(0.8)
    rec.append({"step": "typed 'Add a torch'", "phrase": phrase()})
    # Attach file and the three pickers come before Send message.
    rec += vo_steps(r, [("Tab (Attach file)", 48, ""), ("Tab (Resolution)", 48, ""), ("Tab (Chat model)", 48, ""),
                        ("Tab (Reasoning effort)", 48, ""), ("Tab (to Send message)", 48, ""),
                        ("VO-Space (Send message)", 49, VO)], wait=0.5)
    # Send puts focus back in the composer; Stop generating follows the pickers.
    rec += vo_steps(r, [("Tab (Attach file)", 48, ""), ("Tab (Resolution)", 48, ""), ("Tab (Chat model)", 48, ""),
                        ("Tab (Reasoning effort)", 48, ""), ("Tab (to Stop generating)", 48, ""),
                        ("VO-Space (Stop generating)", 49, VO)], wait=0.5)
    sh("sleep 2")
    rec.append({"step": "after Stop", "phrase": phrase()})
    rec.append({"step": "log", "phrase": json.dumps(r.log_lines()[-4:], ensure_ascii=False)})
    r.save("vo.json", rec)
    r.shot("vo-end")
    key("f5", "cmd,fn")


def settled(p, pick, tries=50):
    """The element pick() finds in the tree, once its bounds read the same
    twice 200 ms apart: the list has stopped scrolling."""
    last = None
    for _ in range(tries):
        n = pick(tree(p))
        b = (tuple(n["AXPosition"]), tuple(n["AXSize"])) if n else None
        if b is not None and b == last:
            return n
        last = b
        time.sleep(0.2)
    sys.exit("bounds never settled")


def table_top(p):
    return first(tree(p), AXRole="AXTable")["AXPosition"][1]


def c_cell(r):
    """Drag from the first table cell to below the table, with the frames read
    after the list has settled; the copy must hold the cells tab-separated and
    the text after the table."""
    p = pid()
    # The first chat is open at launch. Re-opening it here would scroll its current
    # message into view after the scrolls below, moving the table under the pointer.
    # Small scroll steps until the whole table sits between the header and the composer.
    time.sleep(2.0)
    steps = []

    def read():
        # The table's top and bottom, and where every message laid out sits:
        # the messages move with each scroll the app has taken, the table only
        # once it is laid out.
        nodes = tree(p)
        t = first(nodes, AXRole="AXTable")
        rows = tuple(n["AXPosition"][1] for n in nodes if n.get("AXSubrole") == "AXDocumentArticle")
        steps.append(t and [t["AXPosition"], t["AXSize"]])
        return (t and (t["AXPosition"][1], t["AXPosition"][1] + t["AXSize"][1])), rows

    def place():
        # Each scroll is waited for (the table appears or moves) before the next,
        # so no scrolls pile up while the app is behind.
        b, rows = read()
        for _ in range(40):
            if b and b[0] > 150 and b[1] + 40 < 740:
                return b[0]
            # Not laid out yet: the list shows the latest turn, below the table.
            gx(G, "scroll", "700", "400", "3" if b is None or b[0] <= 150 else "-3")
            for _ in range(120):
                time.sleep(0.5)
                now = read()
                if now[1] != rows:
                    break
            else:
                sys.exit("the list never answered a scroll")
            b, rows = now
        sys.exit("the table never came into view")

    # In the guest the app can fall behind the scroll events for many seconds
    # (frames and the tree stop while it catches up), and the tree then reads
    # the same twice while scrolls are still queued. One line up and back down
    # that lands where it started means no scroll was still queued.
    for _ in range(5):
        y = place()
        answered = 0
        for lines in ("1", "-1"):
            gx(G, "scroll", "700", "400", lines)
            for _ in range(60):
                time.sleep(0.5)
                now = table_top(p)
                if (lines == "1" and now != y) or (lines == "-1" and now == y):
                    answered += 1
                    break
            steps.append(["round trip", lines, now])
        if answered == 2 and table_top(p) == y:
            break
    else:
        sys.exit("the list never settled")
    cell = settled(p, lambda t: first(t, AXRole="AXCell"))
    table = first(tree(p), AXRole="AXTable")
    x1, y1 = cell["AXPosition"][0] + 4, cell["AXPosition"][1] + cell["AXSize"][1] / 2
    x2, y2 = table["AXPosition"][0] + 150, table["AXPosition"][1] + table["AXSize"][1] + 40
    r.shot("before-drag")
    gx(G, "pbclear")
    gx(G, "drag", str(x1), str(y1), str(x2), str(y2))
    time.sleep(0.3)
    key("c", "cmd")
    time.sleep(0.4)
    copied = json.loads(gx(G, "pb")).get("string")
    r.shot("cell-drag")
    after = first(tree(p), AXRole="AXTable")
    res = {"steps": steps, "table_after": after and after["AXPosition"], "focus_after": focus(p),
           "cell": [cell.get("AXValue"), cell["AXPosition"]], "from": [x1, y1], "to": [x2, y2], "copied": copied}
    r.save("cell.json", res)
    print("cell drag from", cell.get("AXValue"), (x1, y1), "to", (x2, y2), "copied", repr(copied))
    if not (copied or "").startswith("Block\tCount\tLight\nstone\t9\t0\n") or "A lamp only" not in copied:
        sys.exit("the copy is not the table's cells and the text after it")


def on_battery_stop():
    """After a check: if vm/powerwatch.sh found the Mac on battery, stop the
    guest (vm/down.sh) and end the run."""
    flag = os.path.join(OUT, "logs", f"{VM}-on-battery")
    if os.path.exists(flag):
        print("the Mac is on battery: stopping the guest after this check", open(flag).read(), flush=True)
        subprocess.run([os.path.join(os.path.dirname(os.path.abspath(__file__)), "vm", "down.sh")], check=False)
        sys.exit(2)


CHECKS = {"launch": c_launch, "tree": c_tree, "text": c_text, "actions": c_actions, "events": c_events,
          "keys": c_keys, "select": c_select, "cell": c_cell, "ime": c_ime, "cache": c_cache, "window": c_window, "vo": c_vo}

if __name__ == "__main__":
    run = sys.argv[1]
    for c in sys.argv[2:]:
        on_battery_stop()
        CHECKS[c](Run(run, c))
        on_battery_stop()
