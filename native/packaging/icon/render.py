#!/usr/bin/env python3
"""Render the mcbench-lite placeholder icon in every format the installers use.

The icon is the app's own mark, read from web/icons/block.svg (the file the
page's tab draws, and the one the native window's mark-wire.svg and
mark-block.svg are cut from), placed on a rounded dark square. This script
reads that file's elements, writes the composed SVG from them, draws every
PNG from them, and packs the PNGs into the Windows ICO and the macOS ICNS.
A change to block.svg changes the icon on the next run. It uses only the
Python standard library, and two runs give identical bytes.

Run: python3 native/packaging/icon/render.py
"""

import copy
import math
import re
import struct
import xml.etree.ElementTree as ET
import zlib
from pathlib import Path

OUT = Path(__file__).resolve().parent
MARK = OUT.parents[2] / "web" / "icons" / "block.svg"
SVG_NS = "http://www.w3.org/2000/svg"

# The icon is drawn on a 1024 by 1024 canvas: a rounded dark square, and the
# mark's viewBox scaled into the square MARK_BOX (x, y, side) on top of it.
CANVAS = 1024
BACKGROUND = (32, 32, 960, 960, 208, "#1e232c")
MARK_BOX = (64, 64, 896)

# A path drawn only as a line (no fill), such as the wire, is never narrower
# than MIN_LINE_PX output pixels, so the wire stays legible at 16 and 24
# pixels. Outlines of filled faces keep their own width.
MIN_LINE_PX = 1.0

PNG_SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
ICO_SIZES = [16, 24, 32, 48, 64, 256]
ICNS_ENTRIES = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"ic11", 32),
    (b"ic12", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic13", 256),
    (b"ic09", 512),
    (b"ic14", 512),
    (b"ic10", 1024),
]

# Pixels on a shape's edge are sampled on a SUPERSAMPLE by SUPERSAMPLE grid.
SUPERSAMPLE = 8
HALF_DIAGONAL = math.sqrt(0.5)

INHERITED = ("fill", "stroke", "stroke-width", "stroke-linecap", "stroke-linejoin")


def fail(message):
    raise SystemExit(f"render.py: {message}")


def tag(element):
    return element.tag.removeprefix("{" + SVG_NS + "}")


def rgb(colour):
    if not re.fullmatch(r"#[0-9a-fA-F]{6}", colour):
        fail(f"{MARK.name} uses the colour {colour!r}; render.py reads only #rrggbb colours.")
    return tuple(int(colour[i:i + 2], 16) for i in (1, 3, 5))


# Reading the mark.

def class_strokes(root):
    """The stroke of each class in the file's dark (default) scheme: the
    style rules outside any @media block."""
    text = "".join(style.text for style in root.iter("{" + SVG_NS + "}style"))
    default = re.sub(r"@media[^{]*\{(?:[^{}]*\{[^{}]*\})*[^{}]*\}", "", text)
    return dict(re.findall(r"\.([\w-]+)\{stroke:(#[0-9a-fA-F]{6})\}", default))


def parse_path(d):
    """Subpaths of a path's data as (points, closed), from absolute M, L, H, V and Z."""
    tokens = re.findall(r"[A-Za-z]|-?(?:\d+\.?\d*|\.\d+)", d)
    subpaths = []
    points = []
    command = None
    i = 0
    while i < len(tokens):
        token = tokens[i]
        if token.isalpha():
            if token not in "MLHVZ":
                fail(f"{MARK.name} uses the path command {token}; render.py reads only absolute M, L, H, V and Z.")
            command = token
            i += 1
            if command == "Z":
                subpaths.append((points, True))
                points = [points[0]]
                continue
        if command == "M":
            if len(points) > 1:
                subpaths.append((points, False))
            points = [(float(tokens[i]), float(tokens[i + 1]))]
            i += 2
            command = "L"
        elif command == "L":
            points.append((float(tokens[i]), float(tokens[i + 1])))
            i += 2
        elif command == "H":
            points.append((float(tokens[i]), points[-1][1]))
            i += 1
        elif command == "V":
            points.append((points[-1][0], float(tokens[i])))
            i += 1
    if len(points) > 1:
        subpaths.append((points, False))
    return subpaths


