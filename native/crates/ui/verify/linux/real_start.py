"""The wait for the first real frame, part by part (pane agent;
real_pane.sh with MCBENCH_REAL_DRIVER=real_start.py), X11. From the
driver's start, every 0.25 s: the cover's text over the pane, a hash of
the pane's pixels (the working dots moving change it), and whether
app.log holds the `[renderer]` line yet (its wall time; the app prints it
untimed). Every half second for 6 s a key is typed into the composer;
the window's metrics (written at quit) say how long each key took to
paint and the window's frame times show any stall. Once the pane shows
the world: 2 s more, then Ctrl+Q. Writes start.json; summarize with the
app's actions.jsonl (`live show`, `live pane`, `live state`, `pane frame`
carry `t_ms` since the process started; `first_frame` carries both clocks)."""

import hashlib
import json
import os
import subprocess
import time

import common

Atspi = common.Atspi
report = common.Report("real-start")
PANE = "Live Minecraft world"
COVERS = ("Preparing", "Placing", "The world shows", "Loading", "Starting", "No graphics")
seen = {"samples": [], "keys": [], "renderer_line_epoch_ms": None}
t0 = time.time()


def app():
    return common.app(60.0)


def covers():
    try:
        return sorted({n.get_name() for n in common.find(app()) if (n.get_name() or "").startswith(COVERS)})
    except Exception as e:  # the tree changes under the walk
        return [f"(walk failed: {e})"]


def pane_hash():
    from PIL import Image
    found = common.find(app(), name=PANE)
    if not found:
        return None
    e = found[0].get_extents(Atspi.CoordType.WINDOW)
    path = common.screenshot("tmp-start.png")
    crop = Image.open(path).convert("RGB").crop((e.x + 2, e.y + 2, e.x + e.width, e.y + e.height))
    os.remove(path)
    return hashlib.sha1(crop.tobytes()).hexdigest()[:10]


def renderer_line():
    try:
        with open(common.out("app.log")) as f:
            return any(line.startswith("[renderer]") for line in f)
    except OSError:
        return False


def shown():
    return bool(common.find(app(), name="Live world"))


def type_key(letter):
    composer = common.find(app(), role="text")
    if composer:
        Atspi.Component.grab_focus(composer[-1])
        time.sleep(0.05)
    subprocess.run(["xdotool", "type", "--delay", "0", letter], check=True)
    seen["keys"].append({"at_s": round(time.time() - t0, 2), "letter": letter, "epoch_ms": time.time() * 1e3})


# MCBENCH_KEY_EVERY (seconds): from the driver's first moment, before the
# accessibility bus is up, a key to the composer (which has the focus at
# start) and a pointer move over the sidebar at that interval, through
# xdotool alone, until the world shows; a picture every half second.
every = float(os.environ.get("MCBENCH_KEY_EVERY") or "0")
if every:
    n = 0
    while time.time() - t0 < 12.0:
        n += 1
        subprocess.run(["xdotool", "type", "--delay", "0", "k"], check=False)
        subprocess.run(["xdotool", "mousemove", str(60 + 40 * (n % 3)), str(200 + 20 * (n % 5))], check=False)
        seen["keys"].append({"at_s": round(time.time() - t0, 3), "letter": "k", "epoch_ms": time.time() * 1e3})
        if n % 5 == 0:
            common.screenshot(f"cold-{n:03}.png")
        if n % 5 == 0 and renderer_line() and seen["renderer_line_epoch_ms"] is None:
            seen["renderer_line_epoch_ms"] = time.time() * 1e3
        try:
            with open(common.out("actions.jsonl")) as f:
                if any('"pane frame"' in line for line in f):
                    break
        except OSError:
            pass
        time.sleep(every)
app()
common.focus_window()
# A key every half second from the start: one lands in any stall.
keys_at = [] if every else [0.5 * i for i in range(1, 13)]
done_at = None
while time.time() - t0 < 120.0:
    now = time.time() - t0
    if keys_at and now >= keys_at[0]:
        keys_at.pop(0)
        type_key("k")
    if seen["renderer_line_epoch_ms"] is None and renderer_line():
        seen["renderer_line_epoch_ms"] = time.time() * 1e3
    is_shown = shown()
    seen["samples"].append({"t_s": round(now, 2), "epoch_ms": time.time() * 1e3, "covers": covers(),
                            "pane_hash": pane_hash(), "shown": is_shown})
    if is_shown and done_at is None:
        done_at = now
        common.screenshot("start-shown.png")
    if done_at is not None and now - done_at > 2.0:
        break
    if len(seen["samples"]) in (4, 20, 40):
        common.screenshot(f"start-{len(seen['samples']):03}.png")
    time.sleep(0.25)
report.check("the world was shown", done_at is not None, f"{done_at}")
with open(common.out("start.json"), "w") as f:
    json.dump(seen, f, indent=1)
common.quit_key(settle=3.0)
report.finish()
