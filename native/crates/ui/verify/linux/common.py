"""Helpers the Linux checks share: the app on the AT-SPI bus, screenshots,
real key and pointer events on the private display, its clipboard, and the
action log. Everything works on the display and bus the check runs in
(run.sh sets them); MCBENCH_BACKEND is x11 or wayland."""

import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import atspi_tree  # noqa: E402
from atspi_tree import Atspi  # noqa: E402,F401

OUT = os.environ["MCBENCH_OUT"]
BACKEND = os.environ["MCBENCH_BACKEND"]


def out(name):
    return os.path.join(OUT, name)


def app(wait=20.0):
    return atspi_tree.find_app(wait)


def focus_window():
    """Gives the app's window the keyboard, as a window manager would on a
    click (X11 has no window manager here; cage focuses its one client)."""
    if BACKEND == "x11":
        subprocess.run(["xdotool", "search", "--sync", "--name", "mcbench-lite$", "windowfocus", "--sync"], check=True)
    else:
        # The first key of a new virtual keyboard reaches cage before the
        # window has keyboard focus and is lost: send a bare Shift first.
        subprocess.run(["wtype", "-M", "shift", "-m", "shift"], check=True)
    time.sleep(0.3)


def deactivate_window():
    """Takes the keyboard from the app's window, as another window taking
    focus would: on X11 the input focus goes to no window; on Wayland a
    second client (foot, started here) opens and cage activates it. Returns
    the process to stop, or None."""
    if BACKEND == "x11":
        from Xlib import X, display
        d = display.Display()
        d.set_input_focus(X.NONE, X.RevertToNone, X.CurrentTime)
        d.sync()
        time.sleep(0.5)
        return None
    other = subprocess.Popen(["foot", "sleep", "600"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(2.0)
    return other


def find(root, role=None, name=None, pred=None):
    """Every descendant of `root` with this role name and name."""
    found = []

    def visit(obj):
        if obj is None:
            return
        if (role is None or obj.get_role_name() == role) and (name is None or obj.get_name() == name) and (
            pred is None or pred(obj)
        ):
            found.append(obj)
        for i in range(obj.get_child_count()):
            visit(obj.get_child_at_index(i))

    visit(root)
    return found


def one(root, role=None, name=None, pred=None):
    found = find(root, role, name, pred)
    if len(found) != 1:
        raise AssertionError(f"expected one {role} {name!r}, found {len(found)}")
    return found[0]


# sidebar agent: a list row by its title (its name adds its status, its
# resolution and its search snippet after a comma).
def row(root, title):
    """The one list item titled `title`."""
    return one(root, "list item", pred=lambda o: o.get_name() == title or o.get_name().startswith(title + ", "))


def focused(root):
    found = find(root, pred=lambda o: o.get_state_set().contains(Atspi.StateType.FOCUSED))
    return found[0] if found else None


def label(obj):
    return f"{obj.get_role_name()} {obj.get_name()!r}" if obj is not None else "nothing"


def screenshot(name):
    path = out(name)
    if BACKEND == "x11":
        subprocess.run(["import", "-window", "root", "-display", os.environ["DISPLAY"], path], check=True)
    else:
        subprocess.run(["grim", path], check=True)
    return path


# xdotool key names for the keys the checks press.
X_KEYS = {"Tab": "Tab", "shift+Tab": "shift+Tab", "Left": "Left", "h": "h", "shift+h": "shift+h", "k": "k", "l": "l", "t": "t", "KP_Enter": "KP_Enter", "BackSpace": "BackSpace", "Delete": "Delete", "Right": "Right", "shift+Left": "shift+Left", "Up": "Up", "Down": "Down", "Home": "Home", "End": "End",
          "Page_Up": "Prior", "Page_Down": "Next", "Return": "Return", "space": "space", "Escape": "Escape",
          "shift+Return": "shift+Return", "shift+Tab": "shift+Tab", "Left": "Left", "h": "h", "shift+h": "shift+h", "k": "k", "l": "l", "t": "t", "KP_Enter": "KP_Enter", "BackSpace": "BackSpace", "Delete": "Delete", "Right": "Right", "shift+Left": "shift+Left", "ctrl+a": "ctrl+a", "ctrl+c": "ctrl+c", "ctrl+q": "ctrl+q",
          "ctrl+b": "ctrl+b", "alt+Left": "alt+Left", "alt+Right": "alt+Right"}

# bench agent: the mode keys and the palette's.
X_KEYS.update({"ctrl+1": "ctrl+1", "ctrl+2": "ctrl+2", "slash": "slash", "comma": "comma", "alt+Down": "alt+Down"})


def key(name, settle=0.25):
    """A real key press and release on the private display."""
    if BACKEND == "x11":
        subprocess.run(["xdotool", "key", "--clearmodifiers", X_KEYS[name]], check=True)
    else:
        mods = name.split("+")[:-1]
        base = name.split("+")[-1]
        args = ["wtype"]
        for m in mods:
            args += ["-M", m]
        args += ["-k", base]
        for m in reversed(mods):
            args += ["-m", m]
        subprocess.run(args, check=True)
    time.sleep(settle)


def type_text(text, settle=0.3):
    if BACKEND == "x11":
        subprocess.run(["xdotool", "type", "--delay", "20", text], check=True)
    else:
        subprocess.run(["wtype", "-d", "20", text], check=True)
    time.sleep(settle)


_wl_pointer = None


def pointer(*actions):
    """Real pointer events: XTest through xdotool on X11; on Wayland a
    virtual pointer on the private cage (wl_pointer.py), one for the whole
    check so a held button stays held."""
    global _wl_pointer
    if BACKEND == "x11":
        subprocess.run(["xdotool", *actions], check=True)
        return
    if _wl_pointer is None:
        _wl_pointer = subprocess.Popen(
            [sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "wl_pointer.py"), "serve"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True,
        )
        # pane agent: the private compositor's seat has no pointer until
        # this virtual one exists; the app binds its wl_pointer only after
        # the seat says so, and what the pointer did before that is lost
        # (the first drag of a check never reached the window). One move
        # and a moment first, as a desktop's pointer is there all along.
        _wl_pointer.stdin.write("mousemove 1 1\n")
        _wl_pointer.stdin.flush()
        if _wl_pointer.stdout.readline().strip() != "ok":
            raise RuntimeError("wl_pointer.py failed on its first move")
        time.sleep(0.5)
    _wl_pointer.stdin.write(" ".join(actions) + "\n")
    _wl_pointer.stdin.flush()
    if _wl_pointer.stdout.readline().strip() != "ok":
        raise RuntimeError(f"wl_pointer.py failed on {actions}")


def close_window():
    """Asks the window to close, as a window manager's close does:
    WM_DELETE_WINDOW sent with python-xlib on X11, the foreign-toplevel
    close on Wayland. Returns how many app windows were found (and asked)."""
    if BACKEND == "x11":
        from Xlib import X, display, protocol

        d = display.Display(os.environ["DISPLAY"])
        wm_protocols = d.intern_atom("WM_PROTOCOLS")
        wm_delete = d.intern_atom("WM_DELETE_WINDOW")

        def windows(window):
            yield window
            for child in window.query_tree().children:
                yield from windows(child)

        targets = [w for w in windows(d.screen().root) if (w.get_wm_name() or "").endswith("mcbench-lite")]
        for target in targets:
            target.send_event(protocol.event.ClientMessage(
                window=target, client_type=wm_protocols, data=(32, [wm_delete, X.CurrentTime, 0, 0, 0])
            ))
        d.flush()
        return len(targets)
    subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "wl_pointer.py"), "close"], check=True)
    return 1