def read_mark():
    """The mark as a list of layers. A layer is (shapes, opacity, blur) where
    shapes is a list of (kind, subpaths, colour, width) painted in order and
    blur is (stdDeviation, filter region as fractions of the bounding box) or
    None."""
    root = ET.parse(MARK).getroot()
    strokes = class_strokes(root)
    filters = {f.get("id"): f for f in root.iter("{" + SVG_NS + "}filter")}
    base = {name: root.get(name) for name in INHERITED if root.get(name)}

    def shapes_of(path, inherited):
        style = dict(inherited)
        style.update({name: path.get(name) for name in INHERITED if path.get(name)})
        if path.get("class"):
            style["stroke"] = strokes[path.get("class")]
        subpaths = parse_path(path.get("d"))
        shapes = []
        if style.get("fill", "black") != "none":
            shapes.append(("fill", subpaths, style["fill"], 0.0))
        if style.get("stroke", "none") != "none":
            for points, closed in subpaths:
                if not closed and style.get("stroke-linecap") != "round":
                    fail(f"{MARK.name} has an open line without round caps; render.py draws round caps and joins only.")
                if len(points) > 2 and style.get("stroke-linejoin") != "round":
                    fail(f"{MARK.name} has a joined line without round joins; render.py draws round caps and joins only.")
            line_only = style.get("fill", "black") == "none"
            shapes.append(("line" if line_only else "stroke", subpaths, style["stroke"], float(style["stroke-width"])))
        return shapes

    layers = []
    for element in root:
        name = tag(element)
        if name == "path":
            layers.append((shapes_of(element, base), float(element.get("opacity", "1")), None))
        elif name == "g":
            inherited = dict(base)
            inherited.update({n: element.get(n) for n in INHERITED if element.get(n)})
            shapes = [s for path in element for s in shapes_of(path, inherited)]
            blur = None
            if element.get("filter"):
                ident = re.fullmatch(r"url\(#(.+)\)", element.get("filter")).group(1)
                f = filters[ident]
                region = tuple(float(f.get(k).rstrip("%")) / 100 for k in ("x", "y", "width", "height"))
                deviation = float(f.find("{" + SVG_NS + "}feGaussianBlur").get("stdDeviation"))
                blur = (deviation, region)
            layers.append((shapes, float(element.get("opacity", "1")), blur))
        elif name != "defs":
            fail(f"{MARK.name} has a <{name}> element at the top level; render.py reads path, g and defs.")
    return root, layers


# Drawing.

def segment_distance(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)
    t = min(max(t, 0.0), 1.0)
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def stroke_sdf(segments, half_width):
    def sdf(x, y):
        return min(segment_distance(x, y, *s) for s in segments) - half_width
    return sdf


def fill_sdf(polygons):
    edges = [(p[i], p[(i + 1) % len(p)]) for p in polygons for i in range(len(p))]

    def sdf(x, y):
        winding = 0
        for (ax, ay), (bx, by) in edges:
            if ay <= y < by and (bx - ax) * (y - ay) - (x - ax) * (by - ay) > 0:
                winding += 1
            elif by <= y < ay and (bx - ax) * (y - ay) - (x - ax) * (by - ay) < 0:
                winding -= 1
        distance = min(segment_distance(x, y, ax, ay, bx, by) for (ax, ay), (bx, by) in edges)
        return -distance if winding else distance
    return sdf


def rounded_rect_sdf(x0, y0, w, h, r):
    cx, cy = x0 + w / 2, y0 + h / 2

    def sdf(x, y):
        qx = abs(x - cx) - (w / 2 - r)
        qy = abs(y - cy) - (h / 2 - r)
        return math.hypot(max(qx, 0.0), max(qy, 0.0)) + min(max(qx, qy), 0.0) - r
    return sdf


def coverage(sdf, px, py):
    """Fraction of output pixel (px, py) inside the shape whose signed distance is sdf."""
    centre = sdf(px + 0.5, py + 0.5)
    if centre <= -HALF_DIAGONAL:
        return 1.0
    if centre >= HALF_DIAGONAL:
        return 0.0
    hits = 0
    for j in range(SUPERSAMPLE):
        y = py + (j + 0.5) / SUPERSAMPLE
        for i in range(SUPERSAMPLE):
            if sdf(px + (i + 0.5) / SUPERSAMPLE, y) <= 0:
                hits += 1
    return hits / (SUPERSAMPLE * SUPERSAMPLE)


