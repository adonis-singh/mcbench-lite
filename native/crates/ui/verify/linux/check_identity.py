"""The window's identity, which a desktop entry ties to its launcher and
icon: `xprop WM_CLASS` on X11 reads "mcbench-lite", "mcbench-lite" and the
Wayland `app_id` (through the private cage's foreign-toplevel manager)
reads "mcbench-lite". Also the renderer line the app logs at start
(`--log`): one line, naming the backend, the adapter, the driver and
whether it is a software adapter.

Run with the gate fixture (run.sh)."""

import os
import subprocess
import sys
import time

import common

report = common.Report("identity")
common.app()
time.sleep(1.0)
if common.BACKEND == "x11":
    from Xlib import display

    d = display.Display(os.environ["DISPLAY"])

    def windows(window):
        yield window
        for child in window.query_tree().children:
            yield from windows(child)

    targets = [w for w in windows(d.screen().root) if (w.get_wm_name() or "").endswith("mcbench-lite")]
    report.check("one window named mcbench-lite", len(targets) == 1, f"{len(targets)} found")
    for target in targets:
        line = subprocess.run(["xprop", "-id", hex(target.id), "WM_CLASS"], capture_output=True, text=True,
                              check=True).stdout.strip()
        report.check("xprop WM_CLASS", line == 'WM_CLASS(STRING) = "mcbench-lite", "mcbench-lite"', line)
else:
    lines = subprocess.run([sys.executable, os.path.join(os.path.dirname(os.path.abspath(__file__)), "wl_pointer.py"),
                            "identity"], capture_output=True, text=True, check=True).stdout.splitlines()
    report.check("one toplevel", len(lines) == 1, "; ".join(lines))
    for line in lines:
        report.check("the toplevel's app_id", line.startswith("app_id='mcbench-lite' "), line)
renderers = [a for a in common.actions() if a["action"] == "renderer"]
report.check("one renderer line in the log", len(renderers) == 1, str(renderers))
for entry in renderers:
    # wgpu's GL backend leaves the driver's name empty and names it in the
    # driver information ("4.6 (Core Profile) Mesa ...").
    report.check("it names the backend, adapter, driver and software",
                 all(isinstance(entry.get(k), str) and entry[k] for k in ("backend", "adapter"))
                 and (entry.get("driver") or entry.get("driver_info"))
                 and isinstance(entry.get("software"), bool), str(entry))
report.finish()
