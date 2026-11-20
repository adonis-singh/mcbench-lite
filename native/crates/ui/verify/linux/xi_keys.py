"""Every key press and release the X server sees on the private display
(XInput2 raw events on the root window), one line each with the time and
the keycode, until killed (pane agent: whether a key's release reached
the server, beside what the window logged).

    xi_keys.py <out file>"""

import sys
import time

from Xlib import display
from Xlib.ext import xinput

d = display.Display()
d.xinput_query_version()
root = d.screen().root
root.xinput_select_events([(xinput.AllMasterDevices, xinput.RawKeyPressMask | xinput.RawKeyReleaseMask)])
d.flush()
with open(sys.argv[1], "w", buffering=1) as out:
    while True:
        e = d.next_event()
        kind = {xinput.RawKeyPress: "down", xinput.RawKeyRelease: "up"}.get(getattr(e, "evtype", None))
        if kind:
            out.write(f"{time.monotonic():.3f} {kind} {e.data.detail}\n")