class Buffer:
    """Premultiplied RGBA floats over the pixel rectangle [x0, x0+w) x [y0, y0+h)."""

    def __init__(self, x0, y0, w, h):
        self.x0, self.y0, self.w, self.h = x0, y0, w, h
        self.px = [[0.0, 0.0, 0.0, 0.0] for _ in range(w * h)]

    def paint(self, sdf, bounds, colour, opacity):
        r, g, b = rgb(colour)
        left, top, right, bottom = bounds
        for py in range(max(self.y0, math.floor(top)), min(self.y0 + self.h, math.ceil(bottom))):
            for px in range(max(self.x0, math.floor(left)), min(self.x0 + self.w, math.ceil(right))):
                c = coverage(sdf, px, py) * opacity
                if c:
                    p = self.px[(py - self.y0) * self.w + px - self.x0]
                    k = 1 - c
                    p[0] = r * c + p[0] * k
                    p[1] = g * c + p[1] * k
                    p[2] = b * c + p[2] * k
                    p[3] = 255 * c + p[3] * k

    def over(self, layer):
        """Composite layer (a Buffer inside this one) over this buffer."""
        for ly in range(layer.h):
            for lx in range(layer.w):
                s = layer.px[ly * layer.w + lx]
                if s[3]:
                    d = self.px[(layer.y0 + ly - self.y0) * self.w + layer.x0 + lx - self.x0]
                    k = 1 - s[3] / 255
                    for ch in range(4):
                        d[ch] = s[ch] + d[ch] * k


def blur_line(values, deviation):
    """feGaussianBlur along one line, as the Filter Effects specification
    describes it: three box blurs for a deviation of 2 or more, the Gaussian
    kernel itself below that."""
    n = len(values)
    if deviation >= 2.0:
        d = math.floor(deviation * 3 * math.sqrt(2 * math.pi) / 4 + 0.5)
        passes = [(d, (d - 1) // 2)] * 3 if d % 2 else [(d, d // 2), (d, d // 2 - 1), (d + 1, d // 2)]
        for size, lead in passes:
            prefix = [0.0]
            for v in values:
                prefix.append(prefix[-1] + v)
            values = [(prefix[min(n, max(0, i - lead + size))] - prefix[min(n, max(0, i - lead))]) / size for i in range(n)]
        return values
    radius = math.ceil(3 * deviation)
    kernel = [math.exp(-k * k / (2 * deviation * deviation)) for k in range(-radius, radius + 1)]
    total = sum(kernel)
    kernel = [k / total for k in kernel]
    return [sum(kernel[k + radius] * values[i + k] for k in range(-radius, radius + 1) if 0 <= i + k < n) for i in range(n)]


def blurred(layer, deviation):
    out = Buffer(layer.x0, layer.y0, layer.w, layer.h)
    for ch in range(4):
        plane = [p[ch] for p in layer.px]
        rows = [blur_line(plane[y * layer.w:(y + 1) * layer.w], deviation) for y in range(layer.h)]
        columns = [blur_line([rows[y][x] for y in range(layer.h)], deviation) for x in range(layer.w)]
        for y in range(layer.h):
            for x in range(layer.w):
                out.px[y * layer.w + x][ch] = columns[x][y]
    return out


def draw(size, layers, view):
    """RGBA rows of the icon at size by size pixels."""
    k = size / CANVAS
    vx, vy, vside = view
    mx, my, mside = MARK_BOX
    unit = mside / vside * k

    def to_px(point):
        return ((mx + (point[0] - vx) * mside / vside) * k, (my + (point[1] - vy) * mside / vside) * k)

    canvas = Buffer(0, 0, size, size)
    bx, by, bw, bh, br, bcolour = BACKGROUND
    canvas.paint(rounded_rect_sdf(bx * k, by * k, bw * k, bh * k, br * k), (bx * k, by * k, (bx + bw) * k, (by + bh) * k), bcolour, 1.0)

    for shapes, opacity, blur in layers:
        drawn = []
        for kind, subpaths, colour, width in shapes:
            pts = [([to_px(p) for p in points], closed) for points, closed in subpaths]
            if kind == "fill":
                sdf = fill_sdf([p for p, _ in pts])
                pad = 0.0
            else:
                half = width * unit / 2
                if kind == "line":
                    half = max(half, MIN_LINE_PX / 2)
                segments = [(*a, *b) for p, closed in pts for a, b in zip(p, p[1:] + p[:1] if closed else p[1:])]
                sdf = stroke_sdf(segments, half)
                pad = half
            xs = [x for p, _ in pts for x, _ in p]
            ys = [y for p, _ in pts for _, y in p]
            drawn.append((sdf, (min(xs) - pad - 1, min(ys) - pad - 1, max(xs) + pad + 1, max(ys) + pad + 1), colour, xs, ys))
        if blur is None:
            for sdf, bounds, colour, _, _ in drawn:
                canvas.paint(sdf, bounds, colour, opacity)
            continue
        deviation, (rx, ry, rw, rh) = blur
        xs = [x for d in drawn for x in d[3]]
        ys = [y for d in drawn for y in d[4]]
        left, top, w, h = min(xs), min(ys), max(xs) - min(xs), max(ys) - min(ys)
        x0, y0 = max(0, math.floor(left + rx * w)), max(0, math.floor(top + ry * h))
        x1, y1 = min(size, math.ceil(left + (rx + rw) * w)), min(size, math.ceil(top + (ry + rh) * h))
        source = Buffer(x0, y0, x1 - x0, y1 - y0)
        for sdf, bounds, colour, _, _ in drawn:
            source.paint(sdf, bounds, colour, 1.0)
        merged = blurred(source, deviation * unit)
        merged.over(source)
        if opacity != 1.0:
            merged.px = [[v * opacity for v in p] for p in merged.px]
        canvas.over(merged)

    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            r, g, b, a = canvas.px[py * size + px]
            if a:
                row += bytes((round(r * 255 / a), round(g * 255 / a), round(b * 255 / a), round(a)))
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    return rows


# Writing.

def png(size, rows):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))

    raw = b"".join(b"\0" + row for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def ico(pngs):
    header = struct.pack("<HHH", 0, 1, len(ICO_SIZES))
    offset = len(header) + 16 * len(ICO_SIZES)
    entries = b""
    body = b""
    for size in ICO_SIZES:
        data = pngs[size]
        dim = 0 if size == 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset + len(body))
        body += data
    return header + entries + body


