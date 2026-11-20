#!/bin/bash
# One host probe record, $OUT/hostprobe/<label>.json: displays, every window
# (owner, pid, layer, bounds, on screen), the frontmost app, the menu bar
# owner, the pointer, and the activation policy of every running tart process.
# Reads only; no window, no event, no permission.
set -euo pipefail
. "$(dirname "$0")/env.sh"
args=()
for p in $(pgrep -x tart || true); do args+=(--pid "$p"); done
f="$OUT/hostprobe/$1.json"
"$BIN/hoststate" ${args[@]+"${args[@]}"} > "$f"
python3 -I "$(dirname "$0")/hostcompare.py" "$f"
