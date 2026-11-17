"""pngdiff.py A.png B.png: the renderer spike's colour statistics between two native shot PNGs
(same definitions as native/spikes/renderer/src/bin/compare.rs colour_stats, overall pixels):
both are the drawing buffer's raw RGBA, read as min(colour, alpha); per pixel the mean of the three
channel differences and the max-channel difference; p99 index as in compare.rs. SSIM is left out.
Cross-backend thresholds from the renderer report: mean <= 0.5, p99 <= 4."""
import json
import sys

import numpy as np
from PIL import Image


def load(p):
    a = np.asarray(Image.open(p).convert("RGBA"), dtype=np.float32)
    return np.minimum(a[..., :3], a[..., 3:4])


a, b = load(sys.argv[1]), load(sys.argv[2])
if a.shape != b.shape:
    raise SystemExit(f"different sizes {a.shape} {b.shape}")
d = np.abs(a - b)
mx = np.sort(d.max(axis=2).ravel())
n = mx.size
out = {
    "pixels": n,
    "mean_abs": float(d.mean(axis=2).mean()),
    "p50": float(mx[int((n - 1) * 0.5)]),
    "p99": float(mx[int((n - 1) * 0.99)]),
    "max": float(mx[-1]),
    "over_8": float((mx > 8).mean()),
    "over_16": float((mx > 16).mean()),
}
out["pass"] = out["mean_abs"] <= 0.5 and out["p99"] <= 4
print(json.dumps(out))