def icns(pngs):
    body = b"".join(kind + struct.pack(">I", 8 + len(pngs[size])) + pngs[size] for kind, size in ICNS_ENTRIES)
    return b"icns" + struct.pack(">I", 8 + len(body)) + body


def svg(root, view):
    """The composed icon: the background, then the mark's own elements with
    each class replaced by its dark-scheme stroke and the style sheet left out."""
    ET.register_namespace("", SVG_NS)
    strokes = class_strokes(root)
    out = ET.Element("{" + SVG_NS + "}svg", {"width": str(CANVAS), "height": str(CANVAS), "viewBox": f"0 0 {CANVAS} {CANVAS}"})
    x, y, w, h, r, colour = BACKGROUND
    ET.SubElement(out, "{" + SVG_NS + "}rect", {"x": str(x), "y": str(y), "width": str(w), "height": str(h), "rx": str(r), "ry": str(r), "fill": colour})
    vx, vy, vside = view
    mx, my, mside = MARK_BOX
    scale = mside / vside
    group = ET.SubElement(out, "{" + SVG_NS + "}g", {"transform": f"translate({mx - vx * scale:g} {my - vy * scale:g}) scale({scale:g})"})
    group.attrib.update({name: root.get(name) for name in INHERITED if root.get(name)})
    for element in root:
        element = copy.deepcopy(element)
        for node in element.iter():
            for style in [c for c in node if tag(c) == "style"]:
                node.remove(style)
            if node.get("class"):
                node.set("stroke", strokes[node.attrib.pop("class")])
        group.append(element)
    ET.indent(out)
    return ET.tostring(out, encoding="unicode") + "\n"


def main():
    root, layers = read_mark()
    vx, vy, vw, vh = (float(v) for v in root.get("viewBox").split())
    if vw != vh:
        fail(f"{MARK.name} has a viewBox of {vw} by {vh}; render.py places a square viewBox.")
    view = (vx, vy, vw)
    (OUT / "mcbench-lite.svg").write_text(svg(root, view))
    pngs = {}
    for size in PNG_SIZES:
        pngs[size] = png(size, draw(size, layers, view))
        (OUT / f"mcbench-lite-{size}.png").write_bytes(pngs[size])
    (OUT / "mcbench-lite.ico").write_bytes(ico(pngs))
    (OUT / "mcbench-lite.icns").write_bytes(icns(pngs))


main()
