"""Summarises host probe records (hoststate JSON): the frontmost app, the
menu bar owner, displays, windows owned by tart or the Virtualization
service, and each tart process's activation policy. With two files it also
prints what changed between them. Exit 1 if tart owns a window on screen,
is frontmost, or a tart process is not 'prohibited'."""
import json, sys

VM_OWNERS = {"tart", "Virtualization", "com.apple.Virtualization.VirtualMachine"}

def load(p):
    return json.load(open(p))

def summary(d):
    vm_windows = [w for w in d["windows"] if w["owner"] in VM_OWNERS and w["onscreen"]]
    return {
        "frontmost": d["frontmost"]["name"],
        "menuBarOwner": d["menuBarOwner"],
        "displays": [(x["id"], x["bounds"], x["mode"][:4]) for x in d["displays"]],
        "vm_windows_onscreen": vm_windows,
        "tart": [(a["pid"], a["policy"], a.get("active")) for a in d["apps"]],
        "windows_onscreen": sum(1 for w in d["windows"] if w["onscreen"]),
    }

bad = False
docs = [load(p) for p in sys.argv[1:]]
for p, d in zip(sys.argv[1:], docs):
    s = summary(d)
    print(p.rsplit("/", 1)[-1], json.dumps(s))
    if s["vm_windows_onscreen"] or s["frontmost"] in VM_OWNERS or any(pol != "prohibited" for _, pol, _ in s["tart"]):
        bad = True
if len(docs) == 2:
    a, b = summary(docs[0]), summary(docs[1])
    for k in ("frontmost", "menuBarOwner", "displays"):
        if a[k] != b[k]:
            print(f"changed {k}: {a[k]} -> {b[k]}")
print("PROBLEMS" if bad else "ok: no tart window on screen, tart never frontmost, every tart process prohibited")
sys.exit(1 if bad else 0)