# seen: the window hidden and shown again, as minimising and restoring do.
def set_window_mapped(mapped):
    """X11 only: unmaps (hides) or maps (shows) the app's window with
    python-xlib, as a window manager minimising and restoring it does (no
    window manager runs here). GPUI reports an unmapped window hidden."""
    from Xlib import X, display

    d = display.Display(os.environ["DISPLAY"])

    def windows(window):
        yield window
        for child in window.query_tree().children:
            yield from windows(child)

    targets = [w for w in windows(d.screen().root) if (w.get_wm_name() or "").endswith("mcbench-lite")]
    for target in targets:
        if mapped:
            target.map()
        else:
            target.unmap()
    d.sync()
    time.sleep(0.8)
    return len(targets)


def clipboard():
    if BACKEND == "x11":
        return x11_clipboard()
    return subprocess.run(["wl-paste", "-n"], capture_output=True, text=True, check=True).stdout


def x11_clipboard(timeout=3.0):
    """The CLIPBOARD selection as UTF-8, read with python-xlib."""
    from Xlib import X, Xatom, display

    d = display.Display(os.environ["DISPLAY"])
    window = d.screen().root.create_window(0, 0, 1, 1, 0, X.CopyFromParent)
    clip = d.intern_atom("CLIPBOARD")
    utf8 = d.intern_atom("UTF8_STRING")
    prop = d.intern_atom("MCBENCH_CLIP")
    window.convert_selection(clip, utf8, prop, X.CurrentTime)
    deadline = time.time() + timeout
    while time.time() < deadline:
        while d.pending_events():
            event = d.next_event()
            if event.type == X.SelectionNotify:
                if event.property == X.NONE:
                    return None
                data = window.get_full_property(prop, X.AnyPropertyType)
                return data.value.decode("utf-8") if data else None
        time.sleep(0.05)
    return None


