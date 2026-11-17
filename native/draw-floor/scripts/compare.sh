#!/bin/sh
# On adipc: the renderer report's cross-backend colour check (pngdiff.py:
# mean abs <= 0.5, p99 of the max-channel difference <= 4) between two runs
# of measure.sh (or contain.sh) for one build, every shot.
#   scripts/compare.sh <label a> <label b> <build>
set -eu
. "$(dirname "$0")/env.sh"
a=$1 b=$2 build=$3
for png in "$R/results/$a/$build"/native-see-build-*.png; do
  i=$(basename "$png" .png | sed 's/.*-//')
  line=$(python3 -I "$(dirname "$0")/pngdiff.py" "$png" "$R/results/$b/$build/native-see-build-$i.png")
  echo "{\"a\":\"$a\",\"b\":\"$b\",\"build\":\"$build\",\"shot\":$i,${line#\{}" | tee -a "$R/results/compare.jsonl"
done
