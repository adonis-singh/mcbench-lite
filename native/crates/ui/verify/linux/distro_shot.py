"""A distros.sh run's screenshot and verdict.

    distro_shot.py <run dir> <state>
        Xvfb's frame buffer (<run dir>/screen.xwd: XWD, ZPixmap, 32 bits a
        pixel) to <run dir>/screen.png, then the checks: the window is not
        all black, and the empty chat's heading and the sidebar's chat rows
        hold text. A box holds text when at least INK of its pixels differ
        from the box's most common colour (its background) by more than 60
        in a channel: an all-black or all-background box has none. The last
        line is PASS or FAIL with the reasons. For the noadapter state the
        verdict is about the exit instead: the app ended with a nonzero
        status, a sentence on stderr and no crash log.

    distro_shot.py --row <system> <state> <run dir>
        One line of distros.sh's summary table.

The boxes are the gate fixture's window at 1280x820 (--origin 0,0) on
the new chat (--open /new): its heading ("What should we build?", between
the mark above and the line under it), and the sidebar's three chat rows. Standard library only."""

import json
import os
import struct
import sys
import zlib
from collections import Counter

# (x0, y0, x1, y1) in screen pixels.
HEADING = (300, 372, 710, 414)
ROWS = (12, 100, 230, 205)
INK = 120


def read_xwd(path):
    data = open(path, "rb").read()
    fields = struct.unpack(">25I", data[:100])
    header_size, _, pixmap_format, _, width, height = fields[:6]
    byte_order, bits_per_pixel, bytes_per_line = fields[7], fields[11], fields[12]
    red_mask, green_mask, blue_mask = fields[14:17]
    ncolors = fields[19]
    if pixmap_format != 2 or bits_per_pixel != 32:
        sys.exit(f"{path}: format {pixmap_format} at {bits_per_pixel} bits a pixel, not ZPixmap at 32")
    start = header_size + ncolors * 12
    order = ">" if byte_order else "<"

    def shift(mask):
        return (mask & -mask).bit_length() - 1

    rs, gs, bs = shift(red_mask), shift(green_mask), shift(blue_mask)
    rows = []
    for y in range(height):
        line = data[start + y * bytes_per_line:start + y * bytes_per_line + width * 4]
        rows.append([((p >> rs) & 255, (p >> gs) & 255, (p >> bs) & 255)
                     for p in struct.unpack(f"{order}{width}I", line)])
    return width, height, rows


def write_png(path, width, height, rows):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    raw = b"".join(b"\0" + bytes(c for pixel in row for c in pixel) for row in rows)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")
    open(path, "wb").write(png)


def ink(rows, box):
    x0, y0, x1, y1 = box
    pixels = [rows[y][x] for y in range(y0, y1) for x in range(x0, x1)]
    background, _ = Counter(pixels).most_common(1)[0]
    count = sum(1 for p in pixels if max(abs(a - b) for a, b in zip(p, background)) > 60)
    return background, count


def actions(run):
    try:
        return [json.loads(line) for line in open(os.path.join(run, "actions.jsonl"))]
    except FileNotFoundError:
        return []


def renderer(run):
    return next((a for a in actions(run) if a.get("action") == "renderer"), None)


def text_of(run, name):
    try:
        return open(os.path.join(run, name)).read().strip()
    except FileNotFoundError:
        return ""


def verdict(run, state):
    result = text_of(run, "result.txt")
    reasons = []
    if state == "noadapter":
        app_log = text_of(run, "app.log")
        if "status 0" in result or "up: yes" in result:
            reasons.append(f"the app did not end with a nonzero status ({result.splitlines()[0] if result else 'no result'})")
        if not app_log:
            reasons.append("nothing on stderr")
        if "panicked" in app_log or "crash log:" in result:
            reasons.append("a crash")
        print(f"result: {result}")
        print(f"stderr: {app_log}")
        return reasons
    if not result.startswith("up: yes"):
        reasons.append(f"the app is not up: {result}")
    if not os.path.exists(os.path.join(run, "screen.xwd")):
        reasons.append("no screenshot")
        return reasons
    width, height, rows = read_xwd(os.path.join(run, "screen.xwd"))
    write_png(os.path.join(run, "screen.png"), width, height, rows)
    os.remove(os.path.join(run, "screen.xwd"))
    black = sum(1 for row in rows for p in row if p == (0, 0, 0))
    print(f"screen {width}x{height}, {black} of {width * height} pixels black")
    if black > width * height * 0.9:
        reasons.append("the screen is black")
    for name, box in (("heading", HEADING), ("sidebar rows", ROWS)):
        background, count = ink(rows, box)
        print(f"{name} {box}: background {background}, {count} pixels of text")
        if count < INK:
            reasons.append(f"no text in the {name} (background {background}, {count} pixels)")
    return reasons


def main():
    if sys.argv[1] == "--row":
        system, state, run = sys.argv[2:5]
        line = text_of(run, "check.txt").splitlines()
        passed = "PASS" if line and line[-1].startswith("PASS") else "FAIL"
        result = text_of(run, "result.txt").splitlines()
        up = "yes" if result and result[0].startswith("up: yes") else "no"
        entry = renderer(run)
        drawn = (f"{entry['backend']} {entry['adapter']}{' (software)' if entry['software'] else ''}"
                 if entry else "none")
        wm_class = text_of(run, "wm_class.txt").split("=")[-1].strip() or "-"
        status = result[0].split("status ")[-1].split()[0] if result and "status" in result[0] else "-"
        fonts = next((a for a in actions(run) if a.get("action") == "fonts"), None)
        families = f"{fonts['ui']} / {fonts['data']}" if fonts else "-"
        print(f"{system:<14} {state:<9} {up:<4} {drawn[:52]:<52} {families[:44]:<44} {wm_class:<32} {passed:<5} {status}")
        return
    run, state = sys.argv[1:3]
    reasons = verdict(run, state)
    print("PASS" if not reasons else "FAIL: " + "; ".join(reasons))


main()
