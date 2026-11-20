"""Helpers for the shell and chat milestone's checks on macOS (`checks.py`), in the guest VM: every
row ACCESSIBILITY.md added after the gate, through AX (role, title, value,
state, actions, bounds), real keys and pointer (guestctl's HID events) and,
where it says what a person hears, VoiceOver's phrases. Each part launches
`mcbench-ui-dev` with its own flags, writes $OUT/evidence/<run>/<part>/
(checks.json: one record per check, with what was read; shots), and prints
one line per check. A failed check does not stop the part.

Environment: what vm/env.sh needs (RIG_HOME, OUT, VM)."""
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gate  # noqa: E402
from gate import G, APP, gx, sh, jl  # noqa: E402

OUT = gate.OUT
K = dict(gate.K, k=40, b=11, o=31, one=18, two=19, lbracket=33, rbracket=30, v=9, x=7, p=35, comma=43, g=5,
         t=17, s=1, d=2, e=14, n=45, r=15, slash=44, period=47)
VO = "ctrl,alt"
SCRATCH = "/tmp/s4"


class Part:
    def __init__(self, run, part):
        self.dir = os.path.join(OUT, "evidence", run, part)
        os.makedirs(self.dir, exist_ok=True)
        self.name = part
        self.records = []
        self.n = 0

    def check(self, what, ok, seen=None):
        rec = {"check": what, "ok": bool(ok), "seen": seen}
        self.records.append(rec)
        print(("PASS " if ok else "FAIL ") + self.name + ": " + what + ("" if seen is None else " | " + str(seen)[:300]),
              flush=True)
        return ok

    def note(self, what, seen):
        self.records.append({"note": what, "seen": seen})
        print("NOTE " + self.name + ": " + what + " | " + str(seen)[:400], flush=True)

    def shot(self, label, region=None):
        self.n += 1
        path = os.path.join(self.dir, f"{self.n:02d}-{label}.png")
        r = f"-R{region[0]},{region[1]},{region[2]},{region[3]} " if region else ""
        with open(path, "wb") as f:
            f.write(gate.subprocess.run([gate.TART, "exec", gate.VM, "sh", "-c", f"screencapture -x {r}/tmp/p.png && cat /tmp/p.png"],
                                        capture_output=True, env=gate.ENV).stdout)
        return path

    def save(self):
        with open(os.path.join(self.dir, "checks.json"), "w") as f:
            json.dump(self.records, f, ensure_ascii=False, indent=1)
        failed = [r["check"] for r in self.records if r.get("ok") is False]
        print(f"== {self.name}: {sum(1 for r in self.records if r.get('ok'))} passed, {len(failed)} failed", flush=True)
        return failed


# The app ---------------------------------------------------------------

