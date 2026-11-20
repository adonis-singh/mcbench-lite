#!/bin/bash
# shot.sh <name> [x y w h]: screencapture inside the guest (the guest's own
# screen, never the host's), copied back over stdout to $OUT/evidence/<name>.png;
# with a rectangle (points) only that region.
set -euo pipefail
. "$(dirname "$0")/env.sh"
mkdir -p "$(dirname "$OUT/evidence/$1.png")"
if [ $# -ge 5 ]; then r="-R$2,$3,$4,$5"; else r=""; fi
gx sh -c "screencapture -x $r /tmp/shot.png && cat /tmp/shot.png && rm -f /tmp/shot.png" > "$OUT/evidence/$1.png"
echo "$OUT/evidence/$1.png"
