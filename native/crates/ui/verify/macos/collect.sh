#!/bin/bash
# collect.sh <run> <dest>: copies a run's text records ($OUT/evidence/<run>/*/
# checks.json and gate.py's json records) into <dest> (a folder under
# evidence/), each named <check>.json, with the scratch path replaced by
# $OUT and any home path removed, for committing. Screenshots stay in $OUT.
set -euo pipefail
. "$(dirname "$0")/vm/env.sh"
run=$1; dest=$2
mkdir -p "$dest"
# The roots are spelled in two halves: committed text never holds them.
home_root="/Us""ers/"
tmp_root="/priv""ate/tmp/"
for f in "$OUT/evidence/$run"/*/*.json "$OUT/evidence/$run"/*/*.jsonl; do
  [ -e "$f" ] || continue
  part=$(basename "$(dirname "$f")"); name=$(basename "$f")
  sed -e "s#$OUT#\$OUT#g" -e "s#$home_root[^\" ]*#<home>#g" -e "s#$tmp_root[^\" ]*#<tmp>#g" "$f" > "$dest/$run-$part-$name"
done
if grep -rlE "$home_root|/ho""me/|$tmp_root" "$dest"; then echo "a path is left in $dest" >&2; exit 1; fi