def launch(args="", wait=5.0, activate=True, min_nodes=20, wait_tree=True):
    sh("pkill -x mcbench-ui-dev; sleep 0.5", check=False)
    gx(G, "input-select", "com.apple.keylayout.US")
    sh(f"rm -rf {SCRATCH} /tmp/ui.log; mkdir -p {SCRATCH}", check=False)
    env = os.environ.get("APP_ENV", "")
    sh(f"cd /tmp; {env} {APP} --scratch {SCRATCH} --log /tmp/ui.log {args} > /tmp/ui.out 2>&1 &")
    time.sleep(wait)
    p = gate.pid()
    if activate:
        win = [w for w in jl(gx(G, "windows")) if w.get("pid") == int(p)]
        b = win[0]["b"]
        gx(G, "click", str(b[0] + 100), str(b[1] + b[3] // 2))
        time.sleep(0.6)
    # The first AX question turns AccessKit on: the window's content
    # appears in the tree a moment after it.
    if not wait_tree:
        return p
    t0 = time.time()
    if not wait_for(lambda: len(tree(p)) > min_nodes, 120, 0.5):
        raise RuntimeError("the window's content never appeared in the AX tree within 120 s")
    LAUNCHES.append({"args": args, "content_in_ax_after_s": round(time.time() - t0 + wait + (0.6 if activate else 0), 1)})
    print("launch", args, "content in AX after about", LAUNCHES[-1]["content_in_ax_after_s"], "s", flush=True)
    return p


LAUNCHES = []


def log():
    return [json.loads(l) for l in sh("cat /tmp/ui.log 2>/dev/null || true").splitlines() if l.strip()]


def logged(action, **fields):
    return any(a.get("action") == action and all(a.get(k) == v for k, v in fields.items()) for a in log())


def tree(p):
    """Every element of the window (the menu bar left out), with every
    attribute `describe` gives and the element's path."""
    out = []
    for d in jl(gx(G, "ax", p)):
        if d.get("path", "").startswith("1") and d.get("AXRole", "").startswith("AXMenu"):
            continue
        if d.get("AXRole") in ("AXMenuBar", "AXMenuBarItem"):
            continue
        out.append(d)
    return out


def find(t, role=None, title=None, prefix=None, sub=None, pred=None):
    out = []
    for n in t:
        if role and n.get("AXRole") != role:
            continue
        if sub and n.get("AXSubrole") != sub:
            continue
        name = n.get("AXTitle") or n.get("AXDescription") or ""
        if title is not None and name != title:
            continue
        if prefix is not None and not name.startswith(prefix):
            continue
        if pred and not pred(n):
            continue
        out.append(n)
    return out


def one(t, **kw):
    f = find(t, **kw)
    return f[0] if f else None


def name(n):
    return None if n is None else (n.get("AXTitle") or n.get("AXDescription"))


def node(p, path):
    r = gx(G, "node", p, path, check=False)
    return json.loads(r) if r.strip().startswith("{") else {}


def describe_all(p, path):
    """Every attribute, settable flag, parameterised attribute and action of one element."""
    for d in jl(gx(G, "axall", p)):
        if d.get("_path") == path:
            return d
    return None


def summary(n, keys=("path", "AXRole", "AXSubrole", "AXRoleDescription", "AXTitle", "AXDescription", "AXValue",
                     "AXHelp", "AXEnabled", "AXFocused", "AXExpanded", "AXSelected", "AXPosition", "AXSize", "actions")):
    if not n:
        return None
    return {k: n.get(k) for k in keys if n.get(k) is not None}


def focused(p):
    r = gx(G, "focused", p, check=False)
    return json.loads(r) if r.strip().startswith("{") else {}


def key(k, mods=""):
    gx(G, "key", str(K[k]), *([mods] if mods else []))


def keys(*seq, settle=0.4):
    for k in seq:
        name_, _, mods = k.partition(":")
        key(name_, mods)
        time.sleep(settle)


def typ(text, settle=0.4):
    gx(G, "type", text)
    time.sleep(settle)


def visible(p, n):
    """Brings a transcript node that lies outside the list's visible part on
    screen: AXPress on such a node does nothing, AXScrollToVisible is not
    offered, and focusing a part of a row does not scroll; focusing the
    row's article does (the contract's rule). Returns the node as it is now."""
    win = [w for w in jl(gx(G, "windows")) if w.get("pid") == int(p)][0]["b"]
    y = n["AXPosition"][1]
    if win[1] + 60 < y < win[1] + win[3] - 120:
        return n
    parts = n["path"].split(".")
    for k in range(len(parts), 0, -1):
        a = node(p, ".".join(parts[:k]))
        if a.get("AXSubrole") == "AXDocumentArticle":
            setfocus(p, {"path": ".".join(parts[:k])}, settle=1.5)
            want = name(n)
            for m_ in tree(p):
                if name(m_) == want and m_.get("AXRole") == n.get("AXRole"):
                    return m_
    return n


def press(p, n, settle=0.6):
    n = visible(p, n) if n["path"].startswith("0.0.8.") else n
    r = json.loads(gx(G, "act", p, n["path"], "AXPress", check=False) or "{}")
    time.sleep(settle)
    return r.get("AXError")


def click(n, settle=0.6, dx=None, dy=None):
    x = n["AXPosition"][0] + (dx if dx is not None else n["AXSize"][0] / 2)
    y = n["AXPosition"][1] + (dy if dy is not None else n["AXSize"][1] / 2)
    gx(G, "click", str(x), str(y))
    time.sleep(settle)


def setfocus(p, n, settle=0.4):
    gx(G, "set", p, n["path"], "AXFocused", "true", check=False)
    time.sleep(settle)


def fname(p):
    f = focused(p)
    return f.get("AXRole"), name(f)


def composer(p):
    return node(p, "find:AXTextArea:Message")


def wait_for(fn, timeout, step=0.4):
    end = time.time() + timeout
    while time.time() < end:
        v = fn()
        if v:
            return v
        time.sleep(step)
    return None


def title(p):
    w = [w for w in jl(gx(G, "windows")) if w.get("pid") == int(p)]
    return w[0].get("name") if w else None


# VoiceOver ---------------------------------------------------------------

def vo_on():
    if not sh("pgrep -x VoiceOver || true").strip():
        gx(G, "key", "96", "cmd,fn")
        time.sleep(10)


def vo_off():
    if sh("pgrep -x VoiceOver || true").strip():
        gx(G, "key", "96", "cmd,fn")
        time.sleep(2)


def phrase():
    return gate.phrase()


def vo(part, label, k, mods="", wait=0.6):
    """A key (VoiceOver's or the app's), then the last phrase VoiceOver spoke."""
    gx(G, "key", str(K[k] if k in K else k), *([mods] if mods else []))
    time.sleep(wait)
    ph = phrase()
    part.note(f"VO {label}", ph)
    return ph


def vo_watch(seconds, step=0.5):
    """The distinct phrases VoiceOver speaks over `seconds`, polled."""
    seen = []
    end = time.time() + seconds
    while time.time() < end:
        ph = phrase()
        if not seen or seen[-1] != ph:
            seen.append(ph)
        time.sleep(step)
    return seen


PARTS = {}


def part(fn):
    PARTS[fn.__name__.replace("p_", "").replace("_", "-")] = fn
    return fn