def actions():
    path = out("actions.jsonl")
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [json.loads(line) for line in f if line.strip()]


def wait_for(predicate, timeout=10.0, what="a condition"):
    deadline = time.time() + timeout
    while time.time() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.1)
    raise AssertionError(f"timed out waiting for {what}")


class Report:
    """Collects check results; writes <name>.json and fails at the end if any failed."""

    def __init__(self, name):
        self.name = name
        self.rows = []

    def check(self, what, ok, detail=""):
        self.rows.append({"check": what, "ok": bool(ok), "detail": detail})
        print(("ok      " if ok else "FAILED  ") + what + (f": {detail}" if detail else ""), flush=True)

    def finish(self):
        with open(out(self.name + ".json"), "w") as f:
            json.dump(self.rows, f, indent=1, ensure_ascii=False)
        failed = [r for r in self.rows if not r["ok"]]
        print(f"{len(self.rows) - len(failed)} of {len(self.rows)} passed", flush=True)
        sys.exit(1 if failed else 0)


# quit-ask: the quit key at a check's end, answering the quit question.
def quit_asked():
    """How many times the app asked before a quit (the log's `quit asked`)."""
    return sum(1 for line in actions() if line["action"] == "quit asked")


def app_exited():
    """Whether the app's process (MCBENCH_APP_PID) has exited."""
    try:
        with open(f"/proc/{os.environ['MCBENCH_APP_PID']}/stat") as f:
            text = f.read()
    except (FileNotFoundError, ProcessLookupError):
        return True
    return text[text.rindex(")") + 2:].split()[0] == "Z"


def quit_key(settle=1.5):
    """The quit key (Ctrl+Q); when a build runs and the app asks first
    (the log's `quit asked`), `Quit` by keyboard (focus opens on `Keep
    building`: Tab, Return). Then `settle` seconds."""
    asked = quit_asked()
    key("ctrl+q", settle=0)
    deadline = time.time() + 2.0
    while time.time() < deadline and not app_exited():
        if quit_asked() > asked:
            time.sleep(0.3)
            key("Tab")
            key("Return", settle=0)
            break
        time.sleep(0.05)
    time.sleep(settle)
# end of quit-ask's block
