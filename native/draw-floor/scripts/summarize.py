"""summarize.py <native-see-build-times.json> <label> <load before> <load after>: one JSON line
for runs.jsonl from the spike's `shot` report: phases of the cold process, the first shot's draw,
the warm shots' median/min/max (draw + readback + JPEG at quality 90), peak resident memory."""
import json
import statistics
import sys

t = json.load(open(sys.argv[1]))
warm = [w["total_ms"] for w in t["warm"]]
draw = [w["draw_ms"] for w in t["warm"]]
print(json.dumps({
    "adapter": sys.argv[2], "label_adapter": t["adapter"], "build": t["build"], "blocks": t["blocks"], "quads": t["quads"],
    "load_before": sys.argv[3], "load_after": sys.argv[4],
    "device_ms": t["device_ms"], "pipelines_ms": t["pipelines_ms"], "upload_ms": t["upload_ms"],
    "first_shot_draw_ms": t["shots"][0]["draw_ms"], "first_shot_total_ms": t["shots"][0]["total_ms"],
    "cold_ms": t["cold_ms"],
    "warm_shot_ms_median": statistics.median(warm) if warm else None, "warm_shot_ms_min": min(warm) if warm else None, "warm_shot_ms_max": max(warm) if warm else None,
    "warm_draw_ms_median": statistics.median(draw) if draw else None,
    "peak_rss_mib": round(t["peak_rss_mib"]),
}))
